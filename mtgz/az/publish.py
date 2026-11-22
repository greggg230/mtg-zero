"""The run's own delverai publisher (docs/ENGINE.md 163; zero-core docs/az.md 49): the run setting `publish`, the
supervisor's companion hook that turns it into `tools/delverai_publish.py`'s command, and the fork hook.

`publish` is a dict of the publisher's options (None or {} = off):

  title, note        the run's name and a sentence about it on the site
  side               a side run (never the site's default run; default False)
  every              record games of iterations divisible by this (default 8 in the Delver presets)
  games              games (best-of-three: matches) recorded per generation (6)
  census             a best-of-three run's sideboard census starts per recorded generation (200; 0: none)
  device, threads    the recording's inference device ('cpu') and torch threads (4)
  keep_yardstick     the keep-your-60 yardstick's directory (the publisher's --keep-yardstick)
  forks              fork-record files for the lineage (a path or a list; '~' expanded; default
                     ['~/delverai/forks.json'] in the Delver presets)
  loop, progress_every   seconds between passes (300) and between progress posts (60)
  sims, min_iter, state_dir, url, key_file, dry_run   as the publisher's flags of the same names
  combo_every        a metagame run's combo games every N-th generation (the publisher's --combo-every; 0: none)
  for_run            the run directory name the setting was made for: a run of another name that carries it (a
                     launcher that copied another run's flags) publishes as a SIDE run titled after itself instead,
                     said loudly (docs/ENGINE.md 223)

The Delver presets (`delver-mirror`, and so `delver-mirror-bo3` and `delver-mirror-bo3-dealt`) set DELVER_PUBLISH and
the metagame presets (`legacy-meta3`, `legacy-meta3-ddwheel`) META3_PUBLISH (since 2026-11-21, docs/ENGINE.md 223);
the smoke presets leave it off. A run whose setting is off says so loudly at every start (PUBLISH OFF in
supervisor.log and on stderr), and so does tools/fork_run.py for a fork that will not publish: an experiment that
nobody will see is never silent. On the command line `--publish '<json>'` replaces the dict and `--no-publish` turns
it off (`python -m mtgz.az.run` reads it as `--publish {}`); both are saved in config.json like any setting, and a
run that saved neither takes its preset's (a running run started before this setting existed: on at its next start).

The supervisor (tools/supervise.py) asks `companion(run, cfg)` at every start of the run and keeps one publisher per
run, started from the run's own checkout (the engine and layout of its checkpoints), hidden, below normal priority,
its console in <run>/publisher_console.txt; it ends with the run (`--supervised`: the publisher's final pass and
'finished', or 'stopped'). A publisher that cannot start (a bad `publish` dict, no upload key) is said loudly in
supervisor.log and the run goes on: publishing is deliberately never a reason to stop training.

A fork (`tools/fork_run.py --source A --dest B`, the `on_fork` hook): a source that publishes makes the fork publish
as a SIDE run titled after it (unless --set publish=... says otherwise; to make a fork the new main line give it
`--publish '{..., "side": false}'` when it starts), and its fork record (the A/B state files' `done` shape: name,
flag, k, main_before, main_after, raw.a) is appended to the first of its `forks` files, so its charts continue
from the source's lineage.

Light (no torch, no numpy): the supervisor imports it.
"""
import json
import os
import sys
import time

DEFAULT_FORKS = '~/delverai/forks.json'
# the Delver presets' publisher (the main line's options since 2026-11-12: 6 games every 8th generation on the CPU,
# the census of each recorded best-of-three generation)
DELVER_PUBLISH = {'side': False, 'every': 8, 'games': 6, 'census': 200, 'device': 'cpu', 'threads': 4,
                  'forks': [DEFAULT_FORKS]}
# the metagame presets' publisher (docs/ENGINE.md 223): the options legacy-meta3-r1's hand-started publisher ran with
# since 2026-11-17 - 9 games (every pairing once) every 8th generation on the CPU, no combo games, no census (best of
# one) - and no fork-record file (the metagame lineage has none: a fork's charts start from the metrics it copied)
META3_PUBLISH = {'side': False, 'every': 8, 'games': 9, 'combo_every': 0, 'census': 0, 'device': 'cpu', 'threads': 4,
                 'forks': []}

KEYS = {'title': str, 'note': str, 'side': bool, 'every': int, 'games': int, 'census': int, 'device': str,
        'threads': int, 'keep_yardstick': str, 'forks': (str, list), 'loop': (int, float),
        'progress_every': (int, float), 'sims': int, 'min_iter': int, 'state_dir': str, 'url': str, 'key_file': str,
        'dry_run': bool, 'combo_every': int, 'for_run': str}
