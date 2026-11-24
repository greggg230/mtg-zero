"""Dealt decks around the seat's choice table (docs/ENGINE.md 261, the spec's `dealt_decks.around`): the helpers write
the option only when asked (no default moves), the engine parses it and refuses anything but "main" / "table", and the
matchup model's belief about a DEALT opponent follows the engine's deal exactly - `sample_around` (the constructed
prior's sampler with the base as its main) against the engine's own dealt 60s on a fixed context, and the choice
tables' opp_dealt contexts drawn around the other deck's own opp_dealt 0 choice for the opponent's context."""
import json
import os

import numpy as np
import pytest

import mtgz.az as A
from mtgz.az import decklists, evaldecks
from mtgz.az import games as G
from mtgz.az import matchup as MU

AROUND = {'p': 1.0, 'swaps': [1, 4], 'per_seat': True, 'around': 'table'}


def test_the_helpers_write_around_only_when_asked():
    # no default moves: the presets' and helpers' specs carry no "around"
    for name in ('legacy-pair-bo3-varied', 'legacy-pair-bo3-varied-smoke', 'delver-mirror-bo3-varied'):
        assert 'around' not in json.loads(A.PRESETS[name]['spec'])['dealt_decks'], name
    sp, ev = A.pair_varied_specs(decklists.IZZET_DELVER, decklists.LEGACY_DOOMSDAY)
    assert 'around' not in json.loads(sp)['dealt_decks']
    sp, ev = A.pair_varied_specs(decklists.IZZET_DELVER, decklists.LEGACY_DOOMSDAY, around='table')
    dd = json.loads(sp)['dealt_decks']
    assert dd == dict(A.PAIR_DEALT, around='table') and 'dealt_decks' not in json.loads(ev)
    assert MU.dealt_around(sp) == 'table' and MU.dealt_around(ev) == 'main'
    s = A.pair_varied_settings(decklists.IZZET_DELVER, decklists.IZZET_DELVER, smoke=True,
                               dealt_decks={'p': 0.5, 'swaps': [1, 8], 'per_seat': True}, around='table')
    assert json.loads(s['spec'])['dealt_decks'] == {'p': 0.5, 'swaps': [1, 8], 'per_seat': True, 'around': 'table'}
    with pytest.raises(ValueError, match='around'):
        A.pair_varied_specs(decklists.IZZET_DELVER, decklists.LEGACY_DOOMSDAY, around='tables')
    m = json.loads(decklists.mirror_bo3_spec(dealt_decks=AROUND))
    assert m['dealt_decks']['around'] == 'table'
    p = json.loads(decklists.pair_spec(decklists.IZZET_DELVER, decklists.LEGACY_DOOMSDAY, fmt='bo3', dealt_decks=AROUND))
    assert p['dealt_decks']['around'] == 'table'
    # the engine parses them (around table without any table: every base is the main) and drops them for evaluation
    E = G.engine()
    for spec in (sp, json.dumps(m), json.dumps(p)):
        E.mtg_spec_check(spec)
        assert 'dealt_decks' not in json.loads(evaldecks.eval_spec(spec))
    bad = dict(m, dealt_decks=dict(m['dealt_decks'], around='Table'))
    with pytest.raises(Exception, match='dealt_decks.around'):
        E.mtg_spec_check(json.dumps(bad))
    with pytest.raises(ValueError, match='around'):
        MU.dealt_around(bad)


def _table(lists, main, seed):
    """A choice table of the 75 `lists` ([[name, copies], ...]) whose every context is the main with 2..6 random
    one-for-one swaps (seeded)."""
    r = np.random.default_rng(seed)
    copies = [k for _, k in lists]
    choices = []
    for (g, f, l, o) in MU.contexts():
        c = list(main)
        for _ in range(int(r.integers(2, 7))):
            outs = [i for i in range(len(c)) if c[i] > 0]
            ins = [i for i in range(len(c)) if c[i] < copies[i]]
            a, b = int(r.choice(outs)), int(r.choice(ins))
            if a != b:
                c[a] -= 1
                c[b] += 1
        choices.append([g, f, l, o, c])
    return {'version': f'around{seed}', 'list': [list(x) for x in lists], 'choices': choices}


