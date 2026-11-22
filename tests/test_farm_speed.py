"""The farm release's speed gate (tools/farm_speed.py, docs/ENGINE.md 200): the pass / fail rules, the noisy retry,
the baseline lookup, the ONNX facts, the idle rules (waiting, discarding, "machine in use"), the quiet lock, the
runner's windows with a fake worker, the result files and farm_release.py's speed step. No real worker, no Mac."""
import json
import os
import shutil
import subprocess
import sys
import types

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import farm_speed as F  # noqa: E402

A, B, C, D, X = ('a' * 24, 'b' * 24, 'c' * 24, 'd' * 24, 'e' * 24)


def win(i, side, rate, ok=True, **kw):
    return dict({'index': i, 'side': side, 'leaves_per_s': rate, 'ok': ok}, **kw)


def abba(new, old, new2=None, old2=None, first=0):
    return [win(first, 'new', new), win(first + 1, 'old', old), win(first + 2, 'old', old2 or old), win(first + 3, 'new', new2 or new)]


# ------------------------------------------------------------------------------------------------ the judge

def test_the_pass_rule_is_the_ratio_of_the_sides_means():
    j = F.judge(abba(1000, 1000), 0.05)
    assert j['passed'] and j['ratio'] == 1.0 and j['pairs'] == [1.0, 1.0] and not j['noisy']
    assert F.judge(abba(960, 1000), 0.05)['passed'], '4% slower passes at a 5% tolerance'
    j = F.judge(abba(940, 1000), 0.05)
    assert not j['passed'] and not j['error'] and j['ratio'] == 0.94
    assert F.judge(abba(1200, 1000), 0.05)['passed'], 'faster passes'
    j = F.judge(abba(1000, 1000, new2=1100), 0.05)
    assert j['spread']['new'] == round(100 / 1050, 4) and j['noisy']
    j = F.judge(abba(1000, 900, new2=1000, old2=1100), 0.05)
    assert j['pairs'] == [round(1000 / 900, 4), round(1000 / 1100, 4)] and j['ratio'] == 1.0 and j['noisy']
    j = F.judge(abba(1000, 1000, new2=1060, old2=980), 0.05)
    assert j['spread'] == {'new': round(60 / 1030, 4), 'old': round(20 / 990, 4)} and j['pair_spread'] == round(1060 / 980 - 1, 4)
    assert j['noisy'], 'the two pairs disagree by more than the tolerance (1.00 vs 1.08): noisy, though each side is tight'


def test_an_error_on_either_side_fails_and_discarded_windows_do_not_count():
    ws = abba(1000, 1000)
    ws[1] = win(1, 'old', 0, ok=False, error='the worker exited (status 3)')
    j = F.judge(ws, 0.05)
    assert not j['passed'] and j['error'] and 'window 1 (old)' in j['why'] and not j['in_use']
    ws = abba(1000, 1000) + [win(4, 'new', 0, ok=False, in_use=True, error='machine in use: ...')]
    j = F.judge(ws, 0.05)
    assert not j['passed'] and j['in_use']
    ws = [win(0, 'new', 500, discarded=True, why='user input'), *abba(1000, 1000, first=1)]
    j = F.judge(ws, 0.05)
    assert j['passed'] and j['windows'] == 4 and j['discarded'] == 1
    assert not F.judge([win(0, 'new', 1000)], 0.05)['passed'], 'no window of the old side'


def test_a_noisy_failure_gets_one_more_block_a_clean_one_does_not():
    plan = {'machine': 'cpu', 'blocks': 1, 'retry_blocks': 1, 'order': F.ORDER}
    blocks = iter([abba(880, 1000, new2=1000), abba(1000, 1000)])
    calls = []

    orders = []

    def runner(order, first):
        calls.append(first)
        orders.append(order)
        return [dict(w, index=w['index'] + first) for w in next(blocks)]
    ws, j, k = F.run_blocks(runner, plan, 0.05, log=lambda m: None)
    assert k == 2 and calls == [0, 4] and len(ws) == 8 and j['passed'], 'noisy (new 880 and 1,000 / 1,000): judged on both blocks'
    assert orders == [['new', 'old', 'old', 'new'], ['old', 'new', 'new', 'old']], 'the second block mirrored'
    blocks = iter([abba(900, 1000), abba(1000, 1000)])
    ws, j, k = F.run_blocks(runner, plan, 0.05, log=lambda m: None)
    assert k == 1 and not j['passed'], 'a clean 0.9x is a regression, not noise: no retry'
    blocks = iter([[win(0, 'new', 0, ok=False, error='boom')]])
    ws, j, k = F.run_blocks(runner, plan, 0.05, log=lambda m: None)
    assert k == 1 and j['error']
    blocks = iter([abba(1000, 1000), abba(1000, 1000)])
    ws, j, k = F.run_blocks(runner, dict(plan, blocks=2), 0.05, log=lambda m: None)
    assert k == 2 and len(ws) == 8


def wstat(i, side, rates, others, own=None):
    """A window with 10 s status lines after a 30 s warmup at `rates` and an 'others' CPU sample per interval."""
    st, samples, leaves = [{'wall_s': 30.0, 'leaves': 0}], [], 0
    for k, (r, o) in enumerate(zip(rates, others)):
        leaves += r * 10
        st.append({'wall_s': 40.0 + 10 * k, 'leaves': leaves})
        samples.append({'t': 40.0 + 10 * k, 'others_pct': o})
    return win(i, side, own if own is not None else F.quantile(rates, 0.75), status=st, samples=samples, warmup_s=30.0)


