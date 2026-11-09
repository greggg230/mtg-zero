"""How a generation plays each blunder test case, for delverai.greggg230.com's Blunders tab (the user, 11-03:
"pick a generation and see how it plays each test case. Starting from the canary case, record until end of
the turn and let me click through the steps with a game player UI").

For a checkpoint and every spot the site counts (`delverai_blunders.scored_spots`: the user-kept canary
spots), with the Play tab's setup (delver-lab playcore.Net: the run's evaluation seat at 1024 simulations, the
most-visited move, the full budget for every decision - zero-core's Python driver has no sims_minor):

1. the spot is searched REPEATS (8) times, fresh trees, its own search seeds: the tally per option and the
   verdict (a right answer in at least PASS of them, delverai_blunders' rule);
2. the move most of those searches chose is played (its search is the spot's frame), then the generation plays
   both seats on to the end of that turn (the turn number changes, or the game ends), each decision recorded.

Each spot -> one zero-replay/1 game (zero.replay.RecordingDriver's frames: the viewer's searches, win chance,
hand read) starting at the spot, with a `blunder` block (the spot, its right answer, the tally, the verdict),
uploaded to the site as a game of the run `blunders.<run>` at that generation (its listing record says
kind 'blunder'; the earlier upload of that generation is deleted first). The tallies (tally.json) replace
the scoring cache's scores of those spots (delverai_blunders.merge_games, whenever the cache is read under
its lock) and the run's blunder record is uploaded again (`refresh`), so the Progress tab's counts and the
Blunders tab's verdicts are the same searches.

  python tools/delverai_blunder_games.py RUN [--iters 720,712 | --every 32 [--min-iter N] [--max-iter N]]
      [--engine CHECKOUT] [--upload] [--url URL] [--key K] [--redo]

A generation recorded for the current library (tally.json's library id) is not recorded again (--redo does);
its upload to a site is remembered per URL (uploaded.json). Files: runs/<run>/blunders/games/iter_NNNN/
<spot>.json.gz (format zero-replay/1), tally.json, uploaded.json.
"""
import argparse
import collections
import gzip
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import delver_blunders as B  # noqa: E402
import delver_fingerprints as F  # noqa: E402
import delverai_blunders as D  # noqa: E402

MAX_STEPS = 600          # a turn's steps at most (a spot's turn: a few dozen)
SEED = 20261103


def _driver_class():
    from zero import replay as core

    class SpotDriver(core.RecordingDriver):
        """zero.replay's recording driver, game k started at `starts[k]` (a game at a spot) instead of a new
        game; a game ends when its turn number changes or, with `first_only`, after its first network
        decision."""

        def __init__(self, starts, seat, spec, first_only=False, seed=SEED, **kw):
            self.starts = starts
            self.first_only = first_only
            self.turn0 = {}
            super().__init__('mtg', len(starts), lambda k: [seat, seat], spec=spec, concurrency=len(starts), seed=seed,
                             max_steps=MAX_STEPS, **kw)

        def _refill(self):
            n0 = len(self.active)
            super()._refill()
            for slot in self.active[n0:]:
                slot.game = self.starts[slot.gid]
                self.turn0[slot.gid] = json.loads(slot.game.export_state())['turn']

        def _apply(self, slot, a):
            nn = slot.frame is not None and slot.frame.get('k') == 'nn'
            super()._apply(slot, a)
            g = slot.game
            if (self.first_only and nn) or (not g.over and json.loads(g.export_state())['turn'] != self.turn0[slot.gid]):
                slot.steps = self.max_steps              # done: _round finishes it

    return SpotDriver


def play_seat(ckpt, sims=D.SIMS):
    """The Play tab's seat for `ckpt` (delver-lab playcore.Net with play.PLAY_SELECT 'visits') -> (seat, cfg, seat info)."""
    import mtgz.az.replays as R
    from zero import replay as core
    run, it = core.run_of(ckpt)
    cfg = core.run_config(run)
    seat_cfg = dict(cfg, eval_leaf_value_view=R.leaf_view(cfg), eval_root_search=R.root_search(cfg), eval_select='visits',
                    eval_select_min_visits=0, eval_select_c_scale=None, eval_select_c_visit=None)
    seat = core._nn('a', int(sims), seat_cfg, False, int(cfg.get('chance_k', 0)))
    info = {'name': f'{os.path.basename(run)} iter {it}', 'kind': 'nn', 'checkpoint': os.path.basename(ckpt),
            'run': os.path.basename(run), 'iter': it, 'sims': int(sims), 'select': 'visits',
            'leaf_value_view': seat.leaf_value_view}
    return seat, cfg, info


