"""Reference matches on delverai by default (docs/ENGINE.md 225; mtgz.az.site_refmatch): which cells references are
from outside the run, the record `refmatch.<run>` rebuilt from every row on disk (mirror cells of a registered network
only, the newest row of a generation, delverai's upload_refmatch.py's fields), a record with no row refused, and the
three ways it is posted - `run_yardsticks.py one` by hand (default on; --no-publish loud; refused before any game when
the post could not work), the watcher after a result, `run_yardsticks.py refmatch` (adopting a sidecar's rows). The
site's POST is a fake: nothing here touches the network."""
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from mtgz.az import publish as P  # noqa: E402
from mtgz.az import site_refmatch as RM  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402
import run_yardsticks as RY  # noqa: E402

D, Z = 'izzet_delver_8016541', 'doomsday_8027957'


def _cell(score, elo, se, pent):
    return {'units': sum(pent), 'games': 2 * sum(pent), 'score': score, 'base': 0.5, 'elo': elo, 'se': se,
            'pentanomial': pent}


def _row(it, ref='pb262', sha='539c1d5fb979', pair=(D, D), t=1000, elo=-199.1, score=0.2412, **kw):
    cells = {f'{pair[0]}|{pair[1]}': _cell(score, elo, 14.0, [227, 0, 153, 0, 20])}
    if pair[0] != pair[1]:
        cells[f'{pair[1]}|{pair[0]}'] = _cell(0.5, 0.0, 10.0, [100, 0, 200, 0, 100])
    r = {'kind': 'cells', 'iter': it, 'ref': ref, 'ref_sha': sha, 'sha': 'b' * 12, 'pair': list(pair), 'cells': cells,
         'games': 800, 'unfinished': 0, 'sims': 64, 'time': t}
    r.update(kw)
    return r


def _run(tmp_path, name='legacy-meta3-mirror-snp', refs=None):
    run = tmp_path / 'runs' / name
    (run / 'models').mkdir(parents=True)
    (run / 'models' / 'iter_0000.pt').write_bytes(b'gen0')
    (run / 'models' / 'iter_0016.pt').write_bytes(b'gen16')
    outside = tmp_path / 'elsewhere' / 'delver-bo3-pb1' / 'models' / 'iter_0262.pt'
    outside.parent.mkdir(parents=True)
    outside.write_bytes(b'pb262 network')
    ys = {'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_decks': [D, Z],
          'cells_refs': refs if refs is not None else ['iter0', f'pb262={outside.as_posix()}']}
    (run / 'config.json').write_text(json.dumps({'yardsticks': ys}))
    return str(run), str(outside), ys


@pytest.fixture
def posts(monkeypatch):
    sent = []

    def fake_post(url, body, key, timeout=60):
        sent.append((url, json.loads(body.decode('utf-8')), key))
        return {'ok': True}
    monkeypatch.setattr(P, 'post', fake_post)
    monkeypatch.setenv(P.KEY_ENV, 'test-key')
    return sent


# ---------------------------------------------------------------- outside references

def test_only_a_network_from_outside_the_run_is_an_outside_reference(tmp_path):
    run, outside, ys = _run(tmp_path)
    inside = os.path.join(run, 'models', 'iter_0016.pt')
    ys = Y.normalize(dict(ys, cells_refs=['iter0', f'pb262={outside}', f'own16={inside}']))
    cfg = Y.cfg_of(run)
    assert RM.outside_ref(run, cfg, ys, 'pb262')
    assert not RM.outside_ref(run, cfg, ys, 'gen0')           # generation 0 of the run
    assert not RM.outside_ref(run, cfg, ys, 'own16')          # a file of the run
    assert not RM.outside_ref(run, cfg, ys, 'iter_0016')      # the gen-over-gen kind
    assert not RM.outside_ref(run, cfg, ys, 'nope')
    frozen = Y.normalize(dict(ys, cells_refs=[{'label': 'f1', 'path': outside, 'iter': 16, 'since': 0}]))
    assert not RM.outside_ref(run, cfg, frozen, 'f1')          # frozen from the run by cells-freeze
    assert Y.normalize(dict(ys))['refmatch_publish'] is True


