"""Farm analysis campaigns - play jobs with traces (docs/ENGINE.md 165; mtgz.az.farm_campaign): the settings that make
a farm job's seats zero.h2h's (the yardsticks' best of one) and the refusal of a match one eval job cannot express;
real Delver game records (a traced rolling driver, uniform evaluator) collected from a fake coordinator - credited
rows only, first copy wins, digests checked, a tampered row refusing the campaign, positive coverage, the spot check
replaying from seed + moves, the per-game records; self-play records (the samples option); the yardsticks' backend
choice and lanes. No farm, no worker, no network is needed."""
import gzip
import hashlib
import json
import os
import sys
from urllib.parse import parse_qs, urlparse

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401
from mtgz import _engine as E  # noqa: E402
from mtgz.az import farm_campaign as C  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402
from zero import farm_def as FD, farm_eval as FE, games as G  # noqa: E402
from zero.infer import drive  # noqa: E402

SEED = 0


def _run(tmp_path, name='run', cache=None, pool=True):
    run = tmp_path / name
    (run / 'models').mkdir(parents=True)
    for i in (0, 16):
        (run / 'models' / f'iter_{i:04d}.pt').write_bytes(b'x')
    cfg = {'preset': 'delver-mirror-bo3-dealt', 'eval_every': 16,
           'eval_spec': mtgz.az.PRESETS['delver-mirror-bo3-dealt']['eval_spec']}
    if cache is not None:
        cfg['eval_cache'] = cache
    if pool:
        cfg.update(selfplay_remote='mac,pool:farm-x', pool_build='b' * 24, pool_runtime='c' * 24,
                   pool_worker=str(tmp_path / 'w.exe'))
    (run / 'config.json').write_text(json.dumps(cfg))
    return str(run), cfg


def test_match_settings_give_the_local_matchs_seats(tmp_path):
    run, cfg = _run(tmp_path)
    ys = Y.normalize(Y.DELVER_YARDSTICKS)
    spec = Y.bo1_spec(cfg, ys)
    a, b = Y.model_path(run, 16), Y.model_path(run, 0)
    s, rule = C.match_settings(a, 64, spec, tilts=Y.bo1_tilts(cfg), ckpt_b=b)
    assert s['eval_trace'] == 1 and s['eval_sims'] == 64 and s['sparse'] == 0 and s['chance_k'] == 2
    assert json.loads(s['eval_spec']) == json.loads(spec)
    assert 'deck' not in (s.get('eval_belief_tilts') or {})
    seat = Y.bo1_seat(a)
    assert {k: rule.get(k) for k in seat} == seat, 'the rule is the yardsticks\' seat'
    assert C.farm_seats(s, [{'net': 1}, {'net': 2}]) == [C.match_seat(a, 64, rule), C.match_seat(b, 64, rule)]
    # an opponent whose run evaluates with another cache: one eval job cannot seat both
    other, _ = _run(tmp_path, 'other', cache=1234)
    with pytest.raises(C.CampaignError, match='eval_cache'):
        C.match_settings(a, 64, spec, tilts={}, ckpt_b=Y.model_path(other, 0))


def _sides(*argv):
    import argparse
    from zero import seat_settings as SS
    ap = argparse.ArgumentParser()
    SS.add_side_args(ap, local=False)
    return ap.parse_args(list(argv))


