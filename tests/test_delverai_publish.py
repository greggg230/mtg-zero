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
    vs = os.path.join(fork, 'models', 'iter_0475.pt')     # the frozen 475 (the anchor a475's file)
    a = argparse.Namespace(record_vs=vs, record_vs_label='Gen 475 (frozen)',
                           record_label='Exploiter gen {iter}', title='Exploiter: trained only against gen 475',
                           note='Every self-play game against a frozen 475.', side=True)
    rep = {'seats': [{'checkpoint': 'r3-2b_iter_0475.pt'}, {'checkpoint': 'C:\\runs\\x\\models\\iter_0480.pt'.replace('\\', os.sep)}]}
    assert P.seat_labels(rep, os.path.join('runs', 'x', 'models', 'iter_0480.pt'), a, 480) == ['Gen 475 (frozen)', 'Exploiter gen 480']
    assert P.record_extra(a) == {'title': 'Exploiter: trained only against gen 475', 'note': 'Every self-play game against a frozen 475.',
                                 'side': True, 'record_vs': {'label': 'Gen 475 (frozen)', 'sha': hashlib.sha256(b'net 475').hexdigest()[:12]}}


def test_the_upload_body_is_compact_and_drops_none_fields():
    p = {'run': 'r', 'best': None, 'iters': [{'iter': 1, 'time': 1793869394.534, 'elo': None, 'h2h': [], 'phases': {'a': None}}],
         'lineage': {'segments': [{'run': 'r', 'last': None}], 'rows': [{'iter': 1, 'g': 7, 'elo': None, 'time': 5.6}]}}
    body = P.wire_progress(p)
    assert b': ' not in body and b', ' not in body
    w = json.loads(body)
    assert w['iters'] == [{'iter': 1, 'time': 1793869395, 'h2h': [], 'phases': {'a': None}}]   # only the rows' own None fields go
    assert w['lineage'] == {'segments': [{'run': 'r', 'last': None}], 'rows': [{'iter': 1, 'g': 7, 'time': 6}]}
    assert w['best'] is None and p['iters'][0]['elo'] is None                                 # the record itself unchanged
    assert json.loads(P.wire_progress({'run': 'r', 'iters': [], 'lineage': None}))['lineage'] is None


