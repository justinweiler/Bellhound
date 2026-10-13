#!/usr/bin/env python3
"""Own an experimental Bellhound relay and private MCP client in one user-held process.

This is an operator canary tool, not a background service. It never dials. The remote
agent owns every Bellhound MCP action after this supervisor reports authenticated ready.
"""

from __future__ import annotations

import argparse
import datetime
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
import uuid

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


class SupervisorError(RuntimeError):
    """A fail-closed preflight or lifecycle error."""


class TunnelRegistrationGate:
    """Classify redacted tunnel-client JSON logs without retaining credentials."""

    def __init__(self) -> None:
        self.registered = threading.Event()
        self.failed = threading.Event()
        self.failure_code: str | None = None

    def feed(self, line: str) -> None:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            return
        status_code = record.get("status_code")
        error_code = str(record.get("error_code", ""))
        if status_code == 401 or error_code == "invalid_api_key":
            self.failure_code = "control_plane_authentication_failed"
            self.failed.set()
            return
        if (
            record.get("component") == "controlplane"
            and record.get("msg") == "tunnel metadata fetched"
        ):
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


class LinePump:
    def __init__(
        self,
        stream,
        destination: Path,
        secrets: tuple[str, ...],
        callback=None,
    ) -> None:
        self.stream = stream
        self.destination = destination
        self.secrets = tuple(secret for secret in secrets if secret)
        self.callback = callback
        self.thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def join(self, timeout: float = 2) -> None:
        self.thread.join(timeout=timeout)

    def _redact(self, text: str) -> str:
        for secret in self.secrets:
            text = text.replace(secret, "[REDACTED]")
        return text

    def _run(self) -> None:
        try:
            with self.destination.open("w") as output:
                for line in self.stream:
                    if self.callback is not None:
                        self.callback(line)
                    output.write(self._redact(line))
                    output.flush()
        finally:
            self.stream.close()


def stop_group(process: subprocess.Popen | None, grace_seconds: float = 10) -> None:
    if process is None:
        return
    process_group = process.pid

    def group_exists() -> bool:
        leader_exited = process.poll() is not None
        try:
            os.killpg(process_group, 0)
            return True
        except ProcessLookupError:
            return False
        except PermissionError:
            if leader_exited:
                return False
            raise

    for sig in (signal.SIGINT, signal.SIGTERM):
        if not group_exists():
            if process.poll() is None:
                process.wait()
            return
        try:
            os.killpg(process_group, sig)
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
            os.killpg(process_group, signal.SIGKILL)
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
            raise SupervisorError("process_group_survived_sigkill")
    if process.poll() is None:
        process.wait()


def wait_for_registration(
    process: subprocess.Popen,
    gate: TunnelRegistrationGate,
    timeout: float,
) -> None:
    deadline = time.monotonic() + timeout
    registered_at = None
    while time.monotonic() < deadline:
        if gate.failed.is_set():
            raise SupervisorError(gate.failure_code or "control_plane_registration_failed")
        if gate.registered.is_set() and registered_at is None:
            registered_at = time.monotonic()
        if registered_at is not None and time.monotonic() - registered_at >= 1:
            return
        code = process.poll()
        if code is not None:
            raise SupervisorError(f"tunnel_client_exited_before_registration:{code}")
        time.sleep(0.05)
    raise SupervisorError("tunnel_client_registration_timeout")


def wait_for_owner(
    process: subprocess.Popen,
    gate: OwnerReadyGate,
    timeout: float,
) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if gate.ready.is_set() and gate.payload is not None:
            return gate.payload
        code = process.poll()
        if code is not None:
            raise SupervisorError(f"bellhound_owner_exited_before_ready:{code}")
        time.sleep(0.05)
    raise SupervisorError("bellhound_owner_ready_timeout")


def clean_environment(source: dict[str, str]) -> dict[str, str]:
    env = dict(source)
    for name in list(env):
        if name.startswith(SECRET_PREFIXES) or name in (
            "OPEN_WEB_UI",
            "ALLOW_REMOTE_UI",
            "PID_FILE",
        ):
            env.pop(name)
    return env