def test_per_side_options_are_the_seats_sets(tmp_path):
    """zero-core docs/az.md 68, docs/ENGINE.md 214: zero.h2h's per-side options give each seat its set (A against itself:
    one network, two settings), each checked against the local match's seat of that side; the readout names each side's
    rule; without options the settings and seats of before; B's run's own cache is still no per-side option."""
    run, cfg = _run(tmp_path)
    ys = Y.normalize(Y.DELVER_YARDSTICKS)
    spec = Y.bo1_spec(cfg, ys)
    a = Y.model_path(run, 16)
    s0, r0 = C.match_settings(a, 32, spec, tilts={})
    assert C.match_settings(a, 32, spec, tilts={}, sides=_sides()) == (s0, r0)
    assert C.campaign_seats(C.side_sets(s0, _sides()), False) == [{'net': 1}, {'net': 1}]
    sides = _sides('--a-infoset-merge', '1', '--a-leaf-value-view', 'root', '--b-sims', '16')
    s, rule = C.match_settings(a, 32, spec, tilts={}, sides=sides)
    assert (s, rule) == (s0, r0), "the settings are the match's: the options are the seats'"
    seats = C.campaign_seats(C.side_sets(s, sides), False)
    assert seats == [{'net': 1, 'set': {'eval_infoset_merge': 1, 'eval_leaf_value_view': 'root'}},
                     {'net': 1, 'set': {'eval_sims': 16}}]
    assert C.check_seats(seats, None) == seats
    got = C.farm_seats(s, seats)
    assert got[0]['infoset_merge'] and got[0]['leaf_value_view'] == 'root' and got[1]['sims'] == 16
    with pytest.raises(C.CampaignError, match='samples'):
        C.check_seats(seats, None, samples=True)
    with pytest.raises(C.CampaignError):
        C.check_seats([{'net': 1}, {'net': 3}], 'b.pt')
    with pytest.raises(C.CampaignError, match='second network'):
        C.check_seats(seats, 'b.pt')
    with pytest.raises(C.CampaignError, match='leaf-value-view'):
        C.match_settings(a, 32, spec, tilts={}, sides=_sides('--a-infoset-merge', '1', '--a-leaf-value-view', 'mover'))
    other, _ = _run(tmp_path, 'other', cache=1234)
    with pytest.raises(C.CampaignError, match='eval_cache'):
        C.match_settings(a, 32, spec, tilts={}, ckpt_b=Y.model_path(other, 0), sides=sides)
    # the readout: A = seats[0] as a whole, each side's rule and simulations
    d = str(tmp_path / 'cmp-sides')
    os.makedirs(d)
    rows = []
    for g in range(4):
        rows.append({'gid': g, 'pair': g // 2, 'seats': FE.seats_of(seats, g), 'result': [1.0, 0.0] if g % 3 else [0.0, 1.0],
                     'margin': 1.0, 'rounds': 3, 'over': True})
    C._append_rows(d, rows)
    st = {'seats': seats, 'paired': True, 'settings': s, 'job': {'kind': 'play', 'trace': True, 'samples': False}}
    out = C.readout(d, st)
    assert out['games'] == 4 and out['select_a'].endswith('-ism') and not out['select_b'].endswith('-ism')
    assert (out['sims_a'], out['sims_b']) == (32, 16)
    assert C.readout(d, dict(st, seats=[{'net': 1}, {'net': 1}])) is None


def test_the_yardsticks_backend_is_the_farm_only_where_it_plays_the_same_match(tmp_path):
    run, cfg = _run(tmp_path)
    other, _ = _run(tmp_path, 'other', cache=1234, pool=False)
    ys = Y.normalize(dict(Y.DELVER_YARDSTICKS, bo1_opponents=['iter0', f'x={Y.model_path(other, 0)}']))
    opps = {o['name']: o for o in Y.opponents(run, cfg, ys)}
    assert Y.bo1_backend(run, cfg, ys, opps['gen0']) == 'farm'
    assert Y.bo1_backend(run, cfg, ys, opps['x']) == 'local'
    assert Y.bo1_lanes(run, cfg, ys) == ['farm', 'gpu']
    assert {x['lane'] for x in Y.wanted(run, cfg, dict(ys, blunders=False), settle=0)} == {'farm', 'gpu'}
    assert Y.bo1_backend(run, cfg, dict(ys, bo1_backend='local'), opps['gen0']) == 'local'
    nopool, ncfg = _run(tmp_path, 'nopool', pool=False)
    assert Y.bo1_lanes(nopool, ncfg, ys) == ['gpu']
    with pytest.raises(ValueError, match='bo1_backend'):
        Y.normalize(dict(Y.DELVER_YARDSTICKS, bo1_backend='mac'))


class Uniform:
    def __call__(self, lb):
        import numpy as np
        cnt = np.diff(lb.opt_off)
        pri = np.repeat((1.0 / np.maximum(cnt, 1)).astype(np.float32), cnt)
        v = np.full((lb.B, 8), 0.5, np.float32)
        return pri, v, np.zeros((lb.B, 8), np.float32)


def _spec():
    return Y.bo1_spec({'eval_spec': mtgz.az.PRESETS['delver-mirror-bo3-dealt']['eval_spec']}, Y.normalize(Y.DELVER_YARDSTICKS))


