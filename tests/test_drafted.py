"""Team drafting end to end (M6 part 3, docs/ENGINE.md 45, DESIGN.md 2.3): `team_draft`'s deferred
glue (`mtgz.az.drafted`: the payload's matches as `mtg` specs, the matches' results back, a
malformed result refused), events parked by a draft driver and resolved by matches a second
driver plays from those specs (zero-core's coordinator), the token network's actions by identity
on layout draft_v2, the human-pick yardstick, the co-training presets."""
import json
import os

import numpy as np
import pytest

from mtgz.az import drafted, games as G


def an_event(players, seed, fmt='bo3'):
    """A `team_draft` event of random picks and decks at its deferred terminal (the bindings)."""
    E = G.engine()
    g = E.Game('team_draft', json.dumps({'format': fmt}), players, seed)
    rng = np.random.default_rng(seed)
    while not g.over:
        legal = list(g.legal())
        g.step(int(legal[rng.integers(len(legal))]))
    assert g.deferred
    return g


def test_the_glue_maps_payloads_to_drafted_mtg_specs():
    E = G.engine()
    for players, n, fmt in ((8, 12, 'bo3'), (4, 4, 'bo3'), (4, 4, 'bo1')):
        g = an_event(players, players + 1, fmt)
        payload = g.deferred_payload()
        p = json.loads(payload)
        specs = drafted.episodes(payload)
        assert len(specs) == n
        for spec, m in zip(specs, p['matches']):
            s = json.loads(spec)
            assert s['max_turns'] == 60 and s['format'] == fmt and s['seed'] == m['seed']
            a, b = m['players']
            assert m['seats'][0] % 2 == 0 and m['seats'][1] % 2 == 1            # the A player is mtg seat 0
            for k, pl in (('p0', a), ('p1', b)):
                assert s['decks'][k] == pl['deck'] and len(pl['deck']) == 40
                assert s['decks'][k + '_sideboard'] == pl['sideboard'] and len(pl['deck'] + pl['sideboard']) - \
                    sum(x in ('Plains', 'Island', 'Swamp', 'Mountain', 'Forest') for x in pl['deck']) == 45
                assert s['decks'][k + '_first_picks'] == pl['first_picks'] and len(pl['first_picks']) == 3
                assert s['decks'][k + '_elsewhere'] == pl['elsewhere']
            assert len(a['elsewhere']) == {8: 306, 4: 132}[players]                # 348 or 174 known, less this opponent's 42
            mg = E.Game('mtg', spec, 2, 0)                                      # the engine takes the spec
            assert mg.to_move is not None and not mg.deferred
        # results: rows of the matches -> resolve's input; the event takes it
        rows = [{'result': [1.0, 0.0] if k % 3 else [0.5, 0.5], 'scores': [2, 0], 'over': k != 1} for k in range(n)]
        res = drafted.results(payload, rows)
        assert json.loads(res)[0] == {'result': [0.5, 0.5], 'scores': [2.0, 0.0]}
        g.resolve(res)
        assert not g.deferred and abs(sum(g.result()) - 1) < 1e-6


def test_a_malformed_result_is_refused():
    g = an_event(4, 3)
    payload = g.deferred_payload()
    good = [{'result': [0.0, 1.0], 'scores': [0, 2]}] * 4
    assert drafted.results(payload, good)
    for bad in (good[:3], good + good[:1], [{'result': [0.3, 0.7], 'scores': [0, 0]}] + good[1:],
                [{'result': [1.0, 0.0, 0.0], 'scores': [1, 0, 0]}] + good[1:], [{'scores': [0, 2]}] + good[1:]):
        with pytest.raises(ValueError):
            drafted.results(payload, bad)
    for bad in ('{}', '{"event": {}, "matches": []}', 'not json'):
        with pytest.raises(ValueError):
            drafted.episodes(bad)
    # the engine refuses a malformed resolve too, and the event stays at its deferred terminal
    with pytest.raises(ValueError):
        g.resolve(json.dumps([{'result': [1, 1], 'scores': [0, 0]}] * 4))
    assert g.deferred


