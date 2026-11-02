"""tools/delver_blunders.py (docs/blunders.md): the blunder counts on crafted games (states and frames
as the spectator view writes them), the standard errors, the fingerprints' frame / state pairing, and
the rebuild of a driver's move log from its seed (the engine)."""
import collections
import importlib.util
import json
import math
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _tool(name):
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'tools', name + '.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


B = _tool('delver_blunders')
F = _tool('delver_fingerprints')


# ------------------------------------------------------------------------------ crafted games
def card(name, cid, tapped=False, pt=None, damage=0, types=None):
    c = {'name': name, 'id': cid, 'types': types or (['creature'] if pt else ['land'] if name in B.LANDS else ['instant'])}
    if tapped:
        c['tapped'] = True
    if pt:
        c['pt'] = list(pt)
    if damage:
        c['damage'] = damage
    return c


def player(pid, hand=(), bf=(), pool=None, lands_played=0):
    p = {'id': pid, 'life': 20, 'hand': [card(n, 900 + 10 * pid + i) if isinstance(n, str) else n for i, n in enumerate(hand)],
         'battlefield': list(bf), 'graveyard': [], 'exile': [], 'library': 30, 'lands_played': lands_played, 'mulligans': 0}
    if pool:
        p['pool'] = pool
    return p


class Craft:
    """A game built step by step: add(state fields, the decider, its options, the chosen one, new log
    lines) -> states + frames as a rebuilt game has them (a state at every step)."""

    def __init__(self):
        self.states, self.frames, self.log = [], [], []

    def add(self, players, decider=0, kind='Priority', options=('Pass priority',), chosen=None, log=(), turn=3,
            step='main1', active=0, stack=()):
        self.log.extend(f'T{turn} {step.capitalize()} {ln}' for ln in log)
        opts = [{'a': i, 't': t} for i, t in enumerate(options)]
        s = {'turn': turn, 'step': step, 'active': active, 'starting': 0, 'players': [dict(p) for p in players],
             'stack': [dict(o) for o in stack], 'log': self.log[-40:], 'log_n': len(self.log),
             'decision': {'kind': kind, 'player': decider, 'options': opts}}
        self.states.append(s)
        a = options.index(chosen if chosen is not None else options[0])
        self.frames.append({'s': len(self.states) - 1, 'p': decider, 'k': 'nn', 'a': a})
        return self

    def done(self, players, log=(), turn=3, step='main1', active=0):
        self.log.extend(f'T{turn} {step.capitalize()} {ln}' for ln in log)
        self.states.append({'turn': turn, 'step': step, 'active': active, 'starting': 0, 'players': players, 'stack': [],
                            'log': self.log[-40:], 'log_n': len(self.log), 'decision': None})
        return {'states': self.states, 'frames': self.frames, 'result': [1.0, 0.0], 'final': len(self.states) - 1}


def bolt_spell(controller, sid=5):
    return {'name': 'Brainstorm', 'id': 50, 'stack': sid, 'spell': True, 'controller': controller}


def daze_game(them_bf=(), them_pool=None, pay=None, own=False):
    """P0 casts Daze (returning its tapped Island) at P1's Brainstorm; P1 answers the payment question."""
    me_bf = [card('Island', 1, tapped=True)]
    target_ctl = 0 if own else 1
    spell = bolt_spell(target_ctl)
    daze = {'name': 'Daze', 'id': 60, 'stack': 6, 'spell': True, 'controller': 0, 'targets': [{'id': 50, 'name': 'Brainstorm', 'live': True}]}
    p0 = player(0, hand=['Daze'], bf=me_bf)
    p1 = player(1, bf=list(them_bf), pool=them_pool)
    c = Craft()
    c.add([p0, p1], 0, options=('Pass priority', 'Cast Daze'), chosen='Cast Daze', stack=[spell], active=1)
    p0b = player(0, bf=[])
    c.add([p0b, p1], 0, kind='BounceCost', options=('Return Island to hand',), stack=[spell, dict(daze, targets=None)],
          log=['Daze Hand -> Stack', 'P0 casts Daze (Hand)', 'cost 1', 'target Brainstorm#50'], active=1)
    if pay is not None:
        c.add([p0b, p1], 1, kind='YesNo', options=('No', 'Yes'), chosen='Yes' if pay else 'No', stack=[spell, daze],
              log=['P0 returns Island', 'Island Battlefield -> Hand'], active=1)
    else:
        c.add([p0b, p1], 1, options=('Pass priority',), stack=[spell, daze], log=['P0 returns Island'], active=1)
    return c.done([p0b, p1], log=['Daze Stack -> Graveyard'], active=1)


