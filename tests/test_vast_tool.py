"""tools/vast.py (docs/ENGINE.md 97), the parts that need no rented box: the run arguments a box run gets
(remote settings dropped, desktop paths remapped), the managed ssh-config blocks, the homing marker a sync loop
stops at, the deadline guard's start script, and the 'vast' remote spec of the game package."""
import json
import os
import sys

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
