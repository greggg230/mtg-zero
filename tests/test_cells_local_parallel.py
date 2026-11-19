"""The cells' local games in parallel (yardsticks cells_local_jobs / cells_local_split, docs/ENGINE.md 199;
mtgz.az.local_judge): the settings (the defaults change nothing: one measurement at a time in the lane's own thread,
its chooser matches one after the other in-process; the live run's switch accepted; the refusals), the watcher's local
lane running exactly cells_local_jobs at once (every measurement once, each cell after its baseline, the oldest
checkpoint first), a measurement's two chooser matches at once giving play_pair_local's results, h2h.match arguments,
rows, baselines (the same cache keys and files) and game records, and the match processes themselves (the job
protocol, the records written by them into the measurement's directory, a failure stopping the other match). No GPU
and no network: a fake zero.h2h.match."""
import gzip
import hashlib
import json
import os
import subprocess
import sys
import threading
import time

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz.az import local_judge as LJ  # noqa: E402
from mtgz.az import mac_judge as MJ  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402
from zero import gamerec as GR  # noqa: E402

D, S, N = Y.CELL_DECKS
# the live run's local GPU setting (legacy-meta3-r1, 2026-11-18) with the switch: four measurements at once, each's two
# chooser matches at once (8 match processes), 4 search threads and one driver group each
LIVE = {'bo1_games': 0, 'blunders': False, 'cells_mode': 'pool', 'cells_games': 200, 'cells_refs': ['iter0'],
        'cells_gen_games': 100, 'cells_gen_every': 256, 'cells_every': 256, 'cells_device': 'cuda', 'bo1_threads': 16,
        'bo1_groups': 1, 'timeout_s': 14400, 'cells_batch': 40, 'from': 512, 'bo1_farm_share': 1.0,
        'cells_backend': 'local'}
SWITCH = dict(LIVE, cells_local_jobs=4, cells_local_split=True, bo1_threads=4, bo1_groups=1)


def _run(tmp_path, iters, ys):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'eval_every': 16, 'game_records': 1, 'yardsticks': ys}
    (run / 'config.json').write_text(json.dumps(cfg))
    for i in iters:
        p = run / 'models' / f'iter_{i:04d}.pt'
        p.write_bytes(f'net {i}'.encode())
        os.utime(p, (time.time() - 3600, time.time() - 3600))
    return str(run), cfg


@pytest.fixture
def stub_seat(monkeypatch):
    monkeypatch.setattr(Y, 'cells_seat', lambda run: {'select': 'cq', 'cq_c_scale': 0.3})
    monkeypatch.setattr(Y, '_seat_keys', lambda ref: {})


# ---------------------------------------------------------------- the settings

def test_the_defaults_change_nothing_and_the_live_switch_is_accepted(tmp_path, stub_seat):
    ys = Y.normalize(LIVE)
    assert (ys['cells_local_jobs'], ys['cells_local_split']) == (1, False)
    assert (Y.DEFAULTS['cells_local_jobs'], Y.DEFAULTS['cells_local_split']) == (1, False)
    assert all(Y.lane_parallel(ys, x) == 1 for x in ('gpu', 'cpu_bo1', 'farm', 'fit', 'cpu'))
    assert Y.lane_parallel(ys, 'mac') == 2                               # (the Mac's lane: as before)
    sw = Y.normalize(SWITCH)
    assert (sw['cells_local_jobs'], sw['cells_local_split'], sw['bo1_threads'], sw['bo1_groups']) == (4, True, 4, 1)
    run, cfg = _run(tmp_path, (0, 256, 512), SWITCH)
    w = Y.wanted(run, cfg, sw, settle=0)
    assert {x['lane'] for x in w if x['kind'] != Y.FIT_KIND} == {'gpu'}
    assert Y.lanes(run, cfg, sw) == ['fit', 'gpu']
    assert Y.lane_parallel(sw, 'gpu') == 4 and Y.lane_parallel(sw, 'cpu_bo1') == 1 and Y.lane_parallel(sw, 'fit') == 1
    # the measurements (and the baselines' cache keys) do not read the new settings
    w0 = Y.wanted(run, cfg, Y.normalize(dict(SWITCH, cells_local_jobs=1, cells_local_split=False)), settle=0)
    assert w == w0 and {x['base'] for x in w if x.get('base')}
    cpu = Y.normalize(dict(SWITCH, cells_device='cpu'))
    assert Y.lane_parallel(cpu, 'cpu_bo1') == 4 and Y.lane_parallel(cpu, 'gpu') == 1


