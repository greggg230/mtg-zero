"""A run's checkpoint evaluated against the yardstick under several move rules (zero-core
docs/search.md 3.3: eval_select 'visits' | 'cq', guard eval_select_min_visits) through the
production evaluation path (`zero.run.Run.evaluate`, the code a run and its eval_remote run), on
scratch mirrors of the run - the run directory is only read (docs/ENGINE.md 57).

  python tools/select_eval.py --run runs/delver1 --iter 26 --rules visits,cq,cq-n2 --games 200 \
      --out E:/exp-select2 [--threads 6 --groups 2 --infer-workers 2 --torch-threads 2]
  python tools/select_eval.py --run runs/delver1 --iter 24 --elo-timing --out E:/exp-select2

Each rule gets the mirror <out>/<run>-<rule>/ (config.json = the run's settings + the rule +
`--games` yardstick games, no random games, everything local on this machine's CPU; the
checkpoint copied once to <out>/ckpt/ and hard-linked). Every rule plays the same seeds (the
run's `7919 x iter + 1`), so game g has the same deal and bot stream under every rule (paired
games; the bot's choices diverge once the network's moves do). One JSON line per rule to
<out>/select_eval.jsonl: the eval_yard row (with its `select`), seconds, the per-game scores,
and the paired comparison against the first rule.

`--elo-timing`: the run's evaluation phase as it is (its eval_random_games, elo_games, elo_opponents
or elo_offsets) against the proposed one (--new-flags, JSON), timed phase by phase (Run.evaluate, the
Elo matches of Run.elo_update; the reference games excluded; both at the 'cq' rule) on this machine:
the ratio says how a remote evaluation's time changes (docs/ENGINE.md 57.5).
Below-normal priority; CPU inference.
"""
import argparse
import json
import math
import os
import shutil
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401  (registers the game package, puts the pinned core on sys.path)
from zero import elo  # noqa: E402
from zero.run import Run, model_path  # noqa: E402


def rule_of(name):
    """'visits' | 'cq' | 'cq-n<k>' -> (eval_select, eval_select_min_visits)."""
    if name == 'visits':
        return 'visits', 0
    if name == 'cq':
        return 'cq', 0
    if name.startswith('cq-n') and name[4:].isdigit():
        return 'cq', int(name[4:])
    raise SystemExit(f'rule {name!r}: visits, cq or cq-n<k>')


def local_cfg(cfg, a):
    """The run's settings for a local scratch evaluation on this machine's CPU."""
    return dict(cfg, eval_remote='', selfplay_remote='', infer_device='cpu', train_device='cpu', threads=a.threads,
                groups=a.groups, infer_workers=a.infer_workers, torch_threads=a.torch_threads)


def ckpt(run, it, out):
    d = os.path.join(out, 'ckpt', os.path.basename(os.path.normpath(run)))
    os.makedirs(d, exist_ok=True)
    dst = os.path.join(d, f'iter_{it:04d}.pt')
    if not os.path.exists(dst):
        shutil.copyfile(model_path(run, it), dst + '.tmp')
        os.replace(dst + '.tmp', dst)
    return dst


def mirror(a, cfg, name, iters):
    d = os.path.join(a.out, f'{os.path.basename(os.path.normpath(a.run))}-{name}')
    if os.path.exists(d):
        shutil.rmtree(d)
    os.makedirs(os.path.join(d, 'models'))
    for it in iters:
        os.link(ckpt(a.run, it, a.out), model_path(d, it))
    with open(os.path.join(d, 'config.json'), 'w', encoding='utf-8') as f:
        json.dump(cfg, f, indent=1)
    return d


def paired(base, rows):
    """Mean score difference vs the first rule over the same games (+- 1 se)."""
    d = [b - a for a, b in zip(base, rows)]
    m = sum(d) / len(d)
    sd = math.sqrt(sum((x - m) ** 2 for x in d) / max(1, len(d) - 1))
    return {'diff': round(m, 4), 'se': round(sd / math.sqrt(len(d)), 4), 'better': sum(x > 0 for x in d),
            'worse': sum(x < 0 for x in d)}


