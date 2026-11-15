"""The run's standing yardsticks (docs/ENGINE.md 164): the run setting `yardsticks` (on in the delver-mirror* presets,
off in the smokes; --yardsticks / --no-yardsticks override it and are saved), the best-of-one spec and opponents, the
catch-up order (every evaluated checkpoint once, oldest first, in two lanes), the retry and missing-result paths, the
results file's rows, the watcher end to end with fake measurements, the one-per-run lock, the companion keeper (both
companions in one; a child restarted 3 times then given up; it ends with the run) and the publisher's record. No
supervisor is started here (docs/ENGINE.md 164.6: the live run's processes)."""
import json
import os
import subprocess
import sys
import time

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az as A  # noqa: E402
from mtgz.az import companions as C  # noqa: E402
from mtgz.az import publish as PUB  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402

LIB_FORMAT = 'delver-canary-library/1'


def _cfg(argv, saved=None):
    from mtgz.az import run as R           # (zero.run)
    return R.resolve_config(R.parse(['--run', 'x'] + Y.no_yardsticks(PUB.no_publish(argv))), dict(saved or {}))


# ---------------------------------------------------------------- the setting

def test_the_delver_presets_measure_yardsticks_and_the_smokes_do_not():
    for name in ('delver-mirror', 'delver-mirror-bo3', 'delver-mirror-bo3-dealt'):
        assert A.PRESETS[name]['yardsticks'] == Y.DELVER_YARDSTICKS
    for name in ('delver-smoke', 'delver-bo3-smoke', 'mtg-smoke'):
        assert not A.PRESETS[name].get('yardsticks')
    assert A.PACKAGE.settings['yardsticks'] == {}
    d = Y.normalize(Y.DELVER_YARDSTICKS)
    assert (d['bo1_games'], d['bo1_sims'], d['bo1_opponents'], d['bo1_device'], d['blunders'], d['blunders_sims']) == \
        (400, 64, ['anchors', 'iter0'], 'cuda', True, 1024)


def test_flags_override_the_preset_and_a_run_without_the_setting_takes_its_presets():
    assert _cfg(['--preset', 'delver-mirror-bo3-dealt'])['yardsticks'] == Y.DELVER_YARDSTICKS
    # pb2 (started before the setting existed): its preset's at its next start
    assert _cfg([], {'preset': 'delver-mirror-bo3-dealt', 'iters': 1000})['yardsticks'] == Y.DELVER_YARDSTICKS
    assert _cfg(['--preset', 'delver-mirror-bo3', '--yardsticks', '{"bo1_games": 200}'])['yardsticks'] == \
        {'bo1_games': 200}
    off = _cfg(['--preset', 'delver-mirror-bo3', '--no-yardsticks'])
    assert off['yardsticks'] == {}
    assert _cfg([], {'preset': 'delver-mirror-bo3', 'yardsticks': {}})['yardsticks'] == {}
    assert _cfg(['--preset', 'delver-bo3-smoke'])['yardsticks'] == {}
    assert Y.of_run(None, {'preset': 'delver-mirror-bo3-dealt'})['bo1_games'] == 400        # (not saved: preset's)
    assert Y.of_run(None, {'preset': 'delver-mirror-bo3-dealt', 'yardsticks': {}}) is None


def test_a_bad_yardsticks_setting_is_refused():
    for bad, why in (({'bo1_game': 4}, 'unknown option'), ({'bo1_games': 3}, 'even'), ({'bo1_games': '4'}, 'type'),
                     ({'blunders': 1}, 'type'), ({'bo1_sims': True}, 'type'), ({'bo1_device': 'mps'}, 'cuda'),
                     ({'bo1_opponents': ['x']}, 'LABEL=PATH'), ({'bo1_games': 0, 'blunders': False}, 'nothing'),
                     ([1], 'JSON object'), ({'retry_s': 0}, '> 0')):
        with pytest.raises(ValueError, match=why):
            Y.normalize(bad)
    assert Y.normalize(None) is None and Y.normalize({}) is None


def test_the_best_of_one_spec_is_the_runs_game_1():
    ys = Y.normalize({'blunders': True})
    for name in ('delver-mirror-bo3', 'delver-mirror-bo3-dealt', 'delver-mirror'):
        assert json.loads(Y.bo1_spec(A.PRESETS[name], ys)) == json.loads(A.SPEC_DELVER), name
    assert Y.bo1_spec({}, dict(ys, bo1_spec='{"x": 1}')) == '{"x": 1}'
    with pytest.raises(ValueError, match='bo1_spec'):
        Y.bo1_spec({}, ys)


