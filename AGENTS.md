# Notes for coding agents working on mtg-zero

Read [`README.md`](README.md) first: setup, the quick start, the repo map and which doc covers
what. This file is about **changing** the repo without breaking it. The framework underneath
has its own rules in zero-core's
[`AGENTS.md`](https://github.com/greggg230/zero-core/blob/main/AGENTS.md).

## Setup

```bash
export CARGO_TARGET_DIR="$PWD/target"      # one target dir per checkout, never shared (see Pitfalls)
export RUSTFLAGS="-C link-arg=-undefined -C link-arg=dynamic_lookup"   # macOS only (no build.rs adds them)
bash tools/build_py.sh                     # rebuild mtgz/_engine after ANY Rust change: Python uses the copied .pyd/.so
cargo test --release -p engine --test az   # quick engine check (the full suite: drop --test az)
python -m pytest tests -q                  # ~2 min
python -m mtgz.az.run --run runs/smoke1 --preset mtg-smoke     # ~45 s end-to-end loop
```

Python 3.13 (not 3.14) with `torch numpy psutil flask pytest`. Use Git Bash on Windows.

## Project decisions you must not undo

* **Pure self-play.** Human data (CubeCobra, 17lands, Draft Ranker, rotisserie picks) is a
  yardstick only, never a training target, reward or prior (`DESIGN.md` §0).
* **The Comprehensive Rules and the Oracle text are the spec.** When anything disagrees with
  them (the Python mtg-bot engine, an old test, a convenient simplification), the rules win,
  and the difference gets written down. Behaviour a card's Oracle text doesn't support is a
  bug, even if a test expects it: fix the test.
* **Every choice is an action.** An effect that needs a choice (target, mode, X, a may, which
  card, the order of triggers, damage assignment, which mana pays) suspends and asks through a
  `DecisionKind`. It never picks for the player. A choice among hidden cards names
  definitions, not slots (`docs/ENGINE.md` §10-11).
* **Cards are data, with no silent drops.** A card is supported only when the engine
  implements every part of its IR. Unknown or unimplemented fields `deny` loudly. Never add
  per-card special cases: a new `Op` is named after the rules action and cites the CR. Never
  wrap a missing feature in a fallback that approximates it.
* **Determinism.** `Game` has no `HashMap`, threads, callbacks or interior mutability, and
  randomness comes only from the seeded RNG or chance keys. A game replays exactly from its
  seed and actions.

## Before you change things

* **Engine rules or cards.** Follow `docs/ENGINE.md` §16 (porting a card). Add a test per
  behaviour that names the CR rule it checks (`fn cr_704_5j_the_legend_rule_for_planeswalkers`
  style). Check the card against its Oracle text. For official rulings, see
  `docs/rulings.md` (`// ruling: <hash8>` in the test). Add newly supported cards to
  `engine/tests/support.rs::SUPPORTED`. Before merging, the `suite` gate runs the full engine suite and a sharded
  2,000-game fuzz per pool (`python tools/fuzz_shards.py --pool <pool> --games 2000`, hidden decklists included); the
  20k depth runs continuously on the farm, not in the gate (the user's decision, `docs/ENGINE.md` 167).
* **Card data (the IR).** Cards come from the oracle-text parser in `ir/mtgz_ir` (moved from
  mtg-bot, `docs/ENGINE.md` 169). A card the parser reads wrong is fixed at the parser, with a
  test in `tests/ir/`, never patched in the JSON; then `python tools/card_ir.py export`
  regenerates `data/cards/*.ir.json`, the schema and `card_ir/src/generated.rs` together, and
  `python tools/card_ir.py check` (run by `tests/ir/test_pipeline.py`) proves they match
  (`docs/card_ir.md`). A changed IR or `generated.rs` is an engine change (engine tier).
* **Behaviour digests.** `cargo test --release -p engine --test digest -- --ignored --nocapture`
  hashes 2,500 fixed games against `engine/tests/data/digests.txt`. A change meant to keep
  behaviour (a refactor, a speed-up) must leave it unchanged. A change meant to alter play
  re-records it on purpose (`DIGEST_WRITE=1`), and the commit message and the ENGINE.md section
  state the old -> new hash. `DIGEST_ENCODING=0` leaves the encodings out, for encoding-only
  changes. Those games are the cube's best-of-one: `--test digest_formats` (same flags) hashes
  constructed best-of-three, the legacy decks, EoE and zero-core's driver against
  `engine/tests/data/digests_{constructed,eoe,driver}.txt`, with the same rules (`docs/ENGINE.md` 148).
* **The encoding (what the network sees).** A layout change (feature counts, token objects,
  action slots) makes existing checkpoints incompatible. zero-core's encoding guard refuses to
  resume them (exit 78). New layouts are additive versions (v4 -> v5 only added things, so a
  v4 network widens onto v5 exactly with `tools/fork_run.py`). Hidden information must never
  leak into a player's encoding: the public-state gate tests (`public_state_reaches_the_encoding`,
  `the_v5_facts_encode_no_hidden_information` in `engine/tests/az.rs`, `docs/ENGINE.md` §58)
  check it.
* **The zero-core pin.** `third_party/zero-core` is a submodule pinned to one commit, and
  `build_py.sh` refuses any other. To change the core: commit on a zero-core branch, push it
  to GitHub (a pin must point at a commit that exists there), stage the new gitlink here
  (`git add third_party/zero-core`), and gate it. `ZERO_CORE_UNPINNED=1` builds an unpinned
  core while you iterate. Never squash or rebase a zero-core branch that something pins.

## Gates: pick the tier from your diff

`python tools/gate.py <tier>`, run from inside a checkout of the commit being gated (a
worktree you create; gate.py never makes one). `--help` lists each tier's exact recipe,
`--dry-run` shows the plan, and `--no-mac` skips the maintainer's MacBook steps. A tier's
essential inputs (the expected digests, a checkpoint, the checkout before the change, the sha
the agent tested) are required; a step you skip on purpose (`--allow-skip <step>`,
`--record-digest`, `--no-mac`) turns the last line into `GATE PASSED WITH SKIPS ...`, naming it
(`docs/ENGINE.md` 91).

| Your change | Tier |
|---|---|
| docs, or tools not on the run path | `docs` |
| Python run loop, settings, presets | `python` |
| encoding only, layout unchanged | `encoding` |
| engine rules or cards | `suite` on your branch's commit (the full engine suite + the sharded 2k fuzz per pool above; its report `gates/<sha12>-suite.json` is the evidence), then `engine --tested-sha <that sha>` |
| zero-core search or behaviour | `search-core` |
| an opt-in zero-core feature, defaults byte-identical | `optin-core` |
| moving a run onto an already-verified commit | `repin` |

**Farm compatibility is required** (the user, 2026-11-14: every build is farm-compatible by default). Friends' PCs
run the release `mtgz-worker` (zero-farm), and it must play and encode exactly as the Python run does. The engine,
encoding, search-core, optin-core and repin tiers build the release worker from your commit and run
`tools/farm_compat.py` (step `farm-compat`: the worker's self-play vs zero-core's Python remote worker, and its game
records replayed by the trainer's path, both byte-identical, on the Delver best-of-three dealt preset). It cannot be
skipped. A change that breaks it is fixed in the worker or the engine, never by loosening the check; a deliberate
behaviour change moves the worker with it (the worker is this repo's `worker/` crate on the same engine). Before a farm
run attaches the farm or moves to a new commit, `tools/farm_gates.py` runs the full gates 2 and 3 on that run's real
config and checkpoint (`docs/ENGINE.md` 114.8, 155).

**Beside a live run the gate works off the desktop** (`docs/ENGINE.md` 166). When a supervisor is live on this machine
(pb2 and friends), `gate.py` sends its pure-CPU steps (cargo tests, digests, the engine suite, the fuzz, pytest, the
run-loop and search smokes; not zero-core's own tests, whose goldens are the desktop's) to the MacBook through `tools/mac.sh` (`nice`, at most two at once there)
and runs the desktop's own heavy steps (builds, the Windows worker, farm compat, private-data tests) each holding a slot
of `tools/slotd.sh`. Same checkers and evidence; the plan's `placement:` line says where each step ran. `--no-mac`
keeps everything here (throttled); `--offload local|mac` and `--throttle on|off` override the automatic choice. Any
other heavy command you run by hand beside a live run goes through the throttle too:
`bash tools/slotd.sh cargo test --release -p engine --test az` (state and settings in `E:/mtg-zero-state`;
`bash tools/slotd.sh --status`).

**Gate in a warm worktree** (`docs/ENGINE.md` 167), not a new one: `python tools/warm_pool.py run --sha <sha> -- python
tools/gate.py <tier>` checks the commit out into one of a few fixed worktrees with their own fixed target dirs, so cargo
rebuilds only what changed, and the Python module and the release worker come from the artifact cache
(`tools/zcache.py`) when the commit changed no Rust input. gate.py runs a tier's independent steps at once, each where
the placement above puts it. Never edit in a warm worktree.

Don't re-run a full suite on a sha that already passed it: each tier writes `gates/<sha12>-<tier>.json`
(`docs/ENGINE.md` 143). The checkout must be exactly its commit: an untracked file under `engine/`,
`mtgz/`, `tests/`, `tools/`, `data/`, ... refuses the gate like an uncommitted change. Say which tier
ran in your merge note or PR.

## Documenting a change

* `docs/ENGINE.md` is a lab notebook with one numbered section per milestone or feature, each
  with why, what, tests, digests and measurements. Append a new section, or a numbered subsection to
  the feature's own section. Numbers can be taken by open branches, so check them first.
* Rules shortcuts you introduce or remove go in `docs/shortcuts.md`. IR problems you find
  (wrong data from the parser in `ir/`) go in `docs/ir_defects.md`. Never work around them silently.
* Milestone status lives in `DESIGN.md` §7 (§8 for the Delver mirror).
* Commit messages say what changed, which sections document it, and the digests. An agent's
  commit ends with its own `Co-Authored-By:` trailer. Branches are named after the work
  (`m7c-tail2`, `delver-d1`, `feat/...`) and merged into `main` by the maintainer.

## Pitfalls

* **Stale engine module.** Python imports `mtgz/_engine.pyd` (`.so` on Linux and macOS), a
  copy that `build_py.sh` makes. After a Rust change, `cargo test` sees your code, but
  pytest and runs still see the old build until you rerun `build_py.sh`.
* **One `CARGO_TARGET_DIR` per checkout.** Two checkouts sharing one can silently reuse each
  other's build, because cargo's dep-info paths are workspace-relative. `gate.py` refuses a
  target dir another checkout built in.
* **Debug-only invariants.** Debug builds and the `fuzz` profile run extra checks
  (`cfg!(debug_assertions)`, e.g. "determinize changed the viewer's decision"). The gates run
  `--release`. A failure that shows only in debug is still a real bug: report it, don't
  silence it.
* **Live runs.** Never start, resume, edit or delete anything under someone's `runs/` unless
  asked. Runs train from their own pinned worktrees (`git worktree add --detach ...`), never
  from a checkout being edited, and a live run's workers import the code of that checkout.
* **Process-churning tests beside a live run.** Tests that start real supervisors or spawn processes in a loop carry
  `@pytest.mark.live_run_hazard` and are skipped while any supervisor is live on the machine (zero-core
  `docs/az.md` 51; a test storm killed a live run on 2026-11-14). Mark any new one. A gate names the fenced tests in
  its final line (a skip); on the Mac they run. `ZERO_ALLOW_LIVE_RUN_HAZARD=1` only on a machine with no one's run.
* **Maintainer-only paths.** `E:/...` defaults, the `mac` and `vast` remotes and `*-mac` presets,
  `tools/mac.sh`, `tools/vast*.py` (rented boxes on the maintainer's Vast.ai account: they cost
  money), `tools/data/*` (hard-coded paths), and `tools/delverai_publish.py` all
  belong to the maintainer's machines. Leave the remote settings empty. Tests that need
  private data skip without it.
* **Windows.** Bash scripts only (Git Bash). When a Python script rewrites a file, open it
  with `newline=''`, or text mode turns LF files into CRLF (whole-file diffs). Heredocs can
  lose a level of backslashes, so put scripts that contain backslashes in a file first.
