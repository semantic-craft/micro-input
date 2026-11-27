#!/usr/bin/env python3
from __future__ import annotations

import sys

sys.dont_write_bytecode = True

import argparse
import bisect
from dataclasses import dataclass
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tokenize

CONFIG_NAME = ".comment-ratchet.json"
ACTIONABLE_FIX = (
    "Delete the ordinary comment, encode its constraint in types/lints/tests, "
    "or record a non-obvious decision in docs/DECISIONS.md"
)

SKIP_DIRS = frozenset({
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".build",
    ".next",
    ".nuxt",
    "__pycache__",
    ".venv",
    "venv",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    "vendor",
    "Vendor",
    "third_party",
    "Pods",
    "DerivedData",
    ".tox",
    "coverage",
    ".turbo",
    ".cache",
    "_done",
    "_资料与归档",
    "_artifacts",
    ".worktrees",
    "downloads",
})

EXT_TO_LANG = {
    ".py": "python",
    ".rs": "rust",
    ".swift": "swift",
    ".js": "js",
    ".jsx": "js",
    ".ts": "js",
    ".tsx": "js",
    ".mjs": "js",
    ".cjs": "js",
    ".c": "c_go",
    ".cc": "c_go",
    ".cpp": "c_go",
    ".h": "c_go",
    ".hpp": "c_go",
    ".go": "c_go",
    ".sh": "shell",
    ".bash": "shell",
    ".zsh": "shell",
}

SHEBANG_PYTHON_RE = re.compile(r"^#!.*python")
SHEBANG_NODE_RE = re.compile(r"^#!.*node")
SHEBANG_SHELL_RE = re.compile(r"^#!.*\b(?:sh|bash|zsh)\b")

CODING_COOKIE_RE = re.compile(r"^#\s*(?:-\*-\s*.*?\bcoding[:=]\s*[-\w.]+.*?-\*-|coding[:=]\s*[-\w.]+)")
SWIFT_TOOLS_RE = re.compile(r"^//\s*swift-tools-version\s*:")

LICENSE_RE = re.compile(
    r"SPDX-License-Identifier|Copyright|MIT License|Apache License|Licensed under|BaoCut Community License",
    re.IGNORECASE,
)

DIRECTIVE_RE = re.compile(
    r"(?:"
    r"eslint-disable"
    r"|@ts-ignore\b"
    r"|@ts-expect-error\b"
    r"|prettier-ignore\b"
    r"|swiftlint:"
    r"|swiftformat:"
    r"|noinspection\b"
    r"|\bnoqa\b"
    r"|\btype:\s*ignore\b"
    r"|\bpyright:"
    r"|\bruff:"
    r"|\bpragma:\s*no\s+cover\b"
    r"|\bclippy::"
    r"|\bSAFETY:"
    r"|\bshellcheck\s+disable\b"
    r"|\bnotary-policy:"
    r"|\bswiftui-perf:"
    r"|(?:^|[\s/#*])#?cgo\b"
    r"|(?:^|[\s/])go:build\b"
    r"|(?:^|[\s/])\+build\b"
    r"|(?:^|[\s/])nolint\b"
    r"|\bistanbul\s+ignore\b"
    r"|\bv8\s+ignore\b"
    r"|\bc8\s+ignore\b"
    r")"
)

EXCUSE_HINT_RE = re.compile(
    r"(?:\b(?:TODO|FIXME|HACK|XXX|WORKAROUND|KLUDGE|for\s+now)\b|凑合|权宜|先这样)",
    re.IGNORECASE,
)
DECISION_HINT_RE = re.compile(
    r"(?:\b(?:because|why|invariant|must\s+not|cannot|deadlock|race)\b|-10814|3,000|因为|不能|否则|必须)",
    re.IGNORECASE,
)

JS_REGEX_KEYWORDS = frozenset({
    "return",
    "throw",
    "case",
    "delete",
    "void",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "yield",
    "await",
    "else",
    "do",
})
JS_REGEX_PUNCT = frozenset({
    "(",
    "[",
    "{",
    ",",
    ";",
    ":",
    "?",
    "=",
    "!",
    "&",
    "|",
    "^",
    "~",
    "+",
    "-",
    "*",
    "%",
    "=>",
    "COND_END",
})
JS_COND_PARENS = frozenset({"if", "while", "for", "with"})

SHELL_HEREDOC_OP_RE = re.compile(
    r"""<<(-?)[ \t]*(?:'([^'\n]+)'|"([^"\n]+)"|\\?([A-Za-z_][A-Za-z0-9_.-]*))"""
)


@dataclass(frozen=True)
class RawComment:
    start_line: int
    end_line: int
    col: int
    text: str
    inline: bool


@dataclass(frozen=True)
class AnalyzedComment:
    file: str
    line: int
    end_line: int
    inline: bool
    hint: str
    text: str