def test_the_opponents_are_the_anchors_and_generation_0(tmp_path):
    cfg = {'elo_anchors': 'a3198=C:/anchors/a.pt,E:/x/iter_0192.pt'}
    ys = Y.normalize({'bo1_games': 4, 'bo1_opponents': ['anchors', 'iter0', 'late=E:/y.pt']})
    o = Y.opponents(str(tmp_path), cfg, ys)
    assert [(x['name'], x['label']) for x in o] == [('a3198', '3198'), ('iter_0192', 'iter_0192'),
                                                    ('gen0', 'generation 0'), ('late', 'late')]
    assert o[0]['path'] == 'C:/anchors/a.pt' and o[2]['path'] == os.path.join(str(tmp_path), 'models', 'iter_0000.pt')
    with pytest.raises(ValueError, match='two'):
        Y.opponents(str(tmp_path), cfg, Y.normalize({'bo1_opponents': ['iter0', 'gen0=E:/z.pt']}))


def test_the_seat_is_the_runs_evaluation_rule_without_the_deck_tilt(tmp_path):
    run = tmp_path / 'r'
    (run / 'models').mkdir(parents=True)
    (run / 'config.json').write_text(json.dumps({
        'eval_select': 'cq', 'eval_select_min_visits': 4, 'eval_select_c_scale': 0.3, 'eval_leaf_value_view': 'root',
        'eval_belief_tilt': 1.0, 'belief_clip': 20.0, 'belief_mix': 0.25, 'eval_belief_tilts': {'deck': 1.0}}))
    seat = Y.bo1_seat(str(run / 'models' / 'iter_0016.pt'))
    assert seat['select'] == 'cq' and seat['select_min_visits'] == 4 and seat['cq_c_scale'] == 0.3
    assert seat['leaf_value_view'] == 'root' and seat['belief_tilt'] == 1.0 and seat['belief_mix'] == 0.25
    assert 'belief_tilts' not in seat


# ---------------------------------------------------------------- a fake run

def _library(path, ids=('s1', 's2')):
    path.write_text(json.dumps({'format': LIB_FORMAT, 'spots': [
        {'id': i, 'answer': 'bad', 'watch': 'x', 'kind': 'k', 'review': {'verdict': 'keep', 'by': 'user'}}
        for i in ids]}))
    return str(path)


def _run(tmp_path, iters=range(0, 50), every=16, old=True, ys=None):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    anchor = tmp_path / 'a3198.pt'
    anchor.write_bytes(b'anchor')
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'eval_every': every, 'elo_anchors': f'a3198={anchor}'}
    if ys is not None:
        cfg['yardsticks'] = ys
    (run / 'config.json').write_text(json.dumps(cfg))
    for i in iters:
        p = run / 'models' / f'iter_{i:04d}.pt'
        p.write_bytes(b'x')
        if old:
            os.utime(p, (time.time() - 3600, time.time() - 3600))
    return str(run), cfg


def _ys(tmp_path, **kw):
    return Y.normalize(dict({'bo1_games': 4, 'blunders': True, 'library': _library(tmp_path / 'lib.json'),
                             'retry_s': 60, 'retry_max_s': 600}, **kw))


def test_catch_up_is_every_evaluated_checkpoint_in_order_in_two_lanes(tmp_path):
    run, cfg = _run(tmp_path)
    ys = _ys(tmp_path)
    assert Y.evaluated(run, cfg, ys) == [0, 16, 32, 48]
    w = Y.wanted(run, cfg, ys, lib='L')
    assert [(x['kind'], x['iter'], x.get('opp')) for x in w if x['lane'] == 'gpu'] == [
        ('bo1', 0, 'a3198'), ('bo1', 16, 'a3198'), ('bo1', 16, 'gen0'), ('bo1', 32, 'a3198'), ('bo1', 32, 'gen0'),
        ('bo1', 48, 'a3198'), ('bo1', 48, 'gen0')]                  # (gen 0 against itself: nothing to measure)
    assert [x['iter'] for x in w if x['lane'] == 'cpu'] == [0, 16, 32, 48]
    rows = []
    for _ in range(7):
        item = Y.pending(run, cfg, ys, 'gpu', rows=rows, lib='L')
        rows.append({'kind': 'bo1', 'iter': item['iter'], 'opp': item['opp'], 'score': 0.5, 'se': 0.0})
    assert [(r['iter'], r['opp']) for r in rows] == [(x['iter'], x['opp']) for x in w if x['lane'] == 'gpu']
    assert Y.pending(run, cfg, ys, 'gpu', rows=rows, lib='L') is None
    # an anchor that is a copy of generation 0 (pb1's a3198): not measured against generation 0
    with open(os.path.join(run, 'models', 'iter_0000.pt'), 'wb') as f:
        f.write(b'anchor')
    os.utime(os.path.join(run, 'models', 'iter_0000.pt'), (time.time() - 3600, time.time() - 3600))
    assert [x['iter'] for x in Y.wanted(run, cfg, ys, lib='L') if x['kind'] == 'bo1'][:2] == [16, 16]
    # `from`, and a checkpoint still being written (settle) waits
    assert Y.evaluated(run, cfg, dict(ys, **{'from': 20})) == [32, 48]
    os.utime(os.path.join(run, 'models', 'iter_0048.pt'))
    assert Y.evaluated(run, cfg, ys) == [0, 16, 32] and Y.evaluated(run, cfg, ys, settle=0)[-1] == 48


