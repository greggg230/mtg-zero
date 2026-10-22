# Can mtg-zero reuse the mtg-bot Rust engine?

Research report, 2026-10-21. Read-only survey of `mtg-bot/rust` (crate `mtg_bot_engine`) as the possible base for a zero-core `SearchGame` covering the PowerLSV cube (546 cards).

**Path roots.** `MB/` = `C:\Users\Gregg Keithley\Documents\Claude\Projects\mtg-bot\` and `ZC/` = `C:\Users\Gregg Keithley\zero-core\`. Rust paths are relative to `MB/rust/src/`.

**What was surveyed.**
- Local `main` at `46efc8b9`. `origin/main` is 13 commits ahead, but none of them touch `rust/` (`git diff --stat main origin/main -- rust` is empty).
- The last Rust commit was 2026-10-03. Before that, the Rust work ran 2026-05-09 to 2026-06-01, across 696 commits touching `rust/src`.

**Compile probe.** A copy of the crate was built with `cargo check` under `CARGO_TARGET_DIR=E:/cargo-target/mtg-zero-research`, then deleted. Nothing in mtg-bot was built or modified.

---

## 0. Verdict

**Recommendation: (c) start fresh.** Use the old crate as a reference and an edge-case catalogue, not as code to extend or extract.

The crate is not a rules engine that Python happens to call. It is a library of rule *primitives* (cast, resolve the top of the stack, apply SBAs, deal combat damage, list legal moves as Python dicts).
- **The game loop is in Python.** A 1,533-line Python adapter runs the priority loop, step changes, the combat phases and even the one-land-per-turn rule.
- **Cards only arrive from Python.** Card definitions come in through a Python dict bridge that is built from the Python oracle parser.
- **The vocabulary is EoE's, not the cube's.** The effect vocabulary is Edge-of-Eternities-shaped. None of the 396 effect classes the Python parser added for the cube and Vintage Roto have a Rust counterpart.
- **It copied Python's mistakes on purpose.** The campaign's policy was to "converge Rust toward Python, never modify Python, regardless of CR-correctness". The parity evidence therefore certifies Python's quirks, not the Comprehensive Rules.
- **Players make almost no in-resolution choices.** Targets, modes, "may", scry/surveil and similar choices are made inside the engine (first legal target, mode 0, always yes, keep on top).
- **There are no Rust tests.** All 2,456 of its tests are Python tests that need the compiled extension and the Python engine.

Option (b), extracting modules, buys little. Even the "pure" helpers depend on a data model that has to change anyway:
- Card types, subtypes, keywords and mana symbols are `String`s.
- Power and toughness come from a hand-written sum over EoE-specific static fields, with no layer system.
- Errors are reported as `PyResult`.

What is worth carrying over is a handful of *ideas and checklists* (section 6.2) and the Python engine as a differential oracle, used with care (section 6.4).

---

## 1. Architecture and PyO3 coupling

### 1.1 Size and shape

| File | Lines | Notes |
|---|---:|---|
| `state_ops_resolve.rs` | 30,259 | 14,661 comment lines. `apply_effect` (`:2864`) is a **single 26,019-line function** that matches on `Effect`. |
| `cards.rs` | 23,217 | `Effect` enum (`:96-10517`), dict parsers (`parse_effect` `:13446`, `parse_printing` `:18107`), `Registry` pyclass (`:22773`). |
| `state.rs` | 5,357 | `GameObject`, `Player`, `StackItem`, `PendingDecision`, `GameState`. |
| `state_ops_legal.rs` | 4,411 | `legal_decisions`, which builds Python dicts. |
| 22 other `state_ops_*.rs` | ~23,500 | One `#[pymethods] impl GameState` block per feature area. |
| **Total** | **86,807** | **50,282 lines (58%) are `//` comments**, mostly parity narrative. Real code is ~36.5k lines. |

### 1.2 `GameState`

`#[pyclass(name = "GameState")] pub struct GameState` is at `state.rs:3061-3063`. It has 47 fields (`state.rs:3064-4046`):
- `players: [Player; 2]`. Each `Player` holds five zones of type `Zone = Arc<Vec<GameObject>>` (`state.rs:1022`): hand, library, battlefield, graveyard and exile.
- `registry: Arc<RegistryData>`: the frozen card table.
- `stack: Vec<StackItem>` and `stack_objects: Vec<GameObject>`.
- `attackers`, `blockers`, `delayed_triggers`, `pending_decisions` and `extra_turn_queue`.
- Five `pending_*_dispatches: Vec<u32>` trigger queues.
- Two `HashSet`s: `triggered_this_turn` and `loyalty_activated_this_turn`.
- `emblems`.
- About 12 `current_resolve_*` slots that hold scratch values while an effect resolves.
- `rng: Option<Py<PyAny>>` (`state.rs:3506`): a **Python `random.Random` object stored inside the state**.