def _run(Driver, starts, seat, spec, ev, first_only, seed):
    from zero import package
    from zero.infer import drive
    kw = {}
    if ev.aux_layout:
        kw = {'aux': {'a': ev}, 'aux_render': package.active().replay_aux_of('mtg')}
    drv = Driver(starts, seat, spec, first_only=first_only, seed=seed, **kw)
    drive(drv, {'a': ev})
    return {r['gid']: r for r in drv.take_replays()}


def right_of(sp, label):
    return (label == sp['watch']) if sp['answer'] == 'good' else (label != sp['watch'])


def record(ckpt, spots, engine_path=B.ROOT, log=B.say, seed=SEED):
    """`ckpt` on each spot: the tally and the turn played out -> {spot id: replay}, [stale spot ids]."""
    E = B.load_engine(engine_path)                  # (the engine checkout's packages on the path: zero, mtgz)
    import torch
    from zero.infer import NetEvaluator
    from zero.net import load
    import mtgz.az.replays as R
    R.check_encoding(ckpt)
    torch.set_num_threads(min(8, os.cpu_count() or 8))
    seat, cfg, info = play_seat(ckpt)
    model, _ = load(ckpt, 'cpu', game='mtg')
    ev = NetEvaluator(model, 'cpu')
    if ev.aux_layout:
        info['aux'] = [n for n, _, _ in ev.aux_layout]
    Driver = _driver_class()
    ok, stale, base = [], [], []
    for sp in spots:
        g = B.spot_game(E, sp)
        st = json.loads(g.export_state())
        if B.spot_sig(st) != sp['sig'] or not B.same_pos(st, sp):
            stale.append(sp['id'])
            continue
        ok.append(sp)
        base.append((g, st))
    if not ok:
        return {}, stale
    spec = json.dumps(dict(json.loads(ok[0]['spec']), log=True))
    t0 = time.time()
    # 1. the tally: REPEATS fresh searches a spot
    starts = [g.clone() for g, _ in base for _ in range(D.REPEATS)]
    first = _run(Driver, starts, seat, spec, ev, True, seed)
    log(f'{info["name"]}: {len(starts)} searches at {len(ok)} spots in {time.time() - t0:.0f} s')
    picks, frame0 = [], []
    for i, (sp, (g, st)) in enumerate(zip(ok, base)):
        labels = {o['a']: o['t'] for o in (st.get('decision') or {}).get('options') or []}
        reps = [first[i * D.REPEATS + r] for r in range(D.REPEATS)]
        acts = [rp['frames'][0]['a'] for rp in reps]
        count = collections.Counter(acts)
        top = max(count.values())
        a = next(x for x in acts if count[x] == top)                 # the most chosen; a tie: the earliest search's
        rp = reps[acts.index(a)]
        picks.append((a, labels, count))
        frame0.append((rp['states'][rp['frames'][0]['s']], rp['frames'][0]))
    # 2. the move most searches chose, then the rest of the turn
    t1 = time.time()
    after = []
    for (g, _), (a, _, _) in zip(base, picks):
        h = g.clone()
        h.step(int(a))
        after.append(h)
    live = [k for k, h in enumerate(after) if not h.over and json.loads(h.export_state())['turn'] == base[k][1]['turn']]
    rest = _run(Driver, [after[k] for k in live], seat, spec, ev, False, seed + 1) if live else {}
    rest = {live[j]: r for j, r in rest.items()}
    log(f'{info["name"]}: the turns played out in {time.time() - t1:.0f} s')
    out = {}
    created = int(time.time())
    run, it = info['run'], info['iter']
    for k, sp in enumerate(ok):
        a, labels, count = picks[k]
        s0, f0 = frame0[k]
        f0 = dict(f0, s=0)
        r = rest.get(k)
        if r is not None:
            states = [s0] + r['states']
            frames = [f0] + [dict(f, s=f['s'] + 1) for f in r['frames']]
            final, res, scores, rounds, over = r['final'] + 1, r['result'], r['scores'], r['rounds'], r['over']
        else:                                                         # the move ended the turn (or the game)
            h = after[k]
            states = [s0, json.loads(h.export_state())]
            frames = [f0]
            final, over = 1, bool(h.over)
            res = [float(x) for x in h.result()] if over else None
            scores, rounds = [float(x) for x in h.scores()], int(h.rounds)
        tally = sorted(((labels.get(x, str(x)), n) for x, n in count.items()), key=lambda t: -t[1])
        right = sum(n for lab, n in tally if right_of(sp, lab))
        st = base[k][1]
        blunder = {'spot': sp['id'], 'kind': sp['kind'], 'label': D.LABELS.get(sp['kind'], sp['kind'].replace('_', ' ')),
                   'answer': sp['answer'], 'watch': sp['watch'], 'why': sp['why'], 'context': _context(sp),
                   'turn': st.get('turn'), 'step': st.get('step'), 'decider': sp['decider'],
                   'tally': [[lab, n] for lab, n in tally], 'right': right, 'repeats': D.REPEATS, 'pass': D.PASS,
                   'passed': right >= D.PASS, 'played': labels.get(a, str(a)), 'played_right': right_of(sp, labels.get(a, str(a))),
                   'sims': info['sims'], 'select': 'visits'}
        out[sp['id']] = {
            'format': 'zero-replay/1', 'game': 'mtg', 'spec': spec, 'seed': sp['seed'], 'game_index': k, 'created': created,
            'title': f'{blunder["label"]} · {run} iter {it}', 'group': f'blunder test cases · {run} iter {it}',
            'selfplay': False, 'hint': None, 'source': {'checkpoint': os.path.basename(ckpt), 'run': run, 'iter': it},
            'seats': [dict(info), dict(info)], 'states': states, 'frames': frames, 'final': final, 'result': res,
            'scores': scores, 'rounds': rounds, 'over': over, 'blunder': blunder}
    return out, stale


