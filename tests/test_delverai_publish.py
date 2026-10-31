"""tools/delverai_publish.py's head-to-heads (the site's "win rate against earlier generations"): a
generation's own games in its Elo pool, from its side ("a|b": [points of a, games], names sorted),
the anchors as copies of the run's checkpoints (same sha256), the global generations of the
lineage, and a bot evaluation of 0 games as no measurement."""
import hashlib
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delverai_publish as P  # noqa: E402

TAG = 'cq-n4-v50-s0.3-rv-b1m0.25'


def _write(path, obj, lines=False):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        if lines:
            f.write(''.join(json.dumps(x) + '\n' for x in obj))
        else:
            json.dump(obj, f)


def _run(tmp_path, name='r3-2b-beliefsearch'):
    run = str(tmp_path / name)
    os.makedirs(os.path.join(run, 'models'))
    for k, body in ((192, b'net 192'), (160, b'net 160'), (191, b'net 191')):
        with open(os.path.join(run, 'models', f'iter_{k:04d}.pt'), 'wb') as f:
            f.write(body)
    sha = lambda b: hashlib.sha256(b).hexdigest()
    _write(os.path.join(run, 'config.json'), {'eval_every': 8, 'elo_offsets': '1,2,4,8', 'eval_games': 0,
                                              'elo_anchors': 'a192=/x/anchors/r3-2b_iter_0192.pt,a160=/x/a160.pt'})
    _write(os.path.join(run, 'elo', TAG, 'results.json'), {
        'select': TAG, 'since': 96, 'anchor': 'heuristic',
        'anchors': {'anchor:a192': {'label': 'a192', 'file': 'a192.pt', 'sha256': sha(b'net 192'), 'elo': 1613.0},
                    'anchor:a160': {'label': 'a160', 'file': 'a160.pt', 'sha256': sha(b'net 160'), 'elo': 1578.2},
                    'anchor:x': {'label': 'x', 'file': 'x.pt', 'sha256': sha(b'another'), 'elo': 1500.0}},
        'pairs': {
            'heuristic|random': [95.0, 100],
            'heuristic|iter_0192': [2.0, 100],          # the bot's games: not a head-to-head
            'iter_0191|iter_0192': [11.0, 25],
            'iter_0184|iter_0192': [11.0, 25],
            'iter_0199|iter_0200': [12.0, 24],
            'iter_0196|iter_0200': [9.0, 24],           # iter 196 scored 9: iter 200 scored 15
            'iter_0192|iter_0200': [10.0, 24],
            'anchor:a160|anchor:a192': [90.0, 200],     # the anchors' calibration
            'anchor:a192|iter_0200': [54.0, 100],       # the anchor scored 54: iter 200 46
            'anchor:a160|iter_0200': [49.0, 100],
            'anchor:x|iter_0200': [50.0, 100],
            'iter_0200|iter_0208': [14.0, 24],          # iter 208's game, not 200's
        }})
    _write(os.path.join(run, 'metrics.jsonl'), [{'iter': 192}, {'iter': 199}, {'iter': 200}], lines=True)
    _write(os.path.join(run, 'evals.jsonl'), [
        {'iter': 192, 'eval_yard': {'win': 0.98, 'se': 0.014, 'games': 100, 'select': TAG},
         'eval_random': {'win': 1.0, 'se': 0.0, 'games': 0}, 'elo': {'elo': 1613.1, 'se': 70.0, 'select': TAG}},
        {'iter': 200, 'eval_yard': {'win': 0.0, 'se': 0.0, 'margin': 0.0, 'games': 0, 'select': TAG},
         'elo': {'elo': 1651.8, 'se': 97.9, 'select': TAG}},
    ], lines=True)
    return run


def test_a_generations_own_games_from_its_side(tmp_path):
    rows = {r['iter']: r for r in P.iteration_rows(_run(tmp_path))}
    h = rows[200]['h2h']
    assert [(m.get('back'), m.get('anchor'), m['iter'], m['points'], m['games']) for m in h] == [
        (1, None, 199, 12.0, 24), (4, None, 196, 15.0, 24), (8, None, 192, 14.0, 24),
        (None, 'a192', 192, 46.0, 100), (None, 'a160', 160, 51.0, 100), (None, 'x', None, 50.0, 100)]
    assert h[1]['win'] == 0.625 and h[3]['win'] == 0.46 and h[3]['elo'] == 1613.0 and len(h[3]['sha']) == 12
    assert [m['back'] for m in rows[192]['h2h']] == [1, 8]
    assert rows[199]['h2h'] == []                         # not evaluated (and no game of its own)


def test_no_games_against_the_bot_is_no_measurement(tmp_path):
    rows = {r['iter']: r for r in P.iteration_rows(_run(tmp_path))}
    assert (rows[200]['yard_win'], rows[200]['yard_se'], rows[200]['yard_games']) == (None, None, 0)
    assert (rows[192]['yard_win'], rows[192]['yard_games']) == (0.98, 100)
    assert (rows[192]['rand_win'], rows[192]['rand_games']) == (None, 0)
    assert rows[200]['select'] == TAG


def test_the_lineage_numbers_the_opponents_too(tmp_path):
    # a widened network continuing delver1's iteration 137: its generation k is the lineage's 137 + k
    src = str(tmp_path / 'delver1')
    _write(os.path.join(src, 'metrics.jsonl'), [{'iter': 136}], lines=True)
    run = _run(tmp_path)
    _write(os.path.join(run, 'MIGRATED_FROM.json'), {'migrated_from': src + '/models/iter_0137.pt', 'layout': 'v4 -> v5'})
    L = P.lineage_record(run, {})
    r = next(x for x in L['rows'] if x['run'] == 'r3-2b-beliefsearch' and x['gen'] == 200)
    assert r['g'] == 337 and [m['g'] for m in r['h2h']] == [336, 333, 329, 329, 297, None]
    assert [m['iter'] for m in r['h2h']] == [199, 196, 192, 192, 160, None]   # the run's own numbering kept
