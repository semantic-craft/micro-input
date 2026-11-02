"""Deterministic guard for UNKNOWN inventory and the physical framing barrier."""
from pathlib import Path
import re
root = Path(__file__).resolve().parent.parent
sources = list((root / 'crates').rglob('*.rs'))
found = set()
for path in sources:
    found.update(re.findall(r'TODO\((UNKNOWN-\d+)\)', path.read_text()))
expected = {f'UNKNOWN-{n:02}' for n in range(1, 8)}
assert found == expected, (found, expected)
doc = (root / 'docs/spec/UNKNOWNS.md').read_text()
assert expected <= set(re.findall(r'UNKNOWN-\d+', doc))
production_framers = []
for path in sources:
    if path.name == 'tests.rs':
        continue
    production_framers += re.findall(r'impl Framer for (\w+)', path.read_text())
assert production_framers == ['UnimplementedFramer'], production_framers
print('UNKNOWN inventory and production framing boundary: PASS')
