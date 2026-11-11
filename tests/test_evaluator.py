"""S4a, the deck evaluator (docs/ENGINE.md 108): the card table and deck features, the dataset held out by game, the
play network's pre-game value of logged games (rebuilt from spec and seed, the chooser forced, the same in every dealt
world; by one checkpoint or by the one that played each game), the given-deck judge, the models, the saved evaluator and
its picker, card ranking v0, the 17lands yardstick (never training), the whole pipeline on a small log and
`python -m mtgz.sealed.compare evalpick` with a tiny network."""
import json
import os

import numpy as np
import pytest

from mtgz.az import games as G
from mtgz.sealed import compare as K
from mtgz.sealed import engine as SE
from mtgz.sealed import evaluator as EV
from mtgz.sealed import games as GL

SEALED = {'set': 'eoe', 'collation': 'arena', 'best_share': 0.5}


def fake_log(d, n=24, seed=0, iters=3):
    """A version-1 game log of `n` self-play games (their decks the sealed spec's seats of each seed, results drawn at
    random) and a metrics line per iteration, as a run writes them: no game is played (the start position needs only
    the spec line and the seed)."""
    os.makedirs(d / 'games')
    rng = np.random.default_rng(seed)
    spec = {'v': 1, 'spec': 's1', 'sealed': SEALED, 'decks': 'random', 'max_turns': 60, 'hidden_decklists': True,
            'eval_decks': 64, 'eval_seed': 1, 'deal_samples': 32}
    lines = [json.dumps(spec)]
    for i in range(n):
        s = 1000 + i
        seats = SE.seats(json.dumps({'sealed': SEALED, 'decks': 'random'}), s)
        r = float(rng.integers(0, 2))
        lines.append(json.dumps({'v': 1, 'spec': 's1', 'seed': s, 'deck_seed': s, 'eval': None, 'aborted': False,
                                 'searched': [True, True], 'result': [r, 1 - r], 'turns': 20, 'time': 100.0 + i,
                                 'seats': [{'mode': x['mode'], 'pool_seed': x['pool_seed'], 'colors': x['build']['colors'],
                                            'splash': x['build']['splash'], 'lands': x['build']['lands'],
                                            'deck': GL.deck_counts(x['deck'])} for x in seats]}))
    with open(d / 'games' / 'games-1.jsonl', 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines) + '\n')
    per = (n + iters - 1) // iters
    with open(d / 'metrics.jsonl', 'w', encoding='utf-8') as f:
        f.write('\n'.join(json.dumps({'iter': k, 'time': 100.0 + per * k - 0.5}) for k in range(1, iters + 1)) + '\n')
    return str(d)


def tiny_net(path, seed=0):
    import torch
    from zero.net import make_net, save
    torch.manual_seed(seed)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    save(make_net(G.info('mtg_eoe'), 'mtg_eoe', 'tokens', 16, 1, heads=2), path)
    return path


@pytest.fixture(scope='module')
def run(tmp_path_factory):
    return fake_log(tmp_path_factory.mktemp('run'))


@pytest.fixture(scope='module')
def models(tmp_path_factory):
    d = tmp_path_factory.mktemp('models')
    return str(d), [tiny_net(str(d / f'iter_{k:04d}.pt'), seed=k) for k in range(4)]


def test_the_card_table_and_deck_features():
    names = EV.vocab()
    assert len(names) == 320 and names == tuple(sorted(names)) and 'Forest' in names
    deck = ['Forest'] * 9 + ['Mountain'] * 8 + ['Frontline War-Rager'] * 2 + ['Fungal Colossus'] * 3 + ['Drill Too Deep']
    X = EV.counts_matrix([deck, {'Forest': 2}])
    assert X.shape == (2, 320) and X[0].sum() == len(deck) and X[1, EV.index()['Forest']] == 2
    F = dict(zip(EV.FEATURES, EV.deck_features(X)[0]))
    assert F['lands'] == 17 and F['basics_G'] == 9 and F['basics_R'] == 8 and F['nonbasic_lands'] == 0
    assert F['creatures'] + F['noncreature_spells'] == len(deck) - 17
    assert F['pair_RG'] == 1 and F['mono'] == 0 and sum(F[f'pair_{p}'] for p in EV.PAIRS) == 1
    assert EV.deck_colours(X[:1]) == [frozenset('RG')]
    with pytest.raises(ValueError, match='not a card'):
        EV.counts_matrix([['Black Lotus']])