@dataclass
class ScanResult:
    files_scanned: int
    loc: int
    pure_comment_lines: int
    inline_comments: int
    exempt_by_kind: dict[str, int]
    comments_by_file: dict[str, list[AnalyzedComment]]


def normalize_prefix(prefix: str) -> str:
    cleaned = prefix.strip().replace("\\", "/")
    while cleaned.startswith("./"):
        cleaned = cleaned[2:]
    return cleaned.lstrip("/")


def is_excluded(rel_posix: str, exclude_prefixes: list[str]) -> bool:
    for raw in exclude_prefixes:
        pref = normalize_prefix(raw)
        if not pref:
            continue
        if pref.endswith("/"):
            if rel_posix.startswith(pref):
                return True
        elif rel_posix == pref or rel_posix.startswith(pref + "/"):
            return True
    return False


def has_skipped_part_or_symlink(root: Path, rel: Path) -> bool:
    parts = rel.parts
    if not parts:
        return True
    for part in parts[:-1]:
        if part in SKIP_DIRS:
            return True
    cur = root
    for part in parts:
        cur = cur / part
        if cur.is_symlink():
            return True
    return not cur.is_file()


def discover_files(root: Path, exclude_prefixes: list[str]) -> list[Path]:
    candidates: list[Path] | None = None
    if (root / ".git").exists():
        proc = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z"],
            capture_output=True,
            check=False,
        )
        if proc.returncode == 0:
            candidates = []
            for raw in proc.stdout.split(b"\x00"):
                if not raw:
                    continue
                candidates.append(Path(raw.decode("utf-8", errors="replace")))

    if candidates is None:
        candidates = []
        for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
            dp = Path(dirpath)
            dirnames[:] = sorted(
                d for d in dirnames if d not in SKIP_DIRS and not (dp / d).is_symlink()
            )
            for fn in sorted(filenames):
                full = dp / fn
                if full.is_symlink() or not full.is_file():
                    continue
                candidates.append(full.relative_to(root))

    accepted: list[Path] = []
    seen: set[str] = set()
    for rel in candidates:
        rel_posix = rel.as_posix()
        if rel_posix in seen:
            continue
        if has_skipped_part_or_symlink(root, rel):
            continue
        if is_excluded(rel_posix, exclude_prefixes):
            continue
        seen.add(rel_posix)
        accepted.append(rel)
    return sorted(accepted, key=lambda p: p.as_posix())


def read_text_if_utf8(path: Path) -> str | None:
    try:
        raw = path.read_bytes()
    except OSError:
        return None
    if b"\x00" in raw[:8192]:
        return None
    return raw.decode("utf-8", errors="replace")


def detect_language(rel: Path, first_line: str) -> str | None:
    ext = rel.suffix.lower()
    if ext in EXT_TO_LANG:
        return EXT_TO_LANG[ext]
    if ext:
        return None
    if not first_line.startswith("#!"):
        return None
    if SHEBANG_PYTHON_RE.match(first_line):
        return "python"
    if SHEBANG_NODE_RE.match(first_line):
        return "js"
    if SHEBANG_SHELL_RE.match(first_line):
        return "shell"
    return None


class LineIndex:
    def __init__(self, text: str) -> None:
        self.text = text
        self.lines = text.splitlines()
        starts = [0]
        for idx, ch in enumerate(text):
            if ch == "\n":
                starts.append(idx + 1)
        self.starts = starts

    def make_comment(self, start: int, end: int) -> RawComment:
        while end > start and self.text[end - 1] in ("\n", "\r"):
            end -= 1
        start_line = bisect.bisect_right(self.starts, start)
        col = start - self.starts[start_line - 1]
        last_idx = max(start, end - 1)
        end_line = bisect.bisect_right(self.starts, last_idx)
        end_col = end - self.starts[end_line - 1]
        before = self.lines[start_line - 1][:col].strip() if start_line <= len(self.lines) else ""
        after = self.lines[end_line - 1][end_col:].strip() if end_line <= len(self.lines) else ""
        return RawComment(
            start_line=start_line,
            end_line=end_line,
            col=col,
            text=self.text[start:end],
            inline=bool(before or after),
        )


def lex_python(text: str) -> list[RawComment]:
    lines = text.splitlines()
    out: list[RawComment] = []
    reader = io.StringIO(text).readline
    try:
        for tok in tokenize.generate_tokens(reader):
            if tok.type == tokenize.COMMENT:
                s_line, s_col = tok.start
                e_line, _ = tok.end
                before = lines[s_line - 1][:s_col].strip() if s_line <= len(lines) else ""
                out.append(
                    RawComment(
                        start_line=s_line,
                        end_line=e_line,
                        col=s_col,
                        text=tok.string.rstrip("\r\n"),
                        inline=bool(before),
                    )
                )
    except (tokenize.TokenError, IndentationError, SyntaxError):
        pass
    return out


