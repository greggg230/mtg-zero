"""The stew's distillation only when wanted (docs/ENGINE.md 257): `recipe.distill_every`, the on-demand request
(`tools/stew.py distil`, DIR/DISTIL), the generations without stew versions and everything that reads "the stew of
generation g" across them. On test_stew.py's fakes: no process, no network, no farm."""
import os
import shutil
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tests'))
import mtgz.az  # noqa: E402,F401
from mtgz.stew import config as C  # noqa: E402
from mtgz.stew.orchestrator import Refused, Stew  # noqa: E402
from mtgz.stew.state import jload, jsave  # noqa: E402

from test_stew import DD, DEL, META3, PAIR_ROWS, SNS, FakeOps, make_cfg, make_source  # noqa: E402

FIVE = list(C.PHASES)
SKIP = ['train', C.SKIP_PHASE]
WIN = {'window': 1, 'matches': 4, 'max_matches': 12, 'z': 1.645}
MIR, SDD, SSNS = C.spec_name(DEL, DEL), C.spec_name(DEL, DD), C.spec_name(DEL, SNS)


def mk(tmp_path, monkeypatch, decks=META3, outcomes=None, **recipe):
    """A stew on the fakes with no hold point and no stall rule unless the recipe says so; `outcomes` as
    test_stew.stall_stew's: {'<alias>-<alias>': per-pair scores of the stall tests' matches}."""
    src = make_source(tmp_path)
    recipe.setdefault('hold_after', [])
    recipe.setdefault('stall', None)
    cfgp = make_cfg(tmp_path, src, decks=decks, **recipe)
    ops = FakeOps(tmp_path)
    ops.farm.update({'src': 'active', 'src-eval': 'active'})
    monkeypatch.setattr(Stew, 'judge_play', lambda self: {'seat': {}, 'sims': 4})
    plain = ops.judge_jobs

    def judge_jobs(jobs, jdir, procs, stop=None):
        out = plain(jobs, jdir, procs, stop)
        for j in jobs:
            if j['id'].startswith('W.'):
                _, pair, k = j['id'].split('.')
                cyc = (outcomes or {}).get(pair, [0.5])
                npair = j['games'] // 2
                rows = []
                for i in range(npair):
                    first, second = PAIR_ROWS[cyc[(int(k) * npair + i) % len(cyc)]]
                    rows += [{'gid': 2 * i, 'result': first, 'over': True, 'games': 2},
                             {'gid': 2 * i + 1, 'result': second, 'over': True, 'games': 2}]
                r = dict(out[j['id']], rows=rows)
                jsave(os.path.join(jdir, j['id'] + '.json'), r)
                out[j['id']] = r
        return out

    ops.judge_jobs = judge_jobs
    s = Stew.init(os.path.join(str(tmp_path), 'stew'), cfgp, ops=ops, log=lambda m: None)
    s.poll_s = 0.0
    return s


def reload(s):
    """The stew read again from its directory (a new process's view)."""
    n = Stew(s.dir, ops=s.ops, log=lambda m: None)
    n.poll_s = 0.0
    return n


def tags(s, n=None):
    t = [h['tag'] for h in s.st.d['history']]
    return t if n is None else t[-n:]


def gens(s, name):
    return [v['gen'] for v in s.st.net(name)['versions']]


def cycle(g, phases):
    return [f'{p}@{g}' for p in phases]


# ------------------------------------------------------------------ the setting
def test_the_default_is_every_generation_and_leaves_no_trace(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch)
    assert s.cfg['recipe']['distill_every'] == 1 and s.cfg['recipe']['judge']['between'] == []
    assert s.run(max_phases=11) == 'done'
    assert tags(s) == cycle(0, FIVE) + cycle(1, FIVE) + ['train@2']
    assert 'distil_log' not in s.st.d and not os.path.exists(s.p('DISTIL.old'))
    for name in ['main'] + [C.deck_net(d) for d in META3]:
        assert gens(s, name) == [0, 1, 2]
    assert s.st.version('main', 2)['parents'] == ['main@1']
    assert jload(s.sources_path('main', 1))['student']['init'] == s.st.ckpt('main', 1)
    assert 'stews' not in jload(s.p('reports', 'gen_001.json'))
    assert '## The stews' not in open(s.p('reports', 'gen_001.md'), encoding='utf-8').read()
    assert 'then every generation' in s.status() and 'stew versions' not in s.status()
    # a request changes nothing where every generation distils: taken at the next seam, the cycle as before
    s.request_distil()
    assert s.run(max_phases=5) == 'done'
    assert tags(s, 5) == cycle(2, FIVE)[1:] + ['train@3'] and gens(s, 'main') == [0, 1, 2, 3]
    assert not os.path.exists(s.p('DISTIL')) and [e['round'] for e in s.st.d['distil_log']] == [3]
    assert s.st.tag() == 'distil_decks@3'


