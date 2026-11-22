"""Judging publishes by default (docs/ENGINE.md 225): tools/ab.py's outside reference (a single-arm experiment judged
against a fixed network; its matches in the Head-to-heads manifest; PUBLISH OFF loud at the judge) and
tools/farm_campaign.py collect's head-to-head posted to the Head-to-heads tab through mtgz.az.site_h2h (checked before
any waiting; --no-publish loud; self-play / unpaired campaigns skipped loudly; a failed post exit 3). No runs, no
games, no network: the uploader is a fake."""
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import ab  # noqa: E402
import farm_campaign as FC  # noqa: E402
from mtgz.az import site_h2h as S  # noqa: E402

D = 'izzet_delver_8016541'


def spec(**kw):
    s = {'id': 'meta3-mirror-ft', 'title': 'Mirror fine-tune', 'description': 'One arm.',
         'source': {'run': '/data/runs/legacy-meta3-r1', 'iter': 1106}, 'iterations': 16,
         'arms': {'mirror': {'lr': 1e-4}}, 'reference': {'path': '/data/runs/delver-bo3-pb1/models/iter_0262.pt',
                                                         'label': 'pb1@262'}}
    s.update(kw)
    return s


# ---------------------------------------------------------------- ab.py: an outside reference

def test_one_arm_with_an_outside_reference_is_an_experiment(tmp_path):
    s = ab.normalize(spec(out=str(tmp_path)))
    assert s['control'] == 'mirror' and s['treatments'] == [] and s['reference']['label'] == 'pb1@262'
    assert ab.judge_matches(s, 1106) == [('mirror_vs_ref', 'mirror', 'ref'), ('mirror_vs_start', 'mirror', 'start')]
    m = ab.manifest(s, 1106, date='2026-11-21T20:00:00-08:00')
    vs_ref = m['matches'][0]
    assert (vs_ref['a_label'], vs_ref['b_label'], vs_ref['key_prefix'], vs_ref['cell_key_prefix']) == (
        'mirror, gen 1122', 'pb1@262', '7-mirror-vs-ref.', '3-mirror-vs-ref.')
    assert os.path.basename(vs_ref['cells']) == 'mirror_vs_ref.json'
    assert 'judged against pb1@262, a fixed network from outside' in m['experiments'][0]['description']
    sent = json.dumps([{k: v for k, v in x.items() if k != 'cells'} for x in m['matches']] + m['experiments'])
    assert '/data/' not in sent and '.pt' not in sent
    # the label defaults to the checkpoint's run and generation; one arm without a reference is refused
    assert ab.reference_of({'path': '/r/delver-bo3-pb1/models/iter_0262.pt'})['label'] == 'delver-bo3-pb1, gen 262'
    with pytest.raises(SystemExit, match='at least two'):
        ab.normalize(spec(reference=None))
    with pytest.raises(SystemExit, match='reference'):
        ab.normalize(spec(reference={'path': 'x.pt', 'gen': 3}))
    # with two arms the treatments still meet the control, and every arm meets the reference
    s2 = ab.normalize(spec(arms={'ctrl': {}, 'mirror': {'lr': 1e-4}}, out=str(tmp_path)))
    assert [k for k, _, _ in ab.judge_matches(s2, 1106)] == ['mirror_vs_ctrl', 'ctrl_vs_ref', 'mirror_vs_ref',
                                                            'ctrl_vs_start', 'mirror_vs_start']


def test_the_command_lines_reference_is_kept_for_a_later_publish(tmp_path):
    s = ab.normalize(spec(reference=None, out=str(tmp_path)), ref_given=True)
    assert s['reference'] is None
    s = ab.with_reference(s, '/r/delver-bo3-pb1/models/iter_0262.pt', 'pb1@262')
    assert s['reference'] == {'path': '/r/delver-bo3-pb1/models/iter_0262.pt', 'label': 'pb1@262'}
    later = ab.with_reference(ab.normalize(spec(reference=None, out=str(tmp_path)), ref_given=True))
    assert later['reference'] == s['reference']                     # (ab_state.json)
    with pytest.raises(SystemExit, match='--ref-label without --ref'):
        ab.with_reference(s, None, 'x')


def test_the_judge_without_publishing_says_so(tmp_path, capsys, monkeypatch):
    runs = tmp_path / 'runs'
    for it in (1106, 1122):
        p = runs / 'meta3-mirror-ft-mirror' / 'models' / f'iter_{it:04d}.pt'
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(b'x')
    ref = tmp_path / 'pb1_iter_0262.pt'
    ref.write_bytes(b'r')
    s = ab.normalize(spec(out=str(tmp_path / 'out'), runs_dir=str(runs), reference={'path': str(ref), 'label': 'pb1@262'}))
    jd = tmp_path / 'out' / 'judge'
    jd.mkdir(parents=True)
    for k in ('mirror_vs_ref', 'mirror_vs_start'):
        (jd / f'{k}.json').write_text(json.dumps({'summary': {'overall': {'elo': -50.0, 'se': 10.0}, 'decks': {}},
                                                  'rows': []}))
    monkeypatch.setattr(ab.subprocess, 'run', lambda *a, **k: pytest.fail('no games: the results are there'))
    assert ab.judge(s, publish=False) == 0
    err = capsys.readouterr().err
    assert 'PUBLISH OFF: h2h.meta3-mirror-ft (--no-publish)' in err and 'ab.py publish SPEC' in err
    assert json.load(open(jd / 'report.json'))['reference'] == {'label': 'pb1@262'}
    s['publish']['h2h'] = False
    assert ab.judge(s) == 0 and '"h2h": false' in capsys.readouterr().err


