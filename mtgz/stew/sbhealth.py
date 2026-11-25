"""The stew's sideboarding health (docs/ENGINE.md 260): every generation's report says whether the specialists are
learning to sideboard - what their matchup models' choice tables board, how the seats of each deck mode do per game
number, and whether the colour-conditional answers (Pyroblast, Hydroblast, ...) are aimed at the colour they need.

Per specialist, at the generation's version (the newest matchup version at or before its checkpoint's iteration; the
play and card use over the window of its newest `iters` iterations, default the recipe's turn_iters):

* **table plan** (`table_plan`): per deck of the pairing, the newest version's choice table - the swaps (cards brought
  in) per context (min / mean / max over the 24 contexts), each card's mean copies in / out, the most common 60s as
  changes against the main - and its streak: how many consecutive versions (newest first) the deck has been in the same
  state ('all main': every context the main 60; 'no table': the version published none, its seats choose by search;
  'boards').
* **play** (`play_by_mode`): per deck, game number and deck mode (main / dealt / table / chosen) the seats, their mean
  score and the mean cards brought in - from the game database when the run is ingested over the window
  (tools/gamedb.py, docs/gamedb.md "Best-of-three matches"), else from the run's own best-of-three game log
  (<run>/games/*.jsonl: local self-play only, mtgz.az.matchup.load_games).
* **card use** (`card_use`): every cast of a colour-conditional answer (a spell whose every mode is `only_if HasColor`
  in the card IR: `conditional_answers`) per casting deck and game number - its target's colour from the IR printings
  (`colour_map`: a split card's faces, a double-faced card's faces, tokens as their creating effect prints them;
  `target_colours`) -> right colour / WRONG colour (the spell does nothing; a land is colourless) / unknown colour
  (never guessed, counted apart) / no target; the counter mode's share (the mode that counters a spell). Only from the
  game database: a run not ingested says so (never a silent zero).
* **the model's data** (`model_health`, docs/ENGINE.md 273): a retrain newer than the newest version that FAILED
  ("matchup model: no new version since vNNNN (last retrain failed: ...)"), the newest version STARVED ("fitted on N
  games (< X)": fewer than `matchup.MIN_GAMES_PER_ITER` times its window's iterations), games of its window
  left out (records another engine played) - each an alert.
* **alerts**: a deck's table all main for `all_main_versions` or more consecutive versions; a card's wrong-colour rate
  above its alert line (`misaim_alert` 'reference': the pb2 reference below - the level before pb3's matchup model found
  a signal; a number: that rate for every card) over at least `min_casts` casts; card use not ingested.

**The reference** (`REFERENCE`): the Delver best-of-three lineage's blast targeting, the maintainer's notes
"blast_targeting_pb3_2026-11-16" and "sideboard_play_analysis_2026-11-15" (pb2 at iteration 96, pb3 at 88; all seats,
the same scripts): do-nothing casts (wrong colour or a land) and the counter mode's share. The user's hypothesis
(supported there): pb3's matchup model found a signal at once because pb2 had practised the sideboard cards on dealt
decks first (dealt p 0.5, swaps [1, 8], 166 iterations), so the misaim rate is the matchup model's readiness signal -
'pb3-like' at or below pb3's rate, 'pb2-like' at or above pb2's.

**Plan vs no plan** (`recipe.judge.plan_arm`, off by default; the jobs are `mtgz.stew.judge.plan_arm_jobs`): per
specialist that trained, its newest net on both seats, one seat its newest table, the other an all-main table; the
statistics here (`plan_stats`).

`recipe.sb_health` (default on; false: off): {iters, misaim_alert, misaim_fallback, min_casts, all_main_versions,
gamedb}. The report never fails the stew: a missing database or game log is said in the section.
"""
import collections
import glob
import json
import math
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT = {'iters': None, 'misaim_alert': 'reference', 'misaim_fallback': 0.15, 'min_casts': 30,
           'all_main_versions': 2, 'gamedb': None}
# The Delver bo3 lineage's blast targeting (module docs): do-nothing share of casts (wrong colour or a land) and the
# counter mode's share, per card. Source: the maintainer's notes blast_targeting_pb3_2026-11-16 and
# sideboard_play_analysis_2026-11-15 (2026-11-16).
REFERENCE = {
    'source': 'notes blast_targeting_pb3_2026-11-16 + sideboard_play_analysis_2026-11-15 (Delver bo3 lineage, all seats)',
    'pb2': {'at': 'pb2 @ iter 96', 'misaim': {'Pyroblast': 0.32, 'Hydroblast': 0.37},
            'counter': {'Pyroblast': 0.12, 'Hydroblast': 0.05}},
    'pb3': {'at': 'pb3 @ iter 88', 'misaim': {'Pyroblast': 0.15, 'Hydroblast': 0.13},
            'counter': {'Pyroblast': 0.20, 'Hydroblast': 0.20}},
}
STATES = ('all main', 'no table', 'boards')


def options(recipe):
    """recipe.sb_health -> the options with defaults, or None (off)."""
    v = (recipe or {}).get('sb_health', True)
    if v is None or v is False:
        return None
    out = dict(DEFAULT)
    if isinstance(v, dict):
        out.update(v)
    return out