@pytest.fixture(scope='module')
def traced():
    """6 paired Delver best-of-one games of a traced rolling driver (uniform network, 2 simulations): rows in the
    worker's traced format (A = seat gid % 2)."""
    spec = _spec()
    seat = G.nn_seat('m', 2)
    drv = G.Driver('mtg', [[seat, seat]], spec=spec, concurrency=6, seed=SEED, threads=2, groups=1, rolling=True,
                   paired=True)
    drv.set_trace(True)
    out = {}
    while len(out) < 6:
        drv.resume(6 - len(out), 0)
        drive(drv, {'m': Uniform()})
        rows, texts, _ = drv.take_game_records()
        for r, t in zip(rows, texts):
            row = FE.row(r, [{'net': 1}, {'net': 2}], True)
            row['digest'] = hashlib.sha256(t.encode()).hexdigest()[:16]
            row['rec'] = json.loads(t)
            out[row['gid']] = row
    return spec, out


class Fake:
    """The coordinator's desktop API for one campaign run: results listed from `chunks`, artifacts, ack, accept, PUT."""

    def __init__(self, run):
        self.run, self.objects, self.chunks, self.calls = run, {}, [], []

    def add(self, rows, ids, machine='m1', kind='eval', raw=None):
        body = raw if raw is not None else '\n'.join(json.dumps(r, separators=(',', ':')) for r in rows).encode()
        key = f'results/{self.run}/{len(self.chunks)}'
        self.objects[key] = body
        self.chunks.append({'id': len(self.chunks) + 1, 'key': key, 'lease_id': f'l{len(self.chunks)}', 'seq': 0,
                            'machine_id': machine, 'kind': kind, 'batch': 'c1' if kind == 'eval' else None,
                            'ids': ids, 'dup_ids': [], 'also': [], 'sha256': hashlib.sha256(body).hexdigest()})

    def __call__(self, method, url, headers, data):
        u = urlparse(url)
        self.calls.append((method, u.path, data))
        if method == 'GET' and u.path.endswith('/results'):
            after = int(parse_qs(u.query).get('after', ['0'])[0])
            res = [c for c in self.chunks if c['id'] > after]
            return 200, json.dumps({'results': res, 'cursor': res[-1]['id'] if res else after}).encode()
        if method == 'GET' and u.path.startswith('/v1/admin/artifact/'):
            return 200, self.objects[u.path[len('/v1/admin/artifact/'):]]
        if method == 'GET' and u.path == f'/v1/admin/run/{self.run}':
            return 200, json.dumps({'eval_state': None}).encode()
        return 200, b'{"ok": true}'


def _campaign(tmp_path, spec, games=6, samples=False):
    st = {'run': 'cmp-test', 'games': games, 'batch': 'c1', 'paired': not samples, 'seed': SEED, 'spec': spec,
          'seats': [{'net': 1}, {'net': 1 if samples else 2}], 'spot': 0.5, 'have': [], 'failed': {}, 'machines': {},
          'leases': {}, 'cursor': 0, 'created': 0, 'deadline_s': 1e9, 'build_id': 'b' * 24, 'job':
          {'kind': 'play', 'trace': True, 'samples': samples},
          'gens': {'1': {'checkpoint': 'a.pt', 'sha256': 'x'}}}
    d = str(tmp_path / 'cmp-test')
    C.save_state(d, st)
    return d, st


def test_pull_checks_and_covers_and_the_spot_check_replays(tmp_path, traced):
    spec, rows = traced
    d, st = _campaign(tmp_path, spec)
    fake = Fake('cmp-test')
    cl = FD.Client('https://farm.test', token='t', transport=fake)
    fake.add([rows[0], rows[1], rows[2], rows[3]], [[0, 2]], machine='m1')       # credits 0, 1 only
    fake.add([rows[2], rows[3], rows[0]], [[2, 4]], machine='m2')                # 0 again: the first copy wins
    assert C.pull(cl, d, st) == 4
    assert st['machines'] == {'m1': 2, 'm2': 2}
    cov = C.coverage(st)
    assert cov['done'] == 4 and cov['missing'] == [[4, 6]] and cov['failed'] == []
    assert ('POST', '/v1/admin/run/cmp-test/ack', b'{"cursor": 2}') in fake.calls
    fake.add([rows[4], rows[5]], [[4, 6]], machine='m1')
    C.pull(cl, d, st)
    assert C.missing(st) == []
    spot = C.spot_check(d, st, frac=1.0, engine=E)
    assert spot['checked'] == 6 and spot['by_machine'] == {'m1': 4, 'm2': 2}
    assert C.spot_sample(st, range(400), 0.05) == C.spot_sample(st, range(400), 0.05)
    assert len(C.spot_sample(st, range(400), 0.05)) == 20
    rec = C.describe(C.rows_of(d)[0], spec, E)
    ms = [m for e in rec['episodes'] for m in e['moves']]
    assert len(ms) == len(rows[0]['rec']['steps']) and all(m['t'] for m in ms)
    assert rec['episodes'][-1]['result'] == rows[0]['result']
    assert FE.games_digest(list(C.rows_of(d).values())) == FE.games_digest(list(rows.values()))
    out = C.readout(d, st)
    assert out['games'] == 6 and out['pairs']['n'] == 3


