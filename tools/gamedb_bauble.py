"""The Bauble targeting measure of the game database (docs/gamedb.md "Bauble targeting", docs/ENGINE.md 194): every
activation of Mishra's Bauble or Urza's Bauble in a game replayed in full, with the activator's position and knowledge
at that moment read from the replayed engine state, and whether its target was a blunder by the user's rules.

The rules (the user's, 2026-11-17):
* Urza's Bauble ("{T}, Sacrifice: Look at a card at random in target player's hand. ..."): targeting YOURSELF is a
  blunder, unless the opponent has no card in hand or the activator already knows every card in the opponent's hand.
* Mishra's Bauble ("{T}, Sacrifice: Look at the top card of target player's library. Draw a card at the beginning of
  the next turn's upkeep."): targeting YOURSELF is a blunder when the activator has no fetchland AVAILABLE, unless it
  already knows the opponent's top card (a remembered look, ENGINE.md 192). Looking at its own top card is worth
  something only if it can shuffle that card away before the Bauble's delayed draw (the user, 2026-11-18, correcting
  the first rule, which counted any fetchland in hand). A fetchland is available when it is
  - on the activator's battlefield and crackable before that draw (`fetch_crackable`: untapped, or tapped on the
    opponent's turn - it untaps in the activator's untap step, before the upkeep trigger resolves), or
  - in the activator's hand and playable before that draw (`fetch_in_hand_playable`): it is the activator's own turn, a
    main phase is still to come or under way (upkeep through main 2), and its land drop is unused
    (`land_drop_available`: lands played this turn below its land drops - one, plus the extra drops its permanents
    grant). A fetchland in hand on the opponent's turn, in the activator's end step, or after its land drop is spent is
    NOT available: the delayed draw comes first.
  A self-target with a fetchland available is not a blunder; it is counted on its own (`blunder_reason`
  'ok_self_with_fetch'), with the library manipulation the activator held (Brainstorm, Ponder, ...) as a column. A
  self-target with a fetchland that is not available is a blunder with its own reason ('self_with_unusable_fetch');
  with no fetchland at all, 'self_without_fetch'. The raw counts (`fetch_in_hand`, `fetch_on_battlefield`) stay
  columns.
* An activation where targeting the opponent is impossible or meaningless is excluded (`excluded`, `blunder` NULL): the
  opponent was not a legal target (no target decision offered it), or for Mishra's Bauble the opponent's library was
  empty. So is one whose game stopped before its target was chosen (a resignation at the target decision: `target`
  NULL, 'game_ended_before_target').

Every condition is the engine's: the spectator state (`export_state`) before the action that activated the Bauble - the
hands, the battlefield, the library sizes - and its `known` block (`view_known`, the engine's exact hidden-information
state `Obj.known`): the opponent's hand cards the activator knows, and whether the activator knows the card on top of
the opponent's library (remembered since ENGINE.md 192; an engine before it never remembers that look, so the column is
false there unless the opponent's owner knew the card too). The target is the engine's own play-by-play line ("target
P<seat>") after the activation. Partial knowledge (the engine's clouds) is not counted as knowing a card. The land drop
is the engine's rule (cast.rs `priority_options_all`: the active player plays a land in a main phase with an empty
stack while its lands played this turn < 1 + the extra drops its permanents grant); where the state's own decision
shows it (the activator's priority in its main phase, the stack empty, a fetchland in hand) the engine's offer to play
that fetchland must agree with `land_drop_available`, or the read stops (loud).
"""
import json
import re

URZA = "Urza's Bauble"
MISHRA = "Mishra's Bauble"
BAUBLES = {URZA: 'urza', MISHRA: 'mishra'}

# Lands that crack to shuffle their controller's library (a sacrifice-and-search land ability): the fetchlands of the
# supported pools and the other cards of that shape.
FETCHLANDS = frozenset({
    'Arid Mesa', 'Bloodstained Mire', 'Flooded Strand', 'Marsh Flats', 'Misty Rainforest', 'Polluted Delta',
    'Scalding Tarn', 'Verdant Catacombs', 'Windswept Heath', 'Wooded Foothills', 'Prismatic Vista', 'Fabled Passage',
    'Evolving Wilds', 'Terramorphic Expanse',
})
# Other cards in hand that see or rearrange the top of the activator's own library (the extra column; never in the
# blunder rule).
LIBRARY_MANIPULATION = frozenset({
    'Brainstorm', 'Ponder', 'Preordain', 'Consider', 'Opt', 'Portent', "Sensei's Divining Top",
    'Jace, the Mind Sculptor', 'Prismari Charm', 'Thundering Falls', 'Flow State',
})

