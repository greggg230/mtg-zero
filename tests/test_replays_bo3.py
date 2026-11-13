"""mtgz.az.replays on a constructed best-of-three (docs/ENGINE.md 120, 121, 130; the delverai site's match replays): a
recorded match is one replay from the sideboarding before game 2 to the match's end, with its games' boundaries
(`match`), each player's deck in the spectator view while sideboarding and after it, the sideboarding options in words,
the deck read on every network decision in a game and on none while sideboarding; `deck_read` against the true unseen
cards; the listing record's match. A tiny untrained network, CPU, 12-turn games."""
import json
import os

import numpy as np
import pytest

import mtgz.az as A
from mtgz.az import belief, decklists, replays as R
from mtgz.az import games as G


def _tiny_run(tmp_path, aux_deck_w=0.5):
    import torch
    from zero.net import make_net, save
    run = tmp_path / 'delver-bo3-tiny'
    (run / 'models').mkdir(parents=True)
    spec = json.loads(decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2))
    cfg = {'game': 'mtg', 'spec': json.dumps(spec), 'sims': 4, 'sims_minor': 2, 'chance_k': 0, 'aux_belief_w': 0.25,
           'aux_deck_w': aux_deck_w, 'eval_select': 'visits'}
    (run / 'config.json').write_text(json.dumps(cfg), encoding='utf-8')
    torch.manual_seed(0)
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=A.PACKAGE.aux_of('mtg'))
    ck = str(run / 'models' / 'iter_0000.pt')
    save(m, ck, {'iter': 0})
    return ck


def test_the_spectator_view_shows_the_decks_and_the_sideboarding_in_words():
    E = G.engine()
    g = E.Game('mtg', decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2), 2, 5)
    s = json.loads(g.export_state())
    m = s['match']
    assert m['post_board'] is True and m['results'] in ([1], [2]) and m['sideboarding']['done'] == [False, False]
    assert [sum(n for _, n in d) for d in m['decks']] == [60, 60]
    assert dict(m['decks'][0])['Prismari Charm'] == 1 and 'Pyroblast' not in dict(m['decks'][0])
    texts = [o['t'] for o in s['decision']['options']]
    assert texts[0] == 'Keep the same deck' and 'Bring in Karakas' in texts and 'Change Pyroblast (now 0 of 3)' in texts
    assert not any('Def(' in t for t in texts)
    # a count: "3 Pyroblast (now 0)", the deck being built shows it
    a = next(o['a'] for o in s['decision']['options'] if o['t'] == 'Change Pyroblast (now 0 of 3)')
    g.step(a)
    s = json.loads(g.export_state())
    three = next(o['a'] for o in s['decision']['options'] if o['t'] == '3 Pyroblast (now 0)')
    g.step(three)
    s = json.loads(g.export_state())
    me = s['decision']['player']
    assert dict(s['match']['decks'][me])['Pyroblast'] == 3 and sum(n for _, n in s['match']['decks'][me]) == 63
    assert s['decision']['options'][0]['t'] != 'Keep the same deck'      # not 60: no PASS yet


