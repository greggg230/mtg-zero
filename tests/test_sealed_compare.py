"""The paired deck comparison (mtgz/sealed/compare.py, docs/ENGINE.md 104): slot layouts, the
games' common random numbers, the statistics and the race, the network pilot's plumbing, the
yardstick's candidates and the CLI. Needs `mtgz._engine` (tools/build_py.sh)."""
import collections
import json
import os

import pytest

from mtgz.sealed import cards as C
from mtgz.sealed import compare as K
from mtgz.sealed import engine as R


@pytest.fixture(scope='module')
def builds():
    """Three builder builds of one arena pool (supported on this base) and the pool."""
    for ps in range(3, 50):
        p = R.pool(ps, collation='arena')
        if all(isinstance(v, int) for v in R.lookup(p['cards']).values()):
            break
    bs = [K.builder_build(p['cards'], s, name=f'b{s}') for s in (1, 2, 3)]
    return p, bs


def one_swap(b, pool_cards, name='swap'):
    """`b` with its first nonland spell replaced by a sideboard card (a one-card difference)."""
    deck = list(b['deck'])
    i = next(j for j, n in enumerate(deck) if 'Land' not in C.cards()[n]['type_line'])
    side = collections.Counter(b['sideboard'])
    new = sorted(n for n in side if n != deck[i])[0]
    side[new] -= 1
    side[deck[i]] += 1
    deck[i] = new
    return dict(b, name=name, deck=deck, sideboard=sorted(side.elements()))


def test_slots_lay_shared_cards_in_shared_slots():
    a = ['Plains'] * 9 + ['Island'] * 8 + [f'x{i}' for i in range(23)]
    b = ['Plains'] * 8 + ['Island'] * 8 + [f'x{i}' for i in range(22)] + ['y0', 'y1']
    la, lb = K.slots([a, b])
    assert sorted(la) == sorted(a) and sorted(lb) == sorted(b)
    assert K.shared_slots(la, lb) == 38                          # every shared copy in one slot
    # the differing cards follow, paired by kind: the land first (x22, y0, y1 are no cards: spells)
    assert [(x, y) for x, y in zip(la, lb) if x != y] == [('Plains', 'y0'), ('x22', 'y1')]
    assert la[38:] == ['Plains', 'x22'] and lb[38:] == ['y0', 'y1']
    # three decks, a 41-card one among them: each layout is its deck, pairwise shared copies kept
    c = a[:-1] + ['z', 'w']
    out = K.slots([a, b, c])
    for d, lay in zip([a, b, c], out):
        assert sorted(lay) == sorted(d) and len(lay) == len(d)
    common = lambda x, y: sum((collections.Counter(x) & collections.Counter(y)).values())  # noqa: E731
    assert K.shared_slots(out[0], out[1]) >= common(a, b) - 2
    assert K.shared_slots(out[0], out[2]) >= common(a, c) - 2
    assert K.slots([a, b]) == K.slots([a, b])                     # deterministic


def test_slots_pair_lands_with_lands():
    # two differences each: a basic for another basic, a spell for another spell - land pairs with land
    names = sorted(n for n, c in C.cards().items() if 'Land' not in c['type_line'])[:30]
    a = ['Forest'] * 16 + names[:24]
    b = ['Forest'] * 15 + ['Mountain'] + names[:23] + [names[24]]
    la, lb = K.slots([a, b])
    diff = {(x, y) for x, y in zip(la, lb) if x != y}
    assert diff == {('Forest', 'Mountain'), (names[23], names[24])}