def check_options(v):
    """recipe.sb_health's check (config.normalize) -> the error message or None."""
    if v is None or isinstance(v, bool):
        return None
    if not isinstance(v, dict):
        return 'recipe.sb_health: true / false or {' + ', '.join(DEFAULT) + '}'
    bad = sorted(set(v) - set(DEFAULT))
    if bad:
        return f'recipe.sb_health: unknown keys {bad} ({sorted(DEFAULT)})'
    o = dict(DEFAULT, **v)
    if o['iters'] is not None and (isinstance(o['iters'], bool) or not isinstance(o['iters'], int) or o['iters'] < 1):
        return f"recipe.sb_health.iters {o['iters']!r}: a positive integer (null: the recipe's turn_iters)"
    m = o['misaim_alert']
    if m != 'reference' and (isinstance(m, bool) or not isinstance(m, (int, float)) or not 0 < m < 1):
        return f"recipe.sb_health.misaim_alert {m!r}: 'reference' (the pb2 level) or a rate in (0, 1)"
    f = o['misaim_fallback']
    if isinstance(f, bool) or not isinstance(f, (int, float)) or not 0 < f < 1:
        return f"recipe.sb_health.misaim_fallback {f!r}: a rate in (0, 1) (a card without a reference)"
    for k in ('min_casts', 'all_main_versions'):
        if isinstance(o[k], bool) or not isinstance(o[k], int) or o[k] < 1:
            return f'recipe.sb_health.{k} {o[k]!r}: a positive integer'
    if o['gamedb'] is not None and not isinstance(o['gamedb'], str):
        return 'recipe.sb_health.gamedb: the game databases\' root directory (null: tools/gamedb.py\'s)'
    return None


# ------------------------------------------------------------------ the card IR: colours and conditional answers
def _ir_cards(root=ROOT):
    out = []
    for f in sorted(glob.glob(os.path.join(root, 'data', 'cards', '*.ir.json'))):
        with open(f, encoding='utf-8') as fh:
            out += json.load(fh).get('cards') or []
    return out


def printing_colours(p):
    """One printing's (a face's, a token's) colours: its face colours / colour indicator if it has one, else its mana
    cost's coloured symbols (CR 105.2, 202.2) -> frozenset of 'W'/'U'/'B'/'R'/'G' (empty: colourless)."""
    for k in ('face_colors', 'color_indicator'):
        v = p.get(k)
        if isinstance(v, list) and v:
            return frozenset(str(c)[:1].upper() for c in v)
    mc = p.get('mana_cost')
    if isinstance(mc, dict):
        return frozenset(c for c, _ in mc.get('colored') or [])
    return frozenset()


def _tokens(x, out):
    """Every token printing under an IR value (a CreateToken's `token_printing`): (name, colours)."""
    if isinstance(x, dict):
        tp = x.get('token_printing')
        if isinstance(tp, dict) and isinstance(tp.get('name'), str):
            out.append((tp['name'], printing_colours(tp)))
        for v in x.values():
            _tokens(v, out)
    elif isinstance(x, list):
        for v in x:
            _tokens(v, out)


def colour_map(cards=None):
    """{object name: frozenset of colours, or None (two sources disagree: unknown, never guessed)} from the card IR.
    A card's name its printing's colours (a split card, CR 709.4: its faces' together; a double-faced or adventure card
    its front's, CR 712.8a); each face's name its own; each token its creating effect's printing."""
    cards = _ir_cards() if cards is None else cards
    seen = collections.defaultdict(set)
    for c in cards:
        p = c.get('printing') or {}
        faces = p.get('split_faces') or []
        if faces:
            seen[c['name']].add(frozenset().union(*(printing_colours(f) for f in faces)))
            for f in faces:
                seen[f['name']].add(printing_colours(f))
        else:
            seen[c['name']].add(printing_colours(p))
            if ' // ' in c['name']:
                seen[c['name'].split(' // ')[0]].add(printing_colours(p))
        for k in ('transform_back_face', 'mdfc_back_face', 'adventure_face'):
            f = p.get(k)
            if isinstance(f, dict) and isinstance(f.get('name'), str):
                seen[f['name']].add(printing_colours(f))
        toks = []
        _tokens(p, toks)
        for n, cols in toks:
            seen[n].add(cols)
    return {n: (next(iter(s)) if len(s) == 1 else None) for n, s in seen.items()}


def _only_if_colour(e):
    oi = e.get('only_if') if isinstance(e, dict) else None
    if isinstance(oi, dict) and oi.get('$t') == 'HasColor' and isinstance(oi.get('color'), str):
        return oi['color']
    return None


def conditional_answers(cards=None):
    """The colour-conditional answers of the card IR: spells whose every mode does something only if its target has a
    colour (`only_if HasColor`: Pyroblast, Hydroblast, Red / Blue Elemental Blast) -> {name: [{'type': the mode's
    effect, 'colour': the colour it needs}, ...] in mode order}."""
    cards = _ir_cards() if cards is None else cards
    out = {}
    for c in cards:
        se = (c.get('printing') or {}).get('spell_effect')
        if not isinstance(se, dict):
            continue
        modes = se.get('modes') if se.get('$t') == 'ModalChooseOne' else [se]
        if not modes:
            continue
        need = [_only_if_colour(m) for m in modes]
        if all(need):
            out[c['name']] = [{'type': m.get('$t'), 'colour': n} for m, n in zip(modes, need)]
    return out


