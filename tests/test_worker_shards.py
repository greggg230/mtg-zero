"""mtgz-worker's shards (zero-farm; zero-core docs/az.md 42, docs/ENGINE.md 114) are taken by zero-core as a remote
worker's: the remote self-play arrival check (`RemotePlay._check`: game / features / actions, the non-finite
guard) accepts every shard, and the training window loads them, sparse and dense. Skipped without a built
worker (`cargo build --release -p mtgz_worker`; env MTGZ_WORKER names the executable)."""
import glob
import json
import os
import subprocess
import threading

import numpy as np
import pytest

import mtgz.az  # noqa: F401  (registers the game package)
from zero import data
from zero import remoteplay as RP
from zero import run as R

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def worker_exe():
    exe = 'mtgz-worker.exe' if os.name == 'nt' else 'mtgz-worker'
    cands = [os.environ.get('MTGZ_WORKER', '')]
    if os.environ.get('CARGO_TARGET_DIR'):
        cands.append(os.path.join(os.environ['CARGO_TARGET_DIR'], 'release', exe))
    cands += [os.path.join(ROOT, 'target', 'release', exe), os.path.join('E:/cargo-target/mtg-zero/farm/release', exe)]
    return next((c for c in cands if c and os.path.exists(c)), None)


WORKER = worker_exe()
pytestmark = pytest.mark.skipif(WORKER is None, reason='mtgz-worker is not built (cargo build --release -p mtgz_worker)')


def settings(sparse):
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': 'mtg-smoke'})
    cfg.update(shard_sparse=int(sparse), shard_rows=400, aux_belief_w=0.25, conc=4, threads=2)
    return cfg


class _Host:
    name = 'farm'

    def log(self, msg):
        raise AssertionError(msg)


@pytest.mark.parametrize('sparse', [1, 0])
def test_worker_shards_pass_the_arrival_check_and_load(tmp_path, sparse):
    cfg = settings(sparse)
    cp = tmp_path / 'config.json'
    cp.write_text(json.dumps(cfg), encoding='utf-8')
    out = tmp_path / 'out'
    p = subprocess.run([WORKER, 'selfplay', '--settings', str(cp), '--uniform', '--games', '3', '--seed', '11', '--net', '5',
                        '--threads', '2', '--out', str(out)], capture_output=True, text=True, encoding='utf-8', timeout=1800)
    assert p.returncode == 0, p.stderr[-2000:]
    events = [json.loads(ln) for ln in p.stdout.splitlines() if ln.strip()]
    end = events[-1]
    assert end['ev'] == 'end' and end['reason'] == 'done' and end['stats']['games'] == 3, end
    paths = sorted(glob.glob(str(out / '*.npz')))
    assert paths and len(paths) == sum(1 for e in events if e['ev'] == 'shard')
    run = R.Run(str(tmp_path / 'run'), cfg)
    host = type('RP', (), {'R': run, 'mu': threading.Lock(), 'fatal': None})()
    rows = 0
    for path in paths:
        status, n, net = RP.RemotePlay._check(host, _Host(), path, 6, os.path.basename(path))
        assert status == 'ok', status
        assert set(np.unique(net)) == {5}
        sh = data.read_shard(path, dense=False)
        assert ('x_val' in sh) == bool(sparse) and 'aux' in sh
        rows += n
    assert rows == end['stats']['rows'] > 0
    info = run.info
    W = data.Window(paths, info['num_features'], info['num_actions'], aux_width=sum(w for _, w in info['aux_layout']))
    assert W.N == rows
    b = W.batch(np.arange(min(rows, 16)))
    assert b is not None