def _context(sp):
    """The spot's own description lines (the miners of the newer kinds write them), else none."""
    c = sp.get('context')
    return [str(x) for x in c][:6] if isinstance(c, list) else []


def meta_of(rep, run):
    """The site's listing record: zero.replay's summary + the test case (kind 'blunder', run 'blunders.<run>')."""
    import mtgz.az.replays as R
    m = R.summary(rep)
    b = rep['blunder']
    m.update(kind='blunder', run=f'blunders.{run}', for_run=run, iter=rep['source']['iter'],
             blunder={k: b[k] for k in ('spot', 'kind', 'label', 'answer', 'watch', 'turn', 'step', 'decider', 'tally', 'right',
                                         'repeats', 'pass', 'passed', 'played', 'played_right')})
    return m


def save(reps, run_dir, it):
    d = os.path.join(run_dir, 'blunders', 'games', f'iter_{it:04d}')
    os.makedirs(d, exist_ok=True)
    for sid, rep in reps.items():
        name = re.sub(r'[^A-Za-z0-9._-]', '_', sid) + '.json.gz'
        with gzip.open(os.path.join(d, name), 'wt', encoding='utf-8') as f:
            json.dump(rep, f, separators=(',', ':'))
    return d


# A spot's reason as the public site words it: the library's notes to the reviewer dropped (who reported it,
# when, the search setting that showed it), a reviewer's question replaced by its kind's reason.
_NOTE = re.compile(r'\s*\([^()]*\b(?:the user|report|cq rule|horizon)\b[^()]*\)', re.I)
KIND_WHY = {'fourth_land_empty_hand': 'a 4th+ mana land from an otherwise empty hand: held, it is Brainstorm fodder and hides information'}


def public_why(sp_kind, why):
    if 'judge whether' in why:
        return KIND_WHY.get(sp_kind, why.split(':')[0])
    return re.sub(r'\s{2,}', ' ', _NOTE.sub('', why)).strip()


def upload_all(reps, run, it, url=D.URL, key=None, log=B.say):
    """Replace the site's games of `blunders.<run>` at generation `it` with these."""
    import base64
    import mtgz.az.replays as R
    key = key or D.upload_key()
    R.post(url.rstrip('/') + '/api/admin/delete', json.dumps({'run': f'blunders.{run}', 'iter': it}).encode(), key)
    n = 0
    for rep in reps.values():
        rep = R.public(rep)
        rep['blunder'] = dict(rep['blunder'], why=public_why(rep['blunder']['kind'], rep['blunder']['why']), context=[])
        body = json.dumps(rep, separators=(',', ':'))
        if re.search(r'[A-Za-z]:[\\/]|/Users/|/home/', body):
            raise ValueError(f'a local path in the game of {rep["blunder"]["spot"]}')
        gz = gzip.compress(body.encode('utf-8'))
        R.post(url.rstrip('/') + '/api/replays/upload',
               json.dumps({'meta': meta_of(rep, run), 'gz': base64.b64encode(gz).decode('ascii')}).encode(), key)
        n += 1
    log(f'blunders.{run} iter {it}: {n} games uploaded')