def test_the_averaged_anchored_elo_takes_twelve_readings_with_their_covariance(tmp_path):
    # 14 evaluated generations (every 8), each 100 games against two fixed anchors and 24 against 8 back
    run = str(tmp_path / 'avg')
    gens = [8 * k for k in range(1, 15)]
    pairs = {}
    for k, g in enumerate(gens):
        pairs[f'anchor:a|iter_{g:04d}'] = [50.0 - k, 100]
        pairs[f'anchor:b|iter_{g:04d}'] = [40.0 - k, 100]
        if k:
            pairs[f'iter_{gens[k - 1]:04d}|iter_{g:04d}'] = [11.0, 24]
    res = {'anchor': 'heuristic', 'anchors': {'anchor:a': {'elo': 1600.0}, 'anchor:b': {'elo': 1550.0}}, 'pairs': pairs}
    _write(os.path.join(run, 'elo', TAG, 'results.json'), res)
    idx, cov = P.anchored_cov(run, TAG)
    fit = P.zelo.fit_fixed(res, {'anchor:a': 1600.0, 'anchor:b': 1550.0}, prior_sd=P.PRIOR_SD)
    for g in gens:                                   # the covariance's diagonal is the fit's se^2
        n = f'iter_{g:04d}'
        assert abs(cov[idx[n], idx[n]] ** 0.5 - fit[n][1]) < 1e-6
    rows = [{'iter': g, 'select': TAG, 'elo_anchored': round(fit[f'iter_{g:04d}'][0], 1),
             'elo_anchored_se': round(fit[f'iter_{g:04d}'][1], 1)} for g in gens]
    rows.insert(3, {'iter': 27, 'select': TAG, 'elo_anchored': None})       # a generation without a reading
    assert P.add_anchored_averages(rows, lambda t: P.anchored_cov(run, t)) == 0
    avg = [r for r in rows if 'elo_avg' in r]
    assert [r['iter'] for r in avg] == [96, 104, 112]                       # from the 12th reading
    win = [r for r in rows if r.get('elo_anchored') is not None][:12]
    assert abs(avg[0]['elo_avg'] - sum(r['elo_anchored'] for r in win) / 12) < 0.051
    ii = [idx[f'iter_{r["iter"]:04d}'] for r in win]
    se = cov[P.np.ix_(ii, ii)].sum() ** 0.5 / 12
    assert abs(avg[0]['elo_avg_se'] - se) < 0.051
    naive = sum(r['elo_anchored_se'] ** 2 for r in win) ** 0.5 / 12
    assert se > naive                                # the games between them correlate the readings
    # a stretch of more than 12 generations starts again; so does another pool
    bare = lambda r: {k: v for k, v in r.items() if not k.startswith('elo_avg')}
    gap = [bare(r) for r in rows if r.get('elo_anchored') is not None]
    for r in gap[6:]:
        r['iter'] += 40
    assert P.add_anchored_averages(gap, lambda t: None) == 0 and not any('elo_avg' in r for r in gap)
    other = [dict(bare(r), select=('visits' if k == 5 else TAG)) for k, r in enumerate(win + rows[-2:])]
    P.add_anchored_averages(other, lambda t: P.anchored_cov(run, t))
    assert not any('elo_avg' in r for r in other)
    # a window with a checkpoint the fit doesn't rate: no average, counted
    lost = [bare(r) for r in win]
    lost[0]['iter'] = 4
    assert P.add_anchored_averages(lost, lambda t: P.anchored_cov(run, t)) == 1 and 'elo_avg' not in lost[-1]


def test_the_gap_rule_is_twice_the_measured_step_of_each_segment(tmp_path):
    # the main line rated every 8th generation, its fork from 40 every 16th (the 128x8 run's
    # --eval-every 16): a reading 16 after the last one is no gap there, 48 is. Measured, not the
    # config's eval_every (1000 on runs evaluated from outside)
    main = str(tmp_path / 'main')
    fork = str(tmp_path / 'fork')
    _write(os.path.join(main, 'config.json'), {'eval_every': 1000})
    _write(os.path.join(fork, 'config.json'), {'eval_every': 16})
    rated = lambda g: {'iter': g, 'elo': {'elo': 1500.0, 'se': 50.0, 'select': TAG}}
    _write(os.path.join(main, 'metrics.jsonl'), [rated(g) for g in (8, 16, 24, 32, 40)], lines=True)
    _write(os.path.join(fork, 'metrics.jsonl'), [rated(g) for g in (8, 16, 24, 32, 40)] + [{'iter': 41}]
           + [rated(g) for g in (88, 104, 120)], lines=True)
    assert (P.rated_every([8, 16, 24, 40]), P.rated_every([1, 2, 6, 10]), P.rated_every([5]), P.rated_every([])) == (8, 4, None, None)
    assert P.rated_every([0, 8, 24]) == 8                                       # a tie: the smaller
    assert (P.max_gap(8), P.max_gap(16), P.max_gap(4), P.max_gap(None)) == (16, 32, 12, 12)   # at least 12
    forks = {P._norm(fork): {'parent': main, 'at': 40, 'label': 'deeper', 'won': False}}
    L = P.lineage_record(fork, forks)
    assert [(sg['run'], sg['from_g'], sg['rated_every']) for sg in L['segments']] == [('main', 0, 8), ('fork', 41, 16)]
    rec = P.progress(fork, forks=forks)
    assert rec['rated_every'] == [[0, 8], [41, 16]]
    own = P.gap_rule(rec['rated_every'])
    assert [own(g) for g in (0, 40, 41, 100)] == [16, 16, 32, 32]
    assert P.gap_rule([])(5) == 12
    # readings every 16 average across the fork's steps; the old 12 never did; a 48-generation hole starts again
    rows = [{'iter': g, 'select': TAG, 'elo_anchored': 1700.0} for g in range(48, 48 + 16 * 14, 16)]
    cov = ({f'iter_{g:04d}': k for k, g in enumerate(range(0, 400))}, P.np.eye(400))
    assert P.add_anchored_averages(rows, lambda t: cov, gap=own) == 0
    assert [r['iter'] for r in rows if 'elo_avg' in r] == [224, 240, 256]
    bare = lambda r: {k: v for k, v in r.items() if not k.startswith('elo_avg')}
    old = [bare(r) for r in rows]
    assert P.add_anchored_averages(old, lambda t: cov) == 0 and not any('elo_avg' in r for r in old)
    holed = [bare(r) for r in rows]
    for r in holed[3:]:
        r['iter'] += 32                                   # 80 -> 128: 48 apart
    P.add_anchored_averages(holed, lambda t: cov, gap=own)
    assert not any('elo_avg' in r for r in holed)        # 11 readings since the hole


