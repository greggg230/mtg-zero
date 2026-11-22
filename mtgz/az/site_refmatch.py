"""Reference matches on delverai (docs/ENGINE.md 2XX): a run's checkpoints against a fixed checkpoint from OUTSIDE the
run (e.g. pb1@262, generation 262 of delver-bo3-pb1) in one deck's mirror, measured by the cells yardstick
(`tools/run_yardsticks.py one <run> --kind cells --ref LABEL --pair X,X`, or the run's watcher with that reference in
its ladder), shown on the run's page as the record `refmatch.<run>` (kind 'refmatch'; delverai worker/index.js
refmatchProblem). The record is the one delverai's tools/upload_refmatch.py builds, field for field; it is built here,
in this repo, and posted with mtgz.az.publish's key and POST.

Each post replaces the record whole, so it is rebuilt every time from every row on disk for the run:

  <run>/yardsticks.jsonl                  the watcher's rows (it is that file's only writer)
  <run>/yardsticks/refmatch_rows.jsonl    rows measured by hand (`one`) against an outside reference
  extra files                             `run_yardsticks.py refmatch RUN --rows FILE` (e.g. a sidecar's rows.jsonl)

A row counts when it is a 'cells' result (no error) of a mirror pair X,X against a reference this run registered
(below) with that reference's network (its ref_sha); the newest row of a generation wins (its `time`). Per reference
and deck one series (key = the reference's label; `<label>.<deck>` when one reference has several mirror decks).

The human fields live in <run>/yardsticks/refmatch.json, written by the flags (`one` / `refmatch`: --series-label,
--title, --note, --deck-label, --ref-run, --ref-gen, --mark ITER=LABEL, --row-note ITER=TEXT) and kept for the next
post; what nobody gave is derived:

  {"marks": [{"iter", "label"}],
   "refs": {"<ref label>": {"sha": the network's sha256[:12] (registered at its first measurement: a row of another
                                   network under the label is left out, loudly; a new network needs a new label),
                            "ref": {"run", "gen"} (default: from the checkpoint's path, models/iter_N.pt),
                            "label" (the site's name, at most 60; default '<run>@<gen>'),
                            "title", "note" (default: said from the deck, the games and the label),
                            "deck_label" (default: data/decks/<deck>.json's name), "format" ('bo1'),
                            "row_notes": {"<iter>": text}}}}

A record with no row is never posted: RefmatchError (a publish that posts nothing is a failure, said loudly).

Light (no torch, no numpy): the yardsticks' watcher imports it.
"""
import json
import math
import os
import re
import sys
import time

from . import publish as P
from . import site_h2h as S
from . import yardsticks as Y

SITE = S.SITE
META = 'refmatch.json'
HAND_ROWS = 'refmatch_rows.jsonl'
SERIES_KEYS = ('key', 'label', 'title', 'note', 'ref', 'deck', 'deck_label', 'format')   # upload_refmatch.py's order
FORMATS = ('bo1', 'bo3')
MAX_SERIES, MAX_ROWS = 8, 2000                    # worker/index.js REFMATCH_MAX_SERIES / REFMATCH_MAX_ROWS
RUN_RE = re.compile(r'^[A-Za-z0-9._-]{1,64}$')
KEY_RE = re.compile(r'^[A-Za-z0-9._-]{1,64}$')
# worker/index.js PATH_RE: a local file path anywhere in the record (the site refuses one)
PATH_RE = re.compile(r'[A-Za-z]:[\\/]|\\|(^|[\s"\'(])/(Users|home|mnt|tmp|root|var)/|\.pt\b')
FIELDS = ('label', 'title', 'note', 'deck_label', 'format')
LIMITS = {'label': 60, 'title': 200, 'note': 1500, 'deck_label': 60}


class RefmatchError(Exception):
    """The record cannot be built or posted (said loudly by the caller; the rows on disk stand)."""


def run_name(run):
    return os.path.basename(os.path.normpath(run))


def record_name(run):
    return f'refmatch.{run_name(run)}'


def meta_path(run):
    return os.path.join(run, Y.DIR, META)