def gen_dir(run_dir, it):
    return os.path.join(run_dir, 'blunders', 'games', f'iter_{it:04d}')


def write_tally(run_dir, it, reps, L):
    """The generation's tallies for the scorer (delverai_blunders.merge_games) and what was recorded."""
    d = gen_dir(run_dir, it)
    t = {'setup': D.setup_key(D.SIMS, D.SELECT), 'library': D.library_id(L),
         'scores': {sid: r['blunder']['right'] / r['blunder']['repeats'] for sid, r in reps.items()},
         'recorded': time.strftime('%Y-%m-%d %H:%M:%S')}
    tmp = os.path.join(d, 'tally.json.tmp')
    with open(tmp, 'w', encoding='utf-8') as f:
        json.dump(t, f, indent=1)
    os.replace(tmp, os.path.join(d, 'tally.json'))


def load_recorded(run_dir, it, spots):
    """The generation's games already on disk, at this setup, of these spots -> {spot id: game} (a spot added to
    the library since: not there; one dropped: not asked for)."""
    d = gen_dir(run_dir, it)
    try:
        with open(os.path.join(d, 'tally.json'), encoding='utf-8') as f:
            t = json.load(f)
    except (OSError, ValueError):
        return {}
    if t.get('setup') != D.setup_key(D.SIMS, D.SELECT):
        return {}
    reps = {}
    for sp in spots:
        p = os.path.join(d, re.sub(r'[^A-Za-z0-9._-]', '_', sp['id']) + '.json.gz')
        if sp['id'] not in t['scores'] or not os.path.exists(p):
            continue                                                  # (a stale spot: none recorded)
        with gzip.open(p, 'rt', encoding='utf-8') as f:
            reps[sp['id']] = json.load(f)
    return reps


