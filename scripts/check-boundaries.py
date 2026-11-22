import base64
from collections.abc import Iterable
import hashlib
import os
from pathlib import Path
import plistlib
import re
import subprocess
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent.parent

_CLEAN_ROOM_COMMON_RULES: tuple[tuple[str, re.Pattern[str]], ...] = (
    ('.scratch/native-redesign', re.compile(r'\.scratch/native-redesign', re.IGNORECASE)),
    ('.scratch/firmware-check', re.compile(r'\.scratch/firmware-check', re.IGNORECASE)),
    ('.scratch/input-study', re.compile(r'\.scratch/input-study(?:/|\b)', re.IGNORECASE)),
    ('~/agent-tasks', re.compile(r'~/agent-tasks', re.IGNORECASE)),
    ('/Applications/input.app', re.compile(r'/Applications/input\.app', re.IGNORECASE)),
    ('vendor bundle/asar path', re.compile(r'app\.asar|dist-electron', re.IGNORECASE)),
    ('vendor hashed chunk', re.compile(r'\b(?:device_config_data|Main|smart_action|PermissionsGuide)-[A-Za-z0-9_-]{6,}\b')),
    ('REA evidence ID', re.compile(r'\bev_[0-9a-f]{16,}\b', re.IGNORECASE)),
    ('vendor bundle offset', re.compile(r'\b[MKPRUW]:\d')),
    ('firmware disassembly address', re.compile(r'\b0x(?:420[0-9a-fA-F]{5}|3c1[0-9a-fA-F]{5}|012598|012f24)\b', re.IGNORECASE)),
    ('firmware internal source file', re.compile(r'\b(?:wl_[A-Za-z0-9_]+|api_lighting)\.cpp\b', re.IGNORECASE)),
    ('host build CL number', re.compile(r'\bCL[: ]+\d{6,}\b')),
    ('non-synthetic serial port', re.compile(r'/dev/cu\.usbmodem\d+', re.IGNORECASE)),
    ('device flash backup id', re.compile(r'firmware-\d{12,}', re.IGNORECASE)),
    ('internal triage path', re.compile(r'input-app-lean-triage', re.IGNORECASE)),
    ('personal notary profile', re.compile(r'\bxw-notary\b', re.IGNORECASE)),
    (
        'custom notary profile',
        re.compile(
            r'(?<![\\a-z0-9])(?!xw-notary(?![a-z0-9-])|macos-notary(?![a-z0-9-]))[a-z0-9][a-z0-9-]*-notary(?![a-z0-9-])',
            re.IGNORECASE,
        ),
    ),
)
_MAC_RE = re.compile(
    r'(?<![0-9A-Za-z])(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}(?![0-9A-Za-z])'
)
_MAC_OCTET_BEFORE_RE = re.compile(r'(?:^|[^0-9A-Za-z])[0-9A-Fa-f]{2}[:-]$')
_MAC_OCTET_AFTER_RE = re.compile(r'^[:-][0-9A-Fa-f]{2}(?:$|[^0-9A-Za-z])')
_ALLOWED_SYNTHETIC_MACS = frozenset({
    '000000000000',
    'aabbccddeeff',
    'aabbcc112233',
    '112233445566',
    '02005e100001',
    'ffffffffffff',
})
_CLEAN_ROOM_HASH_SALT = b'mi-cleanroom-v1:'
_HEX40_RE = re.compile(r'(?<![0-9a-fA-F])(?:0[xX])?([0-9a-fA-F]{40})(?![0-9a-fA-F])')
_HEX64_RE = re.compile(r'(?<![0-9a-fA-F])(?:0[xX])?([0-9a-fA-F]{64})(?![0-9a-fA-F])')
_BANNED_SHA1_TOKEN_DIGESTS = frozenset({
    'fc58f7710e256fbdf9e20f619221de2cbb994de53a058c3ea96043f0b2f1db15',
})
_BANNED_SHA256_PREFIX16_DIGESTS = frozenset({
    '19e5a8098322a291fcdd5c83c7ae400718f500ded4fcc808428eb512e4151005',
    '81e4924894f8c342c58d363668162dfe70572133f91fc450249eec9016c4adc4',
    '9da100e9e39093fca628833528d9c20982fcfee4dd0f0f5811bdf2b72ea20fc4',
    '396b1fcad768472583ad4dc0524572899bae3d38bb0da0deb8ef50f0877998d7',
    '56da296e6c83fbdccce3be9c5019c23c43190f812177157484532ef0d05b31ea',
    '599637330c68e98a1552fbe0b67da97428374ffdd7c844ad3a9dbbe45561d121',
    '35b6e3fee45a34959d5bb03a3384a27b9b9d403c40f751a57779e28aa85abb58',
    '38b93dcbd6914198257013b674a03b8d7fa02376327acfe50ecf064187d287cb',
    '8ab2139f428ae9ce69a79a6e1faab80843b715bf5442142386a9cabaffa2c712',
})
_MD_TIMESTAMPED_QA_RUN = re.compile(r'\.scratch/qa-runs/\d{10,}', re.IGNORECASE)
_CHAR_ESCAPE_RE = re.compile(r"\\(?:['\"\\nrt0]|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,24}\})'")
_UNIX_CONNECT_RE = re.compile(r'\bUnixStream\s*::\s*connect\s*\(')
_EXTERN_FN_RE = re.compile(r'\bextern(?:\s+(?=[A-Za-z_])|\s+)fn\s+([A-Za-z0-9_]+)\s*(?:<|\()')
_FFI_GUARD_CALL_RE = re.compile(r'\bffi_guard\s*\(')


def _is_ident_char(ch: str) -> bool:
    return ch.isalnum() or ch == '_'


def _advance_quoted_string(src: str, quote_idx: int) -> int:
    n = len(src)
    j = quote_idx + 1
    while j < n:
        if src[j] == '\\':
            j += 2
        elif src[j] == '"':
            return j + 1
        else:
            j += 1
    return n


def _advance_char_literal_from_quote(src: str, quote_idx: int) -> int | None:
    n = len(src)
    if quote_idx + 2 < n and src[quote_idx + 1] == '\\':
        m = _CHAR_ESCAPE_RE.match(src, quote_idx + 1)
        if m:
            return m.end()
    elif (
        quote_idx + 2 < n
        and src[quote_idx + 1] not in ('\\', "'", '\n', '\r')
        and src[quote_idx + 2] == "'"
    ):
        return quote_idx + 3
    return None


def _advance_rust_trivia_or_literal(src: str, idx: int) -> int | None:
    n = len(src)
    ch = src[idx]
    if ch in ' \t\r\n':
        return idx + 1
    if src.startswith('//', idx):
        end = src.find('\n', idx + 2)
        return n if end == -1 else end + 1
    if src.startswith('/*', idx):
        depth = 1
        j = idx + 2
        while j < n and depth > 0:
            if src.startswith('/*', j):
                depth += 1
                j += 2
            elif src.startswith('*/', j):
                depth -= 1
                j += 2
            else:
                j += 1
        return j
    at_ident_boundary = idx == 0 or not _is_ident_char(src[idx - 1])
    if at_ident_boundary:
        raw_prefix_len = 0
        if src[idx : idx + 2] in ('br', 'rb', 'cr', 'rc'):
            raw_prefix_len = 2
        elif ch == 'r':
            raw_prefix_len = 1
        if raw_prefix_len:
            j = idx + raw_prefix_len
            while j < n and src[j] == '#':
                j += 1
            if j < n and src[j] == '"':
                closing = '"' + '#' * (j - idx - raw_prefix_len)
                end = src.find(closing, j + 1)
                return n if end == -1 else end + len(closing)
        if ch in ('b', 'c') and idx + 1 < n and src[idx + 1] == '"':
            return _advance_quoted_string(src, idx + 1)
        if ch == 'b' and idx + 1 < n and src[idx + 1] == "'":
            return _advance_char_literal_from_quote(src, idx + 1)
    if ch == '"':
        return _advance_quoted_string(src, idx)
    if ch == "'":
        return _advance_char_literal_from_quote(src, idx)
    return None


