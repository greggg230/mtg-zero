"""The matchup model's games from the run's game records (docs/ENGINE.md 273, mtgz.az.bo3_records): random-move Delver
mirror matches with per-seat dealt decks and random concessions, played with the engine's game log on (what a run's
local self-play writes) and recorded as chained contests the way the drivers record them (the farm's: episode
boundaries `{"next": seat}` in the steps, no `episodes`). The source rule, the records' game dicts equal to the log's,
the cache, records of another engine left out and said, a starved and a failed retrain loud (the version, the status,
the run log, sb-health), and pb3's window still read from its log (its published version's games)."""
import collections
import json
import os
import random
import shutil
import time

import pytest

import mtgz.az  # noqa: F401
from mtgz import _engine as E
from mtgz.az import bo3_records as BR
from mtgz.az import decklists
from mtgz.az import matchup as MU
from mtgz.stew import sbhealth as SB
from zero import gamerec as GR

SPEC = decklists.mirror_bo3_spec(max_turns=10, game_episodes=True,
                                 dealt_decks={'p': 0.5, 'swaps': [1, 8], 'per_seat': True})
OTHER = {'pkg': 'mtgz', 'commit': '0' * 40, 'core': '0' * 40, 'engine': '00000000deadbeef', 'module': 'x'}


def play_match(seed, rng, concede_p):
    """A random-move match recorded as a chained contest (episode boundaries in its steps, as the farm records one)."""
    g = E.Game('mtg', SPEC, 2, seed)
    steps = []
    while True:
        while g.to_move is not None:
            st = json.loads(g.export_state())
            if (st.get('decision') or {}).get('kind') != 'Sideboard' and g.can_concede and rng.random() < concede_p:
                break
            a = rng.choice(list(g.legal()))
            steps.append(a)
            g.step(a)
        if g.to_move is None and g.aborted:
            over, res = False, [0.5, 0.5]
            break
        conceded = None if g.to_move is None else g.to_move
        nxt, res = g.next_episode(conceded)
        if nxt is None:
            over, res = True, [float(x) for x in res][:2]
            break
        steps.append({'next': conceded})
        g = nxt
    return {'v': 1, 'gid': seed % 100000, 'seed': seed, 'spec': GR.spec_key(SPEC), 'n': 2, 'actions': None,
            'steps': steps, 'n_actions': len(steps), 'setup': json.dumps({'names': ['delver', 'delver']}),
            'over': over, 'aborted': not over, 'resigned': None, 'result': res, 'decisions': None, 'rounds': None,
            'nets': None, 't': round(time.time(), 1)}


ITERS = 3
PER_ITER = 6


@pytest.fixture(scope='module')
def played(tmp_path_factory):
    """Run `local`: ITERS iterations of matches with the game log on (games/), records of source 'local' and metrics
    lines between the iterations. -> (base, run, records per iteration)."""
    base = tmp_path_factory.mktemp('matchup_records')
    run = str(base / 'local')
    os.makedirs(os.path.join(run, 'games'))
    rng = random.Random(11)
    recs = {}
    E.game_log(os.path.join(run, 'games', 'games-1.jsonl'))
    try:
        for it in range(1, ITERS + 1):
            recs[it] = [play_match(E.game_seed_of(91, 100 * it + k, False), rng, 0.01 if k % 2 else 0.0)
                        for k in range(PER_ITER)]
            time.sleep(0.01)
            with open(os.path.join(run, 'metrics.jsonl'), 'a', encoding='utf-8') as f:
                f.write(json.dumps({'iter': it, 'time': time.time()}) + '\n')
            time.sleep(0.01)
    finally:
        E.game_log(None)
    w = GR.Records(run)
    for it, rs in recs.items():
        w.add(it, 'local', [(r, SPEC) for r in rs])
        w.close(it, {'local': len(rs)})
    return base, run, recs