def test_a_tampered_row_refuses_the_campaign(tmp_path, traced):
    spec, rows = traced
    d, st = _campaign(tmp_path, spec)
    fake = Fake('cmp-test')
    cl = FD.Client('https://farm.test', token='t', transport=fake)
    bad = json.loads(json.dumps(rows[0]))
    bad['rec']['steps'][5] = int(bad['rec']['steps'][5]) ^ 1
    fake.add([bad], [[0, 1]])
    with pytest.raises(C.CampaignError, match='digest'):
        C.pull(cl, d, st)
    assert 'digest' in C.load_state(d)['refused']
    # a re-hashed tampered move: the digest holds, the spot check's replay refuses it
    d2, st2 = _campaign(tmp_path / 'b', spec)
    bad['digest'] = FE.trace_digest(bad['rec'])
    fake2 = Fake('cmp-test')
    fake2.add([bad, rows[1]], [[0, 2]])
    C.pull(FD.Client('https://farm.test', token='t', transport=fake2), d2, st2)
    with pytest.raises(C.CampaignError):
        C.spot_check(d2, st2, frac=1.0, engine=E)


def test_coverage_names_failed_and_missing_ranges_and_local_pieces_keep_pairs():
    st = {'games': 10, 'have': [0, 1, 2, 3], 'failed': {'6': 'local worker: boom', '7': 'local worker: boom'},
          'local_ranges': [[2, 4]]}
    cov = C.coverage(st)
    assert cov == {'games': 10, 'done': 4, 'farm': 2, 'local': 2, 'failed': [[[6, 8], 'local worker: boom']],
                   'missing': [[4, 6], [8, 10]]}
    assert C.local_ranges([5, 8, 9], True, chunk=64) == [[4, 6], [8, 10]]
    assert C.local_ranges(list(range(0, 130)), True, chunk=64) == [[0, 64], [64, 128], [128, 130]]
    assert C.slug('Delver Bo3 pb2 y96 gen0') == 'cmp-delver-bo3-pb2-y96-gen0'


def test_self_play_records_are_rows_and_replay_with_samples(tmp_path):
    """The samples option: the farm's self-play records file (header + records) read as campaign rows, accepted,
    replayed with its samples by the spot check."""
    spec = _spec()
    seat = G.nn_seat('m', 2, selfplay=True, sims_fast=1, p_full=0.5)
    drv = G.Driver('mtg', [[seat, seat]], spec=spec, concurrency=3, seed=SEED, record=True, threads=2, groups=1,
                   rolling=True)
    drv.set_trace(True)
    texts = []
    while len(texts) < 3:
        drv.resume(3 - len(texts), 0)
        drive(drv, {'m': Uniform()})
        texts += drv.take_game_records()[1]
    head = {'records': 1, 'game': 'mtg', 'spec': spec, 'aux': False, 'seed': SEED}
    body = gzip.compress(('\n'.join([json.dumps(head)] + texts) + '\n').encode())
    d, st = _campaign(tmp_path, spec, games=3, samples=True)
    fake = Fake('cmp-test')
    fake.add(None, [[0, 3]], kind='selfplay', raw=body)
    assert C.pull(FD.Client('https://farm.test', token='t', transport=fake), d, st) == 3
    assert ('POST', '/v1/admin/run/cmp-test/accept', b'{"gen": 1, "games": 3}') in fake.calls
    assert C.spot_check(d, st, frac=1.0, engine=E)['checked'] == 3
    r = C.rows_of(d)[0]
    assert r['rec']['samples']['step'] and r['seats'] == [{'net': 1}, {'net': 1}]
    body = C.run_body(dict(st, build_game='mtg', runtime=None, priority=0.5, games_per_lease='auto', share=0.25,
                           settings={'spec': spec}))
    assert body['eval'] is None and body['selfplay']['quota'] == 3 and body['selfplay']['share'] == 0.25
    assert body['selfplay']['job'] == {'settings': {'spec': spec}, 'seed': SEED}
