"""Publish an EoE sealed run to sealedai.greggg230.com: 17lands-style card statistics from its self-play games, and its
training progress.

  python tools/sealedai_publish.py E:/mtg-zero/runs/eoe1 [--loop 300] [--progress-every 60]
      [--url https://sealedai.greggg230.com] [--key-file F] [--dry-run]
      [--credit-every 4] [--credit-window 4] [--credit-games N] [--no-credit] [--threads 4]

Each pass:
  * card statistics (`mtgz.sealed.cardstats`, docs/ENGINE.md 107.4): the run's game log (`<run>/games/*.jsonl`) read
    incrementally - each file's complete lines once -, every finished self-play game counted for the network's seats
    into its iteration's tally (an iteration is finished when its metrics line is written; the games of the iteration in
    progress wait for it). Per iteration the per-card counts (cardstats.FIELDS: GP, the games with draws, OH, GD, GIH;
    doubled wins) are POSTed to <url>/api/cards/upload, only the iterations that changed since the last upload; the site
    sums any window of iterations and computes the rates and their Wilson intervals itself. Beside them, once (again when
    it changes), the record's `info`: the set's cards (the rows' index), the definitions, the 17lands yardstick column
    (a comparison only, never a training signal: their GIH WR from data/sealed/eoe_17lands_sealed.json.gz, CC BY 4.0, with
    its attribution), and the draw credit;
  * the draw credit (`cardcredit` through `cardstats.draw_credit`): with the newest checkpoint whose iteration is a multiple
    of --credit-every (and the first one seen), over the version-2 self-play games of the latest --credit-window finished
    iterations, the network's seats (`--no-credit`: never);
  * progress: the run's metrics (tools/delverai_publish.py's iteration rows: vs the heuristic and random, Elo, losses,
    self-play) plus the auxiliary losses (deck prediction, hand read), the Elo pool's current fit per checkpoint and the
    best guess's score against the explore builds in self-play (from the game log), the run's state and settings, POSTed
    to <url>/api/progress/upload (with --loop from its own thread every --progress-every seconds).

CPU only and below normal priority: the run trains on the GPU and its self-play is CPU-bound (CUDA_VISIBLE_DEVICES=-1
is set before torch loads; the draw credit uses --threads torch threads, 4 by default). Nothing is written to the run's
directory. Only run names, iterations and numbers reach the site, never a local path. The upload key: --key-file,
$MTGZ_SEALEDAI_KEY or ~/sealedai/.upload_key (the site's Worker secret UPLOAD_KEY).
"""
import os

os.environ.setdefault('CUDA_VISIBLE_DEVICES', '-1')          # never the GPU: the run trains on it

import argparse  # noqa: E402
import hashlib  # noqa: E402
import json  # noqa: E402
import re  # noqa: E402
import sys  # noqa: E402
import threading  # noqa: E402
import time  # noqa: E402
import traceback  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delverai_publish as DP  # noqa: E402
from mtgz.az import replays  # noqa: E402
from mtgz.sealed import cardstats as CS  # noqa: E402
from mtgz.sealed import games as GL  # noqa: E402
from zero import elo as zelo, runctl  # noqa: E402

SITE = 'https://sealedai.greggg230.com'
KEY_ENV = 'MTGZ_SEALEDAI_KEY'
KEY_FILE = os.path.join(os.path.expanduser('~'), 'sealedai', '.upload_key')
SETTINGS = ('preset', 'iters', 'games', 'sims', 'sims_fast', 'p_full', 'sims_minor', 'temp_moves', 'arch', 'hidden',
            'blocks', 'heads', 'ff', 'policy_target', 'eval_games', 'eval_sims', 'eval_random_games', 'elo_games',
            'eval_every', 'yardstick', 'window', 'lr', 'seed', 'eval_select', 'aux_belief_w', 'aux_deck_w', 'reader_deck')
AUX = ('aux_deck', 'aux_deck_hit', 'aux_belief', 'aux_belief_hit', 'reader_deck', 'reader_deck_hit', 'margin', 'lr')


def upload_key(key_file=None):
    """The site's upload key: `key_file`, else $MTGZ_SEALEDAI_KEY, else ~/sealedai/.upload_key."""
    if key_file:
        with open(key_file) as f:
            return f.read().strip()
    if os.environ.get(KEY_ENV):
        return os.environ[KEY_ENV].strip()
    if os.path.exists(KEY_FILE):
        with open(KEY_FILE) as f:
            return f.read().strip()
    raise SystemExit(f'no upload key: --key-file, ${KEY_ENV} or {KEY_FILE}')