def test_the_distill_every_config():
    base = {'name': 'x', 'decks': [{'name': d, 'share': 1} for d in META3], 'candidates': [DEL],
            'source': {'run': 'r'}}
    norm = lambda **r: C.normalize(dict(base, recipe=r), check_lists=False)['recipe']      # noqa: E731
    assert norm()['distill_every'] == 1
    assert norm(distill_every=3)['distill_every'] == 3
    assert norm(distill_every='never')['distill_every'] == 0 and norm(distill_every=0)['distill_every'] == 0
    assert norm(distill_every=None)['distill_every'] == 0
    assert norm(judge={'between': ['gen', 'matchup']})['judge']['between'] == ['gen', 'matchup']
    assert norm(hold_after=['judge_specialists@2'])['hold_after'] == ['judge_specialists@2']
    for bad in ({'distill_every': -1}, {'distill_every': True}, {'distill_every': 1.5}, {'distill_every': 'often'},
                {'judge': {'between': ['stall']}}, {'judge': {'between': 'gen'}}, {'judge': {'between': ['gen', 'gen']}},
                {'hold_after': ['judge_stews@0']}):
        with pytest.raises(C.ConfigError):
            norm(**bad)


# ------------------------------------------------------------------ every N-th generation
def test_every_third_generation_distils_from_the_newest_stews(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch, distill_every=3)
    assert 'after generation 2\'s turns (main v3' in s.next_distil()
    assert 'judge_specialists@0 (no distillation: 1 of 3 turns since main v0' in s.plan()
    assert s.run(max_phases=18) == 'done'
    st = s.st
    assert tags(s) == (cycle(0, SKIP) + cycle(1, SKIP) + cycle(2, FIVE) + cycle(3, SKIP) + cycle(4, SKIP)
                       + cycle(5, FIVE))
    assert st.tag() == 'train@6'
    # the specialists trained a turn every generation, each from its own last checkpoint
    for sn in (MIR, SDD, SSNS):
        assert gens(s, sn) == list(range(7)) and [v['iter'] for v in st.net(sn)['versions']] == list(range(0, 13, 2))
    assert [c[2] for c in s.ops.calls if c[0] == 'launch' and c[1] == C.spec_dir(DEL, DD)] == [2, 4, 6, 8, 10, 12]
    # the stews: versions 0, 3 and 6 only - numbered as the generation that made them
    for name in ['main'] + [C.deck_net(d) for d in META3]:
        assert gens(s, name) == [0, 3, 6], name
        assert st.version(name, 3)['parents'] == [f'{name}@0'] and st.version(name, 6)['parents'] == [f'{name}@3']
    assert st.version('main', 3)['path'] == 'nets/main/gen_003.pt'
    # generation 2's deck stew: warm start = its newest version (0), teachers = the specialists' version 3, rows = the
    # newest iterations of generation 2's turn
    dd = jload(s.sources_path(C.deck_net(DD), 2))
    assert dd['student']['init'] == st.ckpt(C.deck_net(DD), 0)
    assert dd['sources'][0]['teacher'] == st.ckpt(SDD, 3)
    assert [os.path.basename(d) for d in dd['sources'][0]['data']] == ['iter_0005', 'iter_0006']
    assert 'rows/gen_003/' in dd['sources'][0]['data'][0]
    # its main stew: warm start and retention teacher = the newest main (0), the cells' teachers the deck stews' v3
    main = {x['name']: x for x in jload(s.sources_path('main', 2))['sources']}
    assert jload(s.sources_path('main', 2))['student']['init'] == st.ckpt('main', 0)
    assert main['retention']['teacher'] == st.ckpt('main', 0)
    assert main[f'main:{DEL}|{DD}@{DD}']['teacher'] == st.ckpt(C.deck_net(DD), 3)
    # generation 5's: from version 3
    assert jload(s.sources_path('main', 5))['student']['init'] == st.ckpt('main', 3)
    assert {x['name']: x for x in jload(s.sources_path('main', 5))['sources']}['retention']['teacher'] == st.ckpt('main', 3)
    assert jload(s.sources_path(C.deck_net(DEL), 5))['student']['init'] == st.ckpt(C.deck_net(DEL), 3)
    assert not os.path.exists(s.sources_path('main', 0)) and not os.path.isdir(s.p('rows', 'gen_001'))
    # judging: nothing in the generations without stews (no stall rule, between empty), the full set in the others;
    # gen over gen stays one turn (v3 against v2)
    assert not os.path.isdir(s.judge_dir(0)) and not os.path.isdir(s.judge_dir(4))
    assert st.d['history'][1]['result']['jobs'] == 0
    j2 = jload(os.path.join(s.judge_dir(2), 'G.izzet-doomsday.job.json')) or {}
    ids = {f.split('.')[0] for f in os.listdir(s.judge_dir(2))}
    assert ids == {'G', 'B', 'A', 'M'}
    rep = jload(s.p('reports', 'gen_002.json'))
    assert set(rep['judge']['gen_over_gen']) == {MIR, SDD, SSNS} and 'gap_M_A' in rep['judge']['cells'][f'{DEL}|{DD}']
    assert j2 == {} or j2['a'] == st.ckpt(SDD, 3)
    # every generation has its report; each says whether the stews were distilled and which versions exist
    r0, r2 = jload(s.p('reports', 'gen_000.json')), rep
    assert r0['stews']['this_generation'].startswith('not distilled: 1 of 3 turns since main v0')
    assert r0['training'][SDD]['to'] == 2 and r0['distill'] == {} and r0['judge'] == {}
    assert r2['stews']['this_generation'].startswith('distilled: 3 turns since main v0')
    assert r2['stews']['versions']['main'] == [0, 3] and r2['stews']['distill_every'] == 3
    md = open(s.p('reports', 'gen_004.md'), encoding='utf-8').read()
    assert '## The stews' in md and 'not distilled' in md and '| main | v0 v3 |' in md
    assert [(e['round'], e['due']) for e in st.d['distil_log']] == [(0, False), (1, False), (2, True), (3, False),
                                                                    (4, False), (5, True)]
    txt = s.status()
    assert 'stew versions: main v0 v3 v6; the deck stews the same' in txt
    assert "distillation: after generation 8's turns (main v9; every 3 turns, newest main v6)" in txt
    assert 'judge_specialists@6' in s.plan()