def _logged(tmp_path, spec, seeds):
    """The game log's spec line and bo3 lines of matches `seeds` of `spec` played by the bindings (first legal action)."""
    E = G.engine()
    path = str(tmp_path / 'games' / 'games-1.jsonl')
    E.game_log(path)
    try:
        for seed in seeds:
            g = E.Game('mtg', spec, 2, seed)
            while g.to_move is not None:
                g.step(list(g.legal())[0])
    finally:
        E.game_log(None)
    lines = [json.loads(x) for x in open(path, encoding='utf-8')]
    return [x for x in lines if x.get('kind') == 'bo3-spec'], [x for x in lines if x.get('kind') == 'bo3']


def test_the_samplers_dealt_opponent_is_the_engines_deal_on_a_fixed_context(tmp_path):
    """The Delver mirror's post-sideboard matches, both seats dealt around pb3's one table (p 1, swaps [1, 4]): the
    engine's game-2 deals of the seat that sideboards first after losing game 1 - one context, (2, 1, -1, 1) - against
    `sample_around` with that context's 60 as the base: every engine deal within the range of the base, the swap
    counts and every entry's mean copies equal within sampling error."""
    probe = decklists.mirror_bo3_spec(max_turns=2, game_episodes=True, start_game=2, dealt_decks=AROUND)
    specs, _ = _logged(tmp_path / 'probe', probe, [0])
    lists, main = specs[0]['lists'][0], specs[0]['mains'][0]
    table = _table(lists, main, 7)
    spec = MU.with_table(probe, table)
    specs, games = _logged(tmp_path / 'run', spec, range(3000))
    assert specs[0]['dealt_decks']['around'] == 'table' and specs[0]['table'] == 'around7'
    ctx = (2, 1, -1, 1)
    base = next(c[4] for c in table['choices'] if tuple(c[:4]) == ctx)
    eng = []
    for x in games:
        if x['game'] != 2:
            continue
        for q in (0, 1):
            lead = int(np.sign(x['wins'][q] - x['wins'][1 - q]))
            if (2, int(x['chooser'] == q), lead, int(x['dealt'][1 - q])) == ctx:
                assert x['mode'][q] == 'dealt'
                eng.append(x['decks'][q])
    eng = np.asarray(eng)
    n = len(eng)
    assert n > 2000, n
    copies = [k for _, k in lists]
    py = MU.sample_around(copies, base, main, 1, 4, 20000, np.random.default_rng(3)) + np.asarray(main)
    b = np.asarray(base)
    se, sp = np.maximum(eng - b, 0).sum(1), np.maximum(py - b, 0).sum(1)
    assert set(se.tolist()) <= {1, 2, 3, 4} and set(sp.tolist()) == {1, 2, 3, 4}
    for k in (1, 2, 3, 4):
        fe, fp = (se == k).mean(), (sp == k).mean()
        assert abs(fe - 0.25) < 5 * np.sqrt(0.25 * 0.75 / n) and abs(fp - 0.25) < 5 * np.sqrt(0.25 * 0.75 / len(sp)), (k, fe, fp)
    me, mp, var = eng.mean(0), py.mean(0), py.var(0)
    tol = 5 * np.sqrt(var / n + var / len(py)) + 1e-9
    assert (np.abs(me - mp) <= tol).all(), [(lists[i][0], me[i], mp[i]) for i in np.flatnonzero(np.abs(me - mp) > tol)]
    # (not the main's prior: around the main the same entries would centre on the main)
    assert np.abs(mp - np.asarray(main)).max() > 0.5


def test_the_opponents_deal_context_is_the_deciders_mirrored(tmp_path):
    """Where one seat chooses and the other is dealt (p 0.5 per seat), the dealt seat's deal context in the game log is
    `opponent_context` of the choosing seat's: the same game, the other sideboarding position, the opposite lead, its
    own opp_dealt 0 - what the tables' opp_dealt 1 contexts are computed against."""
    dd = dict(AROUND, p=0.5)
    specs, games = _logged(tmp_path, decklists.mirror_bo3_spec(max_turns=2, game_episodes=True, start_game=2,
                                                               dealt_decks=dd), range(400))
    seen = 0
    for x in games:
        for q in (0, 1):
            o = 1 - q
            if x['dealt'][o] and not x['dealt'][q]:
                lead = int(np.sign(x['wins'][q] - x['wins'][o]))
                mine = (x['game'], int(x['chooser'] == q), lead)
                theirs = (x['game'], int(x['chooser'] == o), int(np.sign(x['wins'][o] - x['wins'][q])), int(x['dealt'][q]))
                assert MU.opponent_context(*mine) == theirs
                seen += 1
    assert seen > 50