def _scan_quoted(text: str, quote_pos: int, quote_ch: str, multiline: bool = True) -> int:
    n = len(text)
    i = quote_pos + 1
    while i < n:
        ch = text[i]
        if ch == "\\":
            i += 2
            continue
        if ch == quote_ch:
            return i + 1
        if not multiline and ch == "\n":
            return i
        i += 1
    return n


def _match_rust_raw_string(text: str, pos: int) -> int | None:
    n = len(text)
    if pos > 0 and (text[pos - 1].isalnum() or text[pos - 1] == "_"):
        return None
    prefix_len = 0
    for pref in ("br", "rb", "cr", "rc", "r"):
        if text.startswith(pref, pos):
            prefix_len = len(pref)
            break
    if prefix_len == 0:
        return None
    j = pos + prefix_len
    hashes = 0
    while j < n and text[j] == "#":
        hashes += 1
        j += 1
    if j >= n or text[j] != '"':
        return None
    closing = '"' + ("#" * hashes)
    end_idx = text.find(closing, j + 1)
    return n if end_idx == -1 else end_idx + len(closing)


def _scan_rust_char_or_lifetime(text: str, pos: int) -> int:
    n = len(text)
    if pos + 1 >= n:
        return pos + 1
    nxt = text[pos + 1]
    if nxt == "'":
        return pos + 2
    if nxt == "\\":
        j = pos + 2
        if j < n and text[j] != "\n":
            j += 1
        while j < n and text[j] not in ("'", "\n"):
            j += 1
        return j + 1 if (j < n and text[j] == "'") else pos + 1
    if nxt not in ("\n", "\r") and pos + 2 < n and text[pos + 2] == "'":
        return pos + 3
    return pos + 1


def lex_rust(text: str) -> list[RawComment]:
    idx = LineIndex(text)
    out: list[RawComment] = []
    n = len(text)
    i = 0
    if text.startswith("#!"):
        nl = text.find("\n")
        end = n if nl == -1 else nl
        out.append(idx.make_comment(0, end))
        i = end

    while i < n:
        if text.startswith("//", i):
            nl = text.find("\n", i + 2)
            end = n if nl == -1 else nl
            out.append(idx.make_comment(i, end))
            i = end
            continue
        if text.startswith("/*", i):
            start = i
            i += 2
            depth = 1
            while i < n and depth > 0:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            out.append(idx.make_comment(start, i))
            continue

        raw_end = _match_rust_raw_string(text, i)
        if raw_end is not None:
            i = raw_end
            continue

        if (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) and text[i : i + 2] in ('b"', 'c"'):
            i = _scan_quoted(text, i + 1, '"', multiline=True)
            continue

        if (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) and text.startswith("b'", i):
            i = _scan_quoted(text, i + 1, "'", multiline=False)
            continue

        ch = text[i]
        if ch == '"':
            i = _scan_quoted(text, i, '"', multiline=True)
            continue
        if ch == "'":
            i = _scan_rust_char_or_lifetime(text, i)
            continue
        i += 1
    return out


def lex_swift(text: str) -> list[RawComment]:
    idx = LineIndex(text)
    out: list[RawComment] = []
    n = len(text)
    i = 0
    interp_stack: list[tuple[bool, int, int]] = []

    if text.startswith("#!"):
        nl = text.find("\n")
        end = n if nl == -1 else nl
        out.append(idx.make_comment(0, end))
        i = end

    def continue_swift_string(multiline: bool, hashes: int) -> None:
        nonlocal i
        closing = ('"""' if multiline else '"') + ("#" * hashes)
        escape = "\\" + ("#" * hashes)
        while i < n:
            if text.startswith(closing, i):
                i += len(closing)
                return
            if not multiline and text[i] == "\n":
                return
            if text.startswith(escape, i):
                after_esc = i + len(escape)
                if after_esc < n and text[after_esc] == "(":
                    interp_stack.append((multiline, hashes, 1))
                    i = after_esc + 1
                    return
                i = min(n, after_esc + 1)
                continue
            i += 1

    while i < n:
        if text.startswith("//", i):
            nl = text.find("\n", i + 2)
            end = n if nl == -1 else nl
            out.append(idx.make_comment(i, end))
            i = end
            continue
        if text.startswith("/*", i):
            start = i
            i += 2
            depth = 1
            while i < n and depth > 0:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            out.append(idx.make_comment(start, i))
            continue

        if text[i] == "#":
            j = i
            while j < n and text[j] == "#":
                j += 1
            hashes = j - i
            if j < n and text[j] == '"':
                if text.startswith('"""', j):
                    i = j + 3
                    continue_swift_string(True, hashes)
                else:
                    i = j + 1
                    continue_swift_string(False, hashes)
                continue
            i = j
            continue

        if text[i] == '"':
            if text.startswith('"""', i):
                i += 3
                continue_swift_string(True, 0)
            else:
                i += 1
                continue_swift_string(False, 0)
            continue

        if interp_stack:
            if text[i] == "(":
                ml, hs, depth = interp_stack[-1]
                interp_stack[-1] = (ml, hs, depth + 1)
                i += 1
                continue
            if text[i] == ")":
                ml, hs, depth = interp_stack[-1]
                if depth <= 1:
                    interp_stack.pop()
                    i += 1
                    continue_swift_string(ml, hs)
                else:
                    interp_stack[-1] = (ml, hs, depth - 1)
                    i += 1
                continue

        i += 1
    return out