There is no global object store and no id index. Looking up an object means a linear scan, for example `players.iter().flat_map(|p| p.battlefield.iter()).find(|o| o.id == obj_id)` (`state_ops_combat.rs:1072-1078`). There is no per-player land-drop counter: "Rust has no such field today ... skip the gate, emit for every Land in hand" (`state_ops_legal.rs:2867-2872`).

### 1.3 How deep the PyO3 coupling goes

- **68 `#[pymethods]` blocks** across the crate. This is enabled by pyo3's `multiple-pymethods` feature (`MB/rust/Cargo.toml:11-16`). The crate type is `cdylib` only.
- **The legal-move list is Python-typed.** `legal_decisions(&self, py, player_id) -> PyResult<Vec<Bound<'py, PyDict>>>` (`state_ops_legal.rs:2190-2194`) builds Python dicts directly, using helpers such as `pass_priority_dict` and `cast_spell_dict` (`:534-655`). No Rust-typed list of legal actions exists.
- **Applying a move goes through Python.** `apply_action(slf: Bound<Self>, action_dict: &Bound<PyDict>)` (`state_ops_actions.rs:300`) parses the dict and then calls its *own* Rust methods through Python attribute lookup, for example `slf.call_method1("play_land", ...)` (`:354`), `"declare_attacker"` (`:399`) and `"declare_blocker"` (`:471`). There are 29 `slf.call_method*` / `slf.borrow*` sites in total.
- **Errors are Python-typed.** There are 255 `PyResult` and 105 `Bound<` occurrences, plus 628 `Py{Value,Key,Runtime,Index,Type}Error::new_err` raises.
- **Cards are parsed from Python dicts** (`cards.rs:13446`, `:18107`). There is no JSON or Rust-native card loader.
- **Shuffles can call back into Python.** `external_rng::next_below` calls Python's `random.Random._randbelow` once per draw (`external_rng.rs:57`) so that Rust shuffles match Python's Mersenne Twister. Without a Python RNG, it falls back to per-call xorshift64 seeds.

### 1.4 Clone + Send + Sync

- **The bounds already hold.** A probe, `fn needs<T: Clone + Send + Sync + 'static>() {} needs::<state::GameState>(); needs::<cards::RegistryData>();`, compiles cleanly. pyo3 0.23.5 also requires `Sync` for every pyclass (`pyo3-0.23.5/src/impl_/pyclass/assertions.rs:12,25-27`).
- **The one wrinkle is the RNG.** The hand-written `Clone` (`state.rs:4048-4060`) clones the stored Python RNG inside `Python::with_gil`. That is harmless when `rng` is `None`, but the field has to go.
- **Sizes** were read from the compiler: `GameObject` 104 B, `Player` 104 B, `GameState` 744 B, `StackItem` 184 B, `Effect` 72 B, `CardPrinting` 520 B.
- **Clone cost.** A clone copies 744 bytes and bumps 11 `Arc` counts (10 zones plus the registry). It then allocates once for each non-empty `Vec` or `HashSet` among about 16 such fields, which is usually 0-3 allocations mid-game. Measured: **0.35 µs per clone, versus 11.9 µs in Python** (`MB/docs/BENCHMARKING_FINDINGS.md:450`).
- **The cost is deferred to the first write.** The first write to a zone after a clone copies that whole zone's `Vec` (`Arc::make_mut`), plus each object's `ueot_modifiers` Vec if it has any.

**Conclusion for the `SearchGame` bounds.** These are cheap: drop the Python RNG field and the bounds are met. Everything else the trait needs is missing:
- a Rust-typed `legal_actions`;
- a `step` that runs to the next decision;
- `determinize`, `encode` and chance keys;
- a game loop.

Building those is not cheap (section 4).

### 1.5 The game loop lives in Python