def records_run(base, played_run, name, srcs, builds=None):
    """A copy of the played run (its game log and metrics) whose records come from sources `srcs` per iteration
    ({it: source}), each iteration's file written with `builds[it]` (None: this checkout's engine)."""
    run = str(base / name)
    if os.path.isdir(run):
        shutil.rmtree(run)
    shutil.copytree(played_run, run, ignore=shutil.ignore_patterns('records', 'matchup'))
    w = GR.Records(run)
    for it in range(1, ITERS + 1):
        _, rs = GR.read_iteration(played_run, it)
        w.add(it, srcs[it], [(dict(r), SPEC) for r in rs], build=(builds or {}).get(it))
        w.close(it, {srcs[it]: len(rs)})
    return run


def strip(g):
    return {k: v for k, v in g.items() if k != 'time'}


def canon(games):
    return sorted(json.dumps(strip(g), sort_keys=True) for g in games)


def test_the_source_rule_and_the_records_give_the_logs_games(played):
    base, run, recs = played
    # the matches hold concessions, one inferred at the match's end (the record over, its last game not)
    assert any(isinstance(s, dict) and s['next'] is not None for rs in recs.values() for r in rs for s in r['steps'])
    assert sum(1 for rs in recs.values() for r in rs if r['over']) >= 1
    # local self-play only: the game log covers the window (pb3's case: the versions it always published)
    assert MU.data_source(run, 1, ITERS)[0] == 'log'
    lg, ls = MU.load_games(run, 1, ITERS)
    info = {}
    rg, rs = MU.load_games(run, 1, ITERS, source='records', info=info)
    assert lg and len({g['game'] for g in lg}) >= 2
    assert canon(lg) == canon(rg), 'the records replay into exactly the logged games (every field but the time)'
    assert ls == rs
    assert info['games'] == info['games_records'] == len(lg) and info['matches'] == ITERS * PER_ITER
    t_of = {r['seed']: r['t'] for rs_ in recs.values() for r in rs_}
    assert all(g['time'] == t_of[g['seed']] for g in rg), "a game's time: its record's"
    # farm self-play: the records are the source, the same games
    farm = records_run(base, run, 'farm', {it: 'pool:x' for it in range(1, ITERS + 1)})
    src, why = MU.data_source(farm, 1, ITERS)
    assert src == 'records' and 'off this machine' in why
    info = {}
    fg, _ = MU.load_games(farm, 1, ITERS, info=info, jobs=2)
    assert canon(fg) == canon(lg) and info['source'] == 'records' and info['excluded'] == []
    assert 'replayed from the records of iterations 1-3' in MU.data_note(info)
    # one farm iteration is enough: the whole window from the records
    mixed = records_run(base, run, 'mixed', {1: 'local', 2: 'pool:x', 3: 'local'})
    assert MU.data_source(mixed, 1, ITERS)[0] == 'records' and MU.data_source(mixed, 3, 3)[0] == 'log'


def test_an_iteration_without_records_comes_from_the_log(played):
    base, run, recs = played
    part = records_run(base, run, 'part', {it: 'pool:x' for it in range(1, ITERS + 1)})
    shutil.rmtree(GR.iter_dir(part, 2))
    info = {}
    g, _ = MU.load_games(part, 1, ITERS, info=info)
    lg, _ = MU.load_games(run, 1, ITERS)
    assert canon(g) == canon(lg)
    assert info['records_iterations'] == '1, 3' and info['log_iterations'] == '2'
    assert info['games_log'] == sum(1 for x in lg if x['iter'] == 2)