def _scan_js_regex(text: str, slash_pos: int) -> int | None:
    n = len(text)
    j = slash_pos + 1
    if j >= n or text[j] in ("\n", "\r", "*", "/"):
        return None
    in_class = False
    while j < n:
        ch = text[j]
        if ch in ("\n", "\r"):
            return None
        if ch == "\\":
            if j + 1 >= n or text[j + 1] in ("\n", "\r"):
                return None
            j += 2
            continue
        if ch == "[":
            in_class = True
            j += 1
            continue
        if ch == "]" and in_class:
            in_class = False
            j += 1
            continue
        if ch == "/" and not in_class:
            j += 1
            while j < n and text[j].isalpha():
                j += 1
            return j
        j += 1
    return None


def _js_allows_regex(prev_tok: str | None, prev_before_ident: str | None) -> bool:
    if prev_tok is None:
        return True
    if prev_tok in JS_REGEX_PUNCT:
        return True
    if prev_tok in JS_REGEX_KEYWORDS and prev_before_ident != ".":
        return True
    return False


def lex_js(text: str) -> list[RawComment]:
    idx = LineIndex(text)
    out: list[RawComment] = []
    n = len(text)
    i = 0
    if text.startswith("#!"):
        nl = text.find("\n")
        end = n if nl == -1 else nl
        out.append(idx.make_comment(0, end))
        i = end

    prev_tok: str | None = None
    prev_before_ident: str | None = None
    template_brace_depths: list[int] = []
    paren_cond_stack: list[bool] = []

    def continue_template() -> None:
        nonlocal i, prev_tok, prev_before_ident
        while i < n:
            ch = text[i]
            if ch == "\\":
                i += 2
                continue
            if ch == "`":
                i += 1
                prev_before_ident = prev_tok
                prev_tok = "`"
                return
            if text.startswith("${", i):
                template_brace_depths.append(1)
                i += 2
                prev_before_ident = None
                prev_tok = "{"
                return
            i += 1

    while i < n:
        ch = text[i]
        if ch.isspace():
            i += 1
            continue

        if text.startswith("//", i):
            nl = text.find("\n", i + 2)
            end = n if nl == -1 else nl
            out.append(idx.make_comment(i, end))
            i = end
            continue

        if text.startswith("/*", i):
            end_pos = text.find("*/", i + 2)
            end = n if end_pos == -1 else end_pos + 2
            out.append(idx.make_comment(i, end))
            i = end
            continue

        if ch in ('"', "'"):
            i = _scan_quoted(text, i, ch, multiline=False)
            prev_before_ident = prev_tok
            prev_tok = ch
            continue

        if ch == "`":
            i += 1
            continue_template()
            continue

        if ch == "/" and _js_allows_regex(prev_tok, prev_before_ident):
            regex_end = _scan_js_regex(text, i)
            if regex_end is not None:
                i = regex_end
                prev_before_ident = prev_tok
                prev_tok = "REGEX"
                continue

        if ch == "(":
            paren_cond_stack.append(
                bool(prev_tok in JS_COND_PARENS and prev_before_ident != ".")
            )
            prev_before_ident = prev_tok
            prev_tok = "("
            i += 1
            continue

        if ch == ")":
            is_cond = paren_cond_stack.pop() if paren_cond_stack else False
            prev_before_ident = prev_tok
            prev_tok = "COND_END" if is_cond else ")"
            i += 1
            continue

        if ch == "{" and template_brace_depths:
            template_brace_depths[-1] += 1
            prev_before_ident = prev_tok
            prev_tok = "{"
            i += 1
            continue

        if ch == "}" and template_brace_depths:
            template_brace_depths[-1] -= 1
            i += 1
            if template_brace_depths[-1] == 0:
                template_brace_depths.pop()
                continue_template()
            else:
                prev_before_ident = prev_tok
                prev_tok = "}"
            continue

        if ch.isalpha() or ch in ("_", "$"):
            j = i + 1
            while j < n and (text[j].isalnum() or text[j] in ("_", "$")):
                j += 1
            prev_before_ident = prev_tok
            prev_tok = text[i:j]
            i = j
            continue

        if ch.isdigit():
            j = i + 1
            while j < n and (text[j].isalnum() or text[j] in ("_", ".")):
                j += 1
            prev_before_ident = prev_tok
            prev_tok = "NUM"
            i = j
            continue

        if text.startswith("=>", i) or text.startswith("++", i) or text.startswith("--", i):
            prev_before_ident = prev_tok
            prev_tok = text[i : i + 2]
            i += 2
            continue

        prev_before_ident = prev_tok
        prev_tok = ch
        i += 1

    return out