def test_the_dataset_is_held_out_by_game(run):
    ds = EV.dataset(run, test=0.3, val=0.2)
    d = EV.describe(ds)
    assert d['games'] == 24 and d['rows'] == 48 and d['train_games'] + d['val_games'] + d['test_games'] == 24
    assert d['test_games'] > 0 and d['val_games'] > 0
    # a game's two rows: one split, results that add up, each the other's opponent, the decks of the log
    assert (ds['split'][0::2] == ds['split'][1::2]).all() and (ds['result'][0::2] + ds['result'][1::2] == 1).all()
    assert (ds['opp'][ds['opp']] == np.arange(48)).all() and (ds['game'][0::2] == np.arange(24)).all()
    specs, games = GL.load(run)
    assert ds['X'][1, EV.index()[next(iter(games[0]['seats'][1]['deck']))]] >= 1
    assert sorted(set(ds['iter'])) == [1, 2, 3]
    # the time split: the last iterations are the test, the ones just before them the validation
    t = EV.dataset(run, how='time', time_from=3, val_iters=1)
    assert [sorted(set(t['split'][t['iter'] == k])) for k in (1, 2, 3)] == [[0], [1], [2]]
    # bot seats are left out unless asked for
    games[0]['searched'] = [True, False]
    assert EV.describe(EV.dataset(run, games=games, specs=specs))['skipped']['bot_seat'] == 1
    assert EV.describe(EV.dataset(run, games=games, specs=specs, pilots='all'))['games'] == 24


def test_the_pregame_value_and_the_judge(run, models, tmp_path):
    import torch
    _, cks = models
    out = str(tmp_path / 'v.npz')
    r = EV.pregame_values(run, cks[1], out, threads=1, worlds_check=4)
    assert r['v'].shape == (24, 2, 2) and ((r['v'] > 0) & (r['v'] < 1)).all()
    assert r['meta']['worlds_checked'] == 4 * 2 * 2 * 3, 'every seat and chooser, three dealt worlds'
    back = EV.load_values(out)
    assert np.array_equal(back['v'], r['v']) and back['keys'] == r['keys']
    # the start position: the chooser's play-or-draw decision before any hand
    specs, games = GL.load(run)
    g = EV.start_position(specs['s1'], games[0]['seed'], 1)
    assert g.to_move == 1 and g.rounds == 0 and json.loads(g.export_state())['decision']['kind'] == 'PlayDraw'
    # the given-deck judge values a deck as the logged game did (the same encoding of seat 0)
    seats = SE.seats(json.dumps({'sealed': SEALED, 'decks': 'random'}), games[0]['deck_seed'])
    j = EV.NetJudge(cks[1], sealed=SEALED).values([(seats[0]['deck'], seats[0]['sideboard'])])
    assert np.allclose(j[0], r['v'][0, 0], atol=1e-6)
    # on the dataset: chooser me / chooser the opponent, averaged as the target
    ds = EV.attach_values(EV.dataset(run), back)
    assert np.allclose(ds['v'][0], [r['v'][0, 0, 0], r['v'][0, 0, 1]]) and np.allclose(ds['v'][1], [r['v'][0, 1, 1], r['v'][0, 1, 0]])
    assert np.allclose(EV.pregame_target(ds), ds['v'].mean(1))
    # each game by the network that played it: iteration k by iter_{k-1}
    d, _ = models
    p = EV.pregame_values(run, 'played', None, threads=1, worlds_check=0, models=d)
    assert p['meta']['checkpoints'] == 3
    assert np.allclose(p['v'][:8], EV.pregame_values(run, cks[0], None, threads=1, worlds_check=0)['v'][:8])
    assert np.allclose(p['v'][8:16], r['v'][8:16]) and not np.allclose(p['v'][:8], r['v'][:8])
    torch.set_num_threads(1)