def test_the_replay_cache(played):
    base, run, recs = played
    farm = records_run(base, run, 'cached', {it: 'pool:x' for it in range(1, ITERS + 1)})
    cache = os.path.join(farm, 'matchup', 'replayed')
    i1, i2 = {}, {}
    a, _ = MU.load_games(farm, 1, ITERS, cache=cache, info=i1)
    b, _ = MU.load_games(farm, 1, ITERS, cache=cache, info=i2)
    assert i1['cached_iterations'] == 0 and i2['cached_iterations'] == ITERS and a == b
    # a changed iteration (its DONE.json) is replayed again
    p = os.path.join(GR.iter_dir(farm, 2), GR.DONE)
    d = json.load(open(p, encoding='utf-8'))
    d['time'] += 1
    json.dump(d, open(p, 'w', encoding='utf-8'))
    i3 = {}
    MU.load_games(farm, 1, ITERS, cache=cache, info=i3)
    assert i3['cached_iterations'] == ITERS - 1
    BR.prune_cache(cache, 3)
    assert sorted(os.listdir(cache)) == ['iter_0003.json.gz']


def test_records_of_another_engine_are_left_out_and_said(played):
    base, run, recs = played
    other = records_run(base, run, 'other', {it: 'pool:x' for it in range(1, ITERS + 1)}, builds={1: OTHER})
    info = {}
    g, _ = MU.load_games(other, 1, ITERS, info=info)
    lg, _ = MU.load_games(run, 1, ITERS)
    assert canon(g) == canon([x for x in lg if x['iter'] != 1])
    ex = info['excluded']
    assert len(ex) == 1 and ex[0]['iterations'] == '1' and ex[0]['engine'] == OTHER['engine']
    assert ex[0]['why'] == 'other_engine' and ex[0]['matches'] == PER_ITER
    assert ex[0]['games'] == info['excluded_games'] == sum(BR.episodes_of(r) for r in recs[1])
    note = MU.data_note(info)
    assert f'LEFT OUT {info["excluded_games"]} games ({PER_ITER} matches) of iterations 1' in note


def test_a_record_that_does_not_replay_fails_loudly(played):
    base, run, recs = played
    bad = records_run(base, run, 'bad', {it: 'pool:x' for it in range(1, ITERS + 1)})
    # one match's result changed: the replay no longer ends as the record says
    _, rs = GR.read_iteration(bad, 2)
    w = GR.Records(str(base / 'bad2'))
    r = dict(rs[0], result=[0.25, 0.75])
    w.add(2, 'pool:x', [(r, SPEC)] + [(x, SPEC) for x in rs[1:]])
    w.close(2, {'pool:x': len(rs)})
    shutil.rmtree(GR.iter_dir(bad, 2))
    shutil.copytree(GR.iter_dir(str(base / 'bad2'), 2), GR.iter_dir(bad, 2))
    with pytest.raises(RuntimeError, match='iteration 2 source pool:x match'):
        MU.load_games(bad, 1, ITERS)


class FakeRun:
    def __init__(self, run):
        self.run = run
        self.cfg = dict(MU.SETTINGS, spec=SPEC, matchup_every=1)
        self.lines = []

    def log(self, m):
        self.lines.append(m)


def write_config(run):
    with open(os.path.join(run, 'config.json'), 'w', encoding='utf-8') as f:
        json.dump(dict(MU.SETTINGS, spec=SPEC, matchup_every=1), f)


def test_a_starved_version_is_loud(played):
    base, run, recs = played
    farm = records_run(base, run, 'starved', {it: 'pool:x' for it in range(1, ITERS + 1)})
    write_config(farm)
    s = MU.retrain(farm, ITERS, 1, val_iters=1, half_life=0.0, threads=1, worlds=20, log=lambda m: None, min_gain=10.0,
                   members_n=2, min_games_per_iter=500)
    assert s['ok'] and s['starved'] and s['min_games'] == 500 * ITERS and s['data']['source'] == 'records'
    v = MU._load(os.path.join(MU.mdir(farm), f'v{ITERS:04d}.json'))
    assert v['data']['starved'] and v['data']['games_records'] == s['games'] and v['data']['note'] == s['data_note']
    idx = MU._rows(os.path.join(MU.mdir(farm), 'index.jsonl'))[-1]
    assert idx['starved'] and idx['source'] == 'records' and idx['excluded_games'] == 0
    assert os.listdir(MU.replay_cache(farm)), 'the run\'s replay cache'
    R = FakeRun(farm)
    MU.poll(R)
    assert any(x.startswith(f'MATCHUP MODEL STARVED v{ITERS:04d}: fitted on {s["games"]} games (< {500 * ITERS}')
               for x in R.lines)
    h = SB.model_health(farm)
    assert h['starved'] and h['failed'] is None
    assert any(f'fitted on {s["games"]} games (< {500 * ITERS}' in a for a in h['alerts'])
    assert not h['excluded_games']