def test_bursts_of_other_load_are_left_out_and_a_contended_window_makes_the_machine_noisy():
    clean = ([1000] * 8, [3] * 8)
    burst = ([1000] * 3 + [500] * 5, [3] * 3 + [70] * 5)        # (5 of 8 intervals under another process's burst)
    ws = [wstat(0, 'new', *clean), wstat(1, 'old', *clean), wstat(2, 'old', *burst, own=687.5), wstat(3, 'new', *clean)]
    j = F.judge(ws, 0.05, 20)
    assert j['passed'] and j['ratio'] == 1.0 and j['contended_windows'] == [2] and j['noisy'] and j['others_baseline'] == 3.0
    assert j['contended_intervals'] == 5 and j['rates'] == {'new': [1000.0, 1000.0], 'old': [1000.0]}
    j = F.judge(ws, 0.05, None)
    assert j['ratio'] == round(1000 / ((1000 + 687.5) / 2), 4), 'no margin: the windows\' own rates'
    # (a burst over 2 of 8 intervals is left out of that window's rate: the rest decides)
    short = ([1100, 1000, 900, 500, 500, 1000, 1100, 900], [3, 3, 3, 70, 70, 3, 3, 3])
    j = F.judge([wstat(0, 'new', *clean), wstat(1, 'old', *short), wstat(2, 'old', *clean), wstat(3, 'new', *clean)], 0.05, 20)
    assert j['contended_intervals'] == 2 and j['contended_windows'] == [] and j['rates']['old'][0] == F.quantile([1100, 1000, 900, 1000, 1100, 900], 0.75)
    # (a constant background - the Mac's ~30% - is the baseline, not a burst)
    bg = ([1000] * 8, [30] * 8)
    j = F.judge([wstat(k, s, *bg) for k, s in enumerate(F.ORDER)], 0.05, 20)
    assert j['others_baseline'] == 30.0 and j['contended_intervals'] == 0 and not j['noisy']
    # (every window of a side contended: busy - a failure that a retry block may still turn around, not an error)
    j = F.judge([wstat(0, 'new', *clean), wstat(1, 'old', *burst), wstat(2, 'old', *burst), wstat(3, 'new', *clean)], 0.05, 20)
    assert not j['passed'] and not j['error'] and j['busy'] and j['noisy'] and 'machine busy' in j['why']


def test_the_steady_rate_skips_the_warmup():
    st = [{'wall_s': 10.0, 'leaves': 5000}, {'wall_s': 20.0, 'leaves': 25000}, {'wall_s': 30.0, 'leaves': 45000}]
    r = F.steady_rate(st, 15)
    assert r == {'leaves_per_s': 2000.0, 'median': 2000.0, 'mean': 2000.0, 'from_s': 20.0, 'to_s': 30.0, 'leaves': 20000,
                 'intervals': 1, 'steady': True}
    # (the upper quartile of the 10 s rates: a burst of other load - one slow interval - moves the mean, not the rate)
    st2 = [{'wall_s': 0.0, 'leaves': 0}] + [{'wall_s': 30.0 + 10 * i, 'leaves': 1000 * i} for i in range(6)]
    st2[4]['leaves'] = st2[3]['leaves'] + 400                   # (30 s in: 40 leaves/s instead of 100)
    for k in range(5, len(st2)):
        st2[k]['leaves'] = st2[k - 1]['leaves'] + 1000
    r = F.steady_rate(st2, 30)
    assert r['leaves_per_s'] == 100.0 and r['median'] == 100.0 and r['mean'] == 88.0 and r['intervals'] == 5
    # (a burst over half the window - 10 s rates 100, 40, 40, 40, 100, 100 -: the median and the mean move, the rate not)
    st3 = [{'wall_s': 30.0 + 10 * i, 'leaves': n} for i, n in enumerate((0, 1000, 1400, 1800, 2200, 3200, 4200))]
    r = F.steady_rate(st3, 30)
    assert (r['leaves_per_s'], r['median'], r['mean'], r['intervals']) == (100.0, 70.0, 70.0, 6)
    r = F.steady_rate(st, 25, end={'wall_s': 31.0, 'leaves': 46500})
    assert not r['steady'] and r['leaves_per_s'] == 1500.0, 'one status after the warmup: the whole job'
    assert F.steady_rate([], 30)['leaves_per_s'] == 0.0


# ------------------------------------------------------------------------------------------------ the baseline

def entry(commit, bid, published=True, cuda=None, mac=None, reused_from=None):
    e = {'commit': commit * 40 if len(commit) == 1 else commit, 'build_id': bid, 'published': published}
    if cuda:
        e['cuda'] = {'build_id': cuda}
    if mac:
        e['mac'] = {'release': {'build_id': mac}}
    if reused_from:
        e['reused_from'] = reused_from
    return e


def test_the_baseline_is_the_previous_published_release_build():
    ix = {'builds': [entry('1', A, cuda='1' * 24), entry('2', B, cuda='2' * 24, mac='m' * 24), entry('3', B, reused_from='2' * 40),
                     entry('4', C, published=False), entry('5', D, cuda='5' * 24)]}
    assert F.find_baseline(ix, 'cpu', X)[0] == D, 'a build not in the index: the newest published one'
    assert F.find_baseline(ix, 'cpu', D)[0] == B, 'one in the index: the newest before its first entry'
    assert F.find_baseline(ix, 'cpu', B)[0] == A
    assert F.find_baseline(ix, 'cpu', A) == (None, None)
    assert F.find_baseline(ix, 'cpu', C)[0] == D, 'an unpublished entry is no release'
    assert F.find_baseline(ix, 'cuda', 'z' * 24)[0] == '5' * 24
    assert F.find_baseline(ix, 'cuda', '5' * 24)[0] == '2' * 24
    assert F.find_baseline(ix, 'mac', 'n' * 24)[0] == 'm' * 24
    assert F.find_baseline(ix, 'mac', 'm' * 24) == (None, None)
    assert F.find_baseline(ix, 'cpu', X)[1]['commit'] == '5' * 40