def lex_c_go(text: str) -> list[RawComment]:
    idx = LineIndex(text)
    out: list[RawComment] = []
    n = len(text)
    i = 0
    while i < n:
        if text.startswith("//", i):
            nl = text.find("\n", i + 2)
            end = n if nl == -1 else nl
            out.append(idx.make_comment(i, end))
            i = end
            continue
        if text.startswith("/*", i):
            end_pos = text.find("*/", i + 2)
            end = n if end_pos == -1 else end_pos + 2
            out.append(idx.make_comment(i, end))
            i = end
            continue
        if text[i] == "R" and i + 1 < n and text[i + 1] == '"' and (
            i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")
        ):
            paren = text.find("(", i + 2)
            if paren != -1 and (paren - (i + 2)) <= 16 and "\n" not in text[i + 2 : paren]:
                delim = ")" + text[i + 2 : paren] + '"'
                end_pos = text.find(delim, paren + 1)
                i = n if end_pos == -1 else end_pos + len(delim)
                continue
        ch = text[i]
        if ch == "`":
            end_pos = text.find("`", i + 1)
            i = n if end_pos == -1 else end_pos + 1
            continue
        if ch == '"':
            i = _scan_quoted(text, i, '"', multiline=False)
            continue
        if ch == "'":
            if i > 0 and text[i - 1].isalnum():
                i += 1
                continue
            i = _scan_quoted(text, i, "'", multiline=False)
            continue
        i += 1
    return out


def _skip_shell_param_expansion(text: str, pos: int) -> int:
    n = len(text)
    depth = 1
    i = pos
    while i < n and depth > 0:
        ch = text[i]
        if ch == "\\":
            i += 2
            continue
        if ch == "'":
            end_q = text.find("'", i + 1)
            i = n if end_q == -1 else end_q + 1
            continue
        if ch == '"':
            i = _skip_shell_double_quoted(text, i + 1)
            continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        i += 1
    return i


def _skip_shell_double_quoted(text: str, pos: int) -> int:
    n = len(text)
    i = pos
    while i < n:
        ch = text[i]
        if ch == "\\":
            i += 2
            continue
        if ch == '"':
            return i + 1
        if text.startswith("${", i):
            i = _skip_shell_param_expansion(text, i + 2)
            continue
        i += 1
    return n


def _heredoc_closes_later(text: str, search_from: int, delim: str, strip_tabs: bool) -> bool:
    pos = search_from
    n = len(text)
    while pos < n:
        nl = text.find("\n", pos)
        end = n if nl == -1 else nl
        line = text[pos:end].rstrip("\r")
        candidate = line.lstrip("\t") if strip_tabs else line
        if candidate == delim:
            return True
        if nl == -1:
            break
        pos = nl + 1
    return False


def lex_shell(text: str) -> list[RawComment]:
    idx = LineIndex(text)
    out: list[RawComment] = []
    n = len(text)
    i = 0
    line_start = 0
    pending_heredocs: list[tuple[str, bool]] = []

    while i < n:
        ch = text[i]
        if ch == "\n":
            i += 1
            line_start = i
            while pending_heredocs and i < n:
                delim, strip_tabs = pending_heredocs.pop(0)
                while i < n:
                    nl = text.find("\n", i)
                    end = n if nl == -1 else nl
                    raw_line = text[i:end].rstrip("\r")
                    candidate = raw_line.lstrip("\t") if strip_tabs else raw_line
                    i = n if nl == -1 else nl + 1
                    line_start = i
                    if candidate == delim:
                        break
            continue

        if ch == "\\":
            if i + 1 < n and text[i + 1] == "\n":
                i += 2
                line_start = i
            else:
                i += 2
            continue

        if ch == "'":
            end_q = text.find("'", i + 1)
            i = n if end_q == -1 else end_q + 1
            continue

        if ch == '"':
            i = _skip_shell_double_quoted(text, i + 1)
            continue

        if ch == "$":
            if i + 1 < n:
                nxt = text[i + 1]
                if nxt == "{":
                    i = _skip_shell_param_expansion(text, i + 2)
                    continue
                if nxt == "'":
                    i = _scan_quoted(text, i + 1, "'", multiline=True)
                    continue
                if nxt in ("#", "?", "$", "!", "*", "@", "-") or nxt.isdigit():
                    i += 2
                    continue
            i += 1
            continue

        if text.startswith("<<<", i):
            i += 3
            continue

        if text.startswith("<<", i):
            m = SHELL_HEREDOC_OP_RE.match(text, i)
            if m:
                strip_tabs = m.group(1) == "-"
                delim = m.group(2) or m.group(3) or m.group(4) or ""
                nl = text.find("\n", m.end())
                if delim and nl != -1 and _heredoc_closes_later(text, nl + 1, delim, strip_tabs):
                    pending_heredocs.append((delim, strip_tabs))
                    i = m.end()
                    continue
            i += 2
            continue

        if ch == "#":
            if i == line_start or text[i - 1] in (" ", "\t", ";", "&", "|", "("):
                nl = text.find("\n", i)
                end = n if nl == -1 else nl
                out.append(idx.make_comment(i, end))
                i = end
                continue

        i += 1

    return out


