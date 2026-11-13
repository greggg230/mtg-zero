"""`python -m mtgz.az.replays`: games of a checkpoint, recorded for the replay viewer of
delverai.greggg230.com (zero-core `zero.replay`, its docs/replays.md; the state at every step is
the engine's spectator view, engine/src/az/view.rs: both hands, the board, the stack, the
pending decision's options in words, the game log's tail).

  python -m mtgz.az.replays runs/delver1/models/iter_0012.pt [--games 6] [--selfplay]
      [--sims N] [--device cuda] [--upload [URL]] [--key-file F]

The checkpoint plays itself (or `opp`, another checkpoint: `record(..., opp=path)`, game k with the
checkpoint on seat k % 2), seats alternating between games. Defaults come from the
checkpoint's run (`runs/<run>/config.json`): its spec with `"log": true` (the engine's
play-by-play), its self-play simulations (`sims`) and `chance_k`. Evaluation settings by
default (no noise, the most visited option: how the network plays); `--selfplay` = the run's
self-play noise and temperature for the first `temp_moves` decisions (what training sees).

A checkpoint with the belief head (run setting `aux_belief_w`, docs/ENGINE.md 66) adds to every network
decision's frame `belief: {for, n, cards: [[name, expected count], ...]}` - the decider's prediction of
its opponent's hand, the 12 likeliest cards (`mtgz.az.belief.render`, the package's `replay_aux`); its
seat lists `aux: ["belief"]`, and the listing record (`summary`) says `belief: true`.

A constructed best-of-three (docs/ENGINE.md 120, 121, 130: the Delver best-of-three run's spec, one episode per
game) records whole matches: zero.replay keeps a chained match's episodes in one replay (its `games`: each
game's result; its `result`: the match's), from the sideboarding before game 2 (a post-sideboard match: game 1
is not played, its winner drawn) through game 3. The spectator view says where the match is (`match`: the
game, the score, each player's deck, the sideboarding in progress; engine/src/az/view.rs), and `match_index`
writes the boundaries into the replay as `match` (`games`: each game's first sideboarding frame and first
frame of play, who chose play or draw, each player's swaps against the registered main, the result) - the
listing record carries it too. With a deck head (run setting `aux_deck_w`, ENGINE.md 102.4) every network
decision in a game adds `deck` (`deck_read`): the decider's prediction of its opponent's cards it has not
seen - the most expected ones - and what it makes of their sideboarding (`vs_main`: the 60 it predicts against
the registered main). Sideboarding decisions carry neither prediction: there is no hand and no deck target
between games.

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

import numpy as np

import mtgz.az  # noqa: F401  (registers the game package)
from mtgz.az import belief
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


DECK_TOP = 12        # the most expected unseen cards a frame's deck read lists (the hand read's 12)
DECK_MIN = 0.02      # vs_main leaves out a card predicted within this many copies of the main's


def _tally(pairs):
    """[[name, copies], ...] (or a list of names) -> {name: copies}."""
    out = {}
    for x in pairs or []:
        name, n = (x, 1) if isinstance(x, str) else (x[0], x[1])
        out[name] = out.get(name, 0) + n
    return out


def main_decks(spec):
    """{seat: {name: copies}} of a constructed spec's registered mains (`decks.p0` / `decks.p1`), else {}."""
    try:
        s = json.loads(spec) if isinstance(spec, str) else dict(spec or {})
    except ValueError:
        return {}
    d = s.get('decks') or {}
    return {p: _tally(d[f'p{p}']) for p in (0, 1) if d.get(f'p{p}')}


def deck_pools(spec):
    """{seat: the card names of its 75} (`decks.pN` and `decks.pN_sideboard`) of a constructed spec, else {}."""
    try:
        s = json.loads(spec) if isinstance(spec, str) else dict(spec or {})
    except ValueError:
        return {}
    d = s.get('decks') or {}
    return {p: set(d[f'p{p}']) | set(d.get(f'p{p}_sideboard') or []) for p in (0, 1) if d.get(f'p{p}')}


def swaps(deck, main):
    """A deck against the registered main: {'in': [[name, copies]], 'out': [[name, copies]]}, most copies first."""
    have, main = _tally(deck), main or {}
    ins = [[n, c - main.get(n, 0)] for n, c in have.items() if c > main.get(n, 0)]
    outs = [[n, c - have.get(n, 0)] for n, c in main.items() if c > have.get(n, 0)]
    key = lambda x: (-x[1], x[0])  # noqa: E731
    return {'in': sorted(ins, key=key), 'out': sorted(outs, key=key)}