def read_runtime_key_via_dialog(helper: Path, timeout: float = 300) -> str:
    if not helper.is_file():
        raise SupervisorError("credential_dialog_unavailable")
    try:
        result = subprocess.run(
            ["xcrun", "swift", str(helper)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        raise SupervisorError("credential_dialog_timeout") from error
    if result.returncode == 2:
        raise SupervisorError("credential_entry_cancelled")
    if result.returncode:
        raise SupervisorError("credential_dialog_failed")
    value = result.stdout
    if not value or any(character.isspace() for character in value):
        raise SupervisorError("invalid_runtime_key_input")
    return value


def fetch_relay_token(service: str, account: str) -> str:
    broker = subprocess.run(
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
        text=True,
        timeout=30,
    )
    token = broker.stdout.rstrip("\r\n")
    if broker.returncode or len(token) < 24:
        raise SupervisorError("relay_token_brokerage_failed")
    return token


def load_private_config(path: Path, expected_call_seconds: int) -> None:
    config = tomllib.loads(path.read_text())
    relay = config["relay"]
    if relay["max_call_seconds"] != expected_call_seconds:
        raise SupervisorError("unexpected_call_duration_limit")
    if relay["bind"] != "127.0.0.1:8787" or relay["control_bind"] != "127.0.0.1:8788":
        raise SupervisorError("unexpected_listener_configuration")
    telnyx = config.get("telnyx")
    if not isinstance(telnyx, dict) or not str(telnyx.get("public_wss_base", "")).startswith(
        "wss://"
    ):
        raise SupervisorError("missing_telnyx_runtime_configuration")


def validate_profile(path: Path, binary: Path, runtime: Path) -> None:
    if not path.is_file() or path.stat().st_mode & 0o077:
        raise SupervisorError("live_profile_missing_or_not_private")
    text = path.read_text()
    if str(binary) not in text or str(runtime) not in text:
        raise SupervisorError("live_profile_target_mismatch")
    if 'api_key: "env:CONTROL_PLANE_API_KEY"' not in text:
        raise SupervisorError("live_profile_does_not_broker_runtime_key")


def call_json(command: list[str], env: dict[str, str], timeout: float = 15) -> dict[str, object]:
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise SupervisorError(f"command_failed:{Path(command[0]).name}:{result.returncode}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SupervisorError(f"command_returned_invalid_json:{Path(command[0]).name}") from error


def require_local_survival(
    session_script: Path,
    binary: Path,
    runtime: Path,
    ready_cursor: int,
    env: dict[str, str],
) -> int:
    for _ in range(3):
        status = call_json([str(session_script), "status"], env)
        if not all(status.get(key) is True for key in ("owner_running", "relay_running", "tunnel_running")):
            raise SupervisorError("bellhound_runtime_survival_check_failed")
        time.sleep(0.25)
    call_json([str(binary), "--config", str(runtime), "ping"], env)
    pong = call_json(
        [
            str(binary),
            "--config",
            str(runtime),
            "wait",
            "--after",
            str(ready_cursor),
            "--timeout-seconds",
            "5",
        ],
        env,
        timeout=10,
    )
    if pong.get("event") != "pong":
        raise SupervisorError("bellhound_runtime_ping_failed")
    return int(pong["event_id"])


def wait_for_health(path: Path, timeout: float) -> dict[str, str]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.is_file():
            base = path.read_text().strip().rstrip("/")
            parsed = urllib.parse.urlparse(base)
            if parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost"):
                raise SupervisorError("health_listener_is_not_loopback")
            states = {}
            for endpoint in ("healthz", "readyz"):
                try:
                    with urllib.request.urlopen(f"{base}/{endpoint}", timeout=1) as response:
                        states[endpoint] = str(response.status)
                except OSError:
                    break
            if states == {"healthz": "200", "readyz": "200"}:
                return states
        time.sleep(0.1)
    raise SupervisorError("local_health_readiness_timeout")


def write_json(path: Path, value: dict[str, object]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def wait_for_nonce_release(state: Path, nonce: str, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        release_path = state / "release.json"
        if release_path.is_file():
            try:
                release = json.loads(release_path.read_text())
            except json.JSONDecodeError:
                release = {}
            if release.get("nonce") == nonce:
                return
        time.sleep(0.1)
    raise SupervisorError("authenticated_release_wait_timeout")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worktree", type=Path, required=True)
    parser.add_argument("--base-config", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--profile-dir", type=Path, required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--auth-profile-dir", type=Path, required=True)
    parser.add_argument("--auth-profile", required=True)
    parser.add_argument("--auth-config", type=Path, required=True)
    parser.add_argument(
        "--key-dialog",
        type=Path,
        default=Path(__file__).with_name("mcp-key-dialog.swift"),
    )
    parser.add_argument("--tunnel-client", type=Path, default=Path("/opt/homebrew/bin/tunnel-client"))
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--call-seconds", type=int, default=60)
    parser.add_argument("--owner-seconds", type=int, default=180)
    parser.add_argument("--registration-timeout", type=int, default=20)
    parser.add_argument("--relay-token-service", default="Bellhound relay token")
    parser.add_argument("--relay-token-account", default="default")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not sys.stdin.isatty():
        raise SystemExit("Run this supervisor yourself in Terminal; hidden key entry is required.")
    os.umask(0o077)
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    state = args.state_dir.resolve()
    if not 30 <= args.call_seconds <= 60:
        raise SupervisorError("call_limit_must_be_between_30_and_60_seconds")
    if not 30 <= args.owner_seconds <= 180:
        raise SupervisorError("owner_limit_must_be_between_30_and_180_seconds")
    state.mkdir(parents=True, exist_ok=True)
    control_files = (
        "auth-client.pid",
        "auth-health.url",
        "client.pid",
        "armed.json",
        "release.json",
        "ready.json",
        "stop.json",
        "receipt.json",
        "health.url",
    )
    if any((state / name).exists() for name in control_files):
        raise SupervisorError("state_directory_contains_prior_run_control_files")

    binary = args.target_dir / "release/bellhound"
    runtime = args.worktree / "work/session/bellhound.toml"
    session_script = args.worktree / "skills/bellhound/scripts/session.sh"
    profile_path = args.profile_dir / f"{args.profile}.yaml"
    auth_profile_path = args.auth_profile_dir / f"{args.auth_profile}.yaml"
    head = subprocess.check_output(
        ["git", "-C", str(args.worktree), "rev-parse", "HEAD"], text=True
    ).strip()
    if head != args.expected_head:
        raise SupervisorError("experimental_head_mismatch")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SupervisorError("experimental_binary_unavailable")
    load_private_config(args.base_config, args.call_seconds)
    validate_profile(profile_path, binary, runtime)
    validate_profile(auth_profile_path, binary, args.auth_config)

    runtime_key = read_runtime_key_via_dialog(args.key_dialog)
    relay_token = ""
    env = clean_environment(os.environ)
    env["CONTROL_PLANE_API_KEY"] = runtime_key

    auth_client = owner = client = None
    auth_pump = owner_pump = client_pump = None
    owner_gate = OwnerReadyGate()
    auth_gate = TunnelRegistrationGate()
    tunnel_gate = TunnelRegistrationGate()
    reason = "preflight_failed"
    ready_written = False
    started_at = time.monotonic()
    owner_started_at = None
    nonce = str(uuid.uuid4())
    try:
        auth_client = subprocess.Popen(
            [
                str(args.tunnel_client),
                "run",
                "--profile",
                args.auth_profile,
                "--profile-dir",
                str(args.auth_profile_dir),
                "--open-web-ui=false",
                "--health.url-file",
                str(state / "auth-health.url"),
                "--pid.file",
                str(state / "auth-client.pid"),
            ],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        auth_pump = LinePump(
            auth_client.stdout,
            state / "auth-client.log",
            (runtime_key,),
            auth_gate.feed,
        )
        auth_pump.start()
        wait_for_registration(auth_client, auth_gate, args.registration_timeout)
        stop_group(auth_client)
        auth_pump.join()
        (state / "auth-client.pid").unlink(missing_ok=True)
        (state / "auth-health.url").unlink(missing_ok=True)

        write_json(
            state / "armed.json",
            {
                "state": "authenticated_waiting_release",
                "nonce": nonce,
                "time_utc": datetime.datetime.now(datetime.UTC).isoformat(),
            },
        )
        print(
            "Credential dialog closed and remote authentication passed. "
            "Tell Cora: Gate 2 is authenticated and armed.",
            flush=True,
        )
        wait_for_nonce_release(state, nonce, timeout=900)

        relay_token = fetch_relay_token(args.relay_token_service, args.relay_token_account)
        env.update(
            {
                "BELLHOUND_RELAY_TOKEN": relay_token,
                "BELLHOUND_ROOT": str(args.worktree),
                "BELLHOUND_CONFIG": str(args.base_config),
                "CARGO_TARGET_DIR": str(args.target_dir),
                "BELLHOUND_OWNER_MAX_SECONDS": str(args.owner_seconds),
            }
        )
        owner_started_at = time.monotonic()
        owner = subprocess.Popen(
            [str(session_script), "run"],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        owner_pump = LinePump(
            owner.stdout,
            state / "owner.log",
            (runtime_key, relay_token),
            owner_gate.feed,
        )
        owner_pump.start()
        owner_ready = wait_for_owner(owner, owner_gate, timeout=45)
        ready_cursor = int(owner_ready["event_cursor"])
        pong_cursor = require_local_survival(
            session_script, binary, runtime, ready_cursor, env
        )

        health_url = state / "health.url"
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
                str(health_url),
                "--pid.file",
                str(state / "client.pid"),
            ],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        client_pump = LinePump(
            client.stdout,
            state / "client.log",
            (runtime_key, relay_token),
            tunnel_gate.feed,
        )
        client_pump.start()
        wait_for_registration(client, tunnel_gate, args.registration_timeout)
        health = wait_for_health(health_url, timeout=10)
        if owner.poll() is not None:
            raise SupervisorError("bellhound_owner_ended_before_remote_ready")

        write_json(
            state / "ready.json",
            {
                "state": "ready",
                "nonce": nonce,
                "time_utc": datetime.datetime.now(datetime.UTC).isoformat(),
                "event_cursor": pong_cursor,
                "call_seconds": args.call_seconds,
                "owner_seconds": args.owner_seconds,
                "control_plane_registered": True,
                "health": health,
            },
        )
        ready_written = True
        reason = "operator_stop_or_timeout"
        print("Gate 2 authenticated ready. Keep this Terminal open.", flush=True)
        deadline = owner_started_at + args.owner_seconds
        while time.monotonic() < deadline:
            if tunnel_gate.failed.is_set():
                reason = tunnel_gate.failure_code or "control_plane_registration_failed"
                break
            if client.poll() is not None:
                reason = f"tunnel_client_exited:{client.returncode}"
                break
            if owner.poll() is not None:
                reason = f"bellhound_owner_exited:{owner.returncode}"
                break
            stop_file = state / "stop.json"
            if stop_file.is_file():
                try:
                    stop = json.loads(stop_file.read_text())
                except json.JSONDecodeError:
                    stop = {}
                if stop.get("nonce") == nonce:
                    reason = "operator_stop"
                    break
            time.sleep(0.1)
        else:
            reason = "owner_deadline"
    except KeyboardInterrupt:
        reason = "user_interrupt"
    except Exception as error:
        if isinstance(error, SupervisorError):
            reason = str(error)
        else:
            reason = f"unexpected_error:{type(error).__name__}"
        print(f"Gate 2 stopped before root-ready: {reason}", flush=True)
    finally:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        cleanup_errors = []

        def attempt(label, operation):
            try:
                operation()
            except Exception as error:
                cleanup_errors.append(f"{label}:{type(error).__name__}")

        try:
            attempt("live_client", lambda: stop_group(client))
            attempt("auth_client", lambda: stop_group(auth_client))
            if owner is not None:
                def stop_session():
                    result = subprocess.run(
                        [str(session_script), "stop"],
                        env=env,
                        capture_output=True,
                        text=True,
                        timeout=20,
                    )
                    if result.returncode:
                        raise SupervisorError(
                            f"session_stop_failed:{result.returncode}"
                        )

                attempt("session_stop", stop_session)
            attempt("owner_group", lambda: stop_group(owner))
            if client_pump is not None:
                attempt("live_log_join", client_pump.join)
            if auth_pump is not None:
                attempt("auth_log_join", auth_pump.join)
            if owner_pump is not None:
                attempt("owner_log_join", owner_pump.join)
            for name in (
                "auth-client.pid",
                "auth-health.url",
                "client.pid",
                "armed.json",
                "release.json",
                "ready.json",
                "stop.json",
                "health.url",
            ):
                try:
                    (state / name).unlink(missing_ok=True)
                except OSError as error:
                    cleanup_errors.append(
                        f"control_file_cleanup:{name}:{type(error).__name__}"
                    )
            write_json(
                state / "receipt.json",
                {
                    "reason": reason,
                    "ready_was_published": ready_written,
                    "authentication_preflight_registered": auth_gate.registered.is_set(),
                    "authentication_preflight_failed": auth_gate.failed.is_set(),
                    "control_plane_registered": tunnel_gate.registered.is_set(),
                    "authentication_failed": tunnel_gate.failed.is_set(),
                    "client_exit_code": None if client is None else client.poll(),
                    "owner_exit_code": None if owner is None else owner.poll(),
                    "cleanup_errors": cleanup_errors,
                    "elapsed_seconds": round(time.monotonic() - started_at, 3),
                    "owner_elapsed_seconds": None
                    if owner_started_at is None
                    else round(time.monotonic() - owner_started_at, 3),
                },
            )
        finally:
            env.pop("CONTROL_PLANE_API_KEY", None)
            env.pop("BELLHOUND_RELAY_TOKEN", None)
            runtime_key = relay_token = ""
            if cleanup_errors:
                print(
                    "Gate 2 supervisor cleanup was incomplete; inspect receipt.json. "
                    "Credentials were not persisted by this supervisor.",
                    flush=True,
                )
            else:
                print(
                    "Gate 2 supervisor stopped; credentials were not persisted.",
                    flush=True,
                )
    return (
        0
        if ready_written
        and reason in ("operator_stop", "owner_deadline")
        and not cleanup_errors
        else 1
    )


if __name__ == "__main__":
    raise SystemExit(main())
