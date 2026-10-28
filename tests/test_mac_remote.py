"""Remote evaluation on the MacBook (mtgz/az/mac.py, docs/ENGINE.md 44): the remote spec is the run
checkout's commit, resolved when zero.remote reads it (not at import), refused for a checkout with
uncommitted changes; the remote side refuses a checkout at another commit, changed, or without its
engine stamp. On scratch git repos (no ssh: the preflight is tested end to end, ENGINE.md 44). A
co-training run's resolver mirror (ENGINE.md 48) is evaluated by the same remote side, after the same
check."""
import json
import os
import subprocess
import sys

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
                               workdir='/Users/u/mtgz-remote', env={'X': '1'}, overrides={'threads': 12})


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
