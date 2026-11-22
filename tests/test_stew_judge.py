"""The stew's judging (mtgz.stew.judge, docs/ENGINE.md 222.3): the jobs of a cell (the deck follows the net, both
orientations x both choosers, common deals across arms), the units and the statistics on synthetic results, the
matchup table. No match is played here (the CPU smoke plays them: docs/ENGINE.md 222.3)."""
import json
import math

import pytest

import mtgz.az  # noqa: F401
from mtgz.stew import config as C
from mtgz.stew import judge as J

DEL, DD, SNS = 'izzet_delver_8016541', 'doomsday_8027957', 'sneak_show_8014220'
AL = {DEL: 'izzet', DD: 'doomsday', SNS: 'sneak'}
JC = {'matches': 8, 'gen_matches': 4, 'seed': 0, 'gen_over_gen': True, 'main_arms': True}


def spec_fn(p, q, c):
    return json.dumps({'decks': [p, q], 'chooser': c})


class FakeState:
    """The few State methods plan_jobs reads."""

    def __init__(self, decks, cands, gens=(0, 1)):
        self._d = [{'name': d, 'share': 1, 'alias': AL[d]} for d in decks]
        self._c = list(cands)
        self.gens = gens

    def decks(self):
        return self._d

    def candidates(self):
        return self._c

    def ckpt(self, name, gen):
        return f'{name}@{gen}.pt'

    def version(self, name, gen):
        return {'path': f'{name}@{gen}.pt'} if gen in self.gens else None

    def abs(self, p):
        return p


def test_the_deck_follows_the_net_in_every_cell_job():
    st = FakeState([DEL, DD, SNS], [DEL])
    jobs = J.plan_jobs('judge_deck_gap', st, 0, JC, spec_fn, {})
    by = {j['id']: j for j in jobs}
    kinds = {}
    for j in jobs:
        kinds[j['id'][0]] = kinds.get(j['id'][0], 0) + 1
    assert kinds == {'G': 3, 'B': 8, 'A': 20}          # 3 specialists; 2 non-mirror pairs x 4; 5 cells x 4
    for j in jobs:
        if j['id'][0] != 'A':
            continue
        xs = int(j['layout'][1])
        pilot = j['a'].split('@')[0].split(':', 1)[1]  # deck:<X>@1.pt
        assert j['decks'][xs] == pilot                 # the measured net sits in its deck's seat
        assert j['b'] == C.spec_name(*sorted(j['decks'], key=[DEL, DD, SNS].index)) + '@1.pt'
        assert json.loads(j['spec']) == {'decks': j['decks'], 'chooser': j['chooser']}
        assert j['games'] == 2
    # arms A and B of a cell share their deals: an orientation's seed is the pair's
    a = by['A.izzet-doomsday.s0.c1']
    b = by['B.izzet-doomsday.c1']
    assert a['seed'] == b['seed'] and a['spec'] == b['spec']
    a2 = by['A.izzet-doomsday.s1.c1']                 # deck stew Doomsday in seat 1 of pair_spec(Delver, Doomsday)
    assert a2['seed'] == b['seed'] and a2['a'] == C.deck_net(DD) + '@1.pt'
    # a mirror's two orientations are two seats with seeds of their own (not one deal twice)
    m0, m1 = by['A.izzet-izzet.s0.c0'], by['A.izzet-izzet.s1.c0']
    assert m0['seed'] != m1['seed'] and m0['layout'] == 'a0' and m1['layout'] == 'a1'
    assert 'B.izzet-izzet.c0' not in by                # the mirror's baseline is 0.5 exactly
    # gen over gen: the new version against the old, paired, both seats
    g = by['G.izzet-doomsday']
    assert g['a'] == C.spec_name(DEL, DD) + '@1.pt' and g['b'] == C.spec_name(DEL, DD) + '@0.pt'
    assert g['layout'] == 'paired' and g['games'] == 4 and json.loads(g['spec'])['chooser'] is None
    # the seeds stay the same every generation (common deals across generations too)
    st1 = FakeState([DEL, DD, SNS], [DEL], gens=(1, 2))
    jobs1 = {j['id']: j for j in J.plan_jobs('judge_deck_gap', st1, 1, JC, spec_fn, {})}
    assert jobs1['A.izzet-doomsday.s0.c1']['seed'] == a['seed']
    # the main stew's arm: same jobs with the main stew as the pilot
    mj = {j['id']: j for j in J.plan_jobs('judge', st, 0, JC, spec_fn, {})}
    assert set(mj) == {i.replace('A.', 'M.') for i in by if i.startswith('A.')}
    assert mj['M.izzet-doomsday.s0.c1']['seed'] == a['seed'] and mj['M.izzet-doomsday.s0.c1']['a'] == 'main@1.pt'


def results_for(jobs, fn):
    """Synthetic results: fn(job, gid) -> [seat 0's result, seat 1's]."""
    out = {}
    for j in jobs:
        out[j['id']] = {'id': j['id'], 'rows': [{'gid': g, 'result': fn(j, g), 'over': True, 'games': 2}
                                               for g in range(j['games'])], 'seconds': 1.0}
    return out