# ---------------------------------------------------------------------------------------------------- the game log

class LogTail:
    """A run's game log read incrementally: each file's complete lines, once (a last line without its newline is being
    written: read at a later call)."""

    def __init__(self, run):
        self.run, self.pos, self.lines, self.specs = run, {}, {}, {}

    def new_games(self):
        for f in GL.files(self.run):
            at = self.pos.get(f, 0)
            with open(f, 'rb') as fh:
                fh.seek(at)
                data = fh.read()
            end = data.rfind(b'\n')
            if end < 0:
                continue
            self.pos[f] = at + end + 1
            for line in data[:end + 1].decode('utf-8').split('\n')[:-1]:
                self.lines[f] = self.lines.get(f, 0) + 1
                g = GL.parse_line(line, self.specs, f, self.lines[f])
                if g is not None:
                    yield g


class CardTallies:
    """Per finished iteration, the cardstats.Tally of its finished self-play games (the network's seats)."""

    def __init__(self, run, phase='selfplay', seats='network'):
        self.run, self.phase, self.of = run, phase, CS.seat_filter(seats)
        self.tail = LogTail(run)
        self.done = {}            # iteration -> Tally
        self.pending = []         # games of iterations not finished yet (their metrics line not written)

    def update(self):
        """Read the new lines, count the games whose iteration is finished. -> the iterations that changed."""
        times = GL.iteration_times(self.run)
        new = [g for g in self.tail.new_games() if self.phase == 'all' or g['phase'] == self.phase]
        for g in new:
            g.pop('actions', None)                    # (the counts need the draws, not the replay)
        changed, keep = set(), []
        for g in self.pending + new:
            it = GL.iteration_of(g['time'], times) if times else None
            if it is None:
                keep.append(g)
                continue
            self.done.setdefault(it, CS.Tally()).add(g, self.of(g))
            changed.add(it)
        self.pending = keep
        return changed


# ---------------------------------------------------------------------------------------------------- the card record

def iteration_body(it, t, index, names):
    """One iteration's tally as the site stores it: {iter, games, seats, hand_games, hand_seats, aborted, best: [games,
    doubled wins of the best guess], rows: [[card index, *FIELDS counts]]}. New card names are appended to `names` (and
    `index`): the list only grows, so earlier rows keep their indices."""
    rows = []
    for name, c in t.cards.items():
        if name not in index:
            index[name] = len(names)
            names.append({'name': name, 'colors': '', 'ci': '', 'rarity': None, 'mv': None, 'type': ''})
        rows.append([index[name]] + list(c))
    rows.sort()
    return {'iter': it, 'games': t.games, 'seats': t.seats, 'hand_games': t.hand_games, 'hand_seats': t.hand_seats,
            'aborted': t.aborted, 'best': list(t.best), 'rows': rows}


def card_info_record(run, names, tallies, credit=None, yardstick=True):
    """The card record's `info` (see the module docs)."""
    done = sorted(tallies.done)
    hand = [it for it in done if tallies.done[it].hand_games]
    out = {'run': os.path.basename(os.path.normpath(run)), 'set': 'EOE', 'phase': tallies.phase, 'seats': 'network',
           'fields': list(CS.FIELDS), 'definitions': CS.DEFINITIONS, 'cards': names,
           'iterations': [done[0], done[-1]] if done else None, 'hand_from': hand[0] if hand else None,
           'credit': credit, 'seventeen': None}
    if yardstick:
        y = CS.seventeen_lands()
        out['seventeen'] = {**{k: v for k, v in y.items() if k != 'cards'},
                            'cards': {n: [c['wr'], c['n']] for n, c in y['cards'].items()}}
    return out


