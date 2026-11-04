import importlib.machinery
import importlib.util
from pathlib import Path
import unittest

loader = importlib.machinery.SourceFileLoader("paired_verifier", str(Path(__file__).parents[1] / "verify-cindy-paired"))
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class PairedEvidenceTests(unittest.TestCase):
    def fixture(self):
        phases = [("native-wide", 1, 0), ("native-encoder", 1, 0),
                  ("released-wide", 0, 1), ("released-encoder", 0, 1),
                  ("stale-profile-encoder", 0, 0), ("fresh-encoder", 0, 1),
                  ("lost-wide", 0, 0), ("lost-encoder", 0, 0)]
        ownership = {"paired_native_source": "PASS", "paired_events": [
            dict(event_id=key, native_actions=native, micro_actions=micro) for key, native, micro in phases]}
        native = dict(native_source="WorkLouderCodexLightingController", backend="synthetic-sink",
                      transport="real-unix-socket", native_install="NOT_RUN", physical="NOT_RUN", events=[
                          dict(eventId=key, nativeBusinessActions=count, disconnected=key.startswith("lost-"))
                          for key, count, _ in phases])
        return ownership, native

    def test_accepts_single_consumer_with_real_disconnect_and_no_hardware_claim(self):
        self.assertEqual(module.verify_pair(*self.fixture()), 8)

    def test_rejects_double_execution_missing_actions_and_mismatched_native_receipt(self):
        for index, field, value in [(0, "micro_actions", 1), (2, "micro_actions", 0), (6, "native_actions", 1)]:
            ownership, native = self.fixture()
            ownership["paired_events"][index][field] = value
            with self.assertRaises(ValueError): module.verify_pair(ownership, native)
        ownership, native = self.fixture()
        native["events"][0]["nativeBusinessActions"] = 0
        with self.assertRaises(ValueError): module.verify_pair(ownership, native)

    def test_rejects_duplicate_events_fake_disconnect_and_native_overclaim(self):
        ownership, native = self.fixture()
        ownership["paired_events"][1] = ownership["paired_events"][0]
        with self.assertRaises(ValueError): module.verify_pair(ownership, native)
        ownership, native = self.fixture()
        native["events"][-1]["disconnected"] = False
        with self.assertRaises(ValueError): module.verify_pair(ownership, native)
        native["native_install"] = "PASS"
        with self.assertRaises(ValueError): module.verify_pair(ownership, native)