def test_events_park_and_resolve_through_two_drivers():
    """A random-seat draft driver parks 2v2 and 4v4 events; their matches, queued as the glue's
    specs into a `queue_only` mtg driver of heuristic seats (zero-core's coordinator), resolve them
    with the real outcome."""
    from zero.cotrain import BIG, Coordinator, run_pair
    for players, events in ((4, 6), (8, 2)):
        r = G.seat_dict('random')
        pdrv = G.Driver('team_draft', [[r] * players] * events, spec='{}', concurrency=events, threads=2, seed=5,
                        max_steps=10_000)
        h = G.seat_dict('heuristic')
        rdrv = G.Driver('mtg', [[h, h]], spec='{"format": "bo3"}', concurrency=8, threads=2, seed=6,
                        max_steps=60_000, rolling=True, queue_only=True)
        co = Coordinator(drafted.episodes, drafted.results)
        run_pair(co, pdrv, {}, rdrv, {}, resume=(BIG, 0))
        rows = pdrv.take_results()
        mrows = co.take_rows()
        assert len(rows) == events and pdrv.pending == 0
        n = {4: 4, 8: 12}[players]
        assert len(mrows) == events * n and all(m['over'] for m in mrows)
        assert {m['tag'].split('/')[0] for m in mrows} == {str(r_['gid']) for r_ in rows}
        for row in rows:
            res = row['result']
            assert abs(sum(res) - 1) < 1e-6 and len(res) == players
            team_a, team_b = res[0] * players / 2, res[1] * players / 2
            assert (team_a, team_b) in ((1, 0), (0, 1), (0.5, 0.5))
            assert row['scores'][0] + row['scores'][1] <= n                        # match wins of A and B


def test_token_network_scores_cards_by_identity():
    """Layout draft_v2's `id_actions`: a pack slot scores its card's pick, a pool slot adding its card
    to the deck next (zero-core's TokenNet, M6 part 3; ENGINE.md 47); DONE and the counts are generic."""
    import torch
    from zero.net import make_net
    info = G.info('team_draft')
    torch.manual_seed(0)
    m = make_net(info, 'team_draft', 'tokens', 32, 1, heads=2)
    assert m.n_id == 2 and m.ptr.out_features == 2
    with torch.no_grad():
        m.gen.weight.zero_()
        m.gen.bias.zero_()
        m.ptr.weight.zero_()
        m.ptr.bias.copy_(torch.tensor([1.0, 2.0]))          # pack slots, pool slots
    E = G.engine()
    g = E.Game('team_draft', '{}', 4, 9)
    rng = np.random.default_rng(1)
    seen = {'pick': 0, 'build': 0}
    while not g.over:
        legal = list(g.legal())
        p = g.to_move
        x = np.asarray(g.encode(p))
        with torch.no_grad():
            pol = m(torch.from_numpy(x)[None], torch.tensor([4]))[0][0].numpy()
        if len(legal) > 1 and x[0] == 1.0:                                         # a pick: the pack's cards
            assert all(pol[a] == 1.0 for a in legal)
            seen['pick'] += 1
        elif len(legal) > 1 and x[52] == 1.0:                                      # the next pool card, or DONE
            assert all(pol[a] == 2.0 for a in legal if a < 768) and pol[768] == 0.0
            seen['build'] += 1
        g.step(int(legal[rng.integers(len(legal))]))
    assert seen['pick'] > 100 and seen['build'] > 20


def test_human_pick_agreement_on_a_few_drafts(tmp_path, monkeypatch):
    from mtgz.az import human_picks as H
    if not os.path.exists(H.data_path()):
        pytest.skip(f'no held-out drafts at {H.data_path()} (tools/data/08_heldout_drafts.py)')
    import torch
    from zero.net import make_net, save
    from zero.run import DEFAULTS, Run
    monkeypatch.setenv('MTGZ_HUMAN_PICKS_DRAFTS', '3')
    drafts, pos = H.positions(H.data_path(), 3)
    assert len(drafts) == 3 and 60 < len(pos) <= 3 * 45
    E = G.engine()
    for j, k, elig, h in pos[::17]:
        g = E.Game('team_draft', H.spec(drafts, j, k), 8, 0)
        legal = list(g.legal())
        assert g.to_move == 0 and len(legal) == 15 - k % 15 and set(elig) <= set(legal) and h in elig
    run = tmp_path / 'r'
    (run / 'models').mkdir(parents=True)
    torch.manual_seed(1)
    save(make_net(G.info('team_draft'), 'team_draft', 'tokens', 16, 1, heads=2), str(run / 'models' / 'iter_0000.pt'))
    cfg = dict(DEFAULTS, game='team_draft', players=[8])
    r = H.agreement(Run(str(run), cfg), 0)
    assert r['n'] == len(pos) and r['drafts'] == 3 and r['pct'] and 0 <= r['value'] <= 1
    assert 0.05 < r['refs']['random'] < 0.5 and r['refs']['strongest card'] > r['refs']['random']
    assert H.agreement(Run(str(run), dict(cfg, players=[4])), 0) is None            # 4v4 positions only


