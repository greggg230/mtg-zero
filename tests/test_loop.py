"""The commands live runs and the hub rely on, through MTG Zero's shims of zero-core
(`python -m mtgz.az.run`, tools/supervise.py, kill_run, pause_run, fork_run, the dashboard,
`python -m mtgz.az.h2h` / `.targets`), at toy-smoke scale: iterations, resume, STOP, PAUSE (exit 75
+ resume), the supervisor with a fake run (MTGZ_SUPERVISE_*), kill_run, fork_run, the dashboard
on a real run dir. The loop itself is tested in zero-core (tests/test_loop.py)."""
import json
import os
import subprocess
import sys
import time

import pytest

from mtgz.az import runctl
from mtgz.az.run import latest_iter, read_metrics

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SMOKE = ['--preset', 'toy-smoke', '--conc', '24', '--games', '24', '--eval-games', '8', '--eval-random-games', '4',
         '--elo-games', '4', '--ref-games', '8']


def run_loop(run, *extra, env=None, timeout=300):
    e = dict(os.environ, PYTHONIOENCODING='utf-8', **(env or {}))
    return subprocess.run([sys.executable, '-m', 'mtgz.az.run', '--run', str(run), *SMOKE, *extra], cwd=ROOT,
                          capture_output=True, text=True, timeout=timeout, env=e)


@pytest.fixture(scope='module')
def smoke_run(tmp_path_factory):
    run = tmp_path_factory.mktemp('runs') / 'smoke'
    p = run_loop(run, '--iters', '2', '--yardstick', 'ext:toyheur')
    assert p.returncode == 0, p.stdout + p.stderr
    return run


def test_iterations_metrics_and_elo(smoke_run):
    m = read_metrics(str(smoke_run))
    assert [r['iter'] for r in m] == [0, 1, 2]
    assert m[1]['selfplay']['games'] == 24 and m[1]['selfplay']['samples'] > 0
    assert m[2]['train']['steps'] >= 10 and m[2]['eval_yard']['opp'] == 'ext:toyheur'
    r = json.load(open(smoke_run / 'elo' / 'ratings.json'))
    assert r['anchor'] == 'ext:toyheur' and r['ratings']['ext:toyheur']['elo'] == 1000
    assert {'iter_0000', 'iter_0001', 'iter_0002', 'random'} <= set(r['ratings'])
    idx = json.load(open(smoke_run / 'models' / 'index.json'))
    assert idx['best'] and os.path.exists(smoke_run / 'models' / 'best.pt')
    assert runctl.read_state(str(smoke_run), 'run')['state'] == 'done'


def test_resume_continues_and_stop_file(smoke_run, tmp_path):
    import shutil
    run = tmp_path / 'r'
    shutil.copytree(smoke_run, run)
    p = run_loop(run, '--iters', '3')
    assert p.returncode == 0, p.stderr
    assert [r['iter'] for r in read_metrics(str(run))] == [0, 1, 2, 3]
    (run / 'STOP').write_text('')
    p = run_loop(run, '--iters', '5')
    assert p.returncode == 0 and 'STOP file found' in p.stdout
    assert latest_iter(str(run)) == 3


def test_pause_exits_75_and_resume_finishes(tmp_path):
    run = tmp_path / 'p'
    run.mkdir()
    proc = subprocess.Popen([sys.executable, '-m', 'mtgz.az.run', '--run', str(run), *SMOKE, '--iters', '3',
                             '--games', '600', '--conc', '64'], cwd=ROOT, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, env=dict(os.environ, PYTHONIOENCODING='utf-8'))
    deadline = time.time() + 120
    while not os.path.exists(run / 'progress.json') and time.time() < deadline:   # self-play of iteration 1
        time.sleep(0.2)
    runctl.request_pause(str(run), by='test')
    t0 = time.time()
    out, _ = proc.communicate(timeout=120)
    assert proc.returncode == runctl.EXIT_PAUSED, out
    assert time.time() - t0 < 30
    st = runctl.read_state(str(run), 'run')
    assert st['state'] == 'paused'
    assert runctl.run_status(str(run))['state'] == 'paused'
    # a run started while PAUSE exists does not start
    p = run_loop(run, '--iters', '1')
    assert p.returncode == runctl.EXIT_PAUSED
    runctl.request_resume(str(run))
    p = run_loop(run, '--iters', '1', '--games', '24')
    assert p.returncode == 0, p.stdout + p.stderr
    assert [r['iter'] for r in read_metrics(str(run))] == [0, 1]


FAKE_RUN = '''import os, sys
run = sys.argv[sys.argv.index("--run") + 1]
p = os.path.join(run, "count")
k = open(p).read() if os.path.exists(p) else ""
open(p, "w").write(k + "x")
if len(k) == 1:
    open(os.path.join(run, "PAUSE"), "w").write("{}")
    sys.exit(75)
sys.exit(3 if len(k) == 0 else 0)
'''


def test_supervisor_with_fake_run(tmp_path):
    run = tmp_path / 'sup'
    run.mkdir()
    (run / 'config.json').write_text('{}')
    fake = tmp_path / 'fake.py'
    # run 1 crashes, run 2 pauses itself (PAUSE + exit 75), run 3 finishes
    fake.write_text(FAKE_RUN)
    env = dict(os.environ, MTGZ_SUPERVISE_WAIT='0.2', MTGZ_SUPERVISE_POLL='0.1',
               MTGZ_SUPERVISE_CMD=json.dumps([sys.executable, str(fake)]))
    sup = subprocess.Popen([sys.executable, 'tools/supervise.py', str(run)], cwd=ROOT, env=env)
    deadline = time.time() + 30
    while time.time() < deadline and (runctl.read_state(str(run), 'supervisor') or {}).get('state') != 'paused':
        time.sleep(0.05)
    assert (runctl.read_state(str(run), 'supervisor') or {}).get('state') == 'paused'
    assert (run / 'count').read_text() == 'xx'
    runctl.request_resume(str(run))
    assert sup.wait(timeout=30) == 0
    assert (run / 'count').read_text() == 'xxx'
    log = (run / 'supervisor.log').read_text()
    assert 'crash #1' in log and 'run paused' in log and 'clean exit' in log


