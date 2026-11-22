import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CHECK_COMMENTS = REPO_ROOT / "scripts" / "check_comments.py"
CHECK_SCRIPT = REPO_ROOT / "scripts" / "check"
EXPECTED_FIX = (
    "Delete the ordinary comment, encode its constraint in types/lints/tests, "
    "or record a non-obvious decision in docs/DECISIONS.md"
)


class CommentRatchetLauncherTests(unittest.TestCase):
    def _run_check(self, root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, "-B", str(CHECK_COMMENTS), "check", str(root)],
            capture_output=True,
            text=True,
            check=False,
        )

    def _write_zero_config(self, root: Path) -> None:
        (root / ".comment-ratchet.json").write_text(
            json.dumps({"version": 1, "max_total": 0, "exclude": [], "files": {}}),
            encoding="utf-8",
        )

    def test_allows_doc_comments_safety_inner_attrs_swift_tools_and_shebangs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._write_zero_config(root)
            (root / "lib.rs").write_text(
                "#![allow(dead_code)]\n"
                "//! Crate doc comment.\n"
                "/// Item doc comment.\n"
                "pub unsafe fn ok() {\n"
                "    // SAFETY: invariant verified by caller.\n"
                "}\n",
                encoding="utf-8",
            )
            (root / "Package.swift").write_text(
                "// swift-tools-version: 5.9\n"
                "/// Package doc comment.\n"
                "let value = 1\n",
                encoding="utf-8",
            )
            (root / "tool.py").write_text(
                "#!/usr/bin/env python3\n"
                '"""Module docstring."""\n'
                "VALUE = 1\n",
                encoding="utf-8",
            )
            (root / "run.sh").write_text(
                "#!/bin/bash\n"
                "set -euo pipefail\n"
                "echo ok\n",
                encoding="utf-8",
            )
            proc = self._run_check(root)
            self.assertEqual(proc.returncode, 0, msg=proc.stdout + proc.stderr)
            self.assertEqual(proc.stdout.strip(), "")

    def test_rejects_injected_rust_swift_and_python_ordinary_comments_with_fix(self) -> None:
        cases = [
            (
                "sample.rs",
                "pub fn f() {\n    // narrating rust comment\n}\n",
                "sample.rs:2:",
            ),
            (
                "Sample.swift",
                "// swift-tools-version: 5.9\nlet x = 1\n// narrating swift comment\n",
                "Sample.swift:3:",
            ),
            (
                "sample.py",
                "#!/usr/bin/env python3\n# narrating python comment\nX = 1\n",
                "sample.py:2:",
            ),
        ]
        for filename, content, expected_loc in cases:
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                self._write_zero_config(root)
                (root / filename).write_text(content, encoding="utf-8")
                proc = self._run_check(root)
                self.assertNotEqual(proc.returncode, 0, msg=f"expected failure for {filename}")
                self.assertIn(expected_loc, proc.stdout)
                self.assertIn(EXPECTED_FIX, proc.stdout)

    def test_check_script_invokes_self_hosted_comment_ratchet_unconditionally(self) -> None:
        text = CHECK_SCRIPT.read_text(encoding="utf-8")
        self.assertIn('python3 -B scripts/check_comments.py check "$PWD"', text)
        self.assertNotIn("MICRO_INPUT_COMMENT_RATCHET", text)
        self.assertNotIn("agent-home/tools/comment-ratchet", text)
