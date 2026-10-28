"""Remote evaluation on the MacBook (mtgz/az/mac.py, docs/ENGINE.md 44): the remote spec is the run
checkout's commit, resolved when zero.remote reads it (not at import), refused for a checkout with
uncommitted changes; the remote side refuses a checkout at another commit, changed, or without its
engine stamp. On scratch git repos (no ssh: the preflight is tested end to end, ENGINE.md 44). A
co-training run's resolver mirror (ENGINE.md 48) is evaluated by the same remote side, after the same
check. Remote self-play (ENGINE.md 52): the spec carries the Mac's self-play settings, a worker
refuses other code, a worker plays `mtg` self-play and queued matches through zero-core's remote side,
and tools/mac_backoff.py yields to its busy markers."""
import json
import os
import subprocess
import sys
import threading
import time

import pytest

import mtgz.az  # noqa: F401  (registers the game package)
from mtgz.az import mac
from zero import package, remote


def git(root, *args):
    return subprocess.run(['git', '-C', str(root), '-c', 'user.name=t', '-c', 'user.email=t@t', *args],
                          capture_output=True, text=True, check=True).stdout.strip()


@pytest.fixture
def repo(tmp_path):
    r = tmp_path / 'checkout'
    r.mkdir()
    git(r, 'init', '-q')
    (r / 'a.txt').write_text('one\n')
    git(r, 'add', 'a.txt')
    git(r, 'commit', '-q', '-m', 'one')
    return r


def spec_for(root):
    return mac.RunCommitRemote(str(root), host='u@mac', home='/Users/u', python='/venv/bin/python',
                               workdir='/Users/u/mtgz-remote', env={'X': '1'}, overrides={'threads': 12},
                               selfplay={'workdir': '/Users/u/mtgz-selfplay', 'procs': 2})


def test_the_package_registers_the_mac_lazily():
    r = package.active().remotes['mac']
    assert isinstance(r, mac.RunCommitRemote)
    assert r._spec is None              # importing the package ran no git and no ssh


def test_the_spec_is_the_checkouts_commit(repo, monkeypatch):
    sha = git(repo, 'rev-parse', 'HEAD')
    r = spec_for(repo)
    monkeypatch.setattr(r, 'preflight', lambda spec, head: None)
    monkeypatch.setitem(package.active().remotes, 'mac', r)
    s = remote.remote_spec('mac')       # what RemoteEval does when the run starts
    assert s['repo'] == f'/Users/u/mtg-zero-wt/run-{sha[:12]}'
    assert s['env'] == {'X': '1', 'MTGZ_REMOTE_COMMIT': sha}
    assert (s['host'], s['python'], s['workdir'], s['overrides']) == ('u@mac', '/venv/bin/python',
                                                                      '/Users/u/mtgz-remote', {'threads': 12})
    assert s['selfplay'] == {'workdir': '/Users/u/mtgz-selfplay', 'procs': 2}
    assert remote.remote_spec('mac', 'selfplay_remote')['repo'] == s['repo']    # remote self-play's: the same
    # a new commit: a new spec (a run started at another commit evaluates on another checkout)
    (repo / 'a.txt').write_text('two\n')
    git(repo, 'commit', '-q', '-am', 'two')
    sha2 = git(repo, 'rev-parse', 'HEAD')
    assert spec_for(repo).resolve(preflight=False)['repo'].endswith(f'run-{sha2[:12]}')


def test_a_checkout_with_uncommitted_changes_is_refused(repo):
    (repo / 'a.txt').write_text('changed\n')
    with pytest.raises(SystemExit, match='uncommitted changes'):
        spec_for(repo).resolve(preflight=False)
    (repo / 'a.txt').write_text('one\n')
    (repo / 'untracked.txt').write_text('x\n')      # untracked files (a built engine, runs/) are not code
    assert spec_for(repo).resolve(preflight=False)['env']['MTGZ_REMOTE_COMMIT'] == git(repo, 'rev-parse', 'HEAD')