def test_between_adds_the_specialist_only_comparisons(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch, distill_every=0,
           judge={'matches': 8, 'gen_matches': 4, 'device': 'cpu', 'between': ['gen', 'matchup']})
    assert s.run(max_phases=1) == 'done'
    assert 'jobs' in s.plan() and 'A.' not in s.plan()
    assert s.run(max_phases=1) == 'done'
    ids = {f[:-5] for f in os.listdir(s.judge_dir(0)) if f.endswith('.json') and not f.endswith('.job.json')}
    assert {i.split('.')[0] for i in ids} == {'G', 'B'}
    rep = jload(s.p('reports', 'gen_000.json'))
    assert set(rep['judge']['gen_over_gen']) == {MIR, SDD, SSNS}
    cell = rep['judge']['cells'][f'{DEL}|{DD}']
    assert 'B' in cell and 'A' not in cell and 'M' not in cell
    assert 'specialists' in rep['judge']['field'][DEL] and 'deck stews' not in rep['judge']['field'][DEL]
    assert gens(s, 'main') == [0] and s.st.tag() == 'train@1'


# ------------------------------------------------------------------ on demand
def test_a_request_is_honoured_once_at_the_seam_after_the_turn_under_way(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch, distill_every='never')
    assert 'only when asked for' in s.next_distil()
    assert s.run(max_phases=2) == 'done' and tags(s) == cycle(0, SKIP)
    # asked for while generation 1's first turn is being trained (the fake turn writes the request as it starts)
    launch = s.ops.launch

    def asking(run, args, log):
        if not os.path.exists(s.p('DISTIL')) and not os.path.exists(s.p('DISTIL.old')):
            reload(s).request_distil()
        return launch(run, args, log)

    s.ops.launch = asking
    assert s.run(max_phases=9) == 'done'
    assert tags(s, 9) == cycle(1, FIVE) + cycle(2, SKIP) + cycle(3, SKIP)
    assert gens(s, 'main') == [0, 2] and gens(s, C.deck_net(SNS)) == [0, 2]
    assert not os.path.exists(s.p('DISTIL')) and jload(s.p('DISTIL.old'))['by'] == 'tools/stew.py distil'
    log = s.st.d['distil_log']
    assert [(e['round'], e['due']) for e in log] == [(0, False), (1, True), (2, False), (3, False)]
    assert log[1]['request']['by'] == 'tools/stew.py distil' and log[1]['why'].startswith('asked for')
    # the distillation took every specialist's newest version and the rows of its newest turn
    de = jload(s.sources_path(C.deck_net(DEL), 1))
    assert [x['teacher'] for x in de['sources']] == [s.st.ckpt(C.spec_name(DEL, y), 2) for y in META3]
    assert [os.path.basename(d) for d in de['sources'][0]['data']] == ['iter_0003', 'iter_0004']
    assert de['student']['init'] == s.st.ckpt(C.deck_net(DEL), 0)
    # a file left behind by a kill between the state's save and the file's move is not a second request
    shutil.copyfile(s.p('DISTIL.old'), s.p('DISTIL'))
    assert s.distil_request(peek=True) is None and 'ASKED' not in s.status()
    assert s.run(max_phases=2) == 'done'
    assert tags(s, 2) == cycle(4, SKIP) and gens(s, 'main') == [0, 2] and not os.path.exists(s.p('DISTIL'))
    # cancelled: nothing happens
    s.request_distil()
    assert 'ASKED FOR' in s.status()
    s.request_distil(cancel=True)
    assert s.run(max_phases=2) == 'done' and tags(s, 2) == cycle(5, SKIP)


