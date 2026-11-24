"""The stew's sideboarding health (mtgz.stew.sbhealth, docs/ENGINE.md 260): the table plan from synthetic matchup
versions, the alerts and the reference levels, the game-log fallback, the colour classification of colour-conditional
answers (split cards, double-faced cards, tokens), the plan-vs-no-plan jobs and statistics, the report section, and
recipe.sb_health off. No match is played, no database is read (the real data's check: docs/ENGINE.md 260)."""
import collections
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tests'))
import mtgz.az  # noqa: E402,F401
from mtgz.az import decklists as D  # noqa: E402
from mtgz.az import matchup as M  # noqa: E402
from mtgz.stew import config as C  # noqa: E402
from mtgz.stew import judge as J  # noqa: E402
from mtgz.stew import sbhealth as SB  # noqa: E402
from mtgz.stew.orchestrator import Stew  # noqa: E402
from mtgz.stew.state import jload, jsave  # noqa: E402

from test_stew import DD, DEL, SNS, FakeOps, make_cfg, make_source  # noqa: E402

OPTS = dict(SB.DEFAULT)


# ------------------------------------------------------------------ helpers
def deck75(name):
    """A deck's 75 as the matchup model holds it: (names in list order, copies, the main's copies)."""
    main, side = D._seat_lists(name, 'bo3')
    order = list(dict.fromkeys(list(main) + list(side)))
    c75, cm = collections.Counter(list(main) + list(side)), collections.Counter(main)
    return order, [c75[n] for n in order], [cm[n] for n in order]


def table_of(name, version, swap=None):
    """A choice table of `name`'s 75: the main in every context, or with `swap` (out, in) in every context."""
    names, counts, main = deck75(name)
    c = list(main)
    if swap:
        c[names.index(swap[0])] -= 1
        c[names.index(swap[1])] += 1
    return {'version': version, 'list': [[n, k] for n, k in zip(names, counts)],
            'choices': [[g, f, l, o, list(c)] for g, f, l, o in M.contexts()]}


def write_mirror_version(run, it, swap=None, table=True):
    names, counts, main = deck75(DEL)
    v = {'format': M.FORMAT, 'version': f'v{it:04d}', 'iter': it, 'list': [[n, k] for n, k in zip(names, counts)],
         'main': main, 'table': table_of(DEL, f'v{it:04d}', swap) if table else None}
    jsave(os.path.join(run, 'matchup', f'v{it:04d}.json'), v)


def write_pair_version(run, it, swaps):
    """A two-75 version of DEL vs DD; swaps {deck: (out, in) or None or 'none' (no table at all)}."""
    decks, tabs = [], []
    for d in (DEL, DD):
        names, counts, main = deck75(d)
        decks.append({'name': d, 'list': [[n, k] for n, k in zip(names, counts)], 'main': main})
        tabs.append(table_of(d, f'v{it:04d}', swaps.get(d)))
    v = {'format': M.FORMAT, 'version': f'v{it:04d}', 'iter': it, 'pairing': {'mirror': False, 'decks': decks},
         'model': 'pair', 'table': None, 'tables': tabs}
    jsave(os.path.join(run, 'matchup', f'v{it:04d}.json'), v)


# ------------------------------------------------------------------ the table plan
def test_table_plan_streaks_and_stats(tmp_path):
    run = str(tmp_path / 'spec-mirror')
    write_mirror_version(run, 16, swap=('Force of Will', 'Pyroblast'))
    write_mirror_version(run, 32)
    write_mirror_version(run, 48)
    t = SB.table_plan(run, DEL)
    d = t['decks'][DEL]
    assert t['version'] == 'v0048' and t['versions'] == 3
    assert d['state'] == 'all main' and d['streak'] == 2
    assert d['swaps'] == {'min': 0, 'mean': 0.0, 'max': 0} and d['in'] == {} and d['contexts'] == 24
    # at the iteration of v0016 (the generation's checkpoint before v0032): it boards
    t16 = SB.table_plan(run, DEL, upto_iter=20)
    d16 = t16['decks'][DEL]
    assert t16['version'] == 'v0016' and d16['state'] == 'boards' and d16['streak'] == 1
    assert d16['swaps'] == {'min': 1, 'mean': 1.0, 'max': 1}
    assert d16['in'] == {'Pyroblast': 1.0} and d16['out'] == {'Force of Will': 1.0}
    assert d16['top'] == [{'changes': {'Force of Will': -1, 'Pyroblast': 1}, 'contexts': 24}]
    assert SB.table_plan(run, DEL, upto_iter=10) == {'note': 'no matchup version yet at or before iteration 10'}