def credit_record(run, ckpt_iter, ckpt_path, window, max_games=None, threads=4, log=print):
    """The draw credit of the version-2 self-play games of iterations window[0]..window[1] (network seats) with the
    checkpoint -> the info record's `credit` ({checkpoint_iter, iters, games, draws, mean_change, ..., cards: {name:
    [credit, se, relative, draws]}}), None when the window has no such game."""
    times = GL.iteration_times(run)
    of = CS.seat_filter('network')
    specs, games = {}, []
    for g in GL.stream(run, specs):
        if g['phase'] != 'selfplay' or g.get('v', 1) < 2 or g['result'] is None:
            continue
        it = GL.iteration_of(g['time'], times)
        if it is not None and window[0] <= it <= window[1] and of(g):
            games.append(g)
    if not games:
        return None
    t0 = time.time()
    c = CS.draw_credit(specs, games, run, ckpt_path, of, max_games, threads, None)
    log(f'draw credit: checkpoint {ckpt_iter}, iterations {window[0]}-{window[1]}, {c["games"]} games, {c["draws"]} draws '
        f'({time.time() - t0:.0f} s)')
    return {'checkpoint_iter': ckpt_iter, 'iters': list(window), 'games': c['games'], 'draws': c['draws'],
            'mean_change': c['mean_change'], 'mean_change_se': c['mean_change_se'],
            'opening': {k: c['opening'][k] for k in ('steps', 'mean_change', 'se')}, 'computed': round(time.time()),
            'cards': {n: [x['credit'], x['se'], x['relative'], x['draws']] for n, x in sorted(c['by_card'].items())}}


# ---------------------------------------------------------------------------------------------------- the progress record

def _metrics_by_iter(run):
    return {m['iter']: m for m in DP._jsonl(os.path.join(run, 'metrics.jsonl')) if isinstance(m.get('iter'), int)}


def progress(run, tallies=None, title=None):
    """The site's progress record of a run."""
    cfg = DP._json(os.path.join(run, 'config.json'), {}) or {}
    st = runctl.run_status(run)
    tag, pool = zelo.active_pool(run)
    ratings = (DP._json(os.path.join(pool, 'ratings.json'), {}) or {}).get('ratings') or {}
    mets = _metrics_by_iter(run)
    rows = []
    for r in DP.iteration_rows(run):
        it = r.get('iter')
        tr = (mets.get(it) or {}).get('train') or {}
        fit = ratings.get(f'iter_{it:04d}') if isinstance(it, int) else None
        t = tallies.done.get(it) if tallies is not None and isinstance(it, int) else None
        r.update({k: DP._r(tr.get(k), 5 if k == 'lr' else 4) for k in AUX if k in tr})
        r['elo_fit'] = DP._r((fit or {}).get('elo'), 1)
        r['elo_fit_se'] = DP._r((fit or {}).get('se'), 1)
        if t is not None:
            r['best_games'], r['best_w2'] = t.best
            r['log_games'], r['log_hand_games'] = t.games, t.hand_games
        rows.append({k: v for k, v in r.items() if v is not None and k != 'h2h'})
    try:
        spec = json.loads(cfg.get('spec') or '{}')
    except ValueError:
        spec = {}
    idx = DP._json(os.path.join(run, 'models', 'index.json'), {}) or {}
    best = idx.get('best') or {}
    return {
        'run': os.path.basename(os.path.normpath(run)), 'updated': time.time(), 'title': title,
        'state': st.get('state'), 'detail': st.get('detail') or None, 'paused_since': st.get('paused_since'),
        'progress': DP._json(os.path.join(run, 'progress.json')),
        'settings': {k: cfg[k] for k in SETTINGS if k in cfg}, 'game': cfg.get('game'),
        'sealed': {**(spec.get('sealed') or {}), 'max_turns': spec.get('max_turns'), 'eval_decks': spec.get('eval_decks')},
        'select': tag, 'best': {k: best.get(k) for k in ('iter', 'elo', 'se')} if best else None,
        'anchor': (DP._json(os.path.join(pool, 'ratings.json'), {}) or {}).get('anchor'),
        'iters': rows,
    }


# ---------------------------------------------------------------------------------------------------- posting

def post(url, path, body, key):
    return replays.post(url.rstrip('/') + path, json.dumps(body, separators=(',', ':')).encode('utf-8'), key)


