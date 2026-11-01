"""`python -m mtgz.az.replays`: games of a checkpoint, recorded for the replay viewer of
delverai.greggg230.com (zero-core `zero.replay`, its docs/replays.md; the state at every step is
the engine's spectator view, engine/src/az/view.rs: both hands, the board, the stack, the
pending decision's options in words, the game log's tail).

  python -m mtgz.az.replays runs/delver1/models/iter_0012.pt [--games 6] [--selfplay]
      [--sims N] [--device cuda] [--upload [URL]] [--key-file F]

The checkpoint plays itself, seats alternating between games. Defaults come from the
checkpoint's run (`runs/<run>/config.json`): its spec with `"log": true` (the engine's
play-by-play), its self-play simulations (`sims`) and `chance_k`. Evaluation settings by
default (no noise, the most visited option: how the network plays); `--selfplay` = the run's
self-play noise and temperature for the first `temp_moves` decisions (what training sees).

A checkpoint with the belief head (run setting `aux_belief_w`, docs/ENGINE.md 66) adds to every network
decision's frame `belief: {for, n, cards: [[name, expected count], ...]}` - the decider's prediction of
its opponent's hand, the 12 likeliest cards (`mtgz.az.belief.render`, the package's `replay_aux`); its
seat lists `aux: ["belief"]`, and the listing record (`summary`) says `belief: true`.

Files: `runs/<run>/replays/<stamp>_<k>.json.gz` (format zero-replay/1). `--upload` posts each to
`<url>/api/replays/upload` (default URL `SITE`) with the key from `--key-file`, the environment
variable `MTGZ_DELVERAI_KEY` or `~/delverai/.upload_key` (the site's Worker secret UPLOAD_KEY).
"""
import argparse
import base64
import gzip
import json
import os
import random
import urllib.request

import mtgz.az  # noqa: F401  (registers the game package)
from zero import replay as core

SITE = 'https://delverai.greggg230.com'
KEY_ENV = 'MTGZ_DELVERAI_KEY'
KEY_FILE = os.path.join(os.path.expanduser('~'), 'delverai', '.upload_key')


def spec_with_log(spec):
    """The spec JSON with the engine's log on (`"log": true`, engine/src/az/mod.rs)."""
    s = json.loads(spec) if spec else {}
    s['log'] = True
    return json.dumps(s, separators=(',', ':'))


# A minor decision (which mana source to tap next, which mana to keep: the engine's
# `minor_decision`, engine/src/az/mod.rs) in the spectator view's JSON (serde_json sorts keys; only
# the decision has a "kind").
_MINOR = ('"kind":"PayMana"', '"kind":"PayPool"')


class _Recorder(core.RecordingDriver):
    """zero.replay's driver with self-play's budget for minor decisions: `sims_minor`
    simulations (zero-core searches them so in self-play and never records them). The Python
    driver has no `sims_minor`; without it a payment's decisions got the full budget."""
    sims_minor = None

    def _start(self, slot, seat, p):
        super()._start(slot, seat, p)
        if self.sims_minor and any(k in slot.game.export_state() for k in _MINOR):
            slot.target = self.sims_minor
            if getattr(seat, 'gumbel', False):
                # a Gumbel search's schedule is its simulations' (zero-core docs/search.md 3.7): the
                # minor budget's, its noise kept
                t = slot.trees[p].t
                noise = list(t.gumbel_noise) if hasattr(t, 'gumbel_noise') else list(t.gumbel.g)
                t.set_gumbel(seat.gumbel_m, slot.target, noise, seat.gumbel_c_visit, seat.gumbel_c_scale)


def check_encoding(ckpt):
    """The checkpoint's network reads this build's encoding - zero-core's encoding guard
    (`zero.net.check_checkpoint`, its docs/az.md 10.4: the game, the input and action widths, the
    token layout's segments and bags, the layout version when both carry one; docs/ENGINE.md 61),
    which every zero-core load runs too: a layout v4 checkpoint run by a v5 build - or the reverse -
    is misread without any error (the network takes the first floats it was given, ENGINE.md 60).
    Raises zero.net.EncodingMismatch (a ValueError) naming both; recording such games would show
    moves the network never chose. Checked here first so that nothing is played or written."""
    from zero.net import check_checkpoint
    check_checkpoint(ckpt)


