"""Farm compatibility smoke (docs/ENGINE.md 155; AGENTS.md "Farm compatibility is required"): the release
`mtgz-worker` that zero-farm's PCs run must play and encode exactly as the Python run does. Every gate tier that
covers engine, encoding or search changes runs this (tools/gate.py's `farm-compat` step, after it builds the worker
from the commit under gate), so a change that breaks the worker fails the gate instead of the farm's leases.

    python tools/farm_compat.py --worker <target>/release/mtgz-worker(.exe) [--preset delver-mirror-bo3-dealt]
        [--games 6] [--threads 2] [--sims 24] [--sims-fast 8] [--out DIR] [--set key=JSON ...]

Both checks use the uniform evaluator (exact arithmetic in Rust and Python, no ONNX Runtime) on the preset's
settings - the default is the Delver best-of-three run's self-play preset (dealt decks, chained episodes, layout v7
tails and pointer overrides) - with the farm runs' row shaping (sparse rows and shards, value-only samples,
resignation), made small and deterministic (groups 1, `--games` games in flight, fewer simulations; no root belief:
the uniform evaluator has no auxiliary head, the auxiliary targets are still recorded):

1. **Gate 2, the plumbing** (zero-core tools/worker_parity.py): the native worker's self-play job against zero-core's
   Python remote worker (`remoteplay.Worker`, the run's own driver) from the same seed - every shard array, the
   shards' format meta and the first network batches (dense features, tails, overrides) byte-identical.
2. **Gate 3, records** (zero-core tools/worker_records_gate.py --uniform): the worker's game records replayed by the
   trainer's path (`mtgz._engine.replay_rows`) == the worker's own shards of the same games, bit for bit, with every
   game's decks (`record_setup`) compared by the replay.

What this smoke does NOT cover (tools/farm_gates.py, run on a farm run's real config and checkpoint before it
attaches the farm, ENGINE.md 114.8): the run's own network on both sides through ONNX Runtime (zero-onnx/1 or /2:
the export, the runtime pack, the root belief through the auxiliary head), the run's own simulation counts, a
network switch inside a lease, and more games.

Prints one JSON summary, then a last line "farm compat: PASSED ..." or "farm compat: FAILED ..."; exit 0 iff both
gates passed.
"""
import argparse
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORE = os.path.join(ROOT, 'third_party', 'zero-core')
CORE_TOOLS = os.path.join(CORE, 'tools')


def worker_default():
    exe = 'mtgz-worker.exe' if os.name == 'nt' else 'mtgz-worker'
    t = os.environ.get('CARGO_TARGET_DIR') or os.path.join(ROOT, 'target')
    p = os.path.join(t, 'release', exe)
    return p if os.path.exists(p) else None


def settings(a):
    """The preset's run settings made small and deterministic (module docs)."""
    sys.path.insert(0, ROOT)
    sys.path.insert(0, os.path.join(CORE, 'python'))
    import mtgz.az  # noqa: F401
    from zero import run as R
    ns = type('A', (), {k: None for k in R.DEFAULTS})()
    cfg = R.resolve_config(ns, {'preset': a.preset})
    cfg.update(conc=a.games, threads=a.threads, groups=1, selfplay_belief_tilt=0.0, selfplay_belief_tilts={},
               shard_rows=8192, sims=a.sims, sims_fast=a.sims_fast, infer_device='cpu',
               # (the farm runs' row shaping, as delver-bo3-pb2 plays: sparse rows and shards, value-only samples of
               # the other seat's view, resignation)
               sparse=1, shard_sparse=1, selfplay_view_samples=0.5, selfplay_resign_k=3, selfplay_resign_t=0.02)
    for s in a.set:
        k, v = s.split('=', 1)
        if k not in cfg:
            raise SystemExit(f'--set {k}: not a run setting')
        cfg[k] = json.loads(v)
    return cfg


def checkpoint(cfg, path):
    """A small network of the settings' layout and auxiliary targets (the remote worker builds its seat from a
    checkpoint; the uniform evaluator then answers instead of it)."""
    import torch
    from zero import games as G
    from zero.net import make_net, save
    from zero.run import aux_settings
    info = G.info(cfg['game'])
    _, aux = aux_settings(cfg, info)
    torch.manual_seed(0)
    m = make_net(info, cfg['game'], cfg.get('arch', 'mlp'), 16, 1, 8, 2, aux=aux or None)
    save(m, path, {'iter': 0})


def last_json(text):
    for ln in reversed(text.splitlines()):
        ln = ln.strip()
        if ln.startswith('{'):
            try:
                return json.loads(ln)
            except ValueError:
                pass
    return None


