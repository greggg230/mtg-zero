"""tools/ab.py (the A/B harness, docs/ENGINE.md 193) and mtgz.az.site_h2h (results to delverai's Head-to-heads tab):
the spec's defaults and refusals, each arm's settings (the run-owned publisher as a side run), the judge's matches and
the uploader's manifest; the publishing options. No runs, no games, no network (the uploader's dry run when the
delverai checkout is on this machine)."""
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import ab  # noqa: E402
from mtgz.az import publish as P  # noqa: E402
from mtgz.az import site_h2h as S  # noqa: E402

D, Y = 'izzet_delver_8016541', 'doomsday_8027957'


def spec(**kw):
    s = {'id': 'meta3-x', 'title': 'An A/B', 'description': 'One line.',
         'source': {'run': '/data/runs/legacy-meta3-r1', 'iter': 207}, 'iterations': 28,
         'arms': {'ctrl': {'p_full_rule': 'random'}, 'adapt': {'p_full_rule': 'prior_uncertainty'}},
         'labels': {'ctrl': 'Coin', 'adapt': 'Adaptive'}, 'common': {'eval_every': 100000}}
    s.update(kw)
    return s


def test_spec_defaults():
    s = ab.normalize(spec(), base='/data/scratch/x')
    assert s['control'] == 'ctrl' and s['treatments'] == ['adapt']
    assert s['runs_dir'].replace('\\', '/') == '/data/runs' and s['run_prefix'] == 'meta3-x-'
    assert ab.run_dir(s, 'adapt').endswith('/meta3-x-adapt')
    assert os.path.normpath(s['out']) == os.path.normpath('/data/scratch/x')
    assert s['judge']['games'] == 200 and s['judge']['vs_start'] is True
    assert s['publish']['h2h'] and s['publish']['live'] and s['publish']['link'] == 'legacy-meta3-r1'
    s2 = ab.normalize(spec(control='adapt', labels={}))
    assert s2['treatments'] == ['ctrl'] and s2['labels'] == {'ctrl': 'ctrl', 'adapt': 'adapt'}


@pytest.mark.parametrize('kw,msg', [
    ({'common': {'publish': {}}}, 'ab.py sets those'),
    ({'arms': {'a': {'iters': 3}, 'b': {}}}, 'ab.py sets those'),
    ({'arms': {'a': {}}}, 'at least two'),
    ({'id': 'no spaces'}, 'letters, digits'),
    ({'judge': {'games': 201}}, 'even'),
    ({'control': 'zzz'}, 'is no arm'),
    ({'labels': {'ctrl': 'Same', 'adapt': 'Same'}}, 'one label'),
    ({'source': {'run': 'x'}}, 'source'),
])
def test_spec_refusals(kw, msg):
    with pytest.raises(SystemExit, match=msg):
        ab.normalize(spec(**kw))


def test_arm_settings_live_side_run():
    s = ab.normalize(spec(publish={'live_options': {'every': 4}}))
    o = ab.overrides(s, 'adapt', 207)
    assert o['iters'] == 235 and o['p_full_rule'] == 'prior_uncertainty' and o['eval_every'] == 100000
    pub = o['publish']
    assert pub['side'] is True and pub['title'] == 'meta3-x: adapt' and pub['forks'] == [] and pub['every'] == 4
    assert "Arm 'Adaptive'" in pub['note']
    assert P.normalize(pub) == pub
    # the run-owned publisher (ENGINE.md 163) takes it as a side run: --side and the title on its command line
    cmd = P.command('/data/runs/meta3-x-adapt', pub)
    assert '--side' in cmd and cmd[cmd.index('--title') + 1] == 'meta3-x: adapt' and '--forks' not in cmd
    assert ab.overrides(ab.normalize(spec(publish={'live': False})), 'ctrl', 207)['publish'] == {}
    with pytest.raises(SystemExit, match='live_options'):
        ab.overrides(ab.normalize(spec(publish={'live_options': {'every': 0}})), 'ctrl', 207)