def target_colours(name, cmap):
    """A target's colours (frozenset) or None (unknown: not in the IR, or its sources disagree)."""
    if name is None:
        return None
    return cmap.get(name)


def classify(card, mode, target, cmap, answers):
    """One cast -> 'right' / 'wrong' (the target lacks the colour: the spell does nothing) / 'unknown' / 'no target'."""
    if target is None:
        return 'no target'
    modes = answers[card]
    if mode is not None and 0 <= mode < len(modes):
        need = modes[mode]['colour']
    else:
        cs = {m['colour'] for m in modes}
        if len(cs) != 1:
            return 'unknown'
        need = next(iter(cs))
    cols = target_colours(target, cmap)
    if cols is None:
        return 'unknown'
    return 'right' if need in cols else 'wrong'


# ------------------------------------------------------------------ the table plan
def _versions(run):
    from mtgz.az import matchup as M
    return M.versions(run)


def _load(path):
    with open(path, encoding='utf-8') as f:
        return json.load(f)


def version_decks(v, mirror_name=None):
    """A matchup version's decks -> [{'name', 'names', 'counts' (the 75's copies), 'main', 'table' (None: no table)}]
    (a mirror's one deck)."""
    pr = v.get('pairing')
    if pr and not pr.get('mirror'):
        tabs = v.get('tables') or []
        out = []
        for d in pr['decks']:
            names = [x[0] for x in d['list']]
            t = next((t for t in tabs if [x[0] for x in t['list']] == names
                      and [int(x[1]) for x in t['list']] == [int(x[1]) for x in d['list']]), None)
            out.append({'name': d['name'], 'names': names, 'counts': [int(x[1]) for x in d['list']],
                        'main': list(d['main']), 'table': t})
        return out
    if pr:
        d = pr['decks'][0]
        lst, main, name = d['list'], d['main'], d['name']
    else:
        lst, main, name = v['list'], v['main'], mirror_name or 'mirror'
    return [{'name': name, 'names': [x[0] for x in lst], 'counts': [int(x[1]) for x in lst], 'main': list(main),
             'table': v.get('table')}]


def table_state(table, main):
    if table is None:
        return 'no table'
    return 'all main' if all(list(ch[4]) == list(main) for ch in table['choices']) else 'boards'


def table_stats(table, names, main):
    """One deck's choice table against its main -> swaps per context, cards' mean copies in / out, the top 60s."""
    n = len(table['choices'])
    swaps, cin, cout, tops = [], collections.Counter(), collections.Counter(), collections.Counter()
    for ch in table['choices']:
        c = ch[4]
        s = 0
        delta = []
        for i, (k, m) in enumerate(zip(c, main)):
            if k > m:
                s += k - m
                cin[names[i]] += k - m
                delta.append((names[i], k - m))
            elif k < m:
                cout[names[i]] += m - k
                delta.append((names[i], k - m))
        swaps.append(s)
        tops[tuple(sorted(delta))] += 1
    return {'contexts': n, 'swaps': {'min': min(swaps), 'mean': round(sum(swaps) / n, 2), 'max': max(swaps)},
            'in': {x: round(v / n, 2) for x, v in cin.most_common()},
            'out': {x: round(v / n, 2) for x, v in cout.most_common()},
            'top': [{'changes': {a: b for a, b in ch}, 'contexts': k} for ch, k in tops.most_common(3)]}


def table_plan(run, mirror_name=None, upto_iter=None):
    """The run's newest matchup version at or before `upto_iter` (None: the newest) -> {'version', 'iter', 'versions',
    'decks': {deck: {'state', 'streak', ...table_stats}}} or {'note'} without a version."""
    vs = [(b, p) for b, p in _versions(run) if upto_iter is None or b <= upto_iter]
    if not vs:
        return {'note': 'no matchup version yet' + (f' at or before iteration {upto_iter}' if upto_iter is not None
                                                     else '')}
    loaded = []
    for b, p in reversed(vs):
        loaded.append((b, version_decks(_load(p), mirror_name)))
    newest_b, newest = loaded[0]
    out = {'version': f'v{newest_b:04d}', 'iter': newest_b, 'versions': len(vs), 'decks': {}}
    for k, d in enumerate(newest):
        st = table_state(d['table'], d['main'])
        streak = 0
        for _, ds in loaded:
            e = next((x for x in ds if x['name'] == d['name']), None)
            if e is None or table_state(e['table'], e['main']) != st:
                break
            streak += 1
        row = {'state': st, 'streak': streak}
        if d['table'] is not None:
            row.update(table_stats(d['table'], d['names'], d['main']))
        out['decks'][d['name']] = row
    return out


