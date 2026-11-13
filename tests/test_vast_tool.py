"""tools/vast.py (docs/ENGINE.md 97), the parts that need no rented box: the run arguments a box run gets
(remote settings dropped, desktop paths remapped), the managed ssh-config blocks, the homing marker a sync loop
stops at, the deadline guard's start script, and the 'vast' remote spec of the game package."""
import json
import os
import sys
import time
import types

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import vast  # noqa: E402


def test_run_args_drop_the_remote_settings_and_remap_the_desktop_paths(tmp_path):
    args = ['--preset', 'delver-mirror', '--eval-remote', 'mac', '--selfplay-remote', 'mac', '--selfplay-remote-procs', '3',
            '--games', '2000', '--selfplay-remote-linger', '1', '--selfplay-local-linger', '1',
            '--elo-anchors', 'a192=C:/x/anchors/a_0192.pt,a160=C:/x/anchors/a_0160.pt',
            '--heldout-shards', 'C:/x/heldout/h.npz', '--threads', '16']
    (tmp_path / 'supervisor_state.json').write_text(json.dumps({'args': args}))
    out = vast.run_args(str(tmp_path), {})
    assert out == ['--preset', 'delver-mirror', '--games', '2000',
                   '--elo-anchors', 'a192=/root/runs/_ext/a_0192.pt,a160=/root/runs/_ext/a_0160.pt',
                   '--heldout-shards', '/root/runs/_ext/h.npz', '--threads', '16']


def test_remap_paths_lists_the_files_to_upload():
    paths, files = vast.remap_paths({'elo_anchors': 'a=C:/p/one.pt', 'heldout_shards': 'C:/q/h1.npz,C:/q/h2.npz'})
    assert paths['elo_anchors'] == ('a=C:/p/one.pt', 'a=/root/runs/_ext/one.pt')
    assert paths['heldout_shards'][1] == '/root/runs/_ext/h1.npz,/root/runs/_ext/h2.npz'
    assert files == ['C:/p/one.pt', 'C:/q/h1.npz', 'C:/q/h2.npz']
    assert vast.remap_paths({'elo_anchors': '', 'heldout_shards': ''}) == ({}, [])


def test_ssh_alias_blocks_are_replaced_and_removed_without_touching_the_rest(tmp_path, monkeypatch):
    cfg = tmp_path / 'config'
    cfg.write_text('Host mac\n  HostName 100.95.248.116\n')
    monkeypatch.setattr(vast, 'SSH_CONFIG', str(cfg))
    monkeypatch.delenv('MTGZ_VAST_HOST', raising=False)
    vast.write_alias('vast', '1.2.3.4', 1111)
    vast.write_alias('arm1', '5.6.7.8', 2222)
    vast.write_alias('vast', '9.9.9.9', 3333)              # a new box under the same name replaces the block
    s = cfg.read_text()
    assert s.count('Host mtgz-vast\n') == 1 and 'HostName 9.9.9.9\n  Port 3333' in s and '1.2.3.4' not in s
    assert 'Host mtgz-arm1\n' in s and s.endswith('Host mac\n  HostName 100.95.248.116\n')
    vast.remove_alias('vast')
    vast.remove_alias('arm1')
    assert cfg.read_text() == 'Host mac\n  HostName 100.95.248.116\n'


def test_a_sync_loop_stops_once_run_home_marks_the_run(tmp_path):
    run = str(tmp_path)
    assert vast.homing(run)                                # no marker: not on a box
    (tmp_path / vast.ON_VAST).write_text(json.dumps({'box': 'vast'}))
    assert not vast.homing(run)
    (tmp_path / vast.ON_VAST).write_text(json.dumps({'box': 'vast', 'homing': 1.0}))
    assert vast.homing(run)


def test_the_start_script_carries_the_guard_and_its_deadline():
    s = vast.onstart_script(1_000_000)
    assert f'echo {1_000_000 + vast.GUARD_MARGIN_S} > "$G/deadline"' in s and 'vast_guard.py' in s
    assert len(s) < 4000                                   # Vast's start-script limit