class Publisher:
    def __init__(self, run, a, key, log=print):
        self.run, self.a, self.key, self.log = run, a, key, log
        self.tallies = CardTallies(run)
        self.names = [dict(c) for c in CS.card_info()]
        self.index = {c['name']: i for i, c in enumerate(self.names)}
        self.sent = {}               # iteration -> hash of the body last uploaded
        self.info_sent = None
        self.credit = None
        self.credit_ckpt = None

    def maybe_credit(self):
        """The draw credit with a new checkpoint (module docs), when one is due."""
        if self.a.no_credit:
            return
        ck = DP.latest_checkpoint(self.run, 1)
        if ck is None:
            return
        it, path = ck
        every = max(1, self.a.credit_every)
        due = self.credit_ckpt is None or it // every > self.credit_ckpt // every     # crossed a multiple of N
        done = sorted(i for i in self.tallies.done if self.tallies.done[i].hand_games)
        if not due or not done:
            return
        window = (done[max(0, len(done) - self.a.credit_window)], done[-1])
        rec = credit_record(self.run, it, path, window, self.a.credit_games, self.a.threads, self.log)
        self.credit_ckpt = it
        if rec is not None:
            self.credit = rec

    def cards_pass(self):
        """Count the new games, maybe the draw credit, then upload what changed. -> the body posted (or None)."""
        self.tallies.update()
        self.maybe_credit()
        iters = []
        for it in sorted(self.tallies.done):
            b = iteration_body(it, self.tallies.done[it], self.index, self.names)
            h = hashlib.sha1(json.dumps(b, sort_keys=True).encode()).hexdigest()
            if self.sent.get(it) != h:
                iters.append((b, h))
        info = card_info_record(self.run, self.names, self.tallies, self.credit, not self.a.no_17lands)
        ih = hashlib.sha1(json.dumps({k: v for k, v in info.items()}, sort_keys=True).encode()).hexdigest()
        if not iters and ih == self.info_sent:
            return None
        body = {'run': info['run'], 'info': info, 'iters': [b for b, _ in iters]}
        if self.a.dry_run:
            size = len(json.dumps(body, separators=(',', ':')))
            self.log(f'(dry run) cards: {len(iters)} iterations {[b["iter"] for b, _ in iters][:6]}..., {size:,} bytes')
        else:
            # in batches (a first upload of a long run): the info with each, the iterations a few dozen at a time
            for i in range(0, max(1, len(iters)), 40):
                post(self.a.url, '/api/cards/upload', {**body, 'iters': [b for b, _ in iters[i:i + 40]]}, self.key)
            self.log(f'cards: uploaded {len(iters)} iterations' + (f' ({iters[0][0]["iter"]}-{iters[-1][0]["iter"]})' if iters else ''))
        for b, h in iters:
            self.sent[b['iter']] = h
        self.info_sent = ih
        return body

    def progress_pass(self):
        p = progress(self.run, self.tallies, self.a.title)
        if self.a.dry_run:
            self.log(f'(dry run) progress: {len(p["iters"])} iteration rows, {len(json.dumps(p)):,} bytes, state {p["state"]}')
        else:
            post(self.a.url, '/api/progress/upload', p, self.key)
        return p


def progress_loop(pub, a, log):
    """Post the progress every --progress-every seconds (beside a card pass: it only reads the finished tallies)."""
    while True:
        time.sleep(a.progress_every)
        try:
            pub.progress_pass()
        except Exception as e:                         # the next post will do
            log(f'progress post failed: {e!r}')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('run')
    ap.add_argument('--loop', type=float, default=None, help='repeat every N seconds')
    ap.add_argument('--progress-every', type=float, default=60, help='with --loop: seconds between progress posts')
    ap.add_argument('--credit-every', type=int, default=4, help='draw credit with checkpoints whose iteration is a multiple of N')
    ap.add_argument('--credit-window', type=int, default=4, help='draw credit over the latest N finished iterations')
    ap.add_argument('--credit-games', type=int, default=None, help='draw credit over at most the newest N games')
    ap.add_argument('--no-credit', action='store_true', help='no draw credit')
    ap.add_argument('--no-17lands', action='store_true', help='no 17lands yardstick column')
    ap.add_argument('--threads', type=int, default=4, help='torch threads for the draw credit (CPU)')
    ap.add_argument('--title', default=None, help="the run's title on the site")
    ap.add_argument('--url', default=SITE)
    ap.add_argument('--key-file', default=None)
    ap.add_argument('--dry-run', action='store_true', help='compute everything, upload nothing')
    a = ap.parse_args(argv)
    if not os.path.isdir(a.run):
        raise SystemExit(f'{a.run}: no such run directory')
    key = None if a.dry_run else upload_key(a.key_file)
    import psutil
    psutil.Process().nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)

    def log(msg):
        print(f'{time.strftime("%H:%M:%S")} {msg}', flush=True)

    pub = Publisher(a.run, a, key, log)
    pub.tallies.update()            # the finished iterations first: the progress record carries the builder's score
    pub.progress_pass()
    if a.loop is not None:
        threading.Thread(target=progress_loop, args=(pub, a, log), daemon=True).start()
    while True:
        try:
            pub.cards_pass()
        except Exception:                              # a network hiccup or a half-written file: try again next pass
            if a.loop is None:
                raise
            log('pass failed:\n' + traceback.format_exc())
        if a.loop is None:
            break
        time.sleep(a.loop)
    return pub


if __name__ == '__main__':
    main()