# The steps of the activator's own turn from which it can still play a land this turn (a main phase under way or to
# come): a fetchland played then can be cracked before the next turn's upkeep. (The export's step names, az/view.rs.)
LAND_DROP_STEPS = frozenset({'untap', 'upkeep', 'draw', 'main1', 'begin_combat', 'declare_attackers', 'declare_blockers',
                             'first_strike_damage', 'combat_damage', 'end_combat', 'main2'})
ALL_STEPS = LAND_DROP_STEPS | {'setup', 'end', 'cleanup'}



def _lay():
    """mtgz.az.layouts (docs/ENGINE.md 236): the game - layout v8 `mtg` or v7 `mtg_v7` - of the checkpoints this tool loads
    (`load_used`), every game it plays and encodes (`current`: layout v8 until a checkpoint is loaded)."""
    from mtgz.az import layouts
    return layouts

def _extra_land_drops():
    """{card name: extra land drops a turn} of every card the engine grants them to: its card data's
    `grants_additional_land_per_turn` (data/cards/*.ir.json, lowered into `CardDef::extra_lands`; Fastbond's "any
    number" is the IR's sentinel 1,000,000: more drops than a game has lands, as the engine's ANY_LANDS). The hand-written legacy cards grant none (no other
    `extra_lands` in engine/src)."""
    import glob
    import os
    out = {}
    root = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'data', 'cards')
    for f in sorted(glob.glob(os.path.join(root, '*.ir.json'))):
        with open(f, encoding='utf-8') as fh:
            for c in json.load(fh).get('cards') or []:
                n = int((c.get('printing') or {}).get('grants_additional_land_per_turn') or 0)
                if n:
                    out[c['name']] = n
    return out


EXTRA_LAND_DROPS = _extra_land_drops()

LINE = re.compile(r'^T(\d+) (\w+) (.*)$')
ACTIVATE = re.compile(r"^P(\d) activates (Mishra's Bauble|Urza's Bauble) \(")
TARGET = re.compile(r'^target P(\d)$')


def schema():
    import pyarrow as pa
    s, i, b, ls = pa.string(), pa.int32(), pa.bool_(), pa.list_(pa.string())
    return pa.schema([
        ('game_id', pa.int64()), ('iter', i), ('idx', i), ('action', i), ('turn', i), ('step', s), ('activator', i),
        ('own_turn', b), ('bauble', s), ('target', s), ('opp_targetable', b), ('opp_hand', i), ('opp_hand_known', i),
        ('opp_hand_all_known', b), ('opp_library', i), ('opp_top_known', b), ('own_top_known', b),
        ('fetch_in_hand', i), ('fetch_on_battlefield', i), ('fetch_available', b), ('manipulation_in_hand', ls),
        ('lands_played', i), ('land_drop_available', b), ('fetch_in_hand_playable', b), ('fetch_crackable', i),
        ('brainstorm_in_hand', b), ('ponder_in_hand', b), ('excluded', s), ('blunder', b), ('blunder_reason', s)])


def _knows_top(known, owner, viewer):
    """`viewer` knows the card on top of `owner`'s library (the state's known block: [position, mask, name])."""
    return any(e[0] == 0 and e[1] & (1 << viewer) for e in (known[owner].get('library') or []))


