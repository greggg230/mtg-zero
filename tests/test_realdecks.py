"""Win % against real players' decks (docs/ENGINE.md 113, mtgz.sealed.realdecks): the field and the test pools, the
paired games, the builders, the command line, the run's extra evaluation (local and on a remote's mirror)."""
import collections
import json
import os
import subprocess
import sys

import pytest

from mtgz.sealed import cards as C
from mtgz.sealed import compare as K
from mtgz.sealed import realdecks as RD

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FAST = {'sealed': {'set': 'eoe', 'collation': 'arena', 'best_share': 0.5}, 'max_turns': 8}


def _smoke_run(run, it, **kw):
    """A run directory of eoe-smoke's settings (the games FAST) with a fresh network as checkpoint `it` -> (Run, path)."""
    from zero.net import save
    from zero.run import DEFAULTS, PRESETS, Run, model_path
    cfg = dict(DEFAULTS, **PRESETS['eoe-smoke'])
    cfg.update(eval_spec=json.dumps(dict(FAST, decks='eval', eval_seed=1, eval_decks=64)), **kw)
    (run / 'models').mkdir(parents=True)
    (run / 'config.json').write_text(json.dumps(cfg))
    R = Run(str(run), cfg)
    save(R.new_model(), model_path(str(run), it), {'iter': it})
    return R, model_path(str(run), it)


def test_the_field_and_the_test_pools():
    """The pools that count: every card supported, complete, a 40-card build; the rest skipped and counted by reason.
    The main build: the most games, then the lowest index. The test pools and the field are disjoint, fixed by the seed,
    and every field deck is a real 40-card deck of supported cards."""
    u = RD.universe('Sealed')
    c = u['counts']
    sk = c['skipped']
    assert c['counted'] == len(u['pools']) and c['counted'] + sk['unsupported']['pools'] + sk['incomplete'] + \
        sk['no_40_card_build']['pools'] == c['pools']
    assert set(sk['unsupported']['cards']) <= {'Paradox Haze', 'The Dominion Bracelet'}
    assert sum(sk['no_40_card_build']['build_sizes']) and min(sk['no_40_card_build']['build_sizes']) > 40
    rd = RD.RealDecks(8, 2, seed=0)
    assert len(rd.test) == 8 and len(rd.field_pools) == c['counted'] - 8
    assert not {p.id for p in rd.test} & {p.id for p in rd.field_pools}
    again, other = RD.RealDecks(8, 2, seed=0), RD.RealDecks(8, 2, seed=1)
    assert [p.id for p in again.test] == [p.id for p in rd.test] and [p.id for p in other.test] != [p.id for p in rd.test]
    assert [rd.opponent(k)['pool'] for k in range(16)] == [again.opponent(k)['pool'] for k in range(16)]
    for k in range(16):
        o = rd.opponent(k)
        assert len(o['deck']) == 40 and all(isinstance(v, int) for v in RD.R.lookup(o['deck'] + o['sideboard']).values())
    for p in rd.test + rd.field_pools[:200]:
        b = RD.main_build(p)
        assert b.deck_size == 40 and all(len(x.games) < len(b.games) or (len(x.games) == len(b.games) and x.index >= b.index)
                                         for x in p.builds if x.deck_size == 40)
        h = RD.deck_of(p, b)
        nonbasic = [n for n in h['deck'] if n not in C.BASICS]
        assert collections.Counter(nonbasic + h['sideboard']) == collections.Counter(p.cards)
    with pytest.raises(ValueError, match='field decks'):
        RD.RealDecks(5000, 2)
    d = rd.describe()
    assert d['license'].startswith('17lands public data, CC BY 4.0') and 'yardstick only' in d['rule']