def test_table_plan_of_two_decks_and_a_version_without_table(tmp_path):
    run = str(tmp_path / 'spec-pair')
    write_pair_version(run, 16, {DEL: ('Wasteland', 'Force of Negation')})
    write_pair_version(run, 32, {DEL: ('Wasteland', 'Force of Negation')})
    t = SB.table_plan(run)
    assert t['decks'][DEL]['state'] == 'boards' and t['decks'][DEL]['streak'] == 2
    assert t['decks'][DD]['state'] == 'all main' and t['decks'][DD]['streak'] == 2
    mrun = str(tmp_path / 'spec-m')
    write_mirror_version(mrun, 16)
    write_mirror_version(mrun, 32, table=False)
    d = SB.table_plan(mrun, DEL)['decks'][DEL]
    assert d['state'] == 'no table' and d['streak'] == 1 and 'swaps' not in d
    nt = SB.newest_tables(run)
    assert set(nt) == {DEL, DD} and nt[DD]['counts'] == deck75(DD)[1]


# ------------------------------------------------------------------ alerts and the reference
def test_alerts_all_main_misaim_and_not_ingested():
    h = {'specialists': {
        'spec:a|a': {'table': {'version': 'v0048', 'decks': {'a': {'state': 'all main', 'streak': 2}}},
                     'card_use': {'decks': {'a': {'Pyroblast': {'all': {'right': 60, 'wrong': 40, 'wrong_rate': 0.4},
                                                                '2': {'right': 30, 'wrong': 20, 'wrong_rate': 0.4}},
                                                  'Hydroblast': {'all': {'right': 90, 'wrong': 10, 'wrong_rate': 0.1}}}}}},
        'spec:a|b': {'table': {'version': 'v0064', 'decks': {'a': {'state': 'all main', 'streak': 1},
                                                             'b': {'state': 'boards', 'streak': 4}}},
                     'card_use': {'note': 'not ingested (no database spec-a-b)'}},
        'spec:a|c': {'table': {'note': 'no matchup version yet'},
                     'card_use': {'decks': {'a': {'Pyroblast': {'all': {'right': 5, 'wrong': 20, 'wrong_rate': 0.8}},
                                                  'Mystery Blast': {'all': {'right': 80, 'wrong': 20,
                                                                            'wrong_rate': 0.2}}}}}}}}
    al = SB.alerts(h, OPTS)
    assert any("spec:a|a: a's choice table all main for 2" in x for x in al)
    assert any('spec:a|a' in x and 'Pyroblast' in x and '40.0%' in x and 'pb2-like' in x for x in al)
    assert not any('Hydroblast' in x for x in al)                       # 10%: pb3-like, no alert
    assert any(x == 'spec:a|b: card use not ingested (no database spec-a-b)' for x in al)
    assert not any('spec:a|b' in x and 'all main' in x for x in al)     # one version: not yet
    assert not any('spec:a|c' in x and 'Pyroblast' in x for x in al)    # 25 casts < min_casts
    assert any('Mystery Blast' in x and 'alert above 15%' in x for x in al)   # no reference: the fallback line
    cu = h['specialists']['spec:a|a']['card_use']['decks']['a']
    assert cu['Hydroblast']['all']['level'] == 'pb3-like' and cu['Pyroblast']['2']['level'] == 'pb2-like'
    assert SB.reference_level('Pyroblast', 0.2) == 'between pb3 and pb2'
    assert SB.reference_level('Mystery Blast', 0.2) is None
    # a numeric line for every card
    al2 = SB.alerts(json.loads(json.dumps(h)), dict(OPTS, misaim_alert=0.05))
    assert any('Hydroblast' in x and 'alert above 5%' in x for x in al2)
    assert not any('all main' in x for x in SB.alerts(json.loads(json.dumps(h)), dict(OPTS, all_main_versions=3)))


