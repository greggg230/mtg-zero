"""MTG Zero's AlphaZero stack = zero-core (third_party/zero-core, pinned; its docs/az.md and
docs/search.md) + what is MTG's, registered here as the process's game package: the game `mtg`
(engine/src/az, docs/ENGINE.md 21-22, 36: one game of Magic between two random 40-card decks of the
supported PowerLSV cube cards, or with `"format": "bo3"` a best-of-three match with sideboarding
between games, decks built from random card pools), its object layout for zero-core's token
network (arch 'tokens': `layout_v5.json`, written by the engine's `encode::layout_json`), the game
`team_draft` (docs/ENGINE.md 41, 43, 45, DESIGN.md 2.3: a team cube draft and deckbuilding up to
the matches, its deferred terminal; its layout `layout_draft_v2.json`, ENGINE.md 47), the run
presets (mtg-smoke, mtg-1, mtg-2, mtg-2-mlp, mtg-3, mtg-4, mtg-4-mac; the co-training presets
team_draft-smoke, team_draft-gpu-smoke, team_draft-4v4, team_draft-4v4-mac; the constructed Delver
mirror delver-mirror and delver-smoke, docs/ENGINE.md 54), the default spec, the
evaluation spec, the dashboard's title / port, the remote 'mac' (run setting eval_remote: the
MacBook evaluates with exactly the run's code; `mtgz/az/mac.py`, docs/ENGINE.md 44; run setting
selfplay_remote, or a co-training run's resolver_cfg selfplay_remote: it self-plays too, ENGINE.md
52), and for co-training runs
`team_draft`'s deferred glue (its payload's matches as `mtg` specs and their results back:
`mtgz.az.drafted`) and its other yardstick (the agreement with held-out human picks:
`mtgz.az.human_picks`).

The modules of this package are the core's under the usual names, so every command has the same
CLI as in the other games: `python -m mtgz.az.run`, `python -m mtgz.az.dashboard --host 0.0.0.0
--port 5340`, `python -m mtgz.az.h2h`, ... (each `mtgz.az.<m>` IS `zero.<m>`). Importing this
package stays light (no torch, no numpy): the tools import `mtgz.az.runctl`.

Yardsticks: the built-in Rust bots `heuristic` (the Elo anchor, 1000) and `random`
(engine/src/az/bots.rs). Self-play games use new random decks every game; evaluation matches
(vs the yardstick, vs random, Elo matches) use the fixed evaluation deck set (the spec with
"decks": "eval", `mtgz.az.evaldecks.eval_spec`: the package's evaluation spec of the game, which
zero-core's loop uses unless the run sets `eval_spec`), so win rates and Elo compare across
iterations.
"""
import json
import os

from zero.package import GamePackage, register

from . import decklists
from .evaldecks import eval_spec
from .mac import RunCommitRemote

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Self-play: two new random two-colour decks per game (engine az/decks.rs), London mulligan,
# the game is a draw after 60 turns (both players' turns; ~3% of heuristic-vs-heuristic games).
# Evaluation: 16 fixed deck pairs (evaluation set seed 1).
SPEC = '{"max_turns": 60, "eval_decks": 16, "eval_seed": 1}'
EVAL_SPEC = eval_spec(SPEC)

# The objects of encode layout v5 for zero-core's token network (docs/ENGINE.md 22, 35, 60; v4 + additions
# only: a v4 network widens onto it exactly, `python tools/fork_run.py <v4 checkpoint> <new run>`). layout_v4.json
# stays: the layout of the v4 runs' checkpoints (their configs carry it too).
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'layout_v5.json'), encoding='utf-8') as _f:
    LAYOUT = json.load(_f)
# The team draft's objects (DESIGN.md 2.3.5, docs/ENGINE.md 41, 47: layout draft_v2, the pack and pool
# slots and 13 bags; `id_actions`: a slot scores its card's identity, the pick or adding it to the deck
# next, zero-core's TokenNet since M6 part 3, ENGINE.md 45)
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'layout_draft_v2.json'), encoding='utf-8') as _f:
    LAYOUT_DRAFT = json.load(_f)
# The team draft's default spec: 3 packs of 15, min(T, 3) rounds of best-of-three matches (the seats,
# 2T, are the run's `players`: [8] for 4v4, [4] for 2v2). The co-training presets are below (M6 part 3,
# ENGINE.md 45).
SPEC_DRAFT = '{}'

