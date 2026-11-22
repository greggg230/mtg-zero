"""Results to delverai's Head-to-heads tab (#/h2h; docs/ENGINE.md 193): the `--publish EXPERIMENT` option of
`python -m mtgz.az.h2h`, `python -m mtgz.az.keep_h2h`, `tools/run_yardsticks.py cells-match`, `tools/ab.py`'s
judge and `tools/farm_campaign.py collect` (the last two by default: docs/ENGINE.md 225). The site's own uploader
does the posting (delverai's tools/upload_h2h.py: the record's shape, the sides' labels, the path check, the merge into
the experiment's record); this module finds it, checks before a long measurement starts that the post can work, saves
the result and runs the uploader on it when the measurement ends.

  --publish EXPERIMENT     the experiment's id on the site (h2h.<id>; letters, digits, . _ -)
  --title T                its title; an experiment already on the site keeps its own (read back before the run;
                           a new one needs --title)
  --description D          its description (likewise)
  --a-label / --b-label    the sides' names (default: the uploader's, from the checkpoints' run and generation)
  --publish-key K          the match's key in the experiment (default: from the sides; with cells the keys' prefix)
  --publish-note N         a line under the match
  --publish-link RUN       a run on the site the experiment links to
  --publish-replace        replace a match of the same key with other numbers (or another title / description)
  --publish-dry-run        print the record, post nothing

The uploader: $MTGZ_DELVERAI_UPLOADER, else ~/delverai/tools/upload_h2h.py (the maintainer's checkout of the site,
like the run publisher's key file; tools/delverai_publish.py's ENGINE.md 163). The upload key as the uploader reads
it: $MTGZ_DELVERAI_KEY or ~/delverai/.upload_key. The result is saved under <temp>/mtgz-site-h2h/ before the post; a
post that fails says so loudly (PUBLISH FAILED) with the exact command that retries it, and the tool exits 3 after
printing its result as usual.

Light (no torch): stdlib only.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

UPLOADER_ENV = 'MTGZ_DELVERAI_UPLOADER'
UPLOADER = os.path.join('~', 'delverai', 'tools', 'upload_h2h.py')
KEY_ENV = 'MTGZ_DELVERAI_KEY'
KEY_FILE = os.path.join('~', 'delverai', '.upload_key')
SITE = 'https://delverai.greggg230.com'
NAME_RE = re.compile(r'^[A-Za-z0-9._-]{1,60}$')
EXIT_PUBLISH_FAILED = 3


def add_args(ap):
    """The publishing options on an argparse parser (see the docstring)."""
    g = ap.add_argument_group('publishing to the Head-to-heads tab (docs/ENGINE.md 193)')
    g.add_argument('--publish', default=None, metavar='EXPERIMENT', help="post the result to delverai's h2h.<EXPERIMENT>")
    g.add_argument('--title', default=None, help="the experiment's title (a new experiment needs one)")
    g.add_argument('--description', default=None, help="the experiment's description")
    g.add_argument('--a-label', default=None, help="side A's name on the site")
    g.add_argument('--b-label', default=None, help="side B's name on the site")
    g.add_argument('--publish-key', default=None, help="the match's key (with cells: the keys' prefix)")
    g.add_argument('--publish-note', default=None, help='a line under the match')
    g.add_argument('--publish-link', default=None, help='a run on the site the experiment links to')
    g.add_argument('--publish-replace', action='store_true', help='replace a match of the same key with other numbers')
    g.add_argument('--publish-dry-run', action='store_true', help='print the record, post nothing')
    g.add_argument('--publish-url', default=None, help=argparse.SUPPRESS)
    return ap


def pop_args(argv):
    """argv -> (the publishing options, the rest): for a command whose own parser is not ours (zero.h2h's)."""
    ap = add_args(argparse.ArgumentParser(add_help=False))
    return ap.parse_known_args(argv)


def uploader_path():
    return os.path.expanduser(os.environ.get(UPLOADER_ENV) or UPLOADER)


def has_key():
    return bool(os.environ.get(KEY_ENV)) or os.path.isfile(os.path.expanduser(KEY_FILE))


def site_experiment(exp, url=None):
    """The experiment's record on the site ({title, description, link, matches}) or None (not there yet)."""
    req = urllib.request.Request(f'{(url or SITE).rstrip("/")}/api/progress?run=h2h.{exp}',
                                 headers={'User-Agent': 'mtgz-site-h2h/1'})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return (json.loads(r.read().decode('utf-8')) or {}).get('progress')
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise


def check(o, look_up=True, default_title=None, default_description=None):
    """Before the measurement: the post can work (the uploader there, a key, a valid id, a title: the site's when
    the experiment exists and none was given) - else SystemExit. Fills o.title / o.description / o.publish_link from
    the site's record when they were not given (so a match added later never trips the uploader's 'another title'
    refusal), else from `default_title` / `default_description` (a tool that publishes by default: a new experiment
    named for it, tools/farm_campaign.py collect). Returns o."""
    if not o.publish:
        return o
    if not NAME_RE.match(o.publish):
        raise SystemExit(f'--publish {o.publish!r}: letters, digits, . _ - (at most 60)')
    up = uploader_path()
    if not os.path.isfile(up):
        raise SystemExit(f'--publish: no uploader at {up} (${UPLOADER_ENV}, or the delverai checkout at ~/delverai)')
    if not o.publish_dry_run and not has_key():
        raise SystemExit(f'--publish: no upload key (${KEY_ENV} or {KEY_FILE}): the site would refuse the post')
    have = None
    if look_up and not o.publish_dry_run:
        try:
            have = site_experiment(o.publish, getattr(o, 'publish_url', None))
        except (OSError, ValueError) as e:
            raise SystemExit(f'--publish: could not read h2h.{o.publish} back from the site ({e}): try again, or '
                             f'--publish-dry-run')
    if have:
        o.title = o.title or have.get('title')
        if o.description is None:
            o.description = have.get('description') or ''
        if o.publish_link is None:
            o.publish_link = have.get('link')
    if not have:
        o.title = o.title or default_title
        if o.description is None:
            o.description = default_description
    if not o.title:
        raise SystemExit(f'--publish {o.publish}: a new experiment needs --title')
    return o


def checkpoint_side(ckpt):
    """'.../<run>/models/iter_0235.pt' -> ('<run>', 235); else (its file name without .pt, None)."""
    p = str(ckpt or '').replace('\\', '/')
    m = re.search(r'([^/]+)/models/iter_(\d+)\.pt$', p)
    if m:
        return m.group(1), int(m.group(2))
    m = re.search(r'([^/]+?)_iter_(\d+)\.pt$', p)
    if m:
        return m.group(1), int(m.group(2))
    return re.sub(r'\.pt$', '', p.rsplit('/', 1)[-1]) or None, None


def checkpoint_label(ckpt):
    """A side's plain name from its checkpoint (run and generation; never a path)."""
    run, gen = checkpoint_side(ckpt)
    return f'{run}, gen {gen}' if gen is not None else (run or 'checkpoint')


def slug(x):
    return re.sub(r'[^A-Za-z0-9._-]+', '-', str(x)).strip('-')


def default_key(a, b, prefix=''):
    (ra, ga), (rb, gb) = checkpoint_side(a), checkpoint_side(b)
    k = f"{prefix}{slug(ra)}{'-g%d' % ga if ga is not None else ''}-vs-{slug(rb)}{'-g%d' % gb if gb is not None else ''}"
    return k[:60]


def save_dir():
    d = os.path.join(tempfile.gettempdir(), 'mtgz-site-h2h')
    os.makedirs(d, exist_ok=True)
    return d


def _common(o):
    cmd = ['--experiment', o.publish, '--title', o.title, '--description', o.description or '']
    if o.publish_link:
        cmd += ['--run', o.publish_link]
    if o.publish_note:
        cmd += ['--note', o.publish_note]
    if o.a_label:
        cmd += ['--a-label', o.a_label]
    if o.b_label:
        cmd += ['--b-label', o.b_label]
    if o.publish_replace:
        cmd.append('--replace')
    if o.publish_dry_run:
        cmd.append('--dry-run')
    if getattr(o, 'publish_url', None):
        cmd += ['--url', o.publish_url]
    return cmd


def run_uploader(args, python=None, log=None):
    """Run the uploader with `args` -> its exit code (its output to stderr, so a tool's stdout stays its result).
    A failure: the PUBLISH FAILED line with the command that retries it."""
    log = log or (lambda m: print(m, file=sys.stderr, flush=True))
    cmd = [python or sys.executable, uploader_path()] + list(args)
    p = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace')
    for ln in (p.stdout + p.stderr).splitlines():
        log(f'publish: {ln}')
    if p.returncode != 0:
        log(f'PUBLISH FAILED (exit {p.returncode}); the result is saved - retry: '
            + subprocess.list2cmdline(cmd))
    return p.returncode


def publish_result(res, o, name=None):
    """An h2h result (zero.h2h's / keep_h2h's summary dict) -> the uploader, as one match. -> exit code."""
    f = os.path.join(save_dir(), f'{o.publish}.{slug(name or default_key(res.get("a"), res.get("b")))}.json')
    with open(f, 'w', encoding='utf-8', newline='') as fh:
        fh.write(json.dumps(res) + '\n')
    args = [f] + _common(o) + ['--key', (o.publish_key or name or default_key(res.get('a'), res.get('b')))[:60]]
    return run_uploader(args)


def publish_cells(obj, o, name, overall_only=False):
    """A cells-match result ({summary, rows}) -> the uploader's --from-cells (the overall match and each cell's).
    -> exit code."""
    f = os.path.join(save_dir(), f'{o.publish}.{slug(name)}.cells.json')
    with open(f, 'w', encoding='utf-8', newline='') as fh:
        json.dump(obj, fh)
    args = ['--from-cells', f] + _common(o)
    if o.publish_key:
        args += ['--key', o.publish_key]
    if overall_only:
        args.append('--overall-only')
    return run_uploader(args)