def test_the_vast_remote_is_a_run_commit_remote_on_the_box():
    pytest.importorskip('torch')
    import mtgz.az  # noqa: F401
    from zero import package
    spec = package.active().remotes['vast']
    assert spec.fixed['host'] == os.environ.get('MTGZ_VAST_HOST', 'mtgz-vast') and spec.home == '/root'
    assert spec.selfplay['procs'] == 1 and spec.selfplay['overrides']['infer_device'] == 'cuda'
    assert spec.prepare == 'python tools/vast.py prep vast'
    assert package.active().remotes['mac'].prepare == 'bash tools/mac.sh --eval-checkout'


def test_the_workers_file_of_a_desktop_run(tmp_path, monkeypatch, capsys):
    monkeypatch.setattr(vast, 'load_ledger', lambda: {'w1': {'iid': 1}, 'w2': {'iid': 2}})
    run = str(tmp_path)

    def workers(what, *names):
        vast.cmd_workers(vast.argparse.Namespace(run=run, what=what, names=list(names)))
        lines = (tmp_path / 'selfplay_remotes').read_text().splitlines()
        return [x for x in lines if x and not x.startswith('#')]
    assert workers('set', 'vast:w1', 'gpu:1') == ['vast:w1', 'gpu:1']
    assert workers('add', 'vast:w2', 'vast:w1') == ['vast:w1', 'gpu:1', 'vast:w2']
    assert workers('remove', 'gpu:1') == ['vast:w1', 'vast:w2']
    assert workers('clear') == []
    with pytest.raises(SystemExit):
        workers('add', 'vast:nobox')                     # not a box of the ledger
    with pytest.raises(SystemExit):
        workers('add', 'w1')                             # not a remote name


def test_offers_filter_by_gpu_count():
    a = vast.argparse.Namespace(max_dph=2.0, min_ram=96, min_cores=32, gpus=2)
    assert vast.filters_from(a)['num_gpus'] == 2 and vast.filters_from(a)['gpu_name'] == 'RTX 5090'


def test_any_box_and_a_local_gpu_are_remotes_of_the_package():
    pytest.importorskip('torch')
    import mtgz.az  # noqa: F401
    from zero import package
    r = package.active().remotes
    w1 = r['vast:']('w1')
    assert w1.fixed['host'] == 'mtgz-w1' and w1.prepare == 'python tools/vast.py prep w1'
    assert w1.selfplay['overrides'] == r['vast'].selfplay['overrides']
    g = r['gpu:']('1')
    assert g.device == 1 and g.selfplay_overrides['infer_device'] == 'cuda'


# ---------------------------------------------------------------------------- the Fable review's fixes (ENGINE.md 143)

class _R:
    def __init__(self, rc=0, out='', err=''):
        self.returncode, self.stdout, self.stderr = rc, out, err


def _fake_box(monkeypatch, remote, box_hashes=None):
    """sync_once's remote side stubbed: `remote` = the box's listing {path: (size, mtime)}, `box_hashes` = the
    box's sha256 of each path (asked through remote_hashes). -> (the fetched paths, the hashed paths)."""
    fetched, asked = [], []
    monkeypatch.setattr(vast, 'remote_listing', lambda name, rd: dict(remote))

    def hashes(name, rd, paths):
        asked.extend(paths)
        return {p: box_hashes[p] for p in paths}
    monkeypatch.setattr(vast, 'remote_hashes', hashes)
    def fetch(name, rd, run_, paths, timeout_s=1800):
        for p in paths:                                 # (the box's file: its size, other bytes)
            os.makedirs(os.path.dirname(os.path.join(run_, p)), exist_ok=True)
            with open(os.path.join(run_, p), 'wb') as f:
                f.write(b'?' * remote[p][0])
        fetched.extend(paths)
        return 0
    monkeypatch.setattr(vast, 'tar_fetch', fetch)
    return fetched, asked