def _members(E, dim=0, seed=0):
    r = np.random.default_rng(seed)
    return [{'w': r.normal(size=E).tolist(), 'c': r.normal(size=4).tolist(), 'U': [], 'M': [], 'dim': dim}
            for _ in range(3)]


CC, MC = [4, 4, 2, 3, 2], [4, 3, 1, 0, 0]
CD, MD = [3, 4, 2, 2], [3, 3, 2, 0]


def _record(monkeypatch):
    calls = []
    real = MU.sample_around

    def spy(copies, base, main, lo, hi, n, rng):
        calls.append((list(copies), list(base), list(main), lo, hi))
        return real(copies, base, main, lo, hi, n, rng)
    monkeypatch.setattr(MU, 'sample_around', spy)
    return calls


def test_a_mirror_tables_dealt_contexts_are_against_worlds_around_its_own_choices(monkeypatch):
    names = [f'c{i}' for i in range(len(CC))]
    mem = _members(len(CC), seed=4)
    plain, _ = MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None)
    assert plain == MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None,
                                    around='main')[0], 'around main is the old table'
    calls = _record(monkeypatch)
    t, detail = MU.choice_table(mem, names, CC, MC, (1, 2), 'v1', worlds=300, seed=5, log=lambda s: None, around='table')
    by = {tuple(c[:4]): c[4] for c in t['choices']}
    assert [tuple(c[:4]) for c in t['choices']] == MU.contexts()
    want = {tuple(by[MU.opponent_context(g, f, l)]) for (g, f, l, o) in MU.contexts() if o == 1}
    assert {tuple(c[1]) for c in calls} == want and len(calls) == len(want), 'one world set per distinct base'
    assert all(c[0] == CC and c[2] == MC and (c[3], c[4]) == (1, 2) for c in calls)
    # the opp_dealt 0 contexts are the plain table's (the same worlds of the full prior, drawn first)
    assert all(by[k] == c[4] for c in plain['choices'] for k in [tuple(c[:4])] if k[3] == 0)


def test_pair_tables_dealt_contexts_are_against_worlds_around_the_other_decks_choices(monkeypatch):
    P = MU.Pairing([{'name': 'C', 'list': [[f'c{i}', k] for i, k in enumerate(CC)], 'main': MC, 'index': 0},
                    {'name': 'D', 'list': [[f'd{i}', k] for i, k in enumerate(CD)], 'main': MD, 'index': 1}], False)
    r = np.random.default_rng(6)
    mem = [{'wC': r.normal(size=len(CC)).tolist(), 'wD': r.normal(size=len(CD)).tolist(),
            'c': [0.1, 0.2, 0, 0, 0, 0, 0], 'UC': [], 'UD': [], 'W': [], 'dim': 0} for _ in range(3)]
    plain, _ = MU.pair_choice_tables(mem, P, (1, 2), 'v2', worlds=300, seed=1, log=lambda s: None)
    assert plain == MU.pair_choice_tables(mem, P, (1, 2), 'v2', worlds=300, seed=1, log=lambda s: None, around='main')[0]
    calls = _record(monkeypatch)
    tables, _ = MU.pair_choice_tables(mem, P, (1, 2), 'v2', worlds=300, seed=1, log=lambda s: None, around='table')
    by = [{tuple(c[:4]): c[4] for c in t['choices']} for t in tables]
    want = set()
    for role in (0, 1):
        for (g, f, l, o) in MU.contexts():
            if o == 1:
                want.add((role, tuple(by[1 - role][MU.opponent_context(g, f, l)])))
    got = {(0 if c[0] == CD else 1, tuple(c[1])) for c in calls}
    assert got == want and len(calls) == len(want)
    assert all((c[0], c[2]) in ((CD, MD), (CC, MC)) for c in calls)