# ---------------------------------------------------------------- the record

def test_the_record_is_rebuilt_from_every_mirror_row_of_the_registered_network(tmp_path):
    run, outside, _ = _run(tmp_path)
    meta = RM.register(RM.read_meta(run), 'pb262', '539c1d5fb9795cc0', outside)
    assert meta['refs']['pb262'] == {'sha': '539c1d5fb979', 'ref': {'run': 'delver-bo3-pb1', 'gen': 262}}
    rows = [_row(1106, t=1), _row(1122, t=2, elo=-120.0, score=0.33),
            _row(1106, t=3, elo=-150.0, score=0.30),                     # newer row of 1106: wins
            _row(1138, pair=(D, Z), t=4),                                # no mirror: not in the record
            _row(1154, sha='0123456789ab', t=5),                         # another network under the label
            _row(1170, ref='gen0', t=6),                                 # an unregistered reference
            dict(_row(1186, t=7), error='boom')]
    logs = []
    rec = RM.record(run, meta, RM.read_rows([]) + [r for r in rows if r.get('error') is None], logs.append, now=5.0)
    assert (rec['run'], rec['kind'], rec['for_run'], rec['updated'], rec['iters']) == (
        'refmatch.legacy-meta3-mirror-snp', 'refmatch', 'legacy-meta3-mirror-snp', 5.0, [])
    assert 'marks' not in rec and len(rec['series']) == 1
    s = rec['series'][0]
    assert list(s) == ['key', 'label', 'title', 'note', 'ref', 'deck', 'deck_label', 'format', 'games', 'sims', 'rows']
    assert (s['key'], s['label'], s['deck'], s['deck_label'], s['format'], s['games'], s['sims']) == (
        'pb262', 'delver-bo3-pb1@262', D, 'Izzet Delver', 'bo1', 800, 64)
    assert s['title'] == 'Izzet Delver mirror against delver-bo3-pb1@262' and '800 games a point' in s['note']
    assert [(r['iter'], r['elo'], r['score']) for r in s['rows']] == [(1106, -150.0, 0.3), (1122, -120.0, 0.33)]
    assert s['rows'][0] == {'iter': 1106, 'games': 800, 'score': 0.3, 'se': round(RM.pentanomial_se([227, 0, 153, 0, 20]), 4),
                            'elo': -150.0, 'elo_se': 14.0, 'unfinished': 0}
    assert any('another network' in m for m in logs)
    # the human fields given once are kept; marks and row notes
    RM.apply_fields(meta, 'pb262', {'label': 'pb1@262', 'title': 'Delver mirror against pb1@262', 'note': 'A note.',
                                    'ref_run': 'delver-bo3-pb1', 'ref_gen': 262, 'marks': [(1234, 'shrink-and-perturb')],
                                    'row_notes': [(1106, 'the fork')]})
    rec = RM.record(run, meta, rows[:3], logs.append)
    s = rec['series'][0]
    assert (s['label'], s['title'], s['note'], s['ref']) == ('pb1@262', 'Delver mirror against pb1@262', 'A note.',
                                                            {'run': 'delver-bo3-pb1', 'gen': 262})
    assert rec['marks'] == [{'iter': 1234, 'label': 'shrink-and-perturb'}] and s['rows'][0]['note'] == 'the fork'


def test_no_row_and_what_the_site_refuses_fail_loudly(tmp_path):
    run, outside, _ = _run(tmp_path)
    meta = RM.register(RM.read_meta(run), 'pb262', '539c1d5fb979', outside)
    with pytest.raises(RM.RefmatchError, match='no row to publish'):
        RM.record(run, meta, [_row(1138, pair=(D, Z))])
    with pytest.raises(RM.RefmatchError, match='local file path'):
        RM.record(run, RM.apply_fields(meta, 'pb262', {'note': 'see E:/x/iter_0262.pt'}), [_row(1)])
    with pytest.raises(RM.RefmatchError, match='another network|registered for network'):
        RM.register(meta, 'pb262', 'ffffffffffff')
    with pytest.raises(RM.RefmatchError, match='ITER=TEXT'):
        RM.parse_pairs(['x=1'], '--mark')