def test_the_builders():
    """'best' is the builder's best guess (candidate 0 of the deck choice, a function of the pool); 'evaluator:' and
    'module:attr' plug other builders in; a deck that is not a 40-card build of the pool is refused."""
    rd = RD.RealDecks(3, 2)
    from mtgz.sealed import yardstick as Y
    pool = rd.test[0]
    names = Y._expand(pool.cards)
    best = RD.builder_of('best')
    a, b = best(names, 1), best(names, 2)
    assert a['deck'] == b['deck'] == RD.candidates(names, 7, 4)[0]['deck'] and len(a['deck']) == 40
    RD.check_build(pool, a['deck'])
    plug = RD.builder_of('tests.test_realdecks:_first_random')
    p = plug(names, 5)
    assert p['deck'] == RD.candidates(names, 5, 2)[1]['deck'] and p['how'] == 'tests.test_realdecks:_first_random'
    with pytest.raises(ValueError, match='not a 40-card build'):
        RD.check_build(pool, a['deck'][:39])
    with pytest.raises(ValueError, match='not a 40-card build'):
        RD.check_build(rd.test[1], a['deck'])
    with pytest.raises(ValueError, match="'best', 'value'"):
        RD.builder_of('nonsense')
    with pytest.raises(ValueError, match='network pilot'):
        RD.builder_of('value')


def _first_random(names, seed):
    return RD.candidates(names, seed, 2)[1]


def _human_as_ours(rd):
    """A builder that returns the test pool's own human build (the pairing check: A and B the same deck)."""
    from mtgz.sealed import yardstick as Y
    by = {tuple(Y._expand(p.cards)): rd.human(i) for i, p in enumerate(rd.test)}
    return lambda names, seed: dict(by[tuple(names)], how='the human build')


def test_paired_games_with_the_heuristic():
    """Our deck and the human deck play the same games (seed, opponent, seat, chooser, pilot streams): with the human
    build as 'our' deck every game is the same game (D = 0 throughout); the statistics; reproducible."""
    rd = RD.RealDecks(6, 4)
    base = RD.base_spec(FAST)
    same = RD.measure(rd, _human_as_ours(rd), RD.heuristic_player(2), base, builder_name='human')
    assert same['n'] == 24 and same['pools'] == 6 and same['same'] == 24 and same['diff'] == 0 and same['a'] == same['b']
    assert all(p['shared_slots'] == 40 for p in same['per_pool'])
    out = RD.measure(rd, RD.builder_of('best'), RD.heuristic_player(2), base, builder_name='best')
    again = RD.measure(rd, RD.builder_of('best'), RD.heuristic_player(1), base, builder_name='best')
    for k in ('a', 'b', 'diff', 'diff_se', 'same'):
        assert out[k] == again[k]
    assert out['n'] == 24 and out['value'] == out['a'] and out['refs'] == {'the human deck': out['b']}
    assert abs(out['diff'] - (out['a'] - out['b'])) < 1e-3 and out['failed'] == 0
    assert sum(p['games'] for p in out['per_pool']) == 24 and abs(sum(p['a_points'] for p in out['per_pool']) - 24 * out['a']) < 0.01 and out['license'].startswith('17lands public data')
    # game k: the field's deck k, seat k % 2, chooser (k // 2) % 2, the same seed for both decks
    s0 = RD.game_spec(base, rd.human(0), rd.opponent(3), K.mix(0, RD.GAME_SALT, 3), 1, K.chooser_of(3))
    assert s0['sealed']['p1']['order'] == 'given' and s0['sealed']['p0']['deck'] == rd.opponent(3)['deck']
    assert s0['chooser'] == 1 and s0['max_turns'] == 8 and s0['sealed']['best_share'] == 0.5
    assert RD.base_spec('{"sealed": {"set": "eoe", "collation": "arena", "deck_choice": {"k": 8}}, "decks": "eval", '
                        '"eval_seed": 1, "eval_decks": 64, "max_turns": 60}') == \
        {'sealed': {'set': 'eoe', 'collation': 'arena'}, 'max_turns': 60}