def test_the_settings_are_checked():
    base = {'bo1_games': 0, 'blunders': False, 'cells_games': 40, 'cells_refs': ['iter0']}
    with pytest.raises(ValueError, match='cells_local_jobs 0'):
        Y.normalize(dict(base, cells_local_jobs=0))
    with pytest.raises(ValueError, match='right type'):
        Y.normalize(dict(base, cells_local_jobs=True))
    with pytest.raises(ValueError, match='right type'):
        Y.normalize(dict(base, cells_local_split='yes'))
    with pytest.raises(ValueError, match='right type'):
        Y.normalize(dict(base, cells_local_jobs=2.0))
    for b in ('farm', 'mac'):
        for kw in ({'cells_local_jobs': 2}, {'cells_local_split': True}):
            with pytest.raises(ValueError, match='would do nothing'):
                Y.normalize(dict(base, cells_backend=b, cells_device='cpu', **kw))
        assert Y.normalize(dict(base, cells_backend=b, cells_device='cpu', cells_local_jobs=1))   # (the default)
    with pytest.raises(ValueError, match='cells_local_split is refused'):
        Y.normalize(dict(base, cells_local_split=True, cells_target_se=30, cells_batch=40))
    assert Y.normalize(dict(base, cells_local_jobs=3, cells_target_se=30, cells_batch=40))['cells_local_jobs'] == 3
    for b in (None, 'local'):                                            # (auto: local for a reference the farm can't)
        ys = Y.normalize(dict(base, cells_backend=b, cells_local_jobs=6, cells_local_split=True))
        assert (ys['cells_local_jobs'], ys['cells_local_split']) == (6, True)


# ---------------------------------------------------------------- the watcher's local lane

@pytest.mark.parametrize('jobs', [1, 3])
def test_the_local_lane_runs_cells_local_jobs_at_once_each_cell_after_its_baseline(tmp_path, stub_seat, jobs):
    import run_yardsticks as RY
    ys = {'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_refs': ['iter0'], 'cells_device': 'cuda',
          'cells_backend': 'local', 'cells_local_jobs': jobs, 'cells_boot': 0}
    run, cfg = _run(tmp_path, (0, 16, 32), ys)
    assert Y.lanes(run, cfg, Y.normalize(ys)) == ['gpu']
    w = RY.Watcher(run, once=True, log=lambda m: None, poll=0.01)
    w.tick = 0.01
    state = {'now': 0, 'max': 0}
    lock = threading.Lock()
    spans, threads = [], set()

    def measure(item, ys_):
        with lock:
            state['now'] += 1
            state['max'] = max(state['max'], state['now'])
            threads.add(threading.get_ident())
        t0 = time.time()
        time.sleep(0.15)
        end = time.time()                       # (a baseline's end: before its cache file is there for the cells)
        if item['kind'] == 'cells_base':
            os.makedirs(Y.cache_dir(run, ys_), exist_ok=True)
            with open(Y.base_file(Y.cache_dir(run, ys_), item['base']), 'w') as f:
                json.dump({'results': {}}, f)
            row = {'kind': 'cells_base', 'ref': item['ref'], 'pair': item['pair'], 'base': item['base'], 'scores': {},
                   'time': 1}
        else:
            row = {'kind': item['kind'], 'iter': item['iter'], 'ref': item['ref'], 'pair': item['pair'],
                   'base': item['base'], 'cells': {}, 'time': time.time()}
        with w.write:
            Y.append_row(run, row)
        with lock:
            state['now'] -= 1
            spans.append((item['kind'], item.get('iter'), tuple(item['pair']), t0, end))
        return row
    w.measure = measure
    assert w.main() == 0
    assert state['max'] == jobs                                             # (exactly cells_local_jobs at once)
    if jobs == 1:
        assert len(threads) == 1                                            # (one at a time, in the lane's thread)
    assert sorted((k, i, p) for k, i, p, _, _ in spans) == sorted(
        [('cells_base', None, p) for p in Y.cell_pairs(Y.normalize(ys)) if p[0] != p[1]]
        + [('cells', it, p) for it in (16, 32) for p in Y.cell_pairs(Y.normalize(ys))])     # (each once)
    ends = {p: e for k, _, p, _, e in spans if k == 'cells_base'}
    assert all(t0 >= ends[p] for k, _, p, t0, _ in spans if k == 'cells' and p in ends)   # (after its baseline)
    order = [(i, p) for k, i, p, _, _ in sorted(spans, key=lambda s: s[3]) if k == 'cells']
    assert [i for i, _ in order][:6].count(16) >= 5                         # (oldest checkpoint first)
    if jobs == 1:
        assert [i for i, _ in order] == [16] * 6 + [32] * 6


