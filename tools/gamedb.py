"""The game database (docs/gamedb.md, docs/ENGINE.md 187): every self-play game of a run, replayed through the engine's
play-by-play, as Parquet tables a DuckDB query answers in well under a second - "how often does a Doomsday pile hold
card X since iteration 120, by opponent" is one SQL query.

  python tools/gamedb.py ingest RUN [--sample 1.0] [--jobs 1] [--iters A-B] [--db DIR] [--redo]   # new iterations
  python tools/gamedb.py ingest RUN --follow 60       # ... and again every 60 s, until stopped (beside a live run)
  python tools/gamedb.py query RUN "SELECT ..." [--csv | --json] [--max-rows 200]
  python tools/gamedb.py shell RUN                       # an SQL prompt (statements end with ';')
  python tools/gamedb.py prune RUN [--keep-iters 64] [--old-sample 0.1] [--delete-raw]
  python tools/gamedb.py status RUN
  python tools/gamedb.py series RUN [--since 120] [--by-opponent]   # the Doomsday series (doomsday_series)
  python tools/gamedb.py bauble RUN [--since N] [--by-kind]   # the Bauble targeting series (bauble_blunder_series)
  python tools/gamedb.py bench RUN [--iter N] [--games 200]   # replay cost per game (light and full)
  python tools/gamedb.py import-combo OUT_RUN FILES...    # combo_stats games files -> game records (validation)

RUN is a run directory (its records: <run>/records/iter_NNNN/, zero-core's `game_records` setting, zero/gamerec.py) or,
for query / shell / status / prune of a database alone, a run name. The database of run R lives in
`$MTGZ_GAMEDB/R`, else `<ZERO_BIG_DISK>/mtg-zero-gamedb/R` on a machine that names a big disk (~/.config/zero/local.env),
else `~/.cache/mtg-zero-gamedb/R`; `--db DIR` names the directory itself.

ingest. Each complete iteration of records (its DONE.json; an iteration already in the database is skipped: idempotent,
incremental) is replayed game by game from its seed and actions - CPU only, in --jobs processes at below-normal priority:
* every game LIGHT: the engine with the play-by-play on, the state exported only before the last move and at the end ->
  its `games` row (the decks of each seat, who was on the play, mulligans, the result and the end's method, turns,
  decisions, the resignation, the executor);
* a deterministic share --sample of the games (by game id: the same games every time) FULL: the state exported at every
  step and read by tools/combo_stats.py's Reader (the same code as `combo_stats.py scan`) -> its `events` (every
  play-by-play line and every choice of two or more options) and the combo tables (`doomsday`, `shows`, `spells`,
  `sneak`, `entered`) and its Bauble activations (`bauble`: tools/gamedb_bauble.py, ENGINE.md 194); its `games` row is
  the full replay's (`detailed` true).
Tables are written per iteration (`<db>/<table>/iter=NNNN/part.parquet`, replaced whole); `<db>/state.json` lists the
iterations ingested. A game that does not replay is never dropped silently: the iteration fails loudly.

prune. Keeps every game of the newest --keep-iters iterations and a deterministic --old-sample of the older ones (the
games, events and combo rows of the other games deleted); --delete-raw deletes the raw records of every ingested
iteration (<run>/records/iter_NNNN; never the run's training data).

query / shell. DuckDB over the tables and the views (docs/gamedb.md): doomsday_piles, doomsday_pile_cards, casts,
show_and_tell, sneak_attack, mulligans, game_results, bauble_activations. Needs `duckdb` and `pyarrow` (pip install duckdb pyarrow).

series / doomsday_series(run, since_iter, by_opponent). The Doomsday deck per iteration (a publisher's: plain dict rows):
resolutions and the Oracle's class at each (own pile / forced by the wheel / in hand / not findable / missed), the
undrawn share, games, wins and Oracle wins (docs/gamedb.md "The Doomsday series").
"""
import argparse
import base64
import collections
import glob
import gzip
import hashlib
import json
import os
import re
import shutil
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
for _p in (ROOT, os.path.join(ROOT, 'tools'), os.path.join(ROOT, 'third_party', 'zero-core', 'python')):
    if _p not in sys.path:
        sys.path.insert(0, _p)

FORMAT = 'mtgz-gamedb/1'
TABLES = ('games', 'events', 'doomsday', 'shows', 'spells', 'sneak', 'entered', 'bauble')   # bauble: gamedb_bauble.py
NO_CHOICE_EVENT = ('Priority', 'PayMana')     # decisions that are not `choice` events (their effects are lines)


def say(*a):
    print(*a, flush=True)


# ====================================================================================== where
def db_root():
    if os.environ.get('MTGZ_GAMEDB'):
        return os.environ['MTGZ_GAMEDB']
    import local_config as LC
    return LC.big_disk_path('mtg-zero-gamedb', default=os.path.join(os.path.expanduser('~'), '.cache', 'mtg-zero-gamedb'))


def db_dir(run, db=None):
    return db or os.path.join(db_root(), os.path.basename(os.path.normpath(run)))


