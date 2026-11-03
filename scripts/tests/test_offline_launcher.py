import importlib.machinery
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

loader = importlib.machinery.SourceFileLoader("offline_verifier", str(Path(__file__).parents[1] / "verify-offline"))
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class OfflineEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.run = Path(self.tmp.name)
        self.evidence = self.run / "evidence"
        self.evidence.mkdir()
        self.launcher = dict(bundle_id="com.semanticcraft.microinput.qa", started_ms=100,
                             pid=123, app_path=str(self.run / "gone.app"), document_dir=str(self.run / "gone-docs"),
                             app_bundle_removed=True, documents_removed=True, preferences_removed=True)
        self.write_launcher()
        self.rows = [dict(phase="Verified", config_readback="PASS", physical_key="NOT_RUN", lighting_visual="NOT_RUN")] * 2
        self.write_rows()
        (self.evidence / "lighting.json").write_text(json.dumps(dict(status="PASS", runtime_readback="SYNTHETIC_PASS", physical_key="NOT_RUN", lighting_visual="NOT_RUN")))

    def write_launcher(self):
        (self.evidence / "launcher.json").write_text(json.dumps(self.launcher))

    def write_rows(self):
        (self.evidence / "device-operations.jsonl").write_text("\n".join(json.dumps(r) for r in self.rows))

    def test_complete_lifecycle_is_accepted_without_claiming_hardware(self):
        result = module.verify_evidence(self.run, "lighting-control", 99, lambda _: False)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["lighting_visual"], "NOT_RUN")

    def test_diagnostic_export_requires_only_sanitized_wrapper(self):
        (self.evidence / "diagnostics.json").write_text(json.dumps(dict(status="PASS", failure_stage="config_read", physical_key="NOT_RUN", lighting_visual="NOT_RUN")))
        result = module.verify_evidence(self.run, "diagnostic-export", 99, lambda _: False)
        self.assertEqual(result["diagnostic_export"], "PASS")
        (self.evidence / "diagnostics.json").write_text(json.dumps(dict(status="PASS", failure_stage="config_read", physical_key="PASS", lighting_visual="NOT_RUN")))
        with self.assertRaises(ValueError):
            module.verify_evidence(self.run, "diagnostic-export", 99, lambda _: False)

    def test_stale_evidence_live_process_cleanup_and_overclaim_fail(self):
        for modify in (lambda: self.launcher.update(started_ms=98),
                       lambda: self.launcher.update(documents_removed=False)):
            original = self.launcher.copy()
            modify(); self.write_launcher()
            with self.assertRaises(ValueError): module.verify_evidence(self.run, "config-write", 99, lambda _: False)
            self.launcher = original; self.write_launcher()
        with self.assertRaises(ValueError): module.verify_evidence(self.run, "config-write", 99, lambda _: True)
        self.rows[0]["lighting_visual"] = "PASS"; self.write_rows()
        with self.assertRaises(ValueError): module.verify_evidence(self.run, "config-write", 99, lambda _: False)

    def test_missing_runtime_evidence_is_not_silently_skipped(self):
        (self.evidence / "lighting.json").unlink()
        with self.assertRaises(FileNotFoundError): module.verify_evidence(self.run, "lighting-control", 99, lambda _: False)
