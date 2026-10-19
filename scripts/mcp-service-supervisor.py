#!/usr/bin/env python3
"""Keep Bellhound's local voice runtime and private MCP tunnel alive on macOS.

The supervisor reads both runtime credentials from macOS Keychain, starts the
existing Bellhound foreground owner, waits for its generated live configuration,
then starts tunnel-client against that configuration. Secrets are inherited only
by child processes and are never written to files, arguments, or logs.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import tomllib
import urllib.parse
import urllib.request

sys.dont_write_bytecode = True

SECRET_PREFIXES = (
    "CONTROL_PLANE_",
    "MCP_",
    "HARPOON_",
    "CLOUDFLARED_",
    "HEALTH_",
    "TUNNEL_CLIENT_",
    "ADMIN_UI_",
    "LOG_",
    "BELLHOUND_",
    "TELNYX_",
    "OPENAI_",
)


class ServiceError(RuntimeError):
    """A fail-closed configuration, startup, or lifecycle error."""


class TunnelRegistrationGate:
    def __init__(self) -> None:
        self.registered = threading.Event()
        self.failed = threading.Event()
        self.failure_code: str | None = None

    def feed(self, line: str) -> None:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            return
        if record.get("status_code") == 401 or record.get("error_code") == "invalid_api_key":
            self.failure_code = "control_plane_authentication_failed"
            self.failed.set()
        if record.get("component") == "controlplane" and record.get("msg") == "tunnel metadata fetched":
            self.registered.set()


class OwnerReadyGate:
    def __init__(self) -> None:
        self.ready = threading.Event()
        self.payload: dict[str, object] | None = None

    def feed(self, line: str) -> None:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            return
        if record.get("state") == "ready":
            self.payload = record
            self.ready.set()


class CallWatchdog:
    def __init__(self, limit_seconds: int, grace_seconds: int = 5) -> None:
        self.limit_seconds = limit_seconds
        self.grace_seconds = grace_seconds
        self.started_at: float | None = None
        self.hangups = 0

    def observe(self, active: bool | None, now: float) -> bool:
        if active is True:
            if self.started_at is None:
                self.started_at = now
            return now - self.started_at >= self.limit_seconds + self.grace_seconds
        if active is False:
            self.started_at = None
        return False

    def overdue_without_control(self, now: float) -> bool:
        return self.started_at is not None and now - self.started_at >= self.limit_seconds + self.grace_seconds

    def completed_hangup(self) -> None:
        self.hangups += 1
        self.started_at = None


class LinePump:
    def __init__(self, stream, destination: Path, secrets: tuple[str, ...], callback=None) -> None:
        self.stream = stream
        self.destination = destination
        self.secrets = tuple(value for value in secrets if value)
        self.callback = callback
        self.thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def join(self, timeout: float = 2) -> None:
        self.thread.join(timeout=timeout)

    def _run(self) -> None:
        try:
            with self.destination.open("w") as output:
                os.chmod(self.destination, 0o600)
                for line in self.stream:
                    if self.callback is not None:
                        self.callback(line)
                    for secret in self.secrets:
                        line = line.replace(secret, "[REDACTED]")
                    output.write(line)
                    output.flush()
        finally:
            self.stream.close()


def clean_environment(source: dict[str, str]) -> dict[str, str]:
    env = dict(source)
    for name in list(env):
        if name.startswith(SECRET_PREFIXES) or name in ("OPEN_WEB_UI", "ALLOW_REMOTE_UI", "PID_FILE"):
            env.pop(name)
    return env


def fetch_keychain_secret(service: str, account: str, minimum: int) -> str:
    result = subprocess.run(
        [
            "/usr/bin/security",
            "find-generic-password",
            "-s",
            service,
            "-a",
            account,
            "-w",
        ],
        capture_output=True,
        timeout=30,
    )
    value = bytearray(result.stdout.rstrip(b"\r\n"))
    try:
        if result.returncode or len(value) < minimum or any(chr(byte).isspace() for byte in value):
            raise ServiceError(f"keychain_secret_unavailable:{service}")
        return bytes(value).decode("utf-8")
    finally:
        for index in range(len(value)):
            value[index] = 0


def validate_paths(args: argparse.Namespace) -> None:
    required_files = (args.binary, args.base_config, args.profile_dir / f"{args.profile}.yaml", args.session_script)
    for path in required_files:
        if not path.is_file():
            raise ServiceError(f"required_file_missing:{path}")
    for executable in (args.binary, args.session_script, args.tunnel_client):
        if not os.access(executable, os.X_OK):
            raise ServiceError(f"required_executable_unavailable:{executable}")
    config = tomllib.loads(args.base_config.read_text())
    relay = config.get("relay", {})
    if relay.get("bind", "127.0.0.1:8787") != "127.0.0.1:8787" or relay.get(
        "control_bind", "127.0.0.1:8788"
    ) != "127.0.0.1:8788":
        raise ServiceError("unexpected_listener_configuration")
    call_limit = relay.get("max_call_seconds", 300)
    if not isinstance(call_limit, int) or not 30 <= call_limit <= 3600:
        raise ServiceError("max_call_seconds_must_be_between_30_and_3600")
    args.max_call_seconds = call_limit
    profile = (args.profile_dir / f"{args.profile}.yaml").read_text()
    if str(args.binary) not in profile or str(args.runtime_config) not in profile:
        raise ServiceError("tunnel_profile_target_mismatch")
    if 'api_key: "env:CONTROL_PLANE_API_KEY"' not in profile:
        raise ServiceError("tunnel_profile_does_not_use_runtime_key_reference")


def wait_for_owner(process: subprocess.Popen, gate: OwnerReadyGate, timeout: float) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if gate.ready.is_set() and gate.payload is not None:
            return gate.payload
        if process.poll() is not None:
            raise ServiceError(f"bellhound_owner_exited_before_ready:{process.returncode}")
        time.sleep(0.05)
    raise ServiceError("bellhound_owner_ready_timeout")


def wait_for_registration(process: subprocess.Popen, gate: TunnelRegistrationGate, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    registered_at = None
    while time.monotonic() < deadline:
        if gate.failed.is_set():
            raise ServiceError(gate.failure_code or "control_plane_registration_failed")
        if gate.registered.is_set() and registered_at is None:
            registered_at = time.monotonic()
        if registered_at is not None and time.monotonic() - registered_at >= 1:
            return
        if process.poll() is not None:
            raise ServiceError(f"tunnel_client_exited_before_registration:{process.returncode}")
        time.sleep(0.05)
    raise ServiceError("tunnel_client_registration_timeout")


def wait_for_health(path: Path, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.is_file():
            base = path.read_text().strip().rstrip("/")
            parsed = urllib.parse.urlparse(base)
            if parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost"):
                raise ServiceError("health_listener_is_not_loopback")
            try:
                statuses = []
                for endpoint in ("healthz", "readyz"):
                    with urllib.request.urlopen(f"{base}/{endpoint}", timeout=1) as response:
                        statuses.append(response.status)
                if statuses == [200, 200]:
                    return
            except OSError:
                pass
        time.sleep(0.1)
    raise ServiceError("local_health_readiness_timeout")


def control_status(config: dict[str, object], relay_token: str) -> dict[str, object]:
    relay = config["relay"]
    control_bind = str(relay.get("control_bind", "127.0.0.1:8788"))
    request = urllib.request.Request(
        f"http://{control_bind}/v1/status",
        headers={"x-bellhound-token": relay_token},
    )
    with urllib.request.urlopen(request, timeout=2) as response:
        return json.load(response)


def force_watchdog_hangup(args: argparse.Namespace, env: dict[str, str]) -> None:
    result = subprocess.run(
        [
            str(args.binary),
            "--config",
            str(args.runtime_config),
            "hangup",
            "--reason",
            "service_watchdog",
        ],
        env=env,
        capture_output=True,
        timeout=10,
    )
    if result.returncode:
        raise ServiceError(f"service_watchdog_hangup_failed:{result.returncode}")


def stop_group(process: subprocess.Popen | None, grace_seconds: float = 10) -> None:
    if process is None:
        return
    group = process.pid

    def group_exists() -> bool:
        leader_exited = process.poll() is not None
        try:
            os.killpg(group, 0)
            return True
        except ProcessLookupError:
            return False
        except PermissionError:
            if leader_exited:
                return False
            raise

    for selected_signal in (signal.SIGINT, signal.SIGTERM):
        if not group_exists():
            if process.poll() is None:
                process.wait()
            return
        try:
            os.killpg(group, selected_signal)
        except ProcessLookupError:
            pass
        deadline = time.monotonic() + grace_seconds
        while group_exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        if not group_exists():
            if process.poll() is None:
                process.wait()
            return
    if group_exists():
        try:
            os.killpg(group, signal.SIGKILL)
        except ProcessLookupError:
            pass
        if process.poll() is None:
            try:
                process.wait(timeout=grace_seconds)
            except subprocess.TimeoutExpired:
                pass
        deadline = time.monotonic() + grace_seconds
        while group_exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        if group_exists():
            raise ServiceError("process_group_survived_sigkill")
    if process.poll() is None:
        process.wait()


def write_json(path: Path, value: dict[str, object]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    os.chmod(path, 0o600)


def select_binary(root: Path, explicit: Path | None = None) -> Path:
    if explicit is not None:
        return explicit.resolve()
    source_binary = root / "target/release/bellhound"
    archive_binary = root / "bellhound"
    return (source_binary if source_binary.is_file() else archive_binary).resolve()


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--base-config", type=Path)
    parser.add_argument("--profile-dir", type=Path, default=Path.home() / ".config/tunnel-client")
    parser.add_argument("--profile", default="bellhound")
    parser.add_argument("--tunnel-client", type=Path, default=Path("/opt/homebrew/bin/tunnel-client"))
    parser.add_argument("--state-dir", type=Path)
    parser.add_argument("--registration-timeout", type=int, default=30)
    parser.add_argument("--openai-key-service", default="Bellhound OpenAI tunnel runtime key")
    parser.add_argument("--openai-key-account", default="tunnel-client")
    parser.add_argument("--relay-token-service", default="Bellhound relay token")
    parser.add_argument("--relay-token-account", default="default")
    args = parser.parse_args()
    args.root = args.root.resolve()
    args.base_config = (args.base_config or args.root / "work/bellhound-keychain.toml").resolve()
    args.state_dir = (args.state_dir or args.root / "work/mcp-service").resolve()
    args.binary = select_binary(args.root, args.binary)
    args.runtime_config = args.root / "work/session/bellhound.toml"
    args.session_script = args.root / "skills/bellhound/scripts/session.sh"
    return args


def main() -> int:
    if sys.platform != "darwin":
        raise SystemExit("The current service supervisor uses macOS Keychain and requires macOS.")
    args = parse_args()
    os.umask(0o077)
    args.state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(args.state_dir, 0o700)
    validate_paths(args)
    private_config = tomllib.loads(args.base_config.read_text())
    runtime_key = fetch_keychain_secret(args.openai_key_service, args.openai_key_account, 20)
    relay_token = fetch_keychain_secret(args.relay_token_service, args.relay_token_account, 24)
    if not runtime_key.startswith("sk-"):
        raise ServiceError("openai_runtime_key_has_unexpected_shape")
    if args.check:
        runtime_key = relay_token = ""
        print(json.dumps({"state": "ready_to_install", "profile": args.profile}, separators=(",", ":")))
        return 0

    lock_file = (args.state_dir / "service.lock").open("w")
    try:
        fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as error:
        raise ServiceError("service_already_running") from error

    stop_requested = threading.Event()
    signal.signal(signal.SIGINT, lambda *_: stop_requested.set())
    signal.signal(signal.SIGTERM, lambda *_: stop_requested.set())
    env = clean_environment(os.environ)
    env.update(
        {
            "CONTROL_PLANE_API_KEY": runtime_key,
            "BELLHOUND_RELAY_TOKEN": relay_token,
            "BELLHOUND_ROOT": str(args.root),
            "BELLHOUND_CONFIG": str(args.base_config),
            "BELLHOUND_OWNER_MAX_SECONDS": "0",
        }
    )
    owner = client = None
    owner_pump = client_pump = None
    owner_gate = OwnerReadyGate()
    tunnel_gate = TunnelRegistrationGate()
    reason = "startup_failed"
    ready = False
    watchdog = CallWatchdog(args.max_call_seconds)
    started = time.monotonic()
    try:
        owner = subprocess.Popen(
            [str(args.session_script), "run"],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        owner_pump = LinePump(owner.stdout, args.state_dir / "owner.log", (runtime_key, relay_token), owner_gate.feed)
        owner_pump.start()
        owner_ready = wait_for_owner(owner, owner_gate, 60)

        health_file = args.state_dir / "health.url"
        client = subprocess.Popen(
            [
                str(args.tunnel_client),
                "run",
                "--profile",
                args.profile,
                "--profile-dir",
                str(args.profile_dir),
                "--open-web-ui=false",
                "--health.url-file",
                str(health_file),
                "--pid.file",
                str(args.state_dir / "tunnel-client.pid"),
            ],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        client_pump = LinePump(client.stdout, args.state_dir / "tunnel-client.log", (runtime_key, relay_token), tunnel_gate.feed)
        client_pump.start()
        wait_for_registration(client, tunnel_gate, args.registration_timeout)
        wait_for_health(health_file, 15)
        if owner.poll() is not None:
            raise ServiceError("bellhound_owner_ended_before_remote_ready")
        ready = True
        reason = "stop_requested"
        write_json(
            args.state_dir / "ready.json",
            {
                "state": "ready",
                "profile": args.profile,
                "event_cursor": owner_ready.get("event_cursor"),
                "max_call_seconds": args.max_call_seconds,
                "watchdog_grace_seconds": 5,
                "control_plane_registered": True,
            },
        )
        print(json.dumps({"state": "ready", "profile": args.profile}, separators=(",", ":")), flush=True)
        while not stop_requested.wait(0.25):
            if tunnel_gate.failed.is_set():
                raise ServiceError(tunnel_gate.failure_code or "control_plane_registration_failed")
            if client.poll() is not None:
                raise ServiceError(f"tunnel_client_exited:{client.returncode}")
            if owner.poll() is not None:
                raise ServiceError(f"bellhound_owner_exited:{owner.returncode}")
            try:
                status = control_status(private_config, relay_token)
            except (OSError, ValueError, KeyError):
                status = None
            now = time.monotonic()
            active = None if status is None else status.get("active_call")
            if watchdog.observe(active if isinstance(active, bool) else None, now):
                force_watchdog_hangup(args, env)
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    try:
                        if control_status(private_config, relay_token).get("active_call") is False:
                            watchdog.completed_hangup()
                            break
                    except (OSError, ValueError, KeyError):
                        pass
                    time.sleep(0.25)
                else:
                    raise ServiceError("service_watchdog_call_remained_active")
            elif status is None and watchdog.overdue_without_control(now):
                raise ServiceError("service_watchdog_lost_control_path_during_overdue_call")
    except Exception as error:
        reason = str(error) if isinstance(error, ServiceError) else f"unexpected_error:{type(error).__name__}"
        print(json.dumps({"state": "stopping", "reason": reason}, separators=(",", ":")), flush=True)
    finally:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        cleanup_errors = []
        try:
            stop_group(client)
        except Exception as error:
            cleanup_errors.append(f"tunnel_client:{type(error).__name__}")
        if owner is not None:
            try:
                subprocess.run(
                    [str(args.session_script), "stop"],
                    env=env,
                    capture_output=True,
                    timeout=30,
                    check=True,
                )
            except Exception as error:
                cleanup_errors.append(f"bellhound_session:{type(error).__name__}")
        try:
            stop_group(owner)
        except Exception as error:
            cleanup_errors.append(f"owner_group:{type(error).__name__}")
        for pump in (client_pump, owner_pump):
            if pump is not None:
                pump.join()
        for name in ("ready.json", "health.url", "tunnel-client.pid"):
            (args.state_dir / name).unlink(missing_ok=True)
        write_json(
            args.state_dir / "receipt.json",
            {
                "reason": reason,
                "ready_was_published": ready,
                "control_plane_registered": tunnel_gate.registered.is_set(),
                "max_call_seconds": getattr(args, "max_call_seconds", None),
                "watchdog_hangups": watchdog.hangups,
                "cleanup_errors": cleanup_errors,
                "elapsed_seconds": round(time.monotonic() - started, 3),
            },
        )
        env.pop("CONTROL_PLANE_API_KEY", None)
        env.pop("BELLHOUND_RELAY_TOKEN", None)
        runtime_key = relay_token = ""
        fcntl.flock(lock_file, fcntl.LOCK_UN)
        lock_file.close()
    return 0 if ready and reason == "stop_requested" and not cleanup_errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