def test_run_home_pulls_a_file_the_box_rewrote_at_the_same_size(tmp_path, monkeypatch):
    """B1: models/opt_latest.pt (the optimizer state of a later iteration) and models/best.pt (another checkpoint
    copied over it) have the same size as the desktop's; a size-only check never brought them back, so the desktop
    resumed with a fresh optimizer. Same size + another version -> compared by sha256 -> fetched."""
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    (run / 'models' / 'opt_latest.pt').write_bytes(b'A' * 1000)      # the desktop's: the state at the push
    (run / 'models' / 'best.pt').write_bytes(b'B' * 2000)
    (run / 'models' / 'iter_0100.pt').write_bytes(b'C' * 2000)
    t = time.time() - 3600
    remote = {'models/opt_latest.pt': (1000, t), 'models/best.pt': (2000, t), 'models/iter_0100.pt': (2000, t - 9),
              'models/iter_0101.pt': (2000, t)}
    same = vast.sha256_file(str(run / 'models' / 'iter_0100.pt'))
    fetched, asked = _fake_box(monkeypatch, remote, {'models/opt_latest.pt': 'f' * 64, 'models/best.pt': 'e' * 64,
                                                     'models/iter_0100.pt': same})
    vast.sync_once('box', str(run), '/root/runs/run', data=True, final=True)
    assert sorted(fetched) == ['models/best.pt', 'models/iter_0101.pt', 'models/opt_latest.pt']
    assert sorted(asked) == ['models/best.pt', 'models/iter_0100.pt', 'models/opt_latest.pt']
    # the manifest now holds the box's listing entry of every file the desktop has as the box's: the next pass
    # neither fetches nor hashes them
    pulled = json.loads((run / vast.PULLED).read_text())
    assert pulled == {p: list(v) for p, v in remote.items()}
    fetched.clear()
    asked.clear()
    vast.sync_once('box', str(run), '/root/runs/run', data=True, final=True)
    assert fetched == [] and asked == []
    # the box rewrites the optimizer state again (same size, a new mtime): hashed and fetched once more
    remote['models/opt_latest.pt'] = (1000, t + 600)
    vast.sync_once('box', str(run), '/root/runs/run', data=True, final=True)
    assert fetched == ['models/opt_latest.pt']


def test_an_appended_log_is_extended_only_when_its_tail_matches_the_box(tmp_path, monkeypatch):
    """B18: an append-only log the desktop also wrote to (a supervisor started by mistake logging "PAUSE file
    present") was extended from the wrong offset - two logs spliced. Its last bytes are compared with the box's at
    that offset first; a mismatch fetches it whole."""
    run = tmp_path / 'run'
    run.mkdir()
    box_log = b'line 1\nline 2\nline 3\n'

    def fake_run(argv, **kw):
        cmd = argv[-1]                                  # 'tail -c +S file | head -c N'
        start = int(cmd.split('tail -c +')[1].split()[0]) - 1
        n = int(cmd.split('head -c ')[1])
        return _R(0, box_log[start:start + n])
    monkeypatch.setattr(vast.subprocess, 'run', fake_run)
    (run / 'log.txt').write_bytes(box_log[:7])          # the desktop has the box's first line
    assert vast.append_tail('box', '/r', 'log.txt', str(run / 'log.txt'), 7, len(box_log)) == box_log[7:]
    (run / 'log.txt').write_bytes(b'PAUSE!\n')           # same length, another content: never spliced
    assert vast.append_tail('box', '/r', 'log.txt', str(run / 'log.txt'), 7, len(box_log)) is None
    fetched, _ = _fake_box(monkeypatch, {'log.txt': (len(box_log), time.time() - 60)})
    vast.sync_once('box', str(run), '/r', data=True)
    assert fetched == ['log.txt']