def test_a_request_at_a_generations_start_distils_the_generation_before(tmp_path, monkeypatch):
    # a stopped stew, generations 0 and 1 trained without distillation; the stall rule on (window 1)
    s = mk(tmp_path, monkeypatch, distill_every=0, stall=WIN, outcomes={'izzet-doomsday': [1], 'izzet-sneak': [1],
                                                                       'izzet-izzet': [1]})
    assert s.run(max_phases=4) == 'done' and s.st.tag() == 'train@2'
    n_launch = len([c for c in s.ops.calls if c[0] == 'launch'])
    n_log = len(s.st.net(SDD)['stall_log'])
    played = list(s.ops.judge_played)
    s = reload(s)
    when = s.request_distil()
    assert 'generation 1 is distilled when the stew runs next' in when and 'main v2' in when
    assert 'first (the distillation asked for): distil_decks@1' in s.plan()
    assert s.run(max_phases=4) == 'done'
    st = s.st
    assert tags(s, 4) == cycle(1, FIVE)[1:] and st.tag() == 'train@2'
    assert len([c for c in s.ops.calls if c[0] == 'launch']) == n_launch          # no turn trained again
    assert gens(s, 'main') == [0, 2] and st.version('main', 2)['parents'] == ['main@0']
    assert st.d['distil_log'][-1]['rewound'] is True and st.d['distil_log'][-1]['round'] == 1
    assert jload(s.sources_path(C.deck_net(DD), 1))['sources'][0]['teacher'] == st.ckpt(SDD, 2)
    # generation 1's stall tests were played in judge_specialists@1: not replayed, one log entry each
    new = s.ops.judge_played[len(played):]
    assert not [i for i in new if i.startswith('W.')] and {i.split('.')[0] for i in new} == {'G', 'B', 'A', 'M'}
    assert len(st.net(SDD)['stall_log']) == n_log == 2 and st.net(SDD)['stall']['versions'] == 'v2 vs v1'
    rep = jload(s.p('reports', 'gen_001.json'))
    assert rep['stews']['this_generation'].startswith('distilled: asked for') and SDD in rep['judge']['window']
    # the request is spent: generation 2 trains and goes without
    assert s.run(max_phases=2) == 'done' and tags(s, 2) == cycle(2, SKIP)