def test_cotraining_presets():
    from zero.cotrain import resolver_config
    from zero.run import DEFAULTS, PRESETS
    for name in ('team_draft-4v4', 'team_draft-gpu-smoke', 'team_draft-smoke'):
        cfg = dict(DEFAULTS, **PRESETS[name])
        rc = resolver_config(cfg)
        assert cfg['game'] == 'team_draft' and cfg['resolver'] == 'mtg' and rc['game'] == 'mtg'
        assert cfg['yardstick'] == 'consensus' and rc['yardstick'] == 'heuristic'
        assert json.loads(rc['spec'])['format'] == 'bo3' and json.loads(cfg['spec'])['build_sims_div'] == 2
        assert cfg['arch'] == 'tokens' and rc['arch'] == 'tokens'
    run = PRESETS['team_draft-4v4']
    assert (run['players'], run['games'], run['sims'], run['sims_fast'], run['p_full']) == ([8], 48, 64, 16, 0.25)
    rc = resolver_config(dict(DEFAULTS, **run))
    assert (rc['sims'], rc['sims_fast'], rc['sims_minor'], rc['hidden'], rc['blocks']) == (64, 16, 4, 128, 4)
    assert (run['resolver_conc'], run['draft_eval_net_every'], rc['eval_every']) == (768, 4, 2)
    # the play network's evaluation on the Mac (ENGINE.md 48), the draft network's here; the CPU smoke
    # evaluates everything here
    remote = {n: (PRESETS[n].get('eval_remote', ''), resolver_config(dict(DEFAULTS, **PRESETS[n]))['eval_remote'])
              for n in ('team_draft-4v4', 'team_draft-gpu-smoke', 'team_draft-smoke')}
    assert remote == {'team_draft-4v4': ('', 'mac'), 'team_draft-gpu-smoke': ('', 'mac'), 'team_draft-smoke': ('', '')}
    # remote self-play (ENGINE.md 52): opt-in presets only; the running presets do not change
    sp = {n: (PRESETS[n].get('selfplay_remote', ''), resolver_config(dict(DEFAULTS, **PRESETS[n]))['selfplay_remote'])
          for n in ('team_draft-4v4', 'team_draft-gpu-smoke', 'team_draft-smoke', 'team_draft-4v4-mac')}
    assert sp == {'team_draft-4v4': ('', ''), 'team_draft-gpu-smoke': ('', ''), 'team_draft-smoke': ('', ''),
                  'team_draft-4v4-mac': ('', 'mac')}
    mac4 = PRESETS['team_draft-4v4-mac']
    assert {k: v for k, v in mac4.items() if k != 'resolver_cfg'} == dict(
        {k: v for k, v in run.items() if k != 'resolver_cfg'}, draft_eval_net_remote='mac')
    assert mac4['resolver_cfg'] == dict(run['resolver_cfg'], selfplay_remote='mac')
    # the draft network's play-network-played evaluation on the Mac (ENGINE.md 53): the -mac preset only
    assert [n for n in PRESETS if PRESETS[n].get('draft_eval_net_remote')] == ['team_draft-4v4-mac']
    assert PRESETS['mtg-4-mac'] == dict(PRESETS['mtg-4'], games=200, selfplay_remote='mac')
    assert all(not PRESETS[n].get('selfplay_remote') for n in PRESETS if not n.endswith('-mac'))
