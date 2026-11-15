"""Unit tests for scripts/verify-hardware without touching physical USB hardware."""
import importlib.machinery
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]


def load_verify_hardware_module():
    loader = importlib.machinery.SourceFileLoader(
        "verify_hardware", str(SCRIPTS / "verify-hardware")
    )
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


class VerifyHardwareLauncherTests(unittest.TestCase):
    def setUp(self):
        self.mod = load_verify_hardware_module()
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_parse_args_rejects_abbreviations_and_supports_wait_and_skip_h1(self):
        args = self.mod.parse_args(["--wait", "--timeout", "15", "--skip-h1"])
        self.assertTrue(args.wait)
        self.assertEqual(args.timeout, 15.0)
        self.assertTrue(args.skip_h1)
        with self.assertRaises(SystemExit):
            self.mod.parse_args(["--wa"])

    def test_main_orchestrates_h0_h1_and_live_app_receipt(self):
        output_path = self.root / "latest-result.json"
        candidate = {
            "vid": 0x303A,
            "pid": 0x8298,
            "usage_page": 0xFF00,
            "usage": 1,
            "transport_hint": "Usb",
            "product": "Creator Micro 2",
            "path": "DevSrvsID:synthetic-test",
        }
        h0_summary = {
            "passed": True,
            "kit_dir": str(self.root / "kit-h0"),
            "evidence_dir": str(self.root / "ev-h0"),
            "status": {"version": "0.6.3", "profile_index": 0, "layer_index": 1},
            "keymap_size": 7421,
            "keymap_sha1": "a" * 40,
            "keymap_sha256": "b" * 64,
            "smart_actions_present": True,
            "requests": 14,
        }
        h1_summary = {
            "passed": True,
            "cleanup_confirmed": True,
            "kit_dir": str(self.root / "kit-h1"),
            "evidence_dir": str(self.root / "ev-h1"),
            "target_tempfile": "mi_123456789abc.txt",
            "stage_count": 15,
            "requests": 126,
        }
        app_receipt = {
            "schema_version": 1,
            "status": "PASS",
            "session_id": 1,
            "device_path": "DevSrvsID:synthetic-test",
            "firmware_version": "0.6.3",
            "keymap_sha256": "b" * 64,
            "lighting_rgbcfg_synced": True,
            "lighting_preview_synced": True,
            "lighting_thstatus_synced": True,
        }
        with patch.object(self.mod, "wait_for_single_usb_candidate", return_value=candidate), \
             patch.object(self.mod, "quiesce_competing_hid_readers") as quiesce_mock, \
             patch.object(self.mod, "run_h0", return_value=h0_summary) as h0_mock, \
             patch.object(self.mod, "run_h1", return_value=h1_summary) as h1_mock, \
             patch.object(self.mod, "verify_live_app_usb_auto_acceptance", return_value=app_receipt) as app_mock:
            rc = self.mod.main(["--include-h1", "--output", str(output_path)])
        self.assertEqual(rc, 0)
        quiesce_mock.assert_called_once()
        h0_mock.assert_called_once()
        h1_mock.assert_called_once()
        app_mock.assert_called_once_with(
            expected_device_path="DevSrvsID:synthetic-test",
            expected_keymap_sha256="b" * 64,
        )
        saved = json.loads(output_path.read_text(encoding="utf-8"))
        self.assertEqual(saved["status"], "PASS")
        self.assertEqual(saved["h0"]["keymap_sha256"], "b" * 64)
        self.assertTrue(saved["h1"]["cleanup_confirmed"])
        self.assertEqual(saved["app_auto_acceptance"]["status"], "PASS")

    def test_verify_live_app_launches_target_binary_directly_and_cleans_up_in_finally(self):
        doc_dir = self.root / "doc"
        doc_dir.mkdir(parents=True, exist_ok=True)
        run_cmds = []
        popen_calls = []

        class FakeProc:
            def __init__(self, write_status="PASS"):
                self.terminated = False
                self.waited = False
                self._exited = False
                receipt = {
                    "status": write_status,
                    "firmware_version": "0.6.3",
                    "exclusive_vendor": True,
                    "device_path": "DevSrvsID:synthetic-test",
                    "keymap_sha256": "b" * 64,
                    "lighting_rgbcfg_synced": True,
                    "lighting_preview_synced": True,
                    "lighting_thstatus_synced": True,
                    "timestamp_ms": 9_999_999_999_999,
                }
                (doc_dir / "hardware-auto-acceptance.json").write_text(json.dumps(receipt), encoding="utf-8")

            def poll(self):
                return 0 if self._exited else None

            def terminate(self):
                self.terminated = True
                self._exited = True

            def wait(self, timeout=None):
                self.waited = True
                return 0

        created_procs = []

        def fake_run(cmd, cwd=None, check=False, **kwargs):
            run_cmds.append(cmd)
            return None

        def fake_popen(cmd, cwd=None, env=None, **kwargs):
            popen_calls.append({"cmd": cmd, "env": dict(env or {})})
            proc = FakeProc(write_status="PASS" if len(created_procs) == 0 else "FAIL")
            created_procs.append(proc)
            return proc

        env_overrides = {
            "MI_DOCUMENT_DIR": str(doc_dir),
            "__CFBundleIdentifier": "com.google.antigravity",
            "XPC_SERVICE_NAME": "polluted.service",
        }
        with patch.dict("os.environ", env_overrides), \
             patch.object(self.mod.subprocess, "run", side_effect=fake_run), \
             patch.object(self.mod.subprocess, "Popen", side_effect=fake_popen):
            receipt = self.mod.verify_live_app_usb_auto_acceptance("DevSrvsID:synthetic-test", "b" * 64)
            with self.assertRaises(RuntimeError):
                self.mod.verify_live_app_usb_auto_acceptance("DevSrvsID:synthetic-test", "b" * 64)

        self.assertEqual(receipt["status"], "PASS")
        self.assertFalse(any(cmd[0] in ("rsync", "open") for cmd in run_cmds))
        expected_bin = str(self.mod.ROOT / "target" / "MicroInput.app" / "Contents" / "MacOS" / "MicroInput")
        self.assertEqual(len(popen_calls), 2)
        for call_info in popen_calls:
            self.assertEqual(call_info["cmd"], [expected_bin])
            self.assertEqual(call_info["env"].get("MI_DOCUMENT_DIR"), str(doc_dir))
            self.assertNotIn("__CFBundleIdentifier", call_info["env"])
            self.assertNotIn("XPC_SERVICE_NAME", call_info["env"])
        self.assertTrue(all(p.terminated and p.waited for p in created_procs))

    def test_quiesce_competing_hid_readers_polls_until_stopped_processes_exit(self):
        states = iter([True, True, False])
        sleep_calls = []
        with patch.object(self.mod.subprocess, "run") as run_mock, \
             patch.object(self.mod, "_competing_hid_readers_running", side_effect=lambda: next(states)), \
             patch.object(self.mod.time, "sleep", side_effect=lambda s: sleep_calls.append(s)):
            self.mod.quiesce_competing_hid_readers()
        self.assertEqual(len(run_mock.call_args_list), 2)
        self.assertEqual(sleep_calls, [0.1, 0.1])

    def test_quiesce_competing_hid_readers_uses_exact_prefix_and_os_kill_without_pkill(self):
        import os
        import signal
        import subprocess

        self_pid = os.getpid()
        ps_stdout = (
            f"  {self_pid} /Applications/Micro Input.app/Contents/MacOS/MicroInput\n"
            '  42001 tail -f "/Applications/Micro Input.app/Contents/MacOS/MicroInput"\n'
            "  42002 /Applications/Micro Input.app/Contents/MacOS/MicroInput --background\n"
        )
        invoked_cmds = []
        killed = []

        def fake_run(cmd, **kwargs):
            invoked_cmds.append(cmd)
            if cmd[:2] == ["ps", "-ax"]:
                return subprocess.CompletedProcess(cmd, 0, stdout=ps_stdout, stderr="")
            return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

        with patch.object(self.mod.subprocess, "run", side_effect=fake_run), \
             patch.object(self.mod.os, "kill", side_effect=lambda pid, sig: killed.append((pid, sig))), \
             patch.object(self.mod, "_competing_hid_readers_running", return_value=False):
            self.mod.quiesce_competing_hid_readers()

        self.assertFalse(any(cmd[0] == "pkill" for cmd in invoked_cmds), f"Unexpected pkill call: {invoked_cmds}")
        self.assertEqual(killed, [(42002, signal.SIGTERM)])


if __name__ == "__main__":
    unittest.main()
