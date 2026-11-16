"""A self-play game that panics costs that game, not the run (docs/ENGINE.md 172; zero-core docs/search.md 2.18,
2.19), on real MTG games with the test fault MTGZ_TEST_PANIC=<game seed>:<turn> (the engine panics in that game only;
unset in every real run):

* the in-process driver (the run's own self-play and the Mac's remote worker drive this same binding): exactly the
  armed game is dropped, every other game's row and record are an unarmed run's byte for byte, one failure with its
  site in the engine; the run's failure_metrics writes it to <run>/game_failures.jsonl with `selfplay.panics` 1, and
  the line's `repro` command - run as written, from this checkout, without the fault in its environment - reproduces
  the panic (exit 101, the panic's text);
* the native farm worker (`mtgz-worker serve`, a self-play job writing records): the dropped game is in no records file,
  its failure rides in the next file's header (`failures`), the desktop's pool collector reads it from there
  (zero.farm_pool.header_failures), and every other game's record is the unarmed job's. Skipped without a built
  worker (`cargo build --release -p mtgz_worker`; env MTGZ_WORKER)."""
import gzip
import json
import os
import subprocess
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tests'))

import mtgz.az  # noqa: E402,F401
from mtgz import _engine as E  # noqa: E402
from zero import game_failure as GF, games as G, run as R  # noqa: E402
from zero.infer import drive  # noqa: E402

from test_farm_play import Uniform  # noqa: E402
from test_worker_farm import WORKER, Serve  # noqa: E402

SEED = 172
GAMES = 6
DROP = 3
TURN = 3


def _spec():
    return G.spec_json(mtgz.az.PRESETS['delver-smoke']['spec'])


def _play(env):
    """GAMES Delver games of a traced driver (uniform network, 2 simulations), the fault as `env` says ->
    ({gid: (row, record text or None)}, failures, stats)."""
    old = os.environ.pop('MTGZ_TEST_PANIC', None)
    if env:
        os.environ['MTGZ_TEST_PANIC'] = env
    try:
        seat = G.nn_seat('m', 2)
        drv = G.Driver('mtg', [[seat, seat]], spec=_spec(), concurrency=GAMES, seed=SEED, threads=2, groups=2,
                       rolling=True)
        drv.set_trace(True)
        out = {}
        while len(out) < GAMES:
            drv.resume(GAMES - len(out), 0)
            drive(drv, {'m': Uniform()})
            rows, texts, _ = drv.take_game_records()
            for r, t in zip(rows, texts):
                out[r["gid"]] = (r, t or None)
            for r in drv.take_results():               # (a dropped game: an aborted row without a record)
                out[r['gid']] = (r, None)
        return out, [json.loads(x) for x in drv.take_failures()], drv.stats()
    finally:
        os.environ.pop('MTGZ_TEST_PANIC', None)
        if old is not None:
            os.environ['MTGZ_TEST_PANIC'] = old