def read_state(db):
    try:
        with open(os.path.join(db, 'state.json'), encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return {'format': FORMAT, 'iterations': {}}


def write_state(db, st):
    os.makedirs(db, exist_ok=True)
    tmp = os.path.join(db, f'state.json.{os.getpid()}.tmp')
    with open(tmp, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(st, f, indent=1)
    os.replace(tmp, os.path.join(db, 'state.json'))


def below_normal():
    """This process at below-normal priority (Windows) / nice 10 (elsewhere): ingest never competes with a run."""
    try:
        import psutil
        p = psutil.Process()
        p.nice(psutil.BELOW_NORMAL_PRIORITY_CLASS if os.name == 'nt' else 10)
    except Exception:               # noqa: BLE001 - best effort
        pass


# ====================================================================================== ids and sampling
def game_id(rec):
    """A game's id: 63 bits of blake2b over (iteration, source, game id, seed, actions) - unique across sources and
    restarts (a restarted worker's game ids begin again; its seeds and moves differ)."""
    k = f'{rec["it"]}|{rec["src"]}|{rec["gid"]}|{rec["seed"]}|{rec["n_actions"]}'.encode()
    return int.from_bytes(hashlib.blake2b(k, digest_size=8).digest(), 'little') & ((1 << 63) - 1)


def sampled(gid, rate):
    """Deterministic by game id: the same games are in a share `rate` every time (and a smaller share is a subset)."""
    if rate >= 1.0:
        return True
    if rate <= 0.0:
        return False
    h = int.from_bytes(hashlib.blake2b(gid.to_bytes(8, 'little'), digest_size=8, person=b'gamedb').digest(), 'little')
    return h < int(rate * (1 << 64))


def executor(src):
    s = str(src)
    return 'farm' if s.startswith('pool') else 'local' if s == 'local' else 'combo' if s == 'combo' else 'remote'


# ====================================================================================== decks of a game
def seat_decks(spec, setup):
    """(deck name per seat, main per seat or None) of a game: a metagame's pairing (its setup record), else the
    constructed spec's p0 / p1 (names from the setup or 'p0' / 'p1')."""
    try:
        su = json.loads(setup) if setup else {}
    except ValueError:
        su = {}
    pr = su.get('pairing') if isinstance(su, dict) else None
    if pr:
        names = list(pr['names'])
        decks = (spec.get('metagame') or {}).get('decks') or []
        mains = [decks[i]['main'] if i < len(decks) else None for i in pr['decks']]
        return names, mains
    d = spec.get('decks') or {}
    names = (su.get('names') if isinstance(su, dict) else None) or ['p0', 'p1']
    return list(names), [d.get('p0'), d.get('p1')]


# ====================================================================================== replay
LINE = re.compile(r'^T(\d+) (\w+) (.*)$')
PATTERNS = [
    ('cast', re.compile(r'^P(\d) casts (.+) \((\w+)')),
    ('land', re.compile(r'^P(\d) plays (.+)$')),
    ('mana', re.compile(r'^P(\d) activates a mana ability of (.+)$')),
    ('activate', re.compile(r'^P(\d) activates (.+) \((.+)\)$')),
    ('find', re.compile(r'^P(\d) finds (.+)$')),
    ('forced_find', re.compile(r"^P(\d)'s forced find: (\w+) \(drawn (true|false)\)")),
    ('look', re.compile(r'^P(\d) looks at (\d+) card')),
    ('choose', re.compile(r'^P(\d) chooses: (.+)$')),
    ('empty_draw', re.compile(r'^P(\d) draws from an empty library')),
    ('turn', re.compile(r'^turn (\d+): P(\d)$')),
    ('trigger', re.compile(r'^trigger of (.+) goes on the stack$')),
    ('counter', re.compile(r'^counters (.+)$')),
    ('attack', re.compile(r'^attacks with (.+)$')),
    ('mulligan', re.compile(r'^P(\d) takes mulligan (\d+)$')),
    ('wins', re.compile(r'^P(\d) wins the game')),
    ('life_half', re.compile(r'^P(\d) loses half their life, rounded up: (\d+)$')),
    ('damage', re.compile(r'^(\d+) damage to (.+?)(?:#\d+)?$')),
    ('target', re.compile(r'^target (.+?)(?:#\d+)?$')),
    ('fizzle', re.compile(r'^fizzles$')),
    ('move', re.compile(r'^(.+) (\w+) -> (\w+)$')),
]
ZONE_KEY = {'Hand': 'hand', 'Battlefield': 'battlefield', 'Graveyard': 'graveyard', 'Exile': 'exile'}


def _owner(st, card, zone):
    """The seat whose `zone` (after the step) holds `card`, when exactly one does (else None)."""
    if zone == 'Stack':
        cs = {e.get('controller') for e in st.get('stack') or [] if e.get('name') == card}
        return cs.pop() if len(cs) == 1 else None
    k = ZONE_KEY.get(zone)
    if k is None:
        return None
    ps = [p for p in (0, 1) if any(c.get('name') == card for c in st['players'][p].get(k) or [])]
    return ps[0] if len(ps) == 1 else None


def parse_line(ln, st):
    """A play-by-play line -> (turn, step, player, type, card, from_zone, to_zone, detail dict)."""
    m = LINE.match(ln)
    if not m:
        return None, None, None, 'other', None, None, None, {'text': ln}
    turn, step, rest = int(m.group(1)), m.group(2), m.group(3)
    for kind, rx in PATTERNS:
        x = rx.match(rest)
        if not x:
            continue
        g = x.groups()
        if kind == 'cast':
            return turn, step, int(g[0]), kind, g[1], g[2], 'Stack', {}
        if kind in ('land', 'mana', 'find'):
            return turn, step, int(g[0]), kind, g[1], None, None, {}
        if kind == 'activate':
            return turn, step, int(g[0]), kind, g[1], None, None, {'ability': g[2]}
        if kind == 'forced_find':
            return turn, step, int(g[0]), kind, None, None, None, {'class': g[1], 'drawn': g[2] == 'true'}
        if kind == 'look':
            return turn, step, int(g[0]), kind, None, None, None, {'n': int(g[1])}
        if kind == 'choose':
            return turn, step, int(g[0]), kind, None, None, None, {'text': g[1]}
        if kind == 'empty_draw':
            return turn, step, int(g[0]), kind, None, None, None, {}
        if kind == 'turn':
            return turn, step, int(g[1]), kind, None, None, None, {'turn': int(g[0])}
        if kind in ('trigger', 'counter'):
            return turn, step, None, kind, g[0], None, None, {}
        if kind == 'attack':
            return turn, step, st.get('active'), kind, g[0], None, None, {}
        if kind == 'mulligan':
            return turn, step, int(g[0]), kind, None, None, None, {'n': int(g[1])}
        if kind == 'wins':
            return turn, step, int(g[0]), kind, None, None, None, {}
        if kind == 'life_half':
            return turn, step, int(g[0]), kind, None, None, None, {'amount': int(g[1])}
        if kind == 'damage':
            pm = re.fullmatch(r'P(\d)', g[1])
            return (turn, step, int(pm.group(1)) if pm else None, kind, None if pm else g[1], None, None,
                    {'amount': int(g[0]), **({'player': True} if pm else {})})
        if kind == 'target':
            pm = re.fullmatch(r'P(\d)', g[0])
            return turn, step, int(pm.group(1)) if pm else None, kind, None if pm else g[0], None, None, {}
        if kind == 'fizzle':
            return turn, step, None, kind, None, None, None, {}
        if kind == 'move':
            return turn, step, _owner(st, g[0], g[2]), kind, g[0], g[1], g[2], {}
    return turn, step, None, 'other', None, None, None, {'text': rest}


def actions_of(rec):
    if rec.get('actions') is not None:
        b = base64.b64decode(rec['actions'])
        return list(memoryview(b).cast('H')) if len(b) else []
    steps = rec.get('steps') or []
    if not all(isinstance(a, int) for a in steps):
        raise ValueError(f'game {rec.get("gid")}: a step that is not an action (MTG self-play has none)')
    return list(steps)


def _final_fields(final, n_dec, method, rec, decks):
    pl = final['players']
    w = final.get('winner') if final.get('over') else None
    res = rec.get('result') or [None, None]
    return {'starting': final.get('starting'), 'turns': final.get('turn'), 'mull0': pl[0].get('mulligans'),
            'mull1': pl[1].get('mulligans'), 'life0': pl[0].get('life'), 'life1': pl[1].get('life'),
            'winner': rec['resigned'] if rec.get('resigned') is not None else w,
            'reason': 'Resigned' if rec.get('resigned') is not None else final.get('reason'),
            'method': 'resigned' if rec.get('resigned') is not None else method,
            'score0': res[0], 'score1': res[1] if len(res) > 1 else None, 'decisions': n_dec}


def replay_light(E, C, spec_txt, rec, decks):
    """Light replay (module docs): the game to its end with the log on, two exports. -> games fields."""
    sp = json.loads(spec_txt)
    sp['log'] = True
    g = E.Game('mtg', json.dumps(sp), int(rec['n']), int(rec['seed']))
    acts = actions_of(rec)
    n_dec = 0
    prev = None
    for i, a in enumerate(acts):
        if len(g.legal()) > 1:
            n_dec += 1
        if i == len(acts) - 1:
            prev = json.loads(g.export_state())
        g.step(int(a))
    final = json.loads(g.export_state())
    check_end(g, rec)
    method = C.method_of(final, prev, final.get('log') or [])
    return _final_fields(final, n_dec, method, rec, decks)


def check_end(g, rec):
    """The replayed game ends as its record says (a game that does not replay raises, never dropped silently)."""
    over = g.to_move is None
    if rec.get('resigned') is None and bool(rec.get('over')) != over:
        raise ValueError(f'game {rec["gid"]}: replayed over={over}, the record says over={rec.get("over")}')
    if over and rec.get('resigned') is None:
        got = [round(float(x), 5) for x in g.result()][:int(rec['n'])]
        if rec.get('result') is not None and got != [round(float(x), 5) for x in rec['result']]:
            raise ValueError(f'game {rec["gid"]}: replayed result {got}, the record says {rec["result"]}')


def replay_full(E, C, spec_txt, rec, decks, mains):
    """Full replay (module docs): the state at every step, the Reader of combo_stats.py, every line and choice.
    -> (games fields, events, combo dict)."""
    sp = json.loads(spec_txt)
    sp['log'] = True
    g = E.Game('mtg', json.dumps(sp), int(rec['n']), int(rec['seed']))
    R = C.Reader(decks)
    import gamedb_bauble
    B = gamedb_bauble.Reader()
    if mains and all(m is not None for m in mains):
        R.oracle_main = [list(m).count(C.ORACLE) for m in mains]
    st = json.loads(g.export_state())
    events = []
    lost = 0

    def lines_ev(lines, state, step_ix):
        for ln in lines:
            t, s, p, kind, card, fz, tz, det = parse_line(ln, state)
            events.append((len(events), step_ix, t, s, p, kind, card, fz, tz, det))
    lines_ev(st.get('log') or [], st, -1)
    if st.get('log_n', 0) > len(st.get('log') or []):
        lost += st['log_n'] - len(st['log'])
    prev = None
    n_dec = 0
    for i, a in enumerate(actions_of(rec)):
        d = st.get('decision') or {}
        opts = d.get('options') or []
        text = next((o.get('t') for o in opts if o.get('a') == a), None)
        if len(opts) > 1:
            n_dec += 1
            if d.get('kind') not in NO_CHOICE_EVENT:
                events.append((len(events), i, st.get('turn'), st.get('step'), d.get('player'), 'choice', text, None, None,
                               {'kind': d.get('kind'), 'n_options': len(opts), **({'source': d['source']} if d.get('source') else {})}))
        g.step(int(a))
        nxt = json.loads(g.export_state())
        k = nxt['log_n'] - st['log_n']
        lines = nxt['log'][-k:] if k > 0 else []
        if k > len(nxt['log']):
            # (a step whose lines the next state does not all carry: never since the engine exports every line a step
            # writes, ENGINE.md 191 - before it a state carried its newest 40 and a resolving Doomsday's oldest exile
            # lines were lost. Kept as a check: an event says how many, as combo_stats marks them)
            lost += k - len(nxt['log'])
            lines = ['(log lines lost)'] + lines
            events.append((len(events), i, nxt.get('turn'), nxt.get('step'), None, 'lines_lost', None, None, None,
                           {'n': k - len(nxt['log'])}))
        R.step(st, d.get('kind'), text, lines, nxt)
        B.step(i, st, d, [ln for ln in lines if ln != '(log lines lost)'])
        lines_ev([ln for ln in lines if ln != '(log lines lost)'], nxt, i)
        prev, st = st, nxt
    check_end(g, rec)
    final = st
    method = C.method_of(final, prev, R.recent)
    w = final.get('winner') if final.get('over') else None
    last_attack = None
    if w is not None:
        ts = [t for t, (act, _) in R.attacks.items() if act == w]
        if ts:
            last_attack = {'turn': max(ts), 'attackers': R.attacks[max(ts)][1]}
    combo = {'spells': R.spells, 'doomsday': R.doomsday, 'show': R.show, 'sneak': R.sneak, 'entered': R.entered,
             'last_attack': last_attack, 'lines_lost': lost, 'bauble': B.finish(g.to_move is None)}
    return _final_fields(final, n_dec, method, rec, decks), events, combo


# ====================================================================================== one chunk of games
def _worker_init():
    below_normal()
    import mtgz.az  # noqa: F401


def ingest_chunk(args):
    """[(record, spec JSON)], sample rate -> table rows (dict of lists of row dicts)."""
    items, rate = args
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    import combo_stats as C
    out = {t: [] for t in TABLES}
    t0 = time.process_time()
    for rec, spec_txt in items:
        gid = game_id(rec)
        spec = json.loads(spec_txt)
        decks, mains = seat_decks(spec, rec.get('setup'))
        ff = (spec.get('forced_find') or {}).get('p')
        full = sampled(gid, rate)
        try:
            if full:
                f, events, combo = replay_full(E, C, spec_txt, rec, decks, mains)
            else:
                f, events, combo = replay_light(E, C, spec_txt, rec, decks), None, None
        except Exception as e:         # a game that does not replay: loud (the iteration fails)
            raise RuntimeError(f'iteration {rec["it"]} source {rec["src"]} game {rec["gid"]} (seed {rec["seed"]}): '
                               f'{type(e).__name__}: {e}') from e
        nets = rec.get('nets') or [None, None]
        row = {'game_id': gid, 'iter': int(rec['it']), 'src': str(rec['src']), 'executor': executor(rec['src']),
               'machine': rec.get('machine'), 'gid': int(rec['gid']), 'seed': str(rec['seed']), 'spec': rec['spec'],
               'deck0': decks[0], 'deck1': decks[1], 'net_lo': nets[0], 'net_hi': nets[1], 'forced_find_p': ff,
               'over': bool(rec.get('over')), 'aborted': bool(rec.get('aborted')),
               'resigned': rec.get('resigned') is not None, 'n_actions': int(rec['n_actions']), 'detailed': full,
               'lines_lost': combo['lines_lost'] if combo else None,
               'last_attackers': json.dumps(combo['last_attack']) if combo and combo['last_attack'] else None, **f}
        out['games'].append(row)
        if not full:
            continue
        it = int(rec['it'])
        for seq, step_ix, t, s, p, kind, card, fz, tz, det in events:
            out['events'].append({'game_id': gid, 'iter': it, 'seq': seq, 'action': step_ix, 'turn': t, 'step': s,
                                  'player': p, 'type': kind, 'card': card, 'from_zone': fz, 'to_zone': tz,
                                  'detail': json.dumps(det, separators=(',', ':')) if det else None})
        for k, x in enumerate(combo['doomsday']):
            out['doomsday'].append({'game_id': gid, 'iter': it, 'idx': k, 'caster': x['p'], 'turn': x['turn'],
                                    'pile': x['pile'], 'pile_size': len(x['pile']), 'library': x['library'],
                                    'choices': x['choices'], 'hand': x['hand'], 'life': x['life'],
                                    'oracle_in_pile': x['oracle_in_pile'], 'oracle_in_hand': x['oracle_in_hand'],
                                    'oracle_where': x['oracle_where'], 'oracle': x['ff'], 'ff_drawn': x['ff_drawn']})
        for k, x in enumerate(combo['show']):
            put = x['put']
            out['shows'].append({'game_id': gid, 'iter': it, 'idx': k, 'caster': x['p'], 'turn': x['turn'],
                                'put_caster': put[x['p']] if put[x['p']] is not None else None,
                                'put_opponent': put[1 - x['p']] if put[1 - x['p']] is not None else None})
        for k, x in enumerate(combo['spells']):
            out['spells'].append({'game_id': gid, 'iter': it, 'idx': k, 'player': x['p'], 'card': x['card'],
                                  'turn': x['turn'], 'outcome': x['outcome'], 'countered_by': x['by']})
        for k, x in enumerate(combo['sneak']):
            out['sneak'].append({'game_id': gid, 'iter': it, 'idx': k, 'player': x['p'], 'turn': x['turn']})
        for k, x in enumerate(combo['entered']):
            out['entered'].append({'game_id': gid, 'iter': it, 'idx': k, 'player': x['p'], 'card': x['card'],
                                   'turn': x['turn'], 'how': x['how']})
        for x in combo['bauble']:
            out['bauble'].append({'game_id': gid, 'iter': it, **x})
    out['_cpu_s'] = time.process_time() - t0
    return out


# ====================================================================================== parquet
def schemas():
    import pyarrow as pa
    s, i, b, f, ls = pa.string(), pa.int32(), pa.bool_(), pa.float64(), pa.list_(pa.string())
    return {
        'games': pa.schema([('game_id', pa.int64()), ('iter', i), ('src', s), ('executor', s), ('machine', s), ('gid', pa.int64()),
                            ('seed', s), ('spec', s), ('deck0', s), ('deck1', s), ('net_lo', i), ('net_hi', i),
                            ('forced_find_p', f), ('over', b), ('aborted', b), ('resigned', b), ('n_actions', i),
                            ('detailed', b), ('lines_lost', i), ('last_attackers', s), ('starting', i), ('turns', i),
                            ('mull0', i), ('mull1', i), ('life0', i), ('life1', i), ('winner', i), ('reason', s),
                            ('method', s), ('score0', f), ('score1', f), ('decisions', i)]),
        'events': pa.schema([('game_id', pa.int64()), ('iter', i), ('seq', i), ('action', i), ('turn', i), ('step', s),
                             ('player', i), ('type', s), ('card', s), ('from_zone', s), ('to_zone', s), ('detail', s)]),
        'doomsday': pa.schema([('game_id', pa.int64()), ('iter', i), ('idx', i), ('caster', i), ('turn', i), ('pile', ls),
                               ('pile_size', i), ('library', i), ('choices', ls), ('hand', ls), ('life', i),
                               ('oracle_in_pile', b), ('oracle_in_hand', b), ('oracle_where', s), ('oracle', s),
                               ('ff_drawn', b)]),
        'shows': pa.schema([('game_id', pa.int64()), ('iter', i), ('idx', i), ('caster', i), ('turn', i),
                           ('put_caster', ls), ('put_opponent', ls)]),
        'spells': pa.schema([('game_id', pa.int64()), ('iter', i), ('idx', i), ('player', i), ('card', s), ('turn', i),
                             ('outcome', s), ('countered_by', s)]),
        'sneak': pa.schema([('game_id', pa.int64()), ('iter', i), ('idx', i), ('player', i), ('turn', i)]),
        'entered': pa.schema([('game_id', pa.int64()), ('iter', i), ('idx', i), ('player', i), ('card', s), ('turn', i),
                              ('how', s)]),
        'bauble': __import__('gamedb_bauble').schema(),
    }


def write_part(db, table, it, rows):
    """Iteration `it`'s rows of `table` -> <db>/<table>/iter=NNNN/part.parquet (replaced whole: a temp dir renamed)."""
    import pyarrow as pa
    import pyarrow.parquet as pq
    sch = schemas()[table]
    t = pa.Table.from_pylist(rows, schema=sch) if rows else sch.empty_table()
    d = os.path.join(db, table, f'iter={it:04d}')
    tmp = d + f'.tmp{os.getpid()}'
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    pq.write_table(t, os.path.join(tmp, 'part.parquet'), compression='zstd')
    if os.path.isdir(d):
        old = d + f'.old{os.getpid()}'
        os.replace(d, old)
        os.replace(tmp, d)
        shutil.rmtree(old, ignore_errors=True)
    else:
        os.replace(tmp, d)
    return os.path.getsize(os.path.join(d, 'part.parquet'))


# ====================================================================================== ingest
def parse_iters(s):
    if not s:
        return None
    a, _, b = s.partition('-')
    return int(a), int(b) if b else int(a)


def ingest(run, db=None, sample=1.0, jobs=1, iters=None, redo=False, chunk=64, log=say):
    from zero import gamerec as GR
    below_normal()
    db = db_dir(run, db)
    st = read_state(db)
    st.setdefault('run', os.path.basename(os.path.normpath(run)))
    todo = [it for it in GR.done_iterations(run) if (redo or str(it) not in st['iterations'])
            and (iters is None or iters[0] <= it <= iters[1])]
    if not todo:
        log(f'{run}: nothing new to ingest ({len(st["iterations"])} iterations in {db})')
        return st
    pool = None
    if jobs > 1:
        from concurrent.futures import ProcessPoolExecutor
        pool = ProcessPoolExecutor(jobs, initializer=_worker_init)
    try:
        for it in todo:
            t0 = time.time()
            specs, recs = GR.read_iteration(run, it)
            if not recs:
                raise SystemExit(f'iteration {it}: DONE.json lists no records (an empty iteration is never ingested '
                                 f'silently: look at {GR.iter_dir(run, it)})')
            items = [(r, specs[r['spec']]) for r in recs]
            parts = [(items[k:k + chunk], sample) for k in range(0, len(items), chunk)]
            res = list(pool.map(ingest_chunk, parts)) if pool else [ingest_chunk(p) for p in parts]
            rows = {t: [x for r in res for x in r[t]] for t in TABLES}
            ids = [r['game_id'] for r in rows['games']]
            if len(set(ids)) != len(ids):
                raise SystemExit(f'iteration {it}: {len(ids) - len(set(ids))} duplicate game ids')
            size = sum(write_part(db, t, it, rows[t]) for t in TABLES)
            cpu = sum(r['_cpu_s'] for r in res)
            done = GR.read_done(run, it)
            st['iterations'][str(it)] = {'games': len(rows['games']), 'detailed': sum(1 for g in rows['games'] if g['detailed']),
                                         'sample': sample, 'events': len(rows['events']), 'bytes': size,
                                         'cpu_s': round(cpu, 2), 'wall_s': round(time.time() - t0, 2),
                                         'records_bytes': done.get('bytes'), 'missing': done.get('missing'),
                                         'time': time.time()}
            write_state(db, st)
            n = len(rows['games'])
            log(f'iter {it}: {n} games ({st["iterations"][str(it)]["detailed"]} detailed), {len(rows["events"])} events, '
                f'{size / 1e6:.2f} MB parquet, {cpu:.1f} CPU-s ({cpu / max(1, n) * 1000:.1f} ms a game), '
                f'{time.time() - t0:.1f} s' + (f'; MISSING records {done["missing"]}' if done.get('missing') else ''))
    finally:
        if pool:
            pool.shutdown()
    return st


# ====================================================================================== query
VIEWS = r"""
CREATE OR REPLACE VIEW game_results AS
SELECT g.game_id, g.iter, s.seat, CASE s.seat WHEN 0 THEN g.deck0 ELSE g.deck1 END AS deck,
       CASE s.seat WHEN 0 THEN g.deck1 ELSE g.deck0 END AS opp_deck, (g.starting = s.seat) AS on_play,
       CASE s.seat WHEN 0 THEN g.score0 ELSE g.score1 END AS score,
       CASE WHEN g.winner = s.seat THEN 'win' WHEN g.winner IS NULL THEN 'draw' ELSE 'loss' END AS outcome,
       g.method, g.turns, g.resigned, g.executor,
       CASE s.seat WHEN 0 THEN g.mull0 ELSE g.mull1 END AS mulligans
FROM games g, (SELECT 0 AS seat UNION ALL SELECT 1) s;

CREATE OR REPLACE VIEW mulligans AS
SELECT game_id, iter, seat, deck, opp_deck, on_play, mulligans, score, outcome FROM game_results;

CREATE OR REPLACE VIEW doomsday_piles AS
SELECT d.game_id, d.iter, d.idx, d.caster, CASE d.caster WHEN 0 THEN g.deck0 ELSE g.deck1 END AS caster_deck,
       CASE d.caster WHEN 0 THEN g.deck1 ELSE g.deck0 END AS opp_deck, d.turn, d.pile, d.pile_size,
       d.pile[1] AS card1, d.pile[2] AS card2, d.pile[3] AS card3, d.pile[4] AS card4, d.pile[5] AS card5,
       d.oracle, d.oracle_in_pile, d.oracle_in_hand, d.oracle_where,
       CASE WHEN d.oracle_in_pile THEN 'in_pile' WHEN d.oracle_in_hand THEN 'in_hand'
            WHEN d.oracle = 'not_findable' THEN 'not_findable' ELSE 'missed' END AS oracle_place,
       (d.oracle = 'forced') AS forced, d.ff_drawn, g.forced_find_p, d.life, d.hand, d.choices,
       (g.winner = d.caster) AS won, CASE WHEN g.winner = d.caster THEN g.method END AS win_method, g.method, g.turns
FROM doomsday d JOIN games g USING (game_id, iter);

CREATE OR REPLACE VIEW doomsday_pile_cards AS
SELECT p.game_id, p.iter, p.idx, p.caster, p.caster_deck, p.opp_deck, p.oracle, p.forced, p.won, p.win_method,
       u.pos, u.card
FROM doomsday_piles p, UNNEST(p.pile) WITH ORDINALITY AS u(card, pos);

CREATE OR REPLACE VIEW casts AS
SELECT e.game_id, e.iter, e.seq, e.turn, e.step, e.player, e.card, e.from_zone AS cast_from,
       CASE e.player WHEN 0 THEN g.deck0 ELSE g.deck1 END AS deck, CASE e.player WHEN 0 THEN g.deck1 ELSE g.deck0 END AS opp_deck,
       (g.winner = e.player) AS won
FROM events e JOIN games g USING (game_id, iter) WHERE e.type = 'cast';

CREATE OR REPLACE VIEW show_and_tell AS
SELECT s.game_id, s.iter, s.idx, s.caster, CASE s.caster WHEN 0 THEN g.deck0 ELSE g.deck1 END AS caster_deck,
       CASE s.caster WHEN 0 THEN g.deck1 ELSE g.deck0 END AS opp_deck, s.turn, s.put_caster, s.put_opponent,
       s.put_caster[1] AS caster_pick, s.put_opponent[1] AS opponent_pick, (g.winner = s.caster) AS won, g.method
FROM shows s JOIN games g USING (game_id, iter);

CREATE OR REPLACE VIEW sneak_attack AS
SELECT s.game_id, s.iter, s.idx, s.player, CASE s.player WHEN 0 THEN g.deck0 ELSE g.deck1 END AS deck, s.turn,
       (SELECT list(e.card) FROM entered e WHERE e.game_id = s.game_id AND e.player = s.player AND e.how = 'sneak'
          AND e.turn = s.turn) AS sneaked_in, (g.winner = s.player) AS won, g.method
FROM sneak s JOIN games g USING (game_id, iter);
"""


def connect(db):
    import duckdb
    con = duckdb.connect()
    for t in TABLES:
        files = glob.glob(os.path.join(db, t, 'iter=*', '*.parquet'))
        if files:
            pat = os.path.join(db, t, 'iter=*', '*.parquet').replace('\\', '/')
            con.execute(f"CREATE VIEW {t} AS SELECT * FROM read_parquet('{pat}', hive_partitioning = false)")
        else:
            con.register(f'_{t}_empty', schemas()[t].empty_table())
            con.execute(f'CREATE VIEW {t} AS SELECT * FROM _{t}_empty')
    con.execute(VIEWS)
    import gamedb_bauble
    con.execute(gamedb_bauble.view_sql([d[0] for d in con.execute('SELECT * FROM games LIMIT 0').description]))
    return con


def show_result(cur, fmt='table', max_rows=200, out=sys.stdout):
    cols = [d[0] for d in cur.description] if cur.description else []
    rows = cur.fetchall() if cols else []
    if fmt == 'json':
        json.dump([dict(zip(cols, r)) for r in rows], out, default=str, indent=1)
        out.write('\n')
    elif fmt == 'csv':
        import csv
        w = csv.writer(out, lineterminator='\n')
        w.writerow(cols)
        w.writerows(rows)
    else:
        cells = [[('NULL' if v is None else str(v)) for v in r] for r in rows[:max_rows]]
        width = [min(60, max([len(c)] + [len(r[i]) for r in cells])) for i, c in enumerate(cols)]
        out.write(' | '.join(c.ljust(width[i]) for i, c in enumerate(cols)) + '\n')
        out.write('-+-'.join('-' * w for w in width) + '\n')
        for r in cells:
            out.write(' | '.join(v[:60].ljust(width[i]) for i, v in enumerate(r)) + '\n')
        out.write(f'({len(rows)} rows' + (f', {max_rows} shown' if len(rows) > max_rows else '') + ')\n')
    return rows


def query(run, sql, db=None, fmt='table', max_rows=200):
    con = connect(db_dir(run, db))
    t0 = time.time()
    cur = con.execute(sql)
    rows = show_result(cur, fmt, max_rows)
    if fmt == 'table':
        print(f'({time.time() - t0:.3f} s)', file=sys.stderr)
    return rows


DOOMSDAY_SERIES = """
WITH r AS (
  SELECT iter, {opp} AS opp_deck, count(*) AS resolutions,
         count(*) FILTER (WHERE oracle = 'voluntary') AS own_pile,
         count(*) FILTER (WHERE oracle = 'forced') AS forced,
         count(*) FILTER (WHERE oracle = 'in_hand') AS in_hand,
         count(*) FILTER (WHERE oracle = 'not_findable') AS not_findable,
         count(*) FILTER (WHERE oracle = 'missed') AS missed,
         count(*) FILTER (WHERE ff_drawn IS NOT TRUE) AS undrawn,
         count(*) FILTER (WHERE ff_drawn IS NOT TRUE AND oracle = 'voluntary') AS undrawn_own_pile
  FROM doomsday_piles WHERE caster_deck = $deck AND iter >= $since GROUP BY ALL),
g AS (
  SELECT iter, {opp} AS opp_deck, count(*) AS games, count(*) FILTER (WHERE detailed) AS detailed_games,
         count(*) FILTER (WHERE outcome = 'win') AS wins,
         count(*) FILTER (WHERE outcome = 'win' AND method = 'oracle') AS oracle_wins
  FROM game_results JOIN (SELECT game_id, detailed FROM games) USING (game_id)
  WHERE deck = $deck AND iter >= $since GROUP BY ALL)
SELECT g.iter, g.opp_deck, g.games, g.detailed_games, g.wins, g.oracle_wins,
       coalesce(r.resolutions, 0) AS resolutions, coalesce(r.own_pile, 0) AS own_pile, coalesce(r.forced, 0) AS forced,
       coalesce(r.in_hand, 0) AS in_hand, coalesce(r.not_findable, 0) AS not_findable, coalesce(r.missed, 0) AS missed,
       coalesce(r.undrawn, 0) AS undrawn, coalesce(r.undrawn_own_pile, 0) AS undrawn_own_pile,
       r.own_pile / r.resolutions AS own_pile_share, r.undrawn_own_pile / nullif(r.undrawn, 0) AS undrawn_own_pile_share
FROM g LEFT JOIN r ON r.iter = g.iter AND r.opp_deck IS NOT DISTINCT FROM g.opp_deck
ORDER BY g.iter, g.opp_deck"""


def doomsday_series(run, since_iter=0, by_opponent=False, deck='doomsday_8027957', db=None):
    """The Doomsday deck's per-iteration series (docs/gamedb.md "The Doomsday series"; the delverai publisher's) -> plain
    dict rows, one per iteration (and opponent deck with `by_opponent`): `games` / `wins` / `oracle_wins` of `deck` (every
    game), `detailed_games` (the replayed-in-full ones the pile counts come from), the caster's `resolutions` and how
    the Oracle stood at each - `own_pile` (the network put it in its own pile: 'voluntary'), `forced` (the wheel's
    forced find), `in_hand`, `not_findable`, `missed` -, `undrawn` / `undrawn_own_pile` (the resolutions the wheel's draw
    passed over: the exit gate's, ENGINE.md 180) and the shares `own_pile_share`, `undrawn_own_pile_share` (None
    without a resolution). `opp_deck` is None unless `by_opponent`. One query, milliseconds."""
    con = connect(db_dir(run, db))
    sql = DOOMSDAY_SERIES.replace('{opp}', 'opp_deck' if by_opponent else 'NULL::VARCHAR')
    cur = con.execute(sql, {'deck': deck, 'since': int(since_iter)})
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, r)) for r in cur.fetchall()]