# ---------------------------------------------------------------- a fake world of games

def _u(*key):
    return int.from_bytes(hashlib.sha256('|'.join(map(str, key)).encode()).digest()[:4], 'little') / 2 ** 32


class World:
    """zero.h2h.match stand-in: game gid of a match is the deal (seed, gid), won by A (in seat gid % 2) with probability
    p_a when A and B are different networks, by seat 0 with 0.5 when the same; the chooser moves the deal's uniform.
    Records like zero.h2h's (a game log per row through the thread's sink). Its calls are kept, with their threads."""

    def __init__(self, p_a=0.7, barrier=None):
        self.p_a, self.calls, self.lock, self.barrier = p_a, [], threading.Lock(), barrier

    def match(self, a, b, games=600, sims=64, seed=0, threads=4, groups=2, device='cpu', spec=None, seat_a=None,
              seat_b=None, paired=False, rows=False, sparse=None, **kw):
        chooser = json.loads(spec)['chooser']
        same = os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))
        with self.lock:
            self.calls.append({'a': a, 'b': b, 'games': games, 'sims': sims, 'seed': seed, 'spec': spec,
                               'seat_a': seat_a, 'seat_b': seat_b, 'paired': paired, 'rows': rows, 'sparse': sparse,
                               'device': device, 'threads': threads, 'groups': groups, 'kw': kw,
                               'thread': threading.get_ident()})
        if self.barrier is not None:
            self.barrier.wait()                 # (both chooser matches under way at once)
        out = []
        for g in range(games):
            u = _u(seed, g, chooser)
            win0 = (u < 0.5) if same else ((u < self.p_a) == (g % 2 == 0))
            out.append({'gid': g, 'n': 2, 'over': g != 3, 'aborted': False,
                        'result': [1.0, 0.0] if win0 else [0.0, 1.0],
                        'log': {'seed': seed * 1000 + g, 'spec': spec, 'actions': [g % 7, 0, 1, 0], 'n_actions': 2,
                                'setup': json.dumps({'names': ['p0', 'p1']})}})
        sink = GR.active()
        if sink is not None:                    # (as zero.h2h._play records: A in seat gid % 2)
            pa = GR.net_player(a, sink.labels.get('a'), seat_a, 'cq')
            pb = GR.net_player(b, sink.labels.get('b'), seat_b, 'cq')
            sink.add_rows(out, lambda g: [pa, pb] if g % 2 == 0 else [pb, pa], {'driver_seed': int(seed), 'paired': False})
        for r in out:
            r.pop('log', None)
        a_score = sum(r['result'][r['gid'] % 2] for r in out) / games
        return {'rows': out, 'unfinished': sum(1 for r in out if not r['over']), 'select_a': 'cq-tag',
                'a_score': a_score, 'seconds': 0.1}