def test_the_remote_side_refuses_any_other_code(repo):
    sha = git(repo, 'rev-parse', 'HEAD')
    stamp = os.path.join(str(repo), git(repo, 'rev-parse', '--git-path', mac.STAMP))
    assert any('no stamp' in p for p in mac.problems(str(repo), sha))
    with open(stamp, 'w') as f:
        f.write(sha + '\n')
    assert mac.problems(str(repo), sha) == []
    mac.verify_checkout(str(repo), sha)
    assert mac.problems(str(repo), '') == ['MTGZ_REMOTE_COMMIT is not set (the commit of the run\'s code)']
    with pytest.raises(SystemExit, match='HEAD is'):
        mac.verify_checkout(str(repo), 'f' * 40)
    (repo / 'a.txt').write_text('changed\n')
    with pytest.raises(SystemExit, match='changed: M a.txt'):
        mac.verify_checkout(str(repo), sha)
    git(repo, 'commit', '-q', '-am', 'two')         # a new commit: the old stamp no longer counts
    sha2 = git(repo, 'rev-parse', 'HEAD')
    assert any('not built at' in p for p in mac.problems(str(repo), sha2))
    assert mac.problems(str(repo / 'nowhere'), sha)[0].startswith('not a git checkout')


def test_a_resolver_mirror_is_evaluated_after_the_same_check(tmp_path):
    """A co-training run's resolver mirror (<workdir>/<run>.resolver, zero-core docs/az.md 11.5) holds
    the resolver's settings (team_draft-smoke's `mtg`: a single-game run's): `python -m mtgz.az.remote
    eval` refuses it with other code than the run's before evaluating anything, and the remote side it
    then runs (zero.remote.evaluate_mirror) evaluates it as any mirror."""
    from zero.cotrain import resolver_config
    from zero.net import save
    from zero.run import DEFAULTS, PRESETS, Run, model_path
    rc = resolver_config(dict(DEFAULTS, **PRESETS['team_draft-smoke']))
    assert rc['game'] == 'mtg' and rc['resolver'] == '' and rc['eval_remote'] == ''
    mirror = tmp_path / 'mtgz-remote' / 'td.resolver'
    (mirror / 'models').mkdir(parents=True)
    cfg = dict(rc, eval_games=2, eval_random_games=2, ref_games=2, elo_games=2)
    (mirror / 'config.json').write_text(json.dumps(cfg))
    save(Run(str(mirror), cfg).new_model(), model_path(str(mirror), 0), {'iter': 0})
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.remote', 'eval', str(mirror), '0'], cwd=root,
                       env=dict(os.environ, MTGZ_REMOTE_COMMIT='f' * 40, PYTHONIOENCODING='utf-8'),
                       capture_output=True, text=True, timeout=300)
    assert p.returncode != 0 and "is not the run's code: HEAD is" in p.stderr, p.stdout + p.stderr
    assert not (mirror / 'metrics.jsonl').exists() and not (tmp_path / 'mtgz-remote' / '.eval.lock').exists()
    m = remote.evaluate_mirror(str(mirror), 0)
    assert m['eval_yard']['opp'] == 'heuristic' and m['eval_yard']['games'] == 2 and m['eval_random']['games'] == 2
    assert 'elo' in m and json.load(open(mirror / 'elo' / 'ratings.json'))['anchor'] == 'heuristic'