def model_health(run, upto_iter=None):
    """The matchup model's data and retrains (docs/ENGINE.md 273) at or before `upto_iter` -> {'version', 'games',
    'window', 'source', 'excluded_games', 'starved', 'min_games', 'failed': the newest retrain after the newest version
    that failed, or None, 'alerts': [lines]}. A version written before its data was recorded: its games against
    `matchup.MIN_GAMES_PER_ITER` times its window's iterations."""
    from mtgz.az import matchup as M
    vs = [(b, p) for b, p in _versions(run) if upto_iter is None or b <= upto_iter]
    per = M.MIN_GAMES_PER_ITER
    out = {'version': None, 'failed': None, 'alerts': []}
    newest = vs[-1][0] if vs else None
    if vs:
        v = _load(vs[-1][1])
        d = v.get('data') or {}
        w = v.get('window') or [newest, newest]
        games = (v.get('report') or {}).get('games')
        mn = d.get('min_games', per * (w[1] - w[0] + 1))
        out.update(version=f'v{newest:04d}', games=games, window=w, source=d.get('source', 'log'),
                   excluded_games=d.get('excluded_games', 0), min_games=mn,
                   starved=d['starved'] if 'starved' in d else (games is not None and games < mn))
        if out['starved']:
            out['alerts'].append(f"matchup model {out['version']}: fitted on {games} games (< {mn}: "
                                 f"{per} a game per iteration of its window {w[0]}-{w[1]})"
                                 + (f" - {d['note']}" if d.get('note') else ''))
        if out['excluded_games']:
            out['alerts'].append(f"matchup model {out['version']}: {out['excluded_games']} games of its window left out "
                                 f"(records another engine played) - {d.get('note')}")
    jobs = []
    for dd in glob.glob(os.path.join(M.mdir(run), 'work', 'v*')):
        try:
            b = int(os.path.basename(dd)[1:])
        except ValueError:
            continue
        if (upto_iter is None or b <= upto_iter) and (newest is None or b > newest):
            jobs.append((b, dd))
    for b, dd in sorted(jobs, reverse=True):
        st = os.path.join(dd, 'status.json')
        if os.path.exists(st):
            s = _load(st)
            if s.get('ok'):
                continue                         # (published after the newest version read: a race, not a failure)
            err = s.get('error')
        elif os.path.exists(os.path.join(dd, 'reported')):
            err = 'the job ended without a status (killed?)'
        else:
            continue                             # (running)
        out['failed'] = {'version': f'v{b:04d}', 'error': err}
        since = out['version'] or 'none (no version yet)'
        out['alerts'].append(f"matchup model: no new version since {since} (last retrain failed: v{b:04d}: "
                             f"{' '.join(str(err).split())[:300]})")
        break
    return out


def newest_tables(run, mirror_name=None, upto_iter=None):
    """{deck: {'table', 'names', 'main'}} of the newest version at or before `upto_iter` (the plan arm's), or {}."""
    vs = [(b, p) for b, p in _versions(run) if upto_iter is None or b <= upto_iter]
    if not vs:
        return {}
    return {d['name']: d for d in version_decks(_load(vs[-1][1]), mirror_name)}


def all_main_table(table, main):
    """The table's all-main twin: the same list and contexts, the main 60 in every one (boards nothing)."""
    return {'version': f"{table['version']}-allmain", 'list': [list(x) for x in table['list']],
            'choices': [list(ch[:4]) + [list(main)] for ch in table['choices']]}


def main_only_table(version, names, counts, main):
    """An all-main table of a 75 (names, its copies) with no table of its own: the main 60 in each of the 24 contexts."""
    from mtgz.az import matchup as M
    return {'version': f'{version}-allmain', 'list': [[n, int(k)] for n, k in zip(names, counts)],
            'choices': [[g, f, l, o, list(main)] for g, f, l, o in M.contexts()]}


# ------------------------------------------------------------------ the game database
def _gamedb():
    import importlib
    import sys
    t = os.path.join(ROOT, 'tools')
    if t not in sys.path:
        sys.path.insert(0, t)
    return importlib.import_module('gamedb')


def db_of(run, root=None):
    """The run's game database -> (directory, its ingested self-play iterations, None) or (None, [], why not)."""
    name = os.path.basename(os.path.normpath(run))
    try:
        d = os.path.join(root, name) if root else _gamedb().db_dir(run)
    except Exception as e:                       # noqa: BLE001 - said in the report
        return None, [], f'not ingested (no game database root: {e})'
    sp = os.path.join(d, 'state.json')
    if not os.path.exists(sp):
        return None, [], f'not ingested (no database {name})'
    try:
        st = _load(sp)
    except (OSError, ValueError) as e:
        return None, [], f'not ingested (database {name}: {e})'
    its = sorted(int(k) for k in (st.get('iterations') or {}))
    newest = _newest_model(run)
    if its and newest is not None and its[-1] > newest + 1:
        return None, [], (f'not ingested (database {name} holds iteration {its[-1]}, past the run\'s newest checkpoint '
                          f'{newest}: another run of that name)')
    return d, its, None


def _newest_model(run):
    its = [int(f[5:9]) for f in os.listdir(os.path.join(run, 'models'))
           if f.startswith('iter_') and f.endswith('.pt') and f[5:9].isdigit()] if os.path.isdir(
        os.path.join(run, 'models')) else []
    return max(its) if its else None