def test_reference_values_are_the_notes():
    r = SB.REFERENCE
    assert r['pb2']['misaim'] == {'Pyroblast': 0.32, 'Hydroblast': 0.37}
    assert r['pb3']['misaim'] == {'Pyroblast': 0.15, 'Hydroblast': 0.13}
    assert r['pb2']['counter'] == {'Pyroblast': 0.12, 'Hydroblast': 0.05}
    assert r['pb3']['counter'] == {'Pyroblast': 0.20, 'Hydroblast': 0.20}
    assert 'blast_targeting_pb3_2026-11-16' in r['source'] and ':' not in r['source'].split('(')[0]
    assert SB.misaim_line('Pyroblast', OPTS) == 0.32 and SB.misaim_line('X', OPTS) == 0.15


# ------------------------------------------------------------------ colours
IR = [
    {'name': 'Pyroblast', 'printing': {'mana_cost': {'colored': [['R', 1]]}, 'spell_effect': {
        '$t': 'ModalChooseOne', 'modes': [{'$t': 'CounterSpell', 'only_if': {'$t': 'HasColor', 'color': 'U'}},
                                          {'$t': 'RemoveTargetPermanents', 'only_if': {'$t': 'HasColor', 'color': 'U'}}]}}},
    {'name': 'Red Elemental Blast', 'printing': {'mana_cost': {'colored': [['R', 1]]}, 'spell_effect': {
        '$t': 'ModalChooseOne', 'modes': [{'$t': 'CounterSpell', 'only_if': {'$t': 'HasColor', 'color': 'U'}},
                                          {'$t': 'RemoveTargetPermanents', 'only_if': {'$t': 'HasColor', 'color': 'U'}}]}}},
    {'name': 'Half Blast', 'printing': {'spell_effect': {
        '$t': 'ModalChooseOne', 'modes': [{'$t': 'CounterSpell', 'only_if': {'$t': 'HasColor', 'color': 'U'}},
                                          {'$t': 'DealDamage'}]}}},
    {'name': 'Island', 'printing': {'mana_cost': {'$t': 'ManaCost'}, 'card_types': ['Land']}},
    {'name': 'Brainstorm', 'printing': {'mana_cost': {'colored': [['U', 1]]}}},
    {'name': 'Fire // Ice', 'printing': {'split_faces': [{'name': 'Fire', 'mana_cost': {'colored': [['R', 1]]}},
                                                         {'name': 'Ice', 'mana_cost': {'colored': [['U', 1]]}}]}},
    {'name': 'Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar', 'printing': {
        'mana_cost': {'colored': [['U', 1]]},
        'transform_back_face': {'name': 'Tamiyo, Seasoned Scholar', 'face_colors': ['G', 'U'],
                                'mana_cost': {'colored': [['U', 1]]}},
        'triggered_abilities': [{'effect': {'$t': 'CreateToken', 'token_printing': {
            'name': 'Clue', 'card_types': ['Artifact'], 'mana_cost': {'$t': 'ManaCost'}}}}]}},
    {'name': 'Cori-Steel Cutter', 'printing': {'mana_cost': {'colored': [['R', 1]], 'generic': 1}, 'x': {
        'create': {'token_printing': {'name': 'Monk', 'color_indicator': ['W'], 'mana_cost': {'$t': 'ManaCost'}}}}}},
    {'name': 'Painter', 'printing': {'mana_cost': {'colored': [['R', 1]]}}},
    {'name': 'Painter', 'printing': {'mana_cost': {'colored': [['U', 1]]}}},     # (two sources disagree)
]


def test_colour_map_split_dfc_tokens():
    cm = SB.colour_map(IR)
    assert cm['Island'] == frozenset() and cm['Brainstorm'] == {'U'}
    assert cm['Fire // Ice'] == {'R', 'U'} and cm['Fire'] == {'R'} and cm['Ice'] == {'U'}
    assert cm['Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar'] == {'U'}
    assert cm['Tamiyo, Inquisitive Student'] == {'U'} and cm['Tamiyo, Seasoned Scholar'] == {'G', 'U'}
    assert cm['Clue'] == frozenset() and cm['Monk'] == {'W'}
    assert cm['Painter'] is None                                     # ambiguous: unknown, never guessed