def _ckpts(tmp_path):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True, exist_ok=True)
    (run / 'config.json').write_text(json.dumps({'preset': 'delver-mirror-bo3-dealt', 'eval_every': 16}))
    ck, ref = run / 'models' / 'iter_0016.pt', run / 'models' / 'iter_0000.pt'
    ck.write_bytes(b'checkpoint 16')
    ref.write_bytes(b'generation 0')
    return str(ck), str(ref)


def _ys(**kw):
    return Y.normalize(dict({'bo1_games': 0, 'blunders': False, 'cells_games': 40, 'cells_refs': ['iter0'],
                             'cells_device': 'cuda', 'cells_backend': 'local', 'cells_boot': 0, 'bo1_threads': 4,
                             'bo1_groups': 1}, **kw))


def _in_thread(job, log=print, procs=None):
    """A match process replaced by play_job in run_both's thread (the job as it travels: JSON)."""
    return LJ.play_job(json.loads(json.dumps(job)))


@pytest.fixture
def in_process(monkeypatch):
    monkeypatch.setattr(LJ, 'run_job', _in_thread)


def _records(path):
    with gzip.open(path, 'rt', encoding='utf-8') as f:
        return [{k: v for k, v in json.loads(x).items() if k != 't'} for x in f if x.strip()]


# ---------------------------------------------------------------- both chooser matches at once

def test_the_two_chooser_matches_at_once_are_play_pair_locals_matches_exactly(tmp_path, monkeypatch):
    from zero import h2h
    ck, ref = _ckpts(tmp_path)
    seat = {'select': 'cq', 'belief_tilt': 1.0, 'cq_c_scale': 0.3}
    plain = World()
    monkeypatch.setattr(h2h, 'match', plain.match)
    monkeypatch.setattr(LJ, 'play_pair_split', lambda *a, **k: pytest.fail('the default played a split'))
    want = Y.play_pair_local(ck, ref, (D, S), 40, _ys(), seat, log=lambda m: None)
    assert len({c['thread'] for c in plain.calls}) == 1                  # (the default: in-process, in order)
    assert [json.loads(c['spec'])['chooser'] for c in plain.calls] == [0, 1]
    monkeypatch.undo()
    split = World(barrier=threading.Barrier(2, timeout=20))
    monkeypatch.setattr(h2h, 'match', split.match)
    monkeypatch.setattr(LJ, 'run_job', _in_thread)
    logs = []
    got = Y.play_pair_local(ck, ref, (D, S), 40, _ys(cells_local_split=True), seat, log=logs.append)
    assert got == want and got['unfinished'] == 2 and got['select'] == 'cq-tag'
    assert len({c['thread'] for c in split.calls}) == 2                  # (at once: the barrier passed)
    calls = sorted(split.calls, key=lambda c: json.loads(c['spec'])['chooser'])
    keys = ('a', 'b', 'games', 'sims', 'seed', 'spec', 'seat_a', 'seat_b', 'paired', 'rows', 'sparse', 'device',
            'threads', 'groups', 'kw')
    assert [{k: c[k] for k in keys} for c in calls] == [{k: c[k] for k in keys} for c in plain.calls]
    assert [(c['device'], c['threads'], c['groups'], c['sparse']) for c in calls] == [('cuda', 4, 1, True)] * 2
    assert any('both chooser matches at once' in m for m in logs)
    # a mirror (games / 2 a spec) and a baseline (the reference against itself) too
    for a, pair in ((ck, (N, N)), (ref, (S, N))):
        split.calls.clear()
        plain.calls.clear()
        monkeypatch.setattr(h2h, 'match', plain.match)
        w = Y.play_pair_local(a, ref, pair, 40, _ys(), seat, log=lambda m: None)
        monkeypatch.setattr(h2h, 'match', split.match)
        g = Y.play_pair_local(a, ref, pair, 40, _ys(cells_local_split=True), seat, log=lambda m: None)
        assert g == w and len(split.calls) == len(plain.calls) == 2
        assert [len(r) for r in g['results'].values()] == ([20, 20] if pair[0] == pair[1] else [40, 40])
        assert {(c['a'], c['b']) for c in split.calls} == {(a, ref)}
    # a seat that would not travel exactly is refused before anything plays
    with pytest.raises(ValueError, match='JSON'):
        LJ.pair_jobs(ck, ref, (D, S), 40, _ys(cells_local_split=True), {'select': ('cq', 1)})