def test_an_ssh_failure_is_unknown_never_a_stopped_box_run(monkeypatch):
    """B2: box_supervisor_running read ssh's own failure (exit 255, no output) as 'no': run-home then pulled a live
    run and unpaused the desktop copy. Now three answers: running, stopped, unknown (None)."""
    answers = []
    monkeypatch.setattr(vast, 'ssh', lambda *a, **k: answers.pop(0))
    answers.append(_R(255, '', 'ssh: connect to host ... port 22: Connection timed out'))
    assert vast.box_run_status('box', '/root/runs/r') is None
    answers.append(_R(0, 'Welcome to vast.ai\n'))                     # not the script's answer
    assert vast.box_run_status('box', '/root/runs/r') is None
    answers.append(_R(0, '@@status yes 1\n{"state": "running", "pid": 7}\n'))
    assert vast.box_run_status('box', '/root/runs/r') == {'supervisor': True, 'run': 1, 'state': 'running'}
    answers.append(_R(0, '@@status no 0\n{"state": "done"}\n'))
    assert vast.box_run_status('box', '/root/runs/r') == {'supervisor': False, 'run': 0, 'state': 'done'}
    answers.append(_R(0, '@@status no 0\n'))                         # never started: no state file
    assert vast.box_run_status('box', '/root/runs/r') == {'supervisor': False, 'run': 0, 'state': None}


@pytest.fixture
def home(tmp_path, monkeypatch):
    """run-home against a stubbed box: a desktop run dir on box 'vast', box_run_status answering from a script,
    the pulls and the checks recorded."""
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    (run / 'models' / 'iter_0003.pt').write_bytes(b'x')
    (run / 'config.json').write_text(json.dumps({'threads': 32, 'eval_remote': '', 'extra': 1}))
    (run / 'PAUSE').write_text(json.dumps({'by': vast.PAUSE_BY}))
    marker = {'box': 'vast', 'iid': 1, 'remote_dir': '/root/runs/run', 'iter_at_push': 2,
              'restore': {'eval_remote': 'mac', 'threads': 16}, 'absent': ['extra']}
    vast.write_marker(str(run), marker)
    monkeypatch.setattr(vast, 'load_ledger', lambda: {'vast': {'iid': 1, 'state': 'running'}})
    monkeypatch.setattr(vast.time, 'sleep', lambda s: None)
    monkeypatch.setattr(vast, 'wait_sync_loop', lambda run_: None)
    st = types.SimpleNamespace(status=[], pulls=[], ssh=[], verify=lambda *a: ['ok'])

    def status(name, rd):
        return st.status.pop(0) if st.status else {'supervisor': False, 'run': 0, 'state': 'done'}
    monkeypatch.setattr(vast, 'box_run_status', status)
    monkeypatch.setattr(vast, 'ssh', lambda name, cmd, **k: st.ssh.append(cmd) or _R())
    monkeypatch.setattr(vast, 'sync_once', lambda *a, **k: st.pulls.append(k) or (0, 0, 0))
    monkeypatch.setattr(vast, 'verify_home', lambda *a: st.verify(*a))

    def go(**kw):
        a = dict(name='vast', run=str(run), wait_min=30, tries=3, assume_stopped=False, allow_opt_mismatch=False)
        a.update(kw)
        vast.cmd_run_home(vast.argparse.Namespace(**a))
    st.run, st.go = run, go
    return st


def test_run_home_refuses_while_the_box_cannot_be_reached(home):
    home.status = [None, None, None]
    with pytest.raises(SystemExit, match='unknown is not stopped'):
        home.go()
    m = json.loads((home.run / vast.ON_VAST).read_text())
    assert 'homing' not in m and home.pulls == [] and home.ssh == []      # no STOP, no pull, the mirror goes on
    assert (home.run / 'PAUSE').exists()