def test_conditional_answers_and_classification():
    ans = SB.conditional_answers(IR)
    assert set(ans) == {'Pyroblast', 'Red Elemental Blast'}         # Half Blast: one mode unconditional
    assert ans['Pyroblast'] == [{'type': 'CounterSpell', 'colour': 'U'}, {'type': 'RemoveTargetPermanents', 'colour': 'U'}]
    cm = SB.colour_map(IR)
    k = lambda t, m=1: SB.classify('Pyroblast', m, t, cm, ans)     # noqa: E731
    assert k('Brainstorm', 0) == 'right' and k('Island') == 'wrong' and k('Monk') == 'wrong'
    assert k('Ice', 0) == 'right' and k('Fire', 0) == 'wrong' and k('Fire // Ice') == 'right'
    assert k('Tamiyo, Seasoned Scholar') == 'right' and k('Clue') == 'wrong'
    assert k('Painter') == 'unknown' and k('ability of X (stack #3)', 0) == 'unknown'
    assert k(None) == 'no target' and k('Brainstorm', None) == 'right'      # (mode unknown: both modes need blue)


def test_card_use_rows_by_game_and_counter_share():
    cm, ans = SB.colour_map(IR), SB.conditional_answers(IR)
    mode = lambda m: json.dumps({'text': f'mode {m}'})              # noqa: E731
    rows = [{'deck': 'd', 'game_no': 2, 'card': 'Pyroblast', 'mode_line': mode(0), 'target': 'Brainstorm'},
            {'deck': 'd', 'game_no': 2, 'card': 'Pyroblast', 'mode_line': mode(1), 'target': 'Island'},
            {'deck': 'd', 'game_no': 3, 'card': 'Pyroblast', 'mode_line': mode(1), 'target': 'Painter'},
            {'deck': 'd', 'game_no': 3, 'card': 'Pyroblast', 'mode_line': None, 'target': None}]
    u = SB.card_use_rows(rows, cm, ans)['d']['Pyroblast']
    assert u['2'] == {'casts': 2, 'right': 1, 'wrong': 1, 'unknown': 0, 'no_target': 0, 'wrong_rate': 0.5,
                      'wrong_se': 0.3536, 'counter': 1, 'counter_share': 0.5}
    assert u['3']['unknown'] == 1 and u['3']['no_target'] == 1 and u['3']['wrong_rate'] is None
    assert u['3']['counter_share'] == 0.0                            # one cast's mode unknown: not counted
    assert u['all']['casts'] == 4 and u['all']['wrong_rate'] == 0.5 and u['all']['counter_share'] == pytest.approx(1 / 3,
                                                                                                                  abs=1e-4)


def test_the_real_ir_knows_the_blasts():
    ans = SB.conditional_answers()
    assert {'Pyroblast', 'Hydroblast'} <= set(ans)
    assert {m['colour'] for m in ans['Pyroblast']} == {'U'} and {m['colour'] for m in ans['Hydroblast']} == {'R'}
    cm = SB.colour_map()
    assert cm['Murktide Regent'] == {'U'} and cm['Volcanic Island'] == frozenset()
    assert cm['Monk'] == {'W'}                                       # Cori-Steel Cutter's token


# ------------------------------------------------------------------ the play: the game log fallback, no database
def write_game_log(run):
    names, counts, main = deck75(DEL)
    os.makedirs(os.path.join(run, 'games'))
    with open(os.path.join(run, 'metrics.jsonl'), 'w', encoding='utf-8') as f:
        for it in (1, 2, 3):
            f.write(json.dumps({'iter': it, 'time': 100.0 * it}) + '\n')
    dealt = list(main)
    dealt[names.index('Force of Will')] -= 2
    dealt[names.index('Pyroblast')] += 2
    sp = {'kind': 'bo3-spec', 'spec': 's1', 'dealt_decks': {'p': 0.85, 'swaps': [0, 15]}, 'lists': [[]], 'mains':
          [main, main], 'metagame': {'decks': [0, 0], 'names': [DEL, DEL]}}
    games = [
        {'kind': 'bo3', 'spec': 's1', 'time': 150.0, 'game': 1, 'mode': ['main', 'main'], 'decks': [main, main],
         'result': [1.0, 0.0]},
        {'kind': 'bo3', 'spec': 's1', 'time': 160.0, 'game': 2, 'mode': ['dealt', 'table'], 'decks': [dealt, main],
         'result': [0.0, 1.0]},
        {'kind': 'bo3', 'spec': 's1', 'time': 170.0, 'game': 2, 'mode': ['dealt', 'dealt'], 'decks': [dealt, main],
         'result': [1.0, 0.0]},
        {'kind': 'bo3', 'spec': 's1', 'time': 50.0, 'game': 2, 'mode': ['dealt', 'dealt'], 'decks': [dealt, main],
         'result': [1.0, 0.0]}]                                      # (iteration 1: outside the window)
    with open(os.path.join(run, 'games', 'games-1.jsonl'), 'w', encoding='utf-8') as f:
        for x in [sp] + games:
            f.write(json.dumps(x) + '\n')