def test_rows_baselines_and_their_cache_keys_are_the_same_with_the_split(tmp_path, monkeypatch, in_process):
    from zero import h2h
    world = World()
    monkeypatch.setattr(h2h, 'match', world.match)
    monkeypatch.setattr(Y, '_seat_keys', lambda ref: {'margin_w': 0.0})
    ck, ref = _ckpts(tmp_path)
    seat = {'select': 'cq'}
    rows = {}
    for split in (False, True):
        ys = _ys(cells_local_split=split, cells_local_jobs=4 if split else 1)
        cdir = str(tmp_path / f'cache{int(split)}')
        base = Y.measure_base_core(ref, 'gen0', (D, S), 40, ys, seat, cdir, log=lambda m: None)
        cell = Y.measure_cells_core('cells', ck, 16, ref, 'gen0', (D, S), 40, ys, seat, cdir, log=lambda m: None)
        mirror = Y.measure_cells_core('cells', ck, 16, ref, 'gen0', (N, N), 40, ys, seat, cdir, log=lambda m: None)
        rows[split] = [{k: v for k, v in r.items() if k not in ('seconds', 'time')} for r in (base, cell, mirror)]
        key = base['base']
        assert key == Y.base_key(ref, (D, S), 40, ys, seat)[0] == cell['base']
        with open(Y.base_file(cdir, key)) as f:
            rows[(split, 'file')] = {k: v for k, v in json.load(f).items() if k not in ('seconds', 'time')}
    assert rows[True] == rows[False]
    assert rows[(True, 'file')] == rows[(False, 'file')]                 # (the same cache file under the same key)
    assert rows[True][1]['backend'] == 'local' and rows[True][1]['games'] == 80
    # an existing baseline (the live run's) is reused by the split as is: nothing played again
    world.calls.clear()
    ys = _ys(cells_local_split=True)
    again = Y.measure_base_core(ref, 'gen0', (D, S), 40, ys, seat, str(tmp_path / 'cache0'), log=lambda m: None)
    assert again['cached'] and world.calls == []


def test_the_splits_game_records_are_play_pair_locals(tmp_path, monkeypatch, in_process):
    from zero import h2h
    world = World()
    monkeypatch.setattr(h2h, 'match', world.match)
    ck, ref = _ckpts(tmp_path)
    got = {}
    for split in (False, True):
        rec = str(tmp_path / f'records{int(split)}')
        with GR.recording(Y._sink(rec, 'cells', 16, 'iter_0016', 'gen0', {'ref': 'gen0', 'role': 'main'})) as sink:
            p = Y.play_pair_local(ck, ref, (D, S), 40, _ys(cells_local_split=split), {'select': 'cq'},
                                  log=lambda m: None)
        d = os.path.join(rec, 'cells', 'iter_0016')
        files = sorted(os.listdir(d))
        assert len(files) == 2 and all(f.startswith('local-') for f in files)
        assert sink.stats['files'] == 2 and sink.stats['games'] == 80 and len(sink.paths) == 2
        assert sorted(os.path.basename(x) for x in sink.paths) == files
        recs = sorted((_records(os.path.join(d, f)) for f in files), key=lambda x: x[2]['meta']['chooser'])
        for c, lines in enumerate(recs):
            games = lines[2:]
            assert lines[0]['kind'] == 'build' and [g['gid'] for g in games] == list(range(40))
            assert games[0]['meta'] == {'ref': 'gen0', 'role': 'main', 'pair': [D, S], 'chooser': c,
                                        'driver_seed': Y.pair_seed(_ys(), (D, S)), 'paired': False}
            assert {g['src'] for g in games} == {'local'} and {g['it'] for g in games} == {16}
            assert [g['result'][g['gid'] % 2] for g in games] == p['results'][str(c)]
        got[split] = recs
    assert got[True] == got[False]                                       # (record for record, but the time)