def uploaded_to(run_dir, it):
    try:
        with open(os.path.join(gen_dir(run_dir, it), 'uploaded.json'), encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def mark_uploaded(run_dir, it, url, lib, n):
    u = uploaded_to(run_dir, it)
    u[url.rstrip('/')] = {'library': lib, 'n': n, 'at': time.strftime('%Y-%m-%d %H:%M:%S')}
    with open(os.path.join(gen_dir(run_dir, it), 'uploaded.json'), 'w', encoding='utf-8') as f:
        json.dump(u, f, indent=1)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('run')
    ap.add_argument('--iters', default=None, help='generations, comma-separated (default: --every / the newest)')
    ap.add_argument('--every', type=int, default=None, help='every Nth evaluated generation, newest first')
    ap.add_argument('--min-iter', type=int, default=None)
    ap.add_argument('--max-iter', type=int, default=None)
    ap.add_argument('--engine', default=B.ROOT)
    ap.add_argument('--library', default=B.LIBRARY_PATH)
    ap.add_argument('--spots', default=None, help='only these spot ids (comma-separated; a test)')
    ap.add_argument('--upload', action='store_true', help="upload each generation's games (and refresh the run's blunder record)")
    ap.add_argument('--url', default=D.URL)
    ap.add_argument('--key', default=None, help="the site's upload key (default: delverai_blunders.upload_key)")
    ap.add_argument('--redo', action='store_true', help='record generations that already have games')
    ap.add_argument('--wait', type=int, default=7200, help='seconds to wait for a scorer before the last record refresh')
    ap.add_argument('--follow', type=int, default=None, metavar='START',
                    help='keep going: every --stride th evaluated generation from START on, the newest first (stop: '
                         'runs/<run>/blunders/games/STOP)')
    ap.add_argument('--stride', type=int, default=16)
    ap.add_argument('--loop', type=int, default=600, help='--follow: seconds between looks for a new checkpoint')
    a = ap.parse_args(argv)
    os.environ.setdefault('ZERO_TOKEN_GRAPHS', '1')
    B.load_engine(a.engine)                           # (its packages on the path: an upload of recorded games needs mtgz)
    run_dir = F.run_dir(a.run)
    run = os.path.basename(os.path.normpath(run_dir))
    prod = a.url.rstrip('/') == D.URL.rstrip('/')
    if a.follow is not None:
        return follow(a, run_dir, run, prod)
    L = B.load_library(a.library)
    spots = D.scored_spots(L)
    if a.spots:
        want = set(a.spots.split(','))
        spots = [sp for sp in spots if sp['id'] in want]
    ck = D.checkpoints(run_dir, a.min_iter, a.max_iter, a.every)
    its = [int(x) for x in a.iters.split(',')] if a.iters else sorted(ck, reverse=True)[:None if a.every else 1]
    sent = False
    for it in its:
        sent |= one(a, run_dir, run, it, ck.get(it), L, spots, prod)
    if sent and prod:
        D.refresh(run_dir, do_upload=True, url=a.url, library=a.library, wait=a.wait)
    return 0


def done(a, run_dir, it, L):
    """Whether generation `it` needs nothing: uploaded for this library (with --upload), else recorded for it."""
    lib = D.library_id(L)
    if a.upload:
        return uploaded_to(run_dir, it).get(a.url.rstrip('/'), {}).get('library') == lib
    try:
        with open(os.path.join(gen_dir(run_dir, it), 'tally.json'), encoding='utf-8') as f:
            return json.load(f).get('library') == lib
    except (OSError, ValueError):
        return False


def one(a, run_dir, run, it, path, L, spots, prod):
    """Generation `it`: the spots it lacks recorded (all of them with --redo), then uploaded unless it is already
    -> whether it was sent."""
    path = path or os.path.join(run_dir, 'models', f'iter_{it:04d}.pt')
    have = {} if (a.redo or a.spots) else load_recorded(run_dir, it, spots)
    missing = [sp for sp in spots if sp['id'] not in have]
    if missing:
        new, stale = record(path, missing, a.engine)
        if stale:
            B.say(f'{len(stale)} spots no longer replay on this engine: {stale}')
        d = save(new, run_dir, it)
        if a.spots:
            B.say(f'{run} iter {it}: test spots in {d}')
            return False
        have.update(new)
    order = {sp['id']: k for k, sp in enumerate(spots)}
    reps = {sid: dict(r, game_index=order[sid]) for sid, r in have.items() if sid in order}
    write_tally(run_dir, it, reps, L)
    passed = sum(r['blunder']['passed'] for r in reps.values())
    B.say(f'{run} iter {it}: {passed} of {len(reps)} right ({len(missing)} recorded now)')
    if not a.upload or done(a, run_dir, it, L):
        return False
    upload_all(reps, run, it, a.url, a.key)
    mark_uploaded(run_dir, it, a.url, D.library_id(L), len(reps))
    if prod:                                          # the Progress tab's counts: these searches
        D.refresh(run_dir, do_upload=True, url=a.url, library=a.library)
    return True


def follow(a, run_dir, run, prod):
    """Every `stride`th evaluated generation from `follow` on (the user, 11-03: "record the latest checkpoint we
    evaluated and start from there"), the newest not done first; the library re-read every round (a spot kept
    since: recorded for the generations done, only it). Stops on runs/<run>/blunders/games/STOP."""
    stop = os.path.join(run_dir, 'blunders', 'games', 'STOP')
    B.say(f'{run}: following from generation {a.follow}, every {a.stride}th (stop: {stop})')
    while not os.path.exists(stop):
        L = B.load_library(a.library)
        spots = D.scored_spots(L)
        ck = D.checkpoints(run_dir, a.follow)
        todo = [i for i in sorted(ck, reverse=True) if (i - a.follow) % a.stride == 0 and not done(a, run_dir, i, L)]
        if not todo:
            time.sleep(a.loop)
            continue
        try:
            one(a, run_dir, run, todo[0], ck[todo[0]], L, spots, prod)
        except Exception as e:                        # noqa: BLE001  (an upload failing: logged, tried next round)
            import traceback
            B.say(f'{run} iter {todo[0]}: failed ({type(e).__name__}: {e})')
            traceback.print_exc()
            time.sleep(a.loop)
    B.say(f'{run}: STOP')
    return 0


if __name__ == '__main__':
    sys.exit(main())