# ------------------------------------------------------------------------------------------------ ONNX facts

def _varint(n):
    out = b''
    while True:
        b = n & 0x7F
        n >>= 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out


def _field(num, payload):
    return _varint(num << 3 | 2) + _varint(len(payload)) + payload


def fake_onnx(path, graph=b'GRAPH', **meta):
    """A ModelProto with ir_version, a graph and metadata_props (enough for farm_speed.onnx_info)."""
    body = _varint(1 << 3) + _varint(8) + _field(2, b'pytorch') + _field(7, graph)
    for k, v in meta.items():
        body += _field(14, _field(1, k.encode()) + _field(2, str(v).encode()))
    with open(path, 'wb') as f:
        f.write(body)
    return path


def test_onnx_info_reads_the_metadata_and_digests_the_graph_without_it(tmp_path):
    a = F.onnx_info(fake_onnx(tmp_path / 'a.onnx', checkpoint='run/206', checkpoint_iter=206, zero_onnx=2))
    assert a['meta']['checkpoint_iter'] == '206' and a['meta']['checkpoint'] == 'run/206' and a['meta']['zero_onnx'] == '2'
    b = F.onnx_info(fake_onnx(tmp_path / 'b.onnx', checkpoint='run/206', checkpoint_iter=206, zero_onnx=2, exporter='other'))
    assert b['sha256'] != a['sha256'] and b['graph_sha256'] == a['graph_sha256'], 'metadata is not the graph'
    c = F.onnx_info(fake_onnx(tmp_path / 'c.onnx', graph=b'OTHER', checkpoint_iter=206))
    assert c['graph_sha256'] != a['graph_sha256']


# ------------------------------------------------------------------------------------------------ idle

class Clock:
    def __init__(self):
        self.t = 0.0

    def __call__(self):
        return self.t

    def sleep(self, s):
        self.t += s


def scripted_idle(monkeypatch, seq):
    """F.idle_state answers from `seq` (each (idle_s, ac)); the last one repeats."""
    it = list(seq)

    def f():
        return it.pop(0) if len(it) > 1 else it[0]
    monkeypatch.setattr(F, 'idle_state', f)


def test_waiting_for_an_idle_machine_on_ac_power(monkeypatch):
    cfg = {'min_s': 180, 'wait_max_s': 3600}
    c = Clock()
    scripted_idle(monkeypatch, [(500.0, True)])
    r = F.wait_idle(cfg, log=lambda m: None, clock=c, sleep=c.sleep)
    assert r['ok'] and r['checks'] == 1 and r['waited_s'] == 0
    c, waited = Clock(), []
    scripted_idle(monkeypatch, [(5.0, True), (60.0, True), (200.0, False), (300.0, True)])
    r = F.wait_idle(cfg, log=lambda m: None, clock=c, sleep=c.sleep, on_wait=lambda: waited.append(1))
    assert r['ok'] and r['checks'] == 4 and r['waited_s'] > 0 and waited == [1], 'battery power waits too; on_wait once'
    c = Clock()
    scripted_idle(monkeypatch, [(3.0, True)])
    r = F.wait_idle(cfg, log=lambda m: None, clock=c, sleep=c.sleep)
    assert not r['ok'] and r['in_use'] and 'machine in use' in r['why']
    scripted_idle(monkeypatch, [(None, True)])
    r = F.wait_idle(cfg, log=lambda m: None, clock=Clock(), sleep=lambda s: None)
    assert not r['ok'] and 'cannot tell' in r['why']
    assert F.parse_pmset("Now drawing from 'AC Power'\n -InternalBattery-0") is True
    assert F.parse_pmset("Now drawing from 'Battery Power'\n") is False and F.parse_pmset('') is None


def test_a_held_evaluation_lock_keeps_the_window_waiting(monkeypatch, tmp_path):
    lock = tmp_path / '.eval.lock'
    lock.mkdir()
    (lock / 'owner').write_text(f'{os.getpid()} 1')
    scripted_idle(monkeypatch, [(500.0, True)])
    c = Clock()
    r = F.wait_idle({'min_s': 0, 'wait_max_s': 60}, log=lambda m: None, clock=c, sleep=c.sleep, avoid=[str(lock)])
    assert not r['ok'] and 'machine busy' in r['why'] and not r.get('in_use')
    (lock / 'owner').write_text('999999999 1')
    assert not F.lock_held(str(lock)), 'a dead owner: stale, not held'
    (lock / 'owner').unlink()
    assert F.lock_held(str(lock)), 'no owner yet: being created'
    shutil.rmtree(lock)
    assert F.wait_idle({'min_s': 0}, log=lambda m: None, clock=Clock(), sleep=lambda s: None, avoid=[str(lock)])['ok']


@pytest.mark.skipif(not shutil.which('sleep'), reason='needs a sleep executable (the holder)')
def test_the_quiet_lock_is_held_by_a_live_sleep_and_released(tmp_path):
    lock = tmp_path / 'domz-remote' / '.eval.lock'
    q = F.Quiet({'hold': str(lock), 'settle_s': 0}, 600, log=lambda m: None)
    r = q.acquire()
    assert r['ok'] and q.held and lock.is_dir() and F.lock_held(str(lock))
    assert (lock / 'owner').read_text().split()[0] == str(q.holder.pid)
    q.release()
    assert not lock.exists() and not q.held and 'released' in q.records[-1]
    lock.mkdir(parents=True)
    (lock / 'owner').write_text('999999999 1')
    r = q.acquire()
    assert r['ok'] and r['took_over_stale']
    q.release()
    lock.mkdir()
    (lock / 'owner').write_text(f'{os.getpid()} 1')            # held by a live process (this one)
    assert not q.acquire(wait_max=0)['ok'] and not q.held
    assert lock.is_dir(), 'never another holder\'s lock removed'


