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

**`--mac`: the macOS worker's gates** (zero-farm PROTOCOL.md 11, docs/ENGINE.md 197; off by default, needs the Mac:
`MTGZ_MAC` in ~/.config/zero/local.env, a clean checkout - tools/mac.sh runs this commit there):

    python tools/farm_gates.py --mac --mac-worker <its path on the Mac, from ~> --mac-runtime <the macOS pack there>
        --config <run>/config.json --model <the run's export: model.onnx> --net N [--games3 12]
        [--fuzz-worker <this PC's fuzz build> --mac-fuzz-worker <the Mac's> --fuzz-mode games --fuzz-first F --fuzz-count N]

a. **Farm parity on the Mac**: tools/farm_compat.py (gate.py's farm-compat step: the worker vs the Python remote worker,
   records -> rows, the features) with the macOS worker, worker and Python driver both on the Mac.
b. **Records across platforms**: the macOS worker's self-play records with the run's network through the macOS pack
   (zero-core tools/worker_records_gate.py on the Mac: its records replayed there == its shards), copied here and
   replayed by this PC's engine the way the trainer's farm pool does (`replay_rows` per credited game: header, replay,
   game seed; a ValueError is a refused record) - 0 refused - and their rows compared with the Mac's own shards
   (worker_records_gate.py --replay-only: every array bit for bit; a difference that is not a refusal is reported,
   e.g. the encoding's squash `ln_1p`, ENGINE.md 197).
c. **Fuzz identity**: one fuzz job (mode, seeds) on this PC's fuzz build and on the Mac's, chunks kept: every seed's
   outcome (pass/fail, its fingerprint, result, turns, decisions) identical.

Prints one JSON summary and "MAC FARM GATES PASSED|FAILED: ..."; exit 0 iff every part run passed.

**`--speed`: the speed gate** (docs/ENGINE.md 199; tools/farm_speed.py's `gate`, whose docstring is the manual): a new
worker build against the previous published release build (or `--baseline`) in interleaved windows (new, old, old,
new) of leaves/s on the reference machines (`--machines cpu,mac,cuda`), only while the machine is idle; fails when the
new build is more than `--tolerance` (5%) slower on any machine or a side errors:

    python tools/farm_gates.py --speed --new <build dir | id> [--baseline <id>] [--machines cpu] [--minutes 3] [--out DIR]

The checkpoint is exported to zero-onnx/1 here unless `--model` is given (it must be of this build's encoding: a run's
own checkpoint, or a fork's). Prints one JSON summary; exit 0 iff both gates pass. Reads the config and the checkpoint
only (nothing of the run is written).
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import local_config  # noqa: E402  (the machine's own settings: ~/.config/zero/local.env, ZERO_BIG_DISK)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORE_TOOLS = os.path.join(ROOT, 'third_party', 'zero-core', 'tools')
# the runtime pack c4c4bb77 (ORT 1.31.1), on the machine's big disk (ZERO_BIG_DISK; else give --runtime-dir)
PACK = local_config.big_disk_path('zero-farm-scratch/phase2/pack/c4c4bb776d7f886f5e13474e')


def worker_default():
    t = os.environ.get('CARGO_TARGET_DIR') or os.path.join(ROOT, 'target')
    for p in (os.environ.get('MTGZ_WORKER', ''), os.path.join(t, 'release', 'mtgz-worker.exe')):     # (this checkout's)
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


# ------------------------------------------------------------------------------------------------ --mac

def mac_host():
    h = os.environ.get('MTGZ_MAC') or local_config.get('MTGZ_MAC')
    if not h:
        raise SystemExit('--mac: no Mac (MTGZ_MAC in ~/.config/zero/local.env, tools/local.env.example)')
    return h


def mac_shell(script, timeout=None):
    """A bash script on the Mac (stdin) -> (exit status, output)."""
    # (bytes: text mode on Windows would send CRLF line ends)
    p = subprocess.run(['ssh', '-o', 'BatchMode=yes', mac_host(), 'bash -s'], input=('set -eo pipefail\n' + script).encode(),
                       capture_output=True, timeout=timeout)
    return p.returncode, (p.stdout + p.stderr).decode('utf-8', 'replace')


def mac_scp(src, dst, back=False):
    args = ['scp', '-q', '-r', '-o', 'BatchMode=yes'] + ([f'{mac_host()}:{src}', dst] if back else [src, f'{mac_host()}:{dst}'])
    p = subprocess.run(args, capture_output=True, text=True)
    if p.returncode:
        raise SystemExit(f'scp {"from" if back else "to"} the Mac: {p.stderr.strip()[-500:]}')


def mac_sh(cmd, log, timeout=None):
    """`cmd` in this commit's checkout on the Mac (tools/mac.sh) -> (exit status, output), the output also to `log`."""
    p = subprocess.run(['bash', os.path.join(ROOT, 'tools', 'mac.sh'), cmd], capture_output=True, text=True, cwd=ROOT,
                       timeout=timeout, encoding='utf-8', errors='replace')
    with open(log, 'w', encoding='utf-8') as f:
        f.write(p.stdout + '\n--- stderr\n' + p.stderr)
    return p.returncode, p.stdout + p.stderr


def last_json(text):
    for ln in reversed(text.splitlines()):
        ln = ln.strip()
        if ln.startswith('{'):
            try:
                return json.loads(ln)
            except ValueError:
                pass
    return None


def replay_refusals(wdir, settings, seed):
    """The farm pool's validation of every record in `wdir` (zero.farm_pool.Pool._chunk: the header's game, features,
    actions, spec and seed; each game replayed by `replay_rows`; its seed its id's) -> {files, games, refused: [...],
    rows}."""
    import glob
    import gzip
    from zero import games as G
    from zero.run import DEFAULTS
    E = G.engine()
    rc = dict(DEFAULTS)
    rc.update(settings)
    game, spec = rc['game'], G.spec_json(rc['spec'])
    info = G.info(game)
    out = {'files': 0, 'games': 0, 'rows': 0, 'refused': []}
    for p in sorted(glob.glob(os.path.join(wdir, '*.rec.jsonl.gz'))):
        out['files'] += 1
        with gzip.open(p, 'rt', encoding='utf-8') as f:
            lines = f.read().splitlines()
        h = json.loads(lines[0])
        want = {'game': game, 'features': info['num_features'], 'actions': info['num_actions'], 'seed': seed}
        got = {k: h.get(k) for k in want}
        if got != want or json.loads(h.get('spec') or '{}') != json.loads(spec):
            out['refused'].append({'file': os.path.basename(p), 'why': f'header {got} (want {want}) or another spec'})
            continue
        for ln in lines[1:]:
            gid = json.loads(ln).get('gid')
            try:
                s, g = E.replay_rows(game, spec, ln, bool(h['aux']))
            except ValueError as e:
                out['refused'].append({'file': os.path.basename(p), 'gid': gid, 'why': str(e)[:500]})
                continue
            if g['seed'] != E.game_seed_of(seed, int(g['gid'])):
                out['refused'].append({'file': os.path.basename(p), 'gid': gid, 'why': 'the seed is not its id\'s'})
                continue
            out['games'] += 1
            out['rows'] += 0 if s is None else len(s['player'])
    return out


def fuzz_outcomes(work):
    """Every seed's outcome in a fuzz job's kept chunks (`chunk-*.progress`: pass with its outcome, fail, side) ->
    {seed: outcome}."""
    import glob
    res = {}
    for p in glob.glob(os.path.join(work, 'chunk-*.progress')):
        for ln in open(p, encoding='utf-8', errors='replace'):
            try:
                v = json.loads(ln)
            except ValueError:
                continue
            if v.get('ev') in ('pass', 'fail', 'side') and v.get('seed') is not None:
                res.setdefault(str(v['seed']), []).append({'ev': v['ev'], **({'outcome': v['outcome']} if 'outcome' in v else {}),
                                                           **({'site': v['site']} if 'site' in v else {})})
    return res


def mac_main(a):
    """--mac (module docs)."""
    out = os.path.abspath(a.out or os.path.join(os.getcwd(), f'mac-farm-gates-{int(time.time())}'))
    os.makedirs(out, exist_ok=True)
    stamp = time.strftime('%Y%m%d-%H%M%S')
    remote = f'mtgz-macfarm/gates/{stamp}'
    rc, txt = mac_shell(f'mkdir -p "$HOME/{remote}"\n')
    if rc:
        raise SystemExit(f'the Mac: {txt[-500:]}')
    with open(a.config, encoding='utf-8') as f:
        settings = json.load(f)
    for s in a.set:
        k, v = s.split('=', 1)
        settings[k] = json.loads(v)
    cfgp = os.path.join(out, 'config.json')
    with open(cfgp, 'w', encoding='utf-8') as f:
        json.dump(settings, f, indent=1)
    mac_scp(cfgp, f'{remote}/config.json')
    summary = {'mac_worker': a.mac_worker, 'mac_runtime': a.mac_runtime, 'remote': f'~/{remote}', 'parts': {}}
    W, H = f'"$HOME/{a.mac_worker}"', '"$HOME/' + remote
    # (a) farm parity on the Mac
    if not a.skip_compat:
        t0 = time.time()
        rc, txt = mac_sh(f'bash tools/build_py.sh >/dev/null && nice -n 15 python tools/farm_compat.py --worker {W} --out {H}/compat"',
                         os.path.join(out, 'a-compat.log'), timeout=3 * 3600)
        line = [ln for ln in txt.splitlines() if ln.startswith('farm compat: ')]
        sm = last_json(txt.split('farm compat: ')[0]) if line else None
        summary['parts']['compat'] = {'passed': rc == 0 and bool(line) and line[-1].startswith('farm compat: PASSED'),
                                      'line': line[-1] if line else txt[-800:], 'seconds': round(time.time() - t0),
                                      'gate2': (sm or {}).get('gate2', {}).get('result'), 'gate3': (sm or {}).get('gate3', {}).get('result')}
        print(f'(a) {summary["parts"]["compat"]["line"][:400]}', flush=True)
    # (b) records across platforms
    if a.model:
        t0 = time.time()
        if a.net is None:
            import onnx
            meta = {p.key: p.value for p in onnx.load(a.model, load_external_data=False).metadata_props}
            a.net = int(meta['checkpoint_iter'])
        mac_scp(a.model, f'{remote}/model.onnx')
        g = a.games3
        rc, txt = mac_sh(f'bash tools/build_py.sh >/dev/null && nice -n 15 python third_party/zero-core/tools/worker_records_gate.py '
                         f'--worker {W} --settings {H}/config.json" --package mtgz.az --root . --model {H}/model.onnx" '
                         f'--runtime-dir "$HOME/{a.mac_runtime}" --games {g} --seed {a.seed} --net {a.net} --threads {a.threads} '
                         f'--conc {g} --groups 1 --switch-model {H}/model.onnx" --switch-gen {a.net + 1} --switch-after {max(1, g // 4)} '
                         f'--set shard_rows=4096 --out {H}/records" --json {H}/records.json"',
                         os.path.join(out, 'b-mac-records.log'), timeout=6 * 3600)
        mac_gate3 = last_json(txt.split('GATE 3')[0]) if 'GATE 3' in txt else None
        mac_ok = rc == 0 and 'GATE 3 PASSED' in txt
        local = os.path.join(out, 'mac-records')
        os.makedirs(local, exist_ok=True)
        mac_scp(f'{remote}/records/worker', local, back=True)
        wdir = os.path.join(local, 'worker')
        sys.path.insert(0, ROOT)
        sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'python'))
        import mtgz.az  # noqa: F401
        ref = replay_refusals(wdir, settings, a.seed)
        rc2, r2, s2 = run([sys.executable, os.path.join(CORE_TOOLS, 'worker_records_gate.py'), '--replay-only', '--worker', a.mac_worker,
                           '--settings', cfgp, '--package', 'mtgz.az', '--root', ROOT, '--model', a.model, '--seed', str(a.seed),
                           '--out', local, '--json', os.path.join(out, 'b-desktop-replay.json')],
                          os.path.join(out, 'b-desktop-replay.log'))
        summary['parts']['records'] = {
            'passed': mac_ok and not ref['refused'] and ref['games'] > 0 and rc2 == 0 and bool(r2 and r2.get('identical')),
            'mac_replay': {k: (mac_gate3 or {}).get(k) for k in ('games', 'rows', 'nets', 'setups', 'arrays', 'identical')},
            'desktop_replay': {'games': ref['games'], 'rows': ref['rows'], 'files': ref['files'], 'refused': len(ref['refused']),
                               'refusals': ref['refused'][:20]},
            'desktop_vs_mac_shards': {k: (r2 or {}).get(k) for k in ('games', 'rows', 'arrays', 'identical', 'different')},
            'seconds': round(time.time() - t0)}
        b = summary['parts']['records']
        print(f'(b) Mac records: Mac replay {"identical" if mac_ok else "FAILED"}; desktop replay {b["desktop_replay"]["games"]} games, '
              f'{b["desktop_replay"]["refused"]} refused; desktop rows vs Mac shards '
              f'{"identical" if (r2 or {}).get("identical") else "DIFFERENT"} ({(r2 or {}).get("arrays")} arrays)', flush=True)
    # (c) fuzz identity
    if a.fuzz_worker and a.mac_fuzz_worker:
        t0 = time.time()
        env = sum((['--env', e] for e in a.fuzz_env), [])
        fw = os.path.join(out, 'fuzz-here')
        shutil.rmtree(fw, ignore_errors=True)
        args = ['fuzz', '--mode', a.fuzz_mode, *env, '--first', str(a.fuzz_first), '--count', str(a.fuzz_count), '--threads',
                str(a.fuzz_threads), '--chunk', str(a.fuzz_chunk), '--keep-chunks']
        p = subprocess.run([a.fuzz_worker, *args, '--work', os.path.join(fw, 'work'), '--out', os.path.join(fw, 'out')],
                           capture_output=True, text=True, cwd=out)
        here = fuzz_outcomes(os.path.join(fw, 'work'))
        q = ' '.join(args)
        rc, txt = mac_shell(f'cd "$HOME/{remote}" && rm -rf fuzz && nice -n 15 "$HOME/{a.mac_fuzz_worker}" {q} --work fuzz/work --out fuzz/out '
                            f'> fuzz.log 2>&1; echo "exit $?"\n', timeout=6 * 3600)
        local = os.path.join(out, 'fuzz-mac')
        shutil.rmtree(local, ignore_errors=True)
        mac_scp(f'{remote}/fuzz', local, back=True)
        mac = fuzz_outcomes(os.path.join(local, 'work'))
        diff = sorted((s for s in set(here) | set(mac) if here.get(s) != mac.get(s)), key=int)
        summary['parts']['fuzz'] = {'passed': p.returncode == 0 and len(here) == a.fuzz_count and here == mac,
                                    'mode': a.fuzz_mode, 'env': a.fuzz_env, 'seeds': [a.fuzz_first, a.fuzz_first + a.fuzz_count],
                                    'here': len(here), 'mac': len(mac), 'different': diff[:20],
                                    'fails': sorted(s for s, v in here.items() if any(x['ev'] != 'pass' for x in v)),
                                    'seconds': round(time.time() - t0)}
        f = summary['parts']['fuzz']
        print(f'(c) fuzz {a.fuzz_mode} seeds [{a.fuzz_first}, {a.fuzz_first + a.fuzz_count}): here {f["here"]}, Mac {f["mac"]} seeds, '
              f'{len(diff)} different', flush=True)
    ok = bool(summary['parts']) and all(x['passed'] for x in summary['parts'].values())
    summary['passed'] = ok
    with open(os.path.join(out, 'summary.json'), 'w', encoding='utf-8') as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary))
    print(f'MAC FARM GATES {"PASSED" if ok else "FAILED"}: ' + '; '.join(f'{k} {"passed" if v["passed"] else "FAILED"}'
                                                                      for k, v in summary['parts'].items()) + f' (logs in {out})')
    return 0 if ok else 1


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    if '--speed' in argv:
        import farm_speed
        return farm_speed.main(['gate'] + [x for x in argv if x != '--speed'])
    if '--mac' in argv:
        ap = argparse.ArgumentParser(description='farm_gates.py --mac (module docs)')
        ap.add_argument('--mac', action='store_true')
        ap.add_argument('--mac-worker', required=True, help='the macOS release worker on the Mac (a path from ~)')
        ap.add_argument('--mac-runtime', help='the macOS runtime pack on the Mac (a directory from ~)')
        ap.add_argument('--config', required=True)
        ap.add_argument('--model', help='(b): the run\'s zero-onnx export (model.onnx)')
        ap.add_argument('--net', type=int, help='(b): the rows\' network id (default the export\'s checkpoint_iter)')
        ap.add_argument('--seed', type=int, default=7785777)
        ap.add_argument('--games3', type=int, default=12)
        ap.add_argument('--threads', type=int, default=6)
        ap.add_argument('--skip-compat', action='store_true')
        ap.add_argument('--fuzz-worker', help='(c): this PC\'s fuzz build')
        ap.add_argument('--mac-fuzz-worker', help='(c): the Mac\'s fuzz build (a path from ~)')
        ap.add_argument('--fuzz-mode', default='games')
        ap.add_argument('--fuzz-env', action='append', default=[])
        ap.add_argument('--fuzz-first', type=int, default=0)
        ap.add_argument('--fuzz-count', type=int, default=400)
        ap.add_argument('--fuzz-chunk', type=int, default=100)
        ap.add_argument('--fuzz-threads', type=int, default=4)
        ap.add_argument('--set', action='append', default=[])
        ap.add_argument('--out')
        return mac_main(ap.parse_args(argv))
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