def test_play_from_the_game_log_when_not_ingested(tmp_path):
    run = str(tmp_path / 'spec-izzet-izzet')
    write_game_log(run)
    write_mirror_version(run, 2)
    os.makedirs(os.path.join(run, 'models'))
    open(os.path.join(run, 'models', 'iter_0002.pt'), 'wb').write(b'x')
    h = SB.specialist(run, [DEL], dict(OPTS, gamedb=str(tmp_path / 'nodb')), 2, 1)
    assert h['window'] == [2, 2]
    assert h['card_use'] == {'note': 'not ingested (no database spec-izzet-izzet)'}
    p = h['play']
    assert p['source'] == 'game log (local self-play only)' and 'not ingested' in p['db']
    by = {(r['game_no'], r['mode']): r for r in p['rows']}
    assert by[(1, 'main')]['seats'] == 2 and by[(1, 'main')]['score'] == 0.5
    assert by[(2, 'dealt')] == {'deck': DEL, 'game_no': 2, 'mode': 'dealt', 'seats': 3, 'score': pytest.approx(1 / 3),
                                'cards_in': pytest.approx(4 / 3)}
    assert by[(2, 'table')]['seats'] == 1 and by[(2, 'table')]['score'] == 1.0 and by[(2, 'table')]['cards_in'] == 0
    # no log, no database: said, never a silent zero
    run2 = str(tmp_path / 'spec-x')
    os.makedirs(os.path.join(run2, 'models'))
    h2 = SB.specialist(run2, [DEL], dict(OPTS, gamedb=str(tmp_path / 'nodb')), 2, 4)
    assert h2['play'] == {'note': 'not ingested (no database spec-x); no game log'}


def test_a_database_of_another_run_of_that_name_is_refused(tmp_path):
    run = str(tmp_path / 'spec-a')
    os.makedirs(os.path.join(run, 'models'))
    open(os.path.join(run, 'models', 'iter_0002.pt'), 'wb').write(b'x')
    db = tmp_path / 'dbs' / 'spec-a'
    jsave(str(db / 'state.json'), {'iterations': {'1': {}, '64': {}}})
    d, its, why = SB.db_of(run, str(tmp_path / 'dbs'))
    assert d is None and 'another run of that name' in why
    jsave(str(db / 'state.json'), {'iterations': {'1': {}, '2': {}}})
    d, its, why = SB.db_of(run, str(tmp_path / 'dbs'))
    assert d == str(db) and its == [1, 2] and why is None
    h = SB.specialist(run, [DEL], dict(OPTS, gamedb=str(tmp_path / 'dbs')), 2, 1)
    pytest.importorskip('duckdb')
    assert h['window'] == [2, 2] and h['play'] == {'source': 'game database', 'iterations': 1, 'of': 1, 'rows': []}
    assert h['card_use'] == {'source': 'game database', 'iterations': 1, 'of': 1, 'decks': {}}


# ------------------------------------------------------------------ plan vs no plan: the jobs
class PlanState:
    def __init__(self, decks, cands, skipped=()):
        al = {DEL: 'izzet', DD: 'doomsday', SNS: 'sneak'}
        self._d = [{'name': d, 'share': 1, 'alias': al[d]} for d in decks]
        self._c = list(cands)
        self.skipped = set(skipped)

    def decks(self):
        return self._d

    def candidates(self):
        return self._c

    def version(self, name, gen):
        return {'path': f'{name}@{gen}.pt', 'iter': 2 * gen, 'skipped': 'x' if name in self.skipped else None}

    def abs(self, p):
        return p