def hand_rows_path(run):
    return os.path.join(run, Y.DIR, HAND_ROWS)


# ------------------------------------------------------------------------------------------------ outside references
def _inside(path, run):
    try:
        p = os.path.normcase(os.path.realpath(path))
        r = os.path.normcase(os.path.realpath(run))
        return os.path.commonpath([p, r]) == r
    except ValueError:                     # (another drive)
        return False


def ref_entry(run, cfg, ys, label):
    """The ladder entry of cells reference `label` ({'label', 'path', ...}), or None ('iter_NNNN' or unknown)."""
    if re.fullmatch(r'iter_\d+', label or ''):
        return None
    for r in Y.cell_refs(run, cfg, ys):
        if r['label'] == label:
            return r
    return None


def outside_ref(run, cfg, ys, label):
    """Whether cells reference `label` is a network from outside the run: not the gen-over-gen kind, not frozen from
    the run (`cells-freeze` entries carry its iteration), not generation 0 or any file under the run directory."""
    e = ref_entry(run, cfg, ys, label)
    if e is None or e.get('iter') is not None:
        return False
    return not _inside(e['path'], run)


# ------------------------------------------------------------------------------------------------ the human fields
def read_meta(run):
    try:
        with open(meta_path(run), encoding='utf-8') as f:
            m = json.load(f)
    except FileNotFoundError:
        return {'marks': [], 'refs': {}}
    if not isinstance(m, dict):
        raise RefmatchError(f'{meta_path(run)}: not a JSON object')
    m.setdefault('marks', [])
    m.setdefault('refs', {})
    return m


def save_meta(run, meta):
    path = meta_path(run)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f'{path}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='') as f:
        json.dump(meta, f, indent=1)
        f.write('\n')
    os.replace(tmp, path)


def register(meta, label, sha, ckpt=None):
    """Reference `label` with network `sha` (12+ hex) in `meta` (its ref run / gen derived from the checkpoint path
    `ckpt` when first seen) -> meta. RefmatchError: the label was registered for another network."""
    sha = str(sha or '')[:12]
    if not re.fullmatch(r'[0-9a-f]{12}', sha):
        raise RefmatchError(f'reference {label!r}: no network sha ({sha!r})')
    e = meta['refs'].setdefault(label, {})
    if e.get('sha') and e['sha'] != sha:
        raise RefmatchError(f"reference label {label!r} is registered for network {e['sha']} ({Y.DIR}/{META}), this "
                            f"one is {sha}: give the new network its own label")
    e['sha'] = sha
    if 'ref' not in e and ckpt:
        run, gen = S.checkpoint_side(ckpt)
        e['ref'] = {k: v for k, v in (('run', run if run and RUN_RE.match(run) else None), ('gen', gen))
                    if v is not None}
    return meta


def apply_fields(meta, label, given):
    """The flags' fields for reference `label` into `meta` -> meta. `given`: {field: value} with any of FIELDS,
    'ref_run', 'ref_gen', 'marks' [(iter, label)], 'row_notes' [(iter, text)]; None values are not given."""
    e = meta['refs'].setdefault(label, {})
    for k in FIELDS:
        if given.get(k) is not None:
            e[k] = given[k]
    if given.get('ref_run') is not None or given.get('ref_gen') is not None:
        ref = dict(e.get('ref') or {})
        if given.get('ref_run') is not None:
            ref['run'] = given['ref_run']
        if given.get('ref_gen') is not None:
            ref['gen'] = int(given['ref_gen'])
        e['ref'] = ref
    for it, text in given.get('row_notes') or []:
        e.setdefault('row_notes', {})[str(int(it))] = text
    marks = {int(m['iter']): m['label'] for m in meta['marks']}
    for it, text in given.get('marks') or []:
        marks[int(it)] = text
    meta['marks'] = [{'iter': k, 'label': marks[k]} for k in sorted(marks)]
    return meta