# ------------------------------------------------------------------------------------------------ the runner, a fake worker

FAKE = r'''
import json, os, sys, threading, time
mode, bid = sys.argv[1], sys.argv[2]
if mode == 'info':
    print(json.dumps({'build_id': bid, 'features': []}))
    sys.exit(0)
rate = float(sys.argv[3])
job = json.loads(sys.stdin.readline())
assert job['op'] == 'job' and job['kind'] == 'selfplay' and job['rolling'] is True and job['threads'] == 3
t0 = time.time()
if os.environ.get('FAKE_HOLD'):
    print(json.dumps({'ev': 'note', 'hold_held': os.path.isdir(os.environ['FAKE_HOLD'])}), flush=True)
take = os.environ.get('FAKE_TAKE_LOCK')          # (once: an evaluation lock owned by this process, 0.5 s in)
if take and not os.path.exists(take + '.done'):
    def taker():
        time.sleep(0.5)
        os.makedirs(take)
        open(os.path.join(take, 'owner'), 'w').write(f'{os.getpid()} 1')
        open(take + '.done', 'w').close()
    threading.Thread(target=taker, daemon=True).start()
stop = threading.Event()
def read():
    for ln in sys.stdin:
        if json.loads(ln).get('op') == 'quit':
            stop.set()
            return
threading.Thread(target=read, daemon=True).start()
while not stop.wait(0.2):
    w = time.time() - t0
    if rate < 0 and w > 0.5:
        print(json.dumps({'ev': 'error', 'error': 'boom'}), flush=True)
        sys.exit(3)
    print(json.dumps({'ev': 'status', 'stats': {'wall_s': round(w, 3), 'leaves': int(rate * w), 'games': 0, 'infer_batches': 4,
                                                'infer_rows': 40, 'infer_ms': 20.0}}), flush=True)
w = time.time() - t0
print(json.dumps({'ev': 'end', 'reason': 'quit', 'stats': {'wall_s': w, 'leaves': int(rate * w), 'games': 2, 'infer_batches': 4,
                                                           'infer_rows': 40, 'infer_ms': 20.0, 'peak_rss_mb': 10}}), flush=True)
'''


def fake_plan(tmp_path, rates, minutes=0.05, idle=None, wrong_id=False):
    fake = tmp_path / 'fake_worker.py'
    fake.write_text(FAKE)
    sides = {}
    for name, rate in rates.items():
        bid = name[0] * 24
        sides[name] = {'build_id': bid, 'model': str(tmp_path / 'model.onnx'),
                       'cmd': [sys.executable, str(fake), 'serve', bid, str(rate)],
                       'info_cmd': [sys.executable, str(fake), 'info', ('x' * 24) if wrong_id else bid]}
    return {'machine': 'cpu', 'minutes': minutes, 'warmup_s': 0.5, 'seed': 1, 'gen': 206, 'settings': {'game': 'mtg'},
            'job': dict(F.JOB_BASE, threads=3, conc=8, groups=2), 'sample_s': 0.5, 'quit_s': 20,
            'idle': idle or {'check': True, 'min_s': 0, 'wait_max_s': 5, 'redo': 1, 'poll_s': 0.5}, 'sides': sides}


def test_the_runner_measures_each_window_with_a_fake_worker(tmp_path, monkeypatch):
    scripted_idle(monkeypatch, [(1e6, True)])
    plan = fake_plan(tmp_path, {'new': 1000, 'old': 2000})
    ws = F.run_windows(plan, str(tmp_path / 'out'), F.ORDER, 0, log=lambda m: None)
    assert [w['side'] for w in ws] == F.ORDER and all(w['ok'] for w in ws)
    for w in ws:
        want = 1000 if w['side'] == 'new' else 2000
        assert abs(w['leaves_per_s'] - want) < 0.1 * want and w['rate']['steady'] and w['games'] == 2
        assert w['idle']['before']['ok'] and w['idle']['end']['idle_s'] == 1e6 and w['batch_rows_mean'] == 10.0
        assert os.path.exists(tmp_path / 'out' / f'w{w["index"]}-{w["side"]}' / 'events.jsonl')
    j = F.judge(ws, 0.05)
    assert not j['passed'] and abs(j['ratio'] - 0.5) < 0.05
    rec = json.load(open(tmp_path / 'out' / 'windows-0.json'))
    assert len(rec['windows']) == 4 and rec['host']['logical'] and set(rec['infos']) == {'new', 'old'}


