"""Build `data/sealed/eoe_set.json`: the Edge of Eternities card list as the collation needs it
(docs/sealed_data.md 2).

Per card: the sheet it is printed on (`eoe` the main set, `eos` Stellar Sights, `spg` the set's ten
Special Guests, `basic` the five basic lands), its rarity, collector number, colours, colour
identity, mana value, type line, and whether it is in the engine's card table
(`data/cards/eoe.ir.json`: every card but Paradox Haze, which has no IR). Plus the booster-fun
printings of main-set cards that Play Boosters can hold (EOE 277-316: borderless viewport lands,
triumphant and surreal space cards), which only change how often their card's name comes up.

Sources (read-only): mtg-bot's Scryfall dumps (`data/edge_of_eternities.json`,
`stellar_sights.json`, `special_guests_eoe.json`) and one Scryfall search for EOE 277-316
(`--treatments`, the saved JSON of SCRYFALL_TREATMENTS_URL). Rerun only when the sources change:

    python tools/sealed_eoe_set.py --treatments /path/to/eoe_277_316.json
"""
import argparse
import hashlib
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MTG_BOT_DATA = os.path.join(os.path.expanduser('~'), 'Documents', 'Claude', 'Projects', 'mtg-bot', 'data')
DUMPS = (('edge_of_eternities.json', 'eoe'), ('stellar_sights.json', 'eos'), ('special_guests_eoe.json', 'spg'))
SCRYFALL_TREATMENTS_URL = ('https://api.scryfall.com/cards/search?q=set%3Aeoe+cn%3E%3D277+cn%3C%3D316'
                           '&unique=prints&order=set')
# Collector-number ranges of the Play Booster's booster-fun printings (Wizards' "Collecting Edge of
# Eternities"; mtg.wtf's data/boosters/eoe/play.yaml uses the same ranges).
TREATMENTS = (('viewport', 277, 286), ('triumphant', 287, 302), ('surreal', 303, 316))
BASICS = ('Plains', 'Island', 'Swamp', 'Mountain', 'Forest')
COLOURS = 'WUBRG'


def sha256(path):
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def colours(cs):
    return ''.join(c for c in COLOURS if c in cs)


def card_row(c, sheet, in_ir):
    return {
        'name': c['name'],
        'sheet': 'basic' if c['name'] in BASICS else sheet,
        'rarity': c['rarity'],
        'collector_number': c['collector_number'],
        'colors': colours(c.get('colors') or []),
        'color_identity': colours(c.get('color_identity') or []),
        'mana_value': int(c.get('cmc') or 0),
        'type_line': c['type_line'],
        'in_ir': in_ir,
    }


def build(mtg_bot_data, treatments_path):
    ir = json.load(open(os.path.join(ROOT, 'data', 'cards', 'eoe.ir.json'), encoding='utf-8'))
    ir_names = {c['name'] for c in ir['cards']}
    cards, sources = [], []
    for fname, sheet in DUMPS:
        path = os.path.join(mtg_bot_data, fname)
        dump = json.load(open(path, encoding='utf-8'))
        sources.append({'file': f'mtg-bot data/{fname}', 'sha256': sha256(path), 'cards': len(dump)})
        for c in sorted(dump, key=lambda c: int(c['collector_number'])):
            assert c['layout'] == 'normal', (c['name'], c['layout'])
            cards.append(card_row(c, sheet, c['name'] in ir_names))
    names = [c['name'] for c in cards]
    assert len(names) == len(set(names)), 'a name on two sheets'
    missing = ir_names - set(names)
    assert not missing, f'IR cards without a Scryfall row: {sorted(missing)}'

    found = json.load(open(treatments_path, encoding='utf-8'))
    assert not found.get('has_more'), 'the treatments search has another page'
    sources.append({'url': SCRYFALL_TREATMENTS_URL, 'sha256': sha256(treatments_path),
                    'cards': len(found['data'])})
    rarity = {c['name']: c['rarity'] for c in cards}
    treat = {t: [] for t, _, _ in TREATMENTS}
    for c in sorted(found['data'], key=lambda c: int(c['collector_number'])):
        n = int(c['collector_number'])
        (t,) = [t for t, lo, hi in TREATMENTS if lo <= n <= hi]
        assert c['name'] in rarity and rarity[c['name']] == c['rarity'], c['name']
        treat[t].append({'collector_number': c['collector_number'], 'name': c['name'], 'rarity': c['rarity']})
    assert sum(len(v) for v in treat.values()) == 40
    return {
        'format': 'mtgz-eoe-set',
        'version': 1,
        'set': 'EOE',
        'note': 'Built by tools/sealed_eoe_set.py; docs/sealed_data.md 2. Read by mtgz/sealed.',
        'sources': sources,
        'cards': cards,
        'treatments': treat,
    }


def write(doc, path):
    """One card per line, so a change diffs as the cards it changes."""
    lines = ['{']
    for k in ('format', 'version', 'set', 'note'):
        lines.append(f'  {json.dumps(k)}: {json.dumps(doc[k], ensure_ascii=False)},')
    lines.append('  "sources": [')
    lines.append(',\n'.join('    ' + json.dumps(s, ensure_ascii=False) for s in doc['sources']))
    lines.append('  ],')
    lines.append('  "cards": [')
    lines.append(',\n'.join('    ' + json.dumps(c, ensure_ascii=False) for c in doc['cards']))
    lines.append('  ],')
    lines.append('  "treatments": {')
    parts = []
    for t, rows in doc['treatments'].items():
        body = ',\n'.join('      ' + json.dumps(r, ensure_ascii=False) for r in rows)
        parts.append(f'    {json.dumps(t)}: [\n{body}\n    ]')
    lines.append(',\n'.join(parts))
    lines.append('  }')
    lines.append('}')
    with open(path, 'w', encoding='utf-8', newline='') as f:
        f.write('\n'.join(lines) + '\n')
    json.load(open(path, encoding='utf-8'))      # it parses


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--mtg-bot-data', default=MTG_BOT_DATA)
    ap.add_argument('--treatments', required=True, help=f'the saved JSON of {SCRYFALL_TREATMENTS_URL}')
    ap.add_argument('--out', default=os.path.join(ROOT, 'data', 'sealed', 'eoe_set.json'))
    a = ap.parse_args(argv)
    doc = build(a.mtg_bot_data, a.treatments)
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    write(doc, a.out)
    by = {}
    for c in doc['cards']:
        by[(c['sheet'], c['rarity'])] = by.get((c['sheet'], c['rarity']), 0) + 1
    print(f'{a.out}: {len(doc["cards"])} cards', dict(sorted(by.items())),
          {t: len(v) for t, v in doc['treatments'].items()})


if __name__ == '__main__':
    sys.exit(main())
