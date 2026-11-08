# MTG Zero

An **AlphaZero-style self-play AI for Magic: The Gathering**: a complete rules engine written in
Rust, trained by pure self-play on the shared framework
[zero-core](https://github.com/greggg230/zero-core). No human games or picks are used as
training signal. Human data only serves as a yardstick.

* **The engine** plays every card of LSV's **PowerLSV cube** (all 546). The
  [Comprehensive Rules](https://magic.wizards.com/en/rules) win wherever they disagree with
  anything else. The cube's official rulings were triaged, and each one about engine behaviour
  became an engine test (1,225 of them, `docs/rulings.md`). Every choice a player makes is an
  explicit action the AI decides, including targets, modes, X, mana payment and trigger order.
* **Two games** are registered with zero-core:
  * `mtg`: one game, or a best-of-three match with sideboarding, between two decks. Decks are
    random two-colour decks of cube cards, decks built from drafted pools, or a fixed
    constructed decklist.
  * `team_draft`: a 4v4 **team cube draft** (3 packs of 15, eight seats) with deckbuilding.
    Its result comes from `mtg` matches between the drafted decks, played by a second network
    (co-training).
* **The Legacy Izzet Delver mirror** is a side project in the same repo: both seats play one
  pinned 75 (`data/decks/izzet_delver_8016541.json`). Its runs are where most of the recent
  search and training work happened. Watch it play, see its training curve, or play against it
  at **[delverai.greggg230.com](https://delverai.greggg230.com)**.

Coding agents (Claude Code, Codex, ...): also read [`AGENTS.md`](AGENTS.md).

## Prerequisites

| | Version we use | Notes |
|---|---|---|
| Access | | Read access to **both** `greggg230/mtg-zero` and `greggg230/zero-core`. The submodule clones zero-core from GitHub. |
| Git + **Bash** | | The build scripts are bash. On Windows, use **Git Bash**. |
| Rust | stable, 1.95 | [rustup.rs](https://rustup.rs). Windows needs the MSVC Build Tools, macOS needs the Xcode command line tools, Linux needs a C compiler. |
| Python | **3.13** | PyO3 0.23 builds for up to 3.13, so **3.14 won't work**. `python` must be the interpreter you'll run with. To build for another one, set `PYTHON=/path/to/python` for `build_py.sh`. |
| Python packages | torch 2.11, numpy 2.3 | `pip install torch numpy psutil flask pytest`. CPU-only torch is enough for everything up to real training runs. |
| GPU | optional | NVIDIA + CUDA torch for the real presets (`mtg-3`, `delver-mirror`, `team_draft-4v4`) |
| Disk / RAM | a few GB for builds | Real runs write GBs of training data per iteration. A smoke run needs ~300 MB. |

## Quick start (about 10 minutes)

```bash
git clone --recurse-submodules https://github.com/greggg230/mtg-zero.git
cd mtg-zero

python -m venv ~/.venvs/zero                 # one venv works for every zero repo
source ~/.venvs/zero/bin/activate            # Git Bash on Windows: source ~/.venvs/zero/Scripts/activate
pip install torch numpy psutil flask pytest

# Windows only: the scripts default to the maintainer's E: drive. Pick a target dir.
export CARGO_TARGET_DIR="$PWD/target"
# macOS only: the Python extension must resolve Python's symbols at load time
export RUSTFLAGS="-C link-arg=-undefined -C link-arg=dynamic_lookup"

bash tools/build_py.sh                       # ~2 min: inits/checks the pinned zero-core, builds mtgz/_engine.{pyd,so}
python -m pytest tests -q                    # ~2 min, ~70 tests (one skips without the private human-pick data)
python -m mtgz.az.run --run runs/smoke1 --preset mtg-smoke     # ~45 s: 3 tiny training iterations on CPU
```

`build_py.sh` ends with `mtgz._engine OK: ... | games: ['toy', 'mtg', 'team_draft']`. The smoke
run logs each iteration's self-play, training, results against the `heuristic` and `random`
bots, and Elo, then prints `done`. Everything it wrote is in `runs/smoke1/`:

| File | What |
|---|---|
| `log.txt` | the human-readable log |
| `metrics.jsonl` | one JSON line per iteration: `selfplay`, `train` (losses), `eval_yard` (vs the heuristic), `eval_random`, `elo` |
| `models/iter_NNNN.pt`, `best.pt` | checkpoints |
| `config.json` | every setting of the run (saved: restarting the same command resumes the run) |

Forgot `--recurse-submodules`? `tools/build_py.sh` initialises the submodule itself.

## Next steps

```bash
# watch runs in a browser (reads runs/*; default port 5340, binds 0.0.0.0)
python -m mtgz.az.dashboard --host 0.0.0.0

# the Delver mirror instead of random cube decks
python -m mtgz.az.run --run runs/delver-smoke1 --preset delver-smoke

# every run setting, with its default
python -m mtgz.az.run --help

# two checkpoints head to head (seats alternate; prints score, Elo difference).
# h2h plays the package's default spec (random cube decks). For Delver checkpoints,
# pass --spec with the "spec" value from the run's config.json.
python -m mtgz.az.h2h runs/smoke1/models/iter_0003.pt runs/smoke1/models/iter_0001.pt --games 20 --sims 8

# record games of a checkpoint for the replay viewer (runs/<run>/replays/*.json.gz)
python -m mtgz.az.replays runs/smoke1/models/iter_0003.pt --games 2

# stop a run cleanly after its current iteration, or pause / resume it within seconds
touch runs/smoke1/STOP
python tools/pause_run.py runs/smoke1            # --resume, --status
```

### Presets

A preset is a named bundle of run settings (`mtgz/az/__init__.py`). Flags you pass override
it, and a run's saved `config.json` overrides the preset when you restart.

| Preset | What | Hardware |
|---|---|---|
| `mtg-smoke` | the end-to-end check: tiny network, few simulations, random cube decks | CPU, ~1 min |
| `delver-smoke` | the same on the Delver mirror | CPU, ~1 min |
| `team_draft-smoke` | co-training end to end: 2v2 drafts with tiny draft and play networks | CPU, ~5 min |
| `mtg-1`, `mtg-2`, `mtg-2-mlp` | the early cube runs (MLP; the token network; its MLP control) | CPU / GPU |
| `mtg-3` | the cube with a 128 x 4 token network (transformer over cards) | GPU |
| `mtg-4` | `mtg-3` with best-of-three matches and sideboarding from 45-card pools | GPU |
| `delver-mirror` | the Legacy Izzet Delver mirror with `mtg-3`'s settings | GPU |
| `team_draft-gpu-smoke`, `team_draft-4v4` | the 4v4 team-draft co-training run, and its GPU smoke | GPU (optionally a play-network start via `--resolver-init`) |
| `*-mac` | the maintainer's MacBook as a remote self-play worker | ignore |

## How it's built

```
data/cards/*.ir.json ──(card_ir: generated serde types)──▶ engine::CardDb (cards lowered to an effect VM)
                                                               │
engine::Game ── every decision a typed action, resumable resolution, determinize for hidden info
   │
engine/src/az ── `impl SearchGame` for `mtg` and `team_draft`: action ids, the encoding
   │              (token "objects" for zero-core's transformer: layout_v5.json, layout_draft_v2.json),
   │              chance keys, bots (`heuristic` = the Elo-1000 yardstick, `random`, `consensus`)
py/ ── binding crate ──▶ mtgz._engine ──▶ mtgz.az.* (= zero-core's Python modules + MTG glue)
```

* **Cards are data.** The card IR (`data/cards/powerlsv.ir.json`, plus `legacy.ir.json` for the
  Delver cards outside the cube) is exported by the oracle-text parser of the separate
  **mtg-bot** project and checked in. You don't need mtg-bot to build, test or train; you need
  it only to change card data. An IR field the engine doesn't implement is a hard error, never
  silently ignored.
* **Hidden information** works by determinization: `determinize(viewer)` resamples everything
  the viewer can't see, consistent with what it has seen. Shuffles, draws and random discards
  are chance events with keys (`docs/ENGINE.md` section 10).
* **No HashMaps, threads or callbacks inside `Game`**, so every game replays exactly from its
  seed and actions. Behaviour digests (`engine/tests/digest.rs`) fingerprint thousands of
  games, and any engine change that alters play shows up in them.

## Repository map

| Path | What |
|---|---|
| `engine/` | the Rust rules engine: `src/` (state, turn flow, casting, payment, the effect VM, combat, SBAs, triggers, layers, hidden information), `src/az/` (the zero-core games, encoding, bots, team draft), `tests/` (100+ files: rules tests named by CR rule, card tests, rulings, interactions, fuzz, digests) |
| `card_ir/` | Rust types generated from the IR schema (`tools/codegen/gen_card_ir.py`; don't hand-edit `generated.rs`) |
| `py/` | the PyO3 binding crate (`mtgz._engine`: zero-core's driver + the games) |
| `mtgz/` | the Python package: `mtgz/__init__.py` puts the pinned zero-core on `sys.path`; `mtgz/az/` registers the game package (presets, specs, layouts) and holds the shims (`mtgz.az.run` IS `zero.run`) plus MTG glue (`decklists`, `evaldecks`, `drafted`, `human_picks`, `belief`, `replays`) |
| `data/` | card IR + schema, decklists, the cube's official rulings, card values |
| `tools/` | `build_py.sh`, `gate.py` (tiered verification), `search_smoke.py`, fuzz farm, run control (`supervise`, `pause_run`, `kill_run`, `fork_run`, `watch_run.sh`), analysis scripts |
| `tests/` | pytest: the bindings, the loop on MTG, presets, team draft glue, smoke iteration |
| `docs/` | see below |
| `third_party/zero-core` | the pinned framework (git submodule) |
| `runs/` | training runs (gitignored; docs mention the maintainer's runs, which aren't in the repo) |

## Docs: what to read when

| You want to... | Read |
|---|---|
| the goals, decisions and milestones | [`DESIGN.md`](DESIGN.md) (§0 decisions, §2 the team draft, §3 the engine, §7 milestones, §8 the Delver mirror) |
| how the engine works | [`docs/ENGINE.md`](docs/ENGINE.md) §1-13 (architecture, objects, the agenda, priority, casting, the effect VM, triggers, SBAs, hidden information, the action layout, combat) |
| run or write tests | `docs/ENGINE.md` §15 |
| port a card or a mechanic | `docs/ENGINE.md` §16 (the recipe), `docs/card_ir.md`, `docs/ir_defects.md` |
| the zero-core game, encoding and network | `docs/ENGINE.md` §21-22, §60 (layout v5) |
| the team draft | `DESIGN.md` §2, `docs/ENGINE.md` §41-47 |
| the Delver mirror and its experiments | `DESIGN.md` §8, `docs/ENGINE.md` §54 onward |
| what is tested against which official ruling | `docs/rulings.md`, `docs/interactions.md` |
| rules simplifications that were removed, and the few left | `docs/shortcuts.md` |
| the human draft data (yardsticks only) | `docs/data_m1.md`, `docs/research/` |
| EoE sealed: the 17lands yardstick pools and the Play Booster generator (`mtgz/sealed/`) | `docs/sealed_data.md` |
| the training loop and every run setting | zero-core's `docs/az.md` (in `third_party/zero-core/docs/`) |

`docs/ENGINE.md` is about 16,000 lines, written as a lab notebook: one numbered, dated section
per milestone or feature, each with its tests and measurements. Read §1-16 for the
architecture. After that, search it for the card, rule or setting you care about.

## Testing

```bash
cargo test --release -p engine -p card_ir      # the engine's tests (use --release: what the gates run); ~25 min the first time, mostly compiling 100+ test binaries
cargo test --release -p engine --test fuzz     # random games: panics, invariants, replay determinism
FUZZ_GAMES=20000 cargo test --release -p engine --test fuzz    # before merging an engine change
python -m pytest tests -q                      # Python side
python tools/search_smoke.py                   # zero-core's search drives real games with a uniform evaluator
python tools/gate.py --help                    # the tiered gates: which checks a change needs
```

Debug builds turn on extra invariant checks (`cfg!(debug_assertions)`). If something fails only
in a debug build, compare it with `--release` and report it. The `fuzz` build profile
(`--profile fuzz`) runs at release speed with those checks on.

## The maintainer's setup (you can ignore it)

The docs often mention the maintainer's machines. None of it is required:

* `E:/...` paths: the default Windows build dir (set `CARGO_TARGET_DIR`), and the human-pick data
  behind the draft yardstick (`MTGZ_HUMAN_PICKS`; its test skips without it). The scripts in
  `tools/data/` (the CubeCobra / 17lands pipeline) have hard-coded paths.
* The MacBook remote: the `*-mac` presets, `tools/mac.sh`, run settings `eval_remote` /
  `selfplay_remote` = `mac`. Leave those empty and everything runs on your machine.
* The hub, Tailscale, port numbers, `zerodash`, and `tools/delverai_publish.py` (the publisher
  for delverai.greggg230.com, which needs an upload key).
* Run names like `delver1`, `td1`, `r3-2b-beliefsearch` refer to the maintainer's `runs/`, which
  isn't in git.