def test_an_eval_net_mirror_is_evaluated_after_the_same_check(tmp_path):
    """A co-training run's eval_yard_net mirror (<workdir>/<run>, run setting draft_eval_net_remote,
    zero-core docs/az.md 11.7; ENGINE.md 53) holds the run's settings (team_draft-smoke's) and both
    networks' checkpoints: `python -m mtgz.az.remote evalnet` refuses it with other code than the run's
    before evaluating or locking anything; the remote side it then runs (zero.remote.evaluate_net_mirror)
    plays the draft network's events against `consensus` with the play network's matches."""
    from zero.cotrain import CoRun
    from zero.net import save
    from zero.run import DEFAULTS, PRESETS, model_path
    cfg = dict(DEFAULTS, **PRESETS['team_draft-smoke'])
    cfg.update(eval_games=2, draft_eval_net_every=1)
    mirror = tmp_path / 'mtgz-remote' / 'td'
    (mirror / 'models').mkdir(parents=True)
    (mirror / 'resolver' / 'models').mkdir(parents=True)
    (mirror / 'config.json').write_text(json.dumps(cfg))
    co = CoRun(str(mirror), cfg)
    save(co.P.new_model(), model_path(str(mirror), 0), {'iter': 0})
    save(co.R.new_model(), model_path(co.R.run, 0), {'iter': 0})
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.remote', 'evalnet', str(mirror), '0'], cwd=ROOT,
                       env=dict(os.environ, MTGZ_REMOTE_COMMIT='f' * 40, PYTHONIOENCODING='utf-8'),
                       capture_output=True, text=True, timeout=300)
    assert p.returncode != 0 and "is not the run's code: HEAD is" in p.stderr, p.stdout + p.stderr
    assert not (mirror / 'metrics.jsonl').exists() and not (tmp_path / 'mtgz-remote' / '.eval.lock').exists()
    m = remote.evaluate_net_mirror(str(mirror), 0)
    e = m['eval_yard_net']
    assert e['opp'] == 'consensus' and e['games'] == 2 and e['unfinished'] == 0 and m['seconds'] > 0
    assert [r['iter'] for r in map(json.loads, open(mirror / 'metrics.jsonl'))] == [0]


# ---------------------------------------------------------------- remote self-play (ENGINE.md 52)

def test_the_macs_selfplay_settings():
    """The Mac self-plays in ~/mtgz-selfplay with 50.3's best settings, 2 processes, at nice 5, and backs
    off for the other games' evaluations (its own remote's ~/mtgz-remote/.eval.lock always)."""
    sp = package.active().remotes['mac'].selfplay
    assert sp['workdir'] == '/Users/greggkeithley/mtgz-selfplay' and sp['procs'] == 2 and sp['nice'] == 5
    assert sp['locks'] == ['/Users/greggkeithley/kbz-remote/.eval.lock', '/Users/greggkeithley/domz-remote/.eval.lock']
    o = sp['overrides']
    assert (o['infer_device'], o['conc'], o['threads'], o['groups'], o['infer_workers'], o['torch_threads']) == \
        ('cpu', 768, 12, 4, 4, 3)
    assert sp['env'] == {'OMP_NUM_THREADS': '3'}


ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def test_a_selfplay_worker_refuses_other_code(tmp_path):
    """`python -m mtgz.az.remote selfplay` checks the checkout first: at another commit it refuses
    before reading its mirror or taking its worker slot."""
    mirror = tmp_path / 'mtgz-selfplay' / 'run'
    mirror.mkdir(parents=True)
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.remote', 'selfplay', str(mirror), '0'], cwd=ROOT,
                       env=dict(os.environ, MTGZ_REMOTE_COMMIT='f' * 40, PYTHONIOENCODING='utf-8'),
                       capture_output=True, text=True, timeout=300)
    assert p.returncode != 0 and "is not the run's code: HEAD is" in p.stderr, p.stdout + p.stderr
    assert os.listdir(mirror) == []


def _worker(mirror, cfg, mode):
    """zero-core's remote self-play worker for the mtgz package, as `python -m mtgz.az.remote selfplay`
    runs it after the checkout check (which needs the Mac's stamped checkout)."""
    (mirror / 'models').mkdir(parents=True)
    (mirror / 'config.json').write_text(json.dumps(cfg))
    (mirror / 'selfplay.json').write_text(json.dumps({'mode': mode, 'locks': [], 'run_id': 'test',
                                                      'workdir': str(mirror.parent), 'mirror': mirror.name,
                                                      'test': {'status_s': 0.5}}))
    from zero.net import save
    from zero.run import Run, model_path
    save(Run(str(mirror), cfg).new_model(), model_path(str(mirror), 0), {'iter': 0})
    code = 'import sys, mtgz.az; from zero.remote import main; main(sys.argv[1:])'
    p = subprocess.Popen([sys.executable, '-c', code, 'selfplay', str(mirror), '0'], cwd=ROOT, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8',
                         env=dict(os.environ, PYTHONIOENCODING='utf-8'))
    events = []

    def read():
        for line in p.stdout:
            if line.startswith('ZERO_SP '):
                events.append(json.loads(line[len('ZERO_SP '):]))
    threading.Thread(target=read, daemon=True).start()
    threading.Thread(target=lambda: [None for _ in p.stderr], daemon=True).start()

    def send(**kw):
        p.stdin.write(json.dumps(kw) + '\n')
        p.stdin.flush()

    def wait_for(pred, t=300):
        end = time.time() + t
        while time.time() < end:
            if any(pred(e) for e in list(events)):
                return
            time.sleep(0.2)
        raise AssertionError(f'timed out; events {events[-5:]}')
    return p, events, send, wait_for


