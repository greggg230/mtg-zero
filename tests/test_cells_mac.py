"""The cells' games on the MacBook (yardsticks cells_backend 'mac', docs/ENGINE.md 198; mtgz.az.mac_judge): the setting
(the live run's switch accepted exactly, both cell kinds in the lane 'mac'; the refusals), the matches (both chooser
matches at once, each exactly the local backend's zero.h2h.match call: the same results), the files on the Mac (each
checkpoint under its run's name with the run's config.json beside models/, sent only when missing or different, checked
by sha256), the game records (the Mac's file written whole into the measurement's records directory, checked against
the results), a mirror cell on identical deals with different networks (scored as its games say, never forced to 0.5),
the watcher's parallel lane (cells_mac_jobs // 2 measurements at once, a cell after its baseline), the take-over of a
companion started through a venv launcher, and the Mac process's end when its stdin closes. No Mac and no network: the
Mac's side runs here (mtgz.az.mac_judge.play_job / have / put) on a fake zero.h2h.match."""
import base64
import gzip
import hashlib
import io
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
from mtgz.az import mac_judge as MJ  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402
from zero import gamerec as GR  # noqa: E402

D, S, N = Y.CELL_DECKS
# the live run's switch (legacy-meta3-r1, 2026-11-18): its config.json's yardsticks with the Mac backend and the
# gen-over-gen cells back on at the cells' cadence
SWITCH = {'bo1_games': 0, 'blunders': False, 'cells_mode': 'pool', 'cells_games': 200, 'cells_refs': ['iter0'],
          'cells_gen_games': 200, 'cells_gen_every': 256, 'cells_every': 256, 'cells_device': 'cpu', 'bo1_threads': 16,
          'bo1_groups': 4, 'timeout_s': 14400, 'cells_batch': 40, 'from': 512, 'bo1_farm_share': 1.0,
          'cells_backend': 'mac'}


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


# ---------------------------------------------------------------- the setting and the lane

def test_the_switch_setting_is_accepted_and_both_cell_kinds_play_on_the_mac(tmp_path, stub_seat):
    import run_yardsticks as RY
    ys = Y.normalize(SWITCH)
    assert (ys['cells_backend'], ys['cells_mode'], ys['cells_games'], ys['cells_gen_games']) == ('mac', 'pool', 200, 200)
    assert (ys['cells_every'], ys['cells_gen_every'], ys['from'], ys['cells_device']) == (256, 256, 512, 'cpu')
    assert (ys['cells_mac_jobs'], ys['cells_mac_threads'], ys['cells_mac_groups'], ys['cells_mac_torch_threads'],
            ys['cells_mac_nice']) == (4, 4, 2, 2, 10)
    run, cfg = _run(tmp_path, (0, 240, 256, 496, 512, 528), SWITCH)
    w = Y.wanted(run, cfg, ys, settle=0)
    assert {x.get('iter') for x in w} == {None, 512}                     # (from 512, every 256)
    assert {x['lane'] for x in w if x['kind'] != Y.FIT_KIND} == {'mac'}
    assert Y.lanes(run, cfg, ys) == ['fit', 'mac']
    assert Y.lane_parallel(ys, 'mac') == 2 and Y.lane_parallel(ys, 'farm') == 1 and Y.lane_parallel(ys, 'fit') == 1
    kinds = [(x['kind'], x['ref']) for x in w if x['kind'] != Y.FIT_KIND]
    assert kinds.count(('cells_base', 'gen0')) == 3 and kinds.count(('cells', 'gen0')) == 6
    assert kinds.count(('cells_base', 'iter_0256')) == 3 and kinds.count(('cells_gen', 'iter_0256')) == 6
    assert all(x['games'] == 200 for x in w if x['kind'] in ('cells', 'cells_base'))
    gen = next(x for x in w if x['kind'] == 'cells_gen')
    cmd = RY.Watcher(run, once=True, log=lambda m: None).one_cmd(gen)
    assert cmd[3:] == [run, '--kind', 'cells_gen', '--iter', '512', '--ref', 'iter_0256', '--pair', f'{D},{D}']
    # a lane running several: the measurements under way skipped (a cell waiting for its baseline under way too)
    first = Y.pending(run, cfg, ys, 'mac', settle=0)
    assert first['kind'] == 'cells_base' and first['ref'] == 'gen0' and first['pair'] == [D, S]
    busy = {Y.key_of(first)}
    nxt = Y.pending(run, cfg, ys, 'mac', settle=0, busy=busy)
    assert nxt['kind'] == 'cells_base' and nxt['pair'] == [D, N]
    busy |= {Y.key_of(nxt), Y.key_of(Y.pending(run, cfg, ys, 'mac', settle=0, busy=busy | {Y.key_of(nxt)}))}
    cell = Y.pending(run, cfg, ys, 'mac', settle=0, busy=busy)
    assert (cell['kind'], cell['pair'], cell['role']) == ('cells', [D, D], 'main')     # (a mirror: no baseline)