def _rows(con, sql, params):
    cur = con.execute(sql, params)
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, r)) for r in cur.fetchall()]


PLAY_SQL = """
WITH r AS (SELECT row_number() OVER () AS rid, deck, game_no, coalesce(deck_mode, 'main') AS mode, score, sb_in
           FROM game_results WHERE kind = 'selfplay' AND iter BETWEEN $lo AND $hi AND score IS NOT NULL),
i AS (SELECT rid, sum(CAST(split_part(x, ' ', 1) AS INTEGER)) AS n FROM (SELECT rid, unnest(sb_in) AS x FROM r) GROUP BY rid)
SELECT deck, game_no, mode, count(*) AS seats, avg(score) AS score, avg(coalesce(i.n, 0)) AS cards_in
FROM r LEFT JOIN i USING (rid) GROUP BY ALL ORDER BY deck, game_no, mode"""

CAST_SQL = """
WITH c AS (
  SELECT e.game_id, e.seq, e.game_no, e.player, e.card, CASE e.player WHEN 0 THEN g.deck0 ELSE g.deck1 END AS deck
  FROM events e JOIN games g USING (game_id)
  WHERE e.type = 'cast' AND e.card IN (SELECT unnest($cards)) AND g.kind = 'selfplay' AND g.iter BETWEEN $lo AND $hi),
ev AS (
  SELECT game_id, seq, type, card, detail,
         max(CASE WHEN type IN ('cast', 'activate', 'trigger') THEN seq END)
           OVER (PARTITION BY game_id ORDER BY seq ROWS UNBOUNDED PRECEDING) AS act
  FROM events WHERE game_id IN (SELECT game_id FROM c) AND type IN ('cast', 'activate', 'trigger', 'target', 'other')),
t AS (SELECT game_id, act, arg_min(card, seq) AS target FROM ev WHERE type = 'target' GROUP BY ALL),
m AS (SELECT game_id, act, arg_min(detail, seq) AS mode_line FROM ev
      WHERE type = 'other' AND detail LIKE '%"mode %' GROUP BY ALL)
SELECT c.deck, c.game_no, c.card, m.mode_line, t.target
FROM c LEFT JOIN t ON t.game_id = c.game_id AND t.act = c.seq LEFT JOIN m ON m.game_id = c.game_id AND m.act = c.seq"""


def _mode_of(line):
    if not line:
        return None
    try:
        txt = json.loads(line).get('text', '')
    except ValueError:
        return None
    p = txt.split()
    return int(p[1]) if len(p) == 2 and p[0] == 'mode' and p[1].isdigit() else None


def card_use_rows(rows, cmap, answers):
    """[{deck, game_no, card, mode_line, target}] -> {deck: {card: {game_no or 'all': counts}}}."""
    out = {}
    for r in rows:
        mode = _mode_of(r.get('mode_line'))
        v = classify(r['card'], mode, r.get('target'), cmap, answers)
        ms = answers[r['card']]
        counter = (ms[mode]['type'] == 'CounterSpell') if mode is not None and mode < len(ms) else None
        for gk in (str(r['game_no'] or 1), 'all'):
            c = out.setdefault(r['deck'], {}).setdefault(r['card'], {}).setdefault(gk, collections.Counter())
            c['casts'] += 1
            c[v] += 1
            if counter is None:
                c['mode unknown'] += 1
            elif counter:
                c['counter'] += 1
    res = {}
    for deck, cards in out.items():
        for card, gs in cards.items():
            for gk, c in gs.items():
                n = c['casts']
                known = c['right'] + c['wrong']
                res.setdefault(deck, {}).setdefault(card, {})[gk] = {
                    'casts': n, 'right': c['right'], 'wrong': c['wrong'], 'unknown': c['unknown'],
                    'no_target': c['no target'], 'wrong_rate': round(c['wrong'] / known, 4) if known else None,
                    'wrong_se': round(math.sqrt(c['wrong'] / known * (1 - c['wrong'] / known) / known), 4) if known
                    else None,
                    'counter': c['counter'], 'counter_share': round(c['counter'] / (n - c['mode unknown']), 4)
                    if n - c['mode unknown'] else None}
    return res


def from_db(db, lo, hi, cmap, answers):
    """The play and the card use of iterations lo..hi from the database at `db`."""
    con = _gamedb().connect(db)
    play = _rows(con, PLAY_SQL, {'lo': lo, 'hi': hi})
    rows = _rows(con, CAST_SQL, {'lo': lo, 'hi': hi, 'cards': sorted(answers)}) if answers else []
    return play, card_use_rows(rows, cmap, answers)