def spec_fn(p, q, c):
    s = json.loads(D.pair_spec(p, q, fmt='bo3', game_episodes=True))
    if c is not None:
        s['chooser'] = int(c)
    return json.dumps(s, separators=(',', ':'))


def tables_for(*decks, swap=None):
    out = {}
    for d in decks:
        names, counts, main = deck75(d)
        out[d] = {'name': d, 'names': names, 'counts': counts, 'main': main, 'table': table_of(d, 'v0002', swap.get(d)
                                                                                                if swap else None)}
    return out


def test_plan_arm_jobs_of_a_mirror():
    from mtgz import _engine as E
    st = PlanState([DEL, DD], [DEL])
    mir = C.spec_name(DEL, DEL)
    tabs = {mir: tables_for(DEL, swap={DEL: ('Force of Will', 'Pyroblast')})}
    jobs, arms = J.plan_arm_jobs(st, 0, {'plan_matches': 8, 'seed': 0}, spec_fn, {}, tabs)
    assert [j['id'] for j in jobs] == ['P.izzet-izzet.o0', 'P.izzet-izzet.o1']
    assert arms == [{'name': f'{mir} {DEL}', 'kind': 'mirror', 'deck': DEL, 'jobs': ['P.izzet-izzet.o0', 'P.izzet-izzet.o1']}]
    plan = tabs[mir][DEL]['table']
    for s, j in enumerate(jobs):
        sp = json.loads(j['spec'])
        assert j['a'] == j['b'] == f'{mir}@1.pt' and j['games'] == 4 and j['layout'] == 'a0' and j['chooser'] is None
        assert sp['start_game'] == 2 and 'chooser' not in sp
        assert sp['decks'][f'p{s}_table'] == plan
        none = sp['decks'][f'p{1 - s}_table']
        assert none['list'] == plan['list'] and {tuple(ch[4]) for ch in none['choices']} == {tuple(deck75(DEL)[2])}
        E.mtg_spec_check(j['spec'])                                   # the engine reads it (no chooser, game 2)
    assert jobs[0]['seed'] == jobs[1]['seed']                         # common deals, the seats swapped
    assert J.job_hash(jobs[0]) != J.job_hash(jobs[1])
    # a specialist that sat out, or has no table: no arm
    st2 = PlanState([DEL], [DEL], skipped=[mir])
    assert J.plan_arm_jobs(st2, 0, {'plan_matches': 8, 'seed': 0}, spec_fn, {}, tabs) == ([], [])
    t0 = {mir: {DEL: dict(tabs[mir][DEL], table=None)}}
    assert J.plan_arm_jobs(st, 0, {'plan_matches': 8, 'seed': 0}, spec_fn, {}, t0) == ([], [])


def test_plan_arm_jobs_of_two_decks():
    from mtgz import _engine as E
    st = PlanState([DEL, DD], [DEL])
    sn = C.spec_name(DEL, DD)
    tabs = {sn: tables_for(DEL, DD, swap={DEL: ('Wasteland', 'Force of Negation')})}
    tabs[sn][DD]['table'] = None                                      # DD without a table: all main, fixed
    jobs, arms = J.plan_arm_jobs(st, 0, {'plan_matches': 8, 'seed': 0}, spec_fn, {}, tabs)
    assert [a['name'] for a in arms] == [f'{sn} {DEL}']               # only DEL has a table
    assert len(jobs) == 8 and all(j['games'] == 2 for j in jobs)
    by = {j['id']: j for j in jobs}
    for p_, q_, xs in J.orientations(DEL, DD):
        for c in (0, 1):
            jp, jn = by[f'P.izzet-doomsday.s{xs}.c{c}.plan'], by[f'P.izzet-doomsday.s{xs}.c{c}.none']
            assert jp['seed'] == jn['seed'] and jp['decks'] == [p_, q_] and jp['chooser'] == c
            sp, sn_ = json.loads(jp['spec']), json.loads(jn['spec'])
            assert sp['chooser'] == c and 'start_game' not in sp           # two 75s: game 1 played, the same in both
            assert sp['decks'][f'p{xs}_table'] == tabs[sn][DEL]['table']
            assert {tuple(ch[4]) for ch in sn_['decks'][f'p{xs}_table']['choices']} == {tuple(deck75(DEL)[2])}
            assert sp['decks'][f'p{1 - xs}_table'] == sn_['decks'][f'p{1 - xs}_table']
            E.mtg_spec_check(jp['spec'])
            E.mtg_spec_check(jn['spec'])
    assert ('P.izzet-doomsday.s0.c0.plan', 'P.izzet-doomsday.s0.c0.none', 0) in arms[0]['pairs']
    with pytest.raises(ValueError, match="not seat"):
        J._with_seat_tables(spec_fn(DEL, DD, 0), {0: tabs[sn][DD]['table'] or SB.main_only_table(
            'x', tabs[sn][DD]['names'], tabs[sn][DD]['counts'], tabs[sn][DD]['main'])})