@pytest.mark.skipif(not shutil.which('sleep'), reason='needs a sleep executable (the holder)')
def test_the_runner_holds_the_quiet_lock_and_yields_to_the_runs_evaluation(tmp_path, monkeypatch):
    scripted_idle(monkeypatch, [(1e6, True)])
    hold, avoid = tmp_path / 'domz-remote' / '.eval.lock', tmp_path / 'mtgz-remote' / '.eval.lock'
    avoid.parent.mkdir()
    plan = fake_plan(tmp_path, {'new': 1000, 'old': 1000})
    plan['quiet'] = {'hold': str(hold), 'avoid': [str(avoid)], 'settle_s': 0}
    plan['sides']['new']['env'] = {'FAKE_HOLD': str(hold), 'FAKE_TAKE_LOCK': str(avoid)}
    plan['sides']['old']['env'] = {'FAKE_HOLD': str(hold)}
    out = tmp_path / 'o'
    ws = F.run_windows(plan, str(out), ['new', 'old'], 0, log=lambda m: None)
    assert [w.get('discarded', False) for w in ws] == [True, False, False] and 'mtgz-remote' in ws[0]['why'], \
        "the run's evaluation took its lock during the first window: discarded (its owner, the fake, is gone: the redo runs)"
    assert all(w['quiet']['lock'] == str(hold) and w['quiet']['holder'] for w in ws) and ws[0]['ok'] is False
    for w in ws:
        notes = [json.loads(ln) for ln in open(out / f'w{w["index"]}-{w["side"]}' / 'events.jsonl') if '"note"' in ln]
        assert notes and notes[0]['hold_held'], 'the quiet lock is held while a window runs'
    assert not hold.exists(), 'released at the end'


def test_the_runner_books_the_bench_lock_for_its_sweep(tmp_path, monkeypatch):
    scripted_idle(monkeypatch, [(1e6, True)])
    lock = tmp_path / 'mtgz-macfarm' / 'bench.lock'
    plan = fake_plan(tmp_path, {'new': 1000, 'old': 1000})
    plan['bench_lock'] = str(lock)
    plan['sides']['new']['env'] = {'FAKE_HOLD': str(lock)}
    out = tmp_path / 'o'
    ws = F.run_windows(plan, str(out), ['new', 'old'], 0, log=lambda m: None)
    assert all(w['ok'] for w in ws) and not lock.exists(), 'booked for the sweep, removed after it'
    b = ws[0]['bench_lock']
    assert b['purpose'] == 'farm-speed-gate:cpu:nnnnnnnn-vs-oooooooo' and b['released'] and ws[1]['bench_lock'] == b
    notes = [json.loads(ln) for ln in open(out / 'w0-new' / 'events.jsonl') if '"note"' in ln]
    assert notes[0]['hold_held'], 'held while the windows run'
    # another agent's sweep holds it: wait (here not at all), then "machine busy" - never removed, no window run
    lock.mkdir(parents=True)
    (lock / 'owner').write_text('exporter-ab 1795050000')
    monkeypatch.setattr(F.time, 'sleep', lambda s: None)
    ws = F.run_windows(dict(plan, idle=dict(plan['idle'], wait_max_s=0)), str(tmp_path / 'o2'), ['new', 'old'], 0, log=lambda m: None)
    assert len(ws) == 1 and not ws[0]['ok'] and 'machine busy' in ws[0]['error'] and 'exporter-ab' in ws[0]['error']
    assert (lock / 'owner').read_text() == 'exporter-ab 1795050000'


def test_a_worker_error_or_the_wrong_build_fails_the_window(tmp_path, monkeypatch):
    scripted_idle(monkeypatch, [(1e6, True)])
    ws = F.run_windows(fake_plan(tmp_path, {'new': -1, 'old': 1000}), str(tmp_path / 'o1'), F.ORDER, 0, log=lambda m: None)
    assert len(ws) == 1 and not ws[0]['ok'] and 'ended the job early' in ws[0]['error']
    ws = F.run_windows(fake_plan(tmp_path, {'new': 1000, 'old': 1000}, wrong_id=True), str(tmp_path / 'o2'), F.ORDER, 0,
                       log=lambda m: None)
    assert ws and not any(w['ok'] for w in ws) and 'the worker says build' in ws[0]['error']


def test_input_during_a_window_discards_it_and_too_much_input_fails_as_machine_in_use(tmp_path, monkeypatch):
    # (the first window sees input at its end - the main thread's third question: the wait, the first check, the end -:
    # discarded, redone, then clean; the sampler thread's questions always see an idle machine)
    import threading
    main = []

    def idle_state():
        if threading.current_thread() is not threading.main_thread():
            return 1e6, True
        main.append(1)
        return (0.2, True) if len(main) == 3 else (1e6, True)
    monkeypatch.setattr(F, 'idle_state', idle_state)
    plan = fake_plan(tmp_path, {'new': 1000, 'old': 1000}, idle={'check': True, 'min_s': 0, 'wait_max_s': 5, 'redo': 1, 'poll_s': 100})
    ws = F.run_windows(plan, str(tmp_path / 'o'), ['new', 'old'], 0, log=lambda m: None)
    assert [w.get('discarded', False) for w in ws] == [True, False, False] and 'user input' in ws[0]['why']
    assert F.judge(ws, 0.05)['passed'] and F.judge(ws, 0.05)['discarded'] == 1
    # (input all the time: discarded twice with redo 1, then the machine is in use - a failure, never a pass)
    monkeypatch.setattr(F, 'idle_state', lambda: (0.2, True))
    ws = F.run_windows(plan, str(tmp_path / 'o2'), ['new', 'old'], 0, log=lambda m: None)
    assert [w.get('discarded', False) for w in ws[:2]] == [True, True] and not ws[-1]['ok'] and ws[-1]['in_use']
    j = F.judge(ws, 0.05)
    assert not j['passed'] and j['in_use'] and 'machine in use' in j['why']


# ------------------------------------------------------------------------------------------------ the gate's files

@pytest.fixture
def builds(tmp_path, monkeypatch):
    b = tmp_path / 'builds'
    b.mkdir()
    monkeypatch.setenv('MTGZ_FARM_BUILDS', str(b))
    rt = b / ('r' * 24)
    rt.mkdir()
    (rt / 'manifest.json').write_text(json.dumps({'kind': 'runtime', 'version': 'ort-test'}))
    ref = b / 'reference'
    (ref / 'export').mkdir(parents=True)
    fake_onnx(ref / 'export' / 'model.onnx', checkpoint='run/206', checkpoint_iter=206, zero_onnx=2)
    (ref / 'config.json').write_text(json.dumps({'game': 'mtg', 'seed': 1, 'shard_rows': 65536, 'selfplay_remote': 'mac'}))
    return b