@pytest.mark.parametrize('mode', ['selfplay', 'matches'])
def test_a_worker_plays_mtg(tmp_path, mode):
    """A self-play worker with mtg-smoke's settings (self-play: generated decks; matches: queued
    best-of-three specs with tags, as a co-training run's resolver episodes) plays with checkpoint 0,
    writes compressed shards of mtg samples, answers rows by tag and ends its phase on request."""
    from zero.data import read_shard
    from zero.run import DEFAULTS, PRESETS
    from mtgz.az import SPEC_BO3
    cfg = dict(DEFAULTS, **PRESETS['mtg-smoke'])
    cfg.update(conc=4, threads=2, shard_rows=64, max_steps=60_000)
    mirror = tmp_path / 'mtgz-selfplay' / 'r'
    p, events, send, wait_for = _worker(mirror, cfg, mode)
    try:
        wait_for(lambda e: e['ev'] == 'hello', 120)
        send(cmd='phase', iter=1, model='models/iter_0000.pt')
        if mode == 'matches':
            for k in range(3):
                send(cmd='enqueue', tag=f'7/{k}', spec=json.dumps(dict(json.loads(SPEC_BO3), seed=100 + k)))
            wait_for(lambda e: sum(x['ev'] == 'row' for x in events) >= 3, 600)    # in any order
        else:
            wait_for(lambda e: e['ev'] == 'status' and e.get('games', 0) >= 2, 600)
        send(cmd='end', iter=1)
        wait_for(lambda e: e['ev'] == 'end', 300)
        send(cmd='quit')
        assert p.wait(60) == 0
    finally:
        if p.poll() is None:
            p.kill()
    end = next(e for e in events if e['ev'] == 'end')
    shards = [e for e in events if e['ev'] == 'shard']
    assert end['iter'] == 1 and end['games'] >= 2 and end['samples'] > 0 and end['shards'] == len(shards) >= 1
    rows = 0
    for e in shards:
        sh = read_shard(e['path'])
        assert sh['meta']['game'] == 'mtg' and sh['meta']['source'] == 'remote' and sh['meta']['iter'] == 1
        assert sh['x'].shape[1] == sh['meta']['features'] and int(sh['net'].max()) == 0
        rows += len(sh['player'])
    assert rows == end['samples']
    if mode == 'matches':
        tags = sorted(e['row']['tag'] for e in events if e['ev'] == 'row')
        assert tags == ['7/0', '7/1', '7/2']
        assert all(sorted(e['row']['result']) in ([0.0, 1.0], [0.5, 0.5]) for e in events if e['ev'] == 'row')
    assert not (mirror / '.w00.lock').exists() and not list(mirror.parent.glob('.busy-*'))


def test_the_farm_yields_to_selfplay_busy_markers(tmp_path, monkeypatch):
    """tools/mac_backoff.py (the fuzz farm, the studies) backs off while a self-play worker's busy
    marker is held by a live process, as for an evaluation lock."""
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    import mac_backoff
    assert mac_backoff.SELFPLAY_BUSY.endswith(os.path.join('mtgz-selfplay', '.busy-*'))
    assert mac_backoff.SELFPLAY_BUSY in mac_backoff.lock_dirs()
    pat = str(tmp_path / '.busy-*')
    assert mac_backoff.held([pat]) is None
    b = tmp_path / '.busy-td1.resolver-w00'
    b.mkdir()
    (b / 'owner').write_text(f'{os.getpid()} {time.time()}')
    monkeypatch.setattr(mac_backoff, '_alive', lambda pid: pid == os.getpid())
    assert mac_backoff.held([pat]) == str(b)
    (b / 'owner').write_text(f'{os.getpid() + 1} {time.time()}')
    assert mac_backoff.held([pat]) is None
