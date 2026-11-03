"""Branched self-play games and archive starts on the Delver mirror (docs/ENGINE.md 87; zero-core docs/az.md
24): the archive builder (tools/delver_archive.py) - canary spots rebuilt through their start block,
a Play-tab record replayed and checked against its view, a tampered or stale source skipped and listed,
never dropped -, the Delver driver branching and starting from the archive (each start the archived
spot, branches with their own outcome), and the run settings through mtgz.az.run. CPU only, no network
(zero.infer.UniformEvaluator)."""
import json
import os
import random
import sys

import numpy as np
import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delver_archive as DA  # noqa: E402

import mtgz.az as A  # noqa: E402
from zero import forks as FK  # noqa: E402
from zero import games as G  # noqa: E402
from zero.infer import UniformEvaluator, drive  # noqa: E402
from zero.workqueue import split_games  # noqa: E402

SPEC = G.spec_json(A.PRESETS['delver-mirror']['spec'])
E = DA.DB.load_engine(ROOT)


def random_record(tmp, gid='20990101-000000-aaaaaa', seed=4242, moves=160, you=0):
    """A Play-tab-shaped record of a random Delver mirror game (seed + root actions) and its view (the
    engine's state as the human sees it)."""
    g = E.Game('mtg', SPEC, 2, seed)
    DA.auto_step(g)
    r = random.Random(seed)
    acts = []
    for _ in range(moves):
        if g.over:
            break
        a = r.choice([int(x) for x in g.legal()])
        acts.append(a)
        g.step(a)
        DA.auto_step(g)
    d = tmp / 'play' / gid
    d.mkdir(parents=True)
    (d / 'game.json').write_text(json.dumps({'format': 'delverlab-play/1', 'id': gid, 'seed': seed, 'you': you,
                                             'actions': acts, 'vn': 0, 'model': {'run': 'test', 'iter': 0}}))
    (d / 'view.json').write_text(json.dumps({'n': len(acts), 'state': json.loads(g.export_state())}))
    return acts


def library_file(tmp, n=3, corrupt=None):
    L = DA.DB.load_library(DA.DB.LIBRARY_PATH)
    spots = [dict(sp) for sp in L['spots'][:n]]
    if corrupt is not None:
        spots[corrupt]['sig'] = '0' * 16
    p = tmp / 'lib.json'
    p.write_text(json.dumps(dict(L, spots=spots)))
    return str(p), spots


def build(tmp, lib, extra=()):
    out = tmp / 'archive.json'
    DA.main(['--out', str(out), '--library', lib, '--play', str(tmp / 'play'), '--reports', str(tmp / 'none'), *extra])
    return json.loads(out.read_text()), json.loads((tmp / 'archive.report.json').read_text())


