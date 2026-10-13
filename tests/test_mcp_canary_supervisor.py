import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts/mcp-canary-supervisor.py"
SPEC = importlib.util.spec_from_file_location("mcp_canary_supervisor", MODULE_PATH)
SUPERVISOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUPERVISOR)


class TunnelGateTests(unittest.TestCase):
    def test_local_ready_does_not_prove_registration(self):
        gate = SUPERVISOR.TunnelRegistrationGate()
        gate.feed(json.dumps({"component": "health", "msg": "ready"}))
        self.assertFalse(gate.registered.is_set())
        self.assertFalse(gate.failed.is_set())

    def test_metadata_fetch_proves_registration(self):
        gate = SUPERVISOR.TunnelRegistrationGate()
        gate.feed(
            json.dumps(
                {"component": "controlplane", "msg": "tunnel metadata fetched"}
            )
        )
        self.assertTrue(gate.registered.is_set())
        self.assertFalse(gate.failed.is_set())

    def test_unauthorized_is_immediately_fatal(self):
        gate = SUPERVISOR.TunnelRegistrationGate()
        gate.feed(
            json.dumps(
                {
                    "component": "controlplane",
                    "status_code": 401,
                    "error_code": "invalid_api_key",
                    "error": "masked credential text must never be needed",
                }
            )
        )
        self.assertTrue(gate.failed.is_set())
        self.assertEqual(gate.failure_code, "control_plane_authentication_failed")

    def test_native_dialog_defaults_masked_and_uses_no_clipboard_api(self):
        source = (MODULE_PATH.parent / "mcp-key-dialog.swift").read_text()
        self.assertIn("NSSecureTextField", source)
        self.assertIn('checkboxWithTitle: "Show key"', source)
        self.assertNotIn("NSPasteboard", source)

    def test_environment_scrub_removes_credential_namespaces(self):
        cleaned = SUPERVISOR.clean_environment(
            {
                "PATH": "/bin",
                "OPENAI_API_KEY": "secret",
                "BELLHOUND_RELAY_TOKEN": "secret",
                "CONTROL_PLANE_API_KEY": "secret",
                "UNRELATED": "kept",
            }
        )
        self.assertEqual(cleaned, {"PATH": "/bin", "UNRELATED": "kept"})

    def test_release_barrier_requires_exact_nonce(self):
        with tempfile.TemporaryDirectory() as temp:
            state = Path(temp)
            (state / "release.json").write_text(json.dumps({"nonce": "wrong"}))
            with self.assertRaisesRegex(
                SUPERVISOR.SupervisorError, "authenticated_release_wait_timeout"
            ):
                SUPERVISOR.wait_for_nonce_release(state, "expected", timeout=0.15)
            (state / "release.json").write_text(json.dumps({"nonce": "expected"}))
            SUPERVISOR.wait_for_nonce_release(state, "expected", timeout=0.15)