def bauble_blunder_series(run, since_iter=0, by_kind=False, kind=None, db=None):
    """The Bauble targeting series (docs/gamedb.md "Bauble targeting", tools/gamedb_bauble.py; the delverai
    publisher's) -> plain dict rows, one per iteration and Bauble ('urza', 'mishra', 'all'; and kind of game with
    `by_kind`, `kind` None otherwise; `kind` filters to one): `activations`, `excluded` (targeting the opponent impossible
    or meaningless), `counted` (the denominator), `self_targets`, `blunders` (the user's rules), `self_with_fetch` (a
    Mishra's Bauble at oneself with a fetchland available: not a blunder) and the shares of each in `counted` with their
    binomial standard errors (`self_share` / `self_se`, `blunder_share` / `blunder_se`, `self_with_fetch_share` /
    `self_with_fetch_se`; None without a counted activation)."""
    import gamedb_bauble
    return gamedb_bauble.series(connect(db_dir(run, db)), since_iter, by_kind, kind)


def shell(run, db=None):
    con = connect(db_dir(run, db))
    say(f'gamedb {db_dir(run, db)}: tables {", ".join(TABLES)}; views game_results, mulligans, doomsday_piles, '
        f'doomsday_pile_cards, casts, show_and_tell, sneak_attack, bauble_activations. End statements with ";", .quit to leave.')
    buf = []
    while True:
        try:
            line = input('gamedb> ' if not buf else '   ...> ')
        except EOFError:
            break
        if line.strip() in ('.quit', '.exit', r'\q'):
            break
        buf.append(line)
        if line.rstrip().endswith(';'):
            sql = '\n'.join(buf)
            buf = []
            try:
                t0 = time.time()
                show_result(con.execute(sql))
                say(f'({time.time() - t0:.3f} s)')
            except Exception as e:         # noqa: BLE001 - an SQL error is the user's to fix
                say(f'error: {e}')