def deck_read(pred, state, seat, unseen, main, pool=None, top=DECK_TOP):
    """A frame's `deck` (docs/ENGINE.md 102.4: the deck head predicts the decider's opponent's cards it does not
    know exactly - hand, library, face down - as a distribution over identities, x their number, which is public):
    {for: the opponent, n: how many of their cards the decider has not seen, cards: [[name, expected copies], ...]
    the `top` most expected (as the hand read's), vs_main: [[name, copies], ...] the 60 it predicts less the
    registered main `main` ({name: copies}), largest first, within DECK_MIN left out - what it thinks came in (+)
    and went out (-); with `pool` (the names of their 75) only its cards, and `outside`: the copies it puts on
    cards outside the 75 (none can be there)}. The predicted 60 is the opponent's cards the decider knows (their 60 in `state`'s
    `match.decks` less the true unseen ones, `unseen`: counts by identity; what is left is what the decider has
    seen) plus the predicted unseen ones. None when nothing is unseen."""
    u = np.asarray(unseen, np.float64)
    n = float(u.sum())
    if n <= 0:
        return None
    p = np.clip(np.asarray(pred, np.float64), 0.0, None)
    x = p * (n / p.sum()) if p.sum() > 0 else p
    names = belief.names()
    opp = 1 - int(seat)
    decks = (state.get('match') or {}).get('decks') or [[], []]
    est = {k: float(v) for k, v in _tally(decks[opp]).items()}
    for i in np.nonzero(u)[0]:
        est[names[i]] = est.get(names[i], 0.0) - float(u[i])          # known = the 60 less what is unseen
    for i in np.nonzero(x > 1e-4)[0]:
        nm = names[i] if names[i] is not None else f'#{i}'
        est[nm] = est.get(nm, 0.0) + float(x[i])
    keys = set(pool) if pool else set(est) | set(main)
    vs = [[k, round(est.get(k, 0.0) - main.get(k, 0), 2)] for k in sorted(keys)]
    vs = sorted((v for v in vs if abs(v[1]) >= DECK_MIN), key=lambda v: (-v[1], v[0]))
    out = {'for': opp, 'n': int(round(n)), 'cards': belief.counts_by_name(x, top, digits=2), 'vs_main': vs}
    if pool:
        out['outside'] = round(sum(v for k, v in est.items() if k not in keys and v > 0), 2)
    return out


