"""tools/combo_stats.py (docs/ENGINE.md 179): a game rebuilt from its seed and move log is read into its stats line - a
resolved Doomsday's pile is the library after it, the end's method, the tracked casts - and the report's numbers.
Random-move games of the open Legacy decklists; no network."""
import json
import os
import random
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz import _engine as E  # noqa: E402
import combo_stats as C  # noqa: E402

DD, SNS, DELVER = 'doomsday_8027957', 'sneak_show_8014220', 'izzet_delver_8016541'


def random_row(spec, driver_seed, gid, rng, max_moves=4000):
    """A random-move game as the driver's result row with its move log (seat, action, legal actions)."""
    g = E.Game('mtg', spec, 2, E.game_seed_of(driver_seed, gid, False))
    moves = []
    while g.to_move is not None and len(moves) < max_moves:
        legal = list(g.legal())
        a = rng.choice(legal)
        moves.append([g.to_move, a, legal])
        g.step(a)
    over = g.to_move is None
    return {'gid': gid, 'over': over, 'aborted': False, 'result': [float(x) for x in g.result()] if over else [0.5, 0.5],
            'decisions': len(moves), 'moves': moves}


def meta(spec, decks, seed):
    return {'label': 't', 'batch': 0, 'driver_seed': seed, 'decks': decks, 'sims': [1, 1], 'spec': spec}


def test_spec_has_a_deck_per_seat():
    sp = json.loads(C.deck_spec(DD, SNS))
    assert sp['constructed'] and len(sp['decks']['p0']) == 60 and len(sp['decks']['p1']) == 60
    assert 'Doomsday' in sp['decks']['p0'] and 'Show and Tell' in sp['decks']['p1']
    E.mtg_spec_check(C.deck_spec(DD, SNS))


def test_a_resolved_doomsday_is_read_with_its_pile():
    spec = C.deck_spec(DD, DELVER)
    rng = random.Random(0)
    found = None
    for gid in range(300):
        row = random_row(spec, 11, gid, rng)
        s = C.scan_row(E, meta(spec, [DD, DELVER], 11), row)
        assert s['method'] in ('oracle', 'pact', 'combat', 'damage', 'decking', 'turn_cap', 'unfinished', 'effect',
                               'sba_other') or s['method'].startswith('draw_')
        assert s['decisions'] == len(row['moves'])
        if s['doomsday']:
            found = s
            break
    assert found, 'no random game resolved a Doomsday'
    dd = found['doomsday'][0]
    assert dd['p'] == 0
    assert 1 <= len(dd['pile']) <= 5 and dd['library'] == len(dd['pile'])
    assert dd['oracle_reachable'] == (C.ORACLE in dd['pile'] or C.ORACLE in dd['hand'])
    assert any(t.endswith(' on top') for t in dd['choices'])
    assert any(sp['card'] == 'Doomsday' and sp['outcome'] == 'resolved' for sp in found['spells'])
    rep = C.summarize([dict(found, wall_s_per_game=1.0, cpu_s_per_game=1.0, batch_failures=0)])
    assert rep['doomsday']['p0']['resolved'] == 1
    assert '### t:' in C.table(rep)


def test_method_reads_the_end():
    final = {'over': True, 'reason': 'Sba', 'winner': 1, 'step': 'combat_damage',
             'players': [{'life': -2, 'library': 30}, {'life': 5, 'library': 30}]}
    assert C.method_of(final, None) == 'combat'
    final['step'] = 'main1'
    assert C.method_of(final, None) == 'damage'
    final['players'][0]['life'] = 3
    assert C.method_of(final, None, ['T9 Draw P0 draws from an empty library']) == 'decking'
    eff = {'over': True, 'reason': 'Effect', 'winner': 0, 'players': [{}, {}]}
    assert C.method_of(eff, None, ["T9 Main1 trigger of Thassa's Oracle goes on the stack"]) == 'oracle'
    assert C.method_of(eff, None, ['T9 Upkeep trigger of Pact of Negation goes on the stack']) == 'pact'
    assert C.method_of({'over': True, 'reason': 'TurnLimit', 'winner': None}, None) == 'turn_cap'
    assert C.method_of({'over': False}, None) == 'unfinished'


def test_paired_difference_over_common_deals():
    a = [{'driver_seed': 1, 'gid': g, 'result': [r, 1 - r]} for g, r in enumerate([0, 1, 0, 0])]
    b = [{'driver_seed': 1, 'gid': g, 'result': [r, 1 - r]} for g, r in enumerate([1, 1, 0, 1])]
    p = C.paired(a, b)
    assert p['n'] == 4 and abs(p['diff'] - 0.5) < 1e-9 and p['se'] > 0