PRESETS = {
    # the end-to-end check on CPU (tiny network, few sims; a game has ~300-500 decisions with
    # >= 2 options, docs/ENGINE.md 21.5)
    'mtg-smoke': dict(game='mtg', spec=SPEC, players=[2], iters=3, games=16, conc=16, sims=8, sims_fast=2,
                      p_full=0.25, temp_moves=30, hidden=32, blocks=1, batch=128, min_window_samples=0,
                      min_steps=10, lr_warmup_steps=0, yardstick='heuristic', eval_games=8, eval_sims=4,
                      eval_random_games=4, elo_games=4, ref_games=8, threads=2, max_steps=20_000),
    # the first small self-play run (M5a, docs/ENGINE.md 21.6; CPU inference while other
    # projects' runs use the GPU): a few iterations at modest sims, evaluated against the
    # heuristic (the Elo anchor) and random. Rolling self-play keeps 384 games in flight (CPU
    # inference needs large batches: 96 games in flight gave batches of ~22 and 5.7k leaves/s).
    # runs/mtg1 started with 400 games and 40-game Elo matches and continued from iteration 2
    # with these (Elo matches with both seats searching batch badly on CPU)
    'mtg-1': dict(game='mtg', spec=SPEC, players=[2], iters=10, games=800, rolling=1, conc=384, sims=32,
                  sims_fast=8, p_full=0.25, temp_moves=60, chance_k=0, hidden=256, blocks=3, batch=512, reuse=4.0,
                  window=4, window_grow=1, min_window_samples=20_000, value_q=0.5, value_q_warmup=3,
                  yardstick='heuristic', eval_games=100, eval_sims=32, eval_random_games=50, elo_games=20,
                  ref_games=100, threads=8, groups=2, torch_threads=6, max_steps=20_000),
}
# The second run (M5b, docs/ENGINE.md 22): the token network (a transformer over the cards:
# width 96, 3 layers; CPU inference on 4 threads of 2 torch threads each, 4 batches in flight),
# 64 / 16 simulations, mana payments (minor decisions) at 4 and never recorded, completed-Q
# policy targets, value targets 0.9 search estimate, 2 reuses, evaluation as mtg-1 (100 games vs
# the heuristic and 50 vs random at 32 simulations on the evaluation decks), Elo against the 2
# previous checkpoints in one driver. mtg-2-mlp: the same with mtg-1's MLP (the control).
_M2 = dict(game='mtg', spec=SPEC, eval_spec=EVAL_SPEC, players=[2], iters=40, games=600, rolling=1, conc=768,
           sims=64, sims_fast=16, p_full=0.25, sims_minor=4, temp_moves=60, chance_k=0, policy_target='cq',
           cq_c_visit=50.0, cq_c_scale=1.0, arch='tokens', hidden=96, blocks=3, heads=4, ff=192, batch=512,
           reuse=2.0, lr=1e-3, window=6, window_grow=1, min_window_samples=30_000, value_q=0.9, value_q_warmup=2,
           yardstick='heuristic', eval_games=100, eval_sims=32, eval_random_games=50, elo_games=20, elo_opponents=2,
           elo_together=1, ref_games=100, threads=6, groups=4, infer_workers=4, torch_threads=2, max_steps=20_000,
           keep_data=6)
PRESETS['mtg-2'] = _M2
PRESETS['mtg-2-mlp'] = dict(_M2, arch='mlp', hidden=256, blocks=3)
# the third run (M7c, docs/ENGINE.md 22.6, 27-28): the 467-card pool, layout v3, the M7c heuristic
# (activations) as the yardstick, mimalloc search, and the GPU for inference and training, which
# lets the token network grow (128 x 4). A layout v3 sample is ~60 KB (float16), ~3 GB per 600-game
# iteration: a 12 GB window budget.
PRESETS['mtg-3'] = dict(_M2, hidden=128, blocks=4, heads=4, ff=256, threads=8, groups=2, infer_workers=1,
                        infer_device='cuda', train_device='cuda', ram_budget_gb=12.0, elo_games=20,
                        elo_opponents=2, elo_together=1)

