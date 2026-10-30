import importlib.util
import os
from pathlib import Path
import plistlib
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import uuid


MODULE_PATH = Path(__file__).parents[1] / "scripts/mcp-service-supervisor.py"
SPEC = importlib.util.spec_from_file_location("mcp_service_supervisor", MODULE_PATH)
SERVICE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SERVICE)


class WatchdogTests(unittest.TestCase):
    def test_watchdog_fires_after_limit_plus_grace(self):
        watchdog = SERVICE.CallWatchdog(300, grace_seconds=5)
        self.assertFalse(watchdog.observe(True, 100.0))
        self.assertFalse(watchdog.observe(True, 404.9))
        self.assertTrue(watchdog.observe(True, 405.0))

    def test_inactive_call_resets_watchdog(self):
        watchdog = SERVICE.CallWatchdog(30, grace_seconds=5)
        watchdog.observe(True, 10.0)
        watchdog.observe(False, 20.0)
        self.assertIsNone(watchdog.started_at)
        self.assertFalse(watchdog.observe(True, 40.0))

    def test_completed_hangup_counts_and_resets(self):
        watchdog = SERVICE.CallWatchdog(30)
        watchdog.observe(True, 1.0)
        watchdog.completed_hangup()
        self.assertEqual(watchdog.hangups, 1)
        self.assertIsNone(watchdog.started_at)

    def test_lost_control_path_is_overdue_only_after_deadline(self):
        watchdog = SERVICE.CallWatchdog(30, grace_seconds=5)
        watchdog.observe(True, 10.0)
        self.assertFalse(watchdog.overdue_without_control(44.9))
        self.assertTrue(watchdog.overdue_without_control(45.0))