def test_the_mac_backends_setting_is_checked():
    base = {'bo1_games': 0, 'blunders': False, 'cells_games': 40, 'cells_refs': ['iter0'], 'cells_backend': 'mac'}
    with pytest.raises(ValueError, match="Mac's CPU"):
        Y.normalize(base)                                                # (cells_device: bo1_device 'cuda')
    ok = dict(base, cells_device='cpu')
    assert Y.normalize(ok)['cells_backend'] == 'mac'
    with pytest.raises(ValueError, match='sequential'):
        Y.normalize(dict(ok, cells_target_se=30, cells_batch=40))
    with pytest.raises(ValueError, match='cells_mac_jobs'):
        Y.normalize(dict(ok, cells_mac_jobs=1))
    with pytest.raises(ValueError, match='cells_mac_nice'):
        Y.normalize(dict(ok, cells_mac_nice=25))
    with pytest.raises(ValueError, match='cells_mac_threads'):
        Y.normalize(dict(ok, cells_mac_threads=0))
    with pytest.raises(ValueError, match="'mac'"):
        Y.normalize(dict(ok, cells_backend='vast'))
    assert Y.normalize(dict(ok, cells_mac_jobs=7)) and Y.lane_parallel(Y.normalize(dict(ok, cells_mac_jobs=7)), 'mac') == 3
    assert Y.DEFAULTS['cells_backend'] is None                           # (auto never picks the Mac)


# ---------------------------------------------------------------- a fake world of games, the Mac's side here

def _u(*key):
    return int.from_bytes(hashlib.sha256('|'.join(map(str, key)).encode()).digest()[:4], 'little') / 2 ** 32


