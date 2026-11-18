"""Apply the user's review-page verdicts to the canary library (docs/blunders.md 6a, "review").

The review page keeps one document a spot in its database collection `verdicts` (doc id = spot id, body
{verdict: keep | drop | unsure | null, note, updated}). Save the collection as JSON files first (one file a document,
named by its doc id, `:` possibly percent-encoded; a body may sit under `data`), then:

    python tools/delver_review_apply.py VERDICTS_DIR --date YYYY-MM-DD [--kinds K,K] [--label L] [--dry-run]

Only spots not reviewed yet (`review` null) are touched - an earlier verdict on the library is never rewritten. For each:

* `drop`: the spot leaves the library; `dropped[id]` = "the user's audit: drop" (+ the note);
* `keep` / `unsure`: `review` = {verdict, by 'user', date[, note]} (unsure holds the spot: kept, not scored - HELD);
* no verdict (or a cleared one): left as it is, listed as still to review.

A spot whose id changed after the page was made (reclassified into another kind: the id carries the kind) is found
through the library's `moved` map ({old id: new id}): a verdict saved under the old id applies to the spot's new id,
unless the new id has a verdict of its own (that one wins).

A summary goes under `audit_user[<label>]` (default `review_<date>`). A spot to drop or hold that the stored reference
scores include stops the run (re-measure the reference with `library-score --reference` instead of patching means).
Fails loudly when a kind filter matches nothing to apply or a verdict names no library spot."""
import argparse
import collections
import glob
import json
import os
import sys
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import delver_blunders as B  # noqa: E402

VERDICTS = ('keep', 'drop', 'unsure')


def read_verdicts(path):
    """{doc id: body} from a directory of saved documents (searched recursively) or one JSON file holding a list of
    {id, data} / a {doc id: body} map."""
    out = {}
    if os.path.isdir(path):
        for f in glob.glob(os.path.join(path, '**', '*.json'), recursive=True):
            with open(f, encoding='utf-8') as fh:
                d = json.load(fh)
            sid = d.get('id') if isinstance(d.get('id'), str) and ':' in d.get('id', '') else None
            out[sid or urllib.parse.unquote(os.path.basename(f)[:-5])] = d.get('data', d)
        return out
    with open(path, encoding='utf-8') as fh:
        d = json.load(fh)
    if isinstance(d, list):
        return {x['id']: x.get('data', x) for x in d}
    return {k: v.get('data', v) for k, v in d.items()}


def resolve_moved(L, ver):
    """`ver` with the verdicts saved under a moved spot's old id re-keyed to its new id (the library's `moved` map,
    entries whose value is a library spot id); a verdict under the new id itself wins."""
    ids = {sp['id'] for sp in L['spots']}
    moved = {o: n for o, n in (L.get('moved') or {}).items() if n in ids}
    out = {k: v for k, v in ver.items() if k not in moved}
    for k, v in ver.items():
        if k in moved:
            out.setdefault(moved[k], v)
    return out


def apply(L, ver, date, kinds=None, label=None):
    """Apply `ver` to the library `L` in place -> the summary (also stored under audit_user[label])."""
    ver = resolve_moved(L, ver)
    ids = {sp['id'] for sp in L['spots']}
    unknown = sorted(v for v in ver if v not in ids and v not in (L.get('dropped') or {}))
    if unknown:
        raise SystemExit(f'{len(unknown)} verdict(s) name no library spot: {unknown[:5]}')
    ref = set()
    for r in ((L.get('reference') or {}).get('scores') or {}).values():
        ref |= set(r.get('spots') or {})
    dropped = L.setdefault('dropped', {})
    keep, n = [], collections.Counter()
    pending = []
    for sp in L['spots']:
        if sp.get('review') is not None or (kinds and sp['kind'] not in kinds):
            keep.append(sp)
            continue
        v = ver.get(sp['id']) or {}
        verdict = v.get('verdict')
        if verdict not in VERDICTS:
            if verdict is not None:
                raise SystemExit(f'{sp["id"]}: unknown verdict {verdict!r}')
            pending.append(sp['id'])
            keep.append(sp)
            continue
        if verdict in ('drop', 'unsure') and sp['id'] in ref:
            raise SystemExit(f'{sp["id"]}: in the stored reference scores; re-measure the reference instead')
        note = (v.get('note') or '').strip()
        n[verdict] += 1
        if verdict == 'drop':
            dropped[sp['id']] = "the user's audit: drop" + (f' ({note})' if note else '')
            continue
        sp['review'] = {'verdict': verdict, 'by': 'user', 'date': date, **({'note': note} if note else {})}
        keep.append(sp)
    if kinds and not n and not pending:
        raise SystemExit(f'no unreviewed spot of {sorted(kinds)} in the library')
    L['spots'] = keep
    summary = {'verdicts': sum(n.values()), 'kept': n['keep'], 'dropped': n['drop'], 'held': n['unsure'],
               'still_to_review': len(pending)}
    if kinds:
        summary['kinds'] = sorted(kinds)
    if sum(n.values()):
        L.setdefault('audit_user', {})[label or f'review_{date}'] = summary
    return summary, pending


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('verdicts', help='the saved verdicts collection (a directory of documents, or one JSON file)')
    ap.add_argument('--date', required=True, help='the review date recorded on each spot (YYYY-MM-DD)')
    ap.add_argument('--kinds', default='', help='only spots of these kinds (comma-separated)')
    ap.add_argument('--label', default=None, help='the audit_user key for the summary (default review_<date>)')
    ap.add_argument('--library', default=B.LIBRARY_PATH)
    ap.add_argument('--dry-run', action='store_true')
    a = ap.parse_args(argv)
    L = B.load_library(a.library)
    if L is None:
        raise SystemExit(f'{a.library}: no library')
    ver = read_verdicts(a.verdicts)
    if not ver:
        raise SystemExit(f'{a.verdicts}: no verdict documents')
    kinds = {k for k in a.kinds.split(',') if k} or None
    summary, pending = apply(L, ver, a.date, kinds, a.label)
    print(json.dumps(summary), '; in the library:', len(L['spots']))
    if pending:
        print('still to review:', *pending, sep='\n  ')
    if not a.dry_run:
        B.save_library(L, a.library)


if __name__ == '__main__':
    main()