@unittest.skipUnless(hasattr(os, "killpg"), "POSIX process groups required")
class ProcessTests(unittest.TestCase):
    def run_fake_client(self, record):
        code = (
            "import json,time;"
            f"print(json.dumps({record!r}),flush=True);"
            "time.sleep(30)"
        )
        return subprocess.Popen(
            [sys.executable, "-c", code],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )

    def test_wait_for_registration_accepts_positive_control_plane_event(self):
        process = self.run_fake_client(
            {"component": "controlplane", "msg": "tunnel metadata fetched"}
        )
        gate = SUPERVISOR.TunnelRegistrationGate()
        with tempfile.TemporaryDirectory() as temp:
            pump = SUPERVISOR.LinePump(
                process.stdout, Path(temp) / "client.log", (), gate.feed
            )
            pump.start()
            SUPERVISOR.wait_for_registration(process, gate, timeout=2)
            self.assertTrue(gate.registered.is_set())
            stopped_at = time.monotonic()
            SUPERVISOR.stop_group(process)
            self.assertLess(time.monotonic() - stopped_at, 2)
            pump.join()

    def test_wait_for_registration_rejects_401_without_timeout(self):
        process = self.run_fake_client(
            {
                "component": "controlplane",
                "status_code": 401,
                "error_code": "invalid_api_key",
            }
        )
        gate = SUPERVISOR.TunnelRegistrationGate()
        started = time.monotonic()
        with tempfile.TemporaryDirectory() as temp:
            pump = SUPERVISOR.LinePump(
                process.stdout, Path(temp) / "client.log", (), gate.feed
            )
            pump.start()
            with self.assertRaisesRegex(
                SUPERVISOR.SupervisorError, "control_plane_authentication_failed"
            ):
                SUPERVISOR.wait_for_registration(process, gate, timeout=5)
            self.assertLess(time.monotonic() - started, 2)
            SUPERVISOR.stop_group(process)
            pump.join()

    def test_late_401_during_registration_stability_window_is_fatal(self):
        code = (
            "import json,time;"
            "print(json.dumps({'component':'controlplane','msg':'tunnel metadata fetched'}),flush=True);"
            "time.sleep(.1);"
            "print(json.dumps({'component':'controlplane','status_code':401,'error_code':'invalid_api_key'}),flush=True);"
            "time.sleep(30)"
        )
        process = subprocess.Popen(
            [sys.executable, "-c", code],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        gate = SUPERVISOR.TunnelRegistrationGate()
        with tempfile.TemporaryDirectory() as temp:
            pump = SUPERVISOR.LinePump(
                process.stdout, Path(temp) / "client.log", (), gate.feed
            )
            pump.start()
            with self.assertRaisesRegex(
                SUPERVISOR.SupervisorError, "control_plane_authentication_failed"
            ):
                SUPERVISOR.wait_for_registration(process, gate, timeout=5)
            SUPERVISOR.stop_group(process, grace_seconds=0.05)
            pump.join()

    def test_stop_group_kills_process_ignoring_int_and_term(self):
        code = (
            "import signal,time;"
            "signal.signal(signal.SIGINT,signal.SIG_IGN);"
            "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
            "time.sleep(60)"
        )
        process = subprocess.Popen(
            [sys.executable, "-c", code], start_new_session=True
        )
        time.sleep(0.1)
        SUPERVISOR.stop_group(process, grace_seconds=0.05)
        self.assertIsNotNone(process.returncode)

    def test_stop_group_kills_descendant_after_group_leader_exits(self):
        code = (
            "import subprocess,sys;"
            "child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)'],"
            "stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);"
            "print(child.pid,flush=True)"
        )
        leader = subprocess.Popen(
            [sys.executable, "-c", code],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        child_pid = int(leader.stdout.readline().strip())
        leader.stdout.close()
        leader.wait(timeout=2)
        try:
            SUPERVISOR.stop_group(leader, grace_seconds=0.05)
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                try:
                    os.kill(child_pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail("descendant survived process-group cleanup")
        finally:
            try:
                os.kill(child_pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

    def test_owner_ready_receipt_is_required(self):
        process = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import json,time;print(json.dumps({'state':'ready','event_cursor':7}),flush=True);time.sleep(30)",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        gate = SUPERVISOR.OwnerReadyGate()
        with tempfile.TemporaryDirectory() as temp:
            pump = SUPERVISOR.LinePump(
                process.stdout, Path(temp) / "owner.log", (), gate.feed
            )
            pump.start()
            receipt = SUPERVISOR.wait_for_owner(process, gate, timeout=2)
            self.assertEqual(receipt["event_cursor"], 7)
            SUPERVISOR.stop_group(process, grace_seconds=0.05)
            pump.join()

    def test_owner_exit_before_ready_is_fatal(self):
        process = subprocess.Popen(
            [sys.executable, "-c", "raise SystemExit(3)"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        gate = SUPERVISOR.OwnerReadyGate()
        with tempfile.TemporaryDirectory() as temp:
            pump = SUPERVISOR.LinePump(
                process.stdout, Path(temp) / "owner.log", (), gate.feed
            )
            pump.start()
            with self.assertRaisesRegex(
                SUPERVISOR.SupervisorError, "bellhound_owner_exited_before_ready:3"
            ):
                SUPERVISOR.wait_for_owner(process, gate, timeout=2)
            pump.join()


if __name__ == "__main__":
    unittest.main()