def conditions(st, p):
    """The activator `p`'s position at state `st` (the spectator export before the activating action) -> dict."""
    opp = 1 - p
    me, op = st['players'][p], st['players'][opp]
    known = st.get('known')
    if known is None:
        raise ValueError('the exported state has no `known` block (an engine before ENGINE.md 72?)')
    hand = [c['name'] for c in me.get('hand') or []]
    opp_hand = len(op.get('hand') or [])
    opp_known = len(known[opp].get('hand') or [])
    manip = sorted({n for n in hand if n in LIBRARY_MANIPULATION})
    own = st.get('active') == p
    step = st.get('step')
    if step not in ALL_STEPS:
        raise ValueError(f'the exported state has no known step ({step!r})')
    fib = sum(1 for n in hand if n in FETCHLANDS)
    mine = [c for c in me.get('battlefield') or [] if c.get('controller', p) == p]
    fob = sum(1 for c in mine if c.get('name') in FETCHLANDS)
    # crackable before the delayed draw: untapped; or tapped on the opponent's turn (it untaps in the activator's untap
    # step, before the upkeep's draw trigger resolves); tapped on its own turn it stays tapped through the next upkeep
    crack = sum(1 for c in mine if c.get('name') in FETCHLANDS and (not c.get('tapped') or not own))
    played = int(me.get('lands_played') or 0)
    drops = 1 + sum(EXTRA_LAND_DROPS.get(c.get('name'), 0) for c in mine)
    drop = own and step in LAND_DROP_STEPS and played < drops
    d = st.get('decision') or {}
    if (fib and d.get('kind') == 'Priority' and d.get('player') == p and own and step in ('main1', 'main2')
            and not st.get('stack')):
        offered = any(o.get('t') == f'Play {n}' for o in d.get('options') or [] for n in hand if n in FETCHLANDS)
        if offered != drop:
            raise ValueError(f'turn {st.get("turn")} {step}: the engine {"offers" if offered else "does not offer"} '
                             f'P{p} a land play of its fetchland, land_drop_available = {drop} (lands played {played}, '
                             f'drops {drops})')
    return {'own_turn': own, 'opp_hand': opp_hand, 'opp_hand_known': opp_known,
            'opp_hand_all_known': opp_known >= opp_hand, 'opp_library': int(op.get('library') or 0),
            'opp_top_known': _knows_top(known, opp, p), 'own_top_known': _knows_top(known, p, p),
            'fetch_in_hand': fib, 'fetch_on_battlefield': fob, 'lands_played': played, 'land_drop_available': drop,
            'fetch_in_hand_playable': bool(fib) and drop, 'fetch_crackable': crack,
            'fetch_available': crack > 0 or (bool(fib) and drop),
            'manipulation_in_hand': manip, 'brainstorm_in_hand': 'Brainstorm' in hand,
            'ponder_in_hand': 'Ponder' in hand}


def classify(r):
    """(excluded, blunder, blunder_reason) of an activation row by the user's rules (module docs)."""
    if not r['opp_targetable']:
        return 'opponent_not_targetable', None, None
    if r['bauble'] == 'mishra' and r['opp_library'] == 0:
        return 'opponent_library_empty', None, None
    if r['target'] == 'opponent':
        return None, False, 'ok_target_opponent'
    if r['bauble'] == 'urza':
        if r['opp_hand'] == 0:
            return None, False, 'ok_opponent_hand_empty'
        if r['opp_hand_all_known']:
            return None, False, 'ok_opponent_hand_known'
        return None, True, 'self_with_unknown_opponent_hand'
    if r['opp_top_known']:
        return None, False, 'ok_opponent_top_known'
    if r['fetch_available']:
        return None, False, 'ok_self_with_fetch'
    if r['fetch_in_hand'] or r['fetch_on_battlefield']:
        return None, True, 'self_with_unusable_fetch'
    return None, True, 'self_without_fetch'


class Reader:
    """Fed every step of a full replay (`step`): the Bauble activations of the game, as `rows` (`finish` at the end).
    An activation is its "P<n> activates <Bauble> (...)" line; its conditions are the state before that step; its target
    the first "target P<n>" line after it; the opponent was targetable when the target decision offered two players
    (or the opponent was the target)."""

    def __init__(self):
        self.rows = []
        self.pending = None

    def step(self, i, st, decision, lines):
        """Step `i`: `st` the state before the action, `decision` its decision (dict), `lines` the lines it wrote."""
        p = self.pending
        if p is not None and decision.get('kind') == 'Target' and decision.get('player') == p['activator']:
            if self.pending['n_target_opts'] is None:
                self.pending['n_target_opts'] = len(decision.get('options') or [])
        for ln in lines:
            m = LINE.match(ln)
            if not m:
                continue
            turn, stp, rest = int(m.group(1)), m.group(2), m.group(3)
            a = ACTIVATE.match(rest)
            if a:
                if self.pending is not None:
                    raise ValueError(f'step {i}: a Bauble activation before the last one chose its target')
                p = int(a.group(1))
                self.pending = {'action': i, 'turn': turn, 'step': stp, 'activator': p, 'bauble': BAUBLES[a.group(2)],
                                'n_target_opts': None, **conditions(st, p)}
                continue
            t = TARGET.match(rest)
            if t and self.pending is not None:
                r = self.pending
                self.pending = None
                seat = int(t.group(1))
                r['target'] = 'self' if seat == r['activator'] else 'opponent'
                r['opp_targetable'] = r['target'] == 'opponent' or (r['n_target_opts'] or 0) >= 2
                del r['n_target_opts']
                r['excluded'], r['blunder'], r['blunder_reason'] = classify(r)
                r['idx'] = len(self.rows)
                self.rows.append(r)

    def finish(self, over=True):
        """The game's end: an activation that never chose a target is a broken read (loud) in a game the rules ended;
        in a game that stopped before its target (a resignation at the target decision, an unfinished game) it is a
        row with no target, excluded ('game_ended_before_target'), never dropped."""
        if self.pending is not None:
            if over:
                raise ValueError(f'a Bauble activation at step {self.pending["action"]} never chose a target')
            r = self.pending
            self.pending = None
            r['opp_targetable'] = (r.pop('n_target_opts') or 0) >= 2
            r.update(target=None, excluded='game_ended_before_target', blunder=None, blunder_reason=None,
                     idx=len(self.rows))
            self.rows.append(r)
        return self.rows