# ------------------------------------------------------------------ the stall rule and gen over gen across the gaps
def test_the_stall_rule_keeps_its_cadence_without_distillation(tmp_path, monkeypatch):
    # window 2 (the default's): a specialist's first test is after its second turn - in judge_specialists@1 here
    s = mk(tmp_path, monkeypatch, distill_every=0, stall=dict(WIN, window=2), outcomes={'izzet-doomsday': [1]})
    assert s.run(max_phases=4) == 'done'
    assert s.st.d['history'][1]['result']['jobs'] == 0
    assert not os.path.isdir(s.judge_dir(0))
    assert s.st.net(SDD)['stall']['versions'] == 'v2 vs v0' and s.st.net(SDD)['stall']['stalled'] is False
    assert s.st.net(MIR)['stall']['stalled'] is True
    assert {f.split('.')[0] for f in os.listdir(s.judge_dir(1))} == {'W'}


def test_a_stalled_specialist_sits_out_and_is_distilled_from_its_last_turn(tmp_path, monkeypatch):
    # the mirror's turns gain nothing (stalled after generation 0), the others' win every pair
    s = mk(tmp_path, monkeypatch, distill_every=3, stall=WIN,
           outcomes={'izzet-izzet': [0.5], 'izzet-doomsday': [1], 'izzet-sneak': [1]})
    assert s.run(max_phases=2) == 'done'
    st = s.st
    assert st.net(MIR)['stall']['stalled'] is True and st.net(MIR)['stall']['versions'] == 'v1 vs v0'
    assert st.net(SDD)['stall']['stalled'] is False
    rep = jload(s.p('reports', 'gen_000.json'))
    assert rep['stall'][MIR]['trains'] is False and rep['judge']['window'][SDD]['matches'] == 4
    assert s.run(max_phases=7) == 'done' and st.tag() == 'train@3'
    assert tags(s, 7) == cycle(1, SKIP) + cycle(2, FIVE)
    assert st.version(MIR, 2)['skipped'].startswith('stalled') and st.version(MIR, 3)['path'] == st.version(MIR, 1)['path']
    assert [e['versions'] for e in st.net(SDD)['stall_log']] == ['v1 vs v0', 'v2 vs v1', 'v3 vs v2']
    assert [e['round'] for e in st.net(MIR)['stall_log']] == [0]                  # (not re-tested: it did not train)
    # the mirror sat generation 2 out and its last turn (generation 0) was never snapshotted: its rows come from its
    # run now; its teacher is its newest version
    rec = st.d['rows']['2'][C.pair_key(DEL, DEL)]
    assert rec['turn_of'] == 0 and rec['iters'] == [1, 2] and 'reused_from' not in rec
    assert all(os.path.isdir(st.abs(d)) and 'rows/gen_003/' in d for d in rec['dirs'])
    de = jload(s.sources_path(C.deck_net(DEL), 2))
    assert de['sources'][0]['name'] == MIR and de['sources'][0]['teacher'] == st.ckpt(MIR, 3) == st.ckpt(MIR, 1)
    assert [os.path.basename(d) for d in de['sources'][1]['data']] == ['iter_0005', 'iter_0006']
    # generation 2's judging: gen over gen and the stall test of those that trained, arm A of every cell
    jobs = os.listdir(s.judge_dir(2))
    assert not any(f.startswith(('G.izzet-izzet', 'W.izzet-izzet')) for f in jobs)
    assert any(f.startswith('G.izzet-doomsday') for f in jobs) and any(f.startswith('A.izzet-izzet') for f in jobs)
    # three generations on (it still sits out): the snapshot of generation 2 is reused
    assert s.run(max_phases=5) == 'done' and st.tag() == 'distil_decks@5'
    assert s.run(max_phases=1) == 'done'
    assert st.d['rows']['5'][C.pair_key(DEL, DEL)]['reused_from'] == 2
    assert st.version(C.deck_net(DEL), 6)['parents'] == [f'{C.deck_net(DEL)}@3']