# ---------------------------------------------------------------- the match processes

CHILD = (
    'import json, os, sys, time, types\n'
    'sys.path.insert(0, {root!r})\n'
    'from mtgz.az import local_judge as LJ\n'
    'from zero import gamerec as GR\n'
    'import zero\n'
    'flags = {flags!r}\n'
    'def match(a, b, games, sims, seed=0, threads=4, groups=2, device="cpu", spec=None, seat_a=None, seat_b=None,\n'
    '          paired=False, rows=False, sparse=None):\n'
    '    import numpy  # (a DLL loaded while the stdin watcher runs: the Windows hang of 2026-11-18)\n'
    '    c = json.loads(spec)["chooser"]\n'
    '    print(f"match c{{c}} up", file=sys.stderr, flush=True)\n'
    '    if c == 1 and flags.get("fail"):\n'
    '        raise RuntimeError("no such match")\n'
    '    open(os.path.join(flags["dir"], f"c{{c}}.up"), "w").close()\n'
    '    t0 = time.time()\n'
    '    while not os.path.exists(os.path.join(flags["dir"], f"c{{1 - c}}.up")):\n'
    '        if time.time() - t0 > (600 if flags.get("fail") else 60):\n'
    '            raise RuntimeError("the other chooser match never started")\n'
    '        time.sleep(0.05)\n'
    '    out = [{{"gid": g, "n": 2, "over": True, "aborted": False,\n'
    '            "result": [1.0, 0.0] if (g + c) % 3 else [0.0, 1.0],\n'
    '            "log": {{"seed": g, "spec": spec, "actions": [g % 5, 1], "n_actions": 2, "setup": ""}}}}\n'
    '           for g in range(games)]\n'
    '    sink = GR.active()\n'
    '    if sink is not None:\n'
    '        pa, pb = GR.net_player(a, "A", seat_a), GR.net_player(b, "B", seat_b)\n'
    '        sink.add_rows(out, lambda g: [pa, pb] if g % 2 == 0 else [pb, pa], {{"driver_seed": seed}})\n'
    '    for r in out:\n'
    '        r.pop("log", None)\n'
    '    print(json.dumps({{"device": device, "threads": threads}}), file=sys.stderr, flush=True)\n'
    '    return {{"rows": out, "unfinished": 0, "select_a": "tag", "a_score": 0.5, "seconds": 1.0}}\n'
    'zero.h2h = sys.modules["zero.h2h"] = types.ModuleType("zero.h2h")  # (nothing heavy before the watcher starts)\n'
    'zero.h2h.match = match\n'
    'LJ.cmd_play()\n')


def _child_runner(monkeypatch, flags, procs_seen):
    real = MJ.job_process

    def run_job(job, log=print, procs=None):
        procs_seen.append(procs)
        return real([sys.executable, '-c', CHILD.format(root=ROOT, flags=flags)], job, log, procs, tag='local',
                    fail=f"cells_local_split: chooser {job['chooser']}'s match failed", cwd=ROOT)
    monkeypatch.setattr(LJ, 'run_job', run_job)