`MB/src/mtg_bot/search/rust_engine_adapter.py` (1,533 lines) is what turns the primitives into a playable game:
- **Moving to the next decision.** `_advance_to_next_decision` (`:489-511`) calls `advance_phase` until a decision point.
- **The priority loop (CR 117.4).** `_run_priority_action` (`:531-660`): on a completed pass round with a non-empty stack it calls `resolve_top_of_stack()`, then `apply_sbas()`, then resets priority (`:544-553`). Otherwise it advances the phase.
- **Step-entry SBAs and combat damage.** `_run_step_enter_sbas` (`:386-487`) calls `deal_combat_damage` / `apply_sbas` / `clear_combat`.
- **Attack and block declarations.** `_finalize_attacker_phase` / `_finalize_blocker_phase` (`:1321-1445`) assemble per-creature choices into declarations. Eligible attackers and blockers are computed in Python (`:1230-1320`).
- **One land per turn.** This rule is enforced only by a Python-side filter (`:175-190`, `:940-960`).

Rust's own priority helper `complete_step_via_priority_round` (`state_ops_priority.rs:283-302`) still only *pops* the stack: it calls `state.stack.pop()` without resolving it.

The benchmark notes the consequence: "BOTH search-protocol adapters reimplement the priority/search loop in **Python** — only the engine *primitive calls* ... cross into Rust", so full games ran only 1.9-2.8× faster than Python (`BENCHMARKING_FINDINGS.md:454`).

---

## 2. Card model

### 2.1 How cards are defined

The crate uses a **closed vocabulary**: one enum variant per effect "shape". The variants are interpreted by one giant `match`.

- **`Effect`: 248 variants** (`cards.rs:96-10517`).
  - About 35 are named after cards, for example `SeedshipImpactEffect`, `DepressurizeEffect`, `PainForAllETBEffect`, `ArabellaAttackEffect`, `TerrapactIntimidatorETBEffect`, `ConsultTheStarCharts` and `TezzeretEmblemBeginCombat`.
  - Many more are one-card shapes with the parameter baked into the name, for example `DealDamageToOpponentCreatureByArtifactCount`, `TwoSubjectPumpAndDebuffByArtifactCount` and `PumpCreaturesOpponentsControlUntilCasterNextTurn`.
  - There are a few real combinators: `Sequence`, `MayDo`, `IfYouDo`, `IfNot`, `ChooseOneMode`, `ConditionalIfVoid`, `ConditionalIfCastFromHand` and `ConditionalIfSourcePowerAtLeast`.
- **Other enums and structs:**
  - `TargetFilter` has **2 variants** (`AnyPlayer`, `AnyCreature`; `cards.rs:90-94`).
  - `Event` has 17 variants (`cards.rs:10694`) and `Predicate` has 17 (`cards.rs:11011`).
  - There are also a `Condition` enum, `StaticAnthem`, `StaticSelfBonus` and a string-typed `StaticReplacement { event: String, transform_kind: String, transform_param: i32, transform_dest: Option<String> }` (`cards.rs:13232-13237`).
- **`CardPrinting`** (`cards.rs:12364`) has 46 fields:
  - `card_types`, `subtypes`, `supertypes` and `keywords` are `Vec<String>`.
  - Keywords are checked by string comparison (`battlefield_has_keyword(state, id, keyword: &str)`, `state_ops_combat.rs:1235`).
  - Several fields exist for a single card or mechanic, for example `aura_replaces_first_tokens_with_copies`, `restricts_noncreature_spells_to_one_per_turn`, `enters_tapped_unless_any_player_life_at_most`, `warp_castable_from_graveyard` and `station_threshold`.
- **`ManaCost`** is `{generic, white, blue, black, red, green, x_pips: u8}` (`cards.rs:39-47`). It has no hybrid, Phyrexian or snow mana, all of which the cube needs.

### 2.2 How the Rust side gets card definitions

Only from Python. Each card's oracle text is parsed in Python into a `CardPrinting`, then passed through the following chain:
1. `parity/trajectory.py` `_printing_to_card_def` (`:13689`) produces a `CardDef`.
2. `cross_engine._spec_card_to_printing_dict` turns that into a dict.
3. `mtg_bot_engine.Registry().register_printing(dict)` (`MB/src/mtg_bot/parity/cross_engine.py:15163-15190`) registers it.

A printing is admitted only if one of the `_printing_is_bridge_*` predicates matches it (`trajectory.py:5034`, `:5166`, `:7597`, `:8621`, ... `:12486`; about two dozen shape predicates).

