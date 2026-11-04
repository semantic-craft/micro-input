import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "pstack-relay"


class PStackRelayLauncherTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        (self.root / "docs").mkdir(parents=True, exist_ok=True)
        sample = (REPO_ROOT / "docs" / "pstack-relay.md").read_text(encoding="utf-8")
        for letter in ("B", "C", "D", "E"):
            sample = sample.replace(f"### [x] Slice {letter}:", f"### [ ] Slice {letter}:")
        (self.root / "docs" / "pstack-relay.md").write_text(sample, encoding="utf-8")
        self.env = dict(os.environ)
        self.env["MI_PSTACK_RELAY_ROOT"] = str(self.root)
        self.env["MI_DISABLE_PBCOPY"] = "1"

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_status_generates_next_prompt_and_state(self) -> None:
        res = subprocess.run(
            ["python3", str(SCRIPT), "status"],
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertIn("Slice B", res.stdout)
        self.assertIn("## Next Chat Prompt (Copy & Paste)", res.stdout)
        prompt_file = self.root / ".scratch" / "pstack-relay" / "NEXT_PROMPT.md"
        self.assertTrue(prompt_file.exists())
        self.assertIn("Goal (**Slice B:", prompt_file.read_text(encoding="utf-8"))

    def test_advance_marks_slice_done_logs_decision_and_advances_to_c(self) -> None:
        res = subprocess.run(
            [
                "python3",
                str(SCRIPT),
                "advance",
                "B",
                "--evidence",
                "commit-deadbeef",
                "--why",
                "dial scroll verified",
            ],
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertIn("Next: Slice C", res.stdout)
        decisions = (self.root / ".scratch" / "pstack-relay" / "decisions.tsv").read_text(
            encoding="utf-8"
        )
        self.assertIn("slice-b", decisions)
        self.assertIn("commit-deadbeef", decisions)
        doc_text = (self.root / "docs" / "pstack-relay.md").read_text(encoding="utf-8")
        self.assertIn("### [x] Slice B:", doc_text)

    def test_advance_adds_missing_evidence_and_updates_it_without_duplicates(self) -> None:
        doc = self.root / "docs" / "pstack-relay.md"
        doc.write_text("### [ ] Slice F: Ownership\n- **Outcome**: Preserve native owner\n"
                       "- **Done Predicate**: one action\n\n"
                       "### [ ] Slice G: Navigation\n- **Outcome**: precise target\n")
        for receipt in ("receipt-one.json", "receipt-two.json"):
            result = subprocess.run(["python3", str(SCRIPT), "advance", "F", "--evidence", receipt],
                                    env=self.env, capture_output=True, text=True, check=True)
            self.assertIn("Next: Slice G", result.stdout)
            self.assertIn(f"evidence: {receipt}", result.stdout)
            self.assertEqual(doc.read_text().count("- **Evidence**:"), 1)
            self.assertIn(f"- **Evidence**: {receipt}", doc.read_text())

    def test_hooks_return_valid_json_for_session_start_and_stop(self) -> None:
        start_res = subprocess.run(
            ["python3", str(SCRIPT), "hook", "SessionStart"],
            input=json.dumps({"conversationId": "test-1"}),
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )
        payload = json.loads(start_res.stdout)
        self.assertIn("injectSteps", payload)

        stop_res = subprocess.run(
            ["python3", str(SCRIPT), "hook", "Stop"],
            input=json.dumps({"terminationReason": "NO_TOOL_CALL"}),
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(json.loads(stop_res.stdout), {})


if __name__ == "__main__":
    unittest.main()