def make_build(builds, commit, profile='release'):
    d = builds / f'dist-{commit}'
    d.mkdir()
    (d / 'manifest.json').write_text(json.dumps({'kind': 'worker', 'engine_commit': commit * 40, 'profile': profile,
                                                 'requires_runtime': 'r' * 24, 'entry': 'mtgz-worker.exe'}))
    bid = F.build_id_of(str(d))
    shutil.copytree(d, builds / bid)
    return bid, str(d)


def fake_runner(rates, seen):
    def run_windows(plan, out, order, first=0, log=None):
        seen.append(plan)
        os.makedirs(out, exist_ok=True)
        ws = [win(first + i, s, rates[s], batch_rows_mean=50.0, load={'others_pct': 10.0}, peak_rss_mb=400) for i, s in enumerate(order)]
        with open(os.path.join(out, f'windows-{first}.json'), 'w') as f:
            json.dump({'host': {'logical': 8, 'cpu': 'test cpu'}, 'windows': ws}, f)
        return ws
    return run_windows


def write_index(builds, entries):
    (builds / 'index.json').write_text(json.dumps({'schema': 1, 'builds': entries}))


def test_the_gate_finds_the_baseline_and_writes_its_files(builds, tmp_path, monkeypatch):
    old, _ = make_build(builds, '1')
    new, new_dir = make_build(builds, '2')
    write_index(builds, [entry('1', old)])
    seen = []
    monkeypatch.setattr(F, 'run_windows', fake_runner({'new': 990.0, 'old': 1000.0}, seen))
    out = tmp_path / 'speed'
    assert F.main(['gate', '--new', new_dir, '--out', str(out), '--minutes', '0.5', '--job', 'cpu:conc=64']) == 0
    res = json.load(open(out / 'result.json'))
    assert res['passed'] and res['line'].startswith('SPEED GATE PASSED: cpu 0.990x ok')
    assert res['baseline']['build_id'] == old and res['new']['build_id'] == new and 'previous published release' in res['baseline']['how']
    assert res['network']['checkpoint'] == 'run/206' and res['network']['same_graph'] and res['machines']['cpu']['ratio'] == 0.99
    plan = seen[0]
    assert plan['job']['conc'] == 64 and plan['job']['threads'] == 8 and plan['job']['rolling'] and plan['minutes'] == 0.5
    assert plan['priority'] == 'below_normal' and plan['idle']['min_s'] == 180 and plan['gen'] == 206
    assert plan['settings']['selfplay_remote'] == '' and plan['settings']['shard_rows'] == 8192
    assert plan['sides']['old']['runtime_dir'] == str(builds / ('r' * 24))
    hints = json.load(open(out / 'hints.json'))['hints']
    assert hints[0]['layout'] == {'threads': 8, 'infer_workers': 'default', 'infer_threads': 4, 'groups': 4, 'inflight': 64,
                                  'rolling': True, 'output': 'records', 'providers': ['cpu']}
    assert hints[0]['hardware']['cpu'] == 'test cpu' and hints[0]['leaves_per_s'] == {'new': 990.0, 'old': 1000.0}
    m = json.load(open(out / 'cpu' / 'machine.json'))
    assert len(m['windows']) == 4 and 'settings' not in m['plan'] and m['sides']['old']['commit'] == '1' * 40
    # slower: the gate fails loudly
    monkeypatch.setattr(F, 'run_windows', fake_runner({'new': 900.0, 'old': 1000.0}, seen))
    assert F.main(['gate', '--new', new_dir, '--out', str(out), '--retry-blocks', '0']) == 1
    res = json.load(open(out / 'result.json'))
    assert not res['passed'] and res['line'].startswith('SPEED GATE FAILED: cpu 0.900x SLOWER')
    # judge again with another tolerance
    assert F.main(['judge', str(out), '--tolerance', '0.15']) == 0


def test_two_runs_into_one_directory_combine_and_a_given_baseline_wins(builds, tmp_path, monkeypatch):
    old, old_dir = make_build(builds, '1')
    new, new_dir = make_build(builds, '2')
    cold, _ = make_build(builds, '3')
    cnew, cnew_dir = make_build(builds, '4')
    write_index(builds, [entry('1', old, cuda=cold)])
    monkeypatch.setattr(F, 'run_windows', fake_runner({'new': 1000.0, 'old': 1000.0}, []))
    out = tmp_path / 'speed'
    assert F.main(['gate', '--new', new_dir, '--baseline', old, '--out', str(out)]) == 0
    assert F.main(['gate', '--new', new_dir, '--baseline', old, '--machines', 'cuda', '--cuda-new', cnew_dir, '--out', str(out)]) == 0
    res = json.load(open(out / 'result.json'))
    assert set(res['machines']) == {'cpu', 'cuda'} and res['machines']['cuda']['old'] == cold and res['passed']
    m = json.load(open(out / 'cuda' / 'machine.json'))
    assert m['plan']['job']['providers'] == ['cuda'] and m['plan']['minutes'] == 1.0
    # a machine that cannot even start (no macOS build anywhere) fails the gate, recorded, beside the passing ones
    assert F.main(['gate', '--new', new_dir, '--baseline', old, '--machines', 'mac', '--out', str(out)]) == 1
    res = json.load(open(out / 'result.json'))
    assert not res['passed'] and res['machines']['cpu']['passed'] and not res['machines']['mac']['passed']
    assert 'no mac build' in res['machines']['mac']['why'] and 'mac ERROR' in res['line']
    assert F.main(['gate', '--new', old_dir, '--out', str(tmp_path / 'x')]) == 1
    res = json.load(open(tmp_path / 'x' / 'result.json'))
    assert 'no previous published' in res['machines']['cpu']['why']
    fz, fz_dir = make_build(builds, '5', profile='fuzz')
    assert F.main(['gate', '--new', new_dir, '--baseline', fz, '--out', str(tmp_path / 'y')]) == 1
    assert 'profile' in json.load(open(tmp_path / 'y' / 'result.json'))['machines']['cpu']['why']


