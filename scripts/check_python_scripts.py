#!/usr/bin/env python3
"""Zero-dependency Python AST guard for scripts/."""
from __future__ import annotations

import sys

sys.dont_write_bytecode = True

import argparse
import ast
from dataclasses import dataclass
import os
from pathlib import Path

PYTHON3_SHEBANG = "#!/usr/bin/env python3"
SUBPROCESS_SHELL_APIS = frozenset({"run", "Popen", "check_output", "check_call"})
PATH_OPEN_FIX = (
    "Path.open() does not accept opener=; "
    "use built-in open(path, ..., opener=...) instead."
)
SUBPROCESS_SHELL_FIX = (
    "subprocess with shell=True is forbidden; "
    "pass an explicit argv list instead."
)


@dataclass(frozen=True, order=True)
class Violation:
    path: str
    line: int
    message: str

    def format(self) -> str:
        return f"{self.path}:{self.line}: {self.message}"


def has_python3_shebang(path: Path) -> bool:
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            first_line = handle.readline()
    except OSError:
        return False
    return first_line.startswith(PYTHON3_SHEBANG)


def discover_python_scripts(scripts_dir: Path) -> list[Path]:
    discovered: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(scripts_dir):
        dirnames[:] = sorted(name for name in dirnames if name != "__pycache__")
        base = Path(dirpath)
        for filename in sorted(filenames):
            candidate = base / filename
            if not candidate.is_file():
                continue
            if candidate.suffix == ".py" or (
                candidate.suffix == "" and has_python3_shebang(candidate)
            ):
                discovered.append(candidate)
    return sorted(discovered)


def collect_subprocess_bindings(tree: ast.AST) -> tuple[set[str], set[str]]:
    module_names = {"subprocess"}
    func_names: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name == "subprocess":
                    module_names.add(alias.asname or alias.name)
        elif isinstance(node, ast.ImportFrom) and node.module == "subprocess":
            for alias in node.names:
                if alias.name in SUBPROCESS_SHELL_APIS:
                    func_names.add(alias.asname or alias.name)
    return module_names, func_names


def is_subprocess_shell_api(
    call: ast.Call, module_names: set[str], func_names: set[str]
) -> bool:
    if isinstance(call.func, ast.Attribute):
        return (
            call.func.attr in SUBPROCESS_SHELL_APIS
            and isinstance(call.func.value, ast.Name)
            and call.func.value.id in module_names
        )
    if isinstance(call.func, ast.Name):
        return call.func.id in func_names
    return False


def has_keyword_arg(call: ast.Call, name: str) -> bool:
    return any(kw.arg == name for kw in call.keywords)


def has_true_keyword_arg(call: ast.Call, name: str) -> bool:
    return any(
        kw.arg == name and isinstance(kw.value, ast.Constant) and kw.value.value is True
        for kw in call.keywords
    )


def check_file(path: Path, repo_root: Path) -> list[Violation]:
    rel_path = path.relative_to(repo_root).as_posix()
    source = path.read_text(encoding="utf-8")
    try:
        tree = ast.parse(source, filename=rel_path)
    except SyntaxError as exc:
        return [Violation(rel_path, exc.lineno or 1, f"SyntaxError: {exc.msg}")]

    module_names, func_names = collect_subprocess_bindings(tree)
    violations: list[Violation] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        if (
            isinstance(node.func, ast.Attribute)
            and node.func.attr == "open"
            and has_keyword_arg(node, "opener")
        ):
            violations.append(Violation(rel_path, node.lineno, PATH_OPEN_FIX))
        if is_subprocess_shell_api(node, module_names, func_names) and has_true_keyword_arg(
            node, "shell"
        ):
            violations.append(Violation(rel_path, node.lineno, SUBPROCESS_SHELL_FIX))
    return sorted(violations)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Static AST guard for Python scripts under scripts/."
    )
    parser.add_argument(
        "root",
        nargs="?",
        default=str(Path(__file__).resolve().parents[1]),
        help="Repository root containing scripts/.",
    )
    args = parser.parse_args(argv)
    repo_root = Path(args.root).resolve()
    scripts_dir = repo_root / "scripts"
    if not scripts_dir.is_dir():
        print(f"scripts directory not found under {repo_root}", file=sys.stderr)
        return 2

    violations: list[Violation] = []
    for script_path in discover_python_scripts(scripts_dir):
        violations.extend(check_file(script_path, repo_root))

    for violation in sorted(violations):
        print(violation.format())
    return 1 if violations else 0


if __name__ == "__main__":
    raise SystemExit(main())