def test_a_failure_is_retried_after_its_wait_and_the_lane_goes_on(tmp_path):
    run, cfg = _run(tmp_path, iters=(0, 16))
    ys = _ys(tmp_path, bo1_games=0)
    now = time.time()
    fail = {'kind': 'blunders', 'iter': 0, 'library': 'L', 'error': 'boom', 'attempt': 1, 'retry_at': now + 60}
    rows = [fail]
    item = Y.pending(run, cfg, ys, 'cpu', rows=rows, lib='L', now=now)
    assert (item['iter'], item['attempt']) == (16, 1)               # 0 waits; the lane goes on with 16
    st = Y.status(run, cfg, ys, lib='L', rows=rows, now=now)
    assert st['failed'] == [{'kind': 'blunders', 'iter': 0, 'attempts': 1, 'error': 'boom', 'retry_at': now + 60}]
    assert st['missing'] == [{'kind': 'blunders', 'iter': 16}]
    item = Y.pending(run, cfg, ys, 'cpu', rows=rows, lib='L', now=now + 61)
    assert (item['iter'], item['attempt']) == (0, 2)                # its wait over: 0 again, oldest first
    rows.append(dict(fail, attempt=2, retry_at=now + 200))
    assert Y.status(run, cfg, ys, lib='L', rows=rows, now=now)['failed'][0]['attempts'] == 2
    rows.append({'kind': 'blunders', 'iter': 0, 'library': 'L', 'passed': 1, 'total': 2})
    st = Y.status(run, cfg, ys, lib='L', rows=rows, now=now)
    assert not st['failed'] and [r['iter'] for r in st['blunders']] == [0]
    assert Y.backoff(ys, 1) == 60 and Y.backoff(ys, 2) == 120 and Y.backoff(ys, 9) == 600


def test_a_changed_library_makes_every_checkpoint_pending_again(tmp_path):
    run, cfg = _run(tmp_path, iters=(0, 16))
    ys = _ys(tmp_path, bo1_games=0)
    rows = [{'kind': 'blunders', 'iter': i, 'library': 'L1', 'passed': 1, 'total': 2} for i in (0, 16)]
    assert Y.pending(run, cfg, ys, 'cpu', rows=rows, lib='L1') is None
    assert Y.pending(run, cfg, ys, 'cpu', rows=rows, lib='L2')['iter'] == 0
    assert [m['iter'] for m in Y.status(run, cfg, ys, lib='L2', rows=rows)['missing']] == [0, 16]
    # no library at all: still wanted (and failing loudly), never silently nothing
    assert Y.pending(run, cfg, ys, 'cpu', rows=[], lib=None)['library'] is None
    lib = Y.library_id(ys)
    assert isinstance(lib, str) and len(lib) == 12
    _library(tmp_path / 'lib.json', ids=('s1', 's2', 's3'))
    assert Y.library_id(ys) != lib


def test_the_results_file_skips_a_torn_line(tmp_path):
    run, cfg = _run(tmp_path, iters=(0,))
    Y.append_row(run, {'kind': 'bo1', 'iter': 0, 'opp': 'a3198', 'score': 0.75, 'se': 0.02})
    with open(Y.rows_path(run), 'a', encoding='utf-8') as f:
        f.write('{"kind": "bo1", "iter": 16, "op')
    assert Y.read_rows(run) == [{'kind': 'bo1', 'iter': 0, 'opp': 'a3198', 'score': 0.75, 'se': 0.02}]