The bridge code is large: `trajectory.py` is 16,020 lines and `cross_engine.py` is 25,188. Adding an effect took a four-step recipe across both languages (`MB/docs/SESSION_HANDOFF_2026-05-25-RUST-PARITY-CAMPAIGN.md:138`):
1. Rust variant plus dict parser in `cards.rs`.
2. Resolve arm in `state_ops_resolve.rs`.
3. Bridge arm in `cross_engine.py`.
4. Reverse serializer in `trajectory.py`.

### 2.3 Coverage relative to the cube

- **EoE-era vocabulary:** `MB/docs/RUST_EFFECT_COVERAGE.md:9-12` reports "210 of 236 Python Effect classes covered", with 248 Rust variants for 482 parser-claimed printings.
- **Today's Python vocabulary:** `MB/src/mtg_bot/effects/` defines about 948 classes. That count includes helper classes, so it is a ceiling on the number of Effect types.
  - Matching names against the Rust variants gives 214 matches.
  - **The 396 classes defined in `effects/generic/cube_*.py` and `roto_*.py` have 0 Rust counterparts.**
- **Cube mechanics Rust doesn't have:** cube keyword counts come from `MB/data/cube_power_lsv.json`. Rust has zero mentions of flashback (3 cube cards), delve (2), evoke (5), connive (2), splice (2), landcycling (5) and investigate (4).
  - Transform/MDFC, adventure, saga, split, class and prepare layouts are all present in the cube (`MB/docs/CUBE_POOL_COVERAGE.md:22-31`).
  - So are the Vintage staples: Force of Will, Time Vault, Demonic Consultation, Black Lotus, Time Walk, Oath of Druids, Sneak Attack, Tinker, Yawgmoth's Will, Phyrexian Metamorph, Karakas, Containment Priest and others.

---

## 3. Rules fidelity

