"""The run's own delverai publisher (docs/ENGINE.md 163): the run setting `publish` (on in the delver-mirror*
presets, off in the smokes; --publish / --no-publish override it and are saved), the supervisor's companion command
made of it (refused loudly without an upload key), a fork's (a side run, its fork record), the publisher's exit
paths (finished: a final pass then 'finished'; stopped, refused, its supervisor gone: 'stopped'; paused: it stays),
and the real supervisor starting the real publisher (dry run) once for a run that crashes once and then finishes."""
import json
import os
import subprocess
import sys
import threading
import time
import types

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az as A  # noqa: E402
from mtgz.az import publish as PUB  # noqa: E402
from zero import runctl  # noqa: E402


# ---------------------------------------------------------------- the setting

def test_the_delver_presets_publish_and_the_smokes_do_not():
    for name in ('delver-mirror', 'delver-mirror-bo3', 'delver-mirror-bo3-dealt'):
        assert A.PRESETS[name]['publish'] == PUB.DELVER_PUBLISH
    for name in ('delver-smoke', 'delver-bo3-smoke', 'mtg-smoke', 'mtg-3', 'eoe-1'):
        assert not A.PRESETS[name].get('publish')
    assert A.PACKAGE.settings['publish'] == {}
    assert PUB.DELVER_PUBLISH == {'side': False, 'every': 8, 'games': 6, 'census': 200, 'device': 'cpu', 'threads': 4,
                                  'forks': ['~/delverai/forks.json']}


def _cfg(argv, saved=None):
    from mtgz.az import run as R           # (zero.run)
    return R.resolve_config(R.parse(['--run', 'x'] + PUB.no_publish(argv)), dict(saved or {}))


def test_flags_override_the_preset_and_are_what_is_saved():
    assert _cfg(['--preset', 'delver-mirror-bo3-dealt'])['publish'] == PUB.DELVER_PUBLISH
    # a run that saved no publish setting (pb2, started before it existed) takes its preset's at its next start
    assert _cfg([], {'preset': 'delver-mirror-bo3-dealt', 'iters': 1000})['publish'] == PUB.DELVER_PUBLISH
    over = _cfg(['--preset', 'delver-mirror-bo3', '--publish', '{"title": "Run 2", "every": 4}'])
    assert over['publish'] == {'title': 'Run 2', 'every': 4}
    off = _cfg(['--preset', 'delver-mirror-bo3', '--no-publish'])
    # (the preset's yardsticks are still on: their watcher alone, docs/ENGINE.md 164)
    assert off['publish'] == {} and A.PACKAGE.companion_of('/r', off)['name'] == 'yardsticks'
    assert A.PACKAGE.companion_of('/r', dict(off, yardsticks={})) is None
    # saved: config.json holds it, so the next start without a flag keeps it (off stays off, a dict stays)
    assert _cfg([], {'preset': 'delver-mirror-bo3', 'publish': {}})['publish'] == {}
    assert _cfg([], {'preset': 'delver-mirror-bo3', 'publish': {'title': 'Run 2'}})['publish'] == {'title': 'Run 2'}
    assert _cfg(['--preset', 'delver-bo3-smoke'])['publish'] == {}
    assert PUB.no_publish(['--iters', '3', '--no-publish']) == ['--iters', '3', '--publish', '{}']


def test_a_bad_publish_setting_is_refused():
    for bad, why in (({'tilte': 'x'}, 'unknown option'), ({'every': 'eight'}, 'right type'), ({'side': 1}, 'right type'),
                     ({'every': 0}, '>= 1'), ([1], 'JSON object'), ({'forks': [3]}, 'paths'), ({'games': True}, 'type')):
        with pytest.raises(ValueError, match=why):
            PUB.normalize(bad)
    assert PUB.normalize(None) is None and PUB.normalize({}) is None


# ---------------------------------------------------------------- the companion command