class World:
    """zero.h2h.match stand-in: game gid of a match is the deal (seed, gid) - the same in both chooser matches - won by
    A (in seat gid % 2) with probability p_a when A and B are different networks, by seat 0 with 0.5 when they are the
    same; the chooser moves the deal's uniform (choosing play or draw matters). Records like zero.h2h's (a game log per
    row through the thread's sink). Its calls are kept."""

    def __init__(self, p_a=0.7):
        self.p_a, self.calls, self.lock = p_a, [], threading.Lock()

    def match(self, a, b, games=600, sims=64, seed=0, threads=4, groups=2, device='cpu', spec=None, seat_a=None,
              seat_b=None, paired=False, rows=False, sparse=None, **kw):
        chooser = json.loads(spec)['chooser']
        same = os.path.basename(a) == os.path.basename(b) and open(a, 'rb').read() == open(b, 'rb').read()
        with self.lock:
            self.calls.append({'a': os.path.basename(a), 'b': os.path.basename(b), 'games': games, 'sims': sims,
                               'seed': seed, 'spec': spec, 'seat_a': seat_a, 'seat_b': seat_b, 'paired': paired,
                               'rows': rows, 'sparse': sparse, 'device': device, 'threads': threads, 'groups': groups})
        out = []
        for g in range(games):
            u = _u(seed, g, chooser)
            if same:
                win0 = u < 0.5
            else:
                a_wins = u < self.p_a
                win0 = a_wins == (g % 2 == 0)
            out.append({'gid': g, 'n': 2, 'over': True, 'aborted': False, 'result': [1.0, 0.0] if win0 else [0.0, 1.0],
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
        return {'rows': out, 'unfinished': 0, 'select_a': 'cq-tag', 'a_score': a_score, 'seconds': 0.1}


class FakeMac:
    """The Mac in a directory here: mac_judge's ssh replaced by its Mac-side commands (have / put) run in-process, and
    run_job by play_job; `barrier` (2): both chooser matches must be under way at once."""

    def __init__(self, tmp_path, monkeypatch, world, barrier=True):
        self.work = str(tmp_path / 'mac' / MJ.WORK).replace('\\', '/')
        self.rt = {'host': 'user@mac', 'home': '/Users/user', 'python': '/Users/user/.venvs/v/bin/python',
                   'repo': '/Users/user/mtg-zero-wt/run-aaaaaaaaaaaa', 'work': self.work, 'commit': 'a' * 40}
        self.sent, self.jobs = [], []
        self.barrier = threading.Barrier(2, timeout=20) if barrier else None
        from zero import h2h
        monkeypatch.setattr(h2h, 'match', world.match)
        monkeypatch.setattr(MJ, 'remote', lambda: dict(self.rt))
        monkeypatch.setattr(MJ, '_run', self.run)
        monkeypatch.setattr(MJ, 'run_job', self.run_job)

    def run(self, cmd, input=None, stdin=None, timeout=None):
        import shlex
        sh = cmd[-1]
        assert cmd[0] == 'ssh' and cmd[-2] == 'user@mac' and 'BatchMode=yes' in cmd
        assert sh.startswith(f"cd {self.rt['repo']} ") and ' -m mtgz.az.mac_judge ' in sh
        args = shlex.split(sh.split(' -m mtgz.az.mac_judge ', 1)[1])
        data = input if input is not None else stdin.read()
        old_in, old_out = sys.stdin, sys.stdout
        sys.stdin = io.TextIOWrapper(io.BytesIO(data), encoding='utf-8')
        sys.stdout = out = io.StringIO()
        try:
            code = MJ.main(args)
            err = b''
        except SystemExit as e:
            code, err = 1, str(e).encode()
        finally:
            sys.stdin, sys.stdout = old_in, old_out
        if args[0] == 'put':
            self.sent.append(args[1])
        return subprocess.CompletedProcess(cmd, code, out.getvalue().encode(), err)

    def run_job(self, rt, job, ys, log=print, procs=None):
        assert rt['commit'] == job['commit'] == 'a' * 40
        self.jobs.append(job)
        if self.barrier is not None:
            self.barrier.wait()
        return MJ.play_job(json.loads(json.dumps(job)))      # (the job as it travels: JSON)


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
                             'cells_device': 'cpu', 'cells_backend': 'mac', 'cells_boot': 0}, **kw))


def test_the_mac_plays_both_chooser_matches_at_once_exactly_as_the_local_backend(tmp_path, monkeypatch):
    world = World()
    mac = FakeMac(tmp_path, monkeypatch, world)
    ck, ref = _ckpts(tmp_path)
    ys, seat = _ys(), {'select': 'cq', 'belief_tilt': 1.0}
    logs = []
    got = MJ.play_pair_mac(ck, ref, (D, S), 40, ys, seat, log=logs.append)
    mac_calls = sorted(world.calls, key=lambda c: json.loads(c['spec'])['chooser'])
    world.calls.clear()
    want = Y.play_pair_local(ck, ref, (D, S), 40, ys, seat, log=lambda m: None)
    assert got['results'] == want['results'] and got['unfinished'] == want['unfinished'] == 0
    assert got['select'] == want['select'] == 'cq-tag'
    assert got['mac']['commit'] == 'a' * 12 and set(got['mac']) == {'commit', '0', '1'}
    same = ('games', 'sims', 'seed', 'spec', 'seat_a', 'seat_b', 'paired', 'rows', 'sparse', 'device')
    assert [{k: c[k] for k in same} for c in mac_calls] == [{k: c[k] for k in same} for c in world.calls]
    assert all((c['a'], c['b'], c['threads'], c['groups']) == ('iter_0016.pt', 'iter_0000.pt', 4, 2) for c in mac_calls)
    assert [j['chooser'] for j in sorted(mac.jobs, key=lambda j: j['chooser'])] == [0, 1]
    # the files on the Mac: the run's name and models/ kept, its config.json beside them, the same bytes
    rp, base, rd = MJ.remote_path(mac.work, ck)
    assert rp.endswith('/run/models/iter_0016.pt') and os.path.normcase(rd) == os.path.normcase(os.path.dirname(
        os.path.dirname(ck)))
    assert open(rp, 'rb').read() == b'checkpoint 16' and open(base + '/config.json').read() == open(
        os.path.join(rd, 'config.json')).read()
    assert GR.ckpt_id(rp) == GR.ckpt_id(ck) == 'run/iter_0016'
    assert len(mac.sent) == 3                                             # (two checkpoints and the run's config)
    MJ.play_pair_mac(ck, ref, (N, N), 40, ys, seat, log=logs.append)
    assert len(mac.sent) == 3                                             # (already there: nothing sent again)
    with open(ck, 'wb') as f:
        f.write(b'checkpoint 16, rewritten')
    MJ.play_pair_mac(ck, ref, (N, N), 40, ys, seat, log=logs.append)
    assert len(mac.sent) == 4 and open(rp, 'rb').read() == b'checkpoint 16, rewritten'
    # a baseline: one file, the reference against itself
    mac.sent.clear()
    b = MJ.play_pair_mac(ref, ref, (D, S), 40, ys, seat, log=logs.append)
    assert mac.sent == [] and set(b['results']) == {'0', '1'}
    assert any('both chooser matches' in m for m in logs)


