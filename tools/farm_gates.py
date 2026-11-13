"""zero-farm's self-play gates on a run's exact settings, in one command (docs/ENGINE.md 114.8; zero-core docs/az.md 44, 45):

    python tools/farm_gates.py --config <run>/config.json --checkpoint <a checkpoint of the run (.pt)>
        [--worker <mtgz-worker.exe>] [--runtime-dir <runtime pack>] [--model <its zero-onnx/1 graph>]
        [--out DIR] [--games2 8] [--games3 12] [--threads 6] [--infer-threads 4] [--net N] [--set key=JSON ...]

1. **Gate 2, the plumbing** (zero-core tools/worker_parity.py --model): the native worker's self-play job against the
   Python remote worker (zero-core's `remoteplay.Worker`) on the settings - deterministic (groups 1) - with the run's
   network on both sides (OnnxEvaluator through onnxruntime / the graph through the runtime pack's ONNX Runtime: a root
   belief such as the best-of-three's deck tilt needs the auxiliary head, which the uniform evaluator has not): every
   shard array and the first network batches byte-identical.
2. **Gate 3, records** (zero-core tools/worker_records_gate.py): the worker's game records (with a network switch to
   net + 1 after a few games) replayed by the trainer's path (`mtgz._engine.replay_rows`) == the worker's own shards of
   the same games, bit for bit; every game's (episode's) decks compared by the replay.

The checkpoint is exported to zero-onnx/1 here unless `--model` is given (it must be of this build's encoding: a run's
own checkpoint, or a fork's). Prints one JSON summary; exit 0 iff both gates pass. Reads the config and the checkpoint
only (nothing of the run is written).
"""
import argparse
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORE_TOOLS = os.path.join(ROOT, 'third_party', 'zero-core', 'tools')
PACK = 'E:/zero-farm-scratch/phase2/pack/c4c4bb776d7f886f5e13474e'      # the runtime pack c4c4bb77 (ORT 1.31.1)


def worker_default():
    t = os.environ.get('CARGO_TARGET_DIR') or os.path.join(ROOT, 'target')
    for p in (os.environ.get('MTGZ_WORKER', ''), os.path.join(t, 'release', 'mtgz-worker.exe'),
              'E:/cargo-target/mtg-zero/farm/release/mtgz-worker.exe'):
        if p and os.path.exists(p):
            return p
    return None


def run(cmd, log):
    t0 = time.time()
    with open(log, 'w', encoding='utf-8') as f:
        p = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, cwd=ROOT)
    lines = open(log, encoding='utf-8', errors='replace').read().splitlines()
    res = None
    for ln in reversed(lines):
        if ln.startswith('{'):
            try:
                res = json.loads(ln)
                break
            except ValueError:
                pass
    return p.returncode, res, round(time.time() - t0, 1)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--config', required=True)
    ap.add_argument('--checkpoint', required=True)
    ap.add_argument('--worker', default=worker_default())
    ap.add_argument('--runtime-dir', default=PACK)
    ap.add_argument('--model')
    ap.add_argument('--out')
    ap.add_argument('--games2', type=int, default=8)
    ap.add_argument('--games3', type=int, default=12)
    ap.add_argument('--threads', type=int, default=6)
    ap.add_argument('--infer-threads', type=int, default=4)
    ap.add_argument('--net', type=int, help='the rows\' network id (default the checkpoint\'s iteration)')
    ap.add_argument('--set', action='append', default=[], help='key=JSON: a setting changed for both gates')
    a = ap.parse_args(argv)
    if not a.worker:
        ap.error('--worker: no built mtgz-worker found (cargo build --release -p mtgz_worker)')
    out = a.out or os.path.join(os.getcwd(), f'farm-gates-{int(time.time())}')
    os.makedirs(out, exist_ok=True)
    py = sys.executable
    sys.path.insert(0, ROOT)
    sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'python'))
    import mtgz.az  # noqa: F401
    from zero.onnx_export import export_checkpoint, load_checkpoint
    with open(a.config, encoding='utf-8') as f:
        game = json.load(f)['game']
    net = a.net
    if net is None:
        _, info = load_checkpoint(a.checkpoint, game)
        net = int(info.get('checkpoint_iter', 0))
    model = a.model
    if not model:
        model = os.path.join(out, 'model.onnx')
        export_checkpoint(a.checkpoint, model, 'packed', game=game, checkpoint_id=os.path.basename(a.checkpoint))
    dll = os.path.join(a.runtime_dir, 'onnxruntime.dll')
    sets = sum((['--set', s] for s in a.set), [])
    summary = {'config': os.path.abspath(a.config), 'checkpoint': os.path.abspath(a.checkpoint), 'model': os.path.abspath(model),
               'worker': a.worker, 'net': net}
    # gate 2: the plumbing, deterministic (groups 1), the real network on both sides
    rc2, r2, s2 = run([py, os.path.join(CORE_TOOLS, 'worker_parity.py'), '--root', ROOT, '--package', 'mtgz.az', '--config',
                       a.config, '--checkpoint', a.checkpoint, '--worker', a.worker, '--out', os.path.join(out, 'gate2'),
                       '--games', str(a.games2), '--batches', '200', '--set', 'groups=1', '--set', 'shard_rows=8192',
                       '--set', f'conc={a.games2}', *sets, '--threads', str(a.threads), '--iter', str(net + 1),
                       '--model', model, '--ort-dll', dll, '--infer-threads', str(a.infer_threads)],
                      os.path.join(out, 'gate2.log'))
    summary['gate2'] = {'passed': rc2 == 0 and bool(r2 and r2.get('identical')), 'seconds': s2, 'result': r2}
    # gate 3: records -> rows, with a network switch (net + 1) after a quarter of the games
    rc3, r3, s3 = run([py, os.path.join(CORE_TOOLS, 'worker_records_gate.py'), '--worker', a.worker, '--settings', a.config,
                       '--package', 'mtgz.az', '--root', ROOT, '--model', model, '--runtime-dir', a.runtime_dir,
                       '--games', str(a.games3), '--net', str(net), '--threads', str(a.threads), '--conc', str(a.games3),
                       '--groups', '1', '--switch-model', model, '--switch-gen', str(net + 1),
                       '--switch-after', str(max(1, a.games3 // 4)), '--set', 'shard_rows=4096', *sets,
                       '--out', os.path.join(out, 'gate3'), '--json', os.path.join(out, 'gate3.json')],
                      os.path.join(out, 'gate3.log'))
    summary['gate3'] = {'passed': rc3 == 0 and bool(r3 and r3.get('identical')), 'seconds': s3, 'result': r3}
    ok = summary['gate2']['passed'] and summary['gate3']['passed']
    summary['passed'] = ok
    with open(os.path.join(out, 'summary.json'), 'w', encoding='utf-8') as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary))
    print(f'FARM GATES {"PASSED" if ok else "FAILED"}: gate 2 (plumbing) {"passed" if summary["gate2"]["passed"] else "FAILED"}, '
          f'gate 3 (records) {"passed" if summary["gate3"]["passed"] else "FAILED"}; logs in {out}')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
