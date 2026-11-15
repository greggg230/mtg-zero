"""mtgz-worker's game records (zero-farm phase 3; docs/ENGINE.md 114.7; zero-core docs/az.md 45, search.md 2.17): on
the cube, the Delver mirror, EoE sealed and EoE with a deck choice (value choices: searched, never recorded, yet real
moves), every game the worker writes as a record replays through this build's engine module (`replay_rows`, the
trainer's path) into the rows of the shard it writes of the same games, bit for bit (zero-core's
tools/worker_records_gate.py: the uniform evaluator, value-only rows and auxiliary targets on), each game's seed is its
id's and each seat's deck (`record_setup`) is compared by the replay. A served self-play job writes records (its
default); a tampered record is refused. Skipped without a built worker (`cargo build --release -p mtgz_worker`; env
MTGZ_WORKER)."""
import gzip
import json
import os
import subprocess
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tests'))
sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'tools'))

from test_worker_farm import WORKER, Serve  # noqa: E402

pytestmark = pytest.mark.skipif(WORKER is None, reason='mtgz-worker is not built (cargo build --release -p mtgz_worker)')


def _settings(tmp_path, preset, **kw):
    import mtgz.az  # noqa: F401
    from zero import run as R
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': preset})
    # (the uniform evaluator has no auxiliary head: no root belief; the auxiliary targets are still recorded)
    cfg.update(conc=4, threads=2, groups=1, selfplay_belief_tilt=0.0, selfplay_belief_tilts={}, **kw)
    p = tmp_path / f'{preset}.json'
    p.write_text(json.dumps(cfg), encoding='utf-8')
    return p, cfg


@pytest.mark.parametrize('preset,games', [('mtg-smoke', 6), ('delver-smoke', 4), ('eoe-smoke', 6), ('eoe-choice-smoke', 4)])
def test_records_replay_to_the_workers_shards(tmp_path, preset, games):
    import worker_records_gate as WRG
    p, _ = _settings(tmp_path, preset, selfplay_view_samples=0.5, shard_rows=60)
    rep_path = tmp_path / 'rep.json'
    rc = WRG.main(['--worker', WORKER, '--settings', str(p), '--package', 'mtgz.az', '--root', ROOT, '--uniform',
                   '--games', str(games), '--seed', '17', '--gid-base', '3', '--threads', '2', '--out', str(tmp_path / 'g'),
                   '--json', str(rep_path)])
    rep = json.loads(rep_path.read_text(encoding='utf-8'))
    assert rc == 0 and rep['identical'], rep
    assert rep['games'] == games and rep['rows'] > 0 and rep['value_rows'] > 0
    assert rep['setups'] == games, 'every game\'s decks compared by its replay'
    if preset != 'delver-smoke':
        assert rep['files'] >= 2, 'records files cut as the shards are'


def test_a_served_job_writes_records_a_tampered_one_is_refused(tmp_path):
    from mtgz import _engine as E
    from zero import games as G
    p, cfg = _settings(tmp_path, 'eoe-smoke')
    w = Serve(WORKER, tmp_path)
    w.wait_for('hello', 60)
    w.send({'op': 'job', 'lease_id': 'l_rec', 'kind': 'selfplay', 'settings': cfg, 'games': 2, 'gid_base': 10, 'seed': 99,
            'uniform': True, 'threads': 2, 'out_dir': 'out'})
    end = w.wait_for('end', 1200)
    assert w.finish() == 0 and end['reason'] == 'done'
    recs = [e for e in w.events if e['ev'] == 'records']
    assert recs and not [e for e in w.events if e['ev'] == 'shard']
    assert sum(e['games'] for e in recs) == 2 and recs[0]['gids'] == [[10, 12]]
    lines = gzip.decompress((tmp_path / 'out' / recs[0]['path']).read_bytes()).decode().splitlines()
    h = json.loads(lines[0])
    assert (h['records'], h['game'], h['seed'], h['aux']) == (1, 'mtg_eoe', 99, bool(cfg.get('aux_belief_w') or cfg.get('aux_deck_w')))
    spec = G.spec_json(cfg['spec'])
    for ln in lines[1:]:
        s, info = E.replay_rows('mtg_eoe', spec, ln, h['aux'])
        assert info['seed'] == E.game_seed_of(99, info['gid']) and json.loads(info['setup'])['decks']
        bad = json.loads(ln)
        bad['setup'] = json.dumps({'decks': [[], []]})
        with pytest.raises(ValueError, match='setup'):
            E.replay_rows('mtg_eoe', spec, json.dumps(bad), h['aux'])