def counts(d, players=None):
    return B.scan_game(d, players)


def test_daze_into_an_untapped_land_paid():
    c = counts(daze_game(them_bf=[card('Volcanic Island', 11)], pay=True))
    assert (c['daze_cast'], c['daze_payable'], c['daze_pay_land'], c['daze_pay_pool']) == (1, 1, 1, 0)
    assert (c['daze_asked'], c['daze_paid'], c['daze_paid_land']) == (1, 1, 1)


def test_daze_into_floating_mana_is_its_own_class():
    c = counts(daze_game(them_bf=[card('Volcanic Island', 11)], them_pool='U', pay=False))
    assert (c['daze_payable'], c['daze_pay_pool'], c['daze_pay_land'], c['daze_paid']) == (1, 1, 0, 0)


def test_daze_with_nothing_to_pay_and_an_uncracked_fetch():
    c = counts(daze_game(them_bf=[card('Volcanic Island', 11, tapped=True)]))
    assert (c['daze_cast'], c['daze_payable']) == (1, 0)
    c = counts(daze_game(them_bf=[card('Polluted Delta', 12)]))
    assert (c['daze_payable'], c['daze_pay_fetch']) == (1, 1)


def test_daze_at_its_own_spell_is_apart_and_players_filter():
    c = counts(daze_game(them_bf=[card('Island', 11)], own=True))
    assert (c['daze_cast'], c['daze_own_spell']) == (0, 1)
    c = counts(daze_game(them_bf=[card('Island', 11)], pay=True), players={1})
    assert c['daze_cast'] == 0


def wasteland_game(float_=None, cast_bs=False, castable=True, end_float_unseen=False):
    """P0's Wasteland at P1's untapped Volcanic Island; P1 holds Brainstorm."""
    land = card('Volcanic Island', 21)
    w = {'name': 'Wasteland', 'id': 3, 'stack': 9, 'ability': 'activated', 'controller': 0,
         'targets': [{'id': 21, 'name': 'Volcanic Island', 'live': True}]}
    p0 = player(0, bf=[card('Island', 1)])
    p1 = player(1, hand=['Brainstorm'], bf=[land])
    opts = ('Pass priority', 'Cast Brainstorm', 'Activate Volcanic Island') if castable else ('Pass priority', 'Activate Volcanic Island')
    c = Craft()
    if float_ is None and not end_float_unseen:
        c.add([p0, p1], 1, options=opts, stack=[w], log=['P0 activates Wasteland (Activated(1))', 'target Volcanic Island#21'])
        return c.done([p0, player(1, hand=['Brainstorm'])], log=['Volcanic Island Battlefield -> Graveyard'], step='end')
    c.add([p0, p1], 1, options=opts, chosen='Activate Volcanic Island', stack=[w],
          log=['P0 activates Wasteland (Activated(1))', 'target Volcanic Island#21'])
    tapped = player(1, hand=['Brainstorm'], bf=[card('Volcanic Island', 21, tapped=True)], pool='U')
    if end_float_unseen:
        # the step ends before any state shows the pool: only the log has the float
        return c.done([p0, player(1, hand=['Brainstorm'])], log=['P1 activates a mana ability of Volcanic Island',
                                                                  'Volcanic Island Battlefield -> Graveyard'], step='end')
    c.add([p0, tapped], 1, options=('Pass priority', 'Cast Brainstorm'), chosen='Cast Brainstorm' if cast_bs else 'Pass priority',
          stack=[w], log=['P1 activates a mana ability of Volcanic Island'])
    after = player(1, hand=[] if cast_bs else ['Brainstorm'], bf=[card('Volcanic Island', 21, tapped=True)],
                   pool=None if cast_bs else 'U')
    c.add([p0, after], 0, options=('Pass priority',), stack=[w] + ([bolt_spell(1, 10)] if cast_bs else []),
          log=['P1 casts Brainstorm (Hand)'] if cast_bs else [])
    return c.done([p0, player(1, hand=[])], log=['Volcanic Island Battlefield -> Graveyard'], step='end')