def test_plan_stats_mirror_and_pair():
    res = {'o0': {'rows': [{'result': [1.0, 0.0]}, {'result': [1.0, 0.0]}]},
           'o1': {'rows': [{'result': [0.0, 1.0]}, {'result': [1.0, 0.0]}]}}
    s = SB.plan_stats(res, [{'name': 'm', 'kind': 'mirror', 'jobs': ['o0', 'o1']}])['m']
    assert s['score'] == 0.75 and s['pairs'] == 2 and s['kind'] == 'mirror'
    res = {'p': {'rows': [{'result': [1.0, 0.0]}, {'result': [1.0, 0.0]}]},
           'n': {'rows': [{'result': [0.0, 1.0]}, {'result': [1.0, 0.0]}]}}
    s = SB.plan_stats(res, [{'name': 'x', 'kind': 'pair', 'deck': DEL, 'pairs': [('p', 'n', 0)]}])['x']
    assert s['plan'] == 1.0 and s['no_plan'] == 0.5 and s['diff'] == 0.5 and s['matches'] == 2 and s['se'] == 0.5
    assert SB.plan_stats({}, [{'name': 'x', 'kind': 'pair', 'deck': DEL, 'pairs': [('p', 'n', 0)]}]) == {}


# ------------------------------------------------------------------ the report and the options
def test_section_markdown():
    h = {'alerts': ['spec:a|a: x'], 'specialists': {
        'spec:a|a': {'table': {'version': 'v0048', 'versions': 3, 'decks': {'a': {
            'state': 'all main', 'streak': 3, 'swaps': {'min': 0, 'mean': 0.0, 'max': 0}, 'in': {}, 'out': {}}}},
            'window': [33, 64], 'play': {'source': 'game database', 'iterations': 32, 'of': 32, 'rows': [
                {'deck': 'a', 'game_no': 2, 'mode': 'table', 'seats': 10, 'score': 0.6, 'cards_in': 0.0}]},
            'card_use': {'decks': {'a': {'Pyroblast': {'all': {'casts': 10, 'wrong_rate': 0.25, 'wrong_se': 0.1,
                                                               'unknown': 0, 'counter_share': 0.2, 'level': 'between pb3 and pb2'}}}}}},
        'spec:a|b': {'table': {'note': 'no matchup version yet'}, 'play': {'note': 'not ingested\n(x | y)'},
                     'card_use': {'note': 'not ingested (no database spec-a-b)'}}},
        'plan_vs_no_plan': {'spec:a|a a': {'kind': 'mirror', 'what': 'w', 'score': 0.557, 'se': 0.025, 'elo': 40.0,
                                           'elo_se': 18.0, 'matches': 200}}}
    md = '\n'.join(SB.md_lines(h))
    assert '## Sideboarding health' in md and '**ALERT** spec:a|a: x' in md
    assert '| spec:a\\|a | a | v0048 | all main | 3 of 3 | 0 / 0.0 / 0 |' in md
    assert '| 2 | table | 10 | 60.0% | 0.00 |' in md
    assert '25.0% +- 10.0 | 0 | 20.0% | between pb3 and pb2 | pb2 32% / pb3 15% (counter 12% / 20%) |' in md
    assert 'not ingested (x \\| y)' in md and 'not ingested (no database spec-a-b)' in md
    assert '55.7% +- 2.5 (+40.0 +- 18.0 Elo) | 200 |' in md
    rep = {'name': 't', 'round': 0, 'written': 'now', 'phase': 'judge@0', 'sb_health': h}
    assert '## Sideboarding health' in J.report_md(rep)
    assert '## Sideboarding health' not in J.report_md(dict(rep, sb_health=None))