# ---------------------------------------------------------------- the watcher with fake measurements

FAKE_ONE = '''import json, os, sys
kind, it, opp, flag = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
mark = flag + f".{kind}.{it}.{opp}"
if os.path.exists(flag) and not os.path.exists(mark):
    open(mark, "w").write("x")
    print("Traceback: CUDA out of memory", flush=True)
    sys.exit(3)
if kind == "bo1":
    row = {"kind": "bo1", "iter": it, "opp": opp, "score": 0.5 + it / 1000, "se": 0.02, "elo_diff": 10.0,
           "elo_se": 15.0, "games": 4, "pentanomial": [0, 0, 2, 0, 0], "seconds": 1.0, "time": 1}
else:
    row = {"kind": "blunders", "iter": it, "passed": 1, "total": 2, "share": 0.5, "kinds": {"k": [1, 2]},
           "library": os.environ["LIB"], "seconds": 1.0, "time": 1}
print("some log line")
print(json.dumps(row), flush=True)
'''


def _watcher(run, tmp_path, flag, logs):
    import run_yardsticks as RY
    fake = tmp_path / 'fake_one.py'
    fake.write_text(FAKE_ONE)
    return RY.Watcher(run, once=True, log=logs.append, poll=0.05, child_cmd=lambda it: [
        sys.executable, str(fake), it['kind'], str(it['iter']), it.get('opp') or '-', str(flag)])


@pytest.mark.live_run_hazard
def test_the_watcher_measures_everything_in_order_and_retries_a_failure(tmp_path, monkeypatch):
    run, cfg = _run(tmp_path, iters=range(0, 33),
                    ys={'bo1_games': 4, 'blunders': True, 'retry_s': 3600, 'retry_max_s': 7200})
    lib = _library(tmp_path / 'lib.json')
    cfg['yardsticks']['library'] = lib
    with open(os.path.join(run, 'config.json'), 'w', encoding='utf-8') as f:
        json.dump(cfg, f)
    ys = Y.of_run(run)
    monkeypatch.setenv('LIB', Y.library_id(ys))
    flag = tmp_path / 'fail_first'
    flag.write_text('x')                    # every measurement fails once
    logs = []
    assert _watcher(run, tmp_path, flag, logs).main() == 0
    rows = Y.read_rows(run)
    assert all(r.get('error') for r in rows) and len(rows) == 8      # 5 bo1 + 3 blunders, each failed once
    assert sum('YARDSTICK FAILED' in m for m in logs) == 8
    assert 'CUDA out of memory' in rows[0]['error'] and rows[0]['attempt'] == 1
    st = Y.status(run)
    assert len(st['failed']) == 8 and not st['missing'] and not st['bo1']
    assert all(r['retry_at'] > time.time() + 3000 for r in rows)
    with open(Y.rows_path(run), 'w', encoding='utf-8') as f:     # an hour later: their waits are over
        for r in rows:
            f.write(json.dumps(dict(r, retry_at=r['retry_at'] - 3601)) + chr(10))
    logs.clear()
    assert _watcher(run, tmp_path, flag, logs).main() == 0
    st = Y.status(run)
    assert not st['failed'] and not st['missing']
    assert [(r['iter'], r['opp']) for r in st['bo1']] == [(0, 'a3198'), (16, 'a3198'), (16, 'gen0'), (32, 'a3198'),
                                                         (32, 'gen0')]
    assert [r['iter'] for r in st['blunders']] == [0, 16, 32]
    ok = [r for r in Y.read_rows(run) if r.get('error') is None]
    assert [(r['iter'], r.get('opp')) for r in ok if r['kind'] == 'bo1'] == [(r['iter'], r['opp']) for r in st['bo1']]
    # nothing due: a third watcher measures nothing
    n = len(Y.read_rows(run))
    assert _watcher(run, tmp_path, flag, []).main() == 0 and len(Y.read_rows(run)) == n


def test_one_watcher_per_run(tmp_path):
    import run_yardsticks as RY
    run, _ = _run(tmp_path, iters=(0,))
    os.makedirs(os.path.join(run, Y.DIR))
    other = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])
    try:
        with open(RY.lock_path(run), 'w') as f:
            json.dump({'pid': other.pid, 'started': RY._started(other.pid)}, f)
        assert RY.lock_owner(run) == other.pid and not RY.take_lock(run)
        logs = []
        assert RY.Watcher(run, once=True, log=logs.append).main() == 2 and 'refused' in logs[0]
    finally:
        other.kill()
        other.wait()
    with open(RY.lock_path(run), 'w') as f:                       # its owner gone (a reused pid would not match)
        json.dump({'pid': other.pid, 'started': 1.0}, f)
    assert RY.lock_owner(run) is None and RY.take_lock(run)
    RY.drop_lock(run)
    assert not os.path.exists(RY.lock_path(run))


