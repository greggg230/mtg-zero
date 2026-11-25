"""pb3's sideboarding for a pairing of two different 75s (docs/ENGINE.md 240, mtgz.az.matchup): the pairing a model is
fit for (a mirror is pb3's, the same code), the non-mirror model (not antisymmetric: the pairing's base rate) and its fit,
one choice table per deck against the other deck's prior, the tables placed on the decks the engine reads them from
(a metagame deck's entry, a fixed spec's seat, pb3's one table for a fixed mirror), the self-play hook, the specs and
presets of the stew specialists, and every card of the stew's 75s supported in best-of-three."""
import json
import os

import numpy as np
import pytest

import mtgz.az as A
from mtgz.az import decklists as D
from mtgz.az import evaldecks
from mtgz.az import matchup as MU

# two small 75s: (copies, main) per entry
CC, MC = [4, 4, 2, 3, 2], [4, 3, 1, 0, 0]
CD, MD = [3, 4, 2, 2], [3, 3, 2, 0]
LC = [[f'c{i}', k] for i, k in enumerate(CC)]
LD = [[f'd{i}', k] for i, k in enumerate(CD)]


def spec_line(key, seat0, seat1, names=None, idx=None):
    sp = {'v': 1, 'kind': 'bo3-spec', 'spec': key, 'lists': [seat0[0], seat1[0]], 'mains': [seat0[1], seat1[1]],
          'dealt_decks': {'p': 0.85, 'swaps': [0, 2], 'per_seat': True}, 'table': None, 'start_game': 1, 'max_turns': 60}
    if names is not None:
        sp['metagame'] = {'decks': idx, 'names': names}
    return sp


def pair_specs():
    """A metagame restricted to C (index 0) and D (index 1): both seat orders."""
    return {'kA': spec_line('kA', (LC, MC), (LD, MD), ['C', 'D'], [0, 1]),
            'kB': spec_line('kB', (LD, MD), (LC, MC), ['D', 'C'], [1, 0])}


def delta(r, copies, main):
    """A random legal-ish count vector: the main with a few one-for-one swaps."""
    c = list(main)
    for _ in range(int(r.integers(0, 3))):
        o = [k for k in range(len(c)) if c[k] > 0]
        i = [k for k in range(len(c)) if c[k] < copies[k]]
        a, b = o[int(r.integers(len(o)))], i[int(r.integers(len(i)))]
        if a != b:
            c[a] -= 1
            c[b] += 1
    return c