def test_identical_decks_play_identical_games(builds):
    p, bs = builds
    twin = dict(bs[0], name='twin')
    comp = K.Comparison([bs[0], twin, bs[1]], K.Field(seed=1), K.Pilot('heuristic'), seed=5, block=64, threads=2)
    out = K.compare(comp, 128)
    st = out['pairs']['b1-twin']
    assert st['n'] == 128 and st['same'] == 128 and st['diff'] == 0 and st['var_ratio'] == 0
    assert out['candidates'][0]['games'] == 128 and not out['failed']
    # every candidate's game k: one opponent, one seed, one chooser; the seat its place in the block
    s0, s2 = comp.spec_of(0, 7, 1), comp.spec_of(2, 7, 1)
    assert s0['seed'] == s2['seed'] and s0['chooser'] == s2['chooser'] == K.chooser_of(7)
    assert s0['sealed']['p0'] == s2['sealed']['p0']                # the opponent, in seat 0
    assert s0['sealed']['p1']['order'] == 'given'
    assert [comp.info[k]['hero'] for k in range(4)] == [0, 1, 0, 1]
    assert [comp.info[k]['chooser'] for k in range(4)] == [0, 0, 1, 1]


def test_a_comparison_replays_exactly(builds):
    p, bs = builds
    sw = one_swap(bs[0], p['cards'])

    def run(threads):
        comp = K.Comparison([bs[0], sw, bs[1]], K.Field(seed=2), K.Pilot('heuristic'), seed=9, block=32,
                            threads=threads)
        K.play(comp, 64)
        return comp.scores

    a = run(1)
    assert run(1) == a and run(3) == a
    # a one-card difference: most games identical, and both swapped decks share 39 slots
    comp = K.Comparison([bs[0], sw], K.Field(seed=2), K.Pilot('heuristic'), seed=9, block=64, threads=2)
    assert comp.setup()['shared_slots'] == {'b1~swap': 39}
    st = K.compare(comp, 128)['pairs']['b1-swap']
    assert st['same'] > 64 and st['var_ratio'] < 0.6


def test_pairing_modes(builds):
    p, bs = builds
    twin = dict(bs[0], name='twin')
    seen = {}
    for mode in K.PAIRINGS:
        comp = K.Comparison([bs[0], twin], K.Field(seed=3), K.Pilot('heuristic'), seed=4, pairing=mode, block=64,
                            threads=2)
        st = K.compare(comp, 64)['pairs']['b1-twin']
        seen[mode] = st['same']
        a, b = comp.spec_of(0, 3, 1), comp.spec_of(1, 3, 1)
        assert (a['seed'] == b['seed']) == (mode in ('full', 'sorted'))
        assert (a['sealed']['p0'] == b['sealed']['p0']) == (mode != 'none')
        assert ('order' in a['sealed']['p1']) == (mode == 'full')
    # the same deck: identical games when the games are shared, not when only the opponent is
    assert seen['full'] == seen['sorted'] == 64
    assert seen['opponent'] < 64 and seen['none'] < 64
    with pytest.raises(ValueError):
        K.Comparison([bs[0], twin], K.Field(seed=3), K.Pilot('heuristic'), pairing='half')


def test_gsprt_is_zero_h2h_formula():
    from zero import h2h
    d = [1, 0, 0, -1, 0, 0.5, 0, -0.5, 1, 0, 0, 0, -1, -1, 0, 1, 0, 0, 0, 0.5]
    for mu0, mu1 in ((0.0, -0.03), (0.0, 0.05), (-0.02, 0.02)):
        # h2h's GSPRT on scores x = (1 + D) / 2 with expected scores s = (1 + mu) / 2, as Elo
        e = lambda mu: 400 * __import__('math').log10((1 + mu) / (1 - mu))   # noqa: E731
        want = h2h.sprt_llr([(1 + x) / 2 for x in d], e(mu0), e(mu1))
        assert K.gsprt_llr(d, mu0, mu1) == pytest.approx(want, rel=1e-9)
    assert K.sprt_bounds(0.05, 0.05) == pytest.approx(h2h.sprt_bounds(0.05, 0.05))