def test_the_put_refuses_a_file_whose_sha256_differs(tmp_path, monkeypatch):
    dest = str(tmp_path / 'x' / 'f.pt')
    monkeypatch.setattr(sys, 'stdin', io.TextIOWrapper(io.BytesIO(b'abc'), encoding='utf-8'))
    with pytest.raises(SystemExit, match='expected'):
        MJ.main(['put', dest, '0' * 64])
    assert not os.path.exists(dest) and os.listdir(tmp_path / 'x') == []
    monkeypatch.setattr(sys, 'stdin', io.TextIOWrapper(io.BytesIO(b'abc'), encoding='utf-8'))
    assert MJ.main(['put', dest, hashlib.sha256(b'abc').hexdigest()]) == 0 and open(dest, 'rb').read() == b'abc'
    # have: missing / different listed, a null removes a stale copy
    stale = str(tmp_path / 'x' / 'config.json')
    open(stale, 'w').write('{}')
    want = {dest: hashlib.sha256(b'abc').hexdigest(), str(tmp_path / 'nope'): 'f' * 64, stale: None}
    monkeypatch.setattr(sys, 'stdin', io.StringIO(json.dumps(want)))
    out = io.StringIO()
    monkeypatch.setattr(sys, 'stdout', out)
    assert MJ.main(['have']) == 0
    assert json.loads(out.getvalue()) == [str(tmp_path / 'nope')] and not os.path.exists(stale)


def _records(path):
    with gzip.open(path, 'rt', encoding='utf-8') as f:
        return [json.loads(x) for x in f if x.strip()]


def test_the_macs_game_records_land_in_the_measurements_records_directory(tmp_path, monkeypatch):
    world = World()
    FakeMac(tmp_path, monkeypatch, world)
    ck, ref = _ckpts(tmp_path)
    ys, rec = _ys(), str(tmp_path / 'records')
    with GR.recording(Y._sink(rec, 'cells', 16, 'iter_0016', 'gen0', {'ref': 'gen0', 'role': 'main'})) as sink:
        p = MJ.play_pair_mac(ck, ref, (D, S), 40, ys, {'select': 'cq'}, log=lambda m: None)
    files = sorted(os.listdir(os.path.join(rec, 'cells', 'iter_0016')))
    assert len(files) == 2 and files[0].startswith('mac-') and files[0].endswith('-c0.jsonl.gz')
    assert sink.stats['files'] == 2 and sink.stats['games'] == 80
    for c, name in enumerate(files):
        lines = _records(os.path.join(rec, 'cells', 'iter_0016', name))
        assert lines[0]['kind'] == 'build' and lines[1]['kind'] == 'spec'
        games = lines[2:]
        assert [g['gid'] for g in games] == list(range(40))
        assert {g['kind'] for g in games} == {'cells'} and {g['it'] for g in games} == {16}
        assert {g['src'] for g in games} == {'mac'}
        assert games[0]['meta'] == {'ref': 'gen0', 'role': 'main', 'pair': [D, S], 'chooser': c, 'backend': 'mac',
                                    'driver_seed': Y.pair_seed(ys, (D, S)), 'paired': False}
        assert [g['players'][g['gid'] % 2]['label'] for g in games] == ['iter_0016'] * 40       # (A in seat gid % 2)
        assert games[0]['players'][0]['ckpt'] == 'run/iter_0016' and games[1]['players'][1]['ckpt'] == 'run/iter_0016'
        assert games[0]['players'][0]['sha'] == GR.file_sha(ck)                                  # (the same bytes)
        assert [g['result'][g['gid'] % 2] for g in games] == p['results'][str(c)]
    # records that disagree with the results: refused loudly, nothing written
    real = MJ.play_job

    def lying(job):
        out = real(job)
        lines = gzip.decompress(base64.b64decode(out['records'])).decode().splitlines()
        g = json.loads(lines[-1])
        g['result'] = g['result'][::-1]
        lines[-1] = json.dumps(g)
        out['records'] = base64.b64encode(gzip.compress(('\n'.join(lines) + '\n').encode())).decode()
        return out
    monkeypatch.setattr(MJ, 'play_job', lying)
    rec2 = str(tmp_path / 'records2')
    with GR.recording(Y._sink(rec2, 'cells', 16, 'iter_0016', 'gen0')):
        with pytest.raises(RuntimeError, match='disagree'):
            MJ.play_pair_mac(ck, ref, (D, S), 40, ys, {'select': 'cq'}, log=lambda m: None)
    assert not os.path.exists(os.path.join(rec2, 'cells'))


