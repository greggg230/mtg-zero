"""Pull a zero-farm fuzz campaign's failures into `failures/` in tools/fuzz_farm.py's format (docs/ENGINE.md
114.3; zero-farm PROTOCOL.md 4 `GET /v1/admin/campaign/{id}/failures`).

    python tools/farm_fuzz_pull.py --campaign ID [--url URL] [--out failures] [--token-file F] [--all]

Each failure friends' PCs reported (seed, mode, site, panic, repro, output tail, build, engine commit,
machine) is recorded as fuzz_farm.py's `record_failure` records the Mac farm's: one file per distinct
failure, `<out>/<fid>.json`, deduplicated by fuzz_farm.py's key (the panic site and its message's first
line with digits masked: `failure_key`, `fid` = its sha1's first 12 hex digits, the same id the Mac farm
gives the same panic), counts / modes / seeds / commits merged into a failure already there, the first 3
occurrences' output in `<fid>-<k>.log`; `expected` from fuzz_farm.py's list. A cursor per campaign
(`<out>/.zero-farm-<campaign>.cursor`) makes a pull take only the failures that arrived since the last
one (`--all`: from the start, counting nothing twice is then up to you). The admin token comes from the
file ~/.config/zero-farm/admin_token (never printed).
"""
import argparse
import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fuzz_farm as FF  # noqa: E402

ROOT = os.path.dirname(HERE)


def get(url, token):
    req = urllib.request.Request(url, headers={'authorization': f'Bearer {token}', 'user-agent': 'mtg-zero farm_fuzz_pull'})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode('utf-8'))


def kind_of(site):
    if site.startswith('crash exit'):
        return 'crash'
    if site.startswith('HANG '):
        return 'hang'
    return 'panic'


def message_of(f):
    """The panic's message lines as fuzz_farm.py's parser reads them out of a job's output (`panics`), or, for
    a crash / hang (no panic), the text's lines."""
    ps = FF.panics(f.get('panic') or '')
    if ps:
        return ps[0][1]
    return [x.strip() for x in (f.get('panic') or '').splitlines() if x.strip()][:40]


def record(out, f, campaign, expected):
    """fuzz_farm.record_failure for one coordinator row -> (fid, new?)."""
    site = f['site']
    msg = message_of(f)
    key = FF.failure_key(site, msg)
    import hashlib
    fid = hashlib.sha1(key.encode()).hexdigest()[:12]
    p = os.path.join(out, f'{fid}.json')
    rec = FF.read_json(p, None)
    t = float(f.get('t') or 0)
    sha = (f.get('engine_commit') or '')[:40]
    new = rec is None
    if new:
        rec = {'id': fid, 'kind': kind_of(site), 'site': site, 'message': msg[:40], 'key': key, 'modes': [], 'count': 0,
               'first_seen': t, 'last_seen': t, 'repro': f.get('repro'), 'sha': sha, 'seeds': [], 'shas': [],
               'expected': any(e in site or any(e in x for x in msg) for e in expected), 'source': 'zero-farm', 'campaigns': [],
               'machines': [], 'builds': []}
    rec['count'] += 1
    rec['first_seen'] = min(rec['first_seen'], t)
    rec['last_seen'] = max(rec['last_seen'], t)
    for k, v in (('modes', f.get('mode')), ('shas', sha[:12]), ('campaigns', campaign), ('machines', f.get('machine_id')),
                 ('builds', f.get('build_id'))):
        rec.setdefault(k, [])
        if v and v not in rec[k]:
            rec[k].append(v)
    if len(rec['seeds']) < 50:
        rec['seeds'].append({'mode': f.get('mode'), 'seed': f.get('seed'), 'sha': sha[:12], 'campaign': campaign,
                             'machine': f.get('machine_id'), 'build': f.get('build_id')})
    if rec['count'] <= 3:
        with open(os.path.join(out, f'{fid}-{rec["count"]}.log'), 'w', encoding='utf-8') as g:
            g.write(f'mode {f.get("mode")}, {sha}, seed {f.get("seed")} (zero-farm campaign {campaign}, machine {f.get("machine_id")}, '
                    f'build {f.get("build_id")})\nrepro: {f.get("repro")}\n\n')
            g.write((f.get('panic') or '') + '\n\n' + '\n'.join((f.get('output_tail') or '').splitlines()[-400:]))
    FF.write_json(p, rec)
    return fid, new


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--campaign', required=True)
    ap.add_argument('--url', default='https://farm.greggg230.com')
    ap.add_argument('--out', default=os.path.join(ROOT, 'failures'))
    ap.add_argument('--token-file', default=os.path.join(os.path.expanduser('~'), '.config', 'zero-farm', 'admin_token'))
    ap.add_argument('--all', action='store_true', help='from the campaign\'s first failure, not the saved cursor')
    a = ap.parse_args()
    with open(a.token_file, encoding='utf-8') as f:
        token = f.read().strip()
    os.makedirs(a.out, exist_ok=True)
    cur_path = os.path.join(a.out, f'.zero-farm-{a.campaign}.cursor')
    cursor = 0 if a.all else int(FF.read_json(cur_path, {'cursor': 0})['cursor'] or 0)
    expected = FF.CONFIG.get('expected') or []
    n, new_ids, seen = 0, [], set()
    while True:
        r = get(f'{a.url.rstrip("/")}/v1/admin/campaign/{a.campaign}/failures?after={cursor}&limit=500', token)
        rows = r.get('failures') or []
        if not rows:
            break
        for f in rows:
            fid, new = record(a.out, f, a.campaign, expected)
            n += 1
            seen.add(fid)
            if new:
                new_ids.append(fid)
        cursor = r.get('cursor') or rows[-1]['id']
        FF.write_json(cur_path, {'cursor': cursor, 'campaign': a.campaign, 'url': a.url})
    print(json.dumps({'campaign': a.campaign, 'failures': n, 'distinct': len(seen), 'new': new_ids, 'cursor': cursor, 'out': a.out}))
    for fid in sorted(seen):
        rec = FF.read_json(os.path.join(a.out, f'{fid}.json'), {})
        print(f'  [{fid}] x{rec.get("count")} {rec.get("kind")} {",".join(rec.get("modes") or [])} at {rec.get("site")}')
        print(f'      repro: {rec.get("repro")}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