def test_wasteland_float_then_spent():
    c = counts(wasteland_game(float_=True, cast_bs=True))
    assert (c['waste_opp'], c['waste_used'], c['waste_float'], c['waste_float_spent']) == (1, 1, 1, 1)
    assert (c['float'], c['float_spent'], c['float_pointless'], c['float_wasteland']) == (1, 1, 0, 1)


def test_wasteland_float_pointless():
    c = counts(wasteland_game(float_=True, cast_bs=False))
    assert (c['float'], c['float_spent'], c['float_pointless'], c['float_wasted_units']) == (1, 0, 1, 1)
    assert (c['waste_float'], c['waste_float_spent'], c['waste_missed_spend']) == (1, 0, 0)


def test_a_float_no_state_shows_is_read_from_the_log():
    c = counts(wasteland_game(end_float_unseen=True))
    assert (c['float'], c['float_pointless'], c['float_wasteland'], c['waste_float'], c['waste_used']) == (1, 1, 1, 1, 1)


def test_wasteland_without_a_float_on_todays_engine():
    c = counts(wasteland_game())
    assert (c['waste_opp'], c['waste_used'], c['waste_float'], c['float'], c['pool_states']) == (1, 0, 0, 0, 0)
    assert (c['waste_missed_spend'], c['waste_missed_spend_bs']) == (1, 1)
    c = counts(wasteland_game(castable=False))
    assert c['waste_missed_spend'] == 0


def bounce_game(back_tapped=False, other_tapped=True, float_=False, hand=('Brainstorm',)):
    """P0 casts Daze at P1's spell, returning Island 1 (untapped unless back_tapped); Island 2 is the other."""
    isl = [card('Island', 1, tapped=back_tapped), card('Volcanic Island', 2, tapped=other_tapped)]
    spell = bolt_spell(1)
    daze = {'name': 'Daze', 'id': 60, 'stack': 6, 'spell': True, 'controller': 0, 'targets': [{'id': 50, 'name': 'Brainstorm', 'live': True}]}
    p1 = player(1)
    c = Craft()
    c.add([player(0, hand=['Daze'] + list(hand), bf=isl), p1], 0, options=('Pass priority', 'Cast Daze'), chosen='Cast Daze',
          stack=[spell], active=1)
    c.add([player(0, hand=list(hand), bf=isl), p1], 0, kind='BounceCost', options=('Return Island to hand', 'Return Volcanic Island to hand'),
          stack=[spell, dict(daze, targets=None)], log=['Daze Hand -> Stack', 'P0 casts Daze (Hand)'], active=1)
    rest = [isl[1]]
    c.add([player(0, hand=list(hand) + ['Island'], bf=rest, pool='U' if float_ else None), p1], 0,
          options=('Pass priority',) + (('Cast Brainstorm',) if float_ else ()), stack=[spell, daze],
          log=(['P0 activates a mana ability of Island'] if float_ else []) + ['P0 returns Island', 'Island Battlefield -> Hand'],
          active=1)
    return c.done([player(0, bf=rest), p1], active=1, step='end')