# The view (docs/gamedb.md): the rows with the game's kind, the activator's checkpoint, its deck and the opponent's.
# `kind` and the players' checkpoints are columns of `games` since the database records every kind of game (ENGINE.md
# 191 of feat/gamedb-all); before that every game is self-play and both seats are the network `net_hi`.
VIEW = """
CREATE OR REPLACE VIEW bauble_activations AS
SELECT b.game_id, b.iter, {kind} AS kind, {ckpt} AS checkpoint,
       CASE b.activator WHEN 0 THEN g.deck0 ELSE g.deck1 END AS deck,
       CASE b.activator WHEN 0 THEN g.deck1 ELSE g.deck0 END AS opp_deck,
       CASE b.activator WHEN 0 THEN g.deck0 || ' v ' || g.deck1 ELSE g.deck1 || ' v ' || g.deck0 END AS pairing,
       b.idx, b.action, b.turn, b.step, b.activator, b.own_turn, b.bauble, b.target, b.opp_targetable, b.opp_hand,
       b.opp_hand_known, b.opp_hand_all_known, b.opp_library, b.opp_top_known, b.own_top_known, b.fetch_in_hand,
       b.fetch_on_battlefield, b.fetch_available, b.lands_played, b.land_drop_available, b.fetch_in_hand_playable,
       b.fetch_crackable, b.manipulation_in_hand, b.brainstorm_in_hand, b.ponder_in_hand,
       b.excluded, b.blunder, b.blunder_reason, (g.winner = b.activator) AS won
FROM bauble b JOIN games g USING (game_id);
"""


def view_sql(game_columns):
    """The view's SQL for a `games` table with these columns (module VIEW)."""
    cols = set(game_columns)
    kind = 'g.kind' if 'kind' in cols else "'selfplay'"
    if {'p0_ckpt', 'p1_ckpt'} <= cols:
        ckpt = ("coalesce(CASE b.activator WHEN 0 THEN g.p0_ckpt ELSE g.p1_ckpt END, "
                "'iter_' || lpad(CAST(g.net_hi AS VARCHAR), 4, '0'))")
    else:
        ckpt = "'iter_' || lpad(CAST(g.net_hi AS VARCHAR), 4, '0')"
    return VIEW.replace('{kind}', kind).replace('{ckpt}', ckpt)


# The per-iteration series (a publisher's): activations, self-target share, blunders and their share, per Bauble, from
# per-game counts (the standard errors are clustered by game: one game's activations are not independent). The
# denominator of the shares: the activations not excluded.
SERIES = """
SELECT iter, {by} AS kind, bauble, game_id, count(*) AS activations,
       count(*) FILTER (WHERE excluded IS NOT NULL) AS excluded,
       count(*) FILTER (WHERE excluded IS NULL) AS counted,
       count(*) FILTER (WHERE excluded IS NULL AND target = 'self') AS self_targets,
       count(*) FILTER (WHERE blunder) AS blunders,
       count(*) FILTER (WHERE blunder_reason = 'ok_self_with_fetch') AS self_with_fetch,
       count(*) FILTER (WHERE blunder_reason = 'ok_self_with_fetch' AND len(manipulation_in_hand) > 0)
         AS self_with_fetch_and_manipulation,
       count(*) FILTER (WHERE excluded IS NULL AND target = 'self' AND len(manipulation_in_hand) > 0)
         AS self_with_manipulation
FROM bauble_activations WHERE iter >= $since {where} GROUP BY ALL"""
NUMS = ('activations', 'excluded', 'counted', 'self_targets', 'blunders', 'self_with_fetch',
        'self_with_fetch_and_manipulation', 'self_with_manipulation')


