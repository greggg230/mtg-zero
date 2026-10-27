"""Remote evaluation on the MacBook (mtgz/az/mac.py, docs/ENGINE.md 44): the remote spec is the run
checkout's commit, resolved when zero.remote reads it (not at import), refused for a checkout with
uncommitted changes; the remote side refuses a checkout at another commit, changed, or without its
engine stamp. On scratch git repos (no ssh: the preflight is tested end to end, ENGINE.md 44)."""
import os
import subprocess

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