# ---------------------------------------------------------------- farm_campaign.py collect

def _st(**kw):
    st = {'run': 'cmp-meta3-vs-pb262', 'name': 'meta3-vs-pb262', 'a': '/data/runs/legacy-meta3-r1/models/iter_1234.pt',
          'b': '/data/runs/delver-bo3-pb1/models/iter_0262.pt', 'seats': [{'net': 1}, {'net': 2}], 'paired': True,
          'games': 400, 'job': {'kind': 'play', 'trace': True, 'samples': False}}
    st.update(kw)
    return st


def _opts(*argv):
    import argparse
    ap = S.add_args(argparse.ArgumentParser())
    ap.add_argument('--no-publish', action='store_true')
    return ap.parse_args(list(argv))


@pytest.fixture
def uploader(tmp_path, monkeypatch):
    up = tmp_path / 'upload_h2h.py'
    up.write_text('')
    monkeypatch.setenv(S.UPLOADER_ENV, str(up))
    monkeypatch.setenv(S.KEY_ENV, 'k')
    monkeypatch.setattr(S, 'site_experiment', lambda exp, url=None: None)
    monkeypatch.setattr(S, 'save_dir', lambda: str(tmp_path))
    calls = []
    monkeypatch.setattr(S, 'run_uploader', lambda args, python=None, log=None: calls.append(list(args)) or 0)
    return calls


def test_collect_posts_the_head_to_head_by_default(uploader, capsys):
    o = _opts()
    st = _st()
    assert FC.publish_plan(o, st) is True
    assert (o.publish, o.title) == ('cmp-meta3-vs-pb262', 'Farm campaign meta3-vs-pb262')
    assert o.description.startswith('legacy-meta3-r1, gen 1234 against delver-bo3-pb1, gen 262: 400 paired games')
    readout = {'games': 400, 'a_score': 0.42, 'se': 0.02, 'elo_diff': -55.0, 'elo_se': 14.0,
               'pairs': {'n': 200, 'pentanomial': [40, 0, 100, 0, 60]}, 'select_a': 'cq', 'select_b': 'cq'}
    assert FC.publish_collected(o, st, {'readout': readout}) == 0
    args, = uploader
    saved = json.loads(open(args[0], encoding='utf-8').read())
    assert saved['a'] == st['a'] and saved['b'] == st['b'] and saved['a_score'] == 0.42
    flags = dict(zip(args[1::2], args[2::2]))
    assert (flags['--experiment'], flags['--title'], flags['--a-label'], flags['--b-label'], flags['--key']) == (
        'cmp-meta3-vs-pb262', 'Farm campaign meta3-vs-pb262', 'legacy-meta3-r1, gen 1234', 'delver-bo3-pb1, gen 262',
        'cmp-meta3-vs-pb262')
    # nothing to post after a collect that brought no readout: loud, exit 3
    assert FC.publish_collected(o, st, {'readout': None}) == S.EXIT_PUBLISH_FAILED
    assert 'PUBLISH FAILED' in capsys.readouterr().err


def test_collect_says_off_skipped_and_failed(uploader, capsys, monkeypatch):
    assert FC.publish_plan(_opts('--no-publish'), _st()) is False
    assert 'PUBLISH OFF: campaign meta3-vs-pb262 (h2h.cmp-meta3-vs-pb262)' in capsys.readouterr().err
    assert FC.publish_plan(_opts(), _st(b=None, seats=[{'net': 1}, {'net': 1}])) is False
    assert FC.publish_plan(_opts(), _st(job={'samples': True})) is False
    assert FC.publish_plan(_opts(), _st(paired=False)) is False
    err = capsys.readouterr().err
    assert err.count('PUBLISH SKIPPED') == 3 and 'unpaired' in err
    # one checkpoint, two settings: named by each side's rule
    o = _opts()
    st = _st(b=None, seats=[{'net': 1, 'set': {'infoset_merge': 1}}, {'net': 1}], settings={'eval_sims': 64})
    from zero import farm_eval as FE
    monkeypatch.setattr(FE, 'seat_tags', lambda settings, seats: ['cq-ism', 'cq'])
    assert FC.side_labels(o, st) == ('legacy-meta3-r1, gen 1234 (cq-ism)', 'legacy-meta3-r1, gen 1234 (cq)')
    # no key: refused before any waiting
    monkeypatch.delenv(S.KEY_ENV)
    monkeypatch.setattr(S, 'KEY_FILE', '/nonexistent/key')
    with pytest.raises(SystemExit, match='no upload key'):
        FC.publish_plan(_opts(), _st())
    # the uploader fails: exit 3 with the retry
    monkeypatch.setenv(S.KEY_ENV, 'k')
    monkeypatch.setattr(S, 'run_uploader', lambda args, python=None, log=None: 1)
    o = _opts()
    FC.publish_plan(o, _st())
    assert FC.publish_collected(o, _st(), {'readout': {'a_score': 0.5, 'pairs': {}}}) == S.EXIT_PUBLISH_FAILED
    assert 'farm_campaign.py collect meta3-vs-pb262' in capsys.readouterr().err
