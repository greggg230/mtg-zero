"""MTG Zero's AlphaZero stack = zero-core (third_party/zero-core, pinned; its docs/az.md and
docs/search.md) + what is MTG's, registered here as the process's game package: the game `mtg`
(engine/src/az, docs/ENGINE.md 21: one game of Magic between two random 40-card decks of the
supported PowerLSV cube cards), the run presets (mtg-smoke, mtg-1), the default spec and the
dashboard's title / port.

The modules of this package are the core's under the usual names, so every command has the same
CLI as in the other games: `python -m mtgz.az.run`, `python -m mtgz.az.dashboard --host 0.0.0.0
--port 5340`, `python -m mtgz.az.h2h`, ... (each `mtgz.az.<m>` IS `zero.<m>`; `mtgz.az.run` adds
the evaluation deck set, `mtgz.az.evaldecks`). Importing this package stays light (no torch, no
numpy): the tools import `mtgz.az.runctl`.

Yardsticks: the built-in Rust bots `heuristic` (the Elo anchor, 1000) and `random`
(engine/src/az/bots.rs). Self-play games use new random decks every game; evaluation matches
(vs the yardstick, vs random, Elo matches) use the fixed evaluation deck set (spec "decks":
"eval", `mtgz.az.evaldecks`), so win rates and Elo compare across iterations.
"""
import os

from zero.package import GamePackage, register

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Self-play: two new random two-colour decks per game (engine az/decks.rs), London mulligan,
# the game is a draw after 60 turns (both players' turns; ~3% of heuristic-vs-heuristic games).
# Evaluation: 16 fixed deck pairs (evaluation set seed 1).
SPEC = '{"max_turns": 60, "eval_decks": 16, "eval_seed": 1}'

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

PACKAGE = register(GamePackage(
    'mtgz', 'MTG Zero', ROOT, engine='mtgz._engine', run_module='mtgz.az.run', game='mtg',
    specs={'mtg': SPEC}, dashboard_port=5340, hub_id='mtg_zero', env_prefix='MTGZ',
    presets=PRESETS,
))
