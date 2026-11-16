"""The delverai site's matchup-model record of a best-of-three run with the in-run matchup model (docs/ENGINE.md 168.7):
the model's held-out gain over play / draw by version, the network's agreement with the choice table by checkpoint
(tools/matchup_agreement.py), the newest table as a compact list, and the run's self-play matches per hour.

One record per run, a progress record of kind 'matchup' named `matchup.<run>` (delverai worker/index.js: validated
there, read back with GET /api/matchup?run=, never listed with the runs):

    {run: 'matchup.<run>', kind: 'matchup', for_run, updated, bar,             (bar: matchup_table_min_gain, nats a game)
     table_from,                                                               (the first iteration sideboarded by a table)
     versions: [{version, iter, games, val_games, gain, gain_dealt, table}],
     agreement: [{iter, live, prior, top1, keep, main, rough, newest, newest_prior, newest_top1, n}],
     agreement_contexts: {iter, table, contexts: [{game, first, lead, opp_dealt, n, prior, top1, keep, favourite}]},
                                                     (the newest checkpoint against the newest table)
     table: {version, iter, contexts: [{game, first, opp_dealt, lead, in, out}]},                 (the reachable contexts)
     rate: {unit, window_s, points: [[t, total, desktop, mac, farm, games], ...], gaps: [[t0, t1], ...]}}

`agreement[].prior` / `top1` are against the table live for that checkpoint's own iteration (`live`), else (before the
first table) against the first table version; `newest_*` against the newest table at the time of the measurement.

**Matches per hour** (`rate`): one point per iteration end; each iteration's matches (best-of-three contests) are spread
evenly over its run time (its metrics line's `seconds` before its `time`), and a point is the matches of the trailing
hour of RUN time divided by that run time (a gap - a restart or a pause - counts neither games nor time, so it never
reads as a drop; the first hour is the rate so far). Stops: the supervisor log's run exits to the next run start
(their time is no run time), and run time missing between iterations > 60 s; drawn as gaps. Sources: the desktop (`selfplay.games`: this machine's driver), the
farm (the farm pool's games, summed: never a machine's name or id), the Mac (the remote games less the farm's); `games`:
every individual game (the desktop's episodes plus the remote ones). Gaps: run time missing between iterations > 60 s.

    python tools/delverai_matchup.py RUN [--upload] [--url URL] [--key-file F] [--dry-run]
"""
import argparse
import json
import os
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
URL = 'https://delverai.greggg230.com'
KEY_FILE = os.path.join(os.path.expanduser('~'), 'delverai', '.upload_key')
WINDOW = 3600.0
GAP = 60.0
CONTEXT_REACHABLE = [(2, 1, -1), (2, 0, 1), (3, 1, 0), (3, 0, 0)]      # (game, first, lead) without drawn games


def rows(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding='utf-8') as f:
        return [json.loads(x) for x in f if x.strip()]


def mdir(run):
    return os.path.join(run, 'matchup')


def signature(run):
    """What the record is made of (sizes and times): a change means a new upload."""
    out = []
    for p in ('index.jsonl', 'agreement.jsonl', 'used.jsonl'):
        f = os.path.join(mdir(run), p)
        out.append(os.path.getsize(f) if os.path.exists(f) else -1)
    f = os.path.join(run, 'metrics.jsonl')
    out.append(os.path.getsize(f) if os.path.exists(f) else -1)
    return tuple(out)


def _r(x, n=5):
    return None if x is None else round(float(x), n)


def versions_block(run):
    out = []
    for r in rows(os.path.join(mdir(run), 'index.jsonl')):
        out.append({'version': r['version'], 'iter': r['iter'], 'games': r.get('games'), 'val_games': r.get('val_games'),
                    'gain': _r(r.get('gain_nats')),
                    'gain_dealt': _r(r['val_logloss_pd_dealt'] - r['val_logloss_dealt'])
                    if r.get('val_logloss_dealt') is not None and r.get('val_logloss_pd_dealt') is not None else None,
                    'table': bool(r.get('table'))})
    return out