def test_the_companion_is_the_publisher_from_this_checkout(tmp_path, monkeypatch):
    key = tmp_path / 'key'
    key.write_text('k')
    cfg = {'publish': dict(PUB.DELVER_PUBLISH, title='Best of three: run 2', key_file=str(key), keep_yardstick='E:/k')}
    spec = A.PACKAGE.companion_of('E:/runs/pb2', cfg)
    assert spec['name'] == 'publisher' and spec['log'] == 'publisher_console.txt'
    cmd = spec['cmd']
    assert cmd[1] == os.path.join(ROOT, 'tools', 'delverai_publish.py') and cmd[2] == 'E:/runs/pb2'
    flags = ' '.join(cmd[3:])
    for part in ('--loop 300', '--supervised', '--title Best of three: run 2', '--every 8', '--games 6', '--census 200',
                 '--device cpu', '--threads 4', '--keep-yardstick E:/k', f'--key-file {key}',
                 f"--forks {os.path.expanduser('~/delverai/forks.json')}"):
        assert part in flags, part
    assert '--side' not in cmd and '--dry-run' not in cmd
    side = A.PACKAGE.companion_of('/r', {'publish': {'side': True, 'dry_run': True, 'loop': 5}})['cmd']
    assert side[3:] == ['--loop', '5', '--supervised', '--side', '--dry-run']


def test_no_upload_key_refuses_the_companion(tmp_path, monkeypatch):
    monkeypatch.delenv(PUB.KEY_ENV, raising=False)
    monkeypatch.setattr(PUB, 'KEY_FILE', str(tmp_path / 'none'))
    with pytest.raises(ValueError, match='no upload key'):
        A.PACKAGE.companion_of('/r', {'publish': {'title': 'x'}})
    with pytest.raises(ValueError, match='no such file'):
        A.PACKAGE.companion_of('/r', {'publish': {'key_file': str(tmp_path / 'nokey')}})
    monkeypatch.setenv(PUB.KEY_ENV, 'k')
    assert A.PACKAGE.companion_of('/r', {'publish': {'title': 'x'}})['name'] == 'publisher'


# ---------------------------------------------------------------- a fork

def test_a_fork_publishes_as_a_side_run_and_records_its_lineage(tmp_path):
    forks = tmp_path / 'forks.json'
    forks.write_text(json.dumps({'about': 'x', 'done': [{'name': 'old', 'k': 1, 'raw': {'a': 'A/models/iter_0001.pt'}}]}))
    src, dest = str(tmp_path / 'pb2'), str(tmp_path / 'pb2-fork')
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'publish': dict(PUB.DELVER_PUBLISH, title='Run 2', note='n',
                                                                forks=[str(forks)])}
    PUB.on_fork(src, dest, 300, cfg, {'lr': 5e-5})
    assert cfg['publish']['side'] is True and cfg['publish']['title'] == 'pb2-fork: a fork of pb2 at 300'
    assert 'note' not in cfg['publish'] and cfg['publish']['every'] == 8
    done = json.loads(forks.read_text())['done']
    assert len(done) == 2 and done[0]['name'] == 'old'
    rec = done[1]
    assert rec['name'] == 'pb2-fork' and rec['k'] == 300 and rec['flag'] == '--lr 5e-05'
    assert rec['main_before'] == rec['main_after'] == src.replace('\\', '/')
    assert rec['raw']['a'] == dest.replace('\\', '/') + '/models/iter_0300.pt'
    # the publisher reads it as a fork of the source at 300
    import delverai_publish as P
    reg = P.fork_registry([str(forks)])
    assert reg[P._norm(dest)] == {'parent': src.replace('\\', '/'), 'at': 300, 'label': 'lr 5e-05', 'won': False}
    # the same fork again: no second record; --set publish=...: the fork's own choice kept, named for the fork
    # (for_run, docs/ENGINE.md 221)
    cfg2 = {'publish': {'title': 'The new main', 'side': False, 'forks': [str(forks)]}}
    PUB.on_fork(src, dest, 300, cfg2, {'publish': cfg2['publish']})
    assert cfg2['publish'] == {'title': 'The new main', 'side': False, 'forks': [str(forks)], 'for_run': 'pb2-fork'}
    assert len(json.loads(forks.read_text())['done']) == 2