def _blank_rust_comments_and_literals(src: str) -> str:
    n = len(src)
    out: list[str] = []
    i = 0
    while i < n:
        if src[i] in ' \t\r\n':
            out.append(src[i])
            i += 1
            continue
        skipped = _advance_rust_trivia_or_literal(src, i)
        if skipped is not None:
            out.append(re.sub(r'[^\n]', ' ', src[i:skipped]))
            i = skipped
            continue
        out.append(src[i])
        i += 1
    return ''.join(out)


def _find_matching_delimiter(src: str, open_idx: int, open_ch: str, close_ch: str) -> int:
    n = len(src)
    depth = 1
    i = open_idx + 1
    while i < n and depth > 0:
        skipped = _advance_rust_trivia_or_literal(src, i)
        if skipped is not None:
            i = skipped
            continue
        if src[i] == open_ch:
            depth += 1
        elif src[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return n - 1


def _find_matching_delimiter_backward(src: str, close_idx: int, open_ch: str, close_ch: str) -> int:
    depth = 1
    i = close_idx - 1
    while i >= 0 and depth > 0:
        if src[i] == close_ch:
            depth += 1
        elif src[i] == open_ch:
            depth -= 1
            if depth == 0:
                return i
        i -= 1
    return -1


def _is_cfg_test_attribute(attr_src: str) -> bool:
    clean = _blank_rust_comments_and_literals(attr_src).strip()
    m = re.match(r'^cfg\s*\(', clean)
    if not m:
        return False
    close_paren = _find_matching_delimiter(clean, m.end() - 1, '(', ')')
    if clean[close_paren + 1 :].strip():
        return False
    inner = list(clean[m.end() : close_paren])
    while True:
        joined = ''.join(inner)
        nm = re.search(r'\bnot\s*\(', joined)
        if not nm:
            break
        end_not = _find_matching_delimiter(joined, nm.end() - 1, '(', ')')
        for k in range(nm.start(), min(len(inner), end_not + 1)):
            inner[k] = ' '
    return bool(re.search(r'\btest\b', ''.join(inner)))


def _strip_rust_cfg_test(src: str) -> str:
    n = len(src)
    out: list[str] = []
    i = 0
    while i < n:
        skipped = _advance_rust_trivia_or_literal(src, i)
        if skipped is not None:
            out.append(src[i:skipped])
            i = skipped
            continue
        if src.startswith('#[', i):
            close_bracket = _find_matching_delimiter(src, i + 1, '[', ']')
            if _is_cfg_test_attribute(src[i + 2 : close_bracket]):
                item_start = i
                i = close_bracket + 1
                while i < n:
                    skipped = _advance_rust_trivia_or_literal(src, i)
                    if skipped is not None:
                        i = skipped
                        continue
                    if src.startswith('#[', i):
                        i = _find_matching_delimiter(src, i + 1, '[', ']') + 1
                        continue
                    break
                paren_depth = 0
                while i < n:
                    skipped = _advance_rust_trivia_or_literal(src, i)
                    if skipped is not None:
                        i = skipped
                        continue
                    if src[i] == '(':
                        paren_depth += 1
                    elif src[i] == ')':
                        paren_depth = max(0, paren_depth - 1)
                    elif paren_depth == 0 and src[i] == ';':
                        i += 1
                        break
                    elif paren_depth == 0 and src[i] == '{':
                        i = _find_matching_delimiter(src, i, '{', '}') + 1
                        break
                    i += 1
                out.append(re.sub(r'[^\n]', ' ', src[item_start:i]))
                continue
        out.append(src[i])
        i += 1
    return ''.join(out)


def find_production_qa_leaks(
    store_src: str,
    status_item_src: str,
    passive_panel_src: str,
    mi_core_cargo: str,
    ffi_tree_output: str,
) -> list[str]:
    findings: list[str] = []
    prod_store = _strip_rust_cfg_test(store_src)
    for token in (
        'current_or_synthetic_device_snapshot',
        'RecordingPassivePanelHost',
        'RecordingStatusItemHost',
        'PushToTalkController::new_simulated',
    ):
        if token in prod_store:
            findings.append(f'crates/mi-core/src/store.rs contains retired production token {token}')
    if 'NoopStatusItemHost' not in _strip_rust_cfg_test(status_item_src):
        findings.append('crates/mi-macos/src/status_item.rs must define and default to NoopStatusItemHost')
    if 'NoopPassivePanelHost' not in _strip_rust_cfg_test(passive_panel_src):
        findings.append('crates/mi-macos/src/passive_panel.rs must define NoopPassivePanelHost')
    for crate_name in ('espflash', 'serialport'):
        if re.search(rf'(?m)^\s*{crate_name}\s*=', mi_core_cargo):
            findings.append(f'crates/mi-core/Cargo.toml must not depend directly on {crate_name}')
    for forbidden in ('mi-bridge', 'qa-runner', 'synthetic-backend', 'test-util'):
        if forbidden in ffi_tree_output:
            findings.append(f'default mi-ffi dependency tree must not include {forbidden}')
    return findings


def _salted_clean_room_digest(token: str) -> str:
    return hashlib.sha256(_CLEAN_ROOM_HASH_SALT + token.lower().encode('ascii')).hexdigest()


def _iter_hardware_macs(line: str) -> Iterable[str]:
    for m in _MAC_RE.finditer(line):
        if _MAC_OCTET_BEFORE_RE.search(line[:m.start()]) or _MAC_OCTET_AFTER_RE.search(line[m.end():]):
            continue
        yield m.group(0)


def find_clean_room_leaks(
    rel_path: str,
    text: str,
    *,
    extra_sha1_digests: Iterable[str] = (),
    extra_sha256_prefix_digests: Iterable[str] = (),
) -> list[str]:
    is_md = rel_path.endswith('.md')
    sha1_digests = _BANNED_SHA1_TOKEN_DIGESTS | frozenset(extra_sha1_digests)
    sha256_prefix_digests = _BANNED_SHA256_PREFIX16_DIGESTS | frozenset(extra_sha256_prefix_digests)
    findings: list[str] = []
    for lineno, line in enumerate(text.splitlines(), 1):
        matched: list[str] = [label for label, pat in _CLEAN_ROOM_COMMON_RULES if pat.search(line)]
        if any(
            token.replace(':', '').replace('-', '').lower() not in _ALLOWED_SYNTHETIC_MACS
            for token in _iter_hardware_macs(line)
        ):
            matched.append('hardware serial MAC')
        if any(_salted_clean_room_digest(m.group(1)) in sha1_digests for m in _HEX40_RE.finditer(line)):
            matched.append('private keymap sha1')
        if any(
            _salted_clean_room_digest(m.group(1)[:16]) in sha256_prefix_digests
            for m in _HEX64_RE.finditer(line)
        ):
            matched.append('extracted/private sha256')
        if is_md and _MD_TIMESTAMPED_QA_RUN.search(line):
            matched.append('timestamped QA run dir')
        if matched:
            findings.append(f'{rel_path}:{lineno} [{", ".join(matched)}]: {line.strip()[:120]}')
    return findings


def find_forbidden_unix_connect(rel_path: str, text: str) -> list[str]:
    if (
        rel_path == 'crates/mi-bridge/src/ipc.rs'
        or rel_path.endswith('_tests.rs')
        or '/tests/' in rel_path
    ):
        return []
    prod_text = _blank_rust_comments_and_literals(_strip_rust_cfg_test(text))
    lines = prod_text.splitlines()
    findings: list[str] = []
    seen_lines: set[int] = set()
    for match in _UNIX_CONNECT_RE.finditer(prod_text):
        lineno = prod_text.count('\n', 0, match.start()) + 1
        if lineno not in seen_lines:
            seen_lines.add(lineno)
            findings.append(f'{rel_path}:{lineno}: {lines[lineno - 1].strip()}')
    return findings


def _is_c_abi_extern(prod_text: str, extern_start: int, fn_pos: int) -> bool:
    idx = extern_start + len('extern')
    abi: str | None = None
    while idx < fn_pos:
        ch = prod_text[idx]
        if ch in ' \t\r\n' or prod_text.startswith('//', idx) or prod_text.startswith('/*', idx):
            skipped = _advance_rust_trivia_or_literal(prod_text, idx)
            if skipped is not None:
                idx = skipped
                continue
        if ch == '"':
            end = _advance_quoted_string(prod_text, idx)
            abi = prod_text[idx + 1 : end - 1]
            idx = end
            continue
        idx += 1
    return abi is None or abi in ('C', 'C-unwind')


def _has_ffi_export_prefix(code_text: str, extern_start: int) -> bool:
    pos = extern_start
    has_pub = False
    has_export_attr = False
    while pos > 0:
        while pos > 0 and code_text[pos - 1].isspace():
            pos -= 1
        if pos == 0:
            break
        kw_match = re.search(r'(?:^|[^A-Za-z0-9_])(unsafe|pub)$', code_text[:pos])
        if kw_match:
            if kw_match.group(1) == 'pub':
                has_pub = True
            pos -= len(kw_match.group(1))
            continue
        if code_text[pos - 1] == ')':
            open_paren = _find_matching_delimiter_backward(code_text, pos - 1, '(', ')')
            if open_paren == -1:
                break
            pub_match = re.search(r'(?:^|[^A-Za-z0-9_])pub\s*$', code_text[:open_paren])
            if not pub_match:
                break
            pos = open_paren - len(pub_match.group(0))
            if pub_match.group(0)[0] not in 'p':
                pos += 1
            continue
        if code_text[pos - 1] == ']':
            open_bracket = _find_matching_delimiter_backward(code_text, pos - 1, '[', ']')
            if open_bracket <= 0:
                break
            hash_pos = open_bracket - 1
            while hash_pos >= 0 and code_text[hash_pos].isspace():
                hash_pos -= 1
            if hash_pos < 0 or code_text[hash_pos] != '#':
                break
            attr_body = code_text[open_bracket + 1 : pos - 1]
            if re.search(r'\b(?:no_mangle|export_name)\b', attr_body):
                has_export_attr = True
            pos = hash_pos
            continue
        break
    return has_pub or has_export_attr


def find_unguarded_ffi_entrypoints(rel_path: str, text: str) -> list[str]:
    prod_text = _strip_rust_cfg_test(text)
    code_text = _blank_rust_comments_and_literals(prod_text)
    unguarded: list[str] = []
    for match in _EXTERN_FN_RE.finditer(code_text):
        fn_name = match.group(1)
        fn_pos = code_text.find('fn', match.start(), match.end())
        if not _is_c_abi_extern(prod_text, match.start(), fn_pos):
            continue
        if not _has_ffi_export_prefix(code_text, match.start()):
            continue
        idx = match.end() - 1
        found_body = False
        while idx < len(code_text):
            ch = code_text[idx]
            if ch == '(':
                idx = _find_matching_delimiter(code_text, idx, '(', ')') + 1
                continue
            if ch == '[':
                idx = _find_matching_delimiter(code_text, idx, '[', ']') + 1
                continue
            if ch == ';':
                break
            if ch == '{':
                found_body = True
                break
            idx += 1
        if not found_body:
            continue
        close_idx = _find_matching_delimiter(code_text, idx, '{', '}')
        body = code_text[idx : close_idx + 1]
        if not _FFI_GUARD_CALL_RE.search(body):
            unguarded.append(fn_name)
    return unguarded


def _advance_swift_trivia_or_string(src: str, idx: int) -> int | None:
    n = len(src)
    ch = src[idx]
    if ch in ' \t\r\n':
        return idx + 1
    if src.startswith('//', idx):
        end = src.find('\n', idx + 2)
        return n if end == -1 else end + 1
    if src.startswith('/*', idx):
        depth = 1
        j = idx + 2
        while j < n and depth > 0:
            if src.startswith('/*', j):
                depth += 1
                j += 2
            elif src.startswith('*/', j):
                depth -= 1
                j += 2
            else:
                j += 1
        return j
    if ch == '"':
        return _advance_quoted_string(src, idx)
    return None


def _blank_swift_comments(src: str) -> str:
    n = len(src)
    out: list[str] = []
    i = 0
    while i < n:
        if src.startswith('//', i) or src.startswith('/*', i):
            skipped = _advance_swift_trivia_or_string(src, i)
            assert skipped is not None
            out.append(re.sub(r'[^\n]', ' ', src[i:skipped]))
            i = skipped
            continue
        if src[i] == '"':
            skipped = _advance_quoted_string(src, i)
            out.append(src[i:skipped])
            i = skipped
            continue
        out.append(src[i])
        i += 1
    return ''.join(out)


def _find_swift_matching(src: str, open_idx: int, open_ch: str, close_ch: str) -> int:
    n = len(src)
    depth = 1
    i = open_idx + 1
    while i < n and depth > 0:
        skipped = _advance_swift_trivia_or_string(src, i)
        if skipped is not None:
            i = skipped
            continue
        if src[i] == open_ch:
            depth += 1
        elif src[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return n - 1


def _extract_swift_modifier_chain(src: str, start_idx: int) -> str:
    n = len(src)
    pos = start_idx
    while pos < n and src[pos] in ' \t\r\n':
        pos += 1
    if pos < n and src[pos] == '{':
        pos = _find_swift_matching(src, pos, '{', '}') + 1
    chain_start = pos
    while pos < n:
        ws_pos = pos
        while ws_pos < n:
            if src[ws_pos] in ' \t\r\n':
                ws_pos += 1
            elif src.startswith('//', ws_pos) or src.startswith('/*', ws_pos):
                skipped = _advance_swift_trivia_or_string(src, ws_pos)
                ws_pos = n if skipped is None else skipped
            else:
                break
        if ws_pos >= n or src[ws_pos] != '.':
            break
        pos = ws_pos + 1
        while pos < n and _is_ident_char(src[pos]):
            pos += 1
        while pos < n and src[pos] in ' \t':
            pos += 1
        if pos < n and src[pos] == '(':
            pos = _find_swift_matching(src, pos, '(', ')') + 1
        while pos < n and src[pos] in ' \t':
            pos += 1
        if pos < n and src[pos] == '{':
            pos = _find_swift_matching(src, pos, '{', '}') + 1
    return src[chain_start:pos]


def find_kill_switch_focus_violations(ui_sources: dict[str, str], ui_theme_src: str) -> list[str]:
    violations: list[str] = []
    for rel_path, raw_src in sorted(ui_sources.items()):
        src = _blank_swift_comments(raw_src)
        for match in re.finditer(r'\bToggle\s*\(', src):
            open_paren = match.end() - 1
            close_paren = _find_swift_matching(src, open_paren, '(', ')')
            toggle_args = src[open_paren : close_paren + 1]
            if 'SetExecutionEnabled' not in toggle_args and 'SetHostExecutionEnabled' not in toggle_args:
                continue
            modifier_chain = _extract_swift_modifier_chain(src, close_paren + 1)
            if not re.search(
                r'\.toggleStyle\s*\(\s*NeutralSwitchToggleStyle\s*\([^)]*\ballowsKeyboardFocus\s*:\s*false\b',
                modifier_chain,
            ):
                lineno = src.count('\n', 0, match.start()) + 1
                violations.append(
                    f'{rel_path}:{lineno}: global key-mapping Toggle must specify NeutralSwitchToggleStyle(..., allowsKeyboardFocus: false)'
                )

    clean_theme = _blank_swift_comments(ui_theme_src)
    body_match = re.search(r'\bstruct\s+NeutralSwitchBody\b[^{]*\{', clean_theme)
    if not body_match:
        violations.append('UITheme.swift must define struct NeutralSwitchBody')
        return violations

    body_open = body_match.end() - 1
    body_close = _find_swift_matching(clean_theme, body_open, '{', '}')
    body_src = clean_theme[body_open : body_close + 1]
    if_match = re.search(r'\bif\s+(!)?\s*allowsKeyboardFocus\b[^{]*\{', body_src)
    if not if_match:
        violations.append('UITheme.swift NeutralSwitchBody must branch on allowsKeyboardFocus')
        return violations

    if_open = if_match.end() - 1
    if_close = _find_swift_matching(body_src, if_open, '{', '}')
    if_block = body_src[if_open : if_close + 1]
    else_match = re.match(r'\s*else\s*\{', body_src[if_close + 1 :])
    else_block: str | None = None
    if else_match:
        else_open = if_close + 1 + else_match.end() - 1
        else_close = _find_swift_matching(body_src, else_open, '{', '}')
        else_block = body_src[else_open : else_close + 1]

    non_focus_block = if_block if if_match.group(1) else else_block
    if non_focus_block is None:
        violations.append('UITheme.swift NeutralSwitchBody must implement a non-focus branch for allowsKeyboardFocus == false')
    else:
        for required in ('.onTapGesture', '.contentShape(Rectangle())', '.accessibilityAction'):
            if required not in non_focus_block:
                violations.append(
                    f'UITheme.swift NeutralSwitchBody non-focus branch must include {required}'
                )
        if re.search(r'\bButton\b', non_focus_block):
            violations.append(
                'UITheme.swift NeutralSwitchBody non-focus branch must not wrap switch content in Button'
            )
    return violations


def find_status_item_state_violations(swift_app_src: str) -> list[str]:
    violations: list[str] = []
    clean_app = _blank_swift_comments(swift_app_src)
    tracking_blocks: list[str] = []
    for match in re.finditer(r'\bwithObservationTracking\s*\{', clean_app):
        open_brace = match.end() - 1
        close_brace = _find_swift_matching(clean_app, open_brace, '{', '}')
        tracking_blocks.append(clean_app[open_brace : close_brace + 1])
    if not any('store.statusMenu.hostExecutionEnabled' in block for block in tracking_blocks):
        violations.append(
            'MicroInputApp.swift must observe store.statusMenu.hostExecutionEnabled inside withObservationTracking'
        )
    if 'StatusItemIcon.presentation(' not in clean_app:
        violations.append(
            'MicroInputApp.swift must derive status item icon state via StatusItemIcon.presentation('
        )
    if 'StatusItemIcon.toolTip(' not in clean_app:
        violations.append(
            'MicroInputApp.swift must derive status item tooltip via StatusItemIcon.toolTip('
        )
    return violations


_SCATTERED_ANTIGRAVITY_AX_LABELS: tuple[str, ...] = (
    'Record Audio',
    'Record voice memo',
    'Stop Recording',
    'Stop recording',
    'Start Voice mode',
    'Start Live Chat',
    'End Voice mode',
    'End Live chat',
    'Toggle Side Panel',
    'Toggle sidebar',
    'Open sidebar',
    'Search Conversations',
    'New Conversation in Project',
    'New Conversation in Workspace',
    'New Side Question',
    'Open in external browser',
    'Address bar',
    'Search for content to open',
    'Conversation Log',
    'Agent said',
    'Agent working',
    'You said',
    'Conversation turns',
    'Agent response',
    'User message',
    'Markdown view mode',
    'Stop execution',
    'Archive conversation',
    'Pin conversation',
    'Unpin conversation',
    'See all (',
    'Show less',
    'Project options',
    'Pinned Conversations',
    'Jump to bottom',
    'Scroll to bottom',
    'Scroll to Bottom',
    'Send',
    'Send message',
    'Submit',
    '跳到底部',
    '滚动到底部',
    '发送',
    '发送消息',
    '傳送訊息',
    '提交',
)


def _iter_rust_string_literals(src: str) -> Iterable[tuple[int, str]]:
    n = len(src)
    i = 0
    while i < n:
        ch = src[i]
        if src.startswith('//', i) or src.startswith('/*', i):
            skipped = _advance_rust_trivia_or_literal(src, i)
            i = n if skipped is None else skipped
            continue
        at_ident_boundary = i == 0 or not _is_ident_char(src[i - 1])
        if at_ident_boundary and ch == 'r':
            j = i + 1
            while j < n and src[j] == '#':
                j += 1
            if j < n and src[j] == '"':
                hashes = j - i - 1
                closing = '"' + '#' * hashes
                end = src.find(closing, j + 1)
                if end == -1:
                    break
                yield src.count('\n', 0, i) + 1, src[j + 1 : end]
                i = end + len(closing)
                continue
        if ch == "'":
            skipped = _advance_char_literal_from_quote(src, i)
            if skipped is not None:
                i = skipped
                continue
        if ch == '"':
            end = _advance_quoted_string(src, i)
            yield src.count('\n', 0, i) + 1, src[i + 1 : end - 1]
            i = end
            continue
        i += 1


def find_scattered_antigravity_ax_label_violations(rel_path: str, src: str) -> list[str]:
    if rel_path == 'crates/mi-macos/src/antigravity_ax_labels.rs':
        return []
    prod_src = _strip_rust_cfg_test(src)
    forbidden = frozenset(_SCATTERED_ANTIGRAVITY_AX_LABELS)
    violations: list[str] = []
    for lineno, literal in _iter_rust_string_literals(prod_src):
        if literal in forbidden:
            violations.append(
                f'{rel_path}:{lineno}: scattered Antigravity AX label {literal!r}; route through crates/mi-macos/src/antigravity_ax_labels.rs'
            )
    return violations


def run_boundary_checks(root: Path = ROOT) -> None:
    sources = list((root / 'crates').rglob('*.rs'))
    found = set()
    for path in sources:
        found.update(re.findall(r'TODO\((UNKNOWN-\d+)\)', path.read_text()))
    expected = {f'UNKNOWN-{n:02}' for n in range(1, 10)}
    doc = (root / 'docs/spec/UNKNOWNS.md').read_text()
    documented = set(re.findall(r'^\| (UNKNOWN-\d+) \|', doc, re.MULTILINE))
    assert expected <= documented, (expected - documented)
    assert found <= documented, (found - documented)
    for package, behavior in [
        ('mi-proto', 'production_exit_is_strictly_read_only'),
        ('mi-session', 'production_exit_rejects_forbidden_parameters_without_sending'),
        ('mi-session', 'real_framer_never_writes'),
        ('mi-session', 'encoded_mutation_candidates_cannot_bypass_production_exit'),
        ('mi-session', 'encoded_readonly_requests_never_forward_ambiguous_original_bytes'),
    ]:
        subprocess.run(['cargo', 'test', '--locked', '-p', package, '--lib', behavior], cwd=root, check=True)
    assert not (root / 'crates/mi-gpui').exists(), 'Retired crates/mi-gpui directory still exists'
    assert (root / 'crates/mi-core').exists(), 'Missing crates/mi-core pure Rust kernel'
    assert (root / 'crates/mi-ffi').exists(), 'Missing crates/mi-ffi C-ABI facade'
    assert (root / 'app/MicroInput').exists(), 'Missing app/MicroInput native SwiftUI shell'
    ffi_cargo = (root / 'crates/mi-ffi/Cargo.toml').read_text(encoding='utf-8')
    assert 'uniffi' not in ffi_cargo and not (root / 'crates/mi-ffi/src/bin/uniffi-bindgen.rs').exists(), (
        'crates/mi-ffi must remain a pure C-ABI staticlib without UniFFI dependencies'
    )
    swift_entry_files = sorted(p.name for p in (root / 'app/MicroInput/Sources/MicroInput').glob('*.swift'))
    assert swift_entry_files == ['MicroInputApp.swift'], (
        f'app/MicroInput/Sources/MicroInput must only contain thin host entry MicroInputApp.swift, found: {swift_entry_files}'
    )
    swift_ui_files = sorted(p.name for p in (root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'))
    expected_swift_ui_files = [
        'ClassicWorkbenchView.swift',
        'CoreBridge.swift',
        'CreatorMicroBoardView.swift',
        'FirmwareUpdate.swift',
        'InlineInspectorView.swift',
        'KeycapGlyphView.swift',
        'Localization.swift',
        'ModalsAndStatusMenuView.swift',
        'ProjectionModels.swift',
        'SoftwareUpdate.swift',
        'UITheme.swift',
        'WorkbenchRootView.swift',
    ]
    assert swift_ui_files == expected_swift_ui_files, (
        f'app/MicroInput/Sources/MicroInputUI must only contain canonical thin SwiftUI files {expected_swift_ui_files}, found: {swift_ui_files}'
    )
    for ui_file in (root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'):
        ui_text = ui_file.read_text(encoding='utf-8')
        assert not re.search(r'^\s*import\s+Sparkle\b', ui_text, re.MULTILINE), (
            f'{ui_file.relative_to(root)} must not import Sparkle; keep Sparkle confined to host target MicroInput'
        )
        assert not re.search(r'^\s*import\s+ServiceManagement\b', ui_text, re.MULTILINE), (
            f'{ui_file.relative_to(root)} must not import ServiceManagement; keep ServiceManagement confined to host target MicroInput'
        )
    for swift_file in (root / 'app/MicroInput/Sources').rglob('*.swift'):
        line_count = len(swift_file.read_text(encoding='utf-8').splitlines())
        assert line_count < 1000, f'{swift_file.relative_to(root)} exceeds 1000-line ceiling ({line_count} lines)'
    ui_and_bridge = '\n'.join(
        p.read_text()
        for crate in ('crates/mi-bridge/src', 'crates/mi-macos/src', 'crates/mi-core/src', 'crates/mi-ffi/src')
        for p in (root / crate).rglob('*.rs')
    )
    assert not re.search(r'pub async fn (?:request|rpc|send_report)\b', ui_and_bridge), 'raw RPC exposed in UI/bridge crates'
    subprocess.run(['cargo', 'test', '--locked', '-p', 'mi-session', '--features', 'synthetic-backend', '--test', 'config_write', 'synthetic_writer_rejects_native_backend_even_when_feature_is_built'], cwd=root, check=True)

    home_pattern = re.compile(r'/Users/(?!(?:example|test|private|Shared)(?:/|["\'\s),]|$))[A-Za-z0-9._-]+')
    tracked = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard'],
        cwd=root,
        text=True,
    ).splitlines()
    home_violations = []
    for rel in tracked:
        if rel.startswith('.audit/'):
            continue
        path = root / rel
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            if home_pattern.search(line):
                home_violations.append(f'{rel}:{lineno}: {line.strip()}')
    assert not home_violations, (
        'Hardcoded user home path found; resolve via Path.home(), os.environ, or config discovery instead:\n  '
        + '\n  '.join(home_violations)
    )

    residual_patches = sorted(str(p.relative_to(root)) for p in (root / 'integrations').rglob('*.patch'))
    assert not residual_patches, (
        'R-C02 violation: external agent apps must be adapted directly from Micro Input without third-party .patch files:\n  '
        + '\n  '.join(residual_patches)
    )

    run_src = (root / 'scripts/run').read_text(encoding='utf-8')
    assert (
        'argparse.ArgumentParser' in run_src
        and '__CFBundleIdentifier' in run_src
        and "'--verify' in sys.argv" not in run_src
        and "'-u', str(app)" in run_src
        and 'symlinks=True' in run_src
    ), (
        'R-C05 violation in scripts/run: use argparse.ArgumentParser(allow_abbrev=False) with --list/--kill, strip __CFBundleIdentifier, '
        'copy app bundles with symlinks=True, and unregister QA app bundles from LaunchServices on cleanup'
    )
    swift_app_src = (root / 'app/MicroInput/Sources/MicroInput/MicroInputApp.swift').read_text(encoding='utf-8')
    swift_lock_pos = swift_app_src.find('mi_core_acquire_instance_lock(')
    swift_menu_pos = swift_app_src.find('setupStatusItem()')
    assert swift_lock_pos != -1 and swift_menu_pos != -1 and swift_lock_pos < swift_menu_pos, (
        'R-C05 violation in app/MicroInput/Sources/MicroInput/MicroInputApp.swift: '
        'AppDelegate.init() must acquire mi_core_acquire_instance_lock before setupStatusItem()'
    )
    rust_exec_src = (root / 'crates/mi-macos/src/executor.rs').read_text(encoding='utf-8')
    rust_exec_prod = rust_exec_src.split('#[cfg(test)]', 1)[0]
    assert rust_exec_prod.count('AXIsProcessTrustedWithOptions') == 2, (
        'R-C05 violation in crates/mi-macos/src/executor.rs: '
        'AXIsProcessTrustedWithOptions must only appear in its extern declaration and open_accessibility_settings'
    )

    core_bridge_src = (root / 'app/MicroInput/Sources/MicroInputUI/CoreBridge.swift').read_text(encoding='utf-8')
    assert (
        '@Observable' in core_bridge_src
        and '@ObservationIgnored public private(set) var model: ProjectionModel' in core_bridge_src
        and 'ObservableObject' not in core_bridge_src
        and '@Published' not in core_bridge_src
    ), (
        'R-C06 violation in app/MicroInput/Sources/MicroInputUI/CoreBridge.swift: '
        'CoreStore must use @Observable with @ObservationIgnored model and Equatable-gated domain slices; '
        'do not use ObservableObject or @Published var model'
    )
    for view_name in (
        'WorkbenchRootView.swift',
        'ClassicWorkbenchView.swift',
        'CreatorMicroBoardView.swift',
        'InlineInspectorView.swift',
        'ModalsAndStatusMenuView.swift',
        'SoftwareUpdate.swift',
    ):
        view_src = (root / 'app/MicroInput/Sources/MicroInputUI' / view_name).read_text(encoding='utf-8')
        assert (
            '@ObservedObject' not in view_src
            and 'store.model' not in view_src
            and 'let model: ProjectionModel' not in view_src
        ), (
            f'R-C06 violation in app/MicroInput/Sources/MicroInputUI/{view_name}: '
            'SwiftUI views must read fine-grained domain slices (store.header, store.sidebar, store.canvas, '
            'store.sessionStrip, store.inspector, store.statusMenu, store.overlay, store.footer) instead of '
            '@ObservedObject, store.model, or passing monolithic ProjectionModel'
        )
    ffi_lib_src = (root / 'crates/mi-ffi/src/lib.rs').read_text(encoding='utf-8')
    assert 'store.projection_generation() == before_gen' in ffi_lib_src, (
        'R-C06 violation in crates/mi-ffi/src/lib.rs: '
        'mi_core_dispatch_json must short-circuit with null when store.projection_generation() is unchanged'
    )

    for view_path in sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift')):
        lines = view_path.read_text(encoding='utf-8').splitlines()
        for idx, line in enumerate(lines):
            if '.buttonStyle(.plain)' in line or '.onTapGesture' in line:
                window_start = max(0, idx - 25)
                window_end = min(len(lines), idx + 6)
                window_text = '\n'.join(lines[window_start:window_end])
                assert '.contentShape(Rectangle())' in window_text, (
                    f'R-C07 violation in {view_path.relative_to(root)}:{idx + 1}: '
                    'every .buttonStyle(.plain) and .onTapGesture control must attach .contentShape(Rectangle()) '
                    'so edge/spacer clicks within the control bounds are never dead zones'
                )

    inspector_src = (root / 'app/MicroInput/Sources/MicroInputUI/InlineInspectorView.swift').read_text(encoding='utf-8')
    assert (
        'ScrollViewReader' in inspector_src
        and 'scrollTo("inspector-scroll-top"' in inspector_src
        and inspector_src.count('.id(') >= 2
        and inspector_src.count('.onSubmit') >= 2
    ), (
        'R-C07 violation in app/MicroInput/Sources/MicroInputUI/InlineInspectorView.swift: '
        'CreatorMicroInlineInspectorView must bind .id(...) on slot/layer headers, reset scroll via ScrollViewReader, '
        'and attach .onSubmit handlers to search TextFields'
    )

    store_prod_paths = [root / 'crates/mi-core/src/store.rs'] + [
        p
        for p in sorted((root / 'crates/mi-core/src/store').rglob('*.rs'))
        if not p.name.endswith('tests.rs')
    ]
    store_src = '\n'.join(
        _strip_rust_cfg_test(p.read_text(encoding='utf-8')) for p in store_prod_paths
    )
    assert 'inspector_search_active' in store_src, (
        'R-C07 violation in crates/mi-core/src/store: '
        'consume_physical_input_event must guard selected_surface auto-follow with !inspector_search_active '
        'so hardware key events do not hijack InlineInspectorView focus during active search/recording'
    )

    assert (
        'func applicationDockMenu(' in swift_app_src
        and 'win.isMiniaturized' in swift_app_src
        and 'win.deminiaturize(' in swift_app_src
        and 'applicationShouldHandleReopen(' in swift_app_src
        and 'applicationIconImage' in swift_app_src
    ), (
        'R-C07 violation in app/MicroInput/Sources/MicroInput/MicroInputApp.swift: '
        'AppDelegate must implement applicationDockMenu, deminiaturize minimized windows in openMainWindow/reopen, '
        'and set NSApp.applicationIconImage from AppIcon.icns'
    )

    info_plist_src = (root / 'app/MicroInput/Info.plist').read_text(encoding='utf-8')
    build_src = (root / 'scripts/build').read_text(encoding='utf-8')
    assert '<key>CFBundleIconFile</key>' in info_plist_src and '<string>AppIcon</string>' in info_plist_src and 'AppIcon.icns' in build_src, (
        'R-C07 violation: app/MicroInput/Info.plist and scripts/build must package and declare AppIcon.icns'
    )
    info_plist_data = plistlib.loads(info_plist_src.encode('utf-8'))
    assert info_plist_data.get('CFBundleIdentifier') == 'craft.semantic.micro-input.dev', (
        'Info.plist CFBundleIdentifier must remain craft.semantic.micro-input.dev to preserve macOS TCC grants'
    )
    parsed_feed = urlparse(str(info_plist_data.get('SUFeedURL', '')))
    assert parsed_feed.scheme == 'https' and bool(parsed_feed.netloc), (
        'Info.plist SUFeedURL must declare an HTTPS appcast feed URL with non-empty host'
    )
    decoded_ed_key = base64.b64decode(str(info_plist_data.get('SUPublicEDKey', '')), validate=True)
    assert len(decoded_ed_key) == 32, (
        'Info.plist SUPublicEDKey must be a valid 32-byte base64 Ed25519 public key'
    )
    assert (root / 'LICENSE').is_file() and 'MIT License' in (root / 'LICENSE').read_text(encoding='utf-8'), (
        'Repository root must contain MIT LICENSE file'
    )

    projection_src = (root / 'crates/mi-core/src/store/projection.rs').read_text(encoding='utf-8')
    for forbidden in (
        'WorkbenchStore', 'NativeOwnershipController', 'PushToTalkController',
        'AppBundleCache', 'DeviceSession', 'RunningSixLayer', 'Instant::now',
        '.elapsed(', '.load(Ordering', 'std::fs', 'self.panel_host', 'self.status_item_host',
    ):
        assert forbidden not in projection_src, (
            f'R-C08 violation in crates/mi-core/src/store/projection.rs: {forbidden} is a runtime capability; '
            'projection must read captured value inputs only'
        )
    assert 'UnboundedSender<HardwareLoopEvent>' not in store_src and 'UnboundedReceiver<HardwareLoopEvent>' not in store_src, (
        'R-C08 violation: Workbench hardware ingress must remain bounded with independent loss/control delivery'
    )

    six_layer_runtime_prod_paths = [root / 'crates/mi-core/src/six_layer_runtime.rs'] + [
        p
        for p in sorted((root / 'crates/mi-core/src/six_layer_runtime').rglob('*.rs'))
        if not p.name.endswith('tests.rs')
    ]
    six_layer_runtime_src = '\n'.join(
        _strip_rust_cfg_test(p.read_text(encoding='utf-8')) for p in six_layer_runtime_prod_paths
    )
    assert 'LiveHostActionExecutor::new' not in six_layer_runtime_src and '.driver()' not in store_src, (
        'R-C08 violation: six-layer and UI host actions must share the Store executor admission/cleanup boundary'
    )
    assert 'executor.execute_when(' in six_layer_runtime_src and 'executor.execute(' not in six_layer_runtime_src, (
        'R-C08 violation: six-layer admission must be checked inside the executor effect lock'
    )

    assert (root / 'docs/designs/DESIGN.md').is_file(), (
        'R-C09 violation: docs/designs/DESIGN.md must exist as the canonical visual design contract'
    )
    for view_path in sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift')):
        rel_view = view_path.relative_to(root)
        lines = view_path.read_text(encoding='utf-8').splitlines()
        for idx, line in enumerate(lines):
            assert '.controlSize(.mini)' not in line, (
                f'R-C09 violation in {rel_view}:{idx + 1}: .controlSize(.mini) is forbidden; use SurfaceButtonStyle or SelectDropdown'
            )
            assert '.buttonStyle(.borderedProminent)' not in line and '.buttonStyle(.bordered)' not in line, (
                f'R-C09 violation in {rel_view}:{idx + 1}: system .bordered/.borderedProminent button styles are forbidden; use SurfaceButtonStyle'
            )
            if re.search(r'\bPicker\s*\(', line):
                picker_window = '\n'.join(lines[idx : min(len(lines), idx + 21)])
                assert '.pickerStyle(.segmented)' in picker_window, (
                    f'R-C09 violation in {rel_view}:{idx + 1}: non-segmented Picker is forbidden; use SelectDropdown'
                )
            if '.buttonStyle(.plain)' in line:
                focus_window = '\n'.join(lines[max(0, idx - 5) : min(len(lines), idx + 6)])
                assert '.focusEffectDisabled(' in focus_window, (
                    f'R-C09 violation in {rel_view}:{idx + 1}: every .buttonStyle(.plain) control must attach .focusEffectDisabled()'
                )

    forbidden_radial_copy = ('径向菜单', '摇杆菜单', '摇杆径向菜单')
    ui_and_core_paths = (
        sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'))
        + sorted((root / 'crates/mi-macos/src').rglob('*.rs'))
        + sorted((root / 'crates/mi-core/src').rglob('*.rs'))
    )
    for src_path in ui_and_core_paths:
        src_text = src_path.read_text(encoding='utf-8')
        for term in forbidden_radial_copy:
            assert term not in src_text, (
                f'R-C10 violation in {src_path.relative_to(root)}: forbidden legacy term {term!r}; '
                'use single-source 轮盘菜单 naming (WorkbenchDestination.radial / PassivePanelFrameSpec::RADIAL_TITLE)'
            )
    classic_workbench_src = (root / 'app/MicroInput/Sources/MicroInputUI/ClassicWorkbenchView.swift').read_text(encoding='utf-8')
    status_menu_src = (root / 'app/MicroInput/Sources/MicroInputUI/ModalsAndStatusMenuView.swift').read_text(encoding='utf-8')
    assert 'case radial = "轮盘菜单"' in classic_workbench_src, (
        'R-C10 violation in app/MicroInput/Sources/MicroInputUI/ClassicWorkbenchView.swift: '
        'WorkbenchDestination.radial must declare rawValue "轮盘菜单"'
    )
    assert 'WorkbenchDestination.radial.rawValue' in status_menu_src, (
        'R-C10 violation in app/MicroInput/Sources/MicroInputUI/ModalsAndStatusMenuView.swift: '
        'StatusMenuPopoverView must reference WorkbenchDestination.radial.rawValue as single source of truth'
    )

    forbidden_ui_auto_follow = (
        '跟随前台应用切换方案',
        '切换软件时自动切换方案',
        '自动跟随前台软件',
        'SetAutoFollow',
        'SetAutoFollowEnabled',
        'ResumeAutoFollow',
        'ToggleAutoFollowPause',
        'AppProfileLink',
        'EnsureDefaultAgentLinks',
        'autoFollowEnabled',
        'manualOverride',
        'autoSwitchState',
        '手动模式',
        '暂停自动切换',
        '恢复自动跟随',
        '跟随前台应用',
    )
    for swift_dir in ('app/MicroInput/Sources', 'app/MicroInput/Tests'):
        for view_path in sorted((root / swift_dir).rglob('*.swift')):
            view_text = view_path.read_text(encoding='utf-8')
            for term in forbidden_ui_auto_follow:
                assert term not in view_text, (
                    f'R-C11 violation in {view_path.relative_to(root)}: retired foreground auto-follow symbol/copy {term!r}'
                )

    forbidden_rust_auto_follow = (
        'AutoSwitchState',
        'AutoFollow',
        'SetAutoFollow',
        'SetAutoFollowEnabled',
        'ResumeAutoFollow',
        'ToggleAutoFollowPause',
        'AppProfileLink',
        'EnsureDefaultAgentLinks',
        'ensure_default_agent_links',
        'auto_follow_enabled',
        'manual_override',
        'app_profile_links',
        '跟随前台应用',
        '暂停自动切换',
        '恢复自动跟随',
    )
    for rust_dir in ('crates/mi-core/src', 'crates/mi-macos/src', 'crates/mi-ffi/src', 'crates/mi-model/src'):
        for rust_path in sorted((root / rust_dir).rglob('*.rs')):
            rel = rust_path.relative_to(root).as_posix()
            if rel == 'crates/mi-model/src/storage.rs':
                continue
            rust_text = rust_path.read_text(encoding='utf-8')
            if rel == 'crates/mi-model/src/storage/routing.rs':
                rust_text = rust_text.split('#[cfg(test)]', 1)[0]
            for term in forbidden_rust_auto_follow:
                assert term not in rust_text, (
                    f'R-C11 violation in {rel}: retired foreground auto-follow symbol/field {term!r}'
                )

    storage_src = (root / 'crates/mi-model/src/storage.rs').read_text(encoding='utf-8')
    assert 'fn discard_retired_preference_field' in storage_src and '#[serde(deny_unknown_fields)]' in storage_src, (
        'R-C11 violation in crates/mi-model/src/storage.rs: '
        'must retain discard_retired_preference_field and #[serde(deny_unknown_fields)] for legacy workbench.preferences.json'
    )
    serde_attrs = re.findall(r'#\[serde\(([^]]+)\)\]', storage_src)
    for retired_field in ('auto_follow_enabled', 'manual_override', 'app_profile_links'):
        assert sum(
            1
            for attr in serde_attrs
            if f'rename = "{retired_field}"' in attr
            and 'deserialize_with = "discard_retired_preference_field"' in attr
            and 'skip_serializing' in attr
        ) >= 2, (
            f'R-C11 violation in crates/mi-model/src/storage.rs: '
            f'retired preference field {retired_field!r} must retain skip_serializing and discard_retired_preference_field '
            'on both HostPreferencesV1 and HostPreferences so legacy workbench.preferences.json loads without failing #[serde(deny_unknown_fields)]'
        )

    forbidden_retired_docs = (
        'docs/pstack-relay.md',
        'docs/implementation-plan.md',
        'docs/designs/app-following-and-hud.md',
        'docs/verification/app-following-and-hud.md',
        'docs/designs/input-protocol-reconstruction.md',
        'docs/designs/host-execution-and-agent-presets.md',
        'docs/designs/device-write-and-rgb.md',
    )
    for rel_doc in forbidden_retired_docs:
        assert not (root / rel_doc).exists(), (
            f'R-C12 violation: retired document {rel_doc} must not exist'
        )

    for rel_md in subprocess.check_output(['git', 'ls-files', '*.md'], cwd=root, text=True).splitlines():
        md_path = root / rel_md
        if not md_path.is_file():
            continue
        md_text = md_path.read_text(encoding='utf-8')
        md_stripped = re.sub(r'```.*?```', '', md_text, flags=re.DOTALL)
        md_stripped = re.sub(r'`[^`\n]+`', '', md_stripped)
        for match in re.finditer(r'!?\[[^\]]*\]\(([^)]+)\)', md_stripped):
            raw = match.group(1)
            if raw.startswith(('http://', 'https://', 'mailto:', '#', 'file://')):
                continue
            target_part = raw.strip().split()[0].split('#', 1)[0]
            if not target_part:
                continue
            resolved = (md_path.parent / target_part).resolve()
            assert resolved.exists() and (resolved == root or root in resolved.parents), (
                f'R-C12 violation in {rel_md}: broken relative link {raw!r}'
            )

    verify_hw_src = (root / 'scripts/verify-hardware').read_text(encoding='utf-8')
    assert (
        'app_to_launch = ROOT / "target" / "MicroInput.app"' in verify_hw_src
        and 'subprocess.Popen' in verify_hw_src
        and '["open", "-g"' not in verify_hw_src
        and '"rsync"' not in verify_hw_src
        and 'installed_app' not in verify_hw_src
    ), (
        'R-C13 violation in scripts/verify-hardware: '
        'hardware verification must launch ROOT / "target" / "MicroInput.app" directly via Popen without open -g or rsyncing into /Applications/Micro Input.app'
    )

    install_app_path = root / 'scripts/install-app'
    assert install_app_path.is_file() and os.access(install_app_path, os.X_OK), (
        'R-C13 violation: scripts/install-app must exist and be executable'
    )
    install_app_src = install_app_path.read_text(encoding='utf-8')
    install_app_fn_src = install_app_src.split('def install_app(', 1)[1].split('def parse_args(', 1)[0]
    tcc_reset_pos = install_app_fn_src.find("'tccutil'")
    replace_pos = install_app_fn_src.find('atomic_replace_bundle(')
    assert (
        'Developer ID Application:' in install_app_src
        and '--allow-dev' in install_app_src
        and 'spctl' in install_app_src
        and 'tccutil' in install_app_src
        and 'codesign' in install_app_src
        and tcc_reset_pos != -1
        and replace_pos != -1
        and tcc_reset_pos < replace_pos
    ), (
        'R-C13 violation in scripts/install-app: '
        'must enforce Developer ID Application unless --allow-dev and reset TCC before atomic_replace_bundle'
    )

    release_macos_src = (root / 'scripts/release-macos').read_text(encoding='utf-8')
    assert "'--install'" in release_macos_src and 'scripts/install-app' in release_macos_src, (
        'R-C13 violation in scripts/release-macos: must wire --install to scripts/install-app'
    )

    open_ax_slice = rust_exec_prod.split('impl HostOsDriver for MacOsSystemDriver', 1)[1].split('fn open_application(', 1)[0]
    assert (
        'fn open_accessibility_settings(&self)' in open_ax_slice
        and 'AXIsProcessTrustedWithOptions' in open_ax_slice
    ), (
        'R-C13 violation in crates/mi-macos/src/executor.rs: '
        'MacOsSystemDriver::open_accessibility_settings must call AXIsProcessTrustedWithOptions'
    )

    clean_room_leaks: list[str] = []
    for rel in tracked:
        if rel in ('scripts/check-boundaries.py', 'scripts/tests/test_boundaries_launcher.py') or rel.startswith('.audit/'):
            continue
        path = root / rel
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            continue
        clean_room_leaks.extend(find_clean_room_leaks(rel, text))
    assert not clean_room_leaks, (
        'R-C14 violation: private/extracted material leak found in docs or source:\n  '
        + '\n  '.join(clean_room_leaks)
    )

    unix_connect_violations: list[str] = []
    for rust_path in sorted((root / 'crates').glob('*/src/**/*.rs')):
        rel = rust_path.relative_to(root).as_posix()
        unix_connect_violations.extend(
            find_forbidden_unix_connect(rel, rust_path.read_text(encoding='utf-8'))
        )
    assert not unix_connect_violations, (
        'R-C14 violation: non-test UnixStream::connect( is confined to crates/mi-bridge/src/ipc.rs:\n  '
        + '\n  '.join(unix_connect_violations)
    )

    ffi_guard_violations: list[str] = []
    for ffi_path in sorted((root / 'crates/mi-ffi/src').rglob('*.rs')):
        rel = ffi_path.relative_to(root).as_posix()
        for fn_name in find_unguarded_ffi_entrypoints(rel, ffi_path.read_text(encoding='utf-8')):
            ffi_guard_violations.append(f'{rel}: {fn_name}')
    assert not ffi_guard_violations, (
        'R-C14 violation: every extern "C" entrypoint in crates/mi-ffi/src must wrap its body with ffi_guard(:\n  '
        + '\n  '.join(ffi_guard_violations)
    )

    ffi_tree_output = subprocess.check_output(
        ['cargo', 'tree', '-p', 'mi-ffi', '--edges', 'normal', '-e', 'features', '--locked', '--offline'],
        cwd=root,
        text=True,
    )
    prod_qa_leaks = find_production_qa_leaks(
        store_src,
        (root / 'crates/mi-macos/src/status_item.rs').read_text(encoding='utf-8'),
        (root / 'crates/mi-macos/src/passive_panel.rs').read_text(encoding='utf-8'),
        (root / 'crates/mi-core/Cargo.toml').read_text(encoding='utf-8'),
        ffi_tree_output,
    )
    assert not prod_qa_leaks, (
        'Production isolation violation (synthetic/QA/flash/recording host leak):\n  '
        + '\n  '.join(prod_qa_leaks)
    )

    ui_sources = {
        p.relative_to(root).as_posix(): p.read_text(encoding='utf-8')
        for p in sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'))
    }
    assert 'SetExecutionEnabled' in classic_workbench_src and 'SetHostExecutionEnabled' in status_menu_src, (
        'R-C15 violation: expected global key-mapping kill-switch Toggles in ClassicWorkbenchView.swift and ModalsAndStatusMenuView.swift'
    )
    rc15_violations = (
        find_kill_switch_focus_violations(
            ui_sources,
            ui_sources['app/MicroInput/Sources/MicroInputUI/UITheme.swift'],
        )
        + find_status_item_state_violations(swift_app_src)
    )
    assert not rc15_violations, (
        'R-C15 violation (global key-mapping kill-switch focus isolation and paused status item state):\n  '
        + '\n  '.join(rc15_violations)
    )

    ag_labels_path = root / 'crates/mi-macos/src/antigravity_ax_labels.rs'
    assert ag_labels_path.is_file(), (
        'R-C16 violation: missing single-source bilingual registry crates/mi-macos/src/antigravity_ax_labels.rs'
    )
    ag_labels_src = ag_labels_path.read_text(encoding='utf-8')
    assert (
        'struct BilingualLabelSet' in ag_labels_src
        and 'ALL_BILINGUAL_LABEL_SETS' in ag_labels_src
    ), (
        'R-C16 violation: crates/mi-macos/src/antigravity_ax_labels.rs must define struct BilingualLabelSet and ALL_BILINGUAL_LABEL_SETS'
    )
    rc16_violations: list[str] = []
    for macos_path in sorted((root / 'crates/mi-macos/src').rglob('*.rs')):
        rel = macos_path.relative_to(root).as_posix()
        rc16_violations.extend(
            find_scattered_antigravity_ax_label_violations(
                rel, macos_path.read_text(encoding='utf-8')
            )
        )
    assert not rc16_violations, (
        'R-C16 violation (scattered Antigravity AX labels outside antigravity_ax_labels.rs):\n  '
        + '\n  '.join(rc16_violations)
    )

    print('UNKNOWN inventory and read-only/no-send boundaries: PASS')


if __name__ == '__main__':
    run_boundary_checks()