LEXERS = {
    "python": lex_python,
    "rust": lex_rust,
    "swift": lex_swift,
    "js": lex_js,
    "c_go": lex_c_go,
    "shell": lex_shell,
}


def is_doc_comment(lang: str, text: str) -> bool:
    if lang not in ("rust", "swift", "js", "c_go"):
        return False
    if text.startswith("///") and not text.startswith("////"):
        return True
    if text.startswith("//!"):
        return True
    if text.startswith("/**") and len(text) > 4 and text[3] != "*":
        return True
    return False


def compute_license_block_indices(raw_comments: list[RawComment]) -> set[int]:
    pure_indices = [idx for idx, c in enumerate(raw_comments) if not c.inline]
    exempt_indices: set[int] = set()
    pos = 0
    while pos < len(pure_indices):
        group = [pure_indices[pos]]
        pos += 1
        while pos < len(pure_indices):
            prev_c = raw_comments[group[-1]]
            curr_c = raw_comments[pure_indices[pos]]
            if curr_c.start_line <= prev_c.end_line + 1:
                group.append(pure_indices[pos])
                pos += 1
            else:
                break
        if raw_comments[group[0]].start_line <= 30 and any(
            raw_comments[g].start_line <= 30 and LICENSE_RE.search(raw_comments[g].text)
            for g in group
        ):
            for g in group:
                if raw_comments[g].start_line <= 30:
                    exempt_indices.add(g)
    return exempt_indices


def classify_exemption(
    lang: str,
    comment: RawComment,
    in_license_block: bool,
    include_doc_comments: bool,
) -> str | None:
    if comment.start_line == 1 and comment.text.startswith("#!"):
        return "shebang"
    if comment.start_line in (1, 2) and not comment.inline and CODING_COOKIE_RE.match(comment.text):
        return "shebang"
    if comment.start_line == 1 and SWIFT_TOOLS_RE.match(comment.text):
        return "shebang"
    if in_license_block or (comment.start_line <= 30 and LICENSE_RE.search(comment.text)):
        return "license"
    if not include_doc_comments and is_doc_comment(lang, comment.text):
        return "doc"
    if DIRECTIVE_RE.search(comment.text):
        return "directive"
    return None


def classify_hint(text: str) -> str:
    if EXCUSE_HINT_RE.search(text):
        return "excuse"
    if DECISION_HINT_RE.search(text):
        return "decision"
    return "tautology_or_stale"


def snippet_of(text: str, max_len: int = 120) -> str:
    first = text.splitlines()[0].strip() if text else ""
    if len(first) > max_len:
        return first[: max_len - 3] + "..."
    return first


def analyze_file(
    rel_posix: str,
    lang: str,
    text: str,
    include_doc_comments: bool,
) -> tuple[list[AnalyzedComment], dict[str, int], int]:
    raw_comments = LEXERS[lang](text)
    license_indices = compute_license_block_indices(raw_comments)
    ordinary: list[AnalyzedComment] = []
    exempt_counts: dict[str, int] = {}
    pure_lines: set[int] = set()

    for idx, rc in enumerate(raw_comments):
        exempt_kind = classify_exemption(
            lang,
            rc,
            in_license_block=(idx in license_indices),
            include_doc_comments=include_doc_comments,
        )
        if exempt_kind is not None:
            exempt_counts[exempt_kind] = exempt_counts.get(exempt_kind, 0) + 1
            continue
        ordinary.append(
            AnalyzedComment(
                file=rel_posix,
                line=rc.start_line,
                end_line=rc.end_line,
                inline=rc.inline,
                hint=classify_hint(rc.text),
                text=rc.text,
            )
        )
        if not rc.inline:
            for ln in range(rc.start_line, rc.end_line + 1):
                pure_lines.add(ln)

    return ordinary, exempt_counts, len(pure_lines)


def scan_workspace(
    root: Path,
    exclude_prefixes: list[str],
    include_doc_comments: bool = False,
) -> ScanResult:
    files_scanned = 0
    total_loc = 0
    pure_comment_lines = 0
    inline_comments = 0
    exempt_by_kind: dict[str, int] = {}
    comments_by_file: dict[str, list[AnalyzedComment]] = {}

    for rel in discover_files(root, exclude_prefixes):
        text = read_text_if_utf8(root / rel)
        if text is None:
            continue
        lines = text.splitlines()
        first_line = lines[0] if lines else ""
        lang = detect_language(rel, first_line)
        if lang is None:
            continue

        files_scanned += 1
        total_loc += len(lines)
        rel_posix = rel.as_posix()
        ordinary, file_exempts, file_pure = analyze_file(
            rel_posix,
            lang,
            text,
            include_doc_comments=include_doc_comments,
        )
        comments_by_file[rel_posix] = ordinary
        pure_comment_lines += file_pure
        inline_comments += sum(1 for c in ordinary if c.inline)
        for kind, cnt in file_exempts.items():
            exempt_by_kind[kind] = exempt_by_kind.get(kind, 0) + cnt

    return ScanResult(
        files_scanned=files_scanned,
        loc=total_loc,
        pure_comment_lines=pure_comment_lines,
        inline_comments=inline_comments,
        exempt_by_kind=exempt_by_kind,
        comments_by_file=comments_by_file,
    )