def play_from_log(run, lo, hi):
    """The play of iterations lo..hi from the run's best-of-three game log (local self-play only) -> rows as PLAY_SQL's,
    or None without a log."""
    from mtgz.az import matchup as M
    if not glob.glob(os.path.join(run, 'games', '*.jsonl')):
        return None
    games, specs = M.load_games(run, lo, hi, source='log')
    acc = collections.defaultdict(lambda: [0, 0.0, 0])
    for g in games:
        sp = specs[g['spec']]
        mg = sp.get('metagame')
        for p in (0, 1):
            name = mg['names'][p] if mg else f'p{p}'
            main = sp['mains'][p]
            brought = sum(max(0, k - m) for k, m in zip(g['decks'][p], main))
            a = acc[(name, int(g['game']), g['mode'][p] or 'main')]
            a[0] += 1
            a[1] += float(g['result'][p])
            a[2] += brought
    return [{'deck': d, 'game_no': gn, 'mode': m, 'seats': a[0], 'score': a[1] / a[0], 'cards_in': a[2] / a[0]}
            for (d, gn, m), a in sorted(acc.items())]


# ------------------------------------------------------------------ one specialist, the section, the alerts
def reference_level(card, rate):
    """'pb3-like' (at or below pb3's do-nothing rate), 'pb2-like' (at or above pb2's), 'between pb3 and pb2', or None
    (no reference for the card, or no rate)."""
    r2, r3 = REFERENCE['pb2']['misaim'].get(card), REFERENCE['pb3']['misaim'].get(card)
    if rate is None or r2 is None or r3 is None:
        return None
    if rate <= r3:
        return 'pb3-like'
    if rate >= r2:
        return 'pb2-like'
    return 'between pb3 and pb2'


def misaim_line(card, opts):
    if opts['misaim_alert'] == 'reference':
        return REFERENCE['pb2']['misaim'].get(card, opts['misaim_fallback'])
    return float(opts['misaim_alert'])


def specialist(run, decks, opts, upto_iter, iters, cmap=None, answers=None):
    """One specialist's health (module docs). `decks`: its pairing's deck names ([A] for a mirror)."""
    mirror = len(set(decks)) == 1
    out = {'run': os.path.basename(os.path.normpath(run)), 'table': table_plan(run, decks[0] if mirror else None,
                                                                               upto_iter)}
    try:
        out['model'] = model_health(run, upto_iter)
    except Exception as e:                       # noqa: BLE001 - said in the report (an alert), never fails the stew
        out['model'] = {'alerts': [f'matchup model health failed: {type(e).__name__}: {e}']}
    hi = upto_iter if upto_iter is not None else _newest_model(run)
    if hi is None:
        out['play'] = {'note': 'no checkpoint'}
        out['card_use'] = {'note': 'no checkpoint'}
        return out
    lo = max(1, hi - iters + 1)
    out['window'] = [lo, hi]
    db, its, why = db_of(run, opts.get('gamedb'))
    have = [i for i in its if lo <= i <= hi]
    if db is not None and not have:
        why = (f'not ingested for iterations {lo}-{hi} (the database holds '
               + (f'{its[0]}-{its[-1]})' if its else 'no self-play iteration)'))
        db = None
    if db is not None:
        try:
            if cmap is None:
                cmap = colour_map()
            if answers is None:
                answers = conditional_answers()
            play, use = from_db(db, lo, hi, cmap, answers)
            cov = {'source': 'game database', 'iterations': len(have), 'of': hi - lo + 1}
            out['play'] = dict(cov, rows=play)
            out['card_use'] = dict(cov, decks=use)
            return out
        except Exception as e:                   # noqa: BLE001 - said in the report, never fails the stew
            why = f"the game database failed ({type(e).__name__}: {' '.join(str(e).split())[:300]})"
    out['card_use'] = {'note': why}
    try:
        rows = play_from_log(run, lo, hi)
    except Exception as e:                       # noqa: BLE001
        rows, why = None, f'{why}; the game log failed ({type(e).__name__}: {e})'
    out['play'] = ({'source': 'game log (local self-play only)', 'rows': rows, 'db': why} if rows is not None
                   else {'note': f'{why}; no game log'})
    return out


def alerts(h, opts):
    """The specialists' health -> [alert lines]; also marks each card-use row's level."""
    out = []
    for sn, s in (h.get('specialists') or {}).items():
        for a in (s.get('model') or {}).get('alerts') or []:
            out.append(f'{sn}: {a}')
        t = s.get('table') or {}
        for deck, d in (t.get('decks') or {}).items():
            if d['state'] == 'all main' and d['streak'] >= opts['all_main_versions']:
                out.append(f"{sn}: {deck}'s choice table all main for {d['streak']} consecutive versions "
                           f"(newest {t['version']}): the model sees no sideboarding gain")
        cu = s.get('card_use') or {}
        if 'note' in cu:
            out.append(f"{sn}: card use {cu['note']}")
        for deck, cards in (cu.get('decks') or {}).items():
            for card, gs in cards.items():
                a = gs.get('all') or {}
                rate = a.get('wrong_rate')
                for x in gs.values():
                    x['level'] = reference_level(card, x.get('wrong_rate'))
                line = misaim_line(card, opts)
                if rate is not None and a['right'] + a['wrong'] >= opts['min_casts'] and rate > line:
                    out.append(f"{sn}: {deck}'s {card} aimed at the wrong colour in {100 * rate:.1f}% of "
                               f"{a['right'] + a['wrong']} casts (alert above {100 * line:.0f}%"
                               + (f", {a['level']}" if a.get('level') else '') + ')')
    return out