def parse_pairs(items, what):
    """['1234=shrink-and-perturb', ...] -> [(1234, 'shrink-and-perturb'), ...]; RefmatchError for a bad one."""
    out = []
    for x in items or []:
        it, sep, text = str(x).partition('=')
        if not sep or not it.strip().isdigit() or not text.strip():
            raise RefmatchError(f'{what} {x!r}: ITER=TEXT')
        out.append((int(it), text.strip()))
    return out


# ------------------------------------------------------------------------------------------------ the rows
def read_rows(paths):
    """Every 'cells' result row (no error, an integer iteration) of the JSONL files `paths` (missing ones skipped; a
    torn line skipped)."""
    out = []
    for path in paths:
        try:
            f = open(path, encoding='utf-8')
        except FileNotFoundError:
            continue
        with f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if (isinstance(r, dict) and r.get('kind') == 'cells' and r.get('error') is None
                        and isinstance(r.get('iter'), int) and isinstance(r.get('cells'), dict)):
                    out.append(r)
    return out


def row_paths(run, extra=()):
    return [Y.rows_path(run), hand_rows_path(run)] + [p for p in extra if p]


def append_hand_row(run, row):
    """A row measured by hand (`one`) against an outside reference, kept for the record's next rebuild."""
    os.makedirs(os.path.join(run, Y.DIR), exist_ok=True)
    Y.append_jsonl(hand_rows_path(run), row)


def pentanomial_se(p):
    """Standard error of the score over deals from a pentanomial [deals with 0, 0.5, 1, 1.5, 2 points of 2 games]
    (delverai tools/upload_refmatch.py's)."""
    n = sum(p)
    if n < 2:
        return None
    xs = [k / 4 for k in range(5)]
    mean = sum(c * x for c, x in zip(p, xs)) / n
    var = sum(c * (x - mean) ** 2 for c, x in zip(p, xs)) / (n - 1)
    return math.sqrt(var / n)


def deck_label(deck):
    """A deck's name on the site: data/decks/<deck>.json's own name (else the file name)."""
    try:
        with open(os.path.join(Y.ROOT, 'data', 'decks', f'{deck}.json'), encoding='utf-8') as f:
            return json.load(f).get('name') or deck
    except (OSError, ValueError):
        return deck


def _deck_short(deck):
    return re.sub(r'_\d+$', '', deck)


def _row(r, cell, notes):
    c = r['cells'][cell]
    se = pentanomial_se(c['pentanomial']) if c.get('pentanomial') else None
    row = {'iter': r['iter'], 'games': int(c['games']), 'score': round(float(c['score']), 4),
           'se': None if se is None else round(se, 4), 'elo': round(float(c['elo']), 1),
           'elo_se': None if c.get('se') is None else round(float(c['se']), 1),
           'unfinished': int(r.get('unfinished') or 0)}
    if r['iter'] in notes:
        row['note'] = notes[r['iter']]
    return row


