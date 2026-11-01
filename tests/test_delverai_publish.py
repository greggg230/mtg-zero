"""tools/delverai_publish.py's head-to-heads (the site's "win rate against earlier generations"): a
generation's own games in its Elo pool, from its side ("a|b": [points of a, games], names sorted),
the anchors as copies of the run's checkpoints (same sha256), the global generations of the
lineage, and a bot evaluation of 0 games as no measurement. The anchored Elo: each row's from its
evaluation, the anchors' definitions."""
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
        'anchors': {'anchor:a192': {'label': 'a192', 'file': 'a192.pt', 'sha256': sha(b'net 192'), 'elo': 1613.0,
                                    'how': 'set (elo_anchor_elo)', 'since': 200},
                    'anchor:a160': {'label': 'a160', 'file': 'a160.pt', 'sha256': sha(b'net 160'), 'elo': 1578.2, 'se': 24.7,
                                    'how': "calibrated: 200 games against each of ['anchor:a192']"},
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
         'elo': {'elo': 1651.8, 'se': 97.9, 'select': TAG}, 'elo_anchored': {'elo': 1587.54, 'se': 24.43, 'select': TAG}},
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


def test_the_anchored_elo_and_the_anchors(tmp_path):
    run = _run(tmp_path)
    rows = {r['iter']: r for r in P.iteration_rows(run)}
    assert (rows[200]['elo_anchored'], rows[200]['elo_anchored_se'], rows[200]['elo']) == (1587.5, 24.4, 1651.8)
    assert (rows[192]['elo_anchored'], rows[192]['elo_anchored_se']) == (None, None)     # before the anchors: the pool's only
    assert P.anchor_defs(run, TAG, offset=137) == [
        {'label': 'a192', 'iter': 192, 'g': 329, 'elo': 1613.0, 'se': None, 'how': 'set', 'since': 200},
        {'label': 'a160', 'iter': 160, 'g': 297, 'elo': 1578.2, 'se': 24.7, 'how': 'calibrated', 'since': None},
        {'label': 'x', 'iter': None, 'g': None, 'elo': 1500.0, 'se': None, 'how': None, 'since': None}]
    assert P.anchor_defs(run, 'visits') == []


def test_an_exploiter_fork_finds_its_anchors_in_its_parent_and_labels_its_games(tmp_path):
    import argparse
    parent = _run(tmp_path)                                  # r3-2b-beliefsearch: models 160, 191, 192
    fork = str(tmp_path / 'r3-2b-exploit475')
    os.makedirs(os.path.join(fork, 'models'))
    with open(os.path.join(fork, 'models', 'iter_0475.pt'), 'wb') as f:
        f.write(b'net 475')                                  # the fork point: its own copy; 192 / 160 only in the parent
    res = json.load(open(os.path.join(parent, 'elo', TAG, 'results.json')))
    res['anchors']['anchor:a475'] = {'label': 'a475', 'file': 'a475.pt', 'sha256': hashlib.sha256(b'net 475').hexdigest(),
                                     'elo': 1650.0, 'how': 'calibrated: 200 games', 'since': 480}
    res['pairs'] = {'anchor:a475|iter_0480': [62.0, 100], 'anchor:a192|iter_0480': [40.0, 100]}
    _write(os.path.join(fork, 'elo', TAG, 'results.json'), res)
    _write(os.path.join(fork, 'config.json'), {'elo_anchors': 'a475=/x/anchors/r3-2b_iter_0475.pt'})
    _write(os.path.join(fork, 'metrics.jsonl'), [{'iter': 480, 'elo': {'elo': 1700.0, 'se': 90.0, 'select': TAG}}], lines=True)
    forks = {P._norm(fork): {'parent': parent, 'at': 475, 'label': 'exploiter vs 475', 'won': False}}
    segs = P.lineage(fork, forks)
    assert [s['run'] for s in segs] == ['r3-2b-beliefsearch', 'r3-2b-exploit475'] and P.same_numbering(segs, 1) == (parent,)
    alone = {a['label']: a['iter'] for a in P.anchor_defs(fork, TAG)}
    assert alone == {'a192': None, 'a160': None, 'x': None, 'a475': 475}  # without its parent: only its own 475
    defs = {a['label']: a['iter'] for a in P.anchor_defs(fork, TAG, also=(parent,))}
    assert defs == {'a192': 192, 'a160': 160, 'x': None, 'a475': 475}
    rows = P.iteration_rows(fork, also=(parent,))
    assert [(m['anchor'], m['iter'], m['points'], m['win']) for m in rows[0]['h2h']] == [('a192', 192, 60.0, 0.6), ('a475', 475, 38.0, 0.38)]
    # the recorded games against the frozen 475: each seat's label, the record's fields
    a = argparse.Namespace(record_vs='/x/anchors/r3-2b_iter_0475.pt', record_vs_label='Gen 475 (frozen)',
                           record_label='Exploiter gen {iter}', title='Exploiter: trained only against gen 475',
                           note='Every self-play game against a frozen 475.', side=True)
    rep = {'seats': [{'checkpoint': 'r3-2b_iter_0475.pt'}, {'checkpoint': 'C:\\runs\\x\\models\\iter_0480.pt'.replace('\\', os.sep)}]}
    assert P.seat_labels(rep, os.path.join('runs', 'x', 'models', 'iter_0480.pt'), a, 480) == ['Gen 475 (frozen)', 'Exploiter gen 480']
    assert P.record_extra(a) == {'title': 'Exploiter: trained only against gen 475', 'note': 'Every self-play game against a frozen 475.',
                                 'side': True, 'record_vs': {'label': 'Gen 475 (frozen)'}}
