"""The in-run matchup model and its choice table (docs/ENGINE.md 168, mtgz.az.matchup): the constructed prior's exact
sampler, the pairwise model's antisymmetry and fit, the exact strength optimum, the choice table's 24 contexts, the game
log reader, and the self-play hook's versions (one answer per iteration, a retrain launched when due)."""
import itertools
import json
import os

import numpy as np
import pytest

from mtgz.az import matchup as MU

# a small 75: (copies, main) per entry - 3 entries in the main, 2 sideboard entries
COPIES = [4, 4, 2, 3, 2]
MAIN = [4, 3, 1, 0, 0]


def decks_by_swaps(copies, main):
    """Every legal deck (the main's size) of the small 75 by its swap count (brute force)."""
    out = {}
    n = sum(main)
    for c in itertools.product(*[range(k + 1) for k in copies]):
        if sum(c) == n:
            s = sum(max(0, a - b) for a, b in zip(c, main))
            out.setdefault(s, []).append(c)
    return out


def test_the_prior_table_counts_the_decks_exactly():
    by = decks_by_swaps(COPIES, MAIN)
    S = max(by)
    t = MU.prior_table(COPIES, MAIN, 0, S)
    for s in range(S + 1):
        assert t[0][s][s] == len(by.get(s, [])), s


def test_the_prior_sampler_is_uniform_over_the_swap_count_then_the_decks():
    by = decks_by_swaps(COPIES, MAIN)
    S = max(by)
    rng = np.random.default_rng(1)
    n = 24000
    x = MU.sample_prior(COPIES, MAIN, 0, S, n, rng)
    assert (x.sum(1) == sum(MAIN)).all() and (x <= np.asarray(COPIES)).all()
    s = np.maximum(x - np.asarray(MAIN), 0).sum(1)
    for k in range(S + 1):
        f = (s == k).mean()
        assert abs(f - 1 / (S + 1)) < 5 * np.sqrt((1 / (S + 1)) / n), (k, f)
    # within a swap count, every deck equally likely
    k = 2
    sub = [tuple(r) for r in x[s == k]]
    decks = by[k]
    m = len(sub) / len(decks)
    for d in decks:
        assert abs(sub.count(d) - m) < 5 * np.sqrt(m) + 2, (d, sub.count(d), m)
    # the restricted range (a dealt seat's, ENGINE.md 154)
    y = MU.sample_prior(COPIES, MAIN, 1, 2, 2000, rng)
    sy = np.maximum(y - np.asarray(MAIN), 0).sum(1)
    assert set(sy.tolist()) == {1, 2}
    # big counts (the Delver 75 reaches 10^10 decks): the exact draw of a big integer
    assert 0 <= MU._below(rng, 10 ** 30) < 10 ** 30


def member(E, dim, seed=0):
    r = np.random.default_rng(seed)
    return {'w': r.normal(size=E).tolist(), 'c': r.normal(size=4).tolist(), 'U': r.normal(size=(E, dim)).tolist() if dim else [],
            'M': r.normal(size=(dim, dim)).tolist() if dim else [], 'dim': dim}


def test_the_model_is_antisymmetric():
    E = 5
    r = np.random.default_rng(3)
    dA, dB = r.integers(-2, 3, size=(50, E)).astype(float), r.integers(-2, 3, size=(50, E)).astype(float)
    ctx = r.choice([-1.0, 0.0, 1.0], size=(50, 4))
    for dim in (0, 2, 3):
        m = MU._np(member(E, dim, seed=dim))
        assert np.allclose(MU.z_of(m, dA, dB, ctx), -MU.z_of(m, dB, dA, -ctx), atol=1e-12)