def test_daze_returning_an_untapped_island_over_a_tapped_one():
    c = counts(bounce_game())
    assert (c['daze_bounce'], c['daze_bounce_untapped'], c['daze_bounce_choice'], c['daze_bounce_untapped_over_tapped']) == (1, 1, 1, 1)
    assert (c['daze_bounce_float'], c['daze_bounce_missed_spend']) == (0, 1)
    c = counts(bounce_game(back_tapped=True, other_tapped=False))       # the tapped one returned: no loss
    assert (c['daze_bounce'], c['daze_bounce_untapped'], c['daze_bounce_choice'], c['daze_bounce_untapped_over_tapped']) == (1, 0, 1, 0)
    c = counts(bounce_game(other_tapped=False))                         # both untapped: no choice
    assert (c['daze_bounce_untapped'], c['daze_bounce_choice']) == (1, 0)
    c = counts(bounce_game(hand=()))
    assert c['daze_bounce_missed_spend'] == 0


def test_daze_bounce_float():
    c = counts(bounce_game(float_=True))
    assert (c['daze_bounce_float'], c['daze_bounce_untapped_over_tapped'], c['daze_bounce_missed_spend']) == (1, 0, 0)
    assert (c['float'], c['float_daze'], c['float_pointless']) == (1, 1, 1)


def burn_game(pt, damage, dealt, dies, spell='Lightning Bolt'):
    """P0's burn spell at P1's creature resolves."""
    cr = card('Murktide Regent', 31, pt=pt, damage=damage)
    s = {'name': spell, 'id': 40, 'stack': 7, 'spell': True, 'controller': 0, 'targets': [{'id': 31, 'name': 'Murktide Regent', 'live': True}]}
    c = Craft()
    p0 = player(0, bf=[card('Volcanic Island', 1, tapped=True)])
    c.add([p0, player(1, bf=[cr])], 1, options=('Pass priority',), stack=[s])
    after = player(1, bf=[] if dies else [card('Murktide Regent', 31, pt=pt, damage=damage + dealt)])
    return c.done([p0, after], log=[f'{dealt} damage to Murktide Regent#31', f'{spell} Stack -> Graveyard'])


def test_burn_misreads():
    c = counts(burn_game((7, 7), 3, 3, dies=False))
    assert (c['burn_creature'], c['burn_survived'], c['burn_hopeless'], c['burn_damaged'], c['burn_damaged_survived']) == (1, 1, 1, 1, 1)
    c = counts(burn_game((2, 2), 0, 3, dies=True))
    assert (c['burn_creature'], c['burn_survived'], c['burn_hopeless'], c['burn_damaged']) == (1, 0, 0, 0)
    c = counts(burn_game((4, 4), 0, 2, dies=False, spell='Unholy Heat'))
    assert (c['burn_survived'], c['burn_hopeless'], c['burn_survived_heat']) == (1, 1, 1)


def land_drop_game(played_same_turn=True, land='Wasteland'):
    """P0 casts Dragon's Rage Channeler with its only untapped land, the land drop unused and `land` in hand."""
    c = Craft()
    p1 = player(1, hand=['Daze'], bf=[card('Island', 51)])
    c.add([player(0, hand=["Dragon's Rage Channeler", land], bf=[card('Volcanic Island', 1)]), p1], 0,
          options=('Pass priority', "Cast Dragon's Rage Channeler", f'Play {land}'), chosen="Cast Dragon's Rage Channeler")
    tapped = player(0, hand=[land], bf=[card('Volcanic Island', 1, tapped=True)])
    c.add([tapped, p1], 1, options=('Pass priority', 'Cast Daze'), stack=[{'name': "Dragon's Rage Channeler", 'id': 8, 'stack': 4, 'spell': True, 'controller': 0}],
          log=["Dragon's Rage Channeler Hand -> Stack", "P0 casts Dragon's Rage Channeler (Hand)"])
    c.add([tapped, p1], 0, options=('Pass priority', f'Play {land}'), chosen=f'Play {land}' if played_same_turn else 'Pass priority',
          log=["Dragon's Rage Channeler Stack -> Battlefield"])
    if played_same_turn:
        return c.done([tapped, p1], log=[f'{land} Hand -> Battlefield', f'P0 plays {land}'])
    return c.done([tapped, p1], log=['Untap turn 4: P1', f'P0 plays {land}'], turn=4)