def test_paired_stats():
    a = [1, 1, 0, 1, 0, 0.5, 1, 0]
    b = [1, 0, 0, 1, 0, 0.5, 0, 0]
    st = K.paired_stats(a, b)
    assert st['n'] == 8 and st['diff'] == pytest.approx(0.25) and st['same'] == 6 and st['a_better'] == 2
    assert st['ci95'][0] < st['diff'] < st['ci95'][1]
    assert st['se'] < st['se_unpaired'] and 0 < st['var_ratio'] < 1


def test_the_race_drops_a_bad_deck_and_ties_twins(builds):
    p, bs = builds
    good = bs[0]
    # a bad deck: the same pool's cards, half its lands removed for the most expensive sideboard cards
    side = sorted(good['sideboard'], key=lambda n: -C.cards()[n]['mana_value'])
    lands = [j for j, n in enumerate(good['deck']) if n in C.BASICS][:8]
    deck = list(good['deck'])
    for j, n in zip(lands, side):
        deck[j] = n
    bad = {'name': 'bad', 'deck': deck, 'sideboard': []}
    twin = dict(good, name='twin')
    comp = K.Comparison([good, twin, bad], K.Field(seed=4), K.Pilot('heuristic'), seed=1, block=64, threads=2)
    out = K.race(comp, delta=0.05, round_games=64, max_games=2048, min_games=64)
    by = {c['name']: c for c in out['candidates']}
    assert 'dropped' in by['bad'] and by['bad']['games'] < 2048
    assert set(out['survivors']) == {'b1', 'twin'} and out['stop'] == 'equivalent' and not out['decided']
    assert out['vs_winner']['bad']['diff'] > 0.05
    assert out['test']['alpha_per_test'] == pytest.approx(0.025)
    assert out['rounds'][-1]['games'] == out['games']
    # two decks: a decided race names the winner
    comp = K.Comparison([good, bad], K.Field(seed=4), K.Pilot('heuristic'), seed=1, block=64, threads=2)
    out2 = K.race(comp, delta=0.05, round_games=64, max_games=2048, min_games=64)
    assert out2['decided'] and out2['winner'] == 'b1' and out2['stop'] == 'one left'


def test_a_failed_game_is_left_out_for_every_candidate(builds, monkeypatch):
    p, bs = builds
    comp = K.Comparison([bs[0], bs[1]], K.Field(seed=5), K.Pilot('heuristic'), seed=2, block=16, threads=1)
    real = K.Comparison._run
    calls = {'n': 0}

    def flaky(self, i, b, ks):
        calls['n'] += 1
        if i == 1 and 5 in ks:
            raise RuntimeError('game 5 (queued setup "5") (driver seed 1): a panic')
        return real(self, i, b, ks)

    monkeypatch.setattr(K.Comparison, '_run', flaky)
    K.play(comp, 16)
    assert sorted(comp.scores[0]) == sorted(comp.scores[1]) == [k for k in range(16) if k != 5]
    f = comp.failed[0]
    assert f['k'] == 5 and f['candidate'] == 'b2' and f['spec']['seed'] == K.game_seed(2, 5)


def test_a_network_pilot_plays_the_same_comparison(builds, tmp_path):
    import torch
    from zero.net import make_net, save
    from mtgz.az import games as G
    p, bs = builds
    torch.manual_seed(0)
    os.makedirs(tmp_path / 'r' / 'models')
    ck = str(tmp_path / 'r' / 'models' / 'iter_0000.pt')
    save(make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2), ck)
    twin = dict(bs[0], name='twin')
    pilot = K.Pilot(ck, sims=2)
    comp = K.Comparison([bs[0], twin, bs[1]], K.Field(seed=6), pilot, seed=3, block=4, threads=1, max_turns=6)
    out = K.compare(comp, 4)
    assert out['setup']['pilot']['checkpoint'] == ck
    # identical decks, one network searching with the same streams: identical games
    assert out['pairs']['b1-twin']['same'] == 4 and out['candidates'][2]['games'] == 4
    # and a network against the heuristic field
    comp = K.Comparison([bs[0], bs[1]], K.Field(seed=6), pilot, seed=3, block=4, threads=2, max_turns=6,
                        opp_pilot=K.Pilot('heuristic'))
    out = K.compare(comp, 4)
    assert out['setup']['opp_pilot'] == 'heuristic' and out['candidates'][0]['games'] == 4