def test_the_mac_plan_holds_the_quiet_lock_and_runs_the_agents_layout(builds, tmp_path, monkeypatch):
    old, _ = make_build(builds, '1')
    new, new_dir = make_build(builds, '2')
    plans = []

    def mac_runner(self, plan, n, o, out_m):
        plans.append(plan)
        fake = fake_runner({'new': 1000.0, 'old': 1000.0}, [])
        return (lambda order, first: fake(plan, out_m, order, first)), None
    monkeypatch.setattr(F.Gate, 'mac_runner', mac_runner)
    out = tmp_path / 'speed'
    assert F.main(['gate', '--machines', 'mac', '--mac-new', new_dir, '--mac-baseline', old, '--out', str(out)]) == 0
    plan = plans[0]
    assert plan['quiet'] == {'hold': '~/domz-remote/.eval.lock', 'avoid': ['~/mtgz-remote/.eval.lock'], 'settle_s': 60}
    assert plan['bench_lock'] == '~/mtgz-macfarm/bench.lock'
    assert {k: plan['job'][k] for k in ('threads', 'conc', 'groups')} == {'threads': 16, 'conc': 2304, 'groups': 12}
    assert plan['nice'] == 15 and 'infer_workers' not in plan['job'] and plan['sides']['old']['build_id'] == old
    assert json.load(open(out / 'result.json'))['machines']['mac']['passed']
    with pytest.raises(SystemExit, match='--mac-new'):
        F.Gate(types.SimpleNamespace(out=str(tmp_path / 'z'), new=None, baseline=None, mac_new=None, mac_baseline=None)).sides_for('mac')


def test_the_new_export_must_be_of_the_same_checkpoint(builds, tmp_path, monkeypatch):
    old, _ = make_build(builds, '1')
    new, new_dir = make_build(builds, '2')
    monkeypatch.setattr(F, 'run_windows', fake_runner({'new': 1000.0, 'old': 1000.0}, []))
    same = fake_onnx(tmp_path / 'same.onnx', graph=b'FUSED', checkpoint='run/206', checkpoint_iter=206)
    out = tmp_path / 'speed'
    assert F.main(['gate', '--new', new_dir, '--baseline', old, '--model-new', str(same), '--out', str(out)]) == 0
    res = json.load(open(out / 'result.json'))
    assert not res['network']['same_graph'] and 'graph differs' in res['line']
    other = fake_onnx(tmp_path / 'other.onnx', checkpoint='run/207', checkpoint_iter=207)
    with pytest.raises(SystemExit, match='not the same network'):
        F.main(['gate', '--new', new_dir, '--baseline', old, '--model-new', str(other), '--out', str(out)])


# ------------------------------------------------------------------------------------------------ farm_release.py's step

def release_for(tmp_path, builds, **args):
    import farm_release as FR
    a = types.SimpleNamespace(speed_model=None, speed_minutes=None, speed_tolerance=None, speed_blocks=None, speed_cuda=False,
                              no_speed_mac=False, speed_mac_baseline=None, no_slot=True)
    for k, v in args.items():
        setattr(a, k, v)
    s = {'python': sys.executable, 'reference': {'export': str(builds / 'reference' / 'export')},
         'speed': F.deep_merge(F.SPEED_DEFAULTS, {})}
    R = FR.Release.__new__(FR.Release)
    R.a, R.s, R.builds = a, s, str(builds)
    R.result = {'commit': '9' * 40, 'started': 'now', 'steps': {}, 'seconds': {}}
    R.log_dir = str(tmp_path / 'rel')
    os.makedirs(R.log_dir, exist_ok=True)
    return FR, R