# ------------------------------------------------------------------ add-deck
def test_add_deck_takes_the_newest_main_stew_however_old(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch, decks=[DEL, DD], distill_every=0)
    assert s.run(max_phases=6) == 'done' and s.st.tag() == 'train@3'
    assert gens(s, 'main') == [0]
    s.add_deck(SNS)
    assert 'copies of main v0' in s.plan()
    assert s.run(max_phases=1) == 'done'                  # train@3: the deck joins, its specialist trains its first turn
    st = s.st
    dn, sn = C.deck_net(SNS), C.spec_name(DEL, SNS)
    assert gens(s, dn) == [0] and st.version(dn, 0)['parents'] == ['main@0']
    assert open(st.ckpt(dn, 0), 'rb').read() == open(st.ckpt('main', 0), 'rb').read()
    assert st.net(sn)['joined'] == 3 and st.version(sn, 3)['iter'] == 0 and st.version(sn, 4)['iter'] == 2
    assert st.version(sn, 3)['seed'] == st.ckpt('main', 0).replace('\\', '/')
    assert 'stew versions: main v0 (4 turn(s) behind' in s.status()
    # asked for now (the stew is in judge_specialists@3): generation 3 is distilled right after it, the new deck's
    # stew from its copy of main v0 and its specialist's first turn
    assert 'generation 3 is distilled right after this phase' in s.request_distil()
    assert s.run(max_phases=5) == 'done'
    assert tags(s, 5) == [f'{C.SKIP_PHASE}@3'] + cycle(3, FIVE)[1:] and st.tag() == 'train@4'
    assert gens(s, dn) == [0, 4] and st.version(dn, 4)['parents'] == [f'{dn}@0']
    doc = jload(s.sources_path(dn, 3))
    assert doc['student']['init'] == st.ckpt(dn, 0) and doc['sources'][0]['teacher'] == st.ckpt(sn, 4)
    assert gens(s, 'main') == [0, 4] and gens(s, C.deck_net(DD)) == [0, 4]
    assert any(f.startswith('A.sneak-izzet') for f in os.listdir(s.judge_dir(3)))
    assert f'main:{DEL}|{SNS}@{SNS}' in [x['name'] for x in jload(s.sources_path('main', 3))['sources']]


def test_a_request_and_a_pending_deck_at_the_same_start(tmp_path, monkeypatch):
    # the generation before is distilled first (its field), then the deck joins - on the main stew just made
    s = mk(tmp_path, monkeypatch, decks=[DEL, DD], distill_every=0)
    assert s.run(max_phases=4) == 'done' and s.st.tag() == 'train@2'
    s.add_deck(SNS)
    s.request_distil()
    assert s.run(max_phases=5) == 'done'
    st = s.st
    assert tags(s, 5) == cycle(1, FIVE)[1:] + ['train@2']
    assert SNS in C.deck_names(st.decks()) and st.net(C.spec_name(DEL, SNS))['joined'] == 2
    assert gens(s, C.deck_net(SNS)) == [2] and st.version(C.deck_net(SNS), 2)['parents'] == ['main@2']
    assert [x['name'] for x in jload(s.sources_path(C.deck_net(DEL), 1))['sources']] == [MIR, SDD]


# ------------------------------------------------------------------ PAUSE, STOP and crashes in the new states
def test_a_hold_after_judge_specialists_and_the_resume(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch, distill_every=2, hold_after=['judge_specialists@0'], stall=WIN,
           outcomes={'izzet-izzet': [1], 'izzet-doomsday': [1], 'izzet-sneak': [1]})
    assert s.run() == 'held'
    assert s.st.tag() == 'train@1' and jload(s.p('PAUSE'))['tag'] == 'judge_specialists@0'
    with pytest.raises(Refused, match='paused'):
        reload(s).run()
    os.remove(s.p('PAUSE'))
    s = reload(s)
    assert s.run(max_phases=5) == 'done' and tags(s, 5) == cycle(1, FIVE)
    assert gens(s, 'main') == [0, 2]


