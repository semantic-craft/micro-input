"""Deterministic guard for UNKNOWN inventory and the physical framing barrier."""
from pathlib import Path
import re
import subprocess
root = Path(__file__).resolve().parent.parent
sources = list((root / 'crates').rglob('*.rs'))
found = set()
for path in sources:
    found.update(re.findall(r'TODO\((UNKNOWN-\d+)\)', path.read_text()))
expected = {f'UNKNOWN-{n:02}' for n in range(1, 10)}
doc = (root / 'docs/spec/UNKNOWNS.md').read_text()
documented = set(re.findall(r'^\| (UNKNOWN-\d+) \|', doc, re.MULTILINE))
assert expected <= documented, (expected - documented)
assert found <= documented, (found - documented)
# R-D13: guard behavior rather than requiring one placeholder implementation.
# The tests capture the actual session出口 and assert zero reports for rejected requests.
for package, behavior in [
    ('mi-proto', 'production_exit_is_strictly_read_only'),
    ('mi-session', 'production_exit_rejects_forbidden_parameters_without_sending'),
    ('mi-session', 'real_framer_never_writes'),
    ('mi-session', 'encoded_mutation_candidates_cannot_bypass_production_exit'),
    ('mi-session', 'encoded_readonly_requests_never_forward_ambiguous_original_bytes'),
]:
    subprocess.run(['cargo', 'test', '--locked', '-p', package, behavior], cwd=root, check=True)
ffi = '\n'.join(p.read_text() for p in (root / 'crates/mi-ffi/src').rglob('*.rs'))
assert not re.search(r'pub async fn (?:request|rpc|send_report)\b', ffi), 'raw RPC exposed through UniFFI'
subprocess.run(['cargo', 'test', '--locked', '-p', 'mi-session', '--features', 'synthetic-backend', 'synthetic_writer_rejects_native_backend_even_when_feature_is_built'], cwd=root, check=True)
print('UNKNOWN inventory and read-only/no-send boundaries: PASS')