def agreement_block(run):
    """The agreement rows of the newest probe set, one per checkpoint (the newest measurement of each)."""
    rs = rows(os.path.join(mdir(run), 'agreement.jsonl'))
    if not rs:
        return [], None
    probe = rs[-1]['probe']
    by = {}
    for r in rs:
        if r['probe'] == probe:
            by[r['iter']] = r
    out = []
    for it in sorted(by):
        r = by[it]
        a, t = r['all'], r['tables']
        ref = t['live'] or t['first']
        new = a['vs'][t['newest']]
        out.append({'iter': it, 'live': t['live'], 'ref': ref, 'prior': _r(a['vs'][ref]['prior'], 4),
                    'top1': _r(a['vs'][ref]['top1'], 4), 'keep': _r(a['keep'], 4), 'main': _r(a['main'], 4),
                    'rough': _r(a['rough'], 4), 'newest': t['newest'], 'newest_prior': _r(new['prior'], 4),
                    'newest_top1': _r(new['top1'], 4), 'n': r['n'], 'unresolved': a.get('unresolved', 0)})
    last = by[max(by)]
    t = last['tables']
    ref = t['newest']                  # (the table the site lists: the newest)
    ctx = []
    for k, c in last['contexts'].items():
        g, f, lead, o = (int(v) for v in k.split(','))
        if (g, f, lead) not in CONTEXT_REACHABLE:
            continue
        x = {'game': g, 'first': f, 'lead': lead, 'opp_dealt': o, 'n': c['n'], 'prior': _r(c['vs'][ref]['prior'], 4),
             'top1': _r(c['vs'][ref]['top1'], 4), 'keep': _r(c['keep'], 4)}
        if c.get('favourite'):
            x['favourite'] = c['favourite']
        ctx.append(x)
    return out, {'iter': last['iter'], 'table': ref, 'probe': probe, 'contexts': ctx}


def table_block(run):
    """The newest version with a table, its reachable contexts as in / out lists against the main."""
    from mtgz.az import matchup as MU
    for b, p in reversed(MU.versions(run)):
        with open(p, encoding='utf-8') as f:
            v = json.load(f)
        if not v.get('table'):
            continue
        ctx = []
        for d in v['table_detail']:
            g, f_, lead, o = d['context']
            if (g, f_, lead) in CONTEXT_REACHABLE:
                ctx.append({'game': g, 'first': f_, 'lead': lead, 'opp_dealt': o, 'in': d.get('in', {}), 'out': d.get('out', {})})
        return {'version': v['version'] if isinstance(v.get('version'), str) else os.path.basename(p)[:-5],
                'iter': v.get('iter', b), 'contexts': ctx}
    return None