# the fourth run (M8b, docs/ENGINE.md 36): the episode is a best-of-three match with sideboarding
# between games (the terminal result is the match's), decks built by the heuristic deckbuilder from
# random 45-card pools (the rest of the pool the sideboard); evaluation (the yardstick, random, Elo)
# plays matches on the fixed evaluation pools (the evaluation spec keeps the format). Otherwise
# mtg-3's settings; a match is ~2.5 games, so three times the step cap.
SPEC_BO3 = '{"max_turns": 60, "eval_decks": 16, "eval_seed": 1, "format": "bo3"}'
PRESETS['mtg-4'] = dict(PRESETS['mtg-3'], spec=SPEC_BO3, eval_spec=eval_spec(SPEC_BO3), max_steps=60_000)

# Co-training (M6 part 3, DESIGN.md 2.3.9 and 5.1, docs/ENGINE.md 45, zero-core docs/az.md 11): the run's
# game is `team_draft` (the draft network: picks and deckbuilding, at the run root), its events' matches
# are the self-play of the play network (the resolver `mtg`, mtg-4's settings, in <run>/resolver/), each
# event's outcome its matches' (its deferred terminal resolved by `mtgz.az.drafted`). Draft search: picks
# 64 / 16 simulations, deckbuilding half of that (the spec's build_sims_div 2: 32 / 8), p_full 0.25,
# completed-Q targets, temperature for every decision of an event (temp_moves 1000: a 4v4 event has at
# most ~700 searched decisions, so deckbuilding is sampled by visit count too: ENGINE.md 47; lower it
# with --temp-moves on a relaunch once deckbuilding has settled); the draft network the token network
# of mtg-3 (128 x 4, actions by identity). An
# iteration = 48 events resolved (576 matches, mtg5's scale), 64 events in flight drafting, the matches
# 768 in flight with backpressure at 2 x 768 queued or playing. Evaluation: the draft network every
# iteration against `consensus` (the anchor: human pick strength and the heuristic deckbuilder) and
# random teams, Elo against its checkpoints 1, 2, 4 and 8 back, the matches played by the heuristic
# (one play agent for the Elo), and every 4th iteration against `consensus` with the matches played by
# the play network (eval_yard_net); the agreement with held-out human picks (a curiosity); the play
# network every 2nd iteration as mtg-4 (its yardstick the heuristic, the fixed evaluation pools) ON THE
# MAC (resolver_cfg eval_remote 'mac', zero-core docs/az.md 11.5, docs/ENGINE.md 48: ~9 min there, ~30-70
# on the shared desktop; the run's checkout needs `bash tools/mac.sh --eval-checkout` before it starts);
# the draft network's evaluation stays on the desktop. resolver_init: the play network's start, a
# checkpoint of runs/mtg5 chosen at launch (--resolver-init); '' = a fresh play network.
SPEC_DRAFT_RUN = '{"build_sims_div": 2}'
_DRAFT_NET = dict(arch='tokens', hidden=128, blocks=4, heads=4, ff=256)
PRESETS['team_draft-4v4'] = dict(
    game='team_draft', spec=SPEC_DRAFT_RUN, players=[8], iters=100, games=48, rolling=1, conc=64, sims=64,
    sims_fast=16, p_full=0.25, temp_moves=1000, chance_k=0, policy_target='cq', cq_c_visit=50.0, cq_c_scale=1.0,
    **_DRAFT_NET, batch=512, reuse=4.0, lr=1e-3, window=8, window_grow=1, min_window_samples=20_000, value_q=0.5,
    value_q_warmup=5, keep_data=12, ram_budget_gb=6.0, yardstick='consensus', eval_games=32, eval_sims=32,
    eval_random_games=16, eval_every=1, elo_games=16, elo_opponents=4, elo_together=0, ref_games=32, threads=8,
    groups=2, infer_workers=1, torch_threads=2, infer_device='cuda', train_device='cuda', max_steps=20_000,
    resolver='mtg', resolver_cfg={'preset': 'mtg-4', 'eval_every': 2, 'eval_remote': 'mac'}, resolver_init='',
    resolver_conc=768,
    pending_max=0, draft_eval_net_every=4)