def test_publish_posts_the_whole_record_with_the_key(tmp_path, posts):
    run, outside, _ = _run(tmp_path)
    RM.save_meta(run, RM.register(RM.read_meta(run), 'pb262', '539c1d5fb979', outside))
    side = tmp_path / 'sidecar_rows.jsonl'
    side.write_text(json.dumps(_row(1234)) + '\n' + '{"torn')
    Y.append_row(run, _row(1250, t=2))
    rec = RM.publish(run, extra=[str(side)], log=lambda m: None)
    (url, body, key), = posts
    assert url == 'https://delverai.greggg230.com/api/progress/upload' and key == 'test-key' and body == rec
    assert [r['iter'] for r in body['series'][0]['rows']] == [1234, 1250]
    assert RM.publish(run, extra=[str(side)], dry_run=True, log=lambda m: None)['run'] == rec['run'] and len(posts) == 1


# ---------------------------------------------------------------- `one` by hand

def _one(run, ys, *extra):
    return RY.main(['one', run, '--kind', 'cells', '--iter', '16', '--ref', 'pb262', '--pair', f'{D},{D}',
                    '--yardsticks', json.dumps(ys)] + list(extra))


def test_one_against_an_outside_reference_posts_by_default(tmp_path, posts, monkeypatch, capsys):
    run, outside, ys = _run(tmp_path)
    measured = []
    monkeypatch.setattr(Y, 'measure_cells', lambda run, it, label, pair, cfg, ys, **kw: measured.append(pair) or
                        _row(it, sha=Y.sha12(outside), pair=pair))
    monkeypatch.delenv(RY.CHILD_ENV, raising=False)
    assert _one(run, ys, '--series-label', 'pb1@262', '--ref-run', 'delver-bo3-pb1', '--ref-gen', '262',
                '--mark', '16=the fork') == 0
    out = capsys.readouterr().out
    assert json.loads(out.strip().splitlines()[-1])['iter'] == 16      # the result line on stdout, as always
    (url, body, key), = posts
    s = body['series'][0]
    assert body['run'] == 'refmatch.legacy-meta3-mirror-snp' and s['label'] == 'pb1@262' and s['ref']['gen'] == 262
    assert body['marks'] == [{'iter': 16, 'label': 'the fork'}] and [r['iter'] for r in s['rows']] == [16]
    assert len(RM.read_rows([RM.hand_rows_path(run)])) == 1 and not os.path.exists(Y.rows_path(run))
    # the fields are kept: the next measurement keeps the label without the flags
    assert _one(run, ys) == 0 and posts[-1][1]['series'][0]['label'] == 'pb1@262'
    # --no-publish: loud, the row still kept, nothing posted
    assert _one(run, ys, '--no-publish') == 0
    assert 'PUBLISH OFF: refmatch.legacy-meta3-mirror-snp' in capsys.readouterr().err and len(posts) == 2
    assert len(RM.read_rows([RM.hand_rows_path(run)])) == 3
    # the watcher's child never posts (the watcher does)
    monkeypatch.setenv(RY.CHILD_ENV, '1')
    assert _one(run, ys) == 0 and len(posts) == 2 and len(RM.read_rows([RM.hand_rows_path(run)])) == 3