def series(meta, rows, log=None):
    """The record's series from `rows` for every registered reference -> [series]. A row of a registered label with
    another network is left out and said (`log`)."""
    log = log or (lambda m: print(m, file=sys.stderr, flush=True))
    out = []
    for label in sorted(meta['refs']):
        e = meta['refs'][label]
        by_deck, foreign = {}, 0
        for r in rows:
            pair = r.get('pair') or []
            if r.get('ref') != label or len(pair) != 2 or pair[0] != pair[1]:
                continue
            cell = f'{pair[0]}|{pair[0]}'
            if cell not in r['cells']:
                continue
            if e.get('sha') and str(r.get('ref_sha') or '')[:12] != e['sha']:
                foreign += 1
                continue
            old = by_deck.setdefault(pair[0], {}).get(r['iter'])
            if old is None or (r.get('time') or 0) >= (old.get('time') or 0):
                by_deck[pair[0]][r['iter']] = r
        if foreign:
            log(f"refmatch: {foreign} row(s) labelled {label!r} measured another network than {e.get('sha')}: left out")
        notes = {int(k): v for k, v in (e.get('row_notes') or {}).items()}
        for deck in sorted(by_deck):
            cell = f'{deck}|{deck}'
            raw = [by_deck[deck][k] for k in sorted(by_deck[deck])]
            ref = e.get('ref') or {}
            name = e.get('label') or (f"{ref['run']}@{ref['gen']}" if ref.get('run') and ref.get('gen') is not None
                                      else label)
            dl = e.get('deck_label') if len(by_deck) == 1 and e.get('deck_label') else deck_label(deck)
            fmt = e.get('format') or 'bo1'
            games = int(raw[-1]['cells'][cell]['games'])
            title = e.get('title')
            if title and len(by_deck) > 1:
                title = f'{title} ({dl})'
            note = e.get('note') or (
                f"{'Best of one' if fmt == 'bo1' else 'Best of three'} in the {dl} mirror, {games} games a point (each "
                f"deal played twice, the seats swapped): this run's {dl} against {name}'s, a fixed network from outside "
                f"the run. 50% / 0 Elo = as strong as {name}.")
            cfg = {'key': label if len(by_deck) == 1 else f'{label}.{_deck_short(deck)}'[:64], 'label': name,
                   'title': title or f'{dl} mirror against {name}', 'note': note,
                   'ref': ({k: ref[k] for k in ('run', 'gen') if ref.get(k) is not None} or None), 'deck': deck,
                   'deck_label': dl, 'format': fmt}
            s = {k: cfg[k] for k in SERIES_KEYS if cfg.get(k) is not None}
            s['games'] = games
            if raw[-1].get('sims'):
                s['sims'] = int(raw[-1]['sims'])
            s['rows'] = [_row(r, cell, notes) for r in raw]
            out.append(s)
    return out


def record(run, meta, rows, log=None, now=None):
    """The site's record `refmatch.<run>` (upload_refmatch.py's shape) -> dict, checked as the site checks it.
    RefmatchError: no row at all (nothing would be shown), or what the site would refuse."""
    rec = {'run': record_name(run), 'kind': 'refmatch', 'for_run': run_name(run),
           'updated': time.time() if now is None else now, 'series': series(meta, rows, log), 'iters': []}
    if meta.get('marks'):
        rec['marks'] = meta['marks']
    check(rec)
    return rec


def check(rec):
    """What worker/index.js refmatchProblem refuses, said before the post (RefmatchError); no row at all refused too."""
    f = rec['for_run']
    if not RUN_RE.match(f) or re.match(r'^(h2h|blunders|matchup|refmatch)\.', f) or len(rec['run']) > len('refmatch.') + 80:
        raise RefmatchError(f'{f!r}: not a run name the site accepts ([A-Za-z0-9._-], at most 64)')
    ss = rec['series']
    n = sum(len(s['rows']) for s in ss)
    if not ss or not n:
        raise RefmatchError(f"{rec['run']}: no row to publish - no mirror cells result against a registered outside "
                            f"reference on disk (a publish that posts nothing is refused)")
    if len(ss) > MAX_SERIES:
        raise RefmatchError(f'{rec["run"]}: {len(ss)} series, the site takes at most {MAX_SERIES}')
    keys = set()
    for s in ss:
        if not KEY_RE.match(s['key']) or s['key'] in keys:
            raise RefmatchError(f"series key {s['key']!r}: [A-Za-z0-9._-] (at most 64), once each")
        keys.add(s['key'])
        for k, mx in LIMITS.items():
            v = s.get(k)
            if v is not None and (not isinstance(v, str) or not v.strip() or len(v) > mx):
                raise RefmatchError(f"series {s['key']}: {k} {v!r}: a text of at most {mx} characters")
        if s['format'] not in FORMATS:
            raise RefmatchError(f"series {s['key']}: format {s['format']!r}: 'bo1' or 'bo3'")
        ref = s.get('ref') or {}
        if ref.get('run') is not None and not RUN_RE.match(str(ref['run'])):
            raise RefmatchError(f"series {s['key']}: ref.run {ref['run']!r}: a run name")
        if ref.get('gen') is not None and not (isinstance(ref['gen'], int) and ref['gen'] >= 0):
            raise RefmatchError(f"series {s['key']}: ref.gen {ref['gen']!r}: a generation")
        if len(s['rows']) > MAX_ROWS:
            raise RefmatchError(f"series {s['key']}: {len(s['rows'])} rows, the site takes at most {MAX_ROWS}")
        for r in s['rows']:
            if r['games'] < 1 or not 0 <= r['score'] <= 1 or abs(r['elo']) > 4000:
                raise RefmatchError(f"series {s['key']}, generation {r['iter']}: games / score / elo out of range")
            if r.get('note') is not None and len(r['note']) > 120:
                raise RefmatchError(f"series {s['key']}, generation {r['iter']}: a row note of at most 120 characters")
    for m in rec.get('marks') or []:
        if not isinstance(m.get('label'), str) or not m['label'].strip() or len(m['label']) > 60:
            raise RefmatchError(f'mark {m!r}: {{iter, label (at most 60 characters)}}')
    if len(rec.get('marks') or []) > 20:
        raise RefmatchError('at most 20 marks')
    for x in _strings(rec):
        if PATH_RE.search(x):
            raise RefmatchError(f'{rec["run"]}: a local file path would go to the public site ({x[:80]!r})')
    return rec