def stops(run):
    """[(t0, t1)]: the run's stops and restarts from its supervisor's log (the run process exited at t0, a run process
    started again at t1; local times of the machine that runs it)."""
    import re
    p = os.path.join(run, 'supervisor.log')
    out, t0 = [], None
    if not os.path.exists(p):
        return out
    with open(p, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = re.match(r'^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d) (.*)$', line)
            if not m:
                continue
            t = time.mktime(time.strptime(m.group(1), '%Y-%m-%d %H:%M:%S'))
            if m.group(2).startswith('mtgz.az.run exited'):
                t0 = t
            elif m.group(2).startswith('started mtgz.az.run') and t0 is not None:
                out.append((t0, t))
                t0 = None
    return out


def _minus(a, b, cuts):
    """[a, b] less the intervals `cuts` -> [(lo, hi), ...]."""
    out = [(a, b)]
    for c0, c1 in cuts:
        nxt = []
        for lo, hi in out:
            if c1 <= lo or c0 >= hi:
                nxt.append((lo, hi))
                continue
            if c0 > lo:
                nxt.append((lo, c0))
            if c1 < hi:
                nxt.append((c1, hi))
        out = nxt
    return out


def rate_block(run, window=WINDOW, gap=GAP):
    """Matches per hour over the trailing hour of run time (module docs)."""
    st = stops(run)
    segs = []                          # (pieces, run seconds, (total, desktop, mac, farm, games), t0, t1)
    for m in rows(os.path.join(run, 'metrics.jsonl')):
        s = m.get('selfplay')
        if not s or not m.get('seconds') or 'time' not in m:
            continue
        rem = s.get('remote') or {}
        farm = sum(int(p.get('games') or 0) for p in (rem.get('pools') or {}).values())
        remote = int(rem.get('games') or 0)
        desk = int(s.get('games') or 0)
        rg = int(((s.get('resign') or {}).get('remote') or {}).get('games') or 0)
        games = int((s.get('episodes') or {}).get('games') or desk) + rg
        t1 = float(m['time'])
        t0 = t1 - float(m['seconds'])
        pieces = _minus(t0, t1, st)
        dur = sum(hi - lo for lo, hi in pieces)
        if dur > 0:
            segs.append((pieces, dur, (desk + remote, desk, remote - farm, farm, games), t0, t1))
    segs.sort(key=lambda x: x[4])
    raw = list(st)
    for i in range(1, len(segs)):
        a, b = segs[i - 1][4], segs[i][3]
        if b - a > gap:
            raw.append((a, b))
    gaps = []
    for a, b in sorted(raw):                       # (overlapping ones merged)
        if gaps and a <= gaps[-1][1]:
            gaps[-1][1] = max(gaps[-1][1], round(b))
        else:
            gaps.append([round(a), round(b)])
    points = []
    for i, sg in enumerate(segs):
        T = sg[4]
        lo = T - window
        tot = [0.0] * 5
        run_s = 0.0
        for pieces, dur, vals, _, _ in segs[:i + 1]:
            ov = sum(max(0.0, min(hi, T) - max(l0, lo)) for l0, hi in pieces)
            if ov <= 0:
                continue
            run_s += ov
            for j in range(5):
                tot[j] += vals[j] * ov / dur
        h = 3600.0 / run_s if run_s > 0 else 0.0
        points.append([round(T)] + [round(v * h, 1) for v in tot])
    return {'unit': 'matches', 'window_s': int(window), 'columns': ['t', 'total', 'desktop', 'mac', 'farm', 'games'],
            'points': points, 'gaps': gaps}


def record(run, bar=None):
    name = os.path.basename(os.path.normpath(run))
    with open(os.path.join(run, 'config.json'), encoding='utf-8') as f:
        cfg = json.load(f)
    agreement, actx = agreement_block(run)
    from matchup_agreement import table_start
    rec = {'run': f'matchup.{name}', 'kind': 'matchup', 'for_run': name, 'updated': int(time.time()),
           'bar': float(bar if bar is not None else cfg.get('matchup_table_min_gain', 0.005)),
           'table_from': table_start(run), 'versions': versions_block(run), 'agreement': agreement,
           'agreement_contexts': actx, 'table': table_block(run), 'rate': rate_block(run), 'iters': []}
    return rec


def upload_key(key_file=None):
    with open(key_file or KEY_FILE, encoding='utf-8') as f:
        return f.read().strip()


def upload(rec, url=URL, key=None, timeout=60):
    body = json.dumps(rec, separators=(',', ':')).encode()
    req = urllib.request.Request(url.rstrip('/') + '/api/progress/upload', data=body, method='POST', headers={
        'User-Agent': 'delverai-matchup/1', 'Content-Type': 'application/json', 'Authorization': f'Bearer {key or upload_key()}'})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def sync(run, url=URL, key=None, log=print):
    rec = record(run)
    upload(rec, url, key)
    a = rec['agreement'][-1] if rec['agreement'] else None
    log(f"{rec['run']}: uploaded {len(rec['versions'])} versions, {len(rec['agreement'])} checkpoints, "
        f"{len(rec['rate']['points'])} rate points" + (f"; iter {a['iter']}: prior {a['prior']} top-1 {a['top1']}" if a else ''))
    return rec


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('run')
    ap.add_argument('--upload', action='store_true')
    ap.add_argument('--url', default=URL)
    ap.add_argument('--key-file', default=None)
    ap.add_argument('--dry-run', action='store_true')
    a = ap.parse_args(argv)
    sys.path.insert(0, ROOT)
    if a.upload and not a.dry_run:
        sync(a.run, a.url, upload_key(a.key_file))
    else:
        print(json.dumps(record(a.run), indent=1))
    return 0


if __name__ == '__main__':
    sys.exit(main())
