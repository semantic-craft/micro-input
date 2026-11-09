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


if __name__ == "__main__":
    unittest.main()