def test_run_home_waits_through_an_unknown_answer_and_confirms_the_stop(home):
    home.status = [{'supervisor': True, 'run': 1, 'state': 'running'}, None,
                   {'supervisor': True, 'run': 1, 'state': 'running'}, {'supervisor': False, 'run': 0, 'state': 'done'}]
    home.go()
    assert len(home.ssh) == 1 and 'STOP' in home.ssh[0] and home.pulls == [{'data': True, 'final': True}]
    assert not (home.run / vast.ON_VAST).exists() and not (home.run / 'PAUSE').exists() and (home.run / 'STOP').exists()
    cfg = json.loads((home.run / 'config.json').read_text())
    assert cfg == {'threads': 16, 'eval_remote': 'mac'}        # restored; the setting the desktop lacked dropped


def test_run_home_refuses_a_box_run_that_did_not_end_cleanly(home):
    home.status = [{'supervisor': False, 'run': 0, 'state': 'restarting'}]
    with pytest.raises(SystemExit, match="'restarting', not 'done'"):
        home.go()
    assert home.pulls == [] and 'homing' not in json.loads((home.run / vast.ON_VAST).read_text())
    home.status = [{'supervisor': False, 'run': 2, 'state': 'done'}]
    with pytest.raises(SystemExit, match='2 mtgz.az.run process'):
        home.go()
    home.status = [{'supervisor': False, 'run': 0, 'state': 'restarting'}]
    home.go(assume_stopped=True)
    assert home.pulls


def test_a_failed_final_pull_clears_homing(home):
    def bad(*a):
        raise SystemExit('the optimizer state is of iteration 2, the newest checkpoint is 3')
    home.verify = bad
    with pytest.raises(SystemExit, match='iteration 2'):
        home.go()
    m = json.loads((home.run / vast.ON_VAST).read_text())
    assert 'homing' not in m and (home.run / 'PAUSE').exists()            # still on the box; not declared home


def test_verify_home_wants_the_optimizer_state_of_the_newest_checkpoint(tmp_path, monkeypatch):
    torch = pytest.importorskip('torch')
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    for k in (99, 100):
        (run / 'models' / f'iter_{k:04d}.pt').write_bytes(b'ckpt %d' % k)
    torch.save({'iter': 99, 'state': {}}, run / 'models' / 'opt_latest.pt')
    (run / 'models' / 'best.pt').write_bytes(b'ckpt 100')
    (run / 'models' / 'index.json').write_text(json.dumps({'best': {'file': 'iter_0100.pt'}}))
    mine = {p: vast.sha256_file(str(run / p)) for p in ('models/opt_latest.pt', 'models/best.pt')}
    box_files = {'models/opt_latest.pt': (1, 1.0), 'models/best.pt': (1, 1.0)}
    monkeypatch.setattr(vast, 'remote_listing', lambda name, rd: dict(box_files))
    monkeypatch.setattr(vast, 'remote_hashes', lambda name, rd, paths: {p: mine[p] for p in paths})
    with pytest.raises(SystemExit, match='iteration 99, the newest checkpoint is 100'):
        vast.verify_home('box', str(run), '/r')
    assert 'ALLOWED' in ' '.join(vast.verify_home('box', str(run), '/r', allow_opt_mismatch=True))
    torch.save({'iter': 100, 'state': {}}, run / 'models' / 'opt_latest.pt')
    mine['models/opt_latest.pt'] = vast.sha256_file(str(run / 'models' / 'opt_latest.pt'))
    assert vast.verify_home('box', str(run), '/r')[-1] == 'best.pt = iter_0100.pt'
    (run / 'models' / 'best.pt').write_bytes(b'ckpt 099')                 # another checkpoint than index.json's
    mine['models/best.pt'] = vast.sha256_file(str(run / 'models' / 'best.pt'))
    with pytest.raises(SystemExit, match='best.pt is not iter_0100.pt'):
        vast.verify_home('box', str(run), '/r')
    monkeypatch.setattr(vast, 'remote_hashes', lambda name, rd, paths: {p: '0' * 64 for p in paths})
    with pytest.raises(SystemExit, match="is not the box's"):
        vast.verify_home('box', str(run), '/r')
    # a file the box does not have (no evaluation wrote a best.pt there) is not asked for
    (run / 'models' / 'best.pt').write_bytes(b'ckpt 100')
    del box_files['models/best.pt']
    asked = []
    monkeypatch.setattr(vast, 'remote_hashes', lambda name, rd, paths: asked.extend(paths) or {p: mine[p] for p in paths})
    vast.verify_home('box', str(run), '/r')
    assert asked == ['models/opt_latest.pt']