def test_a_fork_of_a_run_that_saved_no_publish_setting_takes_its_presets(tmp_path, monkeypatch):
    forks = tmp_path / 'f' / 'forks.json'
    monkeypatch.setitem(A.PRESETS['delver-mirror-bo3'], 'publish', dict(PUB.DELVER_PUBLISH, forks=str(forks)))
    cfg = {'preset': 'delver-mirror-bo3'}
    PUB.on_fork(str(tmp_path / 'a'), str(tmp_path / 'b'), 7, cfg, {})
    assert cfg['publish']['side'] is True and json.loads(forks.read_text())['done'][0]['k'] == 7
    quiet = {'preset': 'delver-bo3-smoke'}
    PUB.on_fork(str(tmp_path / 'a'), str(tmp_path / 'c'), 7, quiet, {})
    assert 'publish' not in quiet and len(json.loads(forks.read_text())['done']) == 1


# ---------------------------------------------------------------- the publisher's exit paths

def _args(run, **kw):
    return types.SimpleNamespace(**dict(dict(run=run, every=1, follow=None, forks=[], title=None, note=None, side=False,
                                             record_vs=None, state_dir=None, keep_yardstick=None, dry_run=True,
                                             min_iter=None, games=1, census=0), **kw))


@pytest.fixture
def P(monkeypatch):
    import delverai_publish as P
    monkeypatch.setattr(P, 'END', {})
    return P


def _supervised(monkeypatch, run, pid=None, started=None):
    pid = os.getpid() if pid is None else pid
    monkeypatch.setenv('ZERO_COMPANION_RUN', str(run))
    monkeypatch.setenv('ZERO_SUPERVISOR_PID', str(pid))
    import psutil
    monkeypatch.setenv('ZERO_SUPERVISOR_STARTED', repr(started if started is not None else psutil.Process(pid).create_time()))


def _watch(P, a, posts, exits, poll=0.01):
    finish = threading.Event()
    th = threading.Thread(target=P.end_watch, args=(a, None, lambda m: None, finish, poll, exits.append), daemon=True)
    th.start()
    return finish, th


def test_paused_keeps_the_publisher_then_finished_starts_the_final_pass(tmp_path, monkeypatch, P):
    run = tmp_path / 'r'
    run.mkdir()
    _supervised(monkeypatch, run)
    posts, exits = [], []
    monkeypatch.setattr(P, 'post_progress', lambda run, a, key, log: posts.append(dict(P.END)))
    runctl.write_state(str(run), 'supervisor', state='paused', paused_since=time.time())
    finish, th = _watch(P, _args(str(run)), posts, exits)
    time.sleep(0.3)
    assert th.is_alive() and not finish.is_set() and not posts and not exits      # paused: it stays
    runctl.write_state(str(run), 'supervisor', state='done', ended='finished')
    th.join(5)
    assert finish.is_set() and not exits and not posts


@pytest.mark.parametrize('ended,detail', [('stopped', 'STOP file'), ('refused', 'Refused'), ('gave_up', 'same crash')])
def test_stopped_refused_or_gave_up_posts_stopped_and_exits(tmp_path, monkeypatch, P, ended, detail):
    run = tmp_path / 'r'
    run.mkdir()
    _supervised(monkeypatch, run)
    posts, exits = [], []
    monkeypatch.setattr(P, 'post_progress', lambda run, a, key, log: posts.append(dict(P.END)))
    runctl.write_state(str(run), 'supervisor', state='refused' if ended != 'stopped' else 'done', ended=ended)
    finish, th = _watch(P, _args(str(run)), posts, exits)
    th.join(5)
    assert exits == [0] and not finish.is_set()
    assert posts[0]['state'] == 'stopped' and detail in posts[0]['detail'] and posts[0]['paused_since'] is None


def test_its_supervisor_gone_posts_stopped_and_exits(tmp_path, monkeypatch, P):
    run = tmp_path / 'r'
    run.mkdir()
    dead = subprocess.Popen([sys.executable, '-c', 'pass'])
    dead.wait()
    _supervised(monkeypatch, run, pid=dead.pid, started=1.0)
    (run / 'supervisor_state.json').write_text(json.dumps({'pid': dead.pid, 'started': 1.0, 'state': 'running'}))
    posts, exits = [], []
    monkeypatch.setattr(P, 'post_progress', lambda run, a, key, log: posts.append(dict(P.END)))
    _, th = _watch(P, _args(str(run)), posts, exits)
    th.join(5)
    assert exits == [0] and posts[0]['state'] == 'stopped'