def leaf_view(cfg, selfplay=False):
    """The leaf values' view the recorded games search with (docs/ENGINE.md 67): root whenever
    the run trains with root view (`selfplay_leaf_value_view`), whatever its evaluation's setting -
    a run trained and judged with root-view search plays that way; its mover-view values are the
    biased ones the change fixed. Otherwise the run's own (eval_ / selfplay_leaf_value_view)."""
    if cfg.get('selfplay_leaf_value_view') == 'root':
        return 'root'
    return cfg.get(('selfplay' if selfplay else 'eval') + '_leaf_value_view', 'mover')


def root_search(cfg, selfplay=False):
    """The root search the recorded games use (docs/ENGINE.md 68), by `leaf_view`'s rule: the Gumbel
    root whenever the run trains with it (`selfplay_root_search`), whatever its evaluation's setting;
    otherwise the run's own (eval_ / selfplay_root_search)."""
    if cfg.get('selfplay_root_search') == 'gumbel':
        return 'gumbel'
    return cfg.get(('selfplay' if selfplay else 'eval') + '_root_search', 'puct')


def record(ckpt, games=6, sims=None, selfplay=False, device=None, seed=None, conc=8, title=None, log=print,
           lookahead=None):
    """`games` games of `ckpt` against itself, with the run's settings (its leaf values' view:
    `leaf_view`; each network seat names it as `leaf_value_view`). `lookahead` (settings): the
    checkpoint's lookahead seat against its MCTS evaluation seat (zero.replay's lookahead seat,
    docs/ENGINE.md 80.11): the run's evaluation simulations by default and the evaluation seat's
    minor decisions (no `sims_minor`: as zero.h2h's seats). -> zero-replay/1 dicts."""
    check_encoding(ckpt)
    run, it = core.run_of(ckpt)
    cfg = core.run_config(run)
    view = leaf_view(cfg, selfplay)
    rs = root_search(cfg, selfplay)
    if not cfg.get('spec'):
        raise SystemExit(f'{ckpt}: no run config with a spec next to it ({run or "no run directory"})')
    if device is None:
        import torch
        device = 'cuda' if torch.cuda.is_available() else 'cpu'
    seed = random.randrange(1 << 30) if seed is None else seed
    driver = type('Recorder', (_Recorder,), {'sims_minor': None if lookahead else cfg.get('sims_minor')})
    real, core.RecordingDriver = core.RecordingDriver, driver   # core.record builds its driver by this name
    # core.record's seats read the view and the root search from the run's config (zero.replay._nn):
    # give it these
    side = 'selfplay' if selfplay else 'eval'
    real_cfg, core.run_config = core.run_config, lambda r: dict(real_cfg(r), **{side + '_leaf_value_view': view,
                                                                                side + '_root_search': rs})
    try:
        dsims = cfg.get('eval_sims', 32) if lookahead else cfg.get('sims', 64)
        reps = core.record(ckpt, opp='self', games=games, sims=sims or dsims,
                           spec=spec_with_log(cfg['spec']), selfplay=selfplay, hint=None,
                           chance_k=cfg.get('chance_k', 0), device=device, seed=seed, title=title, conc=conc, log=log,
                           lookahead=lookahead)
    finally:
        core.RecordingDriver = real
        core.run_config = real_cfg
    for rep in reps:
        for s in rep['seats']:
            if s.get('kind') == 'nn':
                s['leaf_value_view'] = view
    return reps


def save_json(rep, out_dir, name=None):
    """One replay as a plain JSON file `<out_dir>/<stamp>_<k>.json` (or `name`) -> its path."""
    import datetime
    os.makedirs(out_dir, exist_ok=True)
    stamp = datetime.datetime.fromtimestamp(rep['created']).strftime('%Y%m%d-%H%M%S')
    path = os.path.join(out_dir, name or f'{stamp}_{rep["game_index"]:03d}.json')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(rep, f, separators=(',', ':'))
    return path


def summary(rep):
    """The site's listing record of a replay: who played, who won, how, and how long."""
    final = rep['states'][rep['final']] if rep['states'] else {}
    first = rep['states'][0] if rep['states'] else {}
    res = rep.get('result')
    winner = None
    if res and max(res) > min(res):
        winner = res.index(max(res))
    src = rep.get('source') or {}
    return {'format': rep['format'], 'title': rep['title'], 'group': rep['group'], 'created': rep['created'],
            'run': src.get('run'), 'iter': src.get('iter'), 'game_index': rep['game_index'],
            'seats': [s['name'] for s in rep['seats']], 'sims': rep['seats'][0].get('sims'),
            'selfplay': rep['selfplay'], 'winner': winner, 'draw': bool(res) and winner is None,
            'reason': final.get('reason'), 'turns': final.get('turn', rep.get('rounds')),
            'life': [p.get('life') for p in final.get('players', [])],
            'starting': final.get('starting', first.get('starting')),
            'mulligans': [p.get('mulligans') for p in final.get('players', [])],
            'frames': len(rep['frames']), 'over': rep['over'],
            'belief': any('belief' in (s.get('aux') or []) for s in rep['seats'])}