# ---------------------------------------------------------------- the companions

def test_the_companion_is_the_publisher_the_yardsticks_or_both(tmp_path, monkeypatch):
    monkeypatch.setenv(PUB.KEY_ENV, 'k')
    pub = {'publish': {'title': 'x'}}
    assert A.PACKAGE.companion_of('E:/r', pub)['name'] == 'publisher'          # (ENGINE.md 163's command, unchanged)
    yd = A.PACKAGE.companion_of('E:/r', {'yardsticks': Y.DELVER_YARDSTICKS})
    assert yd['name'] == 'yardsticks' and yd['log'] == 'yardsticks_console.txt'
    assert yd['cmd'][1:] == [os.path.join(ROOT, 'tools', 'run_yardsticks.py'), 'watch', 'E:/r', '--supervised']
    both = A.PACKAGE.companion_of('E:/r', dict(pub, yardsticks=Y.DELVER_YARDSTICKS))
    assert both['name'] == 'companions' and both['cmd'][1:3] == ['-m', 'mtgz.az.companions']
    kids = [both['cmd'][i + 1:i + 4] for i, x in enumerate(both['cmd']) if x == '--child']
    assert [(k[0], k[1]) for k in kids] == [('publisher', 'publisher_console.txt'), ('yardsticks', 'yardsticks_console.txt')]
    assert json.loads(kids[1][2]) == yd['cmd']
    assert A.PACKAGE.companion_of('E:/r', {'publish': {}, 'yardsticks': {}}) is None
    off = _cfg(['--preset', 'delver-mirror-bo3', '--no-publish', '--no-yardsticks'])
    assert A.PACKAGE.companion_of('/r', off) is None
    # the publisher refusing (no key): the yardsticks alone; both refusing: the supervisor's loud refusal
    monkeypatch.delenv(PUB.KEY_ENV)
    monkeypatch.setattr(PUB, 'KEY_FILE', str(tmp_path / 'none'))
    assert A.PACKAGE.companion_of('E:/r', dict(pub, yardsticks=Y.DELVER_YARDSTICKS))['name'] == 'yardsticks'
    with pytest.raises(ValueError, match='no upload key'):
        A.PACKAGE.companion_of('E:/r', dict(pub, yardsticks={'bo1_games': 3}))


@pytest.mark.live_run_hazard
def test_the_keeper_restarts_a_child_3_times_then_gives_up_and_ends_with_the_run(tmp_path, capsys):
    run = tmp_path / 'run'
    run.mkdir()
    stop = tmp_path / 'ended'
    good = [sys.executable, '-c', f'import os, time\nwhile not os.path.exists({str(stop)!r}): time.sleep(0.05)']
    bad = [sys.executable, '-c', 'import sys; print("bad child"); sys.exit(3)']
    clock = [1000.0]

    def end():
        clock[0] += 1
        if clock[0] > 1040:                 # the run ends after the bad child was given up
            stop.write_text('x')
            return 'finished'
        return None
    code = C.keep([('good', 'good.txt', good), ('bad', 'bad.txt', bad)], str(run), poll=0.1, end=end,
                  clock=lambda: clock[0])
    out = capsys.readouterr().out
    assert code == 1
    assert out.count('bad: starting it again') == 3 and 'CHILD GIVEN UP (bad)' in out
    assert 'good: starting it again' not in out and 'the run ended (finished)' in out
    assert 'good exited with code 0' in out and 'no child left: exiting (1)' in out
    assert (run / 'bad.txt').read_text().count('bad child') == 4


@pytest.mark.live_run_hazard
def test_the_keeper_starts_no_child_again_once_the_run_ended(tmp_path, capsys):
    run = tmp_path / 'run'
    run.mkdir()
    quick = [sys.executable, '-c', 'pass']
    code = C.keep([('a', 'a.txt', quick), ('b', 'b.txt', quick)], str(run), poll=0.1, end=lambda: 'stopped')
    out = capsys.readouterr().out
    assert code == 0 and 'starting it again' not in out and out.count('exited with code 0') == 2