def section(stew, g):
    """The report's `sb_health` block of generation g (a Stew), or None when recipe.sb_health is off."""
    opts = options(stew.cfg['recipe'])
    if opts is None:
        return None
    from . import config as C
    st = stew.st
    iters = opts['iters'] or int(stew.cfg['recipe']['turn_iters'])
    h = {'options': opts, 'reference': REFERENCE, 'specialists': {}}
    cmap = answers = None
    for a, b in C.pairings(st.decks(), st.candidates()):
        sn = C.spec_name(a, b)
        try:
            n = st.net(sn)
            v = st.version(sn, g + 1) or st.latest(sn)
            run = st.abs(n['run'])
            if cmap is None:
                cmap, answers = colour_map(), conditional_answers()
            h['specialists'][sn] = specialist(run, [a] if a == b else [a, b], opts, v['iter'] if v else None, iters,
                                              cmap, answers)
            if v:
                h['specialists'][sn]['version'] = f"v{v['gen']} (iteration {v['iter']})"
        except Exception as e:                   # noqa: BLE001 - the report says it, the stew goes on
            h['specialists'][sn] = {'error': f'{type(e).__name__}: {e}'}
    h['plan_vs_no_plan'] = plan_stats_of(stew, g)
    h['alerts'] = alerts(h, opts)
    return h


def status_lines(stew):
    """`tools/stew.py status`: one line per specialist - its newest table's state per deck (no database read)."""
    opts = options(stew.cfg['recipe'])
    if opts is None:
        return []
    from . import config as C
    st = stew.st
    L = []
    for a, b in C.pairings(st.decks(), st.candidates()):
        sn = C.spec_name(a, b)
        try:
            t = table_plan(st.abs(st.net(sn)['run']), a if a == b else None)
        except Exception as e:                   # noqa: BLE001
            L.append(f'  sb health {sn}: {type(e).__name__}: {e}')
            continue
        try:
            for a in model_health(st.abs(st.net(sn)['run']))['alerts']:
                L.append(f'  sb health {sn}: ALERT {a}')
        except Exception as e:                   # noqa: BLE001
            L.append(f'  sb health {sn}: matchup model health: {type(e).__name__}: {e}')
        if 'note' in t:
            L.append(f"  sb health {sn}: {t['note']}")
            continue
        parts = []
        for deck, d in t['decks'].items():
            alert = d['state'] == 'all main' and d['streak'] >= opts['all_main_versions']
            parts.append(f"{deck} {d['state']} x{d['streak']}" + (f", swaps {d['swaps']['mean']}" if 'swaps' in d
                                                                   else '') + (' ALERT' if alert else ''))
        L.append(f"  sb health {sn}: table {t['version']}: " + '; '.join(parts))
    return L


# ------------------------------------------------------------------ plan vs no plan (the statistics)
def plan_stats(res, arms):
    """The plan arm's results ({job id: result}) and its arms (`judge.plan_arm_jobs`'s `arms`) -> {arm: stats}."""
    from . import judge as J
    out = {}
    for arm in arms:
        if arm['kind'] == 'mirror':
            r0, r1 = res.get(arm['jobs'][0]), res.get(arm['jobs'][1])
            if r0 is None or r1 is None:
                continue
            rows = [x for pair in zip(r0['rows'], r1['rows']) for x in pair]
            s = J.gen_stats({'rows': rows})
            if s:
                out[arm['name']] = dict(s, kind='mirror', what='the plan side\'s score (both seats one net)')
            continue
        d, pp, pn = [], [], []
        ok = True
        for jp, jn, xs in arm['pairs']:
            a, b = res.get(jp), res.get(jn)
            if a is None or b is None:
                ok = False
                break
            for x, y in zip(a['rows'], b['rows']):
                pp.append(x['result'][xs])
                pn.append(y['result'][xs])
                d.append(x['result'][xs] - y['result'][xs])
        if not ok or not d:
            continue
        n = len(d)
        md = sum(d) / n
        var = sum((x - md) ** 2 for x in d) / (n - 1) if n > 1 else 0.0
        out[arm['name']] = {'kind': 'pair', 'what': f"{arm['deck']}'s match score with its table minus all main",
                            'plan': round(sum(pp) / n, 4), 'no_plan': round(sum(pn) / n, 4), 'diff': round(md, 4),
                            'se': round(math.sqrt(var / n), 4), 'matches': n}
    return out


def plan_stats_of(stew, g):
    jc = stew.cfg['recipe']['judge']
    if not jc.get('plan_arm'):
        return None
    try:
        from . import judge as J
        _, arms = J.plan_arm_jobs(stew.st, g, jc, stew.pair_spec_json, {}, stew.plan_tables(g))
        return plan_stats(stew.judge_results(g), arms)
    except Exception as e:                       # noqa: BLE001
        return {'error': f'{type(e).__name__}: {e}'}


# ------------------------------------------------------------------ the report's Markdown
def _pct(x, se=None):
    if x is None:
        return ''
    return f'{100 * x:.1f}%' + (f' +- {100 * se:.1f}' if se is not None else '')