def rules(a, cfg):
    out = open(os.path.join(a.out, 'select_eval.jsonl'), 'a', encoding='utf-8')
    base = None
    for name in a.rules.split(','):
        rule, mv = rule_of(name)
        c = local_cfg(cfg, a)
        c.update(eval_select=rule, eval_select_min_visits=mv, eval_games=a.games, eval_random_games=0)
        d = mirror(a, c, name, [a.iter])
        R = Run(d, c)
        cap = []
        play = R.play_match

        def capture(*args, **kw):
            r = play(*args, **kw)
            cap.append(r)
            return r
        R.play_match = capture
        m = {'iter': a.iter}
        t0 = time.time()
        R.evaluate(a.iter, m)                      # the production evaluation (vs the yardstick only here)
        scores = [r[0] for r in cap[0]]
        rec = {'run': os.path.basename(os.path.normpath(a.run)), 'iter': a.iter, 'rule': name, 'tag': R.eval_tag(),
               'eval_yard': m['eval_yard'], 'seconds': round(time.time() - t0, 1),
               'threads': a.threads, 'per_game': scores}
        if base is None:
            base = scores
        else:
            rec['vs_first'] = paired(base, scores)
        print(json.dumps({k: v for k, v in rec.items() if k != 'per_game'}), flush=True)
        out.write(json.dumps(rec) + '\n')
        out.flush()


def elo_timing(a, cfg):
    """The evaluation phase with the run's settings vs --new-flags, at a.iter (timed)."""
    out = open(os.path.join(a.out, 'select_eval.jsonl'), 'a', encoding='utf-8')
    new = json.loads(a.new_flags)
    for name, extra in (('current', {}), ('proposed', new)):
        c = local_cfg(cfg, a)
        c.update(extra)
        if a.timing_eval_games is not None:
            c['eval_games'] = a.timing_eval_games  # the yardstick games: the same in both (0: timed elsewhere)
        e = max(1, int(c['eval_every']))
        opps = [j for j in elo.opponents(a.iter, e, c.get('elo_offsets', ''), c.get('elo_opponents', 0))
                if j >= 0 and os.path.exists(model_path(a.run, j))]
        c['ref_games'] = 0                         # the reference games: once per pool, not per evaluation
        # both at the 'cq' rule: a rule's new pool starts at a.iter (`since`), so elo_update plays
        # this iteration's Elo matches only (the mirror holds the opponents' checkpoints too)
        c['eval_select'], c['eval_select_min_visits'] = 'cq', 0
        d = mirror(a, c, f'timing-{name}', [a.iter] + opps)
        R = Run(d, c)
        m = {'iter': a.iter}
        t0 = time.time()
        R.evaluate(a.iter, m)
        t1 = time.time()
        # only iteration a.iter's own Elo matches (its opponents' checkpoints are the only others here)
        R.elo_update(a.iter, m)
        t2 = time.time()
        rec = {'mode': 'elo_timing', 'run': os.path.basename(os.path.normpath(a.run)), 'iter': a.iter, 'config': name,
               'settings': {k: c.get(k) for k in ('eval_games', 'eval_random_games', 'elo_games', 'elo_opponents',
                                                   'elo_offsets', 'elo_together', 'eval_select')},
               'opponents': opps, 'eval_s': round(t1 - t0, 1), 'elo_s': round(t2 - t1, 1), 'total_s': round(t2 - t0, 1),
               'eval_yard': m.get('eval_yard'), 'eval_random': m.get('eval_random'), 'elo': m.get('elo'),
               'threads': a.threads}
        print(json.dumps(rec), flush=True)
        out.write(json.dumps(rec) + '\n')
        out.flush()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--run', required=True)
    ap.add_argument('--iter', type=int, required=True)
    ap.add_argument('--rules', default='visits,cq,cq-n2')
    ap.add_argument('--games', type=int, default=200)
    ap.add_argument('--out', required=True)
    ap.add_argument('--threads', type=int, default=6)
    ap.add_argument('--groups', type=int, default=2)
    ap.add_argument('--infer-workers', type=int, default=2)
    ap.add_argument('--torch-threads', type=int, default=2)
    ap.add_argument('--elo-timing', action='store_true')
    ap.add_argument('--timing-eval-games', type=int, default=None,
                    help="--elo-timing: the yardstick games of both (default the run's; 0 = only random + Elo)")
    ap.add_argument('--new-flags', default='{"eval_random_games": 0, "elo_offsets": "1,2,4,8", "elo_games": 25, '
                                           '"eval_select": "cq"}')
    a = ap.parse_args(argv)
    import psutil
    import torch
    psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    torch.set_num_threads(a.torch_threads)
    with open(os.path.join(a.run, 'config.json'), encoding='utf-8') as f:
        cfg = json.load(f)
    os.makedirs(a.out, exist_ok=True)
    (elo_timing if a.elo_timing else rules)(a, cfg)


if __name__ == '__main__':
    main()