# ---------------------------------------------------------------- the publisher's record

def test_the_publisher_sends_the_yardsticks_without_paths_or_errors(tmp_path, monkeypatch):
    import delverai_publish as P
    run, cfg = _run(tmp_path, iters=(0, 16, 32), ys={'bo1_games': 4, 'blunders': True})
    lib = _library(tmp_path / 'lib.json')
    cfg['yardsticks']['library'] = lib
    with open(os.path.join(run, 'config.json'), 'w', encoding='utf-8') as f:
        json.dump(cfg, f)
    L = Y.library_id(Y.of_run(run))
    Y.append_row(run, {'kind': 'bo1', 'iter': 0, 'opp': 'a3198', 'opp_label': '3198', 'opp_sha': 'abc', 'score': 0.75,
                       'se': 0.0184, 'elo_diff': 188.5, 'elo_se': 16.9, 'games': 400, 'pentanomial': [2, 0, 97, 0, 101],
                       'sims': 64, 'select': 'cq', 'seconds': 464.3, 'time': 5})
    Y.append_row(run, {'kind': 'bo1', 'iter': 16, 'opp': 'a3198', 'error': 'C:/Users/x/file.pt: boom', 'attempt': 1,
                       'retry_at': time.time() + 300, 'time': 6})
    Y.append_row(run, {'kind': 'blunders', 'iter': 0, 'library': L, 'passed': 27, 'total': 39, 'share': 0.7,
                       'kinds': {'k': [27, 39]}, 'seconds': 270.0, 'time': 7})
    rec = P.yardsticks_record(run)
    assert rec['opponents'] == [{'name': 'a3198', 'label': '3198'}, {'name': 'gen0', 'label': 'generation 0'}]
    assert rec['bo1'] == [{'iter': 0, 'opp': 'a3198', 'score': 0.75, 'se': 0.0184, 'elo_diff': 188.5, 'elo_se': 16.9,
                           'games': 400, 'pentanomial': [2, 0, 97, 0, 101], 'sims': 64, 'time': 5}]
    assert rec['blunders'] == [{'iter': 0, 'passed': 27, 'total': 39, 'share': 0.7, 'kinds': {'k': [27, 39]}, 'time': 7}]
    assert rec['expected'] == [0, 16, 32]
    assert rec['failed'] == [{'kind': 'bo1', 'iter': 16, 'opp': 'a3198', 'attempts': 1}]
    assert {(m['kind'], m['iter'], m.get('opp')) for m in rec['missing']} == {
        ('bo1', 16, 'gen0'), ('bo1', 32, 'a3198'), ('bo1', 32, 'gen0'), ('blunders', 16, None), ('blunders', 32, None)}
    assert 'C:/' not in json.dumps(rec) and 'boom' not in json.dumps(rec)
    assert P.progress(run)['yardsticks'] == rec
    # off: no field
    cfg['yardsticks'] = {}
    with open(os.path.join(run, 'config.json'), 'w', encoding='utf-8') as f:
        json.dump(cfg, f)
    assert P.yardsticks_record(run) is None and 'yardsticks' not in P.progress(run)


def test_the_publisher_posts_the_blunder_record_when_the_cache_changed(tmp_path, monkeypatch):
    import delverai_blunders as DB
    import delverai_publish as P
    run, cfg = _run(tmp_path, iters=(0,), ys={'bo1_games': 0, 'blunders': True})
    cfg['yardsticks']['library'] = _library(tmp_path / 'lib.json')
    with open(os.path.join(run, 'config.json'), 'w', encoding='utf-8') as f:
        json.dump(cfg, f)
    posted = []
    monkeypatch.setattr(DB, 'upload', lambda rec, url=None, key=None: posted.append(rec))
    a = type('A', (), {'dry_run': False, 'url': 'http://x'})()
    state = {}
    assert not P.post_blunders(run, a, 'k', state)                  # no cache yet
    c = DB.load_cache(run, DB.setup_key(1024, DB.SELECT))
    c['scores']['0'] = {'s1': 1.0, 's2': 0.25}
    DB.save_cache(run, c)
    assert P.post_blunders(run, a, 'k', state) and not P.post_blunders(run, a, 'k', state)     # once per change
    rec = posted[0]
    assert rec['run'] == 'blunders.run' and rec['iters'] == [
        {'iter': 0, 'passed': 1, 'total': 2, 'share': 0.625, 'kinds': {'k': [1, 2]}}]
