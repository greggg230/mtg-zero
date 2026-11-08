"""The Edge of Eternities card list for sealed, and the engine's view of it (docs/sealed_data.md 2).

* `eoe_set()`: `data/sealed/eoe_set.json` (built by `tools/sealed_eoe_set.py` from Scryfall data):
  every card a Play Booster can hold, with its sheet (`eoe` main set, `eos` Stellar Sights, `spg`
  Special Guests, `basic`), rarity and colours, plus the booster-fun printings of main-set cards.
* `ir_names()`: the engine's EoE card table (`data/cards/eoe.ir.json`, ENGINE.md 99.1): the set's
  cards but Paradox Haze (no IR), and the basics.
* `supported()`: the cards the engine supports today, read from the `SUPPORTED_EOE_*` lists of
  `engine/tests/support.rs` (the support report's expected set, ENGINE.md 99), so it follows the
  checkout without a rebuilt engine module.
* `map_name()`: a card name from elsewhere (17lands) -> the set's name, or None.
"""
import functools
import json
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DATA = os.path.join(ROOT, 'data', 'sealed')
EOE_SET = os.path.join(DATA, 'eoe_set.json')
EOE_IR = os.path.join(ROOT, 'data', 'cards', 'eoe.ir.json')
SUPPORT_RS = os.path.join(ROOT, 'engine', 'tests', 'support.rs')
BASICS = ('Plains', 'Island', 'Swamp', 'Mountain', 'Forest')
COLOURS = 'WUBRG'


@functools.lru_cache(None)
def eoe_set(path=EOE_SET):
    with open(path, encoding='utf-8') as f:
        doc = json.load(f)
    if doc.get('format') != 'mtgz-eoe-set':
        raise ValueError(f'{path}: not an mtgz-eoe-set file')
    return doc


@functools.lru_cache(None)
def cards(path=EOE_SET):
    """name -> its row of eoe_set.json."""
    return {c['name']: c for c in eoe_set(path)['cards']}


@functools.lru_cache(None)
def ir_names(path=EOE_IR):
    with open(path, encoding='utf-8') as f:
        return frozenset(c['name'] for c in json.load(f)['cards'])


def key(name):
    """A loose matching key: ASCII letters and digits of the front face, lower case."""
    front = name.split(' // ')[0]
    return ''.join(ch for ch in front.encode('ascii', 'ignore').decode().lower() if ch.isalnum())


@functools.lru_cache(None)
def _by_key(path=EOE_SET):
    by = {}
    for n in cards(path):
        by.setdefault(key(n), []).append(n)
    return {k: v[0] for k, v in by.items() if len(v) == 1}


def map_name(name, path=EOE_SET):
    """The set's name for `name` (exact, else the front face of a split / double-faced name, else
    the loose key), or None when nothing matches."""
    if name in cards(path):
        return name
    front = name.split(' // ')[0]
    if front in cards(path):
        return front
    return _by_key(path).get(key(name))


_LIST = re.compile(r'const\s+(SUPPORTED_EOE_\w+)\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\];', re.S)
_USED = re.compile(r'fn\s+supported_eoe\s*\(\s*\)[^{]*\{.*?let\s+lists\s*=\s*\[(.*?)\]\s*;', re.S)


def supported(path=SUPPORT_RS):
    """The names in the `SUPPORTED_EOE_*` lists that `supported_eoe()` unions (support.rs).
    Raises if the file has no such lists, so a renamed list fails loudly, never as 'nothing
    supported'."""
    with open(path, encoding='utf-8') as f:
        src = f.read()
    lists = {m.group(1): re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(2)) for m in _LIST.finditer(src)}
    used = _USED.search(src)
    if not lists or not used:
        raise ValueError(f'{path}: no SUPPORTED_EOE_* lists or no supported_eoe()')
    names = set()
    for ident in (s.strip() for s in used.group(1).split(',')):
        if ident:
            if ident not in lists:
                raise ValueError(f'{path}: supported_eoe() uses {ident}, which is not a list here')
            names.update(lists[ident])
    return frozenset(names)