# The GPU smoke (M6 part 3): the 4v4 run's networks and search at a few events per iteration, modest
# concurrency (other runs share the GPU), small evaluations, the play network's on the Mac as the run's
# (ENGINE.md 48); the play network from --resolver-init.
PRESETS['team_draft-gpu-smoke'] = dict(
    PRESETS['team_draft-4v4'], iters=3, games=8, conc=16, eval_games=4, eval_random_games=2, elo_games=2,
    elo_opponents=2, ref_games=4, min_window_samples=0, draft_eval_net_every=2, resolver_conc=96, pending_max=0,
    resolver_cfg={'preset': 'mtg-4', 'conc': 96, 'eval_games': 8, 'eval_random_games': 4, 'elo_games': 4,
                  'ref_games': 8, 'min_window_samples': 0, 'eval_every': 1, 'eval_remote': 'mac'})
# The CPU smoke (M6 part 3): 2v2 events (4 seats, 4 matches each: both rotations) with tiny token
# networks for both games, few simulations, a few events per iteration; no GPU, everything evaluated here
# (the remote's end-to-end check adds "eval_remote": "mac" to its resolver_cfg, ENGINE.md 48).
# Remote self-play on the Mac (docs/ENGINE.md 52, 56, zero-core docs/az.md 5.3 and 11.6), opt-in: the
# MacBook and the desktop pull an iteration's games from one shared queue (zero-core's zero/workqueue.py),
# each with its measured throughput's share in flight, all with the run's checkpoint i-1 (the Mac's
# samples come back as compressed shards into data/iter_i/), or in a co-training run the Mac plays a
# share of the play network's queued matches. mtg-4-mac: mtg-4 with the Mac (its 600 matches an
# iteration are the total of both machines since ENGINE.md 56: the same data per iteration, sooner);
# team_draft-4v4-mac: team_draft-4v4 with the play network's
# matches on the Mac too, and the draft network's evaluation whose matches the play network plays
# (eval_yard_net: run setting draft_eval_net_remote, ENGINE.md 53). The run's checkout needs `bash tools/mac.sh --eval-checkout` before it starts
# (the same checkout as the evaluations). A running run turns it on at a restart with a flag
# (ENGINE.md 52.5): presets fill only settings a run has not saved.
PRESETS['mtg-4-mac'] = dict(PRESETS['mtg-4'], selfplay_remote='mac')
PRESETS['team_draft-4v4-mac'] = dict(
    PRESETS['team_draft-4v4'], resolver_cfg=dict(PRESETS['team_draft-4v4']['resolver_cfg'], selfplay_remote='mac'),
    draft_eval_net_remote='mac')
PRESETS['team_draft-smoke'] = dict(
    game='team_draft', spec=SPEC_DRAFT_RUN, players=[4], iters=2, games=4, conc=4, sims=8, sims_fast=2, p_full=0.25,
    temp_moves=1000, policy_target='cq', arch='tokens', hidden=32, blocks=1, heads=2, ff=64, batch=64, min_steps=10,
    lr_warmup_steps=0, min_window_samples=0, yardstick='consensus', eval_games=4, eval_sims=4, eval_random_games=2,
    elo_games=2, ref_games=4, threads=2, max_steps=20_000, resolver='mtg', resolver_conc=16, pending_max=0,
    draft_eval_net_every=2,
    resolver_cfg={'preset': 'mtg-smoke', 'spec': SPEC_BO3, 'eval_spec': eval_spec(SPEC_BO3), 'arch': 'tokens',
                  'hidden': 32, 'blocks': 1, 'heads': 2, 'ff': 64, 'conc': 16, 'sims_minor': 2, 'max_steps': 60_000,
                  'eval_games': 4, 'eval_random_games': 2, 'elo_games': 2, 'ref_games': 4})

# The Legacy Izzet Delver mirror (docs/ENGINE.md 54, DESIGN.md 8): both seats the pinned 60 of
# data/decks/izzet_delver_8016541.json (i_b_TRUE, Legacy Challenge 32, 2026-10-23, 1st), best-of-one,
# no sideboards, the London mulligan, a draw after 60 turns; constructed play's card table (the cube's
# cards and Murktide Regent, Prismari Charm). Self-play and every evaluation (the yardstick, the M7c
# heuristic playing the same deck; random; Elo between iterations) play that one spec: the decks are
# fixed, so the evaluation spec is the spec. Otherwise mtg-3's settings (the last best-of-one run): the
# token network 128 x 4 on layout v5 (v4 until ENGINE.md 60; the legacy cards' identities 546..547 are in the free range),
# 64 / 16 simulations, mana payments at 4, completed-Q targets, 600 games an iteration (ENGINE.md 54.6
# for the measured cost and the configuration recommended while td1 holds the GPU).
SPEC_DELVER = decklists.mirror_spec(decklists.IZZET_DELVER, max_turns=60)
PRESETS['delver-mirror'] = dict(PRESETS['mtg-3'], spec=SPEC_DELVER, eval_spec=SPEC_DELVER)
# the end-to-end check of the mirror on CPU (mtg-smoke's tiny network and sims)
PRESETS['delver-smoke'] = dict(PRESETS['mtg-smoke'], spec=SPEC_DELVER, eval_spec=SPEC_DELVER)