| Subsystem | Rust state | Evidence |
|---|---|---|
| **Stack & priority (G7)** | **G7 is closed in the engine.** All spells now go on the stack; the old "harness drains both stacks after a cast" step (CE5) was removed. The loop that passes priority and resolves the stack is the Python adapter's (§1.5). Both engines give priority back to the **active player after any action**, including a non-active player's cast. CR 117.3c says the caster keeps priority. | Spells were pushed to the stack in slice S1 (creature cast, `state_ops_cast.rs:1161-1201,1417`), S4 (warp, `:1452-1530,1695`) and S5a (spell moved into `stack_objects`). G7 S1-S7, including CE5 removal, landed (`MB/docs/SOT_FLIP_CUTOVER_PLAN.md:704-707`). 300/300 games passed the shadow check with `rust_shadow` at step cap 20000 (`MB/s6_scale_verify_v13_post_session_substrate_resweep.json`). That run used EoE 17lands decks, `combat_strategy: attack_all` and `mulligans: 0`. Priority reset: Python `simulate.py:386` and Rust doc `state_ops_priority.rs:28-31`. (A Sept-2026 memory note still calls G7 "open"; the code and docs say it is closed.) |
| **Triggers** | Real trigger dispatch over 17 event kinds, with once-per-turn gates. **No APNAP ordering and no controller ordering choice.** Trigger targets are first-legal. | `dispatch_event` `state_ops_resolve.rs:1096`. Deferred APNAP: `:1068`. `first_legal_trigger_target` `:1665`. `MB/docs/ORDER_SENSITIVE_SURFACES.md:157-170`. |
| **Replacement / prevention** | A few one-off replacement effects (Green Sun's Zenith graveyard→library redirect, Loading Zone, Starfield Vocalist) through the string-typed `StaticReplacement`. Prevention is a single flag, `prevent_all_combat_damage_this_turn`. No general framework (CR 614/615). | `cards.rs:13232`; the `GameState` field list. |
| **Layers (CR 613)** | **None.** Power and toughness are a fixed sum: base (from overrides, else the printed value) + until-end-of-turn bonuses + net counters + aura bonuses + self bonuses + anthem bonuses. There are no timestamps or dependencies within a layer, and types and colors are not computed. Python has since gained effective types and colors (layers 4-5). Rust has not. | `state_ops_combat.rs:1072-1120` (`battlefield_creature_power_unswapped` `:1090`); mtg-bot commit `80dda74e` (Python layers 4-5). |
| **SBAs** | Covered: 704.5a, b, f, g, i (planeswalker loyalty), q (auras) and tokens ceasing to exist; checks repeat until nothing changes. **Missing: the legend rule** (no hits for 704.5j). Counters are one net `i32`, so +1/+1 and -1/-1 counters are never kept separate (704.5r). | `state_ops_sba.rs:9-112,576-1190`; `MB/docs/RUST_PARITY_GAPS.md:61-75` (G8). |
| **Combat** | First-strike and double-strike damage steps (`passes_for`), trample, deathtouch, menace, flying/reach, skulk. **The order in which an attacker assigns damage among its blockers is battlefield order, not the attacking player's choice.** The comment there calls this "technically a bug vs CR, but parity with observable Python is the reference here". Damage assignment is not a player decision. | `state_ops_combat.rs:1639`, `:2090-2630`, `:2140-2152`. Policy: `RUST_PARITY_GAPS.md:3169-3170` ("converge Rust toward Python, never modify Python, regardless of CR-correctness"). |
| **Mana** | `ManaPool` holds counts per color plus "any", and an ordered list of `ManaCoin`s (`state.rs:1045-1148`). Lands are auto-tapped by a heuristic, with an affordability check that reasons about which colors dual lands can make (`state_ops_mana.rs:591-700`, `state_ops_legal.rs:692-870`). Mana abilities are not player actions. Colors and symbols are strings. | Mana abilities "not implemented" in `MB/docs/SEARCH_LIMITATIONS.md` §4. |
| **Costs** | Kicker, warp, X (`x_pips`), impending, sacrifice-as-additional-cost, cost reductions, Room halves and Station. **No alternative or pitch costs, Phyrexian or hybrid mana, delve, or evoke.** | The `Action::CastSpell` fields (`actions.rs`) and `CardPrinting`. |
| **Hidden info and determinization** | **None in Rust.** Python's `rust_determinize.py` reshuffles which card is which among hidden hand and library objects through `set_object_printing_id`. Rust has no `publicly_known_to` set, only `publicly_known_to_caster_bits`, so reveals are ignored on the Rust path. Randomness is a Python callback or per-call xorshift seeds. There are no chance keys. | `MB/src/mtg_bot/search/rust_determinize.py:1-40`; `external_rng.rs`. |
| **Mulligans** | A London mulligan step, `mulligan_atomic` (`state_ops_mulligan.rs:488`), with the bottomed cards chosen by the caller. Scale runs used `mulligans: 0`. | `state_ops_mulligan.rs:12-65`. |
| **Choices made during resolution** | **Made by the engine.** Spell targets are "deterministic-first legal target(s)" chosen at cast time (`state_ops_cast.rs:2045-2052`). Pending choices (surveil, may, mode, scry, pick, sacrifice, ...) are drained by `auto_resolve_pending(state, "keep_all_on_top")` (`state_ops_decisions.rs:205-330`). That means always "yes", mode 0, pick the top card, keep on top. `Sequence` drains pending choices after every child (`state_ops_resolve.rs:14714-14718`). | `MB/docs/NN_RUST_DECISION_SOURCING_WIRING_DESIGN.md:368-380` (G3: "Rust makes these choices internally"). |

**Net.** Within the admitted EoE subset, Rust is faithful to *Python*, at state level and per step. It is not a CR-faithful engine for the cube: it lacks layers, a general replacement framework, APNAP ordering, the legend rule, player-ordered damage assignment, alternative costs and player-made resolution choices.

---

## 4. Decisions and a fixed u16 action space

**What exists.**
- **`Action`** (`actions.rs:33`) has 11 variants:
  - `PassPriority`, `PlayLand{card_id}` and `CastSpell{card_id, use_warp, kicked, cast_for_free, use_impending, cast_room_half}`.
  - `ActivateAbility{source_id, ability_index, target_creature_id, sacrifice_target_id, chosen_mode_idx, target_creature_id_2}`.
  - `TurnFaceUp`, `ActivateStation`, `UnlockRoomHalf`, `DeclareAttackers` and `DeclareBlockers`.
  - Per-creature **`AttackerDecision{attacker_id, attack}`** and **`BlockerDecision{blocker_id, attacker_id}`**.
- **`PendingDecision`** (`state.rs:2399`) has 15 variants (surveil, may-do, pick-card, choose-mode, if-you-do, manifest-dread, scry, tap-permanent, enchant-target, graveyard-card, may-discard, opponent-hand-card, sacrifice, ...). Each has a `submit_*_decision` method (`state_ops_decisions.rs:1649-2862`), but the play loop auto-resolves them (§3).

**Fit with `SearchGame`.** The ideas fit; the code does not.
- **Combat** is already factored per creature, which maps directly onto small action families. Python's codec uses `SLOT_ATTACKER_ATTACK/SKIP` and `SLOT_BLOCKER_BASE + i` with a cap of 8 attackers (`MB/src/mtg_bot/training/action_encoding.py:178,246-252`).
- **Python already has a positional codec.** It maps decisions to fixed slots: 517 slots in `canonical_v2` (`action_encoding.py:19-43,199-254`). The `feature/s2-resolution-decisions` branch adds 288 resolution slots (targets, modes, X, yes/no, ...) for 805 in total. That fits easily in u16, and it is the right template.
- **The Rust side is not shaped this way.** `Action` carries raw `u32` object ids. Targets are not part of `CastSpell`. Legal moves are Python dicts that are de-duplicated by printing name (`state_ops_legal.rs:2874-2878`). A new engine needs:
  - a choice state machine, so that `step(u16)` always takes one atomic choice (pick card, pick mode, pick target k, pick X, pay or decline, order triggers, assign damage);
  - legal-move generation that writes `u16`s directly;
  - a canonical object order, so slot `i` means the same thing in any two states that are equivalent (`canonical_v2` sorting is the precedent).

None of this exists in the crate, so it would be written from scratch in options (a) and (b) too.

---

## 5. Tests

- **No Rust-native tests.** `grep '#\[test\]' rust/src/*.rs` finds 0; there is no `#[cfg(test)]` and no `rust/tests/`. Because the crate is a pyo3 `extension-module` cdylib, `cargo test` cannot link, and there would be nothing to run anyway.
- **`MB/tests_rust/`** holds 50 Python files with 2,456 test functions (68,855 lines). They drive the `.pyd` and skip entirely when `mtg_bot_engine` isn't installed (`tests_rust/conftest.py:52`).
- **`MB/tests/`**: 382 Python test files import `mtg_bot_engine`, and 199 are cross-engine or per-card `*_parity.py` probes. Those probes drive the real Python-parsed printing through both engines and compare field views.
- **What this means.** Every assertion about the Rust engine depends on the Python engine, the Python bridge and a maturin-built `.pyd` in a shared venv. Concurrent rebuilds of that `.pyd` corrupted it in practice (`MB/docs/RUST_PARALLELIZATION_FEASIBILITY.md:24-33`). None of these tests carry over to a pure-Rust engine.

---

## 6. Recommendation

### 6.1 Why (c) start fresh, and not (a) extend or (b) extract

1. **The boundary is in the wrong place.**
   - A `SearchGame` needs `step` / `legal_actions` / `determinize` / `encode` in Rust.
   - The crate's loop, combat assembly, land limit and determinizer are in Python (§1.5, §3), and its legal moves and actions are Python dicts (§1.3).
   - Extending it means writing the turn engine anyway, while also removing about 68 pymethods blocks, 255 `PyResult` sites and 628 `PyErr` raises.
2. **The card model doesn't scale to 546 cards.**
   - It has 248 effect variants for about 480 EoE printings, about 35 of them card-named.
   - Targets are a 2-variant filter, types and keywords are strings, and card-specific booleans sit on `CardPrinting`.
   - It is fed only by a Python dict bridge with shape-admit predicates.
   - It covers 0 of the 396 cube and Roto effect classes.
   - Bug classes repeated arm by arm: the "controller of effect" fix landed one arm at a time (`RUST_PARITY_GAPS.md` I59, I61, I81). Nine sister arms were left using "first legal target" instead of the target bound at cast (`SOT_FLIP_CUTOVER_PLAN.md:945-960`).
3. **"Parity-proven" means proven against Python, not against the Comprehensive Rules.**
   - Python behaviors were mirrored on purpose: priority reset to the active player, battlefield-order damage assignment, no APNAP, net counters.
   - The new engine should follow the CR and treat Python as a fallible reference.
4. **Decisions are hidden.** Mid-resolution choices are resolved inside the engine. The Python side measured searched resolution decisions as worth **+12-13 points** of win rate (mtg-bot `feature/s2-resolution-decisions`, per project memory). A zero-core game must expose every choice.
5. **It is expensive to change.**
   - 58% of the lines are narrative comments.
   - The effect interpreter is one 26k-line function.
   - Every change re-pinned fingerprint baselines (`RUST_SPELL_STACK_SOURCE_ZONE_DESIGN.md:748-753`).
6. **There are no tests to inherit** (§5).
7. **Its hot paths are slow.** It clones well, but the hot paths do string compares and allocations, for example `Vec<String>` clones inside `can_pay_cost_set_based` (`state_ops_mana.rs:663-690`) and linear id scans. In zero-core the *whole* search runs in Rust, so engine cost per step matters far more than it did in mtg-bot. There, NN evaluation was 85% of wall time (`BENCHMARKING_FINDINGS.md:464-471`).

(b) would mean taking files such as `state_ops_combat.rs` or `state_ops_mana.rs` and porting them onto a new data model: typed card types and keywords, layered characteristics, a global object store and Rust errors. At that point you are re-writing them while reading the originals, which is (c).

### 6.2 What to carry over (as reference, re-typed)

| Item | Where | Why |
|---|---|---|
| Copy-on-write zones and cheap clones | `state.rs:1022`; `BENCHMARKING_FINDINGS.md:450` (0.35 µs) | Shows that per-simulation clones of the root state are affordable. A new engine should add an id-indexed object store and one global timestamp order. |
| Stack, resolution and leaving-the-stack semantics (CR 601/608, 608.2m) | `MB/docs/RUST_STACK_RESOLUTION_DESIGN.md` §1.3 (`:134-224`); `RUST_SPELL_STACK_SOURCE_ZONE_DESIGN.md` | A written-down contract for cast → stack → priority round → resolve → move the source. The S5a lesson: the stack must *own* the spell. |
| Combat damage step planner | `state_ops_combat.rs:1619-1660,2090-2630` | A checklist of the cases (first strike, double strike, trample with deathtouch, multiple blockers, a creature leaving combat mid-way). Make damage assignment a decision. |
| SBA loop | `state_ops_sba.rs:1131-1190` | The 704.3 repeat-until-stable loop and the order victims are collected in. Add 704.5j/r/c/d properly. |
| Mana affordability for multi-color sources | `state_ops_mana.rs:591-700`; `state_ops_legal.rs:692-870` | Needed to prune legal casts without enumerating every way to tap. |
| Combinators and predicates | `Sequence`, `MayDo`, `IfYouDo`, `ChooseOneMode`, `Conditional*`; `Predicate`/`StatePredicate` (`cards.rs:10535-11480`) | Seeds for a compositional effect IR. Do **not** carry over the card-named variants. |
| Edge-case catalogues | `RUST_PARITY_GAPS.md` (G/I/CE entries), `ORDER_SENSITIVE_SURFACES.md`, `RUST_EFFECT_RNG_AUDIT.md`, the G7 evidence docs | Turn each entry into a Rust rules test with the CR-correct expectation, not the Python one. |
| Python engine plus per-card tests and deck pools | `MB/src/mtg_bot/`, `MB/tests/test_<card>.py`, `MB/data/cube/power_lsv_decks.json.gz` | The differential oracle and a source of scenarios (§6.4). |

### 6.3 Lessons the new engine must not repeat

1. **Don't split the game loop across languages.** Bugs G5, G6 and the priority-player bug were all port gaps in the Python adapter (`SOT_FLIP_CUTOVER_PLAN.md:526-528,826-843`), and the split capped the speedup at 1.9-2.8× (`BENCHMARKING_FINDINGS.md:454`). The whole loop belongs in Rust behind `step`.
2. **Don't make a reference engine's quirks the spec.** The CE18 "regardless of CR-correctness" policy (`RUST_PARITY_GAPS.md:3169-3170`) is the opposite of the user's later rule that "new code is right" (memory `feedback_parity_new_code_is_right`). Where Python and the CR disagree, follow the CR and record the difference.
3. **Don't chase another language's RNG byte for byte.** That meant the Mersenne Twister vs xorshift fight, `_randbelow` callbacks (`external_rng.rs`), a `PyObject` stored in the state (`RUST_EFFECT_RNG_AUDIT.md`), and an open shuffle-order lane late in the campaign. Instead, let the state own a seedable, clone-safe RNG, emit canonical chance keys and reseed in `determinize`.
4. **Never reconcile in the harness.** The CE5 drain hid atomic resolution across the whole early evidence base (`RUST_STACK_RESOLUTION_DESIGN.md:192-205`). The user called this "false-green" (memory `project_mtg_bot_rust_stack_decision`).
5. **Build the stack zone on day one.** Retrofitting it touched seven surfaces at once (`RUST_SPELL_STACK_SOURCE_ZONE_DESIGN.md:604-635`).
6. **Every choice is a `step(u16)`.** That includes targets, "up to", modes, X, may/if-you-do, trigger order, damage assignment and division, discard, sacrifice, search, scry/surveil, mulligan bottoms, and mana payment wherever it matters. A first-legal default should be a *bot*, not engine behavior.
7. **No card-named effect variants.** Use one IR with shared target, controller and "you" resolution, so a fix applies to every card that uses it (see items 2 and 3 in §6.1).
8. **Use one canonical object order everywhere.** Per-player Vecs with cross-player order made up at snapshot time caused CE4, CE6, CE9 and CE11 (`ORDER_SENSITIVE_SURFACES.md:34-64`). Use one global timestamp and one iteration helper.
9. **Design the state faithfully from the start.** That means lands played per turn, counters by kind, reveal tracking (`publicly_known_to`), "this turn" trackers and effective characteristics computed by layer.
10. **Measure coverage per playable deck, not per card.** In May 2026 per-card PROBED counts kept rising while about 80% of real deck non-land slots were not admittable and "0 of 129,996 real decks" were fully admittable (`SOT_FLIP_CUTOVER_PLAN.md:349-356`). For mtg-zero, measure the share of the generated cube deck pool that is fully playable.
11. **Unsupported cards must fail loudly.** One silent no-op was `CastSpell` of a non-creature returning `Ok(false)` (`SPELL_CARD_BRIDGE_LEVER_DESIGN.md:85-115`). The model to copy is the C6 `LoaderAdmitPolicy.RAISE` (`SOT_FLIP_CUTOVER_PLAN.md:312-320`).
12. **Keep the core pure Rust, tested with `cargo test`, behind a thin binding.** Use zero-core's `engine_module!` (`ZC/docs/new_game.md:30-39`). This avoids maturin and the shared-venv `.pyd` races (`RUST_PARALLELIZATION_FEASIBILITY.md:24-33`).
13. **Keep process and docs light.**
    - About 45% of 3,400 mtg-bot commits were parity work (memory `project_mtg_bot_fresh_eyes_review_2026_09`).
    - The "6-10 sessions" and "4-6 sessions" estimates were missed (`SESSION_RETRO_2026_05_11.md:301-303`, `SESSION_RETRO_2026_05_12.md:254-255`).
    - "Projected" 1800/1800 pass rates were reported without being measured (`SESSION_2026_05_19_FLIP_READY_SYNTHESIS.md:133-142`).
    - Comments ran to 58% of the lines.
    - Measure, don't project, and keep design notes out of code comments.

### 6.4 Main risks of starting fresh, and how to handle them

- **Rules scope.** Power plus 546 cards needs real layers (Humility-style effects, type-changing, control-changing, copy effects such as Phyrexian Metamorph), replacement and prevention, alternative costs (Force of Will), extra turns (Time Walk, Time Vault), cast-from-graveyard (flashback, Yawgmoth's Will, Snapcaster Mage), and the DFC, adventure, saga, class and split layouts.
  - Mitigation: build the CR skeleton (zones, stack, priority, SBAs, layers, replacements, combat) before writing card effects.
  - Grow the card set by playable deck, with a hard admit gate.
- **The oracle is fallible.** The Python engine has known CR deviations (priority reset, no APNAP, damage order, mana abilities not implemented).
  - Run differential tests on *observable outcomes* in scripted scenarios, not on fingerprints of whole states.
  - Keep an explicit allowlist of "Python is wrong here" entries.
  - Never let a Python quirk become a Rust requirement without a CR check.
- **Action space and branching.** Exposing every choice means more decision points per game. On mtg-bot, searched resolution decisions cost -11% throughput.
  - Design the u16 families and the canonical slot order up front. Python's 805-slot `canonical_v3` layout is a good starting point.
  - Rely on zero-core's auto-stepping of single-option decisions (`ZC/docs/search.md:86-87`).
- **Hidden information.** Determinization has to respect reveals, tutors, library manipulation (Brainstorm, Jace) and cards whose identity is known.
  - The Python model (`MB/docs/HIDDEN_INFO_MODEL.md`) is a fair start, but it has gaps: no positional knowledge, and deferred handling of hand mixing.
  - Under zero-core rules, every revealed fact must also be keyed (`ZC/docs/search.md:90-96`).
- **Effort estimates.** The previous campaign blew past every estimate. Use milestones that can be measured: the share of playable cube decks, CR test counts, and games per second.
- **Card representation.** Whether cards become a generic Rust IR or per-card Rust code is the key decision. It is covered by the sibling `card_ir_survey` report. Either way, cards should be data or code in Rust, not dicts from Python.