def public(rep):
    """The replay without this machine's paths (checkpoints by file name)."""
    rep = dict(rep, seats=[dict(s) for s in rep['seats']], source=dict(rep.get('source') or {}))
    for s in rep['seats'] + [rep['source']]:
        if s.get('checkpoint'):
            s['checkpoint'] = os.path.basename(s['checkpoint'])
    return rep


def upload(rep, url, key, timeout=60):
    """POST one replay to the site (`<url>/api/replays/upload`). -> the site's answer (dict)."""
    rep = public(rep)
    gz = gzip.compress(json.dumps(rep, separators=(',', ':')).encode('utf-8'))
    body = json.dumps({'meta': summary(rep), 'gz': base64.b64encode(gz).decode('ascii')}).encode('utf-8')
    return post(url.rstrip('/') + '/api/replays/upload', body, key, timeout)


def post(url, body, key, timeout=60):
    req = urllib.request.Request(url, data=body, method='POST',
                                 headers={'Content-Type': 'application/json', 'Authorization': f'Bearer {key}',
                                          'User-Agent': 'mtgz-delverai/1'})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode('utf-8'))


def upload_key(key_file=None):
    """The site's upload key: `key_file`, else $MTGZ_DELVERAI_KEY, else ~/delverai/.upload_key."""
    if key_file:
        with open(key_file) as f:
            return f.read().strip()
    if os.environ.get(KEY_ENV):
        return os.environ[KEY_ENV].strip()
    if os.path.exists(KEY_FILE):
        with open(KEY_FILE) as f:
            return f.read().strip()
    raise SystemExit(f'no upload key: --key-file, ${KEY_ENV} or {KEY_FILE}')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('ckpt')
    ap.add_argument('--games', type=int, default=6)
    ap.add_argument('--sims', type=int, default=None, help="default: the run's self-play sims")
    ap.add_argument('--selfplay', action='store_true', help="the run's self-play noise and temperature")
    ap.add_argument('--device', default=None, help='default: cuda when available')
    ap.add_argument('--seed', type=int, default=None)
    ap.add_argument('--conc', type=int, default=8, help='games played side by side')
    ap.add_argument('--title', default=None)
    ap.add_argument('--out', default=None, help='default: <run>/replays')
    ap.add_argument('--upload', nargs='?', const=SITE, default=None, help=f'upload to the site (default {SITE})')
    ap.add_argument('--key-file', default=None)
    ap.add_argument('--lookahead', default=None, metavar='SETTINGS',
                    help="the checkpoint's lookahead seat against its MCTS evaluation seat (zero.lookahead.parse, e.g. "
                         "plies=6,worlds=8,opp=min,budget=150000; ENGINE.md 80.11)")
    ap.add_argument('--json', action='store_true', help='write plain .json files (default .json.gz)')
    a = ap.parse_args(argv)
    key = upload_key(a.key_file) if a.upload else None
    if a.lookahead and a.upload:
        ap.error('--lookahead games are not for the site (--upload refused)')
    look = None
    if a.lookahead:
        from zero import lookahead as LA
        try:
            look = LA.parse(a.lookahead)
        except ValueError as e:
            ap.error(str(e))
    reps = record(a.ckpt, games=a.games, sims=a.sims, selfplay=a.selfplay, device=a.device, seed=a.seed, conc=a.conc,
                  title=a.title, lookahead=look)
    run, _ = core.run_of(a.ckpt)
    out = a.out or os.path.join(run or '.', 'replays')
    for r in reps:
        s = summary(r)
        print(f'{save_json(r, out) if a.json else core.save(r, out)}: winner {s["winner"]} turn {s["turns"]} life {s["life"]} '
              f'({s["frames"]} steps)')
        if key:
            print(f'  uploaded: {upload(r, a.upload, key)}')


if __name__ == '__main__':
    main()