def test_the_summary():
    games = [(0, 0, 1.0, 0.0), (0, 1, 1.0, 1.0), (1, 2, 0.0, 0.0), (1, 3, 0.5, 1.0)]
    s = RD.summary(games)
    assert s['n'] == 4 and s['pools'] == 2 and s['a'] == 0.625 and s['b'] == 0.5 and s['diff'] == 0.125
    # over the pools: pool 0's D = 0.5, pool 1's -0.25 -> se 0.375
    assert s['diff_se_pools'] == 0.375 and s['same'] == 2 and s['a_better'] == 1 and s['b_better'] == 1


def test_a_network_pilot_and_the_command_line(tmp_path):
    """The command line with a (tiny, random) network: the run's evaluation seat on both seats, a JSON file per
    checkpoint, the same numbers at 1 and 2 driver threads; option C (--heuristic)."""
    ck = _smoke_run(tmp_path / 'r', 3)[1]
    outs = []
    for th in (1, 2):
        p = subprocess.run([sys.executable, '-m', 'mtgz.sealed.realdecks', ck, '--pools', '2', '--games', '2',
                            '--threads', str(th), '--sims', '2', '--out', str(tmp_path / f'out{th}'), '--heuristic'],
                           cwd=ROOT, capture_output=True, text=True, timeout=600, env=dict(os.environ, PYTHONIOENCODING='utf-8'))
        assert p.returncode == 0, p.stdout + p.stderr
        f = tmp_path / f'out{th}' / 'r-iter_0003.json'
        outs.append(json.loads(f.read_text()))
    o = outs[0]
    assert o['iter'] == 3 and o['n'] == 4 and o['sims'] == 2 and o['builder'] == 'best' and o['run_config_found']
    assert o['spec'] == {'sealed': {'set': 'eoe', 'collation': 'arena', 'best_share': 0.5}, 'max_turns': 8}
    assert o['setting']['test_pools'] == 2 and o['heuristic']['n'] == 4
    for k in ('a', 'b', 'diff', 'same'):
        assert outs[0][k] == outs[1][k]


def test_the_runs_extra_evaluation(tmp_path):
    """The package's `real_decks` (off by default: real_decks_every 0, passed over silently), on at a cadence: the run's
    evaluation seat on both seats, logged under extras.real_decks; the command line on the same checkpoint plays the
    same games."""
    from zero import package
    from zero.run import DEFAULTS, model_path
    pkg = package.active()
    assert DEFAULTS['real_decks_every'] == 0 and pkg.evals_every == {'real_decks': 'real_decks_every'}
    assert 'real_decks' in pkg.evals_of('mtg_eoe') and 'real_decks' not in pkg.evals_of('mtg')
    run = tmp_path / 'eoe'
    R, _ = _smoke_run(run, 4, eval_sims=2, threads=2, real_decks_pools=2, real_decks_games=2)
    m = {}
    R.extra_evals(4, m)
    assert 'real_decks' not in m.get('extras', {})
    R.cfg['real_decks_every'] = 3
    R.extra_evals(4, m)
    assert 'real_decks' not in m.get('extras', {})
    R.cfg['real_decks_every'] = 2
    R.extra_evals(4, m)
    r = m['extras']['real_decks']
    assert r['n'] == 4 and r['pools'] == 2 and r['sims'] == 2 and r['builder'] == 'best' and 'per_pool' not in r
    assert r['value'] == r['a'] and r['pct'] and 'seconds' in r
    log = (run / 'log.txt').read_text(encoding='utf-8')
    assert 'iter 4 real_decks: ' in log and 'skipped' not in log
    p = subprocess.run([sys.executable, '-m', 'mtgz.sealed.realdecks', model_path(str(run), 4), '--pools', '2', '--games',
                        '2', '--threads', '2'], cwd=ROOT, capture_output=True, text=True, timeout=600,
                       env=dict(os.environ, PYTHONIOENCODING='utf-8'))
    assert p.returncode == 0, p.stdout + p.stderr
    o = json.loads(p.stdout.strip().splitlines()[-1])
    for k in ('a', 'b', 'diff', 'same', 'n'):
        assert o[k] == r[k], k