def synthetic(n, w, seed=0, pd=0.4):
    r = np.random.default_rng(seed)
    games = []
    E = len(w)
    for i in range(n):
        dA, dB = r.integers(-1, 2, size=E), r.integers(-1, 2, size=E)
        st = int(r.integers(2))
        z = float(np.dot(w, dA - dB)) + (pd if st == 0 else -pd)
        y = float(r.random() < 1 / (1 + np.exp(-z)))
        games.append({'decks': [(np.asarray(MAIN) + dA).tolist(), (np.asarray(MAIN) + dB).tolist()], 'starter': st,
                      'game': 2 + int(r.integers(2)), 'wins': [1, 0] if r.integers(2) else [0, 1],
                      'dealt': [bool(r.integers(2)), bool(r.integers(2))], 'result': [y, 1 - y], 'iter': i * 6 // n})
    return games


def test_the_fit_recovers_planted_strengths_and_beats_play_draw():
    w = np.array([0.6, -0.4, 0.0, 0.3, -0.5])
    games = synthetic(6000, w, seed=5)
    members, rep = MU.select_and_fit(games, MAIN, val_iters=2, half_life=0.0, threads=2, log=lambda m: None)
    got = np.asarray(rep['per_copy'])
    wc = w - np.dot(w, MAIN) / sum(MAIN)               # (the report centres w at the main's average copy)
    assert np.abs(got - wc).max() < 0.12, (got, wc)
    assert rep['gain_nats'] > 0.02 and rep['val_logloss'] < rep['val_logloss_pd']
    assert abs(rep['ctx'][0] - 0.4) < 0.1, rep['ctx']
    assert len(members) == MU.MEMBERS and rep['selected']['dim'] in MU.DIMS


def test_the_strength_optimum_is_exact():
    by = decks_by_swaps(COPIES, MAIN)
    for seed in range(20):
        m = member(len(COPIES), 0, seed)
        c = MU.strength_optimum([m], COPIES, MAIN)
        best = max(np.dot(m['w'], d) for ds in by.values() for d in ds)
        assert abs(np.dot(m['w'], c) - best) < 1e-12 and sum(c) == sum(MAIN)


def test_the_choice_table_has_24_legal_contexts_and_follows_the_model():
    names = [f'c{i}' for i in range(len(COPIES))]
    m = {'w': [0.0, 0.0, 0.0, 2.0, 0.0], 'c': [0.3, 0.0, 0.0, 0.0], 'U': [], 'M': [], 'dim': 0}
    table, detail = MU.choice_table([m], names, COPIES, MAIN, (0, 4), 'v0001', worlds=300, seed=1, log=lambda s: None)
    assert table['version'] == 'v0001' and len(table['choices']) == 24
    assert [tuple(c[:4]) for c in table['choices']] == MU.contexts()
    for c in table['choices']:
        deck = c[4]
        assert sum(deck) == sum(MAIN) and all(0 <= a <= b for a, b in zip(deck, COPIES))
        assert deck[3] == 3, 'every copy of the strong sideboard card in'
    # a matchup term: the climb improves on both starts, never below them
    m2 = member(len(COPIES), 2, seed=4)
    t2, d2 = MU.choice_table([m2], names, COPIES, MAIN, (0, 4), 'v0002', worlds=300, seed=2, log=lambda s: None)
    for d in d2:
        assert d['F'] >= max(c['F'] for c in d['climbs']) - 1e-12 >= d['F_main'] - 1e-12


def write_log(run, games, spec_extra=None):
    os.makedirs(os.path.join(run, 'games'), exist_ok=True)
    sp = {'v': 1, 'kind': 'bo3-spec', 'spec': 'k1', 'lists': [[[f'c{i}', k] for i, k in enumerate(COPIES)]] * 2,
          'mains': [MAIN, MAIN], 'dealt_decks': {'p': 0.85, 'swaps': [0, 4], 'per_seat': True}, 'table': None,
          'start_game': 2, 'max_turns': 60}
    sp.update(spec_extra or {})
    ev = dict(sp, spec='k2', dealt_decks=None)
    with open(os.path.join(run, 'games', 'games-1.jsonl'), 'w', encoding='utf-8') as f:
        f.write(json.dumps(sp) + '\n' + json.dumps(ev) + '\n')
        for g in games:
            f.write(json.dumps(g) + '\n')
    with open(os.path.join(run, 'metrics.jsonl'), 'w', encoding='utf-8') as f:
        for it in range(1, 8):
            f.write(json.dumps({'iter': it, 'time': 100.0 * it}) + '\n')


def game_line(t, decks, starter=0, result=(1.0, 0.0), spec='k1', game=2):
    return {'v': 1, 'kind': 'bo3', 'spec': spec, 'seed': 1, 'game': game, 'starter': starter, 'chooser': starter,
            'wins': [0, 1], 'dealt': [True, False], 'mode': ['dealt', 'chosen'], 'decks': decks,
            'result': list(result) if result is not None else None, 'aborted': result is None, 'turns': 9,
            'post_board': True, 'time': t}


def test_the_game_log_reader(tmp_path):
    run = str(tmp_path / 'r')
    lines = [game_line(150.0, [MAIN, MAIN]), game_line(250.0, [MAIN, MAIN], result=None),
             game_line(260.0, [MAIN, MAIN], spec='k2'), game_line(901.0, [MAIN, MAIN])]
    write_log(run, lines)
    games, specs = MU.load_games(run)
    assert [g['iter'] for g in games] == [2, None], 'the aborted and the evaluation game left out; by time'
    assert MU.load_games(run, 1, 3)[0][0]['time'] == 150.0
    lst, main = MU.the_75(specs, games)
    assert main == MAIN and [x[1] for x in lst] == COPIES
    cen = MU.census(run)
    assert cen['by']['chosen game 2']['seats'] == 2


class FakeRun:
    def __init__(self, run, **cfg):
        self.run = run
        spec = {'constructed': True, 'format': 'bo3', 'dealt_decks': {'p': 0.85, 'swaps': [0, 15], 'per_seat': True}}
        self.cfg = dict(MU.SETTINGS, spec=json.dumps(spec), **cfg)
        self.lines = []

    def log(self, m):
        self.lines.append(m)


def test_the_hook_answers_once_per_iteration_and_launches_when_due(tmp_path, monkeypatch):
    run = str(tmp_path / 'r')
    os.makedirs(run)
    R = FakeRun(run, matchup_every=0)
    assert MU.selfplay_spec(R, 3) is None, 'off'
    R = FakeRun(run, matchup_every=2, matchup_min_window=2, matchup_val_iters=1, matchup_window=8)
    started = []

    class P:
        pid = 4242

        def poll(self):
            return 0
    monkeypatch.setattr(MU.subprocess, 'Popen', lambda cmd, **k: started.append(cmd) or P())
    assert MU.selfplay_spec(R, 1) is None, 'no version yet: the run\'s spec'
    assert MU.selfplay_spec(R, 1) is None and started == []
    assert MU.selfplay_spec(R, 2) is None and started == [], 'b = 1: not a multiple of 2'
    assert MU.selfplay_spec(R, 3) is None and len(started) == 1, 'b = 2: due (iterations 1-2 logged)'
    assert started[0][3:5] == ['retrain', run] and started[0][started[0].index('--from-iter') + 1] == '1'
    # a published version is used from the next iteration that asks; the answer for an iteration never changes
    table = {'version': 'v0002', 'list': [['x', 1]], 'choices': []}
    with open(os.path.join(MU.mdir(run), 'v0002.json'), 'w', encoding='utf-8') as f:
        json.dump({'format': MU.FORMAT, 'table': table}, f)
    os.makedirs(os.path.join(MU.job_dir(run, 2)), exist_ok=True)
    with open(os.path.join(MU.job_dir(run, 2), 'status.json'), 'w') as f:
        json.dump({'ok': True, 'games': 10, 'window': [1, 2], 'val_logloss': 0.6, 'val_logloss_pd': 0.69,
                   'gain_nats': 0.09, 'seconds': 1}, f)
    assert MU.selfplay_spec(R, 3) is None, 'iteration 3 asked before the version: it keeps its answer'
    s4 = json.loads(MU.selfplay_spec(R, 4))
    assert s4['sideboard_table'] == table and json.loads(MU.selfplay_spec(R, 4)) == s4
    assert any('matchup model v0002 published' in x for x in R.lines)
    assert any('choice table v0002.json' in x for x in R.lines)
    used = MU._rows(os.path.join(MU.mdir(run), 'used.jsonl'))
    assert [(u['iter'], u['version']) for u in used] == [(1, None), (2, None), (3, None), (4, 'v0002.json')]
    # a spec without dealt decks, or with a table of its own, is refused
    bad = FakeRun(run, matchup_every=2)
    bad.cfg['spec'] = json.dumps({'constructed': True, 'format': 'bo3'})
    with pytest.raises(ValueError, match='dealt decks'):
        MU.selfplay_spec(bad, 9)


def test_the_presets():
    from zero import run as R
    import mtgz.az  # noqa: F401
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': 'delver-mirror-bo3-varied'})
    sp = json.loads(cfg['spec'])
    assert sp['dealt_decks'] == {'p': 0.85, 'swaps': [0, 15], 'per_seat': True} and 'sideboard_table' not in sp
    ev = json.loads(cfg['eval_spec'])
    assert 'dealt_decks' not in ev
    assert cfg['matchup_every'] == 4 and cfg['matchup_window'] == 32 and cfg['matchup_half_life'] == 16.0
    smoke = R.resolve_config(ns, {'preset': 'delver-bo3-varied-smoke'})
    assert smoke['matchup_every'] == 1 and json.loads(smoke['spec'])['dealt_decks']['per_seat'] is True