def test_an_unreadable_marker_does_not_end_the_mirror(tmp_path, monkeypatch):
    """B18: ON_VAST.json was rewritten in place, and an unreadable one read as 'run-home has the run' - a sync loop
    looking at that moment ended for good. Now it is written atomically and an unreadable one is not homing."""
    monkeypatch.setattr(vast.time, 'sleep', lambda s: None)
    (tmp_path / vast.ON_VAST).write_text('')
    assert vast.homing(str(tmp_path)) is False
    vast.write_marker(str(tmp_path), {'box': 'vast', 'homing': 1.0})
    assert vast.homing(str(tmp_path)) is True and not list(tmp_path.glob('*.vasttmp'))
    os.remove(tmp_path / vast.ON_VAST)
    assert vast.homing(str(tmp_path)) is True                             # gone: run-home is done with it


def test_a_tar_member_must_be_one_asked_for_inside_the_run(tmp_path):
    run = str(tmp_path)
    asked = {'models/iter_0001.pt'}
    assert vast.member_path(run, 'models/iter_0001.pt', asked) == os.path.join(run, 'models', 'iter_0001.pt')
    with pytest.raises(OSError):
        vast.member_path(run, 'models/other.pt', asked)                   # not asked for
    for bad in ('../x', 'a/../../b', '/etc/passwd', 'C:/x', 'models//x', './models/x', 'a\\..\\b'):
        with pytest.raises(OSError):
            vast.member_path(run, bad, asked | {bad})


def _push_run(tmp_path, cfg, extra=()):
    run = tmp_path / 'run'
    (run / 'models').mkdir(parents=True)
    (run / 'models' / 'iter_0001.pt').write_bytes(b'x')
    (run / 'config.json').write_text(json.dumps(cfg))
    for n in extra:
        (run / n).mkdir()
    return run


def test_run_push_refuses_what_it_does_not_know(tmp_path, monkeypatch):
    """B17: run-push uploaded an allowlist and said nothing about the rest (a co-training run's resolver/, an EoE
    run's games/ and evaluators/, a setting holding a desktop path)."""
    monkeypatch.setattr(vast, 'load_ledger', lambda: {'vast': {'iid': 1, 'state': 'running'}})
    monkeypatch.setattr(vast, 'supervisor_alive', lambda run: None)
    run = _push_run(tmp_path, {'threads': 16, 'pool_worker': 'E:/zero-farm-builds/w.exe',
                               'spec': json.dumps({'evaluator': 'C:\\x\\ev.pt'}), 'preset': 'p/q'},
                    extra=('games', 'replays'))
    (run / 'launch_out.txt').write_text('')
    a = dict(name='vast', run=str(run), include=[], leave=[], clear_setting=[])
    with pytest.raises(SystemExit, match=r'entries run-push does not know: games/\.'):
        vast.cmd_run_push(vast.argparse.Namespace(**a))
    a['leave'] = ['games']
    with pytest.raises(SystemExit, match='desktop path the box does not have: pool_worker=.*spec='):
        vast.cmd_run_push(vast.argparse.Namespace(**a))
    assert not (run / 'PAUSE').exists() and not (run / vast.ON_VAST).exists()     # refused before touching anything
    assert vast.desktop_path_settings({'a': 'C:/x', 'b': 'http://h/p', 'c': 'x:1', 'd': '\\\\srv\\share'}) == ['a', 'd']