def _bo3_spec():
    from mtgz.az import SPEC_DELVER_BO3
    return SPEC_DELVER_BO3


def test_a_best_of_three_run_starts_a_new_history_with_its_own_elo_scale(tmp_path):
    # delver-bo3-pb1: forked from the best-of-one line's 3198, converted (its generation 0), anchored to its own 0 =
    # 1000 (anchor path '..._a3198_iter0000.pt'); the fork record's scale_break keeps the parent's rows out
    parent = _run(tmp_path, 'r3-deep128x8-2410')
    run = str(tmp_path / 'delver-bo3-pb1')
    os.makedirs(os.path.join(run, 'models'))
    with open(os.path.join(run, 'models', 'iter_0000.pt'), 'wb') as f:
        f.write(b'net bo3 0')
    sha0 = hashlib.sha256(b'net bo3 0').hexdigest()
    tag = TAG + '-tdeck1'
    _write(os.path.join(run, 'config.json'), {'eval_every': 16, 'spec': _bo3_spec(), 'games': 1350,
                                              'elo_anchors': 'a3198=/x/anchors/delver-bo3_a3198_iter0000.pt'})
    _write(os.path.join(run, 'elo', tag, 'results.json'), {
        'select': tag, 'anchors': {'anchor:a3198': {'label': 'a3198', 'file': 'a3198.pt', 'sha256': sha0, 'elo': 1000.0,
                                                    'how': 'set (elo_anchor_elo)', 'since': 0}},
        'pairs': {'anchor:a3198|iter_0016': [11.0, 100], 'iter_0015|iter_0016': [13.0, 24]}})
    _write(os.path.join(run, 'metrics.jsonl'), [{'iter': 15}, {'iter': 16}], lines=True)
    _write(os.path.join(run, 'evals.jsonl'), [{'iter': 16, 'elo': {'elo': 1066.1, 'se': 567.9, 'select': tag},
                                               'elo_anchored': {'elo': 1360.4, 'se': 55.0, 'select': tag}}], lines=True)
    state = {'done': [{'name': 'best of three', 'flag': '--preset delver-mirror-bo3', 'note': 'post-sideboard games',
                       'k': 3198, 'main_before': parent, 'main_after': run, 'raw': {'a': run + '/models/iter_0000.pt'},
                       'scale_break': 'its own Elo scale'}]}
    _write(str(tmp_path / 'forks.json'), state)
    forks = P.fork_registry([str(tmp_path / 'forks.json')])
    assert forks[P._norm(run)]['scale_break'] == 'its own Elo scale'
    segs = P.lineage(run, forks)
    assert [(s['run'], s['kind'], s['offset']) for s in segs] == [('delver-bo3-pb1', 'root', 0)]
    assert P.lineage_record(run, forks) is None                       # none of the parent's rows
    ff = P.forked_from(segs, forks)
    assert (ff['run'], ff['iter'], ff['g'], ff['label'], ff['scale_break']) == ('r3-deep128x8-2410', 3198, 3198, 'best of three', 'its own Elo scale')
    assert [s['run'] for s in ff['parent_lineage']] == ['r3-deep128x8-2410']
    rec = P.progress(run, forks=forks, state_dir=str(tmp_path / 'state'))
    assert (rec['format'], rec['start_game'], rec['lineage'], rec['forked_from']['iter']) == ('bo3', 2, None, 3198)
    # (its pool's anchor: its own generation 0, found by the '..._iter0000.pt' path of the run's elo_anchors)
    assert P.anchor_defs(run, tag) == [{'label': 'a3198', 'iter': 0, 'g': 0, 'elo': 1000.0, 'se': None, 'how': 'set', 'since': 0}]
    row = next(r for r in rec['iters'] if r['iter'] == 16)
    assert row['elo_anchored'] == 1360.4 and [(m.get('anchor'), m['iter'], m['win']) for m in row['h2h']] == [(None, 15, 0.4583), ('a3198', 0, 0.89)]
    assert rec['deck']['format'] == 'bo3' and rec['deck']['start_game'] == 2 and rec['deck']['mirror'] is True
    assert sum(n for n, _ in rec['deck']['main']) == 60 and sum(n for n, _ in rec['deck']['sideboard']) == 15
    assert ['Rough // Tumble' in [x for _, x in rec['deck']['sideboard']]] == [True]
    assert 'keep_yardstick' not in rec
    # a best-of-one run's record says so, and nothing more
    assert P.progress(parent)['format'] == 'bo1' and 'forked_from' not in P.progress(parent) and 'start_game' not in P.progress(parent)