def test_the_final_pass_records_the_last_checkpoint_at_once_and_posts_finished(tmp_path, monkeypatch, P):
    run = tmp_path / 'r'
    (run / 'models').mkdir(parents=True)
    (run / 'config.json').write_text(json.dumps({'iters': 2}))
    (run / 'models' / 'iter_0002.pt').write_bytes(b'x')        # written a moment ago: not settled
    assert P.latest_checkpoint(str(run), 1) is None
    passes, posts = [], []
    monkeypatch.setattr(P, 'one_pass', lambda run, a, key, log, kick, settle_s: passes.append(
        (P.latest_checkpoint(run, a.every, settle_s), dict(P.END))))
    monkeypatch.setattr(P, 'post_progress', lambda run, a, key, log: posts.append(dict(P.END)))
    assert P.final_pass(_args(str(run)), None, lambda m: None) == 0
    (ck, end), = passes
    assert ck[0] == 2 and end['state'] == 'finished' and 'its last iteration (2)' in end['detail']
    assert posts == [end]

    def boom(*a, **k):
        raise RuntimeError('recording failed')
    monkeypatch.setattr(P, 'one_pass', boom)
    posts.clear()
    assert P.final_pass(_args(str(run)), None, lambda m: None) == 1 and posts[0]['state'] == 'finished'


def test_every_record_says_when_the_run_started(tmp_path, P):
    run = tmp_path / 'r'
    run.mkdir()
    (run / 'config.json').write_text('{}')
    t = P.run_started(str(run))
    assert t is None or abs(t - time.time()) < 120
    assert P.progress(str(run))['started'] == t
    assert P.run_started(str(tmp_path / 'missing')) is None


# ---------------------------------------------------------------- the real supervisor and the real publisher

FAKE_RUN = '''import json, os, sys, time
sys.path.insert(0, os.environ["MTGZ_ROOT"])
import mtgz.az
from zero import runctl
run = sys.argv[sys.argv.index("--run") + 1]
n = os.path.join(run, "starts")
k = len(open(n).read()) if os.path.exists(n) else 0
open(n, "a").write("x")
json.dump({"preset": "delver-bo3-smoke", "iters": 2,
           "publish": {"dry_run": True, "loop": 1, "progress_every": 1, "every": 1, "games": 1, "census": 0}},
          open(os.path.join(run, "config.json"), "w"))
runctl.write_state(run, "run", state="running")
time.sleep(float(os.environ["FAKE_RUN_S"]))
if k == 0:
    sys.exit(1)
runctl.write_state(run, "run", state="done")
'''


@pytest.mark.live_run_hazard
def test_the_supervisor_starts_one_publisher_per_run_and_it_ends_finished(tmp_path):
    run = tmp_path / 'run'
    run.mkdir()
    fake = tmp_path / 'fake_run.py'
    fake.write_text(FAKE_RUN)
    env = dict(os.environ, MTGZ_SUPERVISE_WAIT='0.5', MTGZ_SUPERVISE_COMPANION_POLL='0.2', MTGZ_ROOT=ROOT,
               FAKE_RUN_S='12', MTGZ_SUPERVISE_CMD=json.dumps([sys.executable, str(fake)]),
               MTGZ_SUPERVISE_COMPANION_WAIT='120')
    env.pop('MTGZ_SUPERVISE_COMPANION', None)
    sup = subprocess.run([sys.executable, os.path.join(ROOT, 'tools', 'supervise.py'), str(run)], cwd=ROOT, env=env,
                         timeout=300, capture_output=True, text=True)
    assert sup.returncode == 0, sup.stderr
    log = (run / 'supervisor.log').read_text(encoding='utf-8')
    assert (run / 'starts').read_text() == 'xx'
    assert log.count('companion publisher started') == 1, log
    assert 'clean exit (finished)' in log and 'companion publisher ended with code 0' in log, log
    console = (run / 'publisher_console.txt').read_text(encoding='utf-8')
    assert console.count('supervised: publishing') == 1, console
    assert 'the run finished: the final pass follows' in console and 'publisher exiting (0)' in console, console
    assert '"state": "finished"' in console, console
    st = runctl.read_state(str(run), 'supervisor')
    assert st['ended'] == 'finished' and st['companion'] is None