def cluster_share(pairs):
    """[(k, n) per game] -> (share, its standard error clustered by game) of sum k / sum n; (None, None) for n = 0 and
    no SE from a single game."""
    n = sum(x[1] for x in pairs)
    if not n:
        return None, None
    p = sum(x[0] for x in pairs) / n
    g = sum(1 for x in pairs if x[1])
    if g < 2:
        return p, None
    v = g / (g - 1) * sum((k - p * m) ** 2 for k, m in pairs)
    return p, v ** 0.5 / n


def series(con, since_iter=0, by_kind=False, kind=None, extra_where=''):
    """Rows of the Bauble series (docs/gamedb.md "Bauble targeting") from a gamedb connection: one per iteration and
    Bauble ('urza', 'mishra', and 'all': the two together), with the counts, the shares of the counted activations
    and their standard errors (clustered by game). `extra_where`: a further condition (' AND ...'; gamedb's
    `branch_filter`)."""
    where = ('AND kind = $kind' if kind else '') + extra_where
    sql = SERIES.replace('{by}', 'kind' if by_kind else 'NULL::VARCHAR').replace('{where}', where)
    cur = con.execute(sql, {'since': int(since_iter), **({'kind': kind} if kind else {})})
    cols = [d[0] for d in cur.description]
    groups = {}
    for r in cur.fetchall():
        r = dict(zip(cols, r))
        for b in (r['bauble'], 'all'):
            groups.setdefault((r['iter'], r['kind'], b), {}).setdefault(r['game_id'], [0] * len(NUMS))
            acc = groups[(r['iter'], r['kind'], b)][r['game_id']]
            for i, k in enumerate(NUMS):
                acc[i] += r[k]
    out = []
    ix = {k: i for i, k in enumerate(NUMS)}
    for (it, kd, b), games in groups.items():
        per = list(games.values())
        row = {'iter': it, 'kind': kd, 'bauble': b, 'games': len(per), **{k: sum(x[ix[k]] for x in per) for k in NUMS}}
        for name, k in (('self', 'self_targets'), ('blunder', 'blunders'), ('self_with_fetch', 'self_with_fetch')):
            row[f'{name}_share'], row[f'{name}_se'] = cluster_share([(x[ix[k]], x[ix['counted']]) for x in per])
        out.append(row)
    order = {'urza': 0, 'mishra': 1, 'all': 2}
    out.sort(key=lambda r: (r['iter'], r['kind'] or '', order[r['bauble']]))
    return out


# ====================================================================================== canary spots
# The canary library's Bauble kinds (tools/delver_blunders.py library-add, docs/blunders.md): the target decision of an
# activation, watched option = the activator itself. 'bad' kinds score the searches that avoid it; the control kind
# (a Mishra's Bauble at oneself with a fetchland ready: not a blunder) is reported as the share choosing it and stays
# out of the right-answer means (answer 'control').
SPOT_KINDS = {
    'bauble_urza_self': ('bad', "Urza's Bauble at itself while the opponent holds cards it doesn't know: looking at a "
                                "random card of its own hand tells it nothing; the opponent's hand was the only "
                                "information on offer (the user's rule, docs/ENGINE.md 194)"),
    'bauble_mishra_self_no_fetch': ('bad', "Mishra's Bauble at itself with no fetchland in hand or on the battlefield "
                                           "and the opponent's top card unknown: it can't act on its own top card "
                                           "(the Bauble draws it anyway), while the opponent's top card is remembered "
                                           "(ENGINE.md 192) (the user's rule, docs/ENGINE.md 194)"),
    'bauble_mishra_self_fetch_unusable': ('bad', "Mishra's Bauble at itself with a fetchland it can't use in time and "
                                                 "the opponent's top card unknown: the Bauble draws its top card at the "
                                                 "beginning of the next upkeep, and the fetchland can't shuffle it away "
                                                 "first - one in hand can't be played (the opponent's turn, its end "
                                                 "step, or its land drop already spent this turn) and none on its "
                                                 "battlefield can be cracked - so looking at its own top card tells it "
                                                 "only what it is about to draw (the user's rule of 2026-11-18, "
                                                 "docs/ENGINE.md 194.8)"),
    'bauble_mishra_fetch_control': ('control', "a control, not a blunder: Mishra's Bauble with a fetchland it can use "
                                               "before the delayed draw (look at its own top card, then shuffle a bad "
                                               "one away) - the share of searches targeting itself is reported, not "
                                               "scored right or wrong"),
}