# ====================================================================================== prune / status
def prune(run, db=None, keep_iters=64, old_sample=0.1, delete_raw=False, log=say):
    """Retention (module docs)."""
    import pyarrow.parquet as pq
    db = db_dir(run, db)
    st = read_state(db)
    its = sorted(int(k) for k in st['iterations'])
    old = [it for it in its if its and it <= its[-1] - keep_iters]
    for it in old:
        info = st['iterations'][str(it)]
        if info.get('pruned_to') is not None and info['pruned_to'] <= old_sample:
            continue
        gpath = os.path.join(db, 'games', f'iter={it:04d}', 'part.parquet')
        games = pq.read_table(gpath).to_pylist()
        keep = {g['game_id'] for g in games if sampled(g['game_id'], old_sample)}
        for t in TABLES:
            p = os.path.join(db, t, f'iter={it:04d}', 'part.parquet')
            if os.path.exists(p):
                rows = [r for r in pq.read_table(p).to_pylist() if r['game_id'] in keep]
                write_part(db, t, it, rows)
        info['pruned_to'] = old_sample
        info['games_kept'] = len(keep)
        log(f'iter {it}: kept {len(keep)} of {len(games)} games (sample {old_sample})')
    if delete_raw and run and os.path.isdir(os.path.join(run, 'records')):
        from zero import gamerec as GR
        for it in its:
            if os.path.isdir(GR.iter_dir(run, it)):
                GR.drop(run, it)
                st['iterations'][str(it)]['raw_deleted'] = True
                log(f'iter {it}: raw records deleted')
    write_state(db, st)
    return st