def test_tap_out_with_an_unused_land_drop():
    c = counts(land_drop_game())
    assert (c['ld_cast'], c['ld_tapout'], c['ld_blunder'], c['ld_blunder_live_daze'], c['ld_blunder_dazed']) == (1, 1, 1, 1, 0)
    c = counts(land_drop_game(played_same_turn=False))
    assert (c['ld_tapout'], c['ld_blunder']) == (1, 0)
    c = counts(land_drop_game(land='Thundering Falls'))      # enters tapped: playing it first gains nothing
    assert (c['ld_cast'], c['ld_blunder']) == (0, 0)


# ------------------------------------------------------------------------------ statistics, report
def test_ratio_and_cluster_standard_errors():
    r, se, n = B.ratio_se([1, 0, 2, 1], [2, 1, 2, 3])
    assert (round(r, 6), n) == (0.5, 8)
    # sum (x - r n)^2 = 0 + 0.25 + 1 + 0.25 = 1.5, x G / (G - 1) = 2, sqrt / N
    assert math.isclose(se, math.sqrt(2.0) / 8)
    m, se, g = B.per100_se([0, 1, 2, 1])
    assert (m, g) == (100.0, 4) and math.isclose(se, 100 * math.sqrt((2 / 3) / 4))
    assert B.ratio_se([0], [0]) == (None, None, 0)


def test_report_table_and_json():
    games = [{'c': dict(B.scan_game(daze_game(them_bf=[card('Island', 11)], pay=True)))} for _ in range(3)]
    s = {'label': 'x', 'games': games}
    t = B.table([s, s])
    assert 'into payable mana' in t and '(2) - (1)' in t and 'does not float mana' in t
    assert '(4) - (2)' in B.table([s, s, s, s], B.parse_diffs('4-2', 4))
    j = B.as_json([s])
    assert j[0]['metrics']['Daze: into payable mana (any)']['value'] == 1.0


# ------------------------------------------------------------------------------ fingerprints: frame -> state
def test_fingerprints_pair_frames_with_their_own_state():
    """zero-replay/1 frames carry their state index; a state without a frame (a forced pick) must not shift
    the later frames' states (the Brainstorm is read at state 2, not state 1)."""
    p = [player(0, hand=['Brainstorm']), player(1)]
    s0 = {'turn': 3, 'step': 'main1', 'active': 0, 'starting': 0, 'players': p, 'stack': [], 'log': [], 'log_n': 0,
          'decision': {'kind': 'Priority', 'player': 0, 'options': [{'a': 0, 't': 'Pass priority'}, {'a': 7, 't': 'Play Island'}]}}
    s1 = dict(s0, decision={'kind': 'PutOnTop', 'player': 0, 'options': [{'a': 3, 't': 'Put X on top'}]})
    s2 = dict(s0, decision={'kind': 'Priority', 'player': 0, 'options': [{'a': 0, 't': 'Pass priority'}, {'a': 5, 't': 'Cast Brainstorm'}]})
    d = {'states': [s0, s1, s2, dict(s0, decision=None)], 'frames': [{'s': 0, 'a': 7}, {'s': 2, 'a': 5}], 'result': [1.0, 0.0]}
    band = F.Band()
    F.scan_game(d, band)
    assert band.c['bs'] == 1


