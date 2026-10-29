"""EXPERIMENT (branch exp/select-rule, not adopted): the move rule of a non-sampled root decision
(zero-core seat key `select`: 'visits' = argmax (N, P0), today's rule; 'cq' = argmax of the
completed-Q improved policy; 'q' + `select_k` = argmax Q among options with >= k visits) measured
on the Delver mirror run (runs/delver1, read only).

  python tools/exp_select.py yard --iter 12 --rules visits,cq,q2,q3 --games 200 --sims 32
  python tools/exp_select.py h2h  --iter 12 --a cq --b visits --games 200 --sims 32
  python tools/exp_select.py check --iter 12 --games 16           (default-rule digest, base vs new)

The checkpoint plays exactly as in the run's evaluation (`Run.nn_seat` eval seat, the run's
eval_spec and max_steps, seed 7919 * it + 1, seats alternating) plus the `select` keys. Every
rule of one checkpoint uses the same driver seed, so game g has the same deal and bot stream
under every rule (paired games). Mulligans are counted from the driver's move log (zero-core
DriverCfg::move_log, EXPERIMENT): the London mulligan's decisions are the game's first ones
(PlayDraw [2, 3], Mulligan [PASS 0, YES 1], MulliganBottom = HAND ids 36-99), YES = a mulligan;
7 mulligans = a hand kept at 0 cards (the engine keeps it for the player).
Rows go to --out (JSON lines). CPU-light: below-normal priority, --threads search threads.
"""
import argparse
import hashlib
import json
import math
import os
import shutil
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401  (registers the game package, puts the pinned core on sys.path)
from zero import games as G  # noqa: E402
from zero import h2h as H  # noqa: E402
from zero.infer import NetEvaluator, drive  # noqa: E402
from zero.net import load  # noqa: E402
from zero.run import Run  # noqa: E402

RUN = r'C:/Users/Gregg Keithley/mtg-zero/runs/delver1'
CKPT = r'E:/exp-select/ckpt'
HAND = range(36, 100)


def rule_keys(rule, sims):
    """'visits' | 'cq' | 'q<k>' | 'q10pct' -> seat-dict extras ({} for the default)."""
    if rule == 'visits':
        return {}
    if rule == 'visits!':                    # the default rule, set explicitly
        return {'select': 'visits'}
    if rule == 'cq':
        return {'select': 'cq'}
    if rule == 'q10pct':
        return {'select': 'q', 'select_k': max(1, round(0.1 * sims))}
    if rule.startswith('q'):
        return {'select': 'q', 'select_k': int(rule[1:])}
    raise ValueError(rule)


def ckpt(it):
    os.makedirs(CKPT, exist_ok=True)
    dst = os.path.join(CKPT, f'iter_{it:04d}.pt')
    if not os.path.exists(dst):
        shutil.copyfile(os.path.join(RUN, 'models', f'iter_{it:04d}.pt'), dst)
    return dst


def mulligans(moves, n=2):
    """Mulligans taken per seat, from the game's first logged moves."""
    m = [0] * n
    for p, a, legal in moves:
        if legal == [2, 3]:
            continue
        if legal == [0, 1]:
            m[p] += a == 1
            continue
        if legal and all(x in HAND for x in legal):
            continue
        break
    return m


def summarize(rows, me_of):
    n = len(rows)
    w = sum(r['result'][me_of(r)] for r in rows) / n
    mull = [mulligans(r.get('moves', []))[me_of(r)] for r in rows]
    opp = [mulligans(r.get('moves', []))[1 - me_of(r)] for r in rows]
    return {'games': n, 'win': round(w, 4), 'se': round(math.sqrt(max(w * (1 - w), 1e-9) / n), 4),
            'mull_per_hand': round(sum(mull) / n, 3), 'mulled_any': round(sum(1 for x in mull if x) / n, 3),
            'at0': round(sum(1 for x in mull if x >= 7) / n, 3), 'opp_mull_per_hand': round(sum(opp) / n, 3),
            'opp_at0': round(sum(1 for x in opp if x >= 7) / n, 3),
            'rounds': round(sum(r['rounds'] for r in rows) / n, 2),
            'draws': sum(1 for r in rows if r['result'][0] == r['result'][1]),
            'unfinished': sum(1 for r in rows if not r['over']),
            'win_kept7': _cond(rows, me_of, mull, lambda x: x == 0), 'win_at0': _cond(rows, me_of, mull, lambda x: x >= 7)}


def _cond(rows, me_of, mull, f):
    sel = [r['result'][me_of(r)] for r, x in zip(rows, mull) if f(x)]
    return [round(sum(sel) / len(sel), 3) if sel else None, len(sel)]


def setup(a):
    import psutil
    import torch
    torch.set_num_threads(a.torch_threads)
    psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    with open(os.path.join(RUN, 'config.json'), encoding='utf-8') as f:
        cfg = json.load(f)
    cfg['infer_device'] = a.device
    return cfg