PACKAGE = register(GamePackage(
    'mtgz', 'MTG Zero', ROOT, engine='mtgz._engine', run_module='mtgz.az.run', game='mtg',
    # the belief target (docs/ENGINE.md 66, zero-core docs/az.md 15): the decider's opponent's hand by
    # card identity, learned as a distribution by an auxiliary head (run setting aux_belief_w, 0 = off);
    # replays of a checkpoint with the head show it per network decision (`mtgz.az.belief.render`)
    aux={'mtg': [{'name': 'belief', 'width': 1024, 'loss': 'multinomial'}]},
    replay_aux={'mtg': 'mtgz.az.belief:render'},
    # team_draft's deferred results and its other yardstick (M6 part 3, docs/ENGINE.md 45)
    deferred={'team_draft': 'mtgz.az.drafted:GLUE'}, evals={'team_draft': 'mtgz.az.human_picks:EVALS'},
    specs={'mtg': SPEC, 'team_draft': SPEC_DRAFT}, dashboard_port=5340, hub_id='mtg_zero', env_prefix='MTGZ',
    presets=PRESETS, tokens={'mtg': LAYOUT, 'team_draft': LAYOUT_DRAFT}, eval_specs={'mtg': eval_spec},
    # remote evaluation (run setting eval_remote='mac', zero.remote; docs/ENGINE.md 44): the MacBook
    # (M4 Max, 16 cores, no CUDA) evaluates with exactly the run's code: its checkout
    # ~/mtg-zero-wt/run-<sha12> of the commit of the run's checkout (`bash tools/mac.sh
    # --eval-checkout` there), checked when the run starts and before every evaluation (mtgz/az/mac.py).
    # CPU inference (MPS: twice the time at these batches of ~6-13 leaves), 3 inference workers of 4
    # torch threads each (ENGINE.md 44.4: the Elo matches 325 s, against 575 s with one worker of 12).
    # Remote self-play (run setting selfplay_remote 'mac', zero.remoteplay; ENGINE.md 50.3, 52): 2 worker
    # processes (one is bound by its own Python loop: 1 / 2 / 3 processes 19.4k / 33.0k / 37.1k leaves/s),
    # each 768 matches in flight, 12 search threads, 4 groups of 192 with 4 inference workers of 3 torch
    # threads (50.3's best), CPU inference, shards of 8192 samples compressed; at nice 5 (above the fuzz
    # farm's and the studies' 19, below anything at normal priority); mirrors in ~/mtgz-selfplay; they
    # back off while an evaluation holds ~/mtgz-remote/.eval.lock (always) or the other games' remotes'
    # locks on the Mac (their evaluations share the cores)
    remotes={'mac': RunCommitRemote(ROOT, host='greggkeithley@100.95.248.116', home='/Users/greggkeithley',
                                    python='/Users/greggkeithley/.venvs/dominion-zero/bin/python',
                                    workdir='/Users/greggkeithley/mtgz-remote', env={'OMP_NUM_THREADS': '4'},
                                    overrides={'infer_device': 'cpu', 'train_device': 'cpu', 'threads': 12,
                                               'groups': 2, 'infer_workers': 3},
                                    selfplay={'workdir': '/Users/greggkeithley/mtgz-selfplay', 'procs': 2, 'nice': 5,
                                              'locks': ['/Users/greggkeithley/kbz-remote/.eval.lock',
                                                        '/Users/greggkeithley/domz-remote/.eval.lock'],
                                              'env': {'OMP_NUM_THREADS': '3'},
                                              'overrides': {'infer_device': 'cpu', 'train_device': 'cpu', 'conc': 768,
                                                            'threads': 12, 'groups': 4, 'infer_workers': 4,
                                                            'torch_threads': 3, 'shard_rows': 8192}})},
))