# ------------------------------------------------------------------------------ the rebuild (engine)
def test_game_seed_pairs_and_rebuilds_a_driver_game():
    """The driver's per-game engine seed (paired: games 2k, 2k + 1 share it) and the rebuild of its move
    log: heuristic against random, unpaired, every game replays to its own result."""
    assert B.game_seed(5, 4, True) == B.game_seed(5, 5, True) != B.game_seed(5, 4, False)
    import mtgz.az as P
    from mtgz.az import games as G
    from mtgz.az.infer import drive
    from zero.games import engine
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r], [r, h]] * 2, spec=P.SPEC_DELVER, concurrency=4, threads=2, seed=17, max_steps=20000,
                   move_log=1_000_000)
    drive(drv, {})
    E = engine()
    rows = drv.take_results()
    assert len(rows) == 4
    for row in rows:
        d = B.rebuild(E, P.SPEC_DELVER, 17, row)
        fin = d['states'][-1]
        assert fin['over'] and len(d['states']) == len(d['frames']) + 1 > 50
        res = row['result']
        if res[0] != res[1]:
            assert fin['winner'] == (0 if res[0] > res[1] else 1)
        B.scan_game(d)                              # (counts a real game without error)
    # another driver seed, or a move log that is not this game's: the rebuild stops loudly
    try:
        B.rebuild(E, P.SPEC_DELVER, 18, rows[0])
        raise AssertionError('a wrong seed replayed')
    except RuntimeError:
        pass
    bad = dict(rows[0], moves=[list(m) for m in rows[0]['moves']])
    bad['moves'][5][2] = list(bad['moves'][5][2]) + [9999]
    try:
        B.rebuild(E, P.SPEC_DELVER, 17, bad)
        raise AssertionError('a wrong move log replayed')
    except RuntimeError:
        pass


# ------------------------------------------------------------------------------ canaries
def test_scans_record_canary_spots():
    sp = []
    B.scan_game(land_drop_game(), None, None, sp)
    assert sp == [('land_drop_tapout', 0, "Cast Dragon's Rage Channeler")]
    sp = []
    B.scan_game(daze_game(them_bf=[card('Island', 11)], pay=True), None, None, sp)
    assert sp == [('daze_payable_land', 0, 'Cast Daze')]
    sp = []
    B.scan_game(wasteland_game(), None, None, sp)
    assert sp == [('waste_brainstorm', 0, 'Cast Brainstorm')]


def test_canary_search_of_a_rebuilt_spot():
    """A spot (seed + actions) replays to its decision, and the evaluation seat's lockstep search (a tiny
    random network, the 'cq' rule, root-view leaves) picks a legal option at every repeat; the table
    reads the results."""
    import torch
    import mtgz.az as P
    from mtgz.az import games as G
    from mtgz.az.infer import drive
    from zero.games import engine
    from zero.infer import NetEvaluator
    from zero.net import make_net
    h, r = G.seat_dict('heuristic'), G.seat_dict('random')
    drv = G.Driver('mtg', [[h, r]], spec=P.SPEC_DELVER, concurrency=1, threads=1, seed=5, max_steps=20000, move_log=1_000_000)
    drive(drv, {})
    row = drv.take_results()[0]
    E = engine()
    d = B.rebuild(E, P.SPEC_DELVER, 5, row)
    k = next(i for i, s in enumerate(d['states'][:-1]) if (s.get('decision') or {}).get('kind') == 'Priority'
             and len(s['decision']['options']) >= 2 and s['turn'] >= 3)
    st = d['states'][k]
    spot = {'kind': 'land_drop_tapout', 'source': 't', 'gid': 0, 'k': k, 'spec': P.SPEC_DELVER, 'seed': B.game_seed(5, 0),
            'actions': [f['a'] for f in d['frames'][:k]], 'watch': st['decision']['options'][0]['t'],
            'decider': st['decision']['player']}
    g = B.spot_game(E, spot)
    assert json.loads(g.export_state())['decision'] == st['decision']
    torch.manual_seed(0)
    ev = NetEvaluator(make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2), 'cpu')
    seat = {'select': 'cq', 'select_min_visits': 4, 'cq_c_visit': 50.0, 'cq_c_scale': 0.3, 'leaf_value_view': 'root'}
    chosen = B.search_many(ev, [g.clone() for _ in range(3)], [1, 2, 3], 8, seat)
    legal = {o['a'] for o in st['decision']['options']}
    assert len(chosen) == 3 and set(chosen) <= legal
    labels = {o['a']: o['t'] for o in st['decision']['options']}
    per = [dict(collections.Counter(labels[a] for a in chosen))]
    t = B.canary_table({'spots': [spot], 'repeats': 3, 'results': {'x_0001': {'8': per}}})
    assert 'land_drop_tapout (1)' in t and 'x_0001 s8' in t


