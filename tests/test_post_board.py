"""Post-sideboard games only (docs/ENGINE.md 130) through the bindings: a match of the presets' spec starts at the
sideboarding before game 2 and replays from its seed; a seat of `"keep_deck"` only passes; and the sideboarding
yardstick `mtgz.az.keep_h2h` (side B keeps its deck, the seats alternating by halves) on a tiny network."""
import json

import pytest

import mtgz.az as P
from mtgz.az import decklists
from mtgz.az import games as G
from mtgz.az import keep_h2h as K


def _spec(**kw):
    return decklists.mirror_bo3_spec(max_turns=kw.pop('max_turns', 60), game_episodes=True, start_game=2, **kw)


def test_a_match_starts_at_the_sideboarding_before_game_2_and_a_keeper_only_passes():
    E = G.engine()
    for seed in range(6):
        g = E.Game('mtg', _spec(keep_deck=[1]), 2, seed)
        m = json.loads(g.export_state())['match']
        assert m['post_board'] is True and m['game'] == 1 and sorted(m['wins']) == [0, 1]
        loser = m['wins'].index(0)
        assert g.to_move == loser
        # sideboard until seat 1 decides: its only option is to keep its deck (PASS = 0)
        while g.to_move is not None and g.to_move != 1:
            g.step(0)
        if g.to_move == 1 and json.loads(g.export_state()).get('decision', {}).get('kind') == 'Sideboard':
            assert list(g.legal()) == [0]


def test_half_specs():
    a, b = K.half_specs(P.SPEC_DELVER_BO3)
    assert json.loads(a)['keep_deck'] == [1] and json.loads(b)['keep_deck'] == [0]
    for s in (a, b):
        assert json.loads(s)['start_game'] == 2
    with pytest.raises(ValueError, match='names keep_deck'):
        K.half_specs(a)
    with pytest.raises(ValueError, match='constructed best-of-three'):
        K.half_specs(P.SPEC_DELVER)


def test_a_self_play_spec_with_keep_deck_is_refused():
    """docs/ENGINE.md 147.4 (the Fable review's flow B3): `"keep_deck"` is an evaluation setting (130.3) - zero.run's
    self-play spec of every phase goes through the package's `selfplay_specs` hook for `mtg`, which keeps the run's
    spec and refuses one holding it (exit 78: an EncodingMismatch, refused and not restarted)."""
    from types import SimpleNamespace
    from zero.run import EncodingMismatch, Run
    for spec in (P.SPEC_DELVER_BO3, P.SPEC_DELVER, decklists.mirror_bo3_spec()):
        fake = SimpleNamespace(cfg={'game': 'mtg', 'spec': spec})
        assert Run.selfplay_spec(fake, 3) == spec                       # the run's own spec, unchanged
    for half in K.half_specs(P.SPEC_DELVER_BO3):
        fake = SimpleNamespace(cfg={'game': 'mtg', 'spec': half})
        with pytest.raises(EncodingMismatch, match='keep_deck'):
            Run.selfplay_spec(fake, 3)
    assert decklists.selfplay_spec(SimpleNamespace(cfg={'spec': ''}), 1) is None    # the game's default spec


def test_keep_h2h_on_a_tiny_network(tmp_path):
    import torch
    from zero.net import make_net, save
    torch.manual_seed(0)
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=P.PACKAGE.aux_of('mtg'))
    ck = str(tmp_path / 'models' / 'iter_0000.pt')
    (tmp_path / 'models').mkdir()
    save(m, ck, {'iter': 0})
    out = K.keep_match(ck, games=4, sims=2, chance_k=0, threads=2, groups=1, spec=_spec(max_turns=12), max_steps=60_000)
    assert out['keeps'] == 'b' and out['b'] == ck and out['games'] == 4 and out['unfinished'] == 0
    assert out['pairs']['n'] == 2 and 0.0 <= out['a_score'] <= 1.0
    # one or two games a match (games 2 and 3), A's games beside the matches
    assert 4 <= out['per_game']['games'] <= 8
    with pytest.raises(ValueError, match='even'):
        K.keep_match(ck, games=3)