def status(run, db=None):
    db = db_dir(run, db)
    st = read_state(db)
    its = sorted(int(k) for k in st['iterations'])
    say(f'{db}: {len(its)} iterations' + (f' ({its[0]}-{its[-1]})' if its else ''))
    if its:
        tot = {k: sum(st['iterations'][str(i)].get(k) or 0 for i in its) for k in ('games', 'detailed', 'events', 'bytes', 'cpu_s')}
        say(f'  {tot["games"]} games ({tot["detailed"]} detailed), {tot["events"]} events, {tot["bytes"] / 1e6:.1f} MB, '
            f'{tot["cpu_s"]:.0f} CPU-s ingesting ({tot["cpu_s"] / max(1, tot["games"]) * 1000:.1f} ms a game)')
    if run and os.path.isdir(os.path.join(run, 'records')):
        from zero import gamerec as GR
        pend = [it for it in GR.done_iterations(run) if str(it) not in st['iterations']]
        say(f'  records waiting: {len(pend)} iteration(s)' + (f' ({pend[0]}-{pend[-1]})' if pend else ''))


# ====================================================================================== bench
def bench(run, it=None, games=200, log=say):
    """Replay cost per game of a run's records (light and full) on this core."""
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    import combo_stats as C
    from zero import gamerec as GR
    below_normal()
    its = GR.done_iterations(run)
    it = it or its[-1]
    specs, recs = GR.read_iteration(run, it)
    recs = recs[:games]
    out = {}
    for mode in ('light', 'full'):
        t0 = time.process_time()
        steps = 0
        for r in recs:
            sp = specs[r['spec']]
            decks, mains = seat_decks(json.loads(sp), r.get('setup'))
            if mode == 'light':
                replay_light(E, C, sp, r, decks)
            else:
                replay_full(E, C, sp, r, decks, mains)
            steps += r['n_actions']
        dt = time.process_time() - t0
        out[mode] = {'ms_per_game': dt / len(recs) * 1000, 'us_per_action': dt / max(1, steps) * 1e6,
                     'actions_per_game': steps / len(recs)}
        log(f'{mode}: {out[mode]["ms_per_game"]:.1f} ms a game ({out[mode]["us_per_action"]:.0f} us an action, '
            f'{out[mode]["actions_per_game"]:.0f} actions a game; {len(recs)} games of iteration {it})')
    return out