# ------------------------------------------------------------------------------ baseline, flags, the watcher
def _scan_file(tmp_path, games, label='x_0424_s32'):
    p = tmp_path / f'{label}.scan.json'
    p.write_text(json.dumps({'format': B.SCAN_FORMAT, 'label': label, 'games': games,
                             'meta': {'label': label, 'ckpt_sha256': 'abc', 'driver_seed': 1, 'sims': 32,
                                      'engine_play': {'digest': None, 'engine_sha256': 'e', 'commit': 'c'}}}))
    return str(p)


def test_baseline_file_adds_only_new_metrics_and_flags_at_two_se(tmp_path):
    games = [{'c': dict(B.scan_game(daze_game(them_bf=[card('Island', 11)], pay=True)))} for _ in range(20)]
    games += [{'c': dict(B.scan_game(daze_game(them_bf=[card('Island', 11, tapped=True)])))} for _ in range(20)]
    out = str(tmp_path / 'base.json')
    b = B.make_baseline(_scan_file(tmp_path, games), out, digest='d1', log=lambda *a: None)
    assert b['engine']['digest'] == 'd1' and b['metrics']['daze_payable']['value'] == 0.5
    assert b['metrics']['daze_cast_per100g']['value'] == 100.0 and b['suite'] == B.SUITE_VERSION
    # an entry already there is kept; a missing one is added (a new category joins)
    b['metrics']['daze_payable']['value'] = 0.25
    del b['metrics']['ld_blunder']
    json.dump(b, open(out, 'w'))
    b2 = B.make_baseline(_scan_file(tmp_path, games), out, log=lambda *a: None)
    assert b2['metrics']['daze_payable']['value'] == 0.25 and 'ld_blunder' in b2['metrics']
    base = b2['metrics']['daze_payable']
    assert B.versus(0.25, 0.01, base, 100)[2] is False
    gap, z, flag = B.versus(0.75, 0.0, base, 400)        # se 0 floored at sqrt(p0 (1 - p0) / n)
    assert flag and math.isclose(z, 0.5 / math.hypot(math.sqrt(0.25 * 0.75 / 400), base['se']))
    assert B.versus(0.75, 0.0, base, 5)[2] is False       # too few events to flag
    t = B.table([B.load_scan(_scan_file(tmp_path, games))], None, B.load_baseline(out))
    assert '(B) baseline x_0424_s32' in t and ' *' in t


def test_watch_measures_the_newest_evaluated_checkpoint_first(tmp_path, monkeypatch):
    run = tmp_path / 'fork'
    os.makedirs(run / 'models')
    (run / 'config.json').write_text(json.dumps({'eval_every': 8}))
    old = 1_000_000_000
    for it in (528, 530, 536, 544):
        f = run / 'models' / f'iter_{it:04d}.pt'
        f.write_bytes(b'')
        os.utime(f, (old, old))
    seen = []

    def fake(d, it, ckpt, out, *a, **k):
        seen.append(it)
        open(os.path.join(out, f'iter_{it:04d}.json'), 'w').write('{}')
    monkeypatch.setattr(B, 'measure_one', fake)
    quiet = dict(once=True, baseline=None, log=lambda *a: None)
    B.watch(str(run), min_iter=528, **quiet)
    assert seen == [544]                                  # behind: 528, 536 skipped
    B.watch(str(run), min_iter=528, backfill=True, **quiet)
    assert seen == [544, 536, 528]                        # backfill: newest pending first
    f = run / 'models' / 'iter_0552.pt'
    f.write_bytes(b'')
    B.watch(str(run), **quiet)                            # default: the newest at launch on
    assert seen == [544, 536, 528] and not (run / 'blunders' / 'iter_0552.json').exists()  # (too fresh: settling)
    os.utime(f, (old, old))
    B.watch(str(run), **quiet)
    assert seen[-1] == 552
    (run / 'blunders' / 'STOP').write_text('')
    assert B.watch(str(run), **quiet) == 0 and seen[-1] == 552
