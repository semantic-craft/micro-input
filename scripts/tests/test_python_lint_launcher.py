import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CHECK_PYTHON_SCRIPTS = REPO_ROOT / "scripts" / "check_python_scripts.py"
CHECK_SCRIPT = REPO_ROOT / "scripts" / "check"


class PythonLintLauncherTests(unittest.TestCase):
    def _run_lint(self, root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, "-B", str(CHECK_PYTHON_SCRIPTS), str(root)],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_rejects_path_open_with_opener_kwarg(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scripts_dir = root / "scripts"
            scripts_dir.mkdir()
            (scripts_dir / "bad_open.py").write_text(
                "from pathlib import Path\n"
                "def write_log(path: Path, private_opener):\n"
                '    with path.open("w", opener=private_opener) as handle:\n'
                '        handle.write("ok")\n',
                encoding="utf-8",
            )
            proc = self._run_lint(root)
            self.assertNotEqual(proc.returncode, 0, msg="expected Path.open(opener=...) to fail")
            output = proc.stdout + proc.stderr
            self.assertIn("scripts/bad_open.py:3:", output)
            self.assertIn("built-in open(path, ..., opener=...)", output)

    def test_rejects_subprocess_shell_true_across_apis_and_extensionless_scripts(self) -> None:
        cases = [
            (
                "bad_run.py",
                "import subprocess\nsubprocess.run(\"ls\", shell=True)\n",
                "scripts/bad_run.py:2:",
            ),
            (
                "bad_popen.py",
                "import subprocess\nsubprocess.Popen(\"ls\", shell=True)\n",
                "scripts/bad_popen.py:2:",
            ),
            (
                "bad_check_output.py",
                "import subprocess\nsubprocess.check_output(\"ls\", shell=True)\n",
                "scripts/bad_check_output.py:2:",
            ),
            (
                "bad_check_call.py",
                "import subprocess\nsubprocess.check_call(\"ls\", shell=True)\n",
                "scripts/bad_check_call.py:2:",
            ),
            (
                "extensionless-tool",
                "#!/usr/bin/env python3\nimport subprocess\nsubprocess.run(\"ls\", shell=True)\n",
                "scripts/extensionless-tool:3:",
            ),
        ]
        for filename, content, expected_loc in cases:
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                scripts_dir = root / "scripts"
                scripts_dir.mkdir()
                (scripts_dir / filename).write_text(content, encoding="utf-8")
                proc = self._run_lint(root)
                self.assertNotEqual(proc.returncode, 0, msg=f"expected failure for {filename}")
                output = proc.stdout + proc.stderr
                self.assertIn(expected_loc, output)
                self.assertIn("explicit argv list", output)

    def test_rejects_syntax_error_with_file_and_line(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scripts_dir = root / "scripts"
            scripts_dir.mkdir()
            (scripts_dir / "broken.py").write_text(
                "def broken(:\n    pass\n",
                encoding="utf-8",
            )
            proc = self._run_lint(root)
            self.assertNotEqual(proc.returncode, 0, msg="expected syntax error to fail")
            output = proc.stdout + proc.stderr
            self.assertIn("scripts/broken.py:1:", output)

    def test_rejects_module_class_function_and_async_docstrings(self) -> None:
        cases = [
            (
                "module_doc.py",
                '"""Module docstring."""\nX = 1\n',
                "scripts/module_doc.py:1:",
            ),
            (
                "cli-tool",
                '#!/usr/bin/env python3\n\'\'\'Extensionless module docstring.\'\'\'\nX = 1\n',
                "scripts/cli-tool:2:",
            ),
            (
                "class_doc.py",
                'class Widget:\n    """Class docstring."""\n    pass\n',
                "scripts/class_doc.py:2:",
            ),
            (
                "func_doc.py",
                'def compute() -> int:\n    """Function docstring."""\n    return 1\n',
                "scripts/func_doc.py:2:",
            ),
            (
                "async_doc.py",
                'async def fetch() -> int:\n    """Async function docstring."""\n    return 1\n',
                "scripts/async_doc.py:2:",
            ),
        ]
        for filename, content, expected_loc in cases:
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                scripts_dir = root / "scripts"
                scripts_dir.mkdir()
                (scripts_dir / filename).write_text(content, encoding="utf-8")
                proc = self._run_lint(root)
                self.assertNotEqual(proc.returncode, 0, msg=f"expected failure for {filename}")
                output = proc.stdout + proc.stderr
                self.assertIn(expected_loc, output)
                self.assertIn("Python docstrings are forbidden", output)

    def test_allows_builtin_open_with_opener_argv_subprocess_and_non_docstring_literals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scripts_dir = root / "scripts"
            scripts_dir.mkdir()
            (scripts_dir / "good.py").write_text(
                "import argparse\n"
                "import subprocess\n"
                "from pathlib import Path\n"
                'HELP_TEXT = """Assigned multi-line string constant."""\n'
                "def run_ok(path: Path, private_opener):\n"
                '    parser = argparse.ArgumentParser(description="Inline CLI description.")\n'
                '    with open(path, "w", opener=private_opener) as handle:\n'
                "        handle.write(HELP_TEXT + str(parser.prog))\n"
                '    subprocess.run(["ls"])\n'
                '    subprocess.run(["ls"], shell=False)\n',
                encoding="utf-8",
            )
            (scripts_dir / "shell-script").write_text(
                "#!/bin/bash\necho not python\n",
                encoding="utf-8",
            )
            proc = self._run_lint(root)
            self.assertEqual(proc.returncode, 0, msg=proc.stdout + proc.stderr)

    def test_real_repository_passes(self) -> None:
        proc = self._run_lint(REPO_ROOT)
        self.assertEqual(proc.returncode, 0, msg=proc.stdout + proc.stderr)

    def test_check_script_invokes_python_lint_unconditionally(self) -> None:
        text = CHECK_SCRIPT.read_text(encoding="utf-8")
        self.assertIn('python3 -B scripts/check_python_scripts.py "$PWD"', text)