def test_the_release_speed_step(builds, tmp_path, monkeypatch):
    FR, R = release_for(tmp_path, builds)
    own = os.path.join(R.log_dir, 'gates')
    os.makedirs(own)
    fake_onnx(os.path.join(own, 'model.onnx'), checkpoint='run/206', checkpoint_iter=206, exporter='this commit')
    cmds = []

    machines = {}

    def run(step, cmd, cwd=None, env=None, slot=False, log=None, timeout=None, check=True):
        cmds.append((cmd, slot, log))
        out = cmd[cmd.index('--out') + 1]
        for m in cmd[cmd.index('--machines') + 1].split(','):
            if m not in dead:
                machines[m] = {'passed': passed}
        with open(os.path.join(out, 'result.json'), 'w') as f:
            json.dump({'passed': passed, 'line': f'SPEED GATE {"PASSED" if passed else "FAILED"}: cpu ...', 'machines': machines,
                       'network': {'checkpoint': 'run/206', 'same_graph': True}}, f)
        return ''
    monkeypatch.setattr(FR.Release, 'run', lambda self, *a, **k: run(*a, **k))
    passed, dead = True, set()
    rec = R.speed('X:/dist/new', None, {'dir': 'X:/dist/mac', 'build_id': 'm' * 24}, None)
    assert rec['passed'] and rec['line'].startswith('SPEED GATE PASSED') and R.result['steps']['speed'] == rec
    (cpu, slot, _), (mac, mslot, mlog) = cmds
    assert slot and not mslot and mlog == 'speed-mac', 'the desktop part holds a slot, the Mac part none'
    assert cpu[cpu.index('--machines') + 1] == 'cpu' and cpu[cpu.index('--model-new') + 1] == os.path.join(own, 'model.onnx')
    assert mac[mac.index('--machines') + 1] == 'mac' and mac[mac.index('--mac-new') + 1] == 'X:/dist/mac'
    assert os.path.basename(cpu[1]) == 'farm_speed.py' and cpu[2] == 'gate'
    # a Mac run that died before writing its result: the desktop's pass is not the release's
    machines.clear()
    dead.add('mac')
    with pytest.raises(FR.Failed, match='not every machine'):
        R.speed('X:/dist/new', None, {'dir': 'X:/dist/mac', 'build_id': 'm' * 24}, None)
    dead.clear()
    passed = False
    cmds.clear()
    with pytest.raises(FR.Failed, match='SPEED GATE FAILED') as x:
        R.speed('X:/dist/new', None, None, None)
    assert x.value.step == 'speed' and len(cmds) == 1
    # a reused build with --regate: nothing to measure when the export's graph is the reference's
    cmds.clear()
    rec = R.speed('X:/builds/b', None, None, 'b' * 24, True)
    assert rec['skipped'] and cmds == []
    fake_onnx(os.path.join(own, 'model.onnx'), graph=b'NEW GRAPH', checkpoint='run/206', checkpoint_iter=206)
    passed = True
    rec = R.speed('X:/builds/b', None, None, 'b' * 24, True)
    assert rec['passed'] and cmds[0][0][cmds[0][0].index('--baseline') + 1] == 'b' * 24
    # the release notes
    R.result['failed'], R.result['error'] = 'speed', 'SPEED GATE FAILED: cpu 0.9x'
    FR.write_notes(R, {'build_id': 'n' * 24, 'gates': {'line': 'FARM GATES PASSED: ...'}, 'speed': rec})
    notes = open(os.path.join(R.log_dir, 'notes.txt'), encoding='utf-8').read().splitlines()
    assert notes[1] == 'build ' + 'n' * 24 and notes[2].startswith('FARM GATES PASSED') and notes[3].startswith('SPEED GATE')
    assert notes[-1].startswith('FAILED at speed')


def test_the_macos_parts_speed_windows_alone(builds, tmp_path, monkeypatch):
    """The macOS part's step speed-mac (docs/ENGINE.md 216): the Mac's windows alone, no slot, into speed-mac/ (the
    desktop's speed/ untouched); slower -> Failed('speed-mac')."""
    FR, R = release_for(tmp_path, builds)
    cmds = []

    def run(step, cmd, cwd=None, env=None, slot=False, log=None, timeout=None, check=True):
        cmds.append((step, cmd, slot, log))
        with open(os.path.join(cmd[cmd.index('--out') + 1], 'result.json'), 'w') as f:
            json.dump({'passed': passed, 'line': f'SPEED GATE {"PASSED" if passed else "FAILED"}: mac ...',
                       'machines': {'mac': {'passed': passed}}, 'network': {'checkpoint': 'run/206', 'same_graph': True}}, f)
        return ''
    monkeypatch.setattr(FR.Release, 'run', lambda self, *a, **k: run(*a, **k))
    os.makedirs(os.path.join(R.log_dir, 'speed'))
    with open(os.path.join(R.log_dir, 'speed', 'result.json'), 'w') as f:
        f.write('{"desktop": true}')
    passed = True
    rec = R.speed('X:/builds/n', None, {'dir': 'X:/dist/mac', 'build_id': 'm' * 24}, None, desktop=False)
    [(step, cmd, slot, log)] = cmds
    assert (step, slot, log) == ('speed-mac', False, 'speed-mac') and cmd[cmd.index('--machines') + 1] == 'mac'
    assert os.path.basename(cmd[cmd.index('--out') + 1]) == 'speed-mac' and cmd[cmd.index('--mac-new') + 1] == 'X:/dist/mac'
    assert rec['passed'] and R.result['steps']['speed-mac'] == rec and 'speed' not in R.result['steps']
    assert open(os.path.join(R.log_dir, 'speed', 'result.json')).read() == '{"desktop": true}'
    passed = False
    with pytest.raises(FR.Failed, match='SPEED GATE FAILED') as x:
        R.speed('X:/builds/n', None, {'dir': 'X:/dist/mac', 'build_id': 'm' * 24}, None, desktop=False)
    assert x.value.step == 'speed-mac'


def test_farm_gates_hands_speed_over(monkeypatch):
    import farm_gates
    got = []
    monkeypatch.setattr(F, 'main', lambda argv: got.append(argv) or 0)
    assert farm_gates.main(['--speed', '--new', 'x', '--machines', 'cpu']) == 0
    assert got == [['gate', '--new', 'x', '--machines', 'cpu']]


def test_the_runner_runs_alone(tmp_path):
    """`farm_speed.py windows` imports nothing of the repo (it runs on the Mac from a copy of the file)."""
    d = tmp_path / 'alone'
    d.mkdir()
    shutil.copy(os.path.join(ROOT, 'tools', 'farm_speed.py'), d)
    code = ('import sys; sys.path.insert(0, "."); import farm_speed; '
            'print(sorted(m for m in sys.modules if m.startswith(("mtgz", "zero", "local_config"))))')
    p = subprocess.run([sys.executable, '-I', '-c', code], cwd=d, capture_output=True, text=True)
    assert p.returncode == 0 and p.stdout.strip() == '[]', p.stderr