def test_judge_matches_and_manifest(tmp_path):
    s = ab.normalize(spec(arms={'ctrl': {}, 'a1': {'x': 1}, 'a2': {'x': 2}}, labels={'ctrl': 'Coin'}, out=str(tmp_path)))
    assert ab.judge_matches(s, 207) == [('a1_vs_ctrl', 'a1', 'ctrl'), ('a2_vs_ctrl', 'a2', 'ctrl'),
                                        ('ctrl_vs_start', 'ctrl', 'start'), ('a1_vs_start', 'a1', 'start'),
                                        ('a2_vs_start', 'a2', 'start')]
    m = ab.manifest(s, 207, {'a1': {'iters': 28, 'games_per_min': 900.0, 'policy_last8': 0.3, 'value_last8': 0.43}},
                    date='2026-11-17T20:00:00-08:00')
    exp = m['experiments'][0]
    assert exp['id'] == 'meta3-x' and exp['run'] == 'legacy-meta3-r1'
    assert exp['description'].startswith('One line. Arms forked from legacy-meta3-r1 at generation 207, 28 training')
    ms = m['matches']
    assert [(x['key_prefix'], x.get('cell_key_prefix'), x.get('overall_only', False)) for x in ms] == [
        ('9-a1-vs-ctrl.', '1-a1-vs-ctrl.', False), ('9-a2-vs-ctrl.', '1-a2-vs-ctrl.', False),
        ('5-ctrl-vs-start.', None, True), ('5-a1-vs-start.', None, True), ('5-a2-vs-start.', None, True)]
    assert [(x['a_label'], x['b_label']) for x in ms[:3]] == [('a1, gen 235', 'Coin, gen 235'),
                                                             ('a2, gen 235', 'Coin, gen 235'),
                                                             ('Coin, gen 235', 'Start, gen 207')]
    assert all(x['date'] == '2026-11-17T20:00:00-08:00' and x['experiment'] == 'meta3-x' for x in ms)
    assert ms[3]['note'].startswith('Both arms start from legacy-meta3-r1 gen 207') and '900.0 self-play games' in ms[3]['note']
    assert os.path.basename(ms[0]['cells']) == 'a1_vs_ctrl.json'
    # only the sources are local files: nothing that goes to the site names a path
    sent = json.dumps([{k: v for k, v in x.items() if k != 'cells'} for x in ms] + m['experiments'])
    assert '/data/' not in sent and ':\\' not in sent and str(tmp_path) not in sent


def test_publishing_options(tmp_path, monkeypatch):
    o, rest = S.pop_args(['a.pt', 'b.pt', '--paired', '--publish', 'exp1', '--title', 'T', '--games', '400',
                          '--a-label', 'A side'])
    assert rest == ['a.pt', 'b.pt', '--paired', '--games', '400']
    assert (o.publish, o.title, o.a_label, o.b_label) == ('exp1', 'T', 'A side', None)
    assert S.checkpoint_label('/data/runs/legacy-meta3-r1/models/iter_0235.pt') == 'legacy-meta3-r1, gen 235'
    assert S.checkpoint_label('C:\\x\\judge\\start_iter_0207.pt') == 'start, gen 207'
    assert S.default_key('/r/meta3-ab-adapt/models/iter_0235.pt', '/r/meta3-ab-ctrl/models/iter_0235.pt') == \
        'meta3-ab-adapt-g235-vs-meta3-ab-ctrl-g235'
    monkeypatch.setenv(S.UPLOADER_ENV, str(tmp_path / 'nowhere.py'))
    with pytest.raises(SystemExit, match='no uploader'):
        S.check(o)
    up = tmp_path / 'upload_h2h.py'
    up.write_text('')
    monkeypatch.setenv(S.UPLOADER_ENV, str(up))
    o2, _ = S.pop_args(['--publish', 'new-exp', '--publish-dry-run'])
    with pytest.raises(SystemExit, match='needs --title'):
        S.check(o2, look_up=False)
    o3, _ = S.pop_args(['--publish', 'bad id!', '--title', 'T'])
    with pytest.raises(SystemExit, match='letters, digits'):
        S.check(o3)
    o4, _ = S.pop_args(['--publish', 'exp', '--title', 'T', '--publish-dry-run'])
    assert S.check(o4, look_up=False).title == 'T'


