"""Dealt decks (docs/ENGINE.md 154, the spec's `"dealt_decks"`): the preset `delver-mirror-bo3-dealt` (delver-mirror-bo3
with dealt decks in self-play only), the evaluation spec without them (`evaldecks.eval_spec` / `undealt`, the engine's
`MtgSpec::eval_json` rule), the tools that never deal (the recorded matches' spec, the sideboard census - end to end on
a tiny network whose run spec deals every match -, keep_h2h refusing a dealt spec), and a dealt match through the
bindings: the dealt seat never has a sideboarding decision, the other seat sideboards as always."""
import json

import pytest

import mtgz.az as A
from mtgz.az import decklists, evaldecks, keep_h2h, replays
from mtgz.az import games as G
from mtgz.az import sideboard_census as C

DEALT = {'p': 0.5, 'swaps': [1, 8]}


def test_the_dealt_preset_is_delver_mirror_bo3_with_dealt_decks_in_self_play_only():
    base, p = A.PRESETS['delver-mirror-bo3'], A.PRESETS['delver-mirror-bo3-dealt']
    s = json.loads(p['spec'])
    assert s['dealt_decks'] == DEALT
    # the spec is delver-mirror-bo3's with the key; the evaluation spec is delver-mirror-bo3's (never dealt)
    assert {k: v for k, v in s.items() if k != 'dealt_decks'} == json.loads(base['spec'])
    assert p['eval_spec'] == base['spec'] == base['eval_spec'] == A.SPEC_DELVER_BO3
    assert 'dealt_decks' not in json.loads(p['eval_spec'])
    # every other setting is delver-mirror-bo3's; delver-mirror-bo3 itself unchanged (no dealt decks)
    assert {k: v for k, v in p.items() if k != 'spec'} == {k: v for k, v in base.items() if k != 'spec'}
    assert 'dealt_decks' not in json.loads(base['spec']) and 'dealt_decks' not in json.loads(A.PRESETS['delver-bo3-smoke']['spec'])
    assert p['spec'] == decklists.mirror_bo3_spec(max_turns=60, game_episodes=True, start_game=2, dealt_decks=DEALT)


def test_the_evaluation_spec_and_the_tools_spec_never_deal():
    dealt = A.SPEC_DELVER_BO3_DEALT
    # eval_spec (the package's evaluation spec, as the engine's MtgSpec::eval_json) drops dealt_decks, keeps the rest
    assert evaldecks.eval_spec(dealt) == evaldecks.eval_spec(A.SPEC_DELVER_BO3) == A.SPEC_DELVER_BO3
    assert A.PACKAGE.eval_specs['mtg'](dealt) == A.SPEC_DELVER_BO3
    # undealt: the same string without the key
    assert evaldecks.undealt(A.SPEC_DELVER_BO3) is A.SPEC_DELVER_BO3
    assert evaldecks.undealt(dealt) == A.SPEC_DELVER_BO3
    assert evaldecks.undealt(A.SPEC) is A.SPEC and evaldecks.eval_spec(A.SPEC) == A.EVAL_SPEC
    # the recorded matches (mtgz.az.replays), the census
    rs = json.loads(replays.spec_with_log(dealt))
    assert rs['log'] is True and 'dealt_decks' not in rs and rs['start_game'] == 2
    assert C.census_spec({'spec': dealt}) == A.SPEC_DELVER_BO3
    assert C.census_spec({'spec': A.SPEC_DELVER_BO3}) == A.SPEC_DELVER_BO3
    # keep_h2h refuses a dealt spec (pass the evaluation spec)
    with pytest.raises(ValueError, match='dealt_decks'):
        keep_h2h.half_specs(dealt)
    a, b = keep_h2h.half_specs(evaldecks.eval_spec(dealt))
    assert json.loads(a)['keep_deck'] == [1] and json.loads(b)['keep_deck'] == [0]


def test_the_engine_reads_the_spec_and_refuses_it_outside_a_constructed_best_of_three():
    E = G.engine()
    E.Game('mtg', A.SPEC_DELVER_BO3_DEALT, 2, 1)
    with pytest.raises(Exception, match='dealt_decks'):
        E.Game('mtg', json.dumps(dict(json.loads(A.SPEC_DELVER), dealt_decks=DEALT)), 2, 1)
    with pytest.raises(Exception, match='keep_deck'):
        E.Game('mtg', decklists.mirror_bo3_spec(game_episodes=True, start_game=2, keep_deck=[0], dealt_decks=DEALT), 2, 1)
    with pytest.raises(Exception, match='swaps'):
        E.Game('mtg', decklists.mirror_bo3_spec(game_episodes=True, start_game=2, dealt_decks={'p': 0.5, 'swaps': [1, 16]}), 2, 1)


def test_a_dealt_match_through_the_bindings():
    # every match dealt (p 1): the dealt seat never has a Sideboard decision, the other seat sideboards (PASS = 0
    # keeps its 60); the dealt seat chooses play or draw when it lost
    E = G.engine()
    spec = decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2, dealt_decks={'p': 1.0, 'swaps': [1, 8]})
    sideboarders = set()
    for seed in range(8):
        g = E.Game('mtg', spec, 2, seed)
        steps, seats = 0, set()
        while g.to_move is not None and steps < 3000:
            st = json.loads(g.export_state())
            d = st.get('decision') or {}
            if d.get('kind') == 'Sideboard':
                seats.add(d['player'])
            legal = list(g.legal())
            g.step(0 if d.get('kind') == 'Sideboard' else legal[(seed + steps) % len(legal)])
            steps += 1
        # one seat sideboards (the other's 60 is dealt)
        assert len(seats) == 1, (seed, seats)
        sideboarders |= seats
    # (the dealt seat is the seed's: across 8 matches each seat was the one sideboarding)
    assert sideboarders == {0, 1}


def test_the_census_of_a_dealt_run_plays_every_players_own_sideboarding(tmp_path):
    import torch
    from zero.net import make_net, save
    run = tmp_path / 'delver-bo3-dealt-tiny'
    (run / 'models').mkdir(parents=True)
    spec = decklists.mirror_bo3_spec(max_turns=12, game_episodes=True, start_game=2, dealt_decks={'p': 1.0, 'swaps': [1, 8]})
    cfg = {'game': 'mtg', 'spec': spec, 'eval_spec': evaldecks.eval_spec(spec), 'sims': 2, 'sims_minor': 2,
           'chance_k': 0, 'aux_belief_w': 0.25, 'aux_deck_w': 0.5, 'eval_select': 'visits'}
    (run / 'config.json').write_text(json.dumps(cfg), encoding='utf-8')
    torch.manual_seed(0)
    m = make_net(G.info('mtg'), 'mtg', 'tokens', 16, 1, heads=2, aux=A.PACKAGE.aux_of('mtg'))
    ck = str(run / 'models' / 'iter_0004.pt')
    save(m, ck, {'iter': 4})
    records, info = C.run_starts(ck, starts=4, device='cpu', seed=2)
    # both players sideboarded in every start (a dealt seat would have no Sideboard decision)
    assert all(len(r['order']) == 2 for r in records), [r['order'] for r in records]
    rec = C.summarize(records, C.census_spec(cfg), info)
    assert all(rec[k]['n'] == 4 for k in C.SPLITS)