_EOE_EVAL = r'''
import sys
from mtgz.az.mac import eval_log
from zero import remote
eval_log(sys.argv[1], int(sys.argv[2]))
remote.evaluate_mirror(sys.argv[1], int(sys.argv[2]))
'''


def test_a_remote_evaluation_runs_it_on_the_mirror(tmp_path):
    """The remote side (`python -m mtgz.az.remote eval`: zero.remote.evaluate_mirror on the mirror, with its config's
    real_decks settings) runs the extra evaluation with the rest and puts extras.real_decks in its result (the metrics
    row it prints and appends); zero.remote brings `extras` back into the run's evals.jsonl (zero-core docs/az.md 40)."""
    from zero.run import DEFAULTS, PRESETS, Run, model_path
    from zero.net import save
    cfg = dict(DEFAULTS, **PRESETS['eoe-smoke'])
    cfg.update(eval_games=2, eval_random_games=0, ref_games=2, elo_games=2, elo_paired=1, elo_opponents=1, threads=2,
               eval_sims=2, real_decks_every=1, real_decks_pools=2, real_decks_games=2,
               eval_spec=json.dumps(dict(FAST, decks='eval', eval_seed=1, eval_decks=64)))
    mirror = tmp_path / 'mtgz-remote' / 'eoe'
    (mirror / 'models').mkdir(parents=True)
    (mirror / 'config.json').write_text(json.dumps(cfg))
    R = Run(str(mirror), cfg)
    for k in (0, 1):
        save(R.new_model(), model_path(str(mirror), k), {'iter': k})
    p = subprocess.run([sys.executable, '-c', _EOE_EVAL, str(mirror), '1'], cwd=ROOT, capture_output=True, text=True,
                       timeout=600, env=dict(os.environ, PYTHONIOENCODING='utf-8'))
    assert p.returncode == 0, p.stdout + p.stderr
    rows = [json.loads(line) for line in open(mirror / 'metrics.jsonl', encoding='utf-8')]
    r = rows[-1]['extras']['real_decks']
    assert rows[-1]['iter'] == 1 and r['n'] == 4 and r['sims'] == 2 and rows[-1]['eval_yard']['games'] == 2


def test_a_failed_game_is_left_out_for_both_decks(monkeypatch):
    """An engine error in one game (a panic: an engine bug) removes it for both decks, its spec kept for the repro; the
    other games keep their seats, opponents and seeds."""
    rd = RD.RealDecks(4, 4)
    base = RD.base_spec(FAST)
    full = RD.measure(rd, RD.builder_of('best'), RD.heuristic_player(2), base)
    real = RD.Player.play
    calls = []

    def flaky(self, specs, driver_seed, tags=None):
        calls.append(list(tags))
        if len(calls) % 2 == 0 and 5 in tags:
            raise RuntimeError('game 5 (queued setup "5") (driver seed 1): a panic')
        return real(self, specs, driver_seed, tags)
    monkeypatch.setattr(RD.Player, 'play', flaky)
    out = RD.measure(rd, RD.builder_of('best'), RD.heuristic_player(2), base)
    assert out['n'] == 15 and out['failed'] == 1 and calls[-1] == [k for k in range(16) if k != 5]
    f = out['failed_games'][0]
    assert f['k'] == 5 and f['deck'] == 'human' and f['spec']['seed'] == K.mix(0, RD.GAME_SALT, 5)
    assert f['spec']['sealed']['p1']['order'] == 'given'           # game 5: the deck under test in seat 1
    assert sum(p['games'] for p in out['per_pool']) == 15 and out['per_pool'][1]['games'] == 3
    assert full['n'] == 16 and full['failed'] == 0