def test_a_failed_upload_leaves_the_desktop_copy_as_it_was(tmp_path, monkeypatch):
    """B18: PAUSE and ON_VAST.json were written before the upload, and a failed upload left the desktop run paused
    and "already on a box"."""
    monkeypatch.setattr(vast, 'load_ledger', lambda: {'vast': {'iid': 1, 'state': 'running'}})
    monkeypatch.setattr(vast, 'supervisor_alive', lambda run: None)
    monkeypatch.setattr(vast, 'ssh', lambda *a, **k: _R())
    sent = []

    def upload(name, dest, members):
        sent.append(dict(members))
        if dest.endswith('/run'):
            vast.die('upload to mtgz-vast:/root/runs/run failed')
        return 0
    monkeypatch.setattr(vast, 'tar_upload', upload)
    run = _push_run(tmp_path, {'threads': 16, 'pool_worker': 'E:/w.exe'}, extra=('games',))
    (run / 'games' / 'g.jsonl').write_text('{}\n')
    a = vast.argparse.Namespace(name='vast', run=str(run), include=['games'], leave=[], clear_setting=['pool_worker'])
    with pytest.raises(SystemExit, match='failed'):
        vast.cmd_run_push(a)
    assert not (run / 'PAUSE').exists() and not (run / vast.ON_VAST).exists()
    box_cfg = json.loads(sent[-1]['config.json'])
    assert box_cfg['pool_worker'] == '' and box_cfg['threads'] == 16 and 'games/g.jsonl' in sent[-1]
    (run / 'PAUSE').write_text('someone else')                             # a PAUSE that was there is put back
    with pytest.raises(SystemExit):
        vast.cmd_run_push(a)
    assert (run / 'PAUSE').read_text() == 'someone else'


def test_run_args_split_name_value_flags(tmp_path):
    (tmp_path / 'supervisor_state.json').write_text(json.dumps(
        {'args': ['--eval-remote=mac', '--heldout-shards=C:/x/h.npz', '--games=2000']}))
    assert vast.run_args(str(tmp_path), {}) == ['--heldout-shards', '/root/runs/_ext/h.npz', '--games', '2000']
    assert vast.flag_settings(['--threads', '32', '--games=2000', 'x', '--']) == ['threads', 'games']


def test_the_start_script_keeps_an_extended_deadline():
    """B18: `extend` rewrote the guard's deadline file, and the start script wrote the rent's deadline again at
    every container start: the guard then fired at once on a box whose deadline had been extended."""
    s = vast.onstart_script(1_000_000)
    assert f'[ -s "$G/deadline" ] || echo {1_000_000 + vast.GUARD_MARGIN_S} > "$G/deadline"' in s


def test_each_box_pins_its_host_key(tmp_path, monkeypatch):
    cfg = tmp_path / 'config'
    monkeypatch.setattr(vast, 'SSH_CONFIG', str(cfg))
    monkeypatch.setattr(vast, 'KNOWN_HOSTS_DIR', str(tmp_path))
    monkeypatch.delenv('MTGZ_VAST_HOST', raising=False)
    (tmp_path / 'known_hosts_mtgz-w1').write_text('old box key')
    vast.write_alias('w1', '1.2.3.4', 1111)                               # a new box under the name: learnt afresh
    s = cfg.read_text()
    assert 'StrictHostKeyChecking accept-new' in s and 'UserKnownHostsFile ~/.ssh/known_hosts_mtgz-w1' in s
    assert 'StrictHostKeyChecking no' not in s and '/dev/null' not in s
    assert not (tmp_path / 'known_hosts_mtgz-w1').exists()