def test_a_mirror_cell_on_identical_deals_scores_what_its_games_say_not_one_half(tmp_path, monkeypatch):
    """A mirror (X against X) is two matches on the same deals (one seed: game gid is the same deal in both chooser
    matches) with different networks: its cell is the checkpoint's score in those games - here 0.7 by construction,
    in both halves (its side choosing, the other side choosing) - on the Mac as here, never 0.5 by structure (a Mac
    run of the 104-iteration A/B, 2026-11-18, read exactly 0.5 in the Delver and Sneak mirrors)."""
    world = World(p_a=0.7)
    FakeMac(tmp_path, monkeypatch, world)
    ck, ref = _ckpts(tmp_path)
    ys = _ys(cells_games=400)
    rows = {}
    for backend in ('local', 'mac'):
        world.calls.clear()
        rows[backend] = Y.measure_cells_core('cells', ck, 16, ref, 'gen0', (N, N), 400, ys, {'select': 'cq'},
                                             str(tmp_path / 'cache'), backend=backend, log=lambda m: None)
        assert len(world.calls) == 2 and world.calls[0]['seed'] == world.calls[1]['seed']     # (identical deals)
        assert {c['a'] for c in world.calls} == {'iter_0016.pt'} and {c['b'] for c in world.calls} == {'iter_0000.pt'}
        assert sorted(json.loads(c['spec'])['chooser'] for c in world.calls) == [0, 1]
    assert rows['mac']['results'] == rows['local']['results']
    for backend, row in rows.items():
        c = row['cells'][Y.cell_id(N, N)]
        res = [Y.res_decode(row['results'][k]) for k in ('0', '1')]
        mean = sum(map(sum, res)) / sum(map(len, res))
        assert c['games'] == 400 and c['score'] == round(mean, 4) and abs(c['score'] - 0.7) < 0.06, (backend, c)
        assert abs(c['chose']['score'] - 0.7) < 0.08 and abs(c['other_chose']['score'] - 0.7) < 0.08
        assert c['base'] == 0.5 and c['elo'] > 100 and row['backend'] == backend
    assert rows['mac']['mac']['commit'] == 'a' * 12


# ---------------------------------------------------------------- the watcher's parallel lane

def test_the_mac_lane_runs_cells_mac_jobs_halved_at_once_each_cell_after_its_baseline(tmp_path, stub_seat):
    import run_yardsticks as RY
    ys = {'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_refs': ['iter0'], 'cells_device': 'cpu',
          'cells_backend': 'mac', 'cells_mac_jobs': 4, 'cells_boot': 0}
    run, cfg = _run(tmp_path, (0, 16, 32), ys)
    w = RY.Watcher(run, once=True, log=lambda m: None, poll=0.01)
    w.tick = 0.01
    state = {'now': 0, 'max': 0}
    lock = threading.Lock()
    spans = []

    def measure(item, ys_):
        with lock:
            state['now'] += 1
            state['max'] = max(state['max'], state['now'])
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
    assert state['max'] == 2                                                # (cells_mac_jobs 4: two at once)
    assert sorted((k, i, p) for k, i, p, _, _ in spans) == sorted(
        [('cells_base', None, p) for p in Y.cell_pairs(Y.normalize(ys)) if p[0] != p[1]]
        + [('cells', it, p) for it in (16, 32) for p in Y.cell_pairs(Y.normalize(ys))])     # (each once)
    ends = {p: e for k, _, p, _, e in spans if k == 'cells_base'}
    assert all(t0 >= ends[p] for k, _, p, t0, _ in spans if k == 'cells' and p in ends)   # (after its baseline)
    order = [(i, p) for k, i, p, _, _ in sorted(spans, key=lambda s: s[3]) if k == 'cells']
    assert [i for i, _ in order][:6].count(16) >= 5                         # (oldest checkpoint first)


