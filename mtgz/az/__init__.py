"""MTG Zero's AlphaZero stack = zero-core (third_party/zero-core, pinned; its docs/az.md and
docs/search.md) + what is MTG's, registered here as the process's game package: the game `mtg`
(engine/src/az, docs/ENGINE.md 21-22, 36: one game of Magic between two random 40-card decks of the
supported PowerLSV cube cards, or with `"format": "bo3"` a best-of-three match with sideboarding
between games, decks built from random card pools), its object layout for zero-core's token
network (arch 'tokens': `layout_v4.json`, written by the engine's `encode::layout_json`), the game
`team_draft` (docs/ENGINE.md 41, 43, DESIGN.md 2.3: a team cube draft and deckbuilding up to the
matches, its deferred terminal; its layout `layout_draft_v1.json`), the run
presets (mtg-smoke, mtg-1, mtg-2, mtg-2-mlp, mtg-3, mtg-4), the default spec, the evaluation spec
and the dashboard's title / port.

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

from .evaldecks import eval_spec
from .mac import RunCommitRemote

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Self-play: two new random two-colour decks per game (engine az/decks.rs), London mulligan,
# the game is a draw after 60 turns (both players' turns; ~3% of heuristic-vs-heuristic games).
# Evaluation: 16 fixed deck pairs (evaluation set seed 1).
SPEC = '{"max_turns": 60, "eval_decks": 16, "eval_seed": 1}'
EVAL_SPEC = eval_spec(SPEC)

# The objects of encode layout v4 for zero-core's token network (docs/ENGINE.md 22, 35)
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'layout_v4.json'), encoding='utf-8') as _f:
    LAYOUT = json.load(_f)
# The team draft's objects (DESIGN.md 2.3.5, docs/ENGINE.md 41: layout draft_v1, the pack and pool slots
# and 13 bags; its `id_actions` key is M6 part 3's and ignored by the pinned core)
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'layout_draft_v1.json'), encoding='utf-8') as _f:
    LAYOUT_DRAFT = json.load(_f)
# The team draft's default spec: 3 packs of 15, min(T, 3) rounds of best-of-three matches (the seats,
# 2T, are the run's `players`: [8] for 4v4, [4] for 2v2). No preset yet: zero-core's deferred hook and
# co-training are in (M6 part 2, ENGINE.md 43); its matches as `mtg` specs and the presets are part 3
# (DESIGN.md 5.1, 7.1).
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

PACKAGE = register(GamePackage(
    'mtgz', 'MTG Zero', ROOT, engine='mtgz._engine', run_module='mtgz.az.run', game='mtg',
    specs={'mtg': SPEC, 'team_draft': SPEC_DRAFT}, dashboard_port=5340, hub_id='mtg_zero', env_prefix='MTGZ',
    presets=PRESETS, tokens={'mtg': LAYOUT, 'team_draft': LAYOUT_DRAFT}, eval_specs={'mtg': eval_spec},
    # remote evaluation (run setting eval_remote='mac', zero.remote; docs/ENGINE.md 44): the MacBook
    # (M4 Max, 16 cores, no CUDA) evaluates with exactly the run's code: its checkout
    # ~/mtg-zero-wt/run-<sha12> of the commit of the run's checkout (`bash tools/mac.sh
    # --eval-checkout` there), checked when the run starts and before every evaluation (mtgz/az/mac.py).
    # CPU inference (MPS: twice the time at these batches of ~6-13 leaves), 3 inference workers of 4
    # torch threads each (ENGINE.md 44.4: the Elo matches 325 s, against 575 s with one worker of 12)
    remotes={'mac': RunCommitRemote(ROOT, host='greggkeithley@100.95.248.116', home='/Users/greggkeithley',
                                    python='/Users/greggkeithley/.venvs/dominion-zero/bin/python',
                                    workdir='/Users/greggkeithley/mtgz-remote', env={'OMP_NUM_THREADS': '4'},
                                    overrides={'infer_device': 'cpu', 'train_device': 'cpu', 'threads': 12,
                                               'groups': 2, 'infer_workers': 3})},
))