def synthetic_pair(n, wC, wD, m, pd=0.3, seed=0):
    """Games of the pairing from the planted non-mirror model z = m + wC . dA - wD . dB + pd * (C starts)."""
    r = np.random.default_rng(seed)
    games = []
    for i in range(n):
        cdeck, ddeck = delta(r, CC, MC), delta(r, CD, MD)
        c_seat = int(r.integers(2))
        st = int(r.integers(2))
        z = m + np.dot(wC, np.subtract(cdeck, MC)) - np.dot(wD, np.subtract(ddeck, MD)) + (pd if st == c_seat else -pd)
        y = float(r.random() < 1 / (1 + np.exp(-z)))
        decks = [cdeck, ddeck] if c_seat == 0 else [ddeck, cdeck]
        res = [y, 1 - y] if c_seat == 0 else [1 - y, y]
        games.append({'spec': 'kA' if c_seat == 0 else 'kB', 'decks': decks, 'starter': st, 'game': 2 + int(r.integers(2)),
                      'wins': [1, 0] if r.integers(2) else [0, 1], 'dealt': [bool(r.integers(2)), bool(r.integers(2))],
                      'result': res, 'iter': i * 6 // n, 'mode': ['dealt', 'chosen']})
    return games


def test_the_pairing_of_the_games():
    sp = pair_specs()
    games = [{'spec': 'kA'}, {'spec': 'kB'}]
    P = MU.the_pairing(sp, games)
    assert not P.mirror and [d['name'] for d in P.decks] == ['C', 'D'] and P.decks[0]['list'] == LC
    assert P.seat_of_c(sp['kA']) == 0 and P.seat_of_c(sp['kB']) == 1
    # a mirror (pb3's: one 75 both seats, a fixed spec): the_75's list and main
    mir = {'k1': spec_line('k1', (LC, MC), (LC, MC))}
    M = MU.the_pairing(mir, [{'spec': 'k1'}])
    assert M.mirror and (M.decks[0]['list'], M.decks[0]['main']) == MU.the_75(mir, [{'spec': 'k1'}])
    # a fixed two-75 spec: C is seat 0
    fx = {'k2': spec_line('k2', (LD, MD), (LC, MC))}
    F = MU.the_pairing(fx, [{'spec': 'k2'}])
    assert not F.mirror and F.decks[0]['list'] == LD and F.seat_of_c(fx['k2']) == 0
    # refused: two pairings, two names of one 75
    two = dict(sp, kC=spec_line('kC', (LC, MC), (LC, MC), ['C', 'C'], [0, 0]))
    with pytest.raises(ValueError, match='2 pairings'):
        MU.the_pairing(two, [{'spec': 'kA'}, {'spec': 'kC'}])
    named = {'kX': spec_line('kX', (LC, MC), (LC, MC), ['C', 'E'], [0, 2])}
    with pytest.raises(ValueError, match='one 75'):
        MU.the_pairing(named, [{'spec': 'kX'}])


def test_the_pair_model_is_c_sided_and_fits_the_planted_base_rate_and_strengths():
    wC = np.array([0.0, -0.4, 0.0, 0.7, 0.0])
    wD = np.array([0.0, 0.0, -0.5, 0.6])
    games = synthetic_pair(8000, wC, wD, m=0.5, seed=3)
    sp = pair_specs()
    P = MU.the_pairing(sp, games)
    dA, dB, ctx, y, it = MU.pair_arrays(games, sp, P)
    assert dA.shape[1] == len(CC) and dB.shape[1] == len(CD) and ctx.shape[1] == len(MU.PAIR_CTX)
    # every row C's side: y is the C seat's result
    for g, yy, row in zip(games[:50], y, ctx):
        cs = 0 if g['spec'] == 'kA' else 1
        assert yy == g['result'][cs] and row[1] == (1.0 if g['starter'] == cs else -1.0) and row[0] == 1.0
    members, rep = MU.select_and_fit_pair(games, sp, P, val_iters=2, half_life=0.0, threads=2, log=lambda m: None)
    cen = lambda w, main: w - np.dot(w, main) / sum(main)      # noqa: E731 (the report's centring)
    assert np.abs(np.asarray(rep['per_copy']['C']) - cen(wC, MC)).max() < 0.15, rep['per_copy']
    assert np.abs(np.asarray(rep['per_copy']['D']) - cen(wD, MD)).max() < 0.15, rep['per_copy']
    assert rep['gain_nats'] > 0.01 and rep['val_logloss'] < rep['val_logloss_pd']
    # the base rate and play / draw (the model's context terms, C's side; the baseline holds them too, so the deck
    # terms are what the gain measures)
    assert abs(rep['ctx'][0] - 0.5) < 0.15 and abs(rep['ctx'][1] - 0.3) < 0.1, rep['ctx']
    assert len(rep['pd_baseline']) == MU.PAIR_BASELINE
    assert len(members) == MU.MEMBERS and rep['selected']['dim'] in MU.DIMS
    # not antisymmetric: a base rate the mirror's model cannot hold
    m0 = MU._npp(members[0])
    z = MU.z_pair(m0, np.zeros((1, len(CC))), np.zeros((1, len(CD))), np.asarray([[1.0, 0, 0, 0, 0, 0, 0]]))
    assert z[0] > 0.3


def pair_members(wC, wD, c0=0.0, dim=0):
    m = {'wC': list(wC), 'wD': list(wD), 'c': [c0, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0], 'UC': [], 'UD': [], 'W': [], 'dim': dim}
    return [m, dict(m, wC=[x + 0.01 for x in wC], wD=[x + 0.01 for x in wD])]


def pair_of():
    return MU.Pairing([{'name': 'C', 'list': LC, 'main': MC, 'index': 0}, {'name': 'D', 'list': LD, 'main': MD, 'index': 1}], False)


def test_one_table_per_deck_each_against_the_other_decks_prior():
    P = pair_of()
    # C gains from its sideboard entry c3, D from its d3 (a larger wD is D's strength: z falls)
    mem = pair_members([0, 0, 0, 2.0, 0], [0, 0, 0, 2.0])
    tables, detail = MU.pair_choice_tables(mem, P, (0, 2), 'v0007', worlds=300, seed=1, log=lambda s: None)
    assert [t['list'] for t in tables] == [LC, LD] and all(t['version'] == 'v0007' for t in tables)
    for t, copies, main, strong in zip(tables, (CC, CD), (MC, MD), (3, 3)):
        assert [tuple(c[:4]) for c in t['choices']] == MU.contexts()
        for c in t['choices']:
            deck = c[4]
            assert sum(deck) == sum(main) and all(0 <= a <= b for a, b in zip(deck, copies))
            assert deck[strong] == copies[strong], 'every copy of the strong sideboard card in'
    assert set(detail) == {'C', 'D'} and all(s['gain'] > 2 * s['se'] for d in detail.values() for x in d for s in x['swaps'])
    # D's objective is its own win probability: sigmoid(-z) (with D's strength 0 and C's strong, D keeps its main)
    mem2 = pair_members([0, 0, 0, 2.0, 0], [0, 0, 0, 0])
    t2, _ = MU.pair_choice_tables(mem2, P, (0, 2), 'v0008', worlds=300, seed=2, log=lambda s: None)
    assert all(c[4] == MD for c in t2[1]['choices']), 'nothing to gain for D'
    # the play / draw rule per role: first takes its better side, second its worse - F(first) >= F(second)
    beliefs = MU.sample_prior(CD, MD, 0, 2, 200, np.random.default_rng(0)) - np.asarray(MD)
    for role, cands in ((0, np.zeros((1, len(CC)))), (1, np.zeros((1, len(CD))))):
        w = beliefs if role == 0 else MU.sample_prior(CC, MC, 0, 2, 200, np.random.default_rng(1)) - np.asarray(MC)
        f1 = MU.PairObjective(mem, w.astype(float), role, 2, 1, 0, 0)(cands)[0]
        f0 = MU.PairObjective(mem, w.astype(float), role, 2, 0, 0, 0)(cands)[0]
        assert f1 >= f0 - 1e-12
    # roles are each other's complement on the same worlds pair (z C-sided): F_C(main) + F_D(main) = 1 on a fixed pair
    one = [dict(mem[0], c=[0.4, 0, 0, 0, 0, 0, 0])]
    fc = MU.PairObjective(one, np.zeros((1, len(CD))), 0, 2, 1, 0, 0)(np.zeros((1, len(CC))))[0]
    fd = MU.PairObjective(one, np.zeros((1, len(CC))), 1, 2, 1, 0, 0)(np.zeros((1, len(CD))))[0]
    assert abs(fc + fd - 1.0) < 1e-12


def test_a_mirror_is_pb3s_model_and_table_exactly():
    # the mirror's games through retrain's builder give exactly what pb3's code gives (select_and_fit, choice_table)
    COPIES, MAIN = CC, MC
    w = np.array([0.6, -0.4, 0.0, 0.3, -0.5])
    r = np.random.default_rng(7)
    games = []
    for i in range(3000):
        a, b = delta(r, COPIES, MAIN), delta(r, COPIES, MAIN)
        st = int(r.integers(2))
        z = float(np.dot(w, np.subtract(a, b))) + (0.4 if st == 0 else -0.4)
        y = float(r.random() < 1 / (1 + np.exp(-z)))
        games.append({'spec': 'k1', 'decks': [a, b], 'starter': st, 'game': 2 + int(r.integers(2)),
                      'wins': [1, 0] if r.integers(2) else [0, 1], 'dealt': [bool(r.integers(2)), bool(r.integers(2))],
                      'result': [y, 1 - y], 'iter': i * 6 // 3000})
    lst = [[f'c{i}', k] for i, k in enumerate(COPIES)]
    specs = {'k1': spec_line('k1', (lst, MAIN), (lst, MAIN))}
    P = MU.the_pairing(specs, games)
    assert P.mirror
    cfg = {'spec': json.dumps({'dealt_decks': {'p': 0.85, 'swaps': [0, 4], 'per_seat': True}})}
    MU.check_table, keep = (lambda spec, table: None), MU.check_table
    try:
        out, rep, tables, changes, per_copy = MU._mirror_version(cfg, games, specs, P, 5, 0, 'v0005', 2, 0.0, 2, 300,
                                                                  lambda m: None, -1.0, 2.0, 5)
    finally:
        MU.check_table = keep
    members, rep0 = MU.select_and_fit(games, MAIN, 2, 0.0, 2, seed=5, log=lambda m: None, members_n=5)
    table, detail = MU.choice_table(members, [x[0] for x in lst], COPIES, MAIN, (0, 4), 'v0005', worlds=300, seed=5,
                                    log=lambda m: None, z=2.0)
    assert out['members'] == members and out['table'] == table and out['table_detail'] == detail
    assert {k: v for k, v in rep.items()} == rep0 and tables == [table] and 'tables' not in out


def big_tables():
    """Real tables of the Delver and Doomsday 75s (each list its 75, every context the main)."""
    out = []
    for name in (D.IZZET_DELVER, D.LEGACY_DOOMSDAY):
        d = D.load(name)
        main, side = D.names(d['main']), D.names(d['sideboard'])
        names = list(dict.fromkeys(main + side))
        copies = [(main + side).count(n) for n in names]
        base = [main.count(n) for n in names]
        out.append({'version': 'v0009', 'list': [[n, k] for n, k in zip(names, copies)],
                    'choices': [[g, f, l, o, base] for (g, f, l, o) in MU.contexts()]})
    return out


def test_tables_go_on_the_decks_the_engine_reads_them_from():
    tD, tDD = big_tables()
    meta = D.pairing_spec(D.IZZET_DELVER, D.LEGACY_DOOMSDAY, dealt_decks=A.PAIR_DEALT)
    sp = json.loads(MU.with_tables(meta, [tD, tDD]))
    assert [e['table']['list'] for e in sp['metagame']['decks']] == [tD['list'], tDD['list']]
    assert MU.has_tables(sp) and not MU.has_tables(json.loads(meta))
    fixed = D.pair_spec(D.LEGACY_DOOMSDAY, D.IZZET_DELVER, fmt='bo3', game_episodes=True)
    fs = json.loads(MU.with_tables(fixed, [tD, tDD]))
    assert fs['decks']['p0_table'] == tDD and fs['decks']['p1_table'] == tD and 'sideboard_table' not in fs
    mirror = D.mirror_bo3_spec(D.IZZET_DELVER, game_episodes=True)
    ms = json.loads(MU.with_tables(mirror, [tD]))
    assert ms['sideboard_table'] == tD and 'p0_table' not in ms['decks'], 'a fixed mirror: pb3\'s one table'
    mm = json.loads(MU.with_tables(D.pairing_spec(D.IZZET_DELVER, D.IZZET_DELVER), [tD]))
    assert mm['metagame']['decks'][0]['table'] == tD, 'a mirror metagame: the deck\'s entry'
    with pytest.raises(ValueError, match='fits no deck'):
        MU.with_tables(D.pairing_spec(D.IZZET_DELVER, D.SNEAK_SHOW), [tD, tDD])
    with pytest.raises(ValueError, match='2 choice tables'):
        MU.with_tables(mirror, [tD, dict(tD, version='v0010')])
    assert MU.version_tables({'table': tD}) == [tD] and MU.version_tables({'table': None, 'tables': [tD, tDD]}) == [tD, tDD]
    assert MU.version_tables({'table': None}) is None
    # the evaluation spec drops every table (and the deal and the forced find)
    for s in (sp, fs, ms, mm):
        ev = json.loads(evaldecks.eval_spec(json.dumps(dict(s, forced_find={'spell': 'Doomsday', 'card': "Thassa's Oracle", 'p': 1}))))
        assert not MU.has_tables(ev) and 'dealt_decks' not in ev and 'forced_find' not in ev


def test_the_engine_reads_the_placed_tables():
    E = pytest.importorskip('mtgz._engine')
    tD, tDD = big_tables()
    meta = D.pairing_spec(D.IZZET_DELVER, D.LEGACY_DOOMSDAY, dealt_decks=A.PAIR_DEALT)
    MU.check_tables(meta, [tD, tDD])
    MU.check_tables(D.pair_spec(D.LEGACY_DOOMSDAY, D.IZZET_DELVER, fmt='bo3', game_episodes=True), [tD, tDD])
    MU.check_tables(D.mirror_bo3_spec(D.IZZET_DELVER, game_episodes=True), [tD])
    MU.check_tables(D.pairing_spec(D.IZZET_DELVER, D.IZZET_DELVER, dealt_decks=A.PAIR_DEALT), [tD])
    E.mtg_spec_check(evaldecks.eval_spec(MU.with_tables(meta, [tD, tDD])))


class FakeRun:
    def __init__(self, run, spec, **cfg):
        self.run = run
        self.cfg = dict(MU.SETTINGS, spec=spec, **cfg)
        self.lines = []

    def log(self, m):
        self.lines.append(m)


def test_the_hook_publishes_one_table_per_deck_into_the_metagame_spec(tmp_path):
    run = str(tmp_path / 'r')
    os.makedirs(run)
    spec = D.pairing_spec(D.IZZET_DELVER, D.LEGACY_DOOMSDAY, dealt_decks=A.PAIR_DEALT)
    R = FakeRun(run, spec, matchup_every=4, matchup_min_window=8, matchup_val_iters=2, matchup_window=32)
    tD, tDD = big_tables()
    os.makedirs(MU.mdir(run), exist_ok=True)
    with open(os.path.join(MU.mdir(run), 'start.json'), 'w') as f:
        json.dump({'first_iter': 1, 'time': 0}, f)
    with open(os.path.join(MU.mdir(run), 'v0009.json'), 'w', encoding='utf-8') as f:
        json.dump({'format': MU.FORMAT, 'model': 'pair', 'table': None, 'tables': [tD, tDD]}, f)
    s = json.loads(MU.selfplay_spec(R, 10))
    assert [e['table'] for e in s['metagame']['decks']] == [tD, tDD]
    assert json.loads(MU.selfplay_spec(R, 10)) == s, 'one answer per iteration'
    assert any('choice tables v0009.json' in x for x in R.lines)
    # a run spec with a table of its own is refused (the retrain writes them)
    bad = FakeRun(run, MU.with_tables(spec, [tD]), matchup_every=4)
    with pytest.raises(ValueError, match='choice table of its own'):
        MU.selfplay_spec(bad, 11)


def test_the_specialist_specs_and_presets():
    from zero import run as R
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': 'legacy-pair-bo3-varied'})
    sp = json.loads(cfg['spec'])
    assert [d['name'] for d in sp['metagame']['decks']] == [D.IZZET_DELVER, D.LEGACY_DOOMSDAY]
    assert sp['metagame']['weights'] == [[0, 1], [1, 0]] and sp['format'] == 'bo3' and sp['game_episodes'] is True
    assert 'start_game' not in sp, 'game 1 played'
    assert sp['dealt_decks'] == {'p': 0.85, 'swaps': [0, 15], 'per_seat': True}
    assert sp['forced_find'] == {'spell': 'Doomsday', 'card': "Thassa's Oracle", 'p': 0.0, 'unless_in_hand': True}
    ev = json.loads(cfg['eval_spec'])
    assert 'dealt_decks' not in ev and 'forced_find' not in ev and ev['metagame'] == sp['metagame']
    # the meta3 run's network and training settings; pb3's matchup settings and best of three's deck tilt
    assert (cfg['hidden'], cfg['blocks'], cfg['lr'], cfg['lr_min'], cfg['reuse'], cfg['window']) == (128, 8, 1e-4, 1e-4, 2.0, 6)
    assert (cfg['min_window_samples'], cfg['sims'], cfg['sims_fast'], cfg['p_full']) == (30000, 64, 16, 0.25)
    assert (cfg['value_q'], cfg['selfplay_view_samples'], cfg['aux_deck_w'], cfg['aux_belief_w']) == (0.6, 0.5, 0.5, 0.25)
    assert cfg['selfplay_belief_tilts'] == {'deck': 1.0} and cfg['eval_belief_tilts'] == {'deck': 1.0}
    assert (cfg['matchup_every'], cfg['matchup_window'], cfg['matchup_min_window'], cfg['matchup_half_life']) == (4, 32, 8, 16.0)
    assert (cfg['matchup_table_min_gain'], cfg['matchup_swap_z'], cfg['matchup_members'], cfg['matchup_worlds']) == (0.005, 2.0, 20, 8000)
    assert cfg['forced_find_schedule'] == {} and cfg['games'] == A.PAIR_GAMES
    smoke = R.resolve_config(ns, {'preset': 'legacy-pair-bo3-varied-smoke'})
    assert smoke['matchup_every'] == 1 and json.loads(smoke['spec'])['forced_find']['p'] == 1.0
    assert smoke['infer_device'] == 'cpu' and smoke['hidden'] == 32
    # each pairing's settings: the forced find only beside a deck that holds Doomsday and Thassa's Oracle
    for a, b, ff in ((D.IZZET_DELVER, D.IZZET_DELVER, False), (D.IZZET_DELVER, D.SNEAK_SHOW, False),
                     (D.IZZET_DELVER, D.LEGACY_DOOMSDAY, True), (D.SNEAK_SHOW, D.LEGACY_DOOMSDAY, True)):
        s = A.pair_varied_settings(a, b)
        x = json.loads(s['spec'])
        assert ('forced_find' in x) == ff and x['metagame']['weights'] == ([[1]] if a == b else [[0, 1], [1, 0]])
        assert s['matchup_every'] == 4 and json.loads(s['eval_spec']) == json.loads(evaldecks.eval_spec(s['spec']))


def test_every_card_of_the_stew_75s_is_supported_in_best_of_three():
    # (the first stew's field: Delver, Doomsday, Sneak and Show, and Hogaak - added to the live stew, docs/ENGINE.md 220)
    E = pytest.importorskip('mtgz._engine')
    field = D.META3 + (D.LEGACY_HOGAAK,)
    for a in field:
        for b in field:
            sp, ev = A.pair_varied_specs(a, b)
            E.mtg_spec_check(sp)
            E.mtg_spec_check(ev)
            E.mtg_spec_check(D.pair_spec(a, b, fmt='bo3', game_episodes=True, dealt_decks=A.PAIR_DEALT))


def test_a_61_card_main_gets_tables_of_61s_the_engine_plays():
    # docs/ENGINE.md 272: a best-of-three main of at least 60 - the Lands deck's 61 against the Delver 60. The matchup
    # model's tables are of each deck's own size (its registered main's: every legal deck after sideboarding), and the
    # engine takes them on the pairing's spec, the fixed spec and the Lands mirror's.
    E = pytest.importorskip('mtgz._engine')
    decks = []
    for k, name in enumerate((D.LEGACY_LANDS, D.IZZET_DELVER)):
        d = D.load(name)
        main, side = D.names(d['main']), D.names(d['sideboard'])
        names = list(dict.fromkeys(main + side))
        decks.append({'name': d['name'], 'list': [[n, (main + side).count(n)] for n in names],
                      'main': [main.count(n) for n in names], 'index': k})
    assert (sum(decks[0]['main']), sum(decks[1]['main'])) == (61, 60)
    P = MU.Pairing(decks, False)
    # the Lands deck gains from its sideboard's Torpor Orb, the Delver from nothing: Torpor Orbs come in for Lands
    torpor = [n for n, _ in decks[0]['list']].index('Torpor Orb')
    wC = [2.0 if i == torpor else 0.0 for i in range(len(decks[0]['list']))]
    mem = pair_members(wC, [0.0] * len(decks[1]['list']))
    tables, _ = MU.pair_choice_tables(mem, P, (0, 4), 'v0272', worlds=200, seed=3, log=lambda s: None)
    for t, d in zip(tables, decks):
        copies = [k for _, k in d['list']]
        for c in t['choices']:
            assert sum(c[4]) == sum(d['main']) and all(0 <= a <= b for a, b in zip(c[4], copies))
    assert all(c[4][torpor] == 3 for c in tables[0]['choices']), 'every Torpor Orb in, the deck still 61'
    MU.check_tables(D.pairing_spec(D.LEGACY_LANDS, D.IZZET_DELVER, dealt_decks=A.PAIR_DEALT), tables)
    MU.check_tables(D.pair_spec(D.LEGACY_LANDS, D.IZZET_DELVER, fmt='bo3', game_episodes=True), tables)
    MU.check_tables(D.mirror_bo3_spec(D.LEGACY_LANDS, game_episodes=True), tables[:1])
    # a table of 60s over the Lands 76's list is not the Lands deck's: refused
    bad = json.loads(json.dumps(tables[0]))
    for c in bad['choices']:
        c[4][[n for n, _ in decks[0]['list']].index('Forest')] -= 1
    with pytest.raises(Exception, match="61 cards here, the table's 60"):
        MU.check_tables(D.pair_spec(D.LEGACY_LANDS, D.IZZET_DELVER, fmt='bo3', game_episodes=True), [bad, tables[1]])