def test_take_over_stops_a_companion_started_through_a_venv_launcher(tmp_path, monkeypatch):
    """Windows: a venv's python.exe is a launcher - the supervisor records its pid, the lock holds its child's (the
    interpreter's). The take-over knows the child as the companion and stops the launcher's whole tree."""
    import run_yardsticks as RY
    from zero import runctl
    assert RY._is_or_child_of(os.getpid(), os.getpid()) and RY._is_or_child_of(os.getpid(), os.getppid())
    assert not RY._is_or_child_of(None, 1) and not RY._is_or_child_of(os.getpid(), None)
    run = str(tmp_path)
    killed, held = [], {'pid': 7001}
    monkeypatch.setattr(RY, 'take_lock', lambda r: held['pid'] is None)
    monkeypatch.setattr(RY, 'lock_owner', lambda r: held['pid'])
    monkeypatch.setattr(RY, '_is_or_child_of', lambda pid, parent: pid == parent or (pid, parent) == (7001, 7000))

    def kill(pid):
        killed.append(pid)
        held['pid'] = None
    monkeypatch.setattr(RY, 'kill_pid_tree', kill)
    state = {'companion': {'pid': 7000, 'cmd': ['venv/python.exe', 'x/tools/run_yardsticks.py', 'watch', run]}}
    monkeypatch.setattr(runctl, 'read_state', lambda r, name: state)
    w = RY.Watcher(run, take_over=True, log=lambda m: None)
    assert w.seize(wait_s=0.2) and killed == [7000]
    held['pid'], killed[:] = 7002, []                                      # (not the launcher's child)
    assert not w.seize(wait_s=0.2) and killed == []


# ---------------------------------------------------------------- the ssh side

def test_the_macs_command_line_and_its_job_protocol(monkeypatch):
    rt = {'host': 'user@mac', 'home': '/Users/user', 'python': '/Users/user/.venvs/v/bin/python',
          'repo': '/Users/user/mtg-zero-wt/run-aaaaaaaaaaaa', 'work': '/Users/user/mtgz-judge', 'commit': 'a' * 40}
    ys = _ys()
    sh = MJ._in_repo(rt, 'play', env={'OMP_NUM_THREADS': 2, 'MTGZ_REMOTE_COMMIT': rt['commit']}, nice=10)
    assert sh.startswith('cd /Users/user/mtg-zero-wt/run-aaaaaaaaaaaa 2>/dev/null || {')
    assert sh.endswith(f"OMP_NUM_THREADS=2 MTGZ_REMOTE_COMMIT={'a' * 40} exec nice -n 10 "
                       f"/Users/user/.venvs/v/bin/python -m mtgz.az.mac_judge play")
    assert 'bash tools/mac.sh --eval-checkout' in sh
    fake = ('import json, sys\n'
            'job = json.loads(sys.stdin.readline())\n'
            'print("progress line", file=sys.stderr, flush=True)\n'
            'if job["chooser"] == 1:\n'
            '    print("Traceback: boom", file=sys.stderr, flush=True)\n'
            '    sys.exit(1)\n'
            'print("noise")\n'
            'print(json.dumps({"rows": [], "games": job["games"], "seconds": 1.0}), flush=True)\n')
    seen = []
    monkeypatch.setattr(MJ, '_ssh', lambda rt_, s: seen.append(s) or [sys.executable, '-c', fake])
    logs = []
    out = MJ.run_job(rt, {'chooser': 0, 'games': 7}, ys, log=logs.append)
    assert out == {'rows': [], 'games': 7, 'seconds': 1.0} and 'mac c0: progress line' in logs
    assert 'nice -n 10' in seen[0] and 'OMP_NUM_THREADS=2' in seen[0]
    with pytest.raises(RuntimeError, match='boom'):
        MJ.run_job(rt, {'chooser': 1, 'games': 7}, ys, log=logs.append)