# the publisher's flags for the plain options (key -> flag)
FLAGS = {'title': '--title', 'note': '--note', 'every': '--every', 'games': '--games', 'census': '--census',
         'device': '--device', 'threads': '--threads', 'keep_yardstick': '--keep-yardstick', 'sims': '--sims',
         'min_iter': '--min-iter', 'state_dir': '--state-dir', 'url': '--url', 'key_file': '--key-file',
         'progress_every': '--progress-every', 'combo_every': '--combo-every'}
KEY_ENV = 'MTGZ_DELVERAI_KEY'
KEY_FILE = os.path.join('~', 'delverai', '.upload_key')
LOG = 'publisher_console.txt'
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def no_publish(argv):
    """`--no-publish` -> `--publish {}` in a run's command line (`python -m mtgz.az.run` does it: the setting off,
    saved in config.json like any flag)."""
    return [x for a in argv for x in (('--publish', '{}') if a == '--no-publish' else (a,))]


def normalize(pub):
    """The `publish` setting -> its dict (validated), or None when off. ValueError for a bad one."""
    if pub is None or pub is False or pub == {}:
        return None
    if not isinstance(pub, dict):
        raise ValueError(f'publish {pub!r}: a JSON object of the publisher\'s options, or {{}} / null = off')
    bad = sorted(set(pub) - set(KEYS))
    if bad:
        raise ValueError(f'publish: unknown option(s) {bad} (known: {sorted(KEYS)})')
    for k, v in pub.items():
        t = KEYS[k]
        if v is None:
            continue
        if (t is int and (isinstance(v, bool) or not isinstance(v, int))) or not isinstance(v, t):
            raise ValueError(f'publish: {k} {v!r} is not of the right type ({t})')
    if isinstance(pub.get('forks'), list) and not all(isinstance(x, str) for x in pub['forks']):
        raise ValueError(f"publish: forks {pub['forks']!r}: paths")
    for k in ('every', 'games'):
        if pub.get(k) is not None and pub[k] < 1:
            raise ValueError(f'publish: {k} {pub[k]} must be >= 1')
    if pub.get('combo_every') is not None and pub['combo_every'] < 0:
        raise ValueError(f"publish: combo_every {pub['combo_every']} must be >= 0")
    return dict(pub)


def forks_of(pub):
    """The fork-record files of a publish dict, '~' expanded."""
    f = pub.get('forks')
    return [os.path.expanduser(x) for x in ([f] if isinstance(f, str) else (f or []))]


def command(run, pub, python=None):
    """The publisher's command line for the run directory `run` (absolute) and the publish dict `pub`."""
    cmd = [python or sys.executable, os.path.join(ROOT, 'tools', 'delverai_publish.py'), run,
           '--loop', str(pub.get('loop') or 300), '--supervised']
    for k, flag in FLAGS.items():
        if pub.get(k) is not None:
            cmd += [flag, str(pub[k])]
    for f in forks_of(pub):
        cmd += ['--forks', f]
    if pub.get('side'):
        cmd.append('--side')
    if pub.get('dry_run'):
        cmd.append('--dry-run')
    return cmd


def check_key(pub):
    """The upload key the publisher will read (mtgz.az.replays.upload_key: key_file, $MTGZ_DELVERAI_KEY, else
    ~/delverai/.upload_key) exists - else ValueError (the supervisor's loud line: the run goes on unpublished)."""
    if pub.get('dry_run'):
        return
    if pub.get('key_file'):
        if not os.path.isfile(os.path.expanduser(pub['key_file'])):
            raise ValueError(f"publish: key_file {pub['key_file']}: no such file")
        return
    if os.environ.get(KEY_ENV) or os.path.isfile(os.path.expanduser(KEY_FILE)):
        return
    raise ValueError(f'publish: no upload key (key_file, ${KEY_ENV} or {KEY_FILE}): the site would refuse every post')


def loud(run, msg):
    """A line the run's people must see: stderr and the run's supervisor.log (the supervisor's own format)."""
    print(f'supervise: {msg}', file=sys.stderr, flush=True)
    try:
        with open(os.path.join(run, 'supervisor.log'), 'a', encoding='utf-8') as f:
            f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}\n")
    except OSError:
        pass


def own_run(run, pub):
    """`pub` for the run directory `run`: a setting made for another run (`for_run`, e.g. a launcher that copied that
    run's flags) publishes as a SIDE run titled after this one, said loudly - never under another run's name, never
    taking the site's default run (docs/ENGINE.md 223)."""
    name = os.path.basename(os.path.normpath(run))
    made_for = pub.get('for_run')
    if not made_for or made_for == name:
        return pub
    loud(run, f'PUBLISH SETTING OF ANOTHER RUN: it was made for {made_for}, this run is {name} - publishing as a side '
              f'run titled "{name}" instead (give this run its own --publish to choose; docs/ENGINE.md 223)')
    out = dict(pub, side=True, title=name, for_run=name)
    out.pop('note', None)
    out.pop('state_dir', None)
    return out


