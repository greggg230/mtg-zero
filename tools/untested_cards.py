"""Supported cube cards, and the legacy cards of constructed play (engine/src/az/legacy_cards.txt:
all supported, `CardDb::constructed`), that no card test names (engine/tests, excluding the
support, az, fuzz, perf and smoke suites). Run from the repo root: python tools/untested_cards.py"""
import glob
import os
import re

src = open('engine/tests/support.rs', encoding='utf-8').read()
m = re.search(r'const SUPPORTED[^=]*=\s*&\[(.*?)\];', src, re.S)
names = re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(1))
legacy = [l.strip() for l in open('engine/src/az/legacy_cards.txt', encoding='utf-8') if l.strip()]
names += legacy
tests = ''
for f in glob.glob('engine/tests/*.rs') + glob.glob('engine/tests/common/*.rs'):
    if os.path.basename(f) in ('support.rs', 'az.rs', 'fuzz.rs', 'perf.rs', 'smoke.rs'):
        continue
    tests += open(f, encoding='utf-8').read()
basics = {'Plains', 'Island', 'Swamp', 'Mountain', 'Forest'}
untested = [n for n in names if n.replace('\\"', '"') not in tests and n not in basics]
print(f'{len(names)} supported ({len(legacy)} legacy), {len(untested)} named by no card test')
print('\n'.join(untested))