def test_kill_run_finds_nothing_and_pause_tool(tmp_path):
    run = tmp_path / 'k'
    run.mkdir()
    (run / 'config.json').write_text('{}')
    p = subprocess.run([sys.executable, 'tools/kill_run.py', str(run)], cwd=ROOT, capture_output=True, text=True)
    assert p.returncode == 0 and 'no processes' in p.stdout
    p = subprocess.run([sys.executable, 'tools/pause_run.py', str(run)], cwd=ROOT, capture_output=True, text=True)
    assert p.returncode == 0 and (run / 'PAUSE').exists()
    p = subprocess.run([sys.executable, 'tools/pause_run.py', str(run), '--resume'], cwd=ROOT, capture_output=True, text=True)
    assert p.returncode == 0 and not (run / 'PAUSE').exists()


def test_fork_run(smoke_run, tmp_path):
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import fork_run
    assert fork_run.main([str(smoke_run / 'models' / 'iter_0002.pt'), str(tmp_path / 'n')]) == 0
    assert (tmp_path / 'n' / 'models' / 'iter_0000.pt').exists()
    c = tmp_path / 'c'
    assert fork_run.main(['--source', str(smoke_run), '--dest', str(c), '--iter', '1', '--set', 'reuse=8']) == 0
    assert json.load(open(c / 'config.json'))['reuse'] == 8
    assert latest_iter(str(c)) == 1 and [r['iter'] for r in read_metrics(str(c))] == [0, 1]
    p = run_loop(c, '--iters', '2')
    assert p.returncode == 0, p.stderr
    assert [r['iter'] for r in read_metrics(str(c))] == [0, 1, 2]


def test_dashboard(smoke_run, tmp_path):
    from mtgz.az import dashboard
    app = dashboard.app
    app.config['RUNS'] = str(tmp_path / 'none')
    os.makedirs(app.config['RUNS'])
    c = app.test_client()
    assert c.get('/').status_code == 200 and b'No runs yet' in c.get('/').data
    assert c.get('/api/runs').get_json() == []
    app.config['RUNS'] = str(smoke_run.parent)
    runs = c.get('/api/runs').get_json()
    assert runs[0]['name'] == 'smoke'
    d = c.get('/api/run/smoke').get_json()
    assert len(d['metrics']) == 3 and d['anchor'] == 'ext:toyheur' and d['control']['state'] == 'stopped'
    assert c.get('/api/run/..%2Fsmoke').status_code in (404, 405)
    assert c.get('/api/run/nope').status_code == 404
    assert c.post('/api/run/smoke/pause').status_code == 400          # JSON body required
    r = c.post('/api/run/smoke/pause', json={})
    assert r.status_code == 200 and os.path.exists(smoke_run / 'PAUSE')
    r = c.post('/api/run/smoke/resume', json={})
    assert r.status_code == 200 and not os.path.exists(smoke_run / 'PAUSE')
    assert c.post('/api/run/smoke/explode', json={}).status_code == 404


def test_dashboard_is_mtg_zeros():
    from mtgz.az import dashboard
    from zero import package
    assert package.active().name == 'mtgz' and package.active().dashboard_port == 5340
    page = dashboard.app.test_client().get('/').data
    assert b'<title>MTG Zero</title>' in page and b'AlphaZero training for MTG' in page
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.dashboard', '--help'], cwd=ROOT, capture_output=True, text=True)
    assert p.returncode == 0 and 'MTG Zero training dashboard' in p.stdout and '5340' in p.stdout


def test_h2h_and_targets_commands(smoke_run):
    m1, m2 = (str(smoke_run / 'models' / f'iter_000{k}.pt') for k in (1, 2))
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.h2h', m1, m2, '--games', '8', '--sims', '4', '--chance-k', '0',
                        '--threads', '1'], cwd=ROOT, capture_output=True, text=True, timeout=300)
    assert p.returncode == 0, p.stderr
    r = json.loads(p.stdout.strip().splitlines()[-1])
    assert r['game'] == 'toy' and r['games'] == 8
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.targets', 'sharpness', '--model', m2, '--game', 'toy', '--games', '4',
                        '--sims', '8', '--sims-fast', '4', '--chance-k', '0', '--c-scales', '1.0', '--threads', '1'],
                       cwd=ROOT, capture_output=True, text=True, timeout=300)
    assert p.returncode == 0, p.stderr
    r = json.loads(p.stdout.strip().splitlines()[-1])
    assert r['game'] == 'toy' and set(r['arms']) == {'visits', 'cq c_scale 1'}


def test_shims_are_the_core_modules():
    import zero
    from mtgz.az import data, elo, games, infer, mcts, net, play, protocol, run, runctl, toybots
    for m in (data, elo, infer, mcts, net, play, protocol, run, runctl, toybots):
        assert m.__name__.startswith('zero.'), m
    assert games.__name__ == 'mtgz.az.games' and games.engine is zero.games.engine
    core = os.path.normcase(os.path.join(ROOT, 'third_party', 'zero-core', 'python', 'zero'))
    assert os.path.normcase(os.path.dirname(zero.__file__)) == core       # the pinned core, not another