def test_the_in_process_driver_drops_one_game_and_its_repro_replays_it(tmp_path):
    target = E.game_seed_of(SEED, DROP)
    plain, f0, _ = _play(None)
    assert not f0 and all(r.get('abort_reason') is None for r, _ in plain.values())
    armed, fails, st = _play(f'{target}:{TURN}')
    assert st.get('game_panics') == 1
    assert sorted(armed) == sorted(plain) == list(range(GAMES))
    row, rec = armed[DROP]
    assert row['abort_reason'] == 'panic' and row['aborted'] and not row['over'] and rec is None
    for g in range(GAMES):
        if g != DROP:
            assert armed[g][1] == plain[g][1], f'game {g}: its record as the unarmed run\'s'
    (f,) = fails
    assert (f['gid'], f['game_seed'], f['players']) == (DROP, target, 2)
    assert 'matches.rs:' in (f['site'] or '') and f['phase'] in ('move', 'search')
    # the run's side: the failure into game_failures.jsonl with a repro command, selfplay.panics
    r = object.__new__(R.Run)
    r.run, r.cfg, r.lines = str(tmp_path), {'game': 'mtg'}, []
    r.log = r.lines.append

    class Drv:
        def take_failures(self):
            return [json.dumps(f)]
    out = {}
    r.failure_metrics(7, out, [row] + [plain[g][0] for g in range(GAMES) if g != DROP], Drv())
    assert out['panics'] == 1 and out['game_failures'] == 1
    (line,) = GF.read(os.path.join(tmp_path, R.GAME_FAILURES))
    assert (line['iter'], line['source'], line['game'], line['game_id'], line['game_seed']) == (7, 'local', 'mtg', DROP, target)
    assert line['site'] == f['site'] and line['build'] and line['repro'].startswith('python -m mtgz.az.game_failure replay ')
    assert line['engine_commit'] and len(line['engine_commit']) == 40
    env = {k: v for k, v in os.environ.items() if k != 'MTGZ_TEST_PANIC'}
    p = subprocess.run(line['repro'].replace('python ', f'"{sys.executable}" ', 1), shell=True, cwd=ROOT, env=env,
                       capture_output=True, text=True, timeout=600)
    assert p.returncode == GF.REPRODUCED, (p.returncode, p.stdout[-2000:], p.stderr[-2000:])
    assert 'REPRODUCED' in p.stdout and 'injected test panic' in p.stdout + p.stderr
    # another game's failure that never panicked: replayed, not reproduced (exit 0)
    calm = dict(line, game_seed=E.game_seed_of(SEED, 0), moves=[], phase='search', panic_move=None, message='x')
    ok, text = GF.replay(calm, rollouts=2)
    assert not ok and json.loads(text)['rollouts'] == 2


def _served(tmp_path, name, env):
    from test_worker_records import _settings
    p, cfg = _settings(tmp_path, 'delver-smoke')
    d = tmp_path / name
    d.mkdir()
    w = Serve(WORKER, d, env=env)
    w.wait_for('hello', 60)
    w.send({'op': 'job', 'lease_id': f'l_{name}', 'kind': 'selfplay', 'settings': cfg, 'games': GAMES, 'gid_base': 0,
            'seed': SEED, 'uniform': True, 'threads': 2, 'out_dir': 'out'})
    end = w.wait_for('end', 1200)
    assert w.finish() == 0 and end['reason'] == 'done', (end, ''.join(w.err)[-2000:])
    heads, lines = [], {}
    for e in [e for e in w.events if e['ev'] == 'records']:
        text = gzip.decompress((d / 'out' / e['path']).read_bytes()).decode().splitlines()
        heads.append(json.loads(text[0]))
        for ln in text[1:]:
            lines[json.loads(ln)['gid']] = ln
    return heads, lines, ''.join(w.err)


@pytest.mark.skipif(WORKER is None, reason='mtgz-worker is not built (cargo build --release -p mtgz_worker)')
def test_the_native_worker_carries_the_failure_in_its_records(tmp_path):
    from zero import farm_pool
    target = E.game_seed_of(SEED, DROP)
    env0 = {k: v for k, v in os.environ.items() if k != 'MTGZ_TEST_PANIC'}
    h0, plain, _ = _served(tmp_path, 'plain', None)
    assert not any('failures' in h for h in h0) and sorted(plain) == list(range(GAMES))
    os.environ.pop('MTGZ_TEST_PANIC', None)
    h1, armed, err = _served(tmp_path, 'armed', {'MTGZ_TEST_PANIC': f'{target}:{TURN}'})
    os.environ.clear()
    os.environ.update(env0)
    assert DROP not in armed and sorted(armed) == [g for g in range(GAMES) if g != DROP]
    for g in armed:
        assert armed[g] == plain[g], f'game {g}: its record as the unarmed job\'s'
    fails = [f for h in h1 for f in farm_pool.header_failures(h)]
    assert len(fails) == 1 and fails[0]['gid'] == DROP and fails[0]['game_seed'] == target
    assert fails[0]['build'] == h1[0]['build_id'] and 'matches.rs:' in (fails[0]['site'] or '')
    assert 'game failure (dropped after a panic)' in err
    # the record the pool collector writes replays the panic
    rec = GF.record(fails[0], it=1, source='pool', game='mtg', engine_commit='x', build=None, path='f', line=1, pkg='mtgz')
    ok, text = GF.replay(rec, rollouts=16)
    assert ok and 'injected test panic' in text
    os.environ.pop('MTGZ_TEST_PANIC', None)