def test_identical_arms_have_no_gap_and_shifts_are_measured():
    st = FakeState([DEL, DD], [DEL])
    jc = dict(JC, matches=400)
    jobs = J.plan_jobs('judge_deck_gap', st, 0, jc, spec_fn, {}) + J.plan_jobs('judge', st, 0, jc, spec_fn, {})

    def deal(j, g, shift=0.0):
        # the deal decides who wins (common random numbers): X (seat of Delver) wins deals with h < 0.6 + shift
        h = ((j['seed'] * 7919 + g * 104729 + (j['chooser'] or 0) * 31) % 1000) / 1000.0
        xs = j['decks'].index(DEL) if DEL in j['decks'] else 0
        win = h < 0.6 + shift
        r = [0.0, 0.0]
        r[xs] = 1.0 if win else 0.0
        r[1 - xs] = 1.0 - r[xs]
        return r

    res = results_for(jobs, lambda j, g: deal(j, g, 0.0))
    sm = J.summarize(st, 0, res)
    c = sm['cells'][f'{DEL}|{DD}']
    assert c['gap_A_B']['elo'] == 0.0 and c['gap_A_B']['se'] == 0.0      # the same results on the same deals
    assert c['gap_M_A']['elo'] == 0.0
    assert c['B']['p'] == c['A']['p'] and c['B']['matches'] == 400
    # deck stew X wins more on the same deals: a positive gap with a small paired se
    res2 = results_for(jobs, lambda j, g: deal(j, g, 0.1 if j['id'].startswith('A.') else 0.0))
    c2 = J.summarize(st, 0, res2)['cells'][f'{DEL}|{DD}']
    assert c2['gap_A_B']['elo'] > 30 and c2['gap_A_B']['se'] < c2['gap_A_B']['se_unpaired']
    assert c2['gap_M_A']['elo'] < -30                                    # main = B's results, below A's
    # Doomsday's cell reads the same games from the other side
    d = J.summarize(st, 0, res)['cells'][f'{DD}|{DEL}']
    assert d['B']['p'] == pytest.approx(1 - c['B']['p'], abs=1e-3)


def test_matchup_table_and_field_rate():
    rates = {f'{DEL}|{DEL}': {'p': 0.5, 'se': 0.0}, f'{DEL}|{DD}': {'p': 0.7, 'se': 0.02},
             f'{DEL}|{SNS}': {'p': 0.4, 'se': 0.03}}
    sh = {DEL: 0.5, DD: 0.25, SNS: 0.25}
    f = J.field_rate(rates, sh, DEL, [DEL, DD, SNS])
    assert f['p'] == pytest.approx(0.5 * 0.5 + 0.25 * 0.7 + 0.25 * 0.4)
    assert f['se'] == pytest.approx(math.sqrt((0.25 * 0.02) ** 2 + (0.25 * 0.03) ** 2), abs=1e-4)
    assert J.field_rate({f'{DEL}|{DEL}': rates[f'{DEL}|{DEL}']}, sh, DEL, [DEL, DD, SNS]) is None


def test_gen_over_gen_statistics():
    # 4 pairs: A wins both, splits, splits, loses both -> score 0.5
    rows = []
    for k, (r0, r1) in enumerate([(1, 1), (1, 0), (0, 1), (0, 0)]):
        rows.append({'gid': 2 * k, 'result': [float(r0), 1.0 - r0]})       # A in seat 0
        rows.append({'gid': 2 * k + 1, 'result': [1.0 - r1, float(r1)]})   # A in seat 1
    s = J.gen_stats({'rows': rows})
    assert s['score'] == 0.5 and s['elo'] == 0.0 and s['pairs'] == 4 and s['pentanomial'] == [1, 0, 2, 0, 1]
    assert s['se'] == pytest.approx(math.sqrt(1 / 6 / 4), abs=1e-4)


def test_valid_result_needs_the_jobs_hash():
    st = FakeState([DEL, DD], [DEL])
    j = J.plan_jobs('judge_deck_gap', st, 0, JC, spec_fn, {})[0]
    r = {'id': j['id'], 'hash': J.job_hash(j), 'rows': [{'gid': g} for g in range(j['games'])]}
    assert J.valid_result(j, r)
    assert not J.valid_result(dict(j, seed=j['seed'] + 1), r)
    assert not J.valid_result(j, dict(r, rows=r['rows'][:-1]))
    assert J.job_hash(dict(j, label='renamed')) == J.job_hash(j)


def test_report_markdown_renders_a_summary():
    st = FakeState([DEL, DD], [DEL])
    jobs = J.plan_jobs('judge_deck_gap', st, 0, JC, spec_fn, {})
    res = results_for(jobs, lambda j, g: [1.0, 0.0] if g % 2 else [0.0, 1.0])
    rep = {'name': 'x', 'round': 0, 'phase': 'distil_main@0', 'written': 'now', 'held': True,
           'training': {'spec:a|b': {'from': 0, 'to': 32, 'games': 900, 'samples': 1, 'rows_per_match': 1, 'hours': 1}},
           'distill': {'deck:x': {'sources': 2, 'steps': 50, 'per_source': [{'name': 's', 'weight': 0.5, 'selected': 9,
                                                                             'policy_kl': 0.1}]}},
           'judge': J.summarize(st, 0, res), 'notes': ['a note']}
    md = J.report_md(rep)
    assert 'held here' in md and f'| {DEL}\\|{DD} |' in md and 'generation over generation' in md and 'a note' in md
    cells = [ln for ln in md.splitlines() if ln.startswith(f'| {DEL}\\|')]
    assert cells and all(ln.replace('\\|', '').count('|') == 8 for ln in cells)       # 7 columns: no stray bar