def test_the_keep_your_60_yardstick_is_read_from_its_files(tmp_path):
    d = str(tmp_path / 'yardstick')
    os.makedirs(d)
    ck = lambda run, k: f'E:/mtg-zero/runs/{run}/models/iter_{k:04d}.pt'
    res = lambda a, b, win, keeps='b': {'a': a, 'b': b, 'keeps': keeps, 'games': 400, 'a_score': 0.3937, 'se': 0.0214, 'sims': 64,
                                        'per_game': {'games': 607, 'win': win, 'se': 0.0198, 'draws': 3}}
    files = {'keep_0000.txt': ['zero.infer: token network CUDA graphs on', json.dumps(res(ck('bo3', 0), ck('bo3', 0), 0.3929)), 'iter 0000 exit 0'],
             'keep_0036.txt': [json.dumps(res(ck('bo3', 36), ck('bo3', 36), 0.4918))],
             'keep_0040.txt': ['not finished yet'],                                                        # no result line
             'keep_0050.txt': [json.dumps(res(ck('bo3', 50), ck('bo3', 48), 0.6))],                         # two checkpoints
             'keep_0060.txt': [json.dumps(res(ck('other', 60), ck('other', 60), 0.6))],                      # another run
             'keep_0070.txt': [json.dumps(res(ck('bo3', 70), ck('bo3', 70), 0.6, keeps='a'))]}               # A keeps
    for name, lines in files.items():
        with open(os.path.join(d, name), 'w', encoding='utf-8') as f:
            f.write('\n'.join(lines) + '\n')
    rows = P.keep_yardstick(d, 'bo3')
    assert [(r['iter'], r['win'], r['se'], r['games'], r['score'], r['matches']) for r in rows] == [
        (0, 0.3929, 0.0198, 607, 0.3937, 400), (36, 0.4918, 0.0198, 607, 0.3937, 400)]
    assert not any('E:' in json.dumps(r) for r in rows)                     # no paths for the public site
    assert [r['iter'] for r in P.keep_yardstick(d, 'other')] == [60]
    assert P.keep_yardstick(None) == [] and P.keep_yardstick(str(tmp_path / 'none')) == []