def test_a_failed_retrain_is_loud(played):
    base, run, recs = played
    other = records_run(base, run, 'failed', {it: 'pool:x' for it in range(1, ITERS + 1)},
                        builds={it: OTHER for it in range(1, ITERS + 1)})
    shutil.rmtree(os.path.join(other, 'games'))
    write_config(other)
    with pytest.raises(RuntimeError, match='no self-play game in iterations 1-3') as e:
        MU.retrain(other, ITERS, 1, val_iters=1, threads=1, worlds=20, log=lambda m: None, members_n=2)
    assert 'LEFT OUT' in str(e.value)
    st = MU._load(os.path.join(MU.job_dir(other, ITERS), 'status.json'))
    assert not st['ok'] and st['data']['excluded_games'] > 0 and 'LEFT OUT' in st['data_note']
    R = FakeRun(other)
    MU.poll(R)
    assert any(x.startswith(f'MATCHUP RETRAIN FAILED v{ITERS:04d}') and 'LEFT OUT' in x for x in R.lines)
    h = SB.model_health(other)
    assert h['failed']['version'] == f'v{ITERS:04d}'
    assert any(a.startswith('matchup model: no new version since none (no version yet) (last retrain failed: v0003')
               for a in h['alerts'])
    # after a published version, a newer failure names it
    with open(os.path.join(MU.mdir(other), 'v0002.json'), 'w', encoding='utf-8') as f:
        json.dump({'format': MU.FORMAT, 'window': [1, 2], 'report': {'games': 5000}, 'table': None}, f)
    h = SB.model_health(other)
    assert any('no new version since v0002 (last retrain failed: v0003' in a for a in h['alerts'])


def test_a_version_before_its_data_was_recorded(tmp_path):
    run = str(tmp_path / 'old')
    os.makedirs(MU.mdir(run))
    with open(os.path.join(MU.mdir(run), 'v0064.json'), 'w', encoding='utf-8') as f:
        json.dump({'format': MU.FORMAT, 'window': [33, 64], 'report': {'games': 6411}, 'table': None}, f)
    h = SB.model_health(run)
    assert h['starved'] and h['min_games'] == 500 * 32
    assert h['alerts'] == ['matchup model v0064: fitted on 6411 games (< 16000: 500 a game per iteration of its window '
                           '33-64)']


def test_pb3_reads_its_game_log_as_before():
    """pb3 (local self-play, no records): the window of its published v0144 from its game log, every game its version
    was fitted on (the whole fit and table: by hand, docs/ENGINE.md 273)."""
    from zero import local_config as LC
    run = LC.big_disk_path('mtg-zero', 'runs', 'delver-bo3-pb3')
    if not run or not os.path.exists(os.path.join(run or '', 'matchup', 'v0144.json')):
        pytest.skip('the pb3 run is not on this machine (the maintainer\'s data)')
    src, why = MU.data_source(run, 113, 144)
    assert src == 'log', why
    info = {}
    games, _ = MU.load_games(run, 113, 144, info=info)
    lg, _ = MU.log_games(run, 113, 144)
    assert games == lg and info['source'] == 'log'
    v = MU._load(os.path.join(run, 'matchup', 'v0144.json'))
    assert len(games) == v['report']['games'] == 61967
    assert collections.Counter(g['iter'] for g in games).keys() == set(v['report']['iterations'])