def test_the_yardstick_candidates_and_a_fixed_field(builds):
    from mtgz.sealed import seventeen_lands as L
    from mtgz.sealed import yardstick as Y
    pools = L.load('Sealed')
    table = Y.lookup(pools)
    pool = next(p for p in pools if not Y.unsupported(p, table) and not p.incomplete
                and any(b.deck_size == 40 for b in p.builds))
    build = next(b for b in pool.builds if b.deck_size == 40)
    cands = K.yardstick_candidates(pool, build, [0, 1])
    assert [c['name'] for c in cands] == [f'human{build.index}', 'ours0', 'ours1']
    assert cands[0]['deck'] == Y.deck(build)
    for c in cands[1:]:
        assert len(c['deck']) == 40
        nonbasic = [n for n in c['deck'] if n not in C.BASICS]
        assert collections.Counter(nonbasic + c['sideboard']) == collections.Counter(pool.cards)
    p, bs = builds
    field = K.Field(decks=[bs[2]])
    comp = K.Comparison(cands, field, K.Pilot('heuristic'), seed=1, block=32, threads=2)
    out = K.compare(comp, 32)
    assert out['setup']['field'] == {'kind': 'fixed', 'decks': 1}
    assert all(comp.spec_of(0, k, 0)['sealed']['p1']['deck'] == bs[2]['deck'] for k in range(4))
    assert len(out['pairs']) == 3


def test_the_cli(builds, tmp_path):
    p, bs = builds
    paths = []
    for b in bs[:2]:
        path = tmp_path / f'{b["name"]}.json'
        path.write_text(json.dumps(b), encoding='utf-8')
        paths.append(str(path))
    txt = tmp_path / 'b3.txt'
    counts = collections.Counter(bs[2]['deck'])
    txt.write_text('\n'.join(f'{k} {n}' for n, k in sorted(counts.items())) + '\nSideboard\n'
                   + '\n'.join(bs[2]['sideboard']), encoding='utf-8')
    d = K.read_deck(str(txt))
    assert sorted(d['deck']) == sorted(bs[2]['deck']) and sorted(d['sideboard']) == sorted(bs[2]['sideboard'])
    out = tmp_path / 'out.json'
    assert K.main(['pair', *paths, '--games', '64', '--block', '32', '--threads', '2', '--quiet', '--json', str(out)]) == 0
    r = json.loads(out.read_text(encoding='utf-8'))
    assert r['pairs']['b1-b2']['n'] == 64 and r['setup']['pairing'] == 'full'
    assert K.main(['race', *paths, str(txt), '--race', '--games', '64', '--block', '32', '--threads', '2',
                   '--max-games', '128', '--min-games', '64', '--quiet', '--json', str(out)]) == 0
    r = json.loads(out.read_text(encoding='utf-8'))
    assert r['games'] <= 128 and len(r['candidates']) == 3
    assert K.main(['builds', '--pool-seed', str(p['seed']), '--seeds', '0,1', '--out', str(tmp_path / 'decks')]) == 0
    assert len(os.listdir(tmp_path / 'decks')) == 2
    # the yardstick: the first fully supported pool's human build against our builds of it
    assert K.main(['yardstick', '--pool', '0', '--ours', '0', '--games', '32', '--block', '32', '--threads', '2',
                   '--quiet', '--json', str(out)]) == 0
    r = json.loads(out.read_text(encoding='utf-8'))
    assert r['yardstick']['note'].startswith('a yardstick only') and [c['name'] for c in r['candidates']][1] == 'ours0'