def _c(x):
    """A table cell: one line, Markdown's column bar escaped."""
    return ' '.join(str(x).split()).replace('|', '\\|')


def _ref(card):
    r2, r3 = REFERENCE['pb2'], REFERENCE['pb3']
    if card not in r2['misaim']:
        return ''
    return (f"pb2 {100 * r2['misaim'][card]:.0f}% / pb3 {100 * r3['misaim'][card]:.0f}% (counter "
            f"{100 * r2['counter'][card]:.0f}% / {100 * r3['counter'][card]:.0f}%)")


def md_lines(h):
    """The report's "Sideboarding health" section (reports/gen_NNN.md)."""
    if not h:
        return []
    L = ['## Sideboarding health (docs/ENGINE.md 260)', '']
    if h.get('error'):
        L += [f"ERROR {h['error']}", '']
    for a in h.get('alerts') or []:
        L.append(f'* **ALERT** {a}')
    if h.get('alerts'):
        L.append('')
    sp = h.get('specialists') or {}
    L += ['### Choice tables (the matchup model\'s plan)', '',
          '| specialist | deck | version | state | streak | swaps min / mean / max | in (mean copies) | out |',
          '|---|---|---|---|---|---|---|---|']
    for sn, s in sp.items():
        if 'error' in s:
            L.append(f"| {_c(sn)} | | | ERROR {_c(s['error'])} | | | | |")
            continue
        t = s.get('table') or {}
        if 'note' in t:
            L.append(f"| {_c(sn)} | | | {_c(t['note'])} | | | | |")
            continue
        for deck, d in t['decks'].items():
            sw = d.get('swaps')
            L.append(f"| {_c(sn)} | {deck} | {t['version']} | {d['state']} | {d['streak']} of {t['versions']} | "
                     + (f"{sw['min']} / {sw['mean']} / {sw['max']} | " if sw else ' | ')
                     + ', '.join(f'{k} {v}' for k, v in list((d.get('in') or {}).items())[:5]) + ' | '
                     + ', '.join(f'{k} {v}' for k, v in list((d.get('out') or {}).items())[:5]) + ' |')
    L += ['', '### Play by game and deck mode', '',
          '| specialist | source | deck | game | mode | seats | score | cards in |', '|---|---|---|---|---|---|---|---|']
    for sn, s in sp.items():
        p = s.get('play') or {}
        if 'note' in p or 'error' in s:
            L.append(f"| {_c(sn)} | {_c(p.get('note', s.get('error')))} | | | | | | |")
            continue
        src = p['source'] + (f", {p['iterations']} of {p['of']} iterations" if 'iterations' in p else '') + \
            (f" {s['window'][0]}-{s['window'][1]}" if s.get('window') else '')
        for r in p['rows']:
            L.append(f"| {_c(sn)} | {src} | {r['deck']} | {r['game_no']} | {r['mode']} | {r['seats']} | "
                     f"{100 * r['score']:.1f}% | {r['cards_in']:.2f} |")
    L += ['', '### Card use: colour-conditional answers', '',
          '| specialist | deck | card | game | casts | wrong colour | unknown | counter mode | level | reference |',
          '|---|---|---|---|---|---|---|---|---|---|']
    for sn, s in sp.items():
        cu = s.get('card_use') or {}
        if 'note' in cu or 'error' in s:
            L.append(f"| {_c(sn)} | {_c(cu.get('note', s.get('error')))} | | | | | | | | |")
            continue
        if not cu.get('decks'):
            L.append(f"| {_c(sn)} | no cast in the window | | | | | | | | |")
        for deck, cards in cu['decks'].items():
            for card, gs in cards.items():
                for gk in sorted(gs, key=lambda k: (k == 'all', k)):
                    x = gs[gk]
                    L.append(f"| {_c(sn)} | {deck} | {card} | {gk} | {x['casts']} | "
                             f"{_pct(x['wrong_rate'], x['wrong_se'])} | {x['unknown']} | {_pct(x['counter_share'])} | "
                             f"{x.get('level') or ''} | {_ref(card) if gk == 'all' else ''} |")
    pv = h.get('plan_vs_no_plan')
    if pv:
        L += ['', '### Plan vs no plan (recipe.judge.plan_arm)', '']
        if 'error' in pv:
            L.append(f"ERROR {pv['error']}")
        else:
            L += ['| arm | what | result | matches |', '|---|---|---|---|']
            for k, x in pv.items():
                if x['kind'] == 'mirror':
                    L.append(f"| {_c(k)} | {x['what']} | {_pct(x['score'], x['se'])} ({x['elo']:+.1f} +- "
                             f"{x['elo_se']:.1f} Elo) | {x['matches']} |")
                else:
                    L.append(f"| {_c(k)} | {x['what']} | {100 * x['diff']:+.1f} +- {100 * x['se']:.1f} points "
                             f"({_pct(x['plan'])} vs {_pct(x['no_plan'])}) | {x['matches']} |")
    L += ['', f"Reference: {REFERENCE['source']}; {REFERENCE['pb2']['at']} and {REFERENCE['pb3']['at']}. Wrong colour "
              f"= the target lacks the colour the mode needs (a land is colourless): the spell does nothing.", '']
    return L
