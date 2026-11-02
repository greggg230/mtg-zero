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
  `engine/tests/support.rs::SUPPORTED`. Before merging, run
  `FUZZ_GAMES=20000 cargo test --release -p engine --test fuzz`.
* **Behaviour digests.** `cargo test --release -p engine --test digest -- --ignored --nocapture`
  hashes 2,500 fixed games against `engine/tests/data/digests.txt`. A change meant to keep
  behaviour (a refactor, a speed-up) must leave it unchanged. A change meant to alter play
  re-records it on purpose (`DIGEST_WRITE=1`), and the commit message and the ENGINE.md section
  state the old -> new hash. `DIGEST_ENCODING=0` leaves the encodings out, for encoding-only
  changes.
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
`--dry-run` shows the plan, and `--no-mac` skips the maintainer's MacBook steps.

| Your change | Tier |
|---|---|
| docs, or tools not on the run path | `docs` |
| Python run loop, settings, presets | `python` |
| encoding only, layout unchanged | `encoding` |
| engine rules or cards | `engine` (plus the 20k fuzz above, run by you) |
| zero-core search or behaviour | `search-core` |
| an opt-in zero-core feature, defaults byte-identical | `optin-core` |
| moving a run onto an already-verified commit | `repin` |

Don't re-run a full suite on a sha that already passed it. Say which tier ran in your merge
note or PR.

## Documenting a change

* `docs/ENGINE.md` is a lab notebook with one numbered section per milestone or feature, each
  with why, what, tests, digests and measurements. Append a new section, or a numbered subsection to
  the feature's own section. Numbers can be taken by open branches, so check them first.
* Rules shortcuts you introduce or remove go in `docs/shortcuts.md`. IR problems you find
  (wrong data from the parser) go in `docs/ir_defects.md`. Never work around them silently.
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
* **Maintainer-only paths.** `E:/...` defaults, the `mac` remote and `*-mac` presets,
  `tools/mac.sh`, `tools/data/*` (hard-coded paths), and `tools/delverai_publish.py` all
  belong to the maintainer's machines. Leave the remote settings empty. Tests that need
  private data skip without it.
* **Windows.** Bash scripts only (Git Bash). When a Python script rewrites a file, open it
  with `newline=''`, or text mode turns LF files into CRLF (whole-file diffs). Heredocs can
  lose a level of backslashes, so put scripts that contain backslashes in a file first.