def _cell(score, base, elo, se, pent):
    return {'units': sum(pent), 'games': 2 * sum(pent), 'score': score, 'base': base, 'elo': elo, 'se': se,
            'pentanomial': pent}


def _cells_result(shift=0.0):
    row = {'kind': 'cells', 'iter': 235, 'ref': 'ctrl', 'sha': 'b' * 12, 'ref_sha': 'a' * 12, 'sims': 32, 'unfinished': 0}
    rows = [dict(row, pair=[D, D], time=1, cells={f'{D}|{D}': _cell(0.52, 0.5, 13.9 + shift, 30.0, [10, 0, 28, 0, 12])}),
            dict(row, pair=[D, Y], time=2, cells={f'{D}|{Y}': _cell(0.82, 0.845, -31.2, 38.3, [2, 0, 14, 0, 34]),
                                                  f'{Y}|{D}': _cell(0.235, 0.195, 41.3, 15.0, [31, 0, 14, 1, 4])}),
            dict(row, pair=[Y, Y], time=3, cells={f'{Y}|{Y}': _cell(0.48, 0.5, -13.9, 31.0, [12, 0, 28, 0, 10])})]
    return {'summary': None, 'rows': rows}


@pytest.mark.skipif(not os.path.isfile(S.uploader_path()), reason="no delverai checkout (the site's uploader)")
def test_publish_dry_run_through_the_uploader(tmp_path, capsys):
    s = ab.normalize(spec(out=str(tmp_path), source={'run': str(tmp_path / 'src'), 'iter': 207}))
    jd = tmp_path / 'judge'
    jd.mkdir()
    for k in ('adapt_vs_ctrl', 'ctrl_vs_start', 'adapt_vs_start'):
        (jd / f'{k}.json').write_text(json.dumps(_cells_result()))
    assert ab.publish_judge(s, dry_run=True) == 0
    out = capsys.readouterr().out
    line = next(x for x in out.splitlines() if '(dry run) h2h record: ' in x)
    rec = json.loads(line.split('(dry run) h2h record: ', 1)[1])
    assert rec['run'] == 'h2h.meta3-x' and rec['kind'] == 'h2h' and rec['link'] == 'src'
    keys = [m['key'] for m in rec['matches']]
    assert keys == ['9-adapt-vs-ctrl.overall', '1-adapt-vs-ctrl.c4.izzet_delver-izzet_delver',
                    '1-adapt-vs-ctrl.c3.izzet_delver-doomsday', '1-adapt-vs-ctrl.c2.doomsday-izzet_delver',
                    '1-adapt-vs-ctrl.c1.doomsday-doomsday', '5-ctrl-vs-start.overall', '5-adapt-vs-start.overall']
    ov = rec['matches'][0]
    assert (ov['a']['label'], ov['b']['label'], ov['elo_diff'], ov['elo_se']) == ('Adaptive, gen 235', 'Coin, gen 235', 2.5, 14.9)
    cell = rec['matches'][3]          # Doomsday into Delver: a 23.5% score, a +41.3 gain
    assert (cell['a_score'], cell['elo_diff'], cell['elo_se']) == (0.235, 41.3, 15.0) and 'GAIN' in cell['note']
    assert str(tmp_path) not in line