def test_the_builder_checks_every_position_and_lists_what_it_skips(tmp_path):
    lib, spots = library_file(tmp_path, n=4, corrupt=1)
    random_record(tmp_path)
    bad = random_record(tmp_path, gid='20990101-000000-bbbbbb', seed=77)
    rec = json.loads((tmp_path / 'play' / '20990101-000000-bbbbbb' / 'game.json').read_text())
    rec['actions'][len(bad) // 2] = 9999                     # an action no engine offers
    (tmp_path / 'play' / '20990101-000000-bbbbbb' / 'game.json').write_text(json.dumps(rec))
    stale = random_record(tmp_path, gid='20990101-000000-cccccc', seed=78)
    view = json.loads((tmp_path / 'play' / '20990101-000000-cccccc' / 'view.json').read_text())
    view['state']['players'][0]['life'] += 1                 # another build's game: replays elsewhere
    (tmp_path / 'play' / '20990101-000000-cccccc' / 'view.json').write_text(json.dumps(view))
    assert stale
    arch, rep = build(tmp_path, lib)
    kinds = [e['kind'] for e in arch['entries']]
    assert kinds.count('canary') == 3 and kinds.count('human') >= 3, kinds
    why = {(s['kind'], s['source'].split('@')[0]): s['reason'] for s in rep['skipped']}
    assert 'fingerprint' in why[('canary', spots[1]['id'])]
    assert 'not legal' in why[('human', '20990101-000000-bbbbbb')]
    assert 'another state than its recorded view' in why[('human', '20990101-000000-cccccc')]
    assert arch['format'] == FK.FORMAT and json.loads(arch['base_spec']) == json.loads(SPEC)
    # every entry starts at its source's state: the start block with deal "none" is the replay itself
    for e in arch['entries']:
        st = e['patch']['start']
        assert st['deal'] == 'actual' and set(st) == {'seed', 'actions', 'deal'}
        s = json.loads(SPEC)
        s['start'] = dict(st, deal='none')
        g = E.Game('mtg', json.dumps(s), 2, 0)
        assert DA.DB.spot_sig(json.loads(g.export_state())) == e['sig'] and g.to_move == e['decider']
        if e['kind'] == 'human':
            assert 3 <= e['turn'] <= 8
    # the archive loads for the run's spec, not for another
    A2 = FK.load_archive(str(tmp_path / 'archive.json'), 'mtg', SPEC)
    assert A2['n'] == len(arch['entries'])
    other = json.loads(SPEC)
    other['max_turns'] = 59
    with pytest.raises(SystemExit) as ex:
        FK.load_archive(str(tmp_path / 'archive.json'), 'mtg', json.dumps(other))
    assert 'another spec' in str(ex.value)


def test_the_delver_driver_branches_and_starts_from_the_archive(tmp_path):
    lib, spots = library_file(tmp_path, n=3)
    (tmp_path / 'play').mkdir()
    arch, _ = build(tmp_path, lib)
    A2 = FK.load_archive(str(tmp_path / 'archive.json'), 'mtg', SPEC)
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, sims_minor=2, temp_moves=60)
    d = G.Driver('mtg', [[seat, seat]] * 12, spec=SPEC, concurrency=12, seed=3, record=True, threads=4,
                 branch={'p': 0.5, 'moves': 3, 'temp': 2.0}, starts={'p': 0.5, 'specs': A2['specs']}, move_log=4000)
    rows = []
    drive(d, {'m': UniformEvaluator()}, on_batch=lambda lb: rows.extend(split_games(d.take_results(), d.take_samples())))
    rows.extend(split_games(d.take_results(), d.take_samples()))
    main = [r for r, _ in rows if 'branch' not in r]
    br = [(r, s) for r, s in rows if 'branch' in r]
    starts = [r for r in main if 'start' in r]
    assert len(main) == 12 and br and starts, (len(main), len(br), len(starts))
    for r in starts:
        e = arch['entries'][r['start']]
        assert r['rounds'] >= e['turn']                     # it began at the archived spot's turn
        assert r['moves'][0][0] == e['decider']             # the archived decider moves first
    for r, s in br:
        b = r['branch']
        assert 1 <= b['moves'] <= 3 and r['samples'] <= r['decisions'] - b['decision'] - b['played']
        if s is not None:
            z = np.asarray(s['z'])[:, :2]
            assert np.allclose(z, [[r['result'][(p + j) % 2] for j in range(2)] for p in s['player']])


def test_the_run_settings(tmp_path):
    from zero.run import DEFAULTS, resolve_config
    import argparse
    assert DEFAULTS['selfplay_branch'] == 0.0 and DEFAULTS['selfplay_archive'] == ''

    def cfg(**kw):
        ns = argparse.Namespace(**{k: None for k in DEFAULTS})
        ns.preset = 'delver-mirror'
        for k, v in kw.items():
            setattr(ns, k, v)
        return resolve_config(ns, {})
    c = cfg(selfplay_branch=0.2, selfplay_branch_temp=2.0, selfplay_archive='x.json')
    assert c['selfplay_branch'] == 0.2 and c['rolling'] == 1
    with pytest.raises(SystemExit) as e:
        cfg(selfplay_branch=0.2, rolling=0)
    assert 'needs rolling' in str(e.value)
    with pytest.raises(SystemExit):
        cfg(selfplay_branch=1.2)