def _strings(x):
    """Every string of a JSON value (worker/index.js strings())."""
    if isinstance(x, str):
        yield x
    elif isinstance(x, dict):
        for v in x.values():
            yield from _strings(v)
    elif isinstance(x, list):
        for v in x:
            yield from _strings(v)


# ------------------------------------------------------------------------------------------------ posting
def check_key(key_file=None, dry_run=False):
    """Before a long measurement: the upload key the post will read exists - else RefmatchError."""
    try:
        P.check_key({'key_file': key_file, 'dry_run': dry_run})
    except ValueError as e:
        raise RefmatchError(str(e))


def publish(run, extra=(), url=None, key_file=None, dry_run=False, log=None, rows_log=True):
    """Rebuild `refmatch.<run>` from every row on disk (row_paths + `extra`) and post it whole -> the record.
    RefmatchError: nothing to post, a record the site would refuse, no key, or the post failed. `rows_log`: a line per
    row (the command line's; the watcher logs the post's one line only)."""
    log = log or (lambda m: print(m, file=sys.stderr, flush=True))
    meta = read_meta(run)
    rec = record(run, meta, read_rows(row_paths(run, extra)), log)
    for s in rec['series'] if rows_log else []:
        for r in s['rows']:
            log(f"refmatch {s['key']} gen {r['iter']:5d}: {100 * r['score']:.1f}% +- {100 * (r['se'] or 0):.1f}, "
                f"{r['elo']:+.1f} +- {r['elo_se']} Elo, {r['games']} games")
    if dry_run:
        log(f"refmatch: (dry run) {rec['run']}: " + json.dumps(rec))
        return rec
    key = None
    try:
        key = P.upload_key(key_file)
    except SystemExit as e:
        raise RefmatchError(str(e))
    try:
        ans = P.post(f"{(url or SITE).rstrip('/')}/api/progress/upload", json.dumps(rec).encode('utf-8'), key,
                     timeout=120)
    except Exception as e:                 # noqa: BLE001 - HTTPError (the site refused), URLError, a timeout
        body = ''
        if hasattr(e, 'read'):
            try:
                body = ': ' + e.read().decode('utf-8', 'replace')[:300]
            except Exception:              # noqa: BLE001
                pass
        raise RefmatchError(f"posting {rec['run']} to {url or SITE} failed ({e!r}{body})")
    n = sum(len(s['rows']) for s in rec['series'])
    log(f"refmatch: posted {rec['run']} to {url or SITE}: {len(rec['series'])} series, {n} rows ({ans})")
    return rec


def published_off(run, why):
    """The loud line of a reference match that is not posted."""
    return (f'PUBLISH OFF: {record_name(run)} is not posted ({why}) - this measurement will not appear on the Delver '
            f'AI site until `python tools/run_yardsticks.py refmatch {run}` posts it (docs/ENGINE.md 2XX)')