def run(cmd, log):
    t0 = time.time()
    p = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace', cwd=ROOT)
    with open(log, 'w', encoding='utf-8') as f:
        f.write(p.stdout + '\n--- stderr\n' + p.stderr)
    return p.returncode, last_json(p.stdout), round(time.time() - t0, 1), (p.stdout + p.stderr)[-1500:]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--worker', default=worker_default(), help='the release mtgz-worker built from this checkout')
    ap.add_argument('--preset', default='delver-mirror-bo3-dealt')
    ap.add_argument('--games', type=int, default=6, help='games (best-of-three: matches) each gate plays')
    ap.add_argument('--threads', type=int, default=2)
    ap.add_argument('--sims', type=int, default=24)
    ap.add_argument('--sims-fast', type=int, default=8)
    ap.add_argument('--batches', type=int, default=150, help='gate 2: network batches compared')
    ap.add_argument('--out', help='work directory (default: gates/tmp/farm-compat-<time>)')
    ap.add_argument('--set', action='append', default=[], help='key=JSON: a setting changed for both gates')
    a = ap.parse_args(argv)
    if not a.worker or not os.path.exists(a.worker):
        print(f'farm compat: FAILED - no built release worker ({a.worker!r}: cargo build --release -p mtgz_worker)')
        return 1
    out = os.path.abspath(a.out or os.path.join(ROOT, 'gates', 'tmp', f'farm-compat-{int(time.time())}'))
    os.makedirs(out, exist_ok=True)
    cfg = settings(a)
    cfg_path = os.path.join(out, 'config.json')
    with open(cfg_path, 'w', encoding='utf-8') as f:
        json.dump(cfg, f, indent=1)
    ck = os.path.join(out, 'iter_0000.pt')
    checkpoint(cfg, ck)
    py = sys.executable
    summary = {'preset': a.preset, 'worker': os.path.abspath(a.worker), 'games': a.games, 'sims': [a.sims, a.sims_fast],
               'settings': cfg_path}
    rc2, r2, s2, tail2 = run([py, os.path.join(CORE_TOOLS, 'worker_parity.py'), '--root', ROOT, '--package', 'mtgz.az',
                              '--config', cfg_path, '--checkpoint', ck, '--worker', a.worker, '--out',
                              os.path.join(out, 'gate2'), '--games', str(a.games), '--batches', str(a.batches),
                              '--iter', '1', '--threads', str(a.threads)], os.path.join(out, 'gate2.log'))
    ok2 = rc2 == 0 and bool(r2 and r2.get('identical')) and (r2 or {}).get('rows', 0) > 0
    summary['gate2'] = {'passed': ok2, 'seconds': s2, 'result': r2, **({} if ok2 else {'tail': tail2})}
    rep = os.path.join(out, 'gate3.json')
    rc3, _, s3, tail3 = run([py, os.path.join(CORE_TOOLS, 'worker_records_gate.py'), '--worker', a.worker, '--settings',
                             cfg_path, '--package', 'mtgz.az', '--root', ROOT, '--uniform', '--games', str(a.games),
                             '--seed', '17', '--gid-base', '3', '--threads', str(a.threads), '--out',
                             os.path.join(out, 'gate3'), '--json', rep], os.path.join(out, 'gate3.log'))
    r3 = None
    if os.path.exists(rep):
        with open(rep, encoding='utf-8') as f:
            r3 = json.load(f)
    ok3 = rc3 == 0 and bool(r3 and r3.get('identical')) and (r3 or {}).get('rows', 0) > 0 and \
        (r3 or {}).get('setups') == a.games
    summary['gate3'] = {'passed': ok3, 'seconds': s3, 'result': r3, **({} if ok3 else {'tail': tail3})}
    ok = ok2 and ok3
    summary['passed'] = ok
    with open(os.path.join(out, 'summary.json'), 'w', encoding='utf-8') as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary))
    g2 = (f'gate 2 (worker vs the Python remote worker) {"identical" if ok2 else "DIFFERENT or failed"}: '
          f'{(r2 or {}).get("games", 0)} games, {(r2 or {}).get("rows", 0)} rows, '
          f'{(r2 or {}).get("batches_compared", 0)} batches')
    g3 = (f'gate 3 (records -> trainer rows) {"identical" if ok3 else "DIFFERENT or failed"}: '
          f'{(r3 or {}).get("games", 0)} games, {(r3 or {}).get("rows", 0)} rows, {(r3 or {}).get("setups", 0)} decks compared')
    print(f'farm compat: {"PASSED" if ok else "FAILED"} - {a.preset}: {g2}; {g3} ({out})')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