class _Recorder(core.RecordingDriver):
    """zero.replay's driver with self-play's budget for minor decisions: `sims_minor`
    simulations (zero-core searches them so in self-play and never records them). The Python
    driver has no `sims_minor`; without it a payment's decisions got the full budget.

    A constructed best-of-three with a deck head (`deck_mains`: the registered mains, main_decks; `deck_pools`: the
    75s' names): each
    network decision in a game adds the deck read (`deck_read`, from the true unseen cards' count: the
    game's `aux_target`); a sideboarding decision carries no prediction (no hand, no deck target)."""
    sims_minor = None
    deck_mains = None
    deck_pools = None

    def __init__(self, *a, **kw):
        super().__init__(*a, **kw)
        self._game = None
        base = self.aux_render
        self.aux_render = lambda pred, state, seat, frame: self._aux(base, pred, state, seat, frame)

    def _aux(self, base, pred, state, seat, frame):
        if (state.get('decision') or {}).get('kind') == 'Sideboard':
            return {}
        out = base(pred, state, seat, frame)
        if self.deck_mains is not None and 'deck' in pred and self._game is not None:
            unseen = np.asarray(self._game.aux_target(int(seat)), np.float64)[belief.D:2 * belief.D]
            o = 1 - int(seat)
            r = deck_read(pred['deck'], state, seat, unseen, self.deck_mains.get(o, {}), (self.deck_pools or {}).get(o))
            if r:
                out = dict(out, deck=r)
        return out

    def _decide(self, slot, seat, tree, p):
        self._game = slot.game
        try:
            super()._decide(slot, seat, tree, p)
        finally:
            self._game = None

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
           lookahead=None, opp=None):
    """`games` games of `ckpt` against itself - or against `opp`, another checkpoint (zero.replay.record:
    seats alternating, game k with `ckpt` on seat k % 2; the same simulations) - with the run's settings
    (its leaf values' view: `leaf_view`; each network seat names it as `leaf_value_view`). `lookahead`
    (settings): the checkpoint's lookahead seat against its MCTS evaluation seat (zero.replay's lookahead
    seat, docs/ENGINE.md 80.11; not with `opp`): the run's evaluation simulations by default and the
    evaluation seat's minor decisions (no `sims_minor`: as zero.h2h's seats). -> zero-replay/1 dicts."""
    if lookahead and opp:
        raise ValueError("a lookahead seat plays against its checkpoint's own MCTS seat: no `opp`")
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
    spec = json.loads(cfg['spec'])
    bo3 = bool(spec.get('constructed')) and spec.get('format') == 'bo3'
    driver = type('Recorder', (_Recorder,), {'sims_minor': None if lookahead else cfg.get('sims_minor'),
                                             'deck_mains': main_decks(spec) if bo3 and cfg.get('aux_deck_w') else None,
                                             'deck_pools': deck_pools(spec)})
    real, core.RecordingDriver = core.RecordingDriver, driver   # core.record builds its driver by this name
    # core.record's seats read the view and the root search from the run's config (zero.replay._nn):
    # give it these
    side = 'selfplay' if selfplay else 'eval'
    real_cfg, core.run_config = core.run_config, lambda r: dict(real_cfg(r), **{side + '_leaf_value_view': view,
                                                                                side + '_root_search': rs})
    try:
        dsims = cfg.get('eval_sims', 32) if lookahead else cfg.get('sims', 64)
        reps = core.record(ckpt, opp=opp or 'self', games=games, sims=sims or dsims,
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
        m = match_index(rep)
        if m:
            rep['match'] = m
    return reps


def _option_text(state, a):
    return next((o.get('t') for o in (state.get('decision') or {}).get('options') or [] if o.get('a') == a), None)


def match_index(rep):
    """A best-of-three replay's games (a chained match: zero.replay keeps its episodes one after the other) -> the
    replay's `match`: {post_board, result (the match's, per seat), wins, games: [...]} or None for a single game.
    Each game in order: {game (1-3), played, sb (the frame of its first sideboarding decision, None for game 1),
    start (its first frame of play), end (the state it ended in), winner (None: a draw), reason, turns, starting,
    life, chooser and choice (who chose play or draw, and what), swaps (each seat's deck against its registered
    main: {in, out}), reads (each seat's first deck read of the other's deck in this game: its `vs_main`)}. A
    post-sideboard match's game 1 (docs/ENGINE.md 130) is {game: 1, played: False, winner}: its winner was drawn
    from the match's seed. A sideboarding before game g belongs to game g (the spectator view still shows game
    g - 1: `match.game` is the current or last game)."""
    states, frames = rep.get('states') or [], rep.get('frames') or []
    m0 = (states[0].get('match') or {}) if states else {}
    if m0.get('format') != 'bo3':
        return None
    mains = main_decks(rep.get('spec'))
    games, order = {}, []

    def entry(g):
        if g not in games:
            games[g] = {'game': g, 'played': True, 'sb': None, 'start': None}
            order.append(g)
        return games[g]

    post = bool(m0.get('post_board'))
    if post:
        entry(1)['played'] = False
    for i, f in enumerate(frames):
        st = states[f['s']]
        sb = (st.get('decision') or {}).get('kind') == 'Sideboard'
        g = int((st.get('match') or {}).get('game') or 1) + (1 if sb else 0)
        e = entry(g)
        if sb:
            if e['sb'] is None:
                e['sb'] = i
            continue
        if e['start'] is None:
            e['start'] = i
            decks = (st.get('match') or {}).get('decks')
            if decks:
                e['swaps'] = [swaps(decks[p], mains.get(p)) for p in (0, 1)]
        if (st.get('decision') or {}).get('kind') == 'PlayDraw' and 'chooser' not in e:
            e['chooser'], e['choice'] = f['p'], _option_text(st, f.get('a'))
        if f.get('deck'):
            reads = e.setdefault('reads', [None, None])
            if reads[f['p']] is None:
                reads[f['p']] = f['deck']['vs_main']
    # each game's end: the last state showing it (before the next sideboarding, or the final one)
    last = {}
    for k, st in enumerate(states):
        if st.get('over'):
            last[int((st.get('match') or {}).get('game') or 1)] = k
    for g in order:
        e = games[g]
        k = last.get(g)
        if k is None:
            continue
        st = states[k]
        e['end'] = k
        e['winner'] = st.get('winner')
        e['reason'] = st.get('reason')
        if e['played']:
            e['turns'] = st.get('turn')
            e['starting'] = st.get('starting')
            e['life'] = [pl.get('life') for pl in st.get('players') or []]
    if post:
        games[1] = {k: games[1][k] for k in ('game', 'played', 'winner', 'end') if k in games[1]}
    fm = (states[rep['final']].get('match') if states and rep.get('final') is not None else None) or {}
    return {'post_board': post, 'result': rep.get('result'), 'wins': fm.get('wins'),
            'games': [games[g] for g in sorted(order)]}


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
            'belief': any('belief' in (s.get('aux') or []) for s in rep['seats']),
            **({'match': match_summary(rep['match'])} if rep.get('match') else {})}


def match_summary(m):
    """The listing record's part of a match (`match_index`): the score and each game's outcome and swaps, without the
    frame positions and the deck reads."""
    keep = ('game', 'played', 'winner', 'reason', 'turns', 'starting', 'chooser', 'choice', 'swaps')
    return {'post_board': m.get('post_board'), 'wins': m.get('wins'),
            'games': [{k: g[k] for k in keep if k in g} for g in m.get('games') or []]}


def public(rep):
    """The replay without this machine's paths (checkpoints by file name)."""
    rep = dict(rep, seats=[dict(s) for s in rep['seats']], source=dict(rep.get('source') or {}))
    for s in rep['seats'] + [rep['source']]:
        if s.get('checkpoint'):
            s['checkpoint'] = os.path.basename(s['checkpoint'])
    return rep


def upload(rep, url, key, timeout=60, meta=None):
    """POST one replay to the site (`<url>/api/replays/upload`), `meta` added to its listing record
    (e.g. `seat_labels`: who plays each seat, shown by the site's viewer). -> the site's answer (dict)."""
    rep = public(rep)
    gz = gzip.compress(json.dumps(rep, separators=(',', ':')).encode('utf-8'))
    body = json.dumps({'meta': dict(summary(rep), **(meta or {})), 'gz': base64.b64encode(gz).decode('ascii')}).encode('utf-8')
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