def test_judge_specialists_interrupted_resumes_without_replaying(tmp_path, monkeypatch):
    from mtgz.stew.control import Interrupted
    s = mk(tmp_path, monkeypatch, distill_every=0, stall=WIN, outcomes={'izzet-sneak': [1, 0.5]})
    assert s.run(max_phases=1) == 'done' and s.st.tag() == 'judge_specialists@0'
    judge = s.ops.judge_jobs
    n = {'calls': 0}

    def stopping(jobs, jdir, procs, stop=None):
        n['calls'] += 1
        if n['calls'] == 2:                       # (the extension of Delver-Sneak's test: stopped before it plays)
            jsave(s.p('PAUSE'), {'by': 'a test'})
            raise Interrupted('judge')
        return judge(jobs, jdir, procs, stop)

    s.ops.judge_jobs = stopping
    assert s.run() == 'paused'
    assert s.st.tag() == 'judge_specialists@0' and s.st.net(SSNS).get('stall') is None
    assert s.st.d['stall_parts']['0'][SSNS] == 2
    played = list(s.ops.judge_played)
    with pytest.raises(Refused, match='paused'):
        s.run()
    os.remove(s.p('PAUSE'))
    s = reload(s)
    assert s.run(max_phases=1) == 'done' and s.st.tag() == 'train@1'
    assert s.ops.judge_played[len(played):] == ['W.izzet-sneak.1']
    assert s.st.net(SSNS)['stall']['matches'] == 8 and s.st.net(SSNS)['stall']['stalled'] is False


def test_pause_and_a_crash_around_a_distillation_asked_for(tmp_path, monkeypatch):
    s = mk(tmp_path, monkeypatch, distill_every=0)
    assert s.run(max_phases=4) == 'done' and s.st.tag() == 'train@2'
    # paused with the request waiting: nothing moves until PAUSE goes, then the request is taken
    s.request_distil()
    jsave(s.p('PAUSE'), {'by': 'a test'})
    with pytest.raises(Refused, match='paused'):
        s.run()
    assert s.st.tag() == 'train@2' and os.path.exists(s.p('DISTIL'))
    os.remove(s.p('PAUSE'))
    # the first distillation crashes: the stew stands in distil_decks@1 with the request spent
    s = reload(s)
    s.ops.distill_fail = 1
    with pytest.raises(RuntimeError):
        s.run()
    assert s.st.tag() == 'distil_decks@1' and not os.path.exists(s.p('DISTIL')) and os.path.exists(s.p('DISTIL.old'))
    assert 'under way: generation 1 (distil_decks; main v2)' in s.status()
    # paused again inside the distillation asked for; resumed: it goes on where it stopped, once
    jsave(s.p('PAUSE'), {'by': 'a test'})
    with pytest.raises(Refused, match='paused'):
        reload(s).run()
    os.remove(s.p('PAUSE'))
    s = reload(s)
    assert s.run(max_phases=6) == 'done'
    assert tags(s, 6) == cycle(1, FIVE)[1:] + cycle(2, SKIP)
    assert gens(s, 'main') == [0, 2] and len([e for e in s.st.d['distil_log'] if e['due']]) == 1
    # asked for during a turn, then STOP in that turn: resumed, the turn's generation is the one distilled
    s = reload(s)
    seen = {'n': 0}

    def sup(o, run):
        seen['n'] += 1
        if seen['n'] == 2:
            reload(s).request_distil()
        if seen['n'] == 3:
            jsave(s.p('STOP'), {})
    s.ops.on_supervisor = sup
    assert s.run() == 'stopped' and s.st.tag() == 'train@3' and s.st.d['turn'] is not None
    assert os.path.exists(s.p('DISTIL'))             # (a turn of generation 3 has begun: no going back to 2)
    s.ops.on_supervisor = None
    s = reload(s)
    assert 'after generation 3\'s turns (main v4)' in s.next_distil()
    assert s.run(max_phases=2) == 'done' and tags(s, 2) == ['train@3', 'distil_decks@3']
    assert not os.path.exists(s.p('DISTIL'))