def test_a_match_is_one_replay_with_its_games_and_the_deck_reads(tmp_path):
    ck = _tiny_run(tmp_path)
    reps = R.record(ck, games=2, device='cpu', seed=3, conc=2, title='tiny')
    assert len(reps) == 2
    for rep in reps:
        m = rep['match']
        assert m['post_board'] is True and m['games'][0]['game'] == 1 and m['games'][0]['played'] is False
        assert m['games'][0]['winner'] in (0, 1)
        played = [e for e in m['games'] if e['played']]
        assert [e['game'] for e in played] == list(range(2, 2 + len(played))) and 1 <= len(played) <= 2
        assert len(rep['games']) == len(played)                         # zero.replay's episodes: games 2 and 3
        assert played[0]['sb'] == 0                                     # the match opens at game 2's sideboarding
        for e in played:
            assert e['sb'] < e['start'] and e['chooser'] in (0, 1) and e['choice'] in ('Play first', 'Draw first')
            assert len(e['swaps']) == 2 and all(set(sw) == {'in', 'out'} for sw in e['swaps'])
            assert sum(n for _, n in e['swaps'][0]['in']) == sum(n for _, n in e['swaps'][0]['out'])
        for i, f in enumerate(rep['frames']):
            st = rep['states'][f['s']]
            sb = (st.get('decision') or {}).get('kind') == 'Sideboard'
            if sb:
                assert 'deck' not in f and 'belief' not in f
            elif f['k'] == 'nn':
                d = f['deck']
                assert d['for'] == 1 - f['p'] and d['n'] > 0 and len(d['cards']) == R.DECK_TOP
                pool = R.deck_pools(rep['spec'])[d['for']]
                assert all(k in pool for k, _ in d['vs_main']) and d['outside'] >= 0
        s = R.summary(rep)
        assert s['match']['wins'] == m['wins'] and [e['game'] for e in s['match']['games']] == [e['game'] for e in m['games']]
        assert 'reads' not in json.dumps(s) and 'start' not in s['match']['games'][1]
        assert len(json.dumps(s)) < 16 * 1024                           # the site's listing record limit


def test_no_deck_head_no_deck_read(tmp_path):
    ck = _tiny_run(tmp_path, aux_deck_w=0)
    rep = R.record(ck, games=1, device='cpu', seed=1, conc=1)[0]
    assert rep['match'] and not any('deck' in f for f in rep['frames'])


def test_the_deck_read_puts_the_seen_cards_and_the_guess_against_the_main():
    names = belief.names()
    ident = {n: i for i, n in enumerate(names) if n}
    main = {'Brainstorm': 4, 'Prismari Charm': 1, 'Island': 1}
    # their real 60 (a stand-in of 6): Prismari Charm out, Pyroblast in; they have shown one Brainstorm
    state = {'match': {'decks': [[], [['Brainstorm', 4], ['Island', 1], ['Pyroblast', 1]]]}}
    unseen = np.zeros(belief.D)
    unseen[ident['Brainstorm']] = 3
    unseen[ident['Island']] = 1
    unseen[ident['Pyroblast']] = 1
    pred = np.zeros(belief.D)
    pred[ident['Brainstorm']] = 3
    pred[ident['Island']] = 1
    pred[ident['Pyroblast']] = 0.6
    pred[ident['Prismari Charm']] = 0.4
    pred[ident['Lightning Bolt']] = 0.1               # outside their 75
    r = R.deck_read(pred / pred.sum(), state, 0, unseen, main, pool={'Brainstorm', 'Island', 'Prismari Charm', 'Pyroblast'})
    assert r['for'] == 1 and r['n'] == 5
    vs = dict(r['vs_main'])
    # the guess: 1 seen Brainstorm + 2.94 unseen, Pyroblast 0.59 in, Prismari Charm 0.39 of its 1 left in
    assert vs['Pyroblast'] == pytest.approx(0.59, abs=0.01) and vs['Prismari Charm'] == pytest.approx(-0.61, abs=0.01)
    assert 'Lightning Bolt' not in vs and r['outside'] == pytest.approx(0.1, abs=0.01)
    assert [k for k, _ in r['cards']][:2] == ['Brainstorm', 'Island']
    assert R.deck_read(pred, state, 0, np.zeros(belief.D), main) is None      # nothing unseen
    assert R.swaps([['Brainstorm', 3], ['Pyroblast', 1]], {'Brainstorm': 4}) == {'in': [['Pyroblast', 1]], 'out': [['Brainstorm', 1]]}
    assert R.main_decks(A.SPEC_DELVER_BO3)[0]['Island'] == 1 and sum(R.main_decks(A.SPEC_DELVER_BO3)[1].values()) == 60
    assert len(R.deck_pools(A.SPEC_DELVER_BO3)[0]) == 29                    # the 75's 29 entries
    assert R.match_index({'states': [{'turn': 1}], 'frames': []}) is None    # a single game: no match