# ====================================================================================== import-combo
def import_combo(out_run, files, it=1, log=say):
    """combo_stats.py games files (rows with a move log of decisions) -> game records of a pseudo-run `out_run`
    (records/iter_NNNN, one iteration per file label... all in iteration `it`, source 'combo'): each game replayed with
    the single-option steps the driver took between its logged moves, so every action is in the record. Validation:
    the database's views against `combo_stats.py scan` of the same files."""
    import mtgz.az  # noqa: F401
    from mtgz import _engine as E
    from zero import gamerec as GR
    w = GR.Records(out_run)
    n = 0
    index = []
    for path in files:
        rec = json.load(gzip.open(path, 'rt', encoding='utf-8'))
        spec_txt = rec['spec']
        setup = json.dumps({'names': rec['decks']}, separators=(',', ':'))
        items = []
        for row in rec['rows']:
            seed = E.game_seed_of(int(rec['driver_seed']), int(row['gid']), False)
            g = E.Game('mtg', rec['spec'], 2, seed)
            acts = []

            def auto():
                while g.to_move is not None and len(g.legal()) == 1:
                    a = g.legal()[0]
                    g.step(a)
                    acts.append(a)
            for seat, a, legal in row['moves']:
                if not (len(legal) == 1 and g.to_move == seat and list(g.legal()) == list(legal)):
                    auto()
                if g.to_move != seat or list(g.legal()) != list(legal):
                    raise RuntimeError(f'{path} game {row["gid"]}: the rebuilt game left its move log')
                g.step(a)
                acts.append(a)
            if row['over']:
                auto()
            gid = len(index)
            index.append({'file': os.path.basename(path), 'gid': row['gid'], 'game': gid})
            items.append(({'v': 1, 'gid': gid, 'seed': seed, 'spec': GR.spec_key(spec_txt), 'n': 2,
                           'actions': base64.b64encode(memoryview(__import__('array').array('H', acts)).tobytes()).decode(),
                           'n_actions': len(acts), 'setup': setup, 'over': bool(row['over']), 'aborted': bool(row.get('aborted')),
                           'resigned': None, 'result': row['result'], 'decisions': row.get('decisions'), 'rounds': None,
                           'nets': None}, spec_txt))
        w.add(it, 'combo', items)
        n += len(items)
    w.close(it)
    with open(os.path.join(GR.iter_dir(out_run, it), 'combo_index.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump(index, f)
    log(f'{n} games of {len(files)} files -> {GR.iter_dir(out_run, it)}')
    return index


# ====================================================================================== main
def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('ingest')
    p.add_argument('run')
    p.add_argument('--db', default=None)
    p.add_argument('--sample', type=float, default=1.0, help='share of the games replayed full (events, combo tables)')
    p.add_argument('--jobs', type=int, default=1)
    p.add_argument('--iters', default=None, help='A-B (inclusive) or N')
    p.add_argument('--redo', action='store_true', help='ingest iterations already in the database again')
    p.add_argument('--chunk', type=int, default=64)
    p.add_argument('--follow', type=float, default=0, help='keep ingesting new iterations, polling every this many '
                   'seconds (0: once)')
    q = sub.add_parser('query')
    q.add_argument('run')
    q.add_argument('sql')
    q.add_argument('--db', default=None)
    q.add_argument('--csv', action='store_true')
    q.add_argument('--json', action='store_true')
    q.add_argument('--max-rows', type=int, default=200)
    s = sub.add_parser('shell')
    s.add_argument('run')
    s.add_argument('--db', default=None)
    r = sub.add_parser('prune')
    r.add_argument('run')
    r.add_argument('--db', default=None)
    r.add_argument('--keep-iters', type=int, default=64)
    r.add_argument('--old-sample', type=float, default=0.1)
    r.add_argument('--delete-raw', action='store_true')
    t = sub.add_parser('status')
    t.add_argument('run')
    t.add_argument('--db', default=None)
    b = sub.add_parser('bench')
    b.add_argument('run')
    b.add_argument('--iter', type=int, default=None)
    b.add_argument('--games', type=int, default=200)
    d = sub.add_parser('series', help='the Doomsday deck per iteration (doomsday_series)')
    d.add_argument('run')
    d.add_argument('--db', default=None)
    d.add_argument('--since', type=int, default=0)
    d.add_argument('--by-opponent', action='store_true')
    d.add_argument('--deck', default='doomsday_8027957')
    u = sub.add_parser('bauble', help='the Bauble targeting series per iteration (bauble_blunder_series)')
    u.add_argument('run')
    u.add_argument('--db', default=None)
    u.add_argument('--since', type=int, default=0)
    u.add_argument('--by-kind', action='store_true')
    u.add_argument('--kind', default=None)
    c = sub.add_parser('import-combo')
    c.add_argument('out_run')
    c.add_argument('files', nargs='+')
    c.add_argument('--iter', type=int, default=1)
    a = ap.parse_args(argv)
    if a.cmd == 'ingest':
        while True:
            # (following: a timestamp on each line, the idle polls' "nothing new" left out)
            quiet = lambda m: None if 'nothing new' in m else say(time.strftime('%H:%M:%S ') + m)  # noqa: E731
            ingest(a.run, a.db, a.sample, a.jobs, parse_iters(a.iters), a.redo, a.chunk, log=quiet if a.follow else say)
            if not a.follow:
                break
            a.redo = False
            time.sleep(a.follow)
    elif a.cmd == 'query':
        query(a.run, a.sql, a.db, 'csv' if a.csv else 'json' if a.json else 'table', a.max_rows)
    elif a.cmd == 'shell':
        shell(a.run, a.db)
    elif a.cmd == 'prune':
        prune(a.run, a.db, a.keep_iters, a.old_sample, a.delete_raw)
    elif a.cmd == 'status':
        status(a.run, a.db)
    elif a.cmd == 'series':
        for r in doomsday_series(a.run, a.since, a.by_opponent, a.deck, a.db):
            say(json.dumps(r, default=str))
    elif a.cmd == 'bauble':
        for r in bauble_blunder_series(a.run, a.since, a.by_kind, a.kind, a.db):
            say(json.dumps(r, default=str))
    elif a.cmd == 'bench':
        bench(a.run, a.iter, a.games)
    else:
        files = [f for x in a.files for f in (sorted(glob.glob(x)) or [x])]
        import_combo(a.out_run, files, a.iter)


if __name__ == '__main__':
    main()