def test_per_arm_code(tmp_path):
    s = ab.normalize(spec(code={'worktree': '/wt/main', 'sha': 'aaaa', 'python': '/py',
                                'arms': {'ctrl': {'worktree': '/wt/old', 'sha': 'bbbb'}}}))
    assert ab.worktree(s, 'ctrl') == '/wt/old' and ab.code_sha(s, 'ctrl') == 'bbbb' and ab.python(s, 'ctrl') == '/py'
    assert ab.worktree(s, 'adapt') == '/wt/main' and ab.code_sha(s, 'adapt') == 'aaaa' and ab.worktree(s) == '/wt/main'
    for bad in ({'zzz': {'worktree': '/x'}}, {'ctrl': {'branch': 'x'}}, {'ctrl': '/x'}):
        with pytest.raises(SystemExit, match='code.arms'):
            ab.normalize(spec(code={'arms': bad}))
    assert ab.normalize(spec())['farm'] is None
    assert ab.normalize(spec(farm={}))['farm'] == {'gates': True, 'pause_when_done': True}


def test_farm_arms(tmp_path):
    assert ab.farm_run({'selfplay_remote': 'mac,pool:bab-fix'}) == 'bab-fix'
    assert ab.farm_run({'selfplay_remote': 'mac'}) is None and ab.farm_run({}) is None
    s = ab.normalize(spec(out=str(tmp_path / 'out'), runs_dir=str(tmp_path), farm={}))
    d = tmp_path / 'meta3-x-adapt'
    d.mkdir()
    (d / 'config.json').write_text(json.dumps({'selfplay_remote': 'pool:meta3-x-adapt', 'pool_worker': 'E:/b/w.exe'}))
    assert 'no passed farm gates' in ab.farm_gate_missing(s, 'adapt')
    g = tmp_path / 'out' / 'farm_gates' / 'adapt'
    g.mkdir(parents=True)
    (g / 'summary.json').write_text(json.dumps({'passed': True, 'worker': 'E:/b/other.exe'}))
    assert 'not the arm\'s pool_worker' in ab.farm_gate_missing(s, 'adapt')
    (g / 'summary.json').write_text(json.dumps({'passed': True, 'worker': 'E:\\b\\w.exe'}))
    assert 'check-config' in ab.farm_gate_missing(s, 'adapt')
    (g / 'check_config.json').write_text(json.dumps({'worker': 'E:/b/w.exe', 'failures': ['game_records']}))
    assert "['game_records']" in ab.farm_gate_missing(s, 'adapt')
    (g / 'check_config.json').write_text(json.dumps({'worker': 'E:/b/w.exe', 'failures': []}))
    assert ab.farm_gate_missing(s, 'adapt') is None and ab.farm_gate_missing(s, 'ctrl') is None
    assert ab.farm_gate_missing(ab.normalize(spec(out=str(tmp_path / 'out'), runs_dir=str(tmp_path), farm={'gates': False})),
                                'adapt') is None
    assert 'needs "farm"' in ab.farm_gate_missing(ab.normalize(spec(runs_dir=str(tmp_path))), 'adapt')
    assert ab.farm_run(ab.overrides(ab.normalize(spec(common={'selfplay_remote': 'pool:x'})), 'ctrl', 207)) == 'x'

    class Client:
        def __init__(self):
            self.calls = []

        def call(self, method, path, body=None):
            self.calls.append((method, path, body))
            return {'id': 'meta3-x-adapt', 'state': 'active', 'build': 'b1',
                    'alternates': [{'build_id': 'b2', 'problems': ['x'], 'when': {'vendor': 'nvidia'}}]}

    c = Client()
    assert ab.farm_pause(s, 'adapt', client=c) == 'meta3-x-adapt'
    (m1, p1, _), (m2, p2, body) = c.calls
    assert (m1, p1, m2, p2) == ('GET', '/v1/admin/run/meta3-x-adapt', 'PUT', '/v1/admin/run/meta3-x-adapt')
    assert body['state'] == 'paused' and body['build'] == 'b1'
    assert body['alternates'] == [{'build_id': 'b2', 'when': {'vendor': 'nvidia'}}]
    assert ab.farm_pause(s, 'ctrl', client=c) is None and len(c.calls) == 2