def companion(run, cfg):
    """zero-core's companion hook (GamePackage.companion, docs/az.md 49): the run's publisher, or None when its
    `publish` setting is off (said loudly: PUBLISH OFF). Raises when it cannot run (a bad setting, no key)."""
    pub = normalize(cfg.get('publish'))
    if pub is None:
        loud(run, f'PUBLISH OFF: {os.path.basename(os.path.normpath(run))} will not appear on the Delver AI site (its '
                  f'`publish` setting is {cfg.get("publish")!r}); start it with --publish \'{{"title": "..."}}\' to '
                  f'publish it (docs/ENGINE.md 163, 223)')
        return None
    pub = own_run(run, pub)
    check_key(pub)
    return {'name': 'publisher', 'cmd': command(run, pub), 'log': LOG}


def fork_record(source, dest, it, overrides):
    """A fork's record in the A/B state files' `done` shape (tools/delverai_publish.py fork_registry): not adopted
    (main_after = the source), its changed settings as the flag."""
    src = os.path.abspath(source).replace('\\', '/')
    dst = os.path.abspath(dest).replace('\\', '/')
    flag = ' '.join(f"--{k.replace('_', '-')} {json.dumps(v) if isinstance(v, (dict, list)) else v}"
                    for k, v in sorted(overrides.items()) if k != 'publish')
    return {'name': os.path.basename(dst), 'flag': flag,
            'note': f'tools/fork_run.py --source {os.path.basename(src)} --dest {os.path.basename(dst)} --iter {it} '
                    f'(recorded by the fork, {time.strftime("%Y-%m-%d")})',
            'k': int(it), 'main_before': src, 'main_after': src, 'raw': {'a': f'{dst}/models/iter_{int(it):04d}.pt'}}


def append_record(path, rec):
    """Append `rec` to the `done` list of the fork-record file `path` (created if missing), atomically; a record of
    the same run directory already there is left as it is -> True if appended."""
    path = os.path.expanduser(path)
    data = {}
    if os.path.exists(path):
        with open(path, encoding='utf-8') as f:
            data = json.load(f)
    done = data.setdefault('done', [])
    if any(os.path.normcase(str((d.get('raw') or {}).get('a', ''))) == os.path.normcase(rec['raw']['a']) for d in done):
        return False
    done.append(rec)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    tmp = f'{path}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='') as f:
        json.dump(data, f, indent=1)
        f.write('\n')
    os.replace(tmp, path)
    return True


def on_fork(source, dest, it, cfg, overrides):
    """zero-core's fork hook (tools/fork_run.py --source, docs/az.md 49): a fork of a publishing run publishes as a
    side run titled after itself (unless --set publish=... chose), and its fork record goes to the first of its
    `forks` files (the lineage)."""
    if 'publish' not in cfg:                   # (a source that never saved one: its preset's, as its run would)
        from . import PRESETS
        if 'publish' in PRESETS.get(cfg.get('preset') or '', {}):
            cfg['publish'] = PRESETS[cfg['preset']]['publish']
    try:
        pub = normalize(cfg.get('publish'))
    except ValueError as e:
        print(f'fork_run: the fork\'s publish setting: {e} - left as it is, no fork record', file=sys.stderr)
        return
    name = os.path.basename(os.path.normpath(dest))
    if pub is None:
        print(f'fork_run: PUBLISH OFF - {name} will NOT appear on the Delver AI site: '
              f'{os.path.basename(os.path.normpath(source))}\'s `publish` setting is off ({cfg.get("publish")!r}). '
              f'Start the fork with --publish \'{{"side": true, "title": "..."}}\' (or --set publish=... here) to show '
              f'it (docs/ENGINE.md 163, 223)', file=sys.stderr)
        return
    if 'publish' not in overrides:
        pub = dict(pub, side=True, title=f'{name}: a fork of {os.path.basename(os.path.normpath(source))} at {int(it)}')
        pub.pop('note', None)
        pub.pop('state_dir', None)
    pub['for_run'] = name
    cfg['publish'] = pub
    files = forks_of(pub)
    if files:
        rec = fork_record(source, dest, it, overrides)
        if append_record(files[0], rec):
            print(f'fork record {rec["name"]} (k {rec["k"]}) appended to {files[0]}')
    elif pub.get('forks') == []:
        pass                                   # (no lineage file on purpose: the metagame presets, ab.py's arms)
    else:
        print('fork_run: the fork publishes but names no forks file: its lineage starts at the fork (add a record by '
              'hand, docs/ENGINE.md 163)', file=sys.stderr)