class BoundaryTests(unittest.TestCase):
    def test_service_environment_disables_runtime_builds(self):
        args = type(
            "Args",
            (),
            {
                "root": Path("/tmp/bellhound"),
                "base_config": Path("/tmp/bellhound/config.toml"),
            },
        )()
        env = SERVICE.service_environment(
            {"PATH": "/bin", "BELLHOUND_OLD": "remove-me"},
            "runtime-key",
            "relay-token",
            args,
        )
        self.assertEqual(env["BELLHOUND_AUTO_BUILD"], "0")
        self.assertEqual(env["BELLHOUND_OWNER_MAX_SECONDS"], "0")
        self.assertNotIn("BELLHOUND_OLD", env)

    def test_begin_service_run_removes_stale_receipt_and_identifies_run(self):
        with tempfile.TemporaryDirectory() as temp:
            state_dir = Path(temp)
            receipt = state_dir / "receipt.json"
            receipt.write_text('{"reason":"old_failure"}\n')
            run_id, started_at_unix_ms = SERVICE.begin_service_run(state_dir)
            self.assertFalse(receipt.exists())
            self.assertEqual(str(uuid.UUID(run_id)), run_id)
            self.assertGreater(started_at_unix_ms, 0)

    def test_service_owner_refuses_to_build_stale_source(self):
        root = MODULE_PATH.parents[1]
        with tempfile.TemporaryDirectory() as temp:
            service_root = Path(temp)
            (service_root / "src").mkdir()
            (service_root / "work").mkdir()
            (service_root / "target/release").mkdir(parents=True)
            binary = service_root / "target/release/bellhound"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
            os.utime(binary, (1, 1))
            (service_root / "Cargo.toml").write_text("[package]\nname='test'\nversion='0.1.0'\n")
            (service_root / "Cargo.lock").write_text("")
            (service_root / "src/main.rs").write_text("fn main() {}\n")
            config = service_root / "work/bellhound-keychain.toml"
            config.write_text("[relay]\nbind='127.0.0.1:8787'\n")
            env = os.environ.copy()
            env.update(
                {
                    "BELLHOUND_ROOT": str(service_root),
                    "BELLHOUND_CONFIG": str(config),
                    "BELLHOUND_RELAY_TOKEN": "test-token-with-at-least-24-characters",
                    "BELLHOUND_OWNER_MAX_SECONDS": "0",
                    "BELLHOUND_AUTO_BUILD": "0",
                }
            )
            result = subprocess.run(
                [str(root / "skills/bellhound/scripts/session.sh"), "run"],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release binary is stale", result.stderr)
            self.assertIn("mcp-service-macos.sh install", result.stderr)

    def test_binary_selection_supports_source_and_release_archive_layouts(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive_binary = root / "bellhound"
            archive_binary.touch()
            self.assertEqual(SERVICE.select_binary(root), archive_binary.resolve())

            source_binary = root / "target/release/bellhound"
            source_binary.parent.mkdir(parents=True)
            source_binary.touch()
            self.assertEqual(SERVICE.select_binary(root), source_binary.resolve())

            explicit = root / "custom-bellhound"
            self.assertEqual(SERVICE.select_binary(root, explicit), explicit.resolve())

    def test_environment_scrub_removes_all_secret_namespaces(self):
        cleaned = SERVICE.clean_environment(
            {
                "PATH": "/bin",
                "OPENAI_API_KEY": "secret",
                "BELLHOUND_RELAY_TOKEN": "secret",
                "CONTROL_PLANE_API_KEY": "secret",
                "UNRELATED": "kept",
            }
        )
        self.assertEqual(cleaned, {"PATH": "/bin", "UNRELATED": "kept"})

    def test_registration_requires_positive_control_plane_event(self):
        gate = SERVICE.TunnelRegistrationGate()
        gate.feed('{"component":"health","msg":"ready"}')
        self.assertFalse(gate.registered.is_set())
        gate.feed('{"component":"controlplane","msg":"tunnel metadata fetched"}')
        self.assertTrue(gate.registered.is_set())

    def test_invalid_key_is_fatal(self):
        gate = SERVICE.TunnelRegistrationGate()
        gate.feed('{"component":"controlplane","status_code":401,"error_code":"invalid_api_key"}')
        self.assertTrue(gate.failed.is_set())

    def test_macos_installer_renders_private_valid_launch_agent(self):
        root = MODULE_PATH.parents[1]
        with tempfile.TemporaryDirectory() as temp:
            env = os.environ.copy()
            env["HOME"] = temp
            result = subprocess.run(
                [str(root / "scripts/mcp-service-macos.sh"), "render"],
                cwd=root,
                env=env,
                check=True,
                capture_output=True,
                text=True,
            )
            destination = Path(result.stdout.strip())
            self.assertEqual(destination.stat().st_mode & 0o777, 0o600)
            with destination.open("rb") as source:
                payload = plistlib.load(source)
            self.assertEqual(payload["Label"], "com.justinweiler.bellhound-mcp")
            self.assertEqual(payload["KeepAlive"], {"SuccessfulExit": False})
            self.assertEqual(payload["ExitTimeOut"], 120)
            self.assertIn("mcp-service-supervisor.py", payload["ProgramArguments"][1])
            self.assertNotIn(
                str(Path(shutil.which("cargo")).parent), payload["EnvironmentVariables"]["PATH"]
            )
            rendered = destination.read_text()
            self.assertNotIn("sk-", rendered)
            self.assertNotIn("BELLHOUND_RELAY_TOKEN", rendered)


@unittest.skipUnless(hasattr(os, "killpg"), "POSIX process groups required")
class ProcessCleanupTests(unittest.TestCase):
    def test_stop_group_kills_process_ignoring_int_and_term(self):
        code = (
            "import signal,time;"
            "signal.signal(signal.SIGINT,signal.SIG_IGN);"
            "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
            "time.sleep(30)"
        )
        process = subprocess.Popen([sys.executable, "-c", code], start_new_session=True)
        try:
            time.sleep(0.1)
            SERVICE.stop_group(process, grace_seconds=0.1)
            self.assertIsNotNone(process.poll())
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()

    def test_stop_group_tolerates_exited_group_leader(self):
        process = subprocess.Popen([sys.executable, "-c", "pass"], start_new_session=True)
        process.wait(timeout=2)
        SERVICE.stop_group(process, grace_seconds=0.1)
        self.assertIsNotNone(process.returncode)


if __name__ == "__main__":
    unittest.main()