# ---------------------------------------------------------------- metagame runs publish by default (docs/ENGINE.md 221)

def test_the_metagame_presets_publish_with_the_r1_publishers_options():
    for name in ('legacy-meta3', 'legacy-meta3-ddwheel'):
        assert A.PRESETS[name]['publish'] == PUB.META3_PUBLISH
    for name in ('legacy-meta3-smoke', 'legacy-meta3-ddwheel-smoke'):
        assert not A.PRESETS[name].get('publish')
    assert PUB.META3_PUBLISH == {'side': False, 'every': 8, 'games': 9, 'combo_every': 0, 'census': 0, 'device': 'cpu',
                                 'threads': 4, 'forks': []}
    assert _cfg(['--preset', 'legacy-meta3-ddwheel'])['publish'] == PUB.META3_PUBLISH


def test_combo_every_is_a_publish_option():
    assert PUB.normalize({'combo_every': 0}) == {'combo_every': 0}
    cmd = PUB.command('/r', {'combo_every': 0, 'every': 8})
    assert cmd[cmd.index('--combo-every') + 1] == '0'
    for bad in ({'combo_every': -1}, {'combo_every': 'none'}, {'combo_every': True}):
        with pytest.raises(ValueError):
            PUB.normalize(bad)


def test_a_run_that_will_not_publish_says_so_loudly(tmp_path, capsys):
    run = tmp_path / 'legacy-meta3-x'
    run.mkdir()
    assert PUB.companion(str(run), {'publish': {}}) is None
    assert 'PUBLISH OFF: legacy-meta3-x will not appear' in capsys.readouterr().err
    assert 'PUBLISH OFF' in (run / 'supervisor.log').read_text(encoding='utf-8')


def test_a_publish_setting_copied_from_another_run_publishes_as_a_side_run(tmp_path, capsys):
    pub = {'title': 'Three decks (run 1)', 'note': 'n', 'side': False, 'for_run': 'legacy-meta3-r1',
           'state_dir': 'E:/somewhere', 'dry_run': True}
    own = tmp_path / 'legacy-meta3-r1'
    own.mkdir()
    cmd = PUB.companion(str(own), {'publish': pub})['cmd']
    assert '--side' not in cmd and cmd[cmd.index('--title') + 1] == 'Three decks (run 1)'
    assert 'ANOTHER RUN' not in capsys.readouterr().err
    copy = tmp_path / 'legacy-meta3-copy'
    copy.mkdir()
    cmd = PUB.companion(str(copy), {'publish': pub})['cmd']
    assert '--side' in cmd and cmd[cmd.index('--title') + 1] == 'legacy-meta3-copy'
    assert '--note' not in cmd and '--state-dir' not in cmd
    assert 'made for legacy-meta3-r1, this run is legacy-meta3-copy' in capsys.readouterr().err


def test_a_fork_of_an_unpublished_run_says_it_will_not_publish(tmp_path, capsys):
    cfg = {'preset': 'legacy-meta3-ddwheel', 'publish': {}}
    PUB.on_fork(str(tmp_path / 'r1'), str(tmp_path / 'r1-exp'), 50, cfg, {})
    assert cfg['publish'] == {}
    assert 'PUBLISH OFF - r1-exp will NOT appear' in capsys.readouterr().err


def test_a_fork_of_a_metagame_run_is_a_side_run_made_for_itself(tmp_path, capsys):
    cfg = {'preset': 'legacy-meta3-ddwheel',
           'publish': dict(PUB.META3_PUBLISH, title='Three decks (run 1)', for_run='r1', state_dir='E:/s')}
    PUB.on_fork(str(tmp_path / 'r1'), str(tmp_path / 'r1-exp'), 50, cfg, {})
    p = cfg['publish']
    assert p['side'] is True and p['title'] == 'r1-exp: a fork of r1 at 50' and p['for_run'] == 'r1-exp'
    assert 'state_dir' not in p and p['combo_every'] == 0
    err = capsys.readouterr().err
    assert 'names no forks file' not in err and 'PUBLISH OFF' not in err