def test_the_macs_process_ends_when_its_stdin_closes():
    """The desktop keeps the Mac process's stdin open while it waits; the watcher killing its child (or the connection
    dropping) closes it, and the Mac's process ends - no orphan games there."""
    code = ('import sys, threading, time\n'
            f'sys.path.insert(0, {ROOT!r})\n'
            'from mtgz.az import mac_judge as M\n'
            'threading.Thread(target=M._die_with_stdin, daemon=True).start()\n'
            'print("up", flush=True)\n'
            'time.sleep(120)\n')
    p = subprocess.Popen([sys.executable, '-c', code], stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=ROOT)
    assert p.stdout.readline().strip() == b'up'
    p.stdin.close()
    assert p.wait(timeout=60) == 75


PLAY = (
    'import json, sys\n'
    'sys.path.insert(0, {root!r})\n'
    'from mtgz.az import mac as MAC, mac_judge as M\n'
    'MAC.verify_checkout = lambda root, expect=None: None\n'
    'def play(job):\n'
    '    if job["games"] < 0:\n'
    '        raise RuntimeError("no such match")\n'
    '    return {{"rows": [], "games": job["games"]}}\n'
    'M.play_job = play\n'
    'M.cmd_play()\n')


@pytest.mark.parametrize('games, code', [(5, 0), (-1, 1)])
def test_the_macs_play_ends_with_its_result_while_stdin_stays_open(games, code):
    """The Mac's play exits 0 after its result line (1 with the traceback on a failure) while the desktop still holds
    its stdin open - never an abort at the interpreter's shutdown with the stdin watcher's thread blocked in a read
    (the first smoke's exit 255, 'could not acquire lock for <_io.BufferedReader name=<stdin>>')."""
    p = subprocess.Popen([sys.executable, '-c', PLAY.format(root=ROOT)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, cwd=ROOT)
    p.stdin.write(json.dumps({'games': games}).encode() + b'\n')
    p.stdin.flush()
    out = p.stdout.read()
    err = p.stderr.read()
    assert p.wait(timeout=60) == code, err
    p.stdin.close()
    if code == 0:
        assert json.loads(out.decode().strip().splitlines()[-1]) == {'rows': [], 'games': 5}
    else:
        assert b'no such match' in err and b'Fatal Python error' not in err


def test_a_dirty_checkout_is_refused_and_the_remote_names_the_commits_checkout(monkeypatch):
    from mtgz.az import mac as M
    monkeypatch.setattr(mtgz.az, 'MAC_HOME', '/Users/user')
    monkeypatch.setattr(mtgz.az, 'MAC_PYTHON', '/Users/user/.venvs/v/bin/python')
    monkeypatch.setattr(M, 'checkout_state', lambda root: ('b' * 40, [' M mtgz/az/yardsticks.py']))
    with pytest.raises(RuntimeError, match='uncommitted'):
        MJ.remote()
    monkeypatch.setattr(M, 'checkout_state', lambda root: ('b' * 40, []))
    monkeypatch.setattr(M, 'moved_problem', lambda root, head: None)
    rt = MJ.remote()
    assert rt['repo'] == '/Users/user/mtg-zero-wt/run-' + 'b' * 12 and rt['commit'] == 'b' * 40
    assert rt['work'] == '/Users/user/' + MJ.WORK and rt['python'] == '/Users/user/.venvs/v/bin/python'
    # Git Bash's conversion of an exported MTGZ_MAC_HOME for a Windows process: refused loudly, never used
    monkeypatch.setattr(mtgz.az, 'MAC_HOME', 'C:/Program Files/Git/Users/user')
    with pytest.raises(RuntimeError, match='not a POSIX path'):
        MJ.remote()


def test_the_mirror_paths_keep_each_runs_name_apart(tmp_path):
    a = MJ.remote_path('/w', os.path.join(str(tmp_path), 'disk1', 'runs', 'r1', 'models', 'iter_0016.pt'))
    b = MJ.remote_path('/w', os.path.join(str(tmp_path), 'disk2', 'runs', 'r1', 'models', 'iter_0016.pt'))
    assert a[0] != b[0] and a[0].endswith('/r1/models/iter_0016.pt') and b[0].endswith('/r1/models/iter_0016.pt')
    ref = MJ.remote_path('/w', os.path.join(str(tmp_path), 'runs', 'r1', 'yardsticks', 'cells_refs', 'g32.pt'))
    assert ref[0].endswith('/yardsticks/cells_refs/g32.pt') and ref[1].endswith('/yardsticks')