def test_status_fails_for_a_box_past_its_deadline(monkeypatch, capsys):
    now = time.time()

    class P:
        def list(self):
            return [{'id': 1, 'status': 'running'}, {'id': 2, 'status': 'running'}]

        def balance(self):
            return 10.0
    monkeypatch.setattr(vast.vp, 'VastCLI', P)
    led = {'ok': {'iid': 1, 'hourly': 1.0, 'created': now - 3600, 'deadline': now + 600, 'state': 'running'},
           'late': {'iid': 2, 'hourly': 1.0, 'created': now - 7200, 'deadline': now - 1200, 'state': 'running'}}
    monkeypatch.setattr(vast, 'load_ledger', lambda: led)
    with pytest.raises(SystemExit) as e:
        vast.cmd_status(None)
    assert e.value.code == 1 and 'late is PAST its deadline' in capsys.readouterr().out
    led['late']['deadline'] = now - 60                                    # within the guard's margin: not yet
    vast.cmd_status(None)


def _load_guard():
    import importlib.util
    spec = importlib.util.spec_from_file_location('_vast_guard', os.path.join(ROOT, 'tools', 'vast_guard.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_the_guard_asks_again_until_the_box_is_destroyed_or_stopped(tmp_path, monkeypatch):
    """B5: at the deadline the guard asked the API once; a 5xx or a timeout on both calls and it exited, the box
    billing on with nothing running."""
    g = _load_guard()
    (tmp_path / 'deadline').write_text('1')
    monkeypatch.setenv('CONTAINER_API_KEY', 'k')
    monkeypatch.setenv('CONTAINER_ID', '42')
    monkeypatch.setattr(g, 'stop_ours', lambda d: {'terminated': 0, 'killed': 0})
    monkeypatch.setattr(g.time, 'sleep', lambda s: None)
    calls = []
    answers = [{'ok': False, 'error': 'HTTP 502'}, {'ok': False, 'error': 'timeout'}] * 2 + [{'ok': True, 'status': 200}]
    monkeypatch.setattr(g, 'api', lambda method, path, body=None: calls.append(method) or answers.pop(0))
    monkeypatch.setattr(g.sys, 'argv', ['vast_guard.py', str(tmp_path)])
    g.main()
    st = json.loads((tmp_path / 'guard.json').read_text())
    assert calls == ['DELETE', 'PUT', 'DELETE', 'PUT', 'DELETE'] and st['attempts'] == 3 and st['destroy']['ok']


def test_the_rent_fails_when_the_guard_is_not_confirmed(monkeypatch):
    """B5: `rent` only warned when the deadline guard was NOT CONFIRMED (exit 0): a script renting boxes carried
    on as if the box were protected."""
    monkeypatch.setattr(vast, 'load_ledger', lambda: {})
    monkeypatch.setattr(vast, 'save_ledger', lambda led: None)
    monkeypatch.setattr(vast, 'write_alias', lambda *a: None)
    offer = {'id': 5, 'cpu': 'Ryzen 9 9950X', 'cores': 32, 'ram_gb': 128, 'geo': 'Texas, US', 'reliability': 0.99,
             'inet_down': 900, 'inet_up': 900}

    class P:
        def create(self, *a):
            return 77

        def destroy(self, iid):
            raise AssertionError('not here')
    monkeypatch.setattr(vast.vp, 'VastCLI', P)
    monkeypatch.setattr(vast.vp, 'hourly', lambda o, disk: 0.5)
    monkeypatch.setattr(vast, 'ranked_offers', lambda *a: [offer])
    monkeypatch.setattr(vast, 'wait_running', lambda p, iid: ({'ssh_routes': [{'kind': 'direct', 'host': 'h', 'port': 1}]},
                                                              None))
    monkeypatch.setattr(vast, 'ssh', lambda name, cmd, **k: _R(0, 'missing' if 'guard.json' in cmd else ''))
    a = vast.argparse.Namespace(name='w1', offer=None, hours=1.0, region='na', max_dph=1.2, min_ram=60, min_cores=16,
                                gpus=1)
    with pytest.raises(SystemExit, match='UP and BILLING, but its deadline guard is not confirmed'):
        vast.cmd_rent(a)