def test_the_match_processes_play_at_once_and_write_their_records(tmp_path, monkeypatch):
    """Two real match processes (the job protocol, each waiting for the other's start: at once), their records written
    by them into the measurement's directory and counted. The fake match imports numpy once the stdin watcher runs: on
    Windows a blocking read of stdin there hung that import (a DLL's C runtime asking the handle's type), 2026-11-18."""
    ck, ref = _ckpts(tmp_path)
    flags = {'dir': str(tmp_path)}
    procs = []
    _child_runner(monkeypatch, flags, procs)
    rec, logs, got = str(tmp_path / 'records'), [], {}
    sink = Y._sink(rec, 'cells_base', None, 'gen0', 'gen0', {'ref': 'gen0'})

    def play():                                 # (in a thread: a hung match process fails the test, never hangs it)
        with GR.recording(sink):
            got['p'] = LJ.play_pair_split(ref, ref, (D, S), 12, _ys(cells_local_split=True), {'select': 'cq'},
                                          log=logs.append)
    t = threading.Thread(target=play, daemon=True)
    t.start()
    t.join(240)
    if t.is_alive():
        for q in procs[0]:
            MJ.kill_proc(q)
        pytest.fail('the match processes hung: ' + ' | '.join(logs[-20:]))
    p = got['p']
    assert p['results'] == {str(c): [float(bool((g + c) % 3) == (g % 2 == 0)) for g in range(12)] for c in (0, 1)}
    assert p['unfinished'] == 0 and p['select'] == 'tag'
    assert all(os.path.exists(tmp_path / f'c{c}.up') for c in (0, 1))   # (each waited for the other: at once)
    assert 'local c0: match c0 up' in logs and 'local c1: match c1 up' in logs
    assert any('"device": "cuda"' in m and '"threads": 4' in m for m in logs)
    files = sorted(os.listdir(os.path.join(rec, 'cells_base', 'misc')))
    assert len(files) == 2 and all(f.startswith('local-') and f.endswith('.jsonl.gz') for f in files)
    assert sink.stats['files'] == 2 and sink.stats['games'] == 24 and len(sink.paths) == 2
    pids = {f.split('-')[1] for f in files}
    assert len(pids) == 2 and str(os.getpid()) not in pids              # (written by the two match processes)
    for f in files:
        lines = _records(os.path.join(rec, 'cells_base', 'misc', f))
        assert lines[0]['kind'] == 'build' and lines[0]['build'] == GR.build_info()     # (the measurement's build)
        assert {g['meta']['pair'] == [D, S] and g['meta']['ref'] == 'gen0' for g in lines[2:]} == {True}
        assert {g['kind'] for g in lines[2:]} == {'cells_base'} and len(lines[2:]) == 12
    assert all(q.poll() is not None for q in procs[0])


def test_a_failed_match_stops_the_other_and_is_raised(tmp_path, monkeypatch):
    ck, ref = _ckpts(tmp_path)
    procs = []
    _child_runner(monkeypatch, {'dir': str(tmp_path), 'fail': True}, procs)
    t0 = time.time()
    with pytest.raises(RuntimeError, match='no such match') as e:
        LJ.play_pair_split(ck, ref, (D, S), 12, _ys(cells_local_split=True), {'select': 'cq'}, log=lambda m: None)
    assert "chooser 1's match failed" in str(e.value)
    assert time.time() - t0 < 300                                         # (chooser 0 waits 600 s unless stopped)
    for q in procs[0]:
        q.wait(timeout=60)
    assert len(procs[0]) == 2 and all(q.poll() is not None for q in procs[0])


def test_a_match_process_ends_when_its_stdin_closes(tmp_path):
    """The measurement keeps each match process's stdin open while it waits; its end (the watcher killed it) closes
    it, and the match process ends (exit 75) with its games: none left playing."""
    ck, ref = _ckpts(tmp_path)
    job = LJ.pair_jobs(ck, ref, (D, S), 12, _ys(cells_local_split=True), {'select': 'cq'})[0]
    flags = {'dir': str(tmp_path), 'fail': True}                          # (chooser 0 waits for chooser 1: 600 s)
    p = subprocess.Popen([sys.executable, '-c', CHILD.format(root=ROOT, flags=flags)], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=ROOT)
    p.stdin.write(json.dumps(job).encode() + b'\n')
    p.stdin.flush()
    assert p.stderr.readline().strip() == b'match c0 up'
    p.stdin.close()
    assert p.wait(timeout=60) == 75
    with pytest.raises(SystemExit, match='usage'):
        LJ.main(['nope'])