def synthetic(n=600, seed=0):
    """Builder decks of generated pools, results drawn from planted card values (a card's value: its index's parity)."""
    rng = np.random.default_rng(seed)
    decks = []
    for i in range(n // 6):
        p = SE.pool(5000 + i, collation='arena')
        decks += [SE.build(p['cards'], s)['deck'] for s in range(6)]
    X = EV.counts_matrix(decks)
    w = np.where(np.arange(X.shape[1]) % 2 == 0, 0.4, -0.4)
    w[[EV.index()[b] for b in EV.BASIC_COLOUR]] = 0
    z = X @ w
    p = 1 / (1 + np.exp(-(z - z.mean())))
    return decks, X, (rng.random(len(p)) < p).astype(np.float32), w


def test_the_models_and_the_saved_evaluator(tmp_path):
    import torch
    decks, X, y, w = synthetic()
    Fz = EV.Scaler(EV.deck_features(X))(EV.deck_features(X))
    lin = EV.fit('linear', X, Fz, y, l2_cards=1e-3)
    seen = X.sum(0) > 20
    assert EV.pearson(lin.wc.detach().numpy()[seen], w[seen]) > 0.5, 'the planted card values come back'
    const = EV.fit('const', X, Fz, y)
    assert abs(float(torch.sigmoid(const.bias.detach())) - y.mean()) < 1e-3
    s = EV.fit('set', X, Fz, y, val=(X[:100], Fz[:100], y[:100]), hyper=EV.SET_HYPER, epochs=3, seed=1)
    p = EV.predict([s], X, Fz)
    assert p.shape == (len(X),) and ((p > 0) & (p < 1)).all()
    again = EV.predict([EV.fit('set', X, Fz, y, val=(X[:100], Fz[:100], y[:100]), hyper=EV.SET_HYPER, epochs=3, seed=1)], X, Fz)
    assert np.allclose(p, again), 'seeded: the same fit'
    # the evaluator: saved and loaded, the same scores; the picker checks builds of the pool
    sc = EV.Scaler(EV.deck_features(X))
    e = EV.Evaluator('set', [s], sc, EV.SET_HYPER)
    path = str(tmp_path / 'e.pt')
    e.save(path)
    e2 = EV.Evaluator.load(path)
    assert np.allclose(e2.score(decks[:20]), e.score(decks[:20])) and np.allclose(e.score(decks[:20]), p[:20])
    pool = SE.pool(5000, collation='arena')['cards']
    assert np.allclose(EV.rank(pool, [{'deck': d} for d in decks[:6]], e2), p[:6])
    with pytest.raises(ValueError, match='not a build'):
        EV.rank(pool, [decks[7] if set(decks[7]) - set(pool) - set(EV.BASIC_COLOUR) else decks[0][:39]], e2)
    # card ranking v0: a planted top card comes first
    lin.wc.data[:] = 0
    top = 'Frontline War-Rager'
    lin.wc.data[EV.index()[top]] = 2.0
    cv = EV.card_values(EV.Evaluator('linear', [lin, lin], sc), X, n_decks=40,
                        cards=[top, 'Fungal Colossus', 'Drill Too Deep'])
    assert cv[0]['name'] == top and cv[0]['value'] > 5 and cv[0]['sd'] == 0.0 and cv[0]['decks'] > 0
    rates = {top: {'gih': 0.6, 'gih_n': 500, 'gp': 0.58}, 'Fungal Colossus': {'gih': 0.5, 'gih_n': 500, 'gp': 0.5},
             'Drill Too Deep': {'gih': 0.4, 'gih_n': 500, 'gp': 0.45}}
    ys = EV.yardstick(cv, rates, min_games=100)
    assert ys['cards'] == 3 and -1 <= ys['spearman_gih'] <= 1
    assert EV.spearman([1, 2, 3, 4], [10, 20, 30, 45]) == pytest.approx(1.0) and EV.rankdata([3, 1, 3]).tolist() == [2.5, 1, 2.5]


def test_seventeen_lands_rates_are_a_yardstick():
    r = EV.seventeen_lands_rates('Sealed')
    assert len(r) > 250 and all(0 <= x['gih_n'] <= x['gp_n'] for x in r.values())
    assert all(0 <= x['gp'] <= 1 for x in r.values()) and not set(r) & set(EV.BASIC_COLOUR)


def test_the_pipeline_on_a_small_log(run, models, tmp_path):
    import torch
    _, cks = models
    vals = str(tmp_path / 'v.npz')
    EV.pregame_values(run, cks[1], vals, threads=1, worlds_check=0)
    out = str(tmp_path / 'out')
    rep = EV.train(run, vals, out, how='hash', seed=0, ensemble=2, pools=2, k=4, threads=1, kinds=['const', 'feat', 'linear', 'set'],
                   lams=(0.0, 1.0), split_kw={'test': 0.3, 'val': 0.25}, set_epochs=2, n_card_decks=5)
    torch.set_num_threads(1)
    assert set(rep['models']) == {'const', 'feat', 'linear', 'set'}
    t = rep['models']['linear']['selected']['test']
    assert t['rows'] == 2 * rep['dataset']['test_games'] and 0 < t['logloss'] < 5
    assert rep['same_pool']['pools'] == 2 and 0 <= rep['same_pool']['linear']['concordance'] <= 1
    assert os.path.exists(os.path.join(out, 'evaluator_set.pt')) and os.path.exists(os.path.join(out, 'cards_linear.json'))
    assert 'spearman_gih' in rep['cards']['linear']['yardstick_17lands'] or rep['cards']['linear']['yardstick_17lands']['cards'] < 3
    assert 'network value (mean)' in EV.summary_table(rep)
    # the command line: rank a pool's candidates
    o = EV.main(['rank', '--evaluator', os.path.join(out, 'evaluator_linear.pt'), '--pool-seed', '7', '--k', '4'])
    assert len(o) == 4 and o[0]['score'] >= o[-1]['score']


def test_evalpick_races_the_evaluators_pick(tmp_path, models):
    _, cks = models
    decks, X, y, _ = synthetic(120)
    sc = EV.Scaler(EV.deck_features(X))
    e = EV.Evaluator('linear', [EV.fit('linear', X, sc(EV.deck_features(X)), y)], sc)
    ep = str(tmp_path / 'e.pt')
    e.save(ep)
    p = SE.pool(11, collation='paper')
    pilot = K.Pilot(cks[0], sims=2)
    e2 = EV.Evaluator('const', [EV.fit('const', X, sc(EV.deck_features(X)), y)], sc)
    info, cands = K.evalpick_candidates(p['cards'], [('linear', e), ('const', e2)], pilot, k=5, worlds=2, seed=11, vs=2,
                                        collation='paper')
    assert set(info['picks']) == {'linear', 'const', 'value'} and len(info['scores']['linear']) == 5
    assert info['scores']['linear'][info['picks']['linear']] == max(info['scores']['linear'])
    assert info['picks']['const'] == 0, 'a constant ties every build: the first'
    picked = {info['names'][x] for x in info['picks']}
    assert [c['name'] for c in cands[:len(picked)]] == list(dict.fromkeys(info['names'][x] for x in info['picks']))
    assert len(cands) == len(picked) + 2 and all(c['name'].startswith('random') for c in cands[-2:])
    out_path = tmp_path / 'evalpick.json'
    rc = K.main(['evalpick', '--evaluator', ep, '--pool-seed', '11', '--collation', 'paper', '--k', '5', '--worlds', '2',
                 '--vs', '1', '--pilot', cks[0], '--sims', '2', '--games', '4', '--block', '4', '--threads', '1',
                 '--max-turns', '6', '--json', str(out_path), '--quiet'])
    out = json.load(open(out_path))
    ev = out['evalpick']
    assert rc == 0 and ev['picks']['linear'] == info['picks']['linear'] and out['candidates'][0]['games'] == 4
    assert out['summary']['linear']['vs_random']['pools'] == 1 and set(out['summary']) == {'linear', 'value'}
    assert (ev['paired']['linear']['vs_value'] is None) == (ev['picks']['linear'] == ev['picks']['value'])
    # over pools: the summary averages each pool's paired differences (a pool where the picks agree: 0 against value)
    s = K.evalpick_summary([{'linear': {'vs_random': {'random1': {'diff': 0.1}, 'random2': {'diff': 0.3}}, 'vs_value': None},
                             'value': {'vs_random': {'random1': {'diff': 0.2}, 'random2': {'diff': 0.2}}}},
                            {'linear': {'vs_random': {'random1': {'diff': -0.1}}, 'vs_value': {'diff': 0.05}},
                             'value': {'vs_random': {'random1': {'diff': -0.15}}}}])
    assert s['linear']['vs_random']['mean'] == pytest.approx(0.05) and s['linear']['vs_value']['per_pool'] == [0.0, 0.05]
    assert s['value']['vs_random']['per_pool'] == [0.2, -0.15] and 'vs_value' not in s['value']