@dataclass(frozen=True)
class RatchetConfig:
    version: int
    max_total: int
    exclude: list[str]
    files: dict[str, int]


def _is_nonneg_int(val: object) -> bool:
    return isinstance(val, int) and not isinstance(val, bool) and val >= 0


def load_config(config_path: Path) -> RatchetConfig:
    raw = json.loads(config_path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise ValueError(f"{config_path}: root must be a JSON object")
    if raw.get("version") != 1:
        raise ValueError(f"{config_path}: unsupported version {raw.get('version')!r} (expected 1)")
    max_total = raw.get("max_total")
    if not isinstance(max_total, int) or isinstance(max_total, bool) or max_total < 0:
        raise ValueError(f"{config_path}: max_total must be a non-negative integer")
    exclude = raw.get("exclude", [])
    if not isinstance(exclude, list) or not all(isinstance(x, str) for x in exclude):
        raise ValueError(f"{config_path}: exclude must be a list of strings")
    files_raw = raw.get("files", {})
    if not isinstance(files_raw, dict):
        raise ValueError(f"{config_path}: files must map relative paths to non-negative integers")
    parsed_files: dict[str, int] = {}
    for key, val in files_raw.items():
        if not isinstance(key, str) or not _is_nonneg_int(val):
            raise ValueError(f"{config_path}: files must map relative paths to non-negative integers")
        parsed_files[key] = int(val)
    return RatchetConfig(
        version=1,
        max_total=max_total,
        exclude=[str(x) for x in exclude],
        files=parsed_files,
    )


def write_config(
    config_path: Path,
    max_total: int,
    exclude: list[str],
    files: dict[str, int],
) -> None:
    payload = {
        "version": 1,
        "max_total": max_total,
        "exclude": sorted({normalize_prefix(x) for x in exclude if normalize_prefix(x)}),
        "files": {k: files[k] for k in sorted(files) if files[k] > 0},
    }
    config_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def cmd_check(args: argparse.Namespace) -> int:
    root = Path(args.root).expanduser().resolve()
    config_path = root / CONFIG_NAME
    if args.zero:
        exclude_list: list[str] = []
        if config_path.is_file():
            try:
                cfg = load_config(config_path)
                exclude_list = list(cfg.exclude)
            except (OSError, ValueError, json.JSONDecodeError) as exc:
                print(f"error: {exc}", file=sys.stderr)
                return 1
        max_total = 0
        files_ceiling: dict[str, int] = {}
    else:
        if not config_path.is_file():
            print(
                f"error: missing {config_path} (run 'check_comments.py tighten {args.root} --init' or pass --zero)",
                file=sys.stderr,
            )
            return 1
        try:
            cfg = load_config(config_path)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
        max_total = cfg.max_total
        exclude_list = list(cfg.exclude)
        files_ceiling = dict(cfg.files)

    scan = scan_workspace(root, exclude_list, include_doc_comments=args.include_doc_comments)
    violations = 0
    total_actual = 0

    for rel_posix in sorted(scan.comments_by_file):
        ordinary = scan.comments_by_file[rel_posix]
        actual = len(ordinary)
        total_actual += actual
        ceiling = files_ceiling.get(rel_posix, 0)
        if actual > ceiling:
            for c in ordinary:
                print(
                    f"{rel_posix}:{c.line}: unexpected comment (ceiling {ceiling}, actual {actual}): "
                    f"{snippet_of(c.text)} | Fix: {ACTIONABLE_FIX}"
                )
            violations += 1

    if total_actual > max_total:
        print(
            f"{CONFIG_NAME}:1: total ordinary comments (ceiling {max_total}, actual {total_actual}) exceeded max_total"
        )
        violations += 1

    return 1 if violations else 0


def cmd_tighten(args: argparse.Namespace) -> int:
    root = Path(args.root).expanduser().resolve()
    config_path = root / CONFIG_NAME
    existing: RatchetConfig | None = None

    if config_path.is_file():
        try:
            existing = load_config(config_path)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
    elif not args.init:
        print(
            f"error: missing {config_path} (pass --init to create initial baseline)",
            file=sys.stderr,
        )
        return 1

    merged_exclude: list[str] = []
    if existing is not None:
        merged_exclude.extend(existing.exclude)
    if args.exclude:
        merged_exclude.extend(args.exclude)
    effective_exclude = sorted({normalize_prefix(x) for x in merged_exclude if normalize_prefix(x)})

    scan = scan_workspace(root, effective_exclude, include_doc_comments=args.include_doc_comments)
    nonzero_files = {
        rel: len(items) for rel, items in scan.comments_by_file.items() if len(items) > 0
    }
    current_total = sum(nonzero_files.values())

    if not args.init and existing is not None:
        old_max_total = existing.max_total
        old_files = existing.files
        regressions: list[str] = []
        for rel in sorted(scan.comments_by_file):
            items = scan.comments_by_file[rel]
            actual = len(items)
            ceiling = old_files.get(rel, 0)
            if actual > ceiling:
                first_line = items[0].line
                regressions.append(
                    f"{rel}:{first_line}: unexpected comment regression (ceiling {ceiling}, actual {actual}): {snippet_of(items[0].text)}"
                )
        if current_total > old_max_total:
            regressions.append(
                f"{CONFIG_NAME}:1: total comments increased above max_total (ceiling {old_max_total}, actual {current_total})"
            )
        if regressions:
            for msg in regressions:
                print(msg)
            return 1

    write_config(config_path, current_total, effective_exclude, nonzero_files)
    return 0


def cmd_report(args: argparse.Namespace) -> int:
    root = Path(args.root).expanduser().resolve()
    config_path = root / CONFIG_NAME
    exclude_list: list[str] = []
    if config_path.is_file():
        try:
            cfg = load_config(config_path)
            exclude_list.extend(cfg.exclude)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
    if args.exclude:
        exclude_list.extend(args.exclude)

    scan = scan_workspace(root, exclude_list, include_doc_comments=args.include_doc_comments)
    all_comments: list[AnalyzedComment] = []
    for rel in sorted(scan.comments_by_file):
        all_comments.extend(scan.comments_by_file[rel])

    shown = all_comments[: args.limit] if args.limit is not None else all_comments
    summary = {
        "files_scanned": scan.files_scanned,
        "loc": scan.loc,
        "ordinary_comments": len(all_comments),
        "pure_comment_lines": scan.pure_comment_lines,
        "inline_comments": scan.inline_comments,
        "exempt_by_kind": dict(sorted(scan.exempt_by_kind.items())),
    }

    if args.json:
        payload = {
            "root": str(root),
            "summary": summary,
            "comments": [
                {
                    "file": c.file,
                    "line": c.line,
                    "inline": c.inline,
                    "hint": c.hint,
                    "text": c.text,
                }
                for c in shown
            ],
        }
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        for c in shown:
            tag = f"{c.hint}, inline" if c.inline else c.hint
            print(f"{c.file}:{c.line}: [{tag}] {snippet_of(c.text)}")
        print(
            f"Summary: files_scanned={summary['files_scanned']} loc={summary['loc']} "
            f"ordinary_comments={summary['ordinary_comments']} "
            f"pure_comment_lines={summary['pure_comment_lines']} "
            f"inline_comments={summary['inline_comments']} "
            f"exempt_by_kind={json.dumps(summary['exempt_by_kind'], ensure_ascii=False)}"
        )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Multi-language comment ratchet gate.")
    sub = parser.add_subparsers(dest="command", required=True)

    p_check = sub.add_parser("check", help="Verify workspace comment counts against .comment-ratchet.json")
    p_check.add_argument("root", help="Workspace root directory")
    p_check.add_argument("--zero", action="store_true", help="Enforce ceiling 0 everywhere without requiring config")
    p_check.add_argument("--include-doc-comments", action="store_true", help="Treat doc comments as ordinary comments")
    p_check.set_defaults(func=cmd_check)

    p_tighten = sub.add_parser("tighten", help="Monotonically lower comment ceilings in .comment-ratchet.json")
    p_tighten.add_argument("root", help="Workspace root directory")
    p_tighten.add_argument("--init", action="store_true", help="Initialize or reset .comment-ratchet.json to current counts")
    p_tighten.add_argument("--exclude", action="append", default=[], metavar="PREFIX", help="Exclude relative path prefix")
    p_tighten.add_argument("--include-doc-comments", action="store_true", help="Treat doc comments as ordinary comments")
    p_tighten.set_defaults(func=cmd_tighten)

    p_report = sub.add_parser("report", help="Report ordinary comments with classification hints and summary")
    p_report.add_argument("root", help="Workspace root directory")
    p_report.add_argument("--json", action="store_true", help="Emit JSON report")
    p_report.add_argument("--limit", type=int, default=None, metavar="N", help="Limit displayed comments to first N")
    p_report.add_argument("--exclude", action="append", default=[], metavar="PREFIX", help="Exclude relative path prefix")
    p_report.add_argument("--include-doc-comments", action="store_true", help="Treat doc comments as ordinary comments")
    p_report.set_defaults(func=cmd_report)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