def spot_kind(r):
    """The canary kind of an activation row (any target: the position is the test), or None."""
    if r['excluded'] is not None:
        return None
    if r['bauble'] == 'urza':
        return 'bauble_urza_self' if r['opp_hand'] > 0 and not r['opp_hand_all_known'] else None
    if r['opp_top_known']:
        return None
    if r['fetch_available']:
        return 'bauble_mishra_fetch_control'
    if r['fetch_in_hand'] or r['fetch_on_battlefield']:
        return 'bauble_mishra_self_fetch_unusable'
    return 'bauble_mishra_self_no_fetch'


def spots(run, db, source, per_kind=6, log=print):
    """Canary spots of a run's recorded games (tools/delver_blunders.py library-add's input: {'spots': [...]}): per
    SPOT_KINDS kind, the first `per_kind` activations in game order (one a game) whose target decision offered both
    players, each as the game's seed and actions up to that decision, the watched option (the one that targets the
    activator: found by stepping each option), the decider, the answer and why. `source` labels them (e.g. the checkpoint)."""
    import gamedb
    from mtgz import _engine as E
    from zero import gamerec as GR
    con = gamedb.connect(db)
    cur = con.execute('SELECT b.*, g.spec AS spec_key, g.gid AS rec_gid, g.iter AS it FROM bauble b JOIN games g '
                      'USING (game_id) ORDER BY g.iter, g.gid, b.idx')
    cols = [d[0] for d in cur.description]
    rows = [dict(zip(cols, x)) for x in cur.fetchall()]
    recs = {}
    for it in sorted({r['it'] for r in rows}):
        specs, rs = GR.read_iteration(run, it)
        for x in rs:
            recs[gamedb.game_id(x)] = (x, specs[x['spec']])
    out, taken, seen = [], {k: 0 for k in SPOT_KINDS}, set()
    for r in rows:
        kind = spot_kind(r)
        if kind is None or taken[kind] >= per_kind or r['game_id'] not in recs or (r['game_id'], kind) in seen:
            continue
        rec, spec = recs[r['game_id']]
        acts = gamedb.actions_of(rec)
        sj = json.loads(spec)
        sj['log'] = True
        g = E.Game(_lay().record_game(rec), json.dumps(sj), int(rec['n']), int(rec['seed']))
        k = r['action'] + 1                        # the target decision: the step after the activation
        for a in acts[:k]:
            g.step(int(a))
        st = json.loads(g.export_state())
        d = st.get('decision') or {}
        if d.get('kind') != 'Target' or d.get('player') != r['activator'] or len(d.get('options') or []) != 2:
            continue
        watch = None
        for o in d['options']:
            h = g.clone()
            n0 = json.loads(h.export_state())['log_n']
            h.step(int(o['a']))
            nx = json.loads(h.export_state())
            if f'target P{r["activator"]}' in [ln.split(' ', 2)[2] for ln in nx['log'][-(nx['log_n'] - n0):]]:
                watch = o['t']
        if watch is None:
            raise ValueError(f'game {r["game_id"]} step {k}: no option targets the activator')
        answer, why = SPOT_KINDS[kind]
        out.append({'kind': kind, 'source': source, 'gid': int(rec['gid']), 'k': k, 'spec': spec,
                    'seed': int(rec['seed']), 'actions': [int(a) for a in acts[:k]], 'watch': watch,
                    'decider': r['activator'], 'turn': st['turn'],
                    'step': st['step'], 'answer': answer, 'why': why, 'chose_self': r['target'] == 'self',
                    'conditions': {c: r[c] for c in ('opp_hand', 'opp_hand_known', 'opp_library', 'fetch_in_hand',
                                                     'fetch_on_battlefield', 'manipulation_in_hand', 'own_turn',
                                                     'lands_played', 'land_drop_available', 'fetch_in_hand_playable',
                                                     'fetch_crackable')}})
        taken[kind] += 1
        seen.add((r['game_id'], kind))
    log(f'{len(out)} Bauble spots: {taken}')
    return {'spots': out}