def test_one_refuses_before_any_game_when_the_post_could_not_work(tmp_path, posts, monkeypatch, capsys):
    run, outside, ys = _run(tmp_path)
    monkeypatch.delenv(RY.CHILD_ENV, raising=False)
    played = []
    monkeypatch.setattr(Y, 'measure_cells', lambda run, it, label, pair, cfg, ys, **kw: played.append(pair) or
                        _row(it, sha=Y.sha12(outside), pair=pair))
    args = ['one', run, '--kind', 'cells', '--iter', '16', '--ref', 'pb262', '--yardsticks', json.dumps(ys)]
    with pytest.raises(SystemExit, match='mirrors only'):
        RY.main(args + ['--pair', f'{D},{Z}'])
    assert RY.main(args + ['--pair', f'{D},{Z}', '--no-publish']) == 0 and played == [(D, Z)]
    monkeypatch.delenv(P.KEY_ENV)
    monkeypatch.setattr(P, 'KEY_FILE', str(tmp_path / 'no_key'))
    with pytest.raises(SystemExit, match='no upload key'):
        _one(run, ys)
    assert played == [(D, Z)] and posts == []
    # a post that fails: PUBLISH FAILED, exit 3, the row kept
    monkeypatch.setenv(P.KEY_ENV, 'k')

    def refuse(*a, **k):
        raise OSError('site down')
    monkeypatch.setattr(P, 'post', refuse)
    assert _one(run, ys) == 3
    assert 'PUBLISH FAILED: refmatch.legacy-meta3-mirror-snp' in capsys.readouterr().err
    assert [r['iter'] for r in RM.read_rows([RM.hand_rows_path(run)]) if r['pair'][1] == D] == [16]
    # a cells reference of the run itself: nothing to post, nothing said, nothing kept
    monkeypatch.setattr(P, 'post', lambda *a, **k: pytest.fail('posted'))
    assert RY.main(['one', run, '--kind', 'cells', '--iter', '16', '--ref', 'gen0', '--pair', f'{D},{D}',
                    '--yardsticks', json.dumps(ys)]) == 0


# ---------------------------------------------------------------- the watcher, `refmatch`

def test_the_watcher_posts_after_a_mirror_result_and_says_off(tmp_path, posts):
    run, outside, ys = _run(tmp_path)
    logs = []
    w = RY.Watcher(run, once=True, log=logs.append, override=ys)
    _, nys = w.settings()
    row = _row(16, sha=Y.sha12(outside))
    Y.append_row(run, row)
    w.refmatch(row, nys)
    assert len(posts) == 1 and posts[0][1]['series'][0]['rows'][0]['iter'] == 16
    assert any('refmatch.legacy-meta3-mirror-snp posted: 1 rows' in m for m in logs)
    assert not any(m.startswith('refmatch pb262 gen') for m in logs)      # (one line a post, not one a row)
    w.refmatch(_row(16, pair=(D, Z)), nys)                     # not a mirror: no post
    w.refmatch(_row(16, ref='gen0'), nys)                      # the run's own reference: no post
    assert len(posts) == 1
    off = Y.normalize(dict(ys, refmatch_publish=False))
    w.refmatch(row, off)
    assert len(posts) == 1 and any(m.startswith('PUBLISH OFF: refmatch.') for m in logs)
    w.refmatch_plan(Y.cfg_of(run), off)
    assert any("outside reference(s) ['pb262']" in m for m in logs)
    w.refmatch(_row(32, sha='0123456789ab'), nys)              # another network under the label: said, never raised
    assert any(m.startswith('PUBLISH FAILED: refmatch.') for m in logs)


def test_refmatch_adopts_a_sidecars_rows(tmp_path, posts, capsys):
    run, outside, _ = _run(tmp_path, refs=['iter0'])           # (the run's own setting does not name pb262)
    side = tmp_path / 'rows.jsonl'
    side.write_text(''.join(json.dumps(_row(i, t=i)) + '\n' for i in (1234, 1250)))
    assert RY.main(['refmatch', run]) == 3 and 'no outside reference registered' in capsys.readouterr().err
    assert RY.main(['refmatch', run, '--ref', 'pb262', '--rows', str(side), '--series-label', 'pb1@262',
                    '--ref-run', 'delver-bo3-pb1', '--ref-gen', '262', '--row-note', '1234=the perturbed start']) == 0
    (_, body, _), = posts
    s = body['series'][0]
    assert (s['label'], s['ref'], [r['iter'] for r in s['rows']], s['rows'][0]['note']) == (
        'pb1@262', {'run': 'delver-bo3-pb1', 'gen': 262}, [1234, 1250], 'the perturbed start')
    assert RM.read_meta(run)['refs']['pb262']['sha'] == '539c1d5fb979'
    assert RY.main(['refmatch', run, '--dry-run']) == 3                   # (its rows were an extra file: none now)
    assert RY.main(['refmatch', run, '--rows', str(side), '--dry-run']) == 0 and len(posts) == 1
