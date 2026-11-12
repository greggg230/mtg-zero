"""Sparse sample shards (zero-core run setting `shard_sparse`, zero-core docs/az.md 32; ENGINE.md 94) on MTG's own
rows: two Delver mirror self-play games' samples (layout v7's heads, the auxiliary targets off) written dense and
sparse read back the same, and the windows of both are the same; their tails and overrides read back from a sparse
v2 shard (ENGINE.md 125); the presets leave the setting off."""
import numpy as np
import torch

from mtgz.az import data as D
from mtgz.az import games as G
from mtgz.az import protocol
from mtgz.az.infer import UniformEvaluator, drive


def _delver_samples():
    import mtgz.az as A
    seat = G.nn_seat('m', 4, selfplay=True, sims_fast=2, p_full=0.5, temp_moves=10)
    drv = G.Driver('mtg', [[seat, seat]] * 2, spec=A.SPEC_DELVER, concurrency=2, threads=1, seed=5, record=True,
                   max_steps=20000)
    parts = []
    drive(drv, {'m': UniformEvaluator()}, on_batch=lambda lb: parts.append(drv.take_samples()))
    parts.append(drv.take_samples())
    return protocol.concat_samples(parts)


def _chunks(s, n=32):
    """Sample dicts of `n` rows each (as the run's takes), opt_off re-based."""
    off = s['opt_off'].astype(np.int64)
    for a in range(0, len(s['player']), n):
        b = min(len(s['player']), a + n)
        c = {k: v[a:b] for k, v in s.items() if k not in ('opt_off', 'opt_action', 'pi')}
        c.update(opt_off=off[a:b + 1] - off[a], opt_action=s['opt_action'][off[a]:off[b]], pi=s['pi'][off[a]:off[b]])
        yield c


def test_delver_rows_in_sparse_shards(tmp_path):
    info = G.info('mtg')
    F, A = info['num_features'], info['num_actions']
    whole = _delver_samples()
    # Layout v7's rows (ENGINE.md 125): their tails and overrides go only to a sparse v2 shard, read back as they
    # were (below); the dense / sparse comparison is of the heads.
    rows = protocol.ROW_SAMPLE_KEYS
    assert all(k in whole for k in rows) and len(whole['t_seg']) > 0
    v2 = D.ShardWriter(str(tmp_path / 'v2'), 'r00', rows=1 << 16,
                       meta={'game': 'mtg', 'features': F, 'actions': A, 'iter': 1, **D.shard_format({'shard_sparse': 1}, info)})
    v2.add(whole)
    assert v2.close()['samples'] == len(whole['player'])
    (p2,) = D.shard_paths(str(tmp_path / 'v2'))
    back = D.read_shard(p2)
    # (format v2's members: the CSR offsets as `t_ptr` / `ov_ptr`, the tail floats in float16 as the head's)
    for k, w in (('t_off', 't_ptr'), ('t_seg', 't_seg'), ('t_slot', 't_slot'), ('ov_off', 'ov_ptr'), ('ov_opt', 'ov_opt'),
                 ('ov_tok', 'ov_tok')):
        assert np.array_equal(back[w], whole[k]), k
    assert np.array_equal(back['t_val'], whole['t_val'].astype(np.float16))
    s = {k: v for k, v in whole.items() if k not in rows}
    parts = list(_chunks(s))
    paths = {}
    for fmt in ({}, D.shard_format({'shard_sparse': 1})):
        kind = 'sparse' if fmt else 'dense'
        w = D.ShardWriter(str(tmp_path / kind), 'r00', rows=max(64, len(s['player']) // 3),
                          meta={'game': 'mtg', 'features': F, 'actions': A, 'iter': 1, **fmt})
        for p in parts:
            w.add(p)
        out = w.close()
        paths[kind] = (D.shard_paths(str(tmp_path / kind)), out)
    (dp, dout), (sp, sout) = paths['dense'], paths['sparse']
    assert len(dp) == len(sp) >= 2 and dout['samples'] == sout['samples'] == len(s['player'])
    assert sout['bytes'] * 10 < dout['bytes']
    for d, p in zip(dp, sp):
        a, b = D.read_shard(d), D.read_shard(p)
        assert a.keys() == b.keys() and b['meta']['x_format'] == 'sparse' and b['meta']['x_cols'] == F
        for k in a:
            if k != 'meta':
                assert a[k].dtype == b[k].dtype and np.array_equal(a[k], b[k]), k
        assert np.array_equal(a['x'].view(np.uint16), b['x'].view(np.uint16))
        raw = D.read_shard(p, dense=False)
        assert raw['x_idx'].dtype == np.uint16 and 'x' not in raw
        assert 0 < len(raw['x_val']) < 0.05 * a['x'].size
    for sparse in (False, True):
        u = D.Window(sp, F, A, sparse=sparse)
        v = D.Window(dp, F, A, sparse=sparse)
        assert np.array_equal(u.x.view(np.uint16), v.x.view(np.uint16)) and u.nbytes == v.nbytes
        idx = np.random.default_rng(0).integers(u.N, size=64)
        bu, bv = u.batch(idx), v.batch(idx)
        assert all(torch.equal(bu[k], bv[k]) for k in bv)


def test_presets_leave_shard_sparse_off():
    from mtgz.az.run import DEFAULTS, PRESETS
    assert DEFAULTS['shard_sparse'] == 0
    assert all(not p.get('shard_sparse') for p in PRESETS.values())