def yard(a, cfg, rule, log_moves=True):
    """Checkpoint a.iter vs the run's yardstick, as Run.play_match + the move log. `rule`: one
    rule, or a list: every rule in ONE driver (bigger batches), game g plays rule g % R with the
    network in seat (g // R) % 2 (seats alternate within each rule)."""
    r = Run(RUN, cfg)
    it = a.iter
    rules = [rule] if isinstance(rule, str) else list(rule)
    R = len(rules)
    ms = []
    for ru in rules:
        m = r.nn_seat('m', a.sims)
        m.update(rule_keys(ru, a.sims))
        ms.append(m)
    o = r.opp_seat(cfg['yardstick'], a.sims)
    P = 2
    seats = [[ms[g % R] if s == (g // R) % P else o for s in range(P)] for g in range(a.games * R)]
    mm, _ = load(ckpt(it), a.device)
    evs = {'m': NetEvaluator(mm, a.device)}
    kw = {'move_log': 200} if log_moves else {}
    drv = G.Driver(cfg['game'], seats, spec=cfg['eval_spec'], concurrency=min(a.conc, len(seats)), seed=a.seed or 7919 * it + 1,
                   threads=a.threads, groups=cfg['groups'], max_steps=cfg['max_steps'], **kw)
    t0 = time.time()
    drive(drv, evs)
    rows = sorted(drv.take_results(), key=lambda x: x['gid'])
    return rows, time.time() - t0


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument('mode', choices=['yard', 'h2h', 'check'])
    ap.add_argument('--iter', type=int, required=True)
    ap.add_argument('--rules', default='visits,cq,q2,q10pct')
    ap.add_argument('--a', default='cq')
    ap.add_argument('--b', default='visits')
    ap.add_argument('--games', type=int, default=200)
    ap.add_argument('--sims', type=int, default=32)
    ap.add_argument('--seed', type=int, default=0)
    ap.add_argument('--conc', type=int, default=200)
    ap.add_argument('--threads', type=int, default=6)
    ap.add_argument('--torch-threads', type=int, default=2)
    ap.add_argument('--device', default='cuda')
    ap.add_argument('--base', action='store_true', help='check: the baseline build (no move log, no select keys)')
    ap.add_argument('--out', default=r'E:/exp-select/results.jsonl')
    a = ap.parse_args(argv)
    cfg = setup(a)
    out = open(a.out, 'a', encoding='utf-8')
    if a.mode == 'check':
        rule = 'visits' if a.base else a.rules.split(',')[0]
        rows, sec = yard(a, cfg, rule, log_moves=not a.base)
        key = [(r['gid'], r['result'], r['scores'], r['steps'], r['decisions'], r['rounds'], r['over']) for r in rows]
        dig = hashlib.sha256(json.dumps(key).encode()).hexdigest()[:16]
        rec = {'mode': 'check', 'base': a.base, 'rule': rule, 'iter': a.iter, 'games': a.games, 'sims': a.sims,
               'device': a.device, 'digest': dig, 'seconds': round(sec, 1),
               'steps': sum(r['steps'] for r in rows)}
        print(json.dumps(rec), flush=True)
        out.write(json.dumps(rec) + '\n')
        return
    if a.mode == 'yard':
        rules = a.rules.split(',')
        R = len(rules)
        rows, sec = yard(a, cfg, rules)
        print(f'{len(rows)} games in {sec:.0f} s', flush=True)
        for k, rule in enumerate(rules):
            rr = [r for r in rows if r['gid'] % R == k]
            me = lambda r: (r['gid'] // R) % 2  # noqa: E731
            s = summarize(rr, me)
            rec = {'mode': 'yard', 'iter': a.iter, 'rule': rule, 'sims': a.sims, 'opp': cfg['yardstick'], **s,
                   'seconds_all_rules': round(sec, 1),
                   'per_game': [[r['gid'], r['result'][me(r)], mulligans(r.get('moves', []))[me(r)], r['rounds']] for r in rr]}
            print(json.dumps({k2: v for k2, v in rec.items() if k2 != 'per_game'}), flush=True)
            out.write(json.dumps(rec) + '\n')
            out.flush()
        return
    # h2h: rule a vs rule b, the same checkpoint, seats alternating (zero.h2h.match)
    p = ckpt(a.iter)
    res = H.match(p, p, games=a.games, sims=a.sims, chance_k=cfg['chance_k'], c_puct=cfg['c_puct'], seed=a.seed or 7919 * a.iter + 3,
                  threads=a.threads, groups=cfg['groups'], device=a.device, game=cfg['game'], spec=cfg['eval_spec'],
                  conc=min(a.conc, a.games), seat_a=rule_keys(a.a, a.sims), seat_b=rule_keys(a.b, a.sims),
                  max_steps=cfg['max_steps'], move_log=200, rows=True)
    rows = res.pop('rows')
    s = summarize(rows, lambda r: r['gid'] % 2)          # seat of A
    sb = summarize(rows, lambda r: 1 - r['gid'] % 2)     # seat of B
    rec = {'mode': 'h2h', 'iter': a.iter, 'a_rule': a.a, 'b_rule': a.b, **res,
           'a_mull_per_hand': s['mull_per_hand'], 'a_at0': s['at0'], 'b_mull_per_hand': sb['mull_per_hand'], 'b_at0': sb['at0'],
           'rounds': s['rounds'], 'draws': s['draws']}
    print(json.dumps(rec), flush=True)
    out.write(json.dumps(rec) + '\n')


if __name__ == '__main__':
    main()