def test_recipe_options():
    assert SB.options({}) == SB.DEFAULT and SB.options({'sb_health': False}) is None
    assert SB.options({'sb_health': {'iters': 8}})['iters'] == 8
    assert SB.check_options(True) is None and SB.check_options({'misaim_alert': 0.2}) is None
    assert 'unknown keys' in SB.check_options({'bogus': 1})
    assert 'misaim_alert' in SB.check_options({'misaim_alert': 2})
    assert 'iters' in SB.check_options({'iters': 0})


def _stew(tmp_path, monkeypatch, **recipe):
    src = make_source(tmp_path)
    recipe.setdefault('stall', None)
    cfgp = make_cfg(tmp_path, src, decks=[DEL, DD], **recipe)
    ops = FakeOps(tmp_path)
    ops.farm.update({'src': 'active', 'src-eval': 'active'})
    monkeypatch.setattr(Stew, 'judge_play', lambda self: {'seat': {}, 'sims': 4})
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=ops, log=lambda m: None)
    s.poll_s = 0.0
    return s


def test_config_refuses_bad_options(tmp_path):
    src = make_source(tmp_path)
    with pytest.raises(C.ConfigError, match='sb_health'):
        C.load(make_cfg(tmp_path, src, sb_health={'bogus': 1}))
    with pytest.raises(C.ConfigError, match='plan_matches'):
        C.load(make_cfg(tmp_path, src, judge={'matches': 8, 'gen_matches': 4, 'plan_arm': True, 'plan_matches': 6}))
    c = C.load(make_cfg(tmp_path, src))
    assert c['recipe']['sb_health'] is True and c['recipe']['judge']['plan_arm'] is False


def test_the_report_has_the_section_and_the_plan_arm(tmp_path, monkeypatch):
    monkeypatch.setenv('MTGZ_GAMEDB', str(tmp_path / 'nodb'))
    s = _stew(tmp_path, monkeypatch, judge={'matches': 8, 'gen_matches': 4, 'device': 'cpu', 'plan_arm': True,
                                            'plan_matches': 8})
    write_mirror_version(s.st.abs(s.st.net(C.spec_name(DEL, DEL))['run']), 2, swap=('Force of Will', 'Pyroblast'))
    write_pair_version(s.st.abs(s.st.net(C.spec_name(DEL, DD))['run']), 2, {DEL: ('Wasteland', 'Force of Negation')})
    assert s.run() == 'held'
    played = [x for x in s.ops.judge_played if x.startswith('P.')]
    assert 'P.izzet-izzet.o0' in played and 'P.izzet-doomsday.s1.c1.plan' in played
    assert 'P.doomsday-izzet.s0.c0.none' in played                    # (DD's table is all main: still a table, an arm)
    rep = jload(s.p('reports', 'gen_000.json'))
    h = rep['sb_health']
    assert set(h['specialists']) == {C.spec_name(DEL, DEL), C.spec_name(DEL, DD)}
    assert h['specialists'][C.spec_name(DEL, DEL)]['table']['decks'][DEL]['state'] == 'boards'
    assert h['specialists'][C.spec_name(DEL, DD)]['card_use']['note'].startswith('not ingested')
    assert set(h['plan_vs_no_plan']) == {f'{C.spec_name(DEL, DEL)} {DEL}', f'{C.spec_name(DEL, DD)} {DEL}',
                                         f'{C.spec_name(DEL, DD)} {DD}'}
    md = open(s.p('reports', 'gen_000.md'), encoding='utf-8').read()
    assert '## Sideboarding health' in md and 'Plan vs no plan' in md
    assert 'sb health spec:' in s.status()


def test_sb_health_off_leaves_the_report_as_before(tmp_path, monkeypatch):
    s = _stew(tmp_path, monkeypatch, sb_health=False)
    assert s.run() == 'held'
    rep = jload(s.p('reports', 'gen_000.json'))
    assert 'sb_health' not in rep and not any(x.startswith('P.') for x in s.ops.judge_played)
    assert '## Sideboarding health' not in open(s.p('reports', 'gen_000.md'), encoding='utf-8').read()
    assert 'sb health' not in s.status()
