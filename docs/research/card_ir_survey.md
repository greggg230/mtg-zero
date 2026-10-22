# Card IR survey: can the PowerLSV cube go to Rust as data?

Research report, 2026-10-21. This is a read-only survey of `mtg-bot` at `main` @ `46efc8b9`. Nothing in mtg-bot was modified, built or committed.

It answers one question: is "Python oracle parser -> serializable card IR (JSON) -> Rust interpreter of the primitives" viable for a zero-core `SearchGame` covering the PowerLSV cube? And if it is, how big is the Rust surface?

**Path roots.**
- `MB/` = `C:\Users\Gregg Keithley\Documents\Claude\Projects\mtg-bot\`
- File references without a prefix are relative to `MB/src/mtg_bot/`.

**Method.** The pool was built with `OracleParserCardSource(pool="cube")`, which claims 546 of 546 cards in about 1.8 s (`cards/parser/source.py:189`). Every `CardPrinting` object graph was then walked by reflection: dataclass fields, non-field `__dict__` attributes, tuples, sets, dicts, enums and nested printings (faces, tokens, emblems). Scripts are in `docs/research/scripts/`:
- `walk_cube_ir.py` builds the inventory and the staging curve. It writes `card_ir_inventory.json`.
- `ir_roundtrip.py` is a prototype generic JSON IR plus a lossless round-trip check.
- `scan_engine_names.py`, `scan_card_slugs.py` and `scan_class_coupling.py` find card names and class coupling in engine code.
- `outline_engine.py` and `subsystem_lines.py` produce the subsystem line counts.

Scratch outputs go to `scripts/_*.json`. Run every script from the mtg-bot root with `PYTHONPATH=src .venv-py313/Scripts/python.exe <script>`.

A companion report, [`rust_engine_reuse.md`](rust_engine_reuse.md), covers the old Rust crate in depth. Section 6 of this report only summarizes its card bridge.

---

## 0. Verdict

**Mechanically, the IR is viable. The card graph is already pure data.**
- Across all 546 cube printings there are **zero** callables, lambdas, closures, `functools.partial` objects, bound methods, class objects used as values, regexes or opaque objects.
- `pickle` succeeds on 546 of 546 printings.
- A generic reflection serializer round-trips all 546 printings exactly, with no per-class code (`scripts/ir_roundtrip.py`, about 200 lines including the checks):
  - The output is `{"$t": "<module>.<Class>", field: value, ...}`. Fields equal to their defaults are omitted.
  - `decode(encode(p)) == p` holds for every card, and `encode(decode(encode(p)))` is a fixpoint.
  - Size: **503 KB of JSON for the whole cube**. That is 921 B per card on average, 4.0 KB at most (Ajani), and 52 KB gzipped.

**The IR is "class tag + fields". Its semantics live in about 500 Python classes and in engine code that reads printing flags.** So "write an interpreter of the primitives" means porting, one by one:
- **494 distinct non-core primitive classes**: 347 effects, 81 predicates, 25 conditions, 11 formulas, 8 cost atoms, 7 replacement transforms, 5 layer modifiers, 2 target types and 8 other.
- **265 further engine "features"** that are not classes:
  - 193 behaviour flags on record classes (140 of them on `CardPrinting`)
  - 26 keyword strings
  - 34 trigger/replacement `Event` values
  - 6 encoded static-scope strings
  - 4 string trigger keys
  - 2 controller enum values

That totals **759 features for 100% of the cube**.

**The primitive vocabulary is a long tail, not a small reusable kernel:**
- 314 of the 494 classes (64%) are used by exactly one cube card. Of those, 241 are effects.
- 226 cards (41%) need at least one class that no other cube card uses.
- The median class is small: 31 Python source lines, with an interquartile range of 18 to 48.

**Staging (greedy, cheapest card first, all feature kinds counted):**

| Features ported | Cube cards fully supported |
|---:|---|
| 68 | 25% |
| 184 | 50% |
| 446 | 80% |
| 569 | 90% |
| 759 | 100% |

Counting classes only (ignoring flags and keywords), the figures are 117 classes for 50%, 274 for 80% and 494 for 100%. **There is no "60 classes cover 80%" stage in this pool.** The first 60 classes by greedy order cover 217 cards (40%) on the classes-only measure, and 60 features of any kind cover 122 cards (22%).

**Recommendation: go ahead with Python parser -> generic JSON IR -> Rust, with five conditions.**
1. **Emit the IR generically from the dataclass graph.** Generate the Rust serde types from the Python class definitions. Do *not* write a per-class bridge. The old Rust bridge did that: it covered 210 classes and lives in the 42k-line `parity/` package (section 6).
2. **Normalize a short list of IR warts in Python first** (section 2.3). These are string-keyed discriminants the engine dispatches on: encoded scope strings, keywords with parameters inside the string, `dynamic_pt_formula` strings, `emblem_kind`, reflective `printing_fields`, and 58 snake_case "enum" fields.
3. **Design Rust resolution as a resumable state machine from day one.** The Python engine makes in-resolution choices through about 400 synchronous `agent.choose_*` callbacks of 37 kinds. Search sees only 11 of those kinds, through an exception-and-replay queue. `SearchGame::step` needs every choice to be an action.
4. **Budget it as a port of about 45-50k Python lines.** That is about 21k lines of card-primitive classes plus about 3k lines of helpers, and about 23.5k lines of engine subsystems (section 5). It is not "a small interpreter".
5. **Treat Python as a differential reference, not ground truth.** 177 of the 546 cube cards are named in `docs/SEARCH_LIMITATIONS.md`. Parity certifies Python's behaviour, not the Comprehensive Rules.

---

## 1. Inventory of reachable classes (task 1)

Raw per-class data is in `card_ir_inventory.json` under `classes[]`. For every class it records:
- `qualname`, `module`, `category` and `source` (file:line)
- `source_lines`, `cube_cards` (cards using it), `instances` (unique objects), `references` and `example_cards`
- per-field value types, and how many cards use a non-default value

### 1.1 Totals

- 546 cards; 502 distinct classes reachable.
  - 6 are core records: `CardPrinting`, `ManaCost`, `ActivationCost`, `ActivatedAbility`, `TriggeredAbility` and `StaticAbility`.
  - 2 are enums: `Event` and `_Controller`.
  - The other **494** are primitives.
- Maximum object-graph depth is 15.
- Per card:
  - **features**: mean 7.1, median 6, maximum 33.
  - **non-core classes**: mean 4.4, median 3, maximum 24.
- 6 cards need no feature at all: the 5 basics and Cogwork Librarian (its only text is draft-matters).
- Nested printings: 623 `CardPrinting` instances for 546 cards. The extras are DFC/MDFC/adventure faces and token printings embedded in `CreateToken.token_printing` and similar.

| Category | Classes | Instances | Class source lines | How it is defined |
|---|---:|---:|---:|---|
| effect | 347 | 1,062 | 16,301 | subclasses of `effects/base.py:26` `Effect` |
| predicate | 81 | 838 | 1,237 | `ObjectPredicate` (`effects/object_predicates.py:30`), `StatePredicate` (`effects/state_predicates.py:37`), `TriggerPredicate` (`effects/predicates.py:383`), plus duck-typed `CardFilter` and similar |
| condition | 25 | 48 | 366 | `Condition` (`effects/conditions.py:48`) plus duck-typed |
| formula | 11 | 21 | 151 | `Formula` (`effects/formulas.py:36`) plus duck-typed "value" objects |
| cost | 8 | 24 | 262 | `AlternativeCost` / `AdditionalCostOption` (`cards/spell_costs.py`), `CostReduction`, and duck-typed `payable`/`pay` atoms in `ActivationCost.extra_costs` |
| transform | 7 | 18 | 165 | `EventTransform`, `EventPattern`, `ReplacementEffect` (`effects/modifiers.py`) |
| modifier | 5 | 16 | 78 | `ContinuousModifier`, `LayerPayload`, `Duration` (`effects/modifiers.py`, `effects/durations.py`) |
| target | 2 | 168 | 53 | `TargetSpec` (`effects/targets.py:53`), `OneOf` (`effects/target_filter.py:51`) |
| other | 8 | 12 | 182 | e.g. `LevelerPT`, `PowerEqualsCount`, `BeginGameOnBattlefield` (`opening_hand.py:62`) |
| enum | 2 | (refs) | 268 | `Event` (`effects/predicates.py:50`), `_Controller` |
| core records | 6 | 2,345 | 2,162 | `CardPrinting` (`cards/base.py:597`, **241 fields**), `ActivationCost` (`effects/base.py:393`, 42 fields), `StaticAbility` (:828, 21), `ActivatedAbility` (:694, 13), `TriggeredAbility` (:1044, 9), `ManaCost` (`cards/base.py:262`) |

Reachable class source totals **21,225 lines**. The 63 modules hosting those classes contain another **3,285 lines** of module-level helper functions that the classes call. For scale:
- the whole `effects/` package is 53.8k lines
- the parser is 43.5k lines
- the rest of the effects package serves the EoE and Vintage Roto pools

### 1.2 How widely each class is used

| Cube cards using the class | Classes |
|---|---:|
| 1 | 314 (241 effects, 33 predicates, 18 conditions, 7 formulas, 6 other, 4 transforms, 3 costs, 2 modifiers); 11,033 source lines |
| 2 | 60 |
| 3-5 | 57 |
| 6-10 | 27 |
| 11-30 | 18 |
| 31+ | 20 |

### 1.3 Most-used primitives

This ranking is by cube cards using the class, excluding the core records.

| # | Class | Category | Module | Cards | Instances | Lines |
|---:|---|---|---|---:|---:|---:|
| 1 | `Event` | enum | `effects.predicates` | 220 | - | 258 |
| 2 | `IsSelf` | predicate | `effects.predicates` | 162 | 59 | 25 |
| 3 | `TargetSpec` | target | `effects.targets` | 129 | 146 | 28 |
| 4 | `IsCreature` | predicate | `effects.object_predicates` | 107 | 63 | 21 |
| 5 | `DrawCards` | effect | `effects.generic.draw_mill` | 82 | 79 | 17 |
| 6 | `And` | predicate | `effects.predicates` | 79 | 75 | 10 |
| 7 | `Controlled` | predicate | `effects.predicates` | 79 | 54 | 41 |
| 8 | `_Controller` | enum | `effects.predicates` | 79 | - | 10 |
| 9 | `Sequence` | effect | `effects.combinators` | 77 | 90 | 92 |
| 10 | `ObjectAnd` | predicate | `effects.object_predicates` | 70 | 94 | 7 |
| 11 | `AddManaToPool` | effect | `effects.generic.mana` | 67 | 70 | 67 |
| 12 | `CardFilter` | predicate | `effects.generic.card_flow` | 66 | 62 | 120 |
| 13 | `CreateToken` | effect | `effects.generic.tokens` | 50 | 60 | 144 |
| 14 | `Subject` | predicate | `effects.predicates` | 48 | 33 | 20 |
| 15 | `IsArtifact` | predicate | `effects.object_predicates` | 42 | 17 | 12 |
| 16 | `IsPlayer` | predicate | `effects.object_predicates` | 42 | 34 | 14 |
| 17 | `ObjectNot` | predicate | `effects.object_predicates` | 42 | 47 | 6 |
| 18 | `IsLand` | predicate | `effects.object_predicates` | 40 | 24 | 10 |
| 19 | `ObjectOr` | predicate | `effects.object_predicates` | 37 | 49 | 7 |
| 20 | `IsPlaneswalker` | predicate | `effects.object_predicates` | 35 | 26 | 17 |
| 21 | `SearchLibrary` | effect | `effects.generic.card_flow` | 21 | 21 | 110 |
| 22 | `RemoveTargetPermanents` | effect | `effects.generic.targeted_removal` | 21 | 23 | 97 |
| 23 | `IsBattle` | predicate | `effects.object_predicates` | 21 | 22 | 16 |
| 24 | `OneOf` | target | `effects.target_filter` | 21 | 22 | 25 |
| 25 | `GainLife` | effect | `effects.generic.life` | 20 | 19 | 18 |
| 26 | `MayDo` | effect | `effects.combinators` | 16 | 19 | 51 |
| 27 | `ControlledByYou` | predicate | `effects.object_predicates` | 16 | 7 | 20 |
| 28 | `IsEnchantment` | predicate | `effects.object_predicates` | 16 | 4 | 19 |
| 29 | `IfConditionElse` | effect | `effects.combinators` | 14 | 14 | 81 |
| 30 | `Destroy` | effect | `effects.generic.destroy` | 14 | 16 | 61 |
| 31 | `CounterSpell` | effect | `effects.generic.counterspells` | 13 | 14 | 99 |
| 32 | `DiscardSelf` | effect | `effects.generic.discard` | 13 | 13 | 41 |
| 33 | `Surveil` | effect | `effects.generic.library_lookat` | 13 | 13 | 54 |
| 34 | `IsPermanent` | predicate | `effects.generic.cube_walkers` | 12 | 13 | 7 |
| 35 | `DealDamageAnyTarget` | effect | `effects.generic.damage` | 12 | 13 | 37 |
| 36 | `AddNManaOfChosenColor` | effect | `effects.generic.mana` | 12 | 12 | 45 |
| 37 | `SearchLibraryForLandCardToBattlefield` | effect | `effects.generic.search` | 12 | 12 | 93 |
| 38 | `ControlledByOpponent` | predicate | `effects.object_predicates` | 12 | 6 | 28 |
| 39 | `ControlsAtMost` | condition | `effects.conditions` | 10 | 10 | 22 |
| 40 | `IsSource` | predicate | `effects.object_predicates` | 10 | 10 | 22 |
| 41 | `StateBound` | predicate | `effects.predicates` | 10 | 9 | 16 |
| 42 | `AlternativeCost` | cost | `cards.spell_costs` | 9 | 9 | 73 |
| 43 | `SourceDealsDamageToController` | effect | `effects.generic.artifact_mechanics` | 9 | 9 | 28 |
| 44 | `PutPlusOneCounterOnSelf` | effect | `effects.generic.counters` | 9 | 9 | 28 |
| 45 | `ReturnCardFromGraveyardToBattlefield` | effect | `effects.generic.reanimation` | 9 | 9 | 120 |

The full list of all 494 classes and all 265 non-class features, with card counts, is in the Appendix.

### 1.4 The interface those classes implement

The Rust trait surface is the methods defined across the reachable classes:

| Method | Classes defining it |
|---|---:|
| `describe` | 362 |
| `resolve` | 334 |
| `requires_target` | 82 |
| `legal_targets` | 82 |
| `test` (object predicates) | 55 |
| `matches` (conditions/trigger predicates) | 52 |
| `target_count_range` | 35 |
| `value` (formulas) | 11 |
| `apply` (transforms) | 11 |
| `payable` / `pay` (cost atoms) | 4 |
| `target_slot_kinds`, `legal_targets_for_slot`, `targets_from_event`, `matches_for_source`, `multiplier`, `base_pt`, `cast_time_shape`, `event_subject_id`, `useful_x_cap`, `candidates` | 2-3 each |

The engine also probes effects through **duck-typed attributes** that make up an informal protocol. Rust must implement these as trait methods with defaults:
- `cast_time_shape`, `count_from_x`, `division_total` and `legal_targets_for_slot` (`actions.py:2068`, `:2081`, `:2123`, `:2137`)
- `mode_mana_cost` (`:2904`), `pay_escalate` (`:3204`) and `target_count_is_x` (`:4062`)
- `dynamic_target_count_range` (`:6221`), `targets_from_event` (`:6483`) and `event_subject_id` (`:6488`)
- `single_mode_mana_costs` (`legal.py:903`)
- `produces` (`game.py:2570`)

Engine-side `isinstance` dispatch on card classes is small: 64 sites over 27 classes, mostly the targeting helpers `effects/generic/targets_helpers.py` and `effects/base.py`, plus `actions.py` on `Sequence` / `ModalChooseOne` / `ChooseModes` / `AddManaToPool` (`scripts/_class_coupling.json`).

---

## 2. Serialization blockers (task 2)

### 2.1 Hard blockers: none

| Kind | Count across 546 printings |
|---|---:|
| lambda / function / closure | 0 |
| `functools.partial` | 0 |
| bound method / builtin callable | 0 |
| class object used as a value | 0 |
| compiled regex | 0 |
| opaque object (no `__dict__`/fields) | 0 |
| `pickle.dumps` failures | 0 / 546 |
| generic JSON round-trip mismatches | 0 / 546 (equality and fixpoint) |

No class reachable from the cube is defined inside a function (no `<locals>` qualnames). The engine compares printings and effects by identity in exactly one place: `roto_board_statics.py:490`, `item.source is source and item.ability is ability`.

### 2.2 Card-name strings inside printings

After excluding basic-land names (they collide with subtypes and land-type fields, 144 raw hits), **2** strings remain. Both are self-references on `"Name Sticker" Goblin`:
- `triggered_abilities[0].effect.gate.name` (`NameStickerGate`)
- `triggered_abilities[0].predicate.right.name` (`ControlsAtMostNamed`)

These are legitimate CR name references ("creatures named ..."), not engine special-casing.

### 2.3 Soft blockers: data that serializes but carries engine-dispatch meaning

All of these round-trip fine. They need a closed Rust vocabulary or a small Python-side cleanup before the IR is frozen.

| Item | Where | Cube examples | Recommendation |
|---|---|---|---|
| **Snake_case string discriminants**: 116 values in 58 class.field slots | e.g. `RemoveTargetPermanents.action` (`exile`/`destroy`/`bounce`); `SearchLibrary.destination`; `ReturnCardFromGraveyardToBattlefield.source`; `ExileTopCardsWithPlayPermission.duration`/`whose`; `AddManaToPool.spend_restriction` (`legendary_spell`); `SearchLibraryForCard.filter_name` (`artifact_small`); `ReplacementEffect.affects`; `Controlled.subject`; `Subject.slot` | 21 cards use `RemoveTargetPermanents` alone | Convert to Python `Enum`s (or at least collect the vocabularies from the class code, not just the observed values) so the Rust enum is complete. |
| **Encoded `StaticAbility.scope` strings** | parsed at `effects/base.py:996-1012` (`StaticScope`, `:790`) | `subtype_you_control:other:Dwarf` (Magda), `subtype_you_control:all:Ninja`, `card_type_you_control:Land` (Nissa emblem) | Make scope a structured value. |
| **`TriggeredAbility.trigger` string keys** (legacy string path, `effects/base.py:1072`) | `chapter_1..3`, `class_level_2` | Fable of the Mirror-Breaker, Urza's Saga (`chapter_N`), Does Machines (`class_level_2`) | Map to events; 4 keys in the cube. All other cube triggers use `Event` + predicate. |
| **Keywords as free strings with parameters** | `CardPrinting.keywords`, parsed by `keyword_rules.py:70-90` | `annihilator 6`, `protection from the chosen player`, `protection from spells that are one or more colors` | Keyword enum plus parameters. |
| **`dynamic_pt_formula`: `str` or object** | dispatched at `game.py:1962-2015` and `cube_creatures_rmc.rmc_dynamic_base_pt` | strings `card_types_in_all_graveyards` (Barrowgoyf), `card_types_in_all_graveyards+1` (Pyrogoyf), `domain` (Territorial Kavu); objects `PowerEqualsCount` (Adeline), `LevelerPT` (Hexdrinker) | Make every value a `Formula` object. |
| **`CreateEmblem.emblem_kind = "tezzeret_cruel_captain"`** | `effects/generic/emblem.py:40-64` hard-codes the emblem's abilities by key; it exists to keep the old Rust bridge a single string | Tezzeret, Cruel Captain | Use `CreateEmblemWithAbilities` (already used by 7 other walkers) so the emblem is inline data. |
| **Reflective field patch** `CreateEmblemWithAbilities.printing_fields` | `effects/generic/cube_walkers.py:1000-1016`: `dataclasses.replace(printing, **dict(printing_fields))` | Wrenn and Six's emblem | Use explicit fields; Rust would otherwise need a field-name to field map. |
| **Derived non-field `targets` attribute** set in `__post_init__` | 8 classes, e.g. `ThenTargetControllerResolves`, `FlickerUpToOneOtherTargetNonlandPermanentYouControl`, `GainControlOfTargetThenAttachIfEquipment` | | Either serialize it (the prototype carries it as `$extra`) or recompute it in Rust. |
| **Mana costs as display strings next to parsed costs** | `*_cost_str`, `CounterSpell.unless_pays` (`{2}`), `OptionalManaCostThenEffect.cost_str`, `AlternativeCost.mana` | 11 values | Fine: parse with the same mana grammar. |
| **Display labels** that only feed `describe()` and logs: 42 values | `CounterSpell.label`, `RemoveAllMatching.label`, `ChooseModes.header`, `CreateEmblemWithAbilities.name` | | Harmless, but mode-choice prompts pass `describe()` strings to agents (`actions.py:1997`, `:2010`). Rust needs only indices. |
| **Printing-level behaviour flags** ("opaque flags") | 140 of `CardPrinting`'s 241 fields are non-default on some cube card, plus 30 `ActivationCost`, 13 `StaticAbility`, 8 `ActivatedAbility` and 2 `TriggeredAbility` fields | `mana_only_from_produces` 74 cards, `color_indicator` 32, `enters_tapped` 27, `loyalty` 23, `shock_land_life_cost` 14, `alternative_costs` 9, ... down to about 60 single-card flags such as `opponents_cant_make_you_discard_or_sacrifice`, `paints_all_objects_chosen_color_at_etb`, `exiles_uncast_nontoken_creatures_entering` | Serializable as a flat struct, but each flag is bespoke engine code (section 3). Count each one as a feature. |

Two observations matter for the IR design:
- **Static abilities are mostly flags, not objects.** Only 24 cube cards carry a `StaticAbility` object at all. The rest of the cube's static text became `CardPrinting` booleans and ints read directly by engine modules. The modifier grammar (`ContinuousModifier`, `ReplacementEffect`; `effects/modifiers.py`) is used by only 3 and 6 cube cards respectively.
- **`grants_look_at_top_of_library`** (Bolas's Citadel) is set by the parser but read by no runtime code. It matters only to hidden-information handling, i.e. what a `determinize`/`encode` must consider visible.

---

## 3. Engine-side special-casing outside card objects (task 3)

`scripts/scan_engine_names.py` and `scripts/scan_card_slugs.py` searched the source in two ways:
- AST string constants equal to any card name in the `all` pool
- card-name slugs (`snake_case` / `CamelCase`) in identifiers

### 3.1 Runtime branches on card names

There are almost none.

| File:line | Card | What it does | Cube? |
|---|---|---|---|
| `game.py:1962-2015` `_dynamic_base_pt` | `"cosmogoyf"`, `"harmonious_grovestrider"`, `"emissary_escort"` formula keys | String-keyed P/T formulas named after cards. The cube reaches the same function through `card_types_in_all_graveyards` and `domain` (via `effects/generic/cube_creatures_rmc.rmc_dynamic_base_pt`). | partly |
| `effects/generic/emblem.py:40-64`, `:110` `TezzeretEmblemBeginCombat` | Tezzeret, Cruel Captain | Emblem abilities hard-coded behind an `emblem_kind` key. | yes |
| `actions.py:1898-1962` `_offer_echoing_deeps_copy` | Echoing Deeps | Flag-gated (`optional_copy_land_from_graveyard`), card-named helper. | no |
| `actions.py:3802` `_lurrus_permission_for`, `effects/generic/cube_creatures_rmc.py:2065`/`:2080` `lurrus_casters`/`lurrus_castable` | Lurrus | Named after the card but driven by a generic field (`StaticAbility.graveyard_permanent_cast_mv_max`). | yes (generic) |
| `game.py:2420` `magus_of_the_moon_active` | Magus of the Moon / Blood Moon | Wrapper over the `nonbasic_lands_are_mountains` flag. | yes (generic) |
| `cards/spell_costs.py:217` `DREAM_HALLS_COST` | Dream Halls | Granted alternative-cost constant. | yes (data) |
| `decks.py:58` `UNMODELED_CARDS = {"Paradox Haze"}` | Paradox Haze | 17lands deck filter (EoE). | no |
| `effects/bespoke.py` (42 card-named classes) | EoE cards | Only **1** is reachable from the cube: `EnduringDiesReturnAsEnchantmentEffect` (`:2043`, Enduring Innocence). | 1 |

Name-based logic that is correct per the CR or the search design, and must be mirrored:
- the legend rule keys on `(controller, printing.name)` (`turn.py:2074`)
- legal-action de-duplication keys (`legal.py:343`, `:1103`, `:1283`, `:1306`, `:1358`, `:1395`, `:1423`)
- "choose a card name" statics (`creature_statics_wg.py:130`, `roto_restrictions.py:318-376`)

Everything else that mentions `printing.name` in `game.py`, `turn.py` and `actions.py` is log text.

### 3.2 Build-time name gates in the parser

These are harmless for an IR, because their output is data. But they do mean the IR is a function of (oracle text, name lists, pool):
- `cards/parser/cube_enchantments.py:65` `CUBE_ENCHANTMENT_NAMES`: a 45-name opt-in list for the cube enchantment grammar.
- `cards/parser/enchantment_builder.py:371`, `:707`, `:712`: aura, honest-no-op and enchantment name lists.
- `cards/parser/room_builder.py:64`: room names.
- `cards/parser/spell_builder.py:1035` `_LEGACY_PARTIAL_MODAL_CLAIMS`: four EoE cards deliberately parsed with the old grammar so the Rust parity bridge keeps seating them.
- `cards/parser/source.py:92-121`: per-name grammar pinning for EoE-only printings (`is_eoe_only_printing`, `wg_grammar_pinned_off` / `creature_wg_effects.py:254` `EOE_PINNED_NAMES`).

### 3.3 Printing-specific flags read by the engine

This is the real special-casing surface. The 140 cube-used `CardPrinting` flag fields are referenced on **407 runtime lines**. That count excludes the parser, the parity code, `review_render` and the definition sites in `cards/base.py`.

| File | Flag-reference lines |
|---|---:|
| `actions.py` | 103 |
| `legal.py` | 46 |
| `layouts.py` | 30 |
| `alt_casting.py` | 29 |
| `game.py` | 23 |
| `creature_casting_ub.py` | 23 |
| `creature_statics_wg.py` | 17 |
| `effects/generic/roto_walkers.py` | 12 |
| `turn.py` | 10 |
| `effects/generic/cube_creatures_rmc.py` | 10 |
| (others) | 104 |

Per-flag consumer files are in `scripts/_engine_name_scan.json` under `flag_consumers`.

Runtime code also rewrites printings:
- about 14 `dataclasses.replace(...printing...)` sites
- 19 `obj.printing = ...` reassignments (copy effects, transform, face-down, emblems)

So the Rust card model must support runtime-derived printings, i.e. a copy plus overrides, not just immutable IR records.

---

## 4. Choice model: the hidden blocker for `SearchGame`

This section is not one of the six tasks, but it determines the shape of the interpreter.

Effects and the engine make in-resolution choices through **synchronous agent callbacks**. There are 37 distinct `choose_*` kinds and about 400 call sites in the engine and effects. The most common:

| Callback | Call sites |
|---|---:|
| `choose_yes_no` | 72 |
| `choose_sacrifice` | 51 |
| `choose_card_from_library` | 48 |
| `choose_target` | 34 |
| `choose_mode` | 31 |
| `choose_discard` | 31 |
| `choose_x` | 17 |
| `choose_mana_color` | 16 |
| `choose_copy_targets` | 10 |
| `choose_permanent_to_tap` | 9 |

Search exposes only **11 kinds** (`search/resolution_channel.py:45-56`). It does this by raising `_ResolutionPause` and **replaying the whole priority action from a clone** with a growing answer queue (`search/resolution_channel.py:1-25`). Every other choice goes to `DefaultTacticalAgent`, which takes the first option or the maximum X (`search/tactical.py:84-110`).

`SearchGame` requires every decision to be a `u16` action from `step()`, and every revealed fact to be a chance key (`zero-core/docs/search.md` section 1). Consequences:
- **The Rust interpreter must be resumable.** Each primitive's `resolve` becomes a step function that can suspend on a pending decision.
- **The differential harness must script Python's choices.** It should drive the Python reference with a recorded choice stream (an agent replaying the Rust engine's decisions), rather than compare against Python's heuristic defaults.
- **Hidden-information reveals must be keyed for chance.** These include library searches, random discards, coin flips, dice and face-down cards.

---

## 5. Engine subsystems the cube needs, beyond per-card primitives (task 4)

Line counts sum the top-level functions and classes implementing each subsystem (`scripts/subsystem_lines.py` over `scripts/_engine_outline.json`; "whole" means the whole file is counted). These are Python lines including docstrings.

| Subsystem | Lines | Where (biggest pieces) |
|---|---:|---|
| Game-state model + copy-on-write cloning | 2,322 | `game.py` `GameState`@3712 (+925 lines, 51 fields), `GameObject`@445 (+592, 120 fields), `_CowList`@37 (+374), `Player`@3349 (60 fields), `StackItem`@3230 (22 fields) |
| Turn structure, steps, priority loop | 798 | `simulate.py` `priority_loop`@326, `run_step`@389; `turn.py` `run_cleanup_step`@1687, `run_untap_step`@93, `draw_card`@293; `roto_turns.py` |
| Legal-action generation (priority, timing, castability) | 1,862 | `legal.py` (whole; `_legal_actions_core`@859 is 962 lines, `battlefield_ability_activations`@457 is 387) |
| Casting + alternative/additional costs (flashback, escape, evoke, emerge, kicker, splice, delve, pitch, plot, dash, warp, miracle, replicate, teamwork, retrace, mayhem, Lurrus) | 3,536 | `actions.py` `_cast_spell`@2333 (**1,067 lines**), `_select_spell_targets`@2084; `alt_casting.py`, `evoke.py`, `creature_casting_ub.py`, `graveyard_casting_wg.py`, `cards/spell_costs.py` (whole) |
| Mana pool, mana abilities, auto-payment (convoke, improvise, phyrexian, sac-for-mana) | 1,179 | `actions.py`@236-1611 (`auto_tap_for_cost`@1405, `_coin_satisfies_context`@353), `game.py` `effective_land_produces`@2506, `ManaCoin`@3568 |
| Activated abilities (loyalty, hand/graveyard abilities, station, crew/saddle) | 1,356 | `actions.py` `_activate_ability`@3857 (**903 lines**), `_activate_hand_ability`@3685; `game.py` `effective_activated_abilities`@2580 |
| Stack and resolution (fizzle, spell copies on stack) | 583 | `actions.py` `_resolve_top_of_stack_item`@5255, `move_spell_off_stack`@5118; `keyword_rules.py:224` |
| Triggered abilities (event dispatch, delayed, granted, state triggers, ETB multipliers) | 1,427 | `actions.py` `dispatch_event`@6347, `push_triggered_ability`@6176; `turn.py` `fire_*` / `run_beginning_of_*` / `run_delayed_*` (@854-1685); `roto_board_statics.py` granted triggers |
| Layers 1-7 / continuous effects / characteristic queries | 1,614 | `game.py`@1171-3227 (`effective_subtypes`@2099, `_static_bonuses`@1459, `_compute_effective_pt`@3082, `_layer6_entries_for`@1667, `ContinuousEffect`@3646); `effects/modifiers.py` `walk_continuous_modifiers`@695 |
| Replacement + prevention | 637 | `effects/modifiers.py` `dispatch_replacement`@436, `_collect_replacement_effects`@534; `turn.py` void/graveyard-exile replacements, `_draw_replaced`; `actions.py` `apply_land_etb_replacements`@1805; `game.py` draw-count, counter-doubling, damage prevention |
| State-based actions | 344 | `turn.py` `_state_based_actions_pass`@1995, `state_based_actions`@1888 |
| Zone changes (ETB/LTB, sacrifice, destroy, land play, counters) | 1,338 | `actions.py` `put_into_battlefield`@5760, `_play_land`@1700; `turn.py` `move_off_battlefield`@466 (374 lines) |
| Combat | 1,276 | `combat.py` (whole; `_resolve_damage_step`@485), `actions.py` declare attackers/blockers, `designations.forced_attackers` |
| Copy effects (clones, token copies, spell copies) | 488 | `actions.py` `apply_clone_enters_as_copy_replacement`@5690, `creature_casting_ub.apply_copy_on_entry`@195, `effects/generic/spell_copies.py` |
| Control change | 217 | `game.py` `effective_controller`@1621, `effects/generic/control.py` |
| Face-down (manifest dread / face-down exile) | 63 + inline | `actions.py` `_turn_face_up`@1667; `face_down` checks inside the `game.py` characteristic queries |
| Multi-face layouts (transform, MDFC, adventure, split, class, saga, room, leveler, prepare, day/night) | 1,021 | `layouts.py` (whole), `effects/generic/layout_effects.py`, `actions.py` `_unlock_room_half`@4819 |
| Monarch / initiative / Undercity dungeon | 320 | `designations.py` (whole) |
| Extra turns / extra combats / skip turn | 63 | `simulate.py` `_pick_next_active_player`@567, `_maybe_skip_turn`@541; `GameState.extra_turn_queue` / `extra_combat_phases` |
| Outside the game (wishes), conspiracies, opening hand, mulligan, setup | 873 | `roto_turns.py:140-180`, `conspiracies.py`, `opening_hand.py`, `mulligan.py`, `simulate.setup_game`@110 |
| Tokens (predefined token printings) | 306 | `cards/tokens.py` |
| Emblems | 192 | `effects/generic/emblem.py` (plus the cube emblem classes in `cube_walkers.py`) |
| Restrictions, protection, ward, hexproof, taxes, lock pieces | 1,435 | `keyword_rules.py`, `roto_restrictions.py`, `player_protections.py`, `creature_statics_wg.py`, `actions._apply_ward_check`@3466, `roto_board_statics.py` |
| Hidden information + determinization | 231 | `game.py` reveal/knowledge helpers @1079-1163, `search/determinize.py` |
| **Total mapped** | **~23,500** | |

Notes:
- **Planeswalkers** have no module of their own. They are spread across `ActivationCost.loyalty` (`effects/base.py:556`), `legal.loyalty_activation_available`@24, the loyalty state-based action in `turn.py`, and the 27 cube-reachable walker classes in `effects/generic/cube_walkers.py`, which are counted in section 1.
- **Sagas** are in `layouts.py:252-316`.
- **Reflexive triggers** appear as combinators such as `IfYouDo` (`effects/combinators.py:639`).
- **Delayed triggers** live in `GameState.delayed_triggers`, plus `turn.run_delayed_*` and `effects/generic/delayed_actions.py`.

The Rust port also needs the subsystems the Python engine leaves to `search/`: canonical hashing and the action codec. They are out of scope here.

---

## 6. Existing serialization and the old Rust bridge (task 5)

**There is no lossless card or game-state serializer in `src/mtg_bot/` today.**
- `serialize.py` (187 lines) renders a `GameState` for the UI and replays: `serialize_state`:122, `serialize_permanent`:53, `serialize_hand_card`:91. Abilities are flattened to `describe()` strings (:77-78, :102), and there is no decoder.
- `replay.py` (159 lines) is gzipped JSON of those snapshots. Its docstring calls it lossy by design (:7-13).
- `search/canonical_state.py` (666 lines) produces a `marshal` + blake2b **hash** for the MCTS cache.
- `training/replay_decisions.py` round-trips decisions only.

**The old Rust engine** (`MB/rust/`, 86,807 lines of `.rs`) receives cards **from Python at runtime as pyo3 dicts**, through a hand-written, per-class bridge:
- **Python side.** `parity/trajectory.py:13689` `_printing_to_card_def` builds a `CardDef` (`parity/cross_engine.py:107`, 43 fields) using per-class recognizers: `_python_ta_to_safe_dict` (trajectory :3631), `_python_spell_effect_to_dict` (:5571) and `_python_aa_to_safe_dict` (cross_engine :15960). It is then flattened by `_spec_card_to_printing_dict` (cross_engine :15193).
- **Rust side.** The flattened dict goes to `Registry.register_printing` (`rust/src/cards.rs:22796`) -> `parse_printing` (:18107) -> `parse_effect` (:13446), a string `kind` match of about 4.5k lines over **248 `Effect` variants**. An unknown kind raises `ValueError` (:17948).
- **Reverse direction.** `_ta_dict_to_python_effect` (cross_engine :1009, about 13k lines) maps dicts back to Python effects.
- **Total parity Python** is 41,948 lines (`parity/`).
- **Coverage.** 210 of 236 Python effect classes in the **EoE** pool (`parity/effect_coverage.py:64` `PARITY_POOL = "eoe"`).
- **Unmapped cards.** A card the Python-to-dict recognizers can't map is silently **dropped and its deck slot padded with a Forest** (`LoaderAdmitPolicy.DROP` default, `parity/trajectory.py:14755` and `:14725-14769`).
- **Card-specific variants.** Several Rust variants are named after single cards, e.g. `atomic_microsizer_attack_effect`, `consult_the_star_charts_effect`, `zero_point_ballad_effect`.

**Status: dormant.** `search/heuristics_config.py:51` sets `DEFAULT_DECISION_SOURCE = "python"`. The last substantive Rust work was 2026-06-01, with one commit since (`cf814faf`, 2026-10-03). The cube is not bridged.

Overlap with the cube is small:
- Of the 347 effect classes reachable from the cube, **66** have an old Rust variant. They account for 494 class-uses across cube cards.
- Only 34 cube cards are in the EoE pool, and 5 of those are basics.

Lesson for the new design: the per-class bridge, not the Rust semantics, is what consumed the old campaign's effort. The generic encoder in section 0 removes that layer entirely. It needs no per-class Python code, and Rust deserializes with `#[serde(tag = "$t")]` enums that can be generated from the dataclass definitions.

---

## 7. Estimate and staging (task 6)

### 7.1 Size of the Rust surface for 100% of the cube

| Item | Count |
|---|---:|
| Primitive classes (non-core) | **494** (347 effect, 81 predicate, 25 condition, 11 formula, 8 cost, 7 transform, 5 modifier, 2 target, 8 other) |
| Core record types | 6, plus 2 enums |
| Record behaviour flags | 193 (140 `CardPrinting`, 30 `ActivationCost`, 13 `StaticAbility`, 8 `ActivatedAbility`, 2 `TriggeredAbility`) |
| Keywords | 26 (flying 41 cards, flash 23, trample 21, haste 16, lifelink 15, menace 12, vigilance 11, ...; 12 single-card keywords incl. cascade, persist, infect, shadow, ascend, annihilator, battle cry) |
| Trigger/replacement events | 34 `Event` values (ENTERS 125 cards, ATTACKS 33, IS_CAST 20, DIES 14, DEALS_COMBAT_DAMAGE 13, ...; 5 `WOULD_*` replacement events) |
| Encoded scopes and string trigger keys | 6 + 4 |
| **Total features** | **759** |
| Python source to port: primitives | about 21.2k lines of class source plus about 3.3k lines of helpers |
| Python source to port: engine subsystems | about 23.5k lines (section 5) |

A Rust LOC guess is about 1-1.5x the Python, which gives roughly **50-70k Rust lines**.

Calibration: the old crate is 86.8k lines for an EoE-shaped engine that kept its game loop in Python. Much of `cards.rs` is doc comments and pyo3 glue. Treat this as an order-of-magnitude figure, not a schedule.

### 7.2 Staging curve

The greedy order repeatedly adopts the card needing the fewest not-yet-ported features; ties go to features shared by more cards. The full order and the per-step unlocks are in `card_ir_inventory.json` under `staging_all_features.steps` and `staging_classes_only.steps`.

| Target coverage | All features (classes + flags + keywords + events) | Classes only |
|---|---:|---:|
| 25% (137 cards) | 68 | 15 |
| 50% (273) | 184 | 117 |
| 60% (328) | 265 | 158 |
| 70% (383) | 354 | 209 |
| 80% (437) | 446 | 274 |
| 90% (492) | 569 | 378 |
| 95% (519) | 646 | 412 |
| 100% (546) | 759 | 494 |

For comparison, porting in plain usage order (most-used class first) is worse. On the classes-only measure:

| Classes ported | Cards covered |
|---:|---:|
| 50 | 188 (34%) |
| 100 | 236 (43%) |
| 200 | 331 (61%) |
| 300 | 403 (74%) |
| 400 | 470 (86%) |

The tail is expensive. The last 10% of cards (54 cards) need 190 features. These are mostly single-card statics and replacements: Hullbreacher, Quantum Riddler, Karn, Urza, Tezzeret, the vehicles (Esika's Chariot, Smuggler's Copter, Reckoner Bankbuster), Parallax Wave and Kaldra Compleat.

### 7.3 Suggested stages

Each stage is followed by a differential gate against the Python reference on decks drawn from the cards it unlocks.
- **Stage 0, before any primitive.** Engine core: state model, turn and priority loop, resumable resolution, mana payment, casting without alt costs, stack, SBAs, combat with the basic keywords, zone moves, the ETB/dies/attacks/cast events, and `TargetSpec` + `IsSelf` + the object-predicate combinators.
- **Stage 1, about 68 features, 25%.** Lands (mana abilities, shock and fetch lands), cantrips, counterspells, burn, the simple tutors.
- **Stage 2, about 184 features, 50%.** Plus the remaining common effects (`Sequence`, `MayDo`, `IfConditionElse`, `CreateToken`, `SearchLibrary`, `RemoveTargetPermanents`), equipment, initiative and monarch, transform.
- **Stage 3, about 446 features, 80%.** Plus layers 4/6/7 statics, alternative costs (flashback, evoke, kicker, escape), planeswalkers, copy effects, delayed triggers.
- **Stage 4, all 759.** The single-card tail, replacement effects (`WOULD_*`), lock pieces and the vehicles.

Cube deck generation (`scripts/build_cube_deck_pool.py`) already samples "claimed cards only". The same mechanism can restrict self-play to the Rust-supported subset while the port is incomplete.

---

## 8. Recommendation

**IR viability: yes.** The printing graph is already a clean, acyclic-in-practice, frozen-dataclass tree with no code in it. A generic encoder gives a lossless 0.5 MB JSON for the cube today.

The real work is the interpreter, and it is large and long-tailed: 494 classes plus 265 other features, about 45-50k Python lines of semantics. "Primitives" here are not a small orthogonal kernel. The parser grew one class per novel oracle shape, and two thirds of the classes serve a single card.

Do this:
1. **Freeze an IR v1 from the generic encoder** (`scripts/ir_roundtrip.py`): sparse fields, `$t` tags, frozensets as sorted lists, `inf` encoded explicitly. Add a schema dump with class -> fields -> types, generated from the dataclass annotations. Codegen the Rust `serde` enums per category: `Effect`, `ObjectPredicate`, `TriggerPredicate`, `StatePredicate`, `Condition`, `Formula`, `CostAtom`, `LayerPayload`, `EventTransform`. Unknown tags must be a hard error.
2. **Clean the warts in Python before freezing** (section 2.3): enums for the 58 discriminant fields, structured scopes and keywords, `Formula` objects for `dynamic_pt_formula`, inline emblems, no reflective `printing_fields`. Each is a small parser/effects change, and each makes the vocabulary closed and checkable.
3. **Keep the 193 flags as a flat `PrintingFlags` struct.** Do not re-architect them into `StaticAbility` first: a flag is no worse than a single-card class. Track each one as a feature in the staging plan.
4. **Make resolution resumable in Rust** (section 4), and build the differential harness around a shared, scripted choice stream. Do not compare against Python's first-option heuristics.
5. **Do not reuse the old Rust bridge.** Mine the old crate for edge cases (see `rust_engine_reuse.md`).
6. **Re-run `walk_cube_ir.py` whenever the parser changes.** The inventory, the feature list and the staging order are all regenerated from the live pool in about 3 s.

---

## Appendix: every primitive and feature with its cube-card count

Generated from `card_ir_inventory.json`. The format is "Name count", where count is the number of cube cards using it.

**effect** (347 classes): DrawCards 82, Sequence 77, AddManaToPool 67, CreateToken 50, RemoveTargetPermanents 21, SearchLibrary 21, GainLife 20, MayDo 16, Destroy 14, IfConditionElse 14, CounterSpell 13, DiscardSelf 13, Surveil 13, AddNManaOfChosenColor 12, DealDamageAnyTarget 12, SearchLibraryForLandCardToBattlefield 12, PutPlusOneCounterOnSelf 9, ReturnCardFromGraveyardToBattlefield 9, SourceDealsDamageToController 9, EquipToTargetCreatureYouControl 8, ModalChooseOne 8, CreateEmblemWithAbilities 7, ExileTopCardsWithPlayPermission 7, LookAtTopCardsPutIntoHand 7, TakeInitiative 7, TransformSource 7, DealDamageFromEvent 6, Exile 6, IfCondition 6, SearchLibraryForCard 6, TargetPlayerRevealsHandYouChooseDiscard 6, UntapSelf 6, ChooseModes 5, EachPlayerDraws 5, IfYouDo 5, PumpTargetCreatureUntilEOT 5, PutNamedCountersOnSelf 5, PutPlusOneCountersOnTargetThenGrantKeywordsUEOT 5, SacrificeSelfIfEvoked 5, ApplyEffect 4, BecomeMonarch 4, BecomeType 4, Bounce 4, CounterTargetSpell 4, DealDamageDividedAmongTargets 4, DealDamageToTargetCreature 4, DestroyTargetArtifact 4, ExileSourceThenReturnTransformed 4, PumpCreaturesYouControlUntilEOT 4, RemoveAllMatching 4, SelfMill 4, AddManaInAnyCombination 3, BecomeLandCreatureUEOT 3, CreateTokenAndAttachSelf 3, DealDamage 3, EachOpponentLosesLife 3, EachPlayerShufflesZonesIntoLibrary 3, ExileTargetCardFromGraveyard 3, ExileTargetReturnAtNextEndStep 3, LinkExiledCardsToSource 3, LoseLife 3, PumpSelfUntilEOT 3, ReturnTargetCardFromYourGraveyardToHand 3, ReturnTargetCreatureToOwnersHand 3, SacrificeSelf 3, TakeExtraTurnAfterThisOne 3, Untap 3, AddManaOfColorPerCount 2, CounterTargetStackObject 2, CreateTokenCopiesOfSelf 2, CreateTokenCopyOfSource 2, CreateTokensScaledByX 2, CreaturesCantBlockThisTurn 2, DealDamageEachOpponent 2, DestroyTargetLand 2, DiscardUpToNThenDrawThatMany 2, ExileOpponentsNonlandPermanentUntilLeaves 2, FlickerUpToOneOtherTargetNonlandPermanentYouControl 2, GrantFlashbackToTargetCardUEOT 2, GrantProtectionOfChoiceUEOT 2, LinkedCreatureControllerSacrifices 2, LookTopNMayRevealMatchingToHandRestBottomRandom 2, MayDoThenReflexive 2, MillTargetPlayer 2, PumpAllCreaturesUEOT 2, PutCardsFromHandOnTopOfLibrary 2, PutCreatureFromHandHasteSacrificeAtNextEndStep 2, PutPlusOneCounterOnEachMatching 2, PutSourceOntoBattlefieldTappedAndAttacking 2, PutTargetPermanentIntoOwnersLibrary 2, ReanimateAndAttachSource 2, ReflexiveWhenYouDo 2, ReorderTopCards 2, ReturnFromGraveyard 2, RevealTopCardToHandLoseLifeEqualMV 2, RollDieTable 2, SacrificeSelfUnlessEscaped 2, ScheduleDelayedEffect 2, Scry 2, SourceBecomesIfSubtype 2, SourceFightsTarget 2, TargetCreatureCantBlockThisTurn 2, TargetPlayerDiscards 2, TargetPlayerDiscardsAtRandom 2, UntapUpToNPermanents 2, WinTheGame 2, Adapt 1, AddOneManaOfColorAmongControlled 1, AddOneManaOfImprintedColors 1, AddOneManaSacrificedLandCouldProduce 1, AddOneOfManaCombinations 1, AdditionalCombatPhaseAfterThisPhase 1, AdjustSourceLoyalty 1, Airbend 1, Amass 1, AnOpponentDrawsCards 1, AtBeginningOfNextEndStep 1, Backup 1, BalanceEffect 1, BecomeSaddled 1, BindEventPlayer 1, Cascade 1, ChooseNameExileThenRevealUntilNamed 1, ChooseNameRevealTopPutNamedIntoHand 1, ChooseNoMode 1, ChooseOpponentsExiledCardWithCounterMayPlayFree 1, ConditionalIfCastFromHand 1, CopyEachTokenYouControlEnteredThisTurn 1, CopyTargetInstantOrSorcerySpellYouControl 1, CopyThisSpell 1, CounterThenDelayedManaEqualToMV 1, CounterThenWhenItAttacksMonarch 1, CreateAttackingTokenForEachOpponent 1, CreateEmblem 1, CreateEmblemWithStatics 1, CreateThenTheyGainKeywordsUEOT 1, CreateThisTurnTriggeredAbility 1, CreateTokenCopyOfTargetPermanent 1, CreateTokenCopyOfTargetTokenYouControl 1, CreateTokenCopyOfTargetWithKeywordsSacAtEndStep 1, CreateTokenPerFormula 1, CreateTokenThenMayAttachSelf 1, CreateTokensEqualToSourcePower 1, CreateTriggersUntilYourNextTurn 1, DealCombatDamageAmountToTarget 1, DealDamageByFormula 1, DealDamageExileIfDiesThisTurn 1, DealDamagePaidInEnergy 1, DealDamageThenChainCopy 1, DealDamageToControllerAndAddMana 1, DealNDamageToEachCreature 1, DealPowerDamageToEachOfUpToKickedTargets 1, DealXDamage 1, DefendingPlayerSacrificesPermanents 1, DestroyTargetNoncreaturePermanent 1, DiscardThenDrawTokensPerNonland 1, DiscardYourHand 1, DoublePlusOneCountersOnEach 1, DrawCardsPerCounterOnSource 1, DrawDiscardThenTokensPerDiscardedType 1, DrawHalfLibraryRoundedUp 1, DrawPerOpponentWhoLostLifeThisTurn 1, EachChoosesOneOfEachTypeSacrificesRest 1, EachOpponentChoiceDestroyOneAtRandom 1, EachOpponentSacrificesCreature 1, EachOpponentSacrificesMatching 1, EachOpponentSacrificesN 1, EachPlayerDiscardsHand 1, EachPlayerExilesHandDrawsNReturnsLater 1, EachPlayerExilesUntilNonlandMayCastFree 1, EachPlayerPutsCardFromGraveyardOntoBattlefield 1, EachPlayerReturnsCardsExiledWithSource 1, Earthbend 1, EnduringDiesReturnAsEnchantmentEffect 1, EventSubjectGetsUEOT 1, ExchangeControlOfTargets 1, ExileAllCreaturesThenIncubate 1, ExileEachOpponentsGraveyard 1, ExileGraveyardCardCounterIfPermanent 1, ExileInsteadOfGraveyardThisTurn 1, ExileLinkedToSource 1, ExileRandomGraveyardCardWithPlayPermission 1, ExileTargetGraveyardCardWithPlayPermission 1, ExileTargetPlayersLibraryThenShuffleHandIn 1, ExileTargetUntilSourceLeaves 1, ExileThatCreature 1, ExileThisSpell 1, ExileTopCardMayCastOtherwise 1, ExileUntilNonlandThenCounterOrMayCastIt 1, ExileUntilOpponentBecomesMonarch 1, ExileUpToOneTargetCardFromGraveyard 1, ExileUpToOneTargetGraveyardCardWithSource 1, ExiledCardOwnerCreatesManaValueToken 1, Explore 1, FlipCoinIfLost 1, FlipCoinStealOrCounterTargetSpell 1, GainClassLevel 1, GainControlOfTargetPermanently 1, GainControlOfTargetThenAttachIfEquipment 1, GainControlOfTriggeringSpellTargets 1, GainProtectionFromEverythingUntilYourNextTurn 1, GetEnergy 1, GetExperienceCounter 1, GrantAbilityToPermanentsYouControlUEOT 1, GrantActivatedAbility 1, GrantExtraLoyaltyActivationsThisTurn 1, GrantKeywordsUEOTAndPutCounters 1, GrantKeywordsUEOTThenExplores 1, GrantUnblockableUEOT 1, HarnessSelf 1, HideawayLookExileOneFaceDown 1, IfSourceHasNoCounters 1, ImprintCardFromHand 1, LookAtHandExileNonlandOwnerMayPlayTaxed 1, LookAtHandMayExileNonlandUntilSourceLeaves 1, LookAtRandomCardInTargetPlayersHand 1, LookAtTargetPlayersHand 1, LookAtTopCardOfTargetPlayersLibrary 1, LookAtTopCardOfTargetPlayersLibraryMayBottom 1, LookAtTopCardsDistribute 1, LookAtTopRepeatPayLifeThenStackTop 1, LookTopNKeepUpToOneOnTopRestBottom 1, LootThenMayCastDiscardedFreeIfCounters 1, LoseLifeEqualToEventAmount 1, LoseLifePerCounterOnSource 1, ManifestDread 1, MayCastSorceriesAsThoughFlashUntilYourNextTurn 1, MayCastSpellFromGraveyardNow 1, MayCastTargetGraveyardCardThisTurn 1, MayCastTargetGraveyardCardThisTurnExileInstead 1, MayExileThatCardFromYourGraveyard 1, MayMillDamageThenMayTakeCreature 1, MayPlayFromGraveyardThisTurn 1, MayTapOrUntapTarget 1, MillChooseOpponentAttackPump 1, MillThenMayPutLandIntoHand 1, MillTwoRepeatWhileShareColor 1, Mobilize 1, ModalChooseOneNotYetChosen 1, Monstrosity 1, OathRevealUntilCreature 1, OpponentGainsControlOfSource 1, OptionalManaCostThenEffect 1, OptionalPutLandFromHandOntoBattlefield 1, OwnerPutsTargetSecondFromTopOrBottom 1, OwnerPutsTargetSpellOnTopOrBottom 1, OwnerShufflesGraveyardIntoLibrary 1, PayEnergy 1, PayToCopyEnchantedCreatureElseToken 1, PlayExiledCardFreeIfAnyLibraryAtMost 1, PoisonCountersEqualToDamage 1, PumpTargetCreatureByFormulaUEOT 1, PumpTargetUntilYourNextTurn 1, PutCardFromHandOntoBattlefield 1, PutCardYouOwnWithCounterFromExileIntoHand 1, PutChargeCountersOnSelf 1, PutCounterDoublingOnSecondResolution 1, PutCountersOnTargetAndAddSubtype 1, PutCountersUntapItBecomesCreature 1, PutCreatureFromHandSacrificeUnlessPay 1, PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl 1, PutLoyaltyCounterOnSelf 1, PutPlusOneCounterOnCreatureYouControl 1, PutPlusOneCounterOnEachCreatureYouControl 1, PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords 1, PutPlusOneCounterOnEachOfUpToNTargetCreatures 1, PutPlusOneCounterOnOpponentsCreature 1, PutPlusOneCounterOnTargetCreature 1, PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower 1, PutSourceOnTopOfOwnersLibrary 1, PutStunCounterOnTappedCreature 1, PutTargetIntoOwnersLibraryNthFromTop 1, ReanimateExiledWithSourceFinalityHasteSacrifice 1, ReanimateTargetCreatureCardFromAGraveyard 1, ReconfigureAttach 1, ReconfigureUnattach 1, RemoveACounterFromSelf 1, RemoveAllCountersAddManaPerCounter 1, RemoveNamedCounterFromSelfOrSacrifice 1, ReturnAllPermanentsToOwnersHands 1, ReturnCardsExiledWithSourceToOwnersHand 1, ReturnChosenGraveyardCardToHand 1, ReturnCreatureFromYourGraveyardToBattlefield 1, ReturnCreatureFromYourGraveyardToHand 1, ReturnExiledWithSourceToGraveyardThenToken 1, ReturnTargetGraveyardCardToHand 1, ReturnTargetSpellOrNonlandPermanentToHand 1, ReturnTargetSpellToOwnersHand 1, ReturnThisCardFromGraveyardThenAttachToThatCreature 1, ReturnThisCardFromGraveyardToBattlefield 1, RevealHandMayExileCardOrChosenCreatureUntilSourceLeaves 1, RevealTopLandToBattlefieldElseHand 1, RevealTopNOnePerCardTypeToHand 1, RevealTopNOpponentChoosesOneRestExiledWithCounter 1, RevealUntilCreatureOntoBattlefieldAttacking 1, SacrificeAPermanent 1, SacrificeArtifactTutorArtifactPayDifference 1, SacrificeCreatureThenDamageEqualToItsPower 1, SacrificeSelfIfNoNamedCounters 1, SearchAnyNumberExileCastFreeThisTurn 1, SearchAnyNumberOntoBattlefield 1, SearchLibraryForGreenCreatureWithMVAtMostX 1, SetBasePTUntilEOT 1, ShuffleSourceIntoOwnersLibrary 1, ShuffleThenExileTopMayPlayFree 1, ShuffleYourLibrary 1, SourceBecomesCopyOfTargetGraveyardCardUEOT 1, SourceBecomesPrepared 1, SourceCantBeBlockedThisTurn 1, SourceConnives 1, SourceDealsLoyaltyDamageToChosen 1, SwapArtifactWithGraveyardArtifact 1, Tap 1, TapSelf 1, TapTargetPutStunCounters 1, TapUpToOneTargetCreature 1, TargetBecomesCopyOfAnotherTargetUEOT 1, TargetBecomesCreatureIndefinitely 1, TargetConnives 1, TargetOpponentCreatesToken 1, TargetOpponentDiscardsCasterChoice 1, TargetOpponentExilesThreeMayCastOneFree 1, TargetOpponentLosesLife 1, TargetOpponentMayLetYouDrawElseMill 1, TargetOpponentMills 1, TargetPlayerDraws 1, TargetPlayerDrawsThenDiscards 1, TargetPlayerExilesTopNYouMayPlayThem 1, TargetPlayerLosesLife 1, TargetPlayerMayCopyThisSpell 1, TargetPlayerPerformsActions 1, TargetPlayerPutsGraveyardOnBottomRandom 1, TargetPlayerTakesExtraTurn 1, ThatPlayerDrawsAndGainsControlOfSource 1, ThenItsControllerDraws 1, ThenTargetBecomesCreatureIfNot 1, ThenTargetControllerResolves 1, ThenTargetLosesAbilitiesWhileSourceRemains 1, UntapAllMatching 1, UntapChosenPermanents 1, UntapTargetArtifactOrCreatureConditionalCounter 1, UntapTargetOtherPermanent 1, UntapUpToNLands 1, UntapXTargets 1, VoteExileNonlandPermanent 1, YouAndTargetOpponentEachDraw 1, YourPermanentsBecomeCreaturesUEOT 1

**predicate** (81 classes): IsSelf 162, IsCreature 107, And 79, Controlled 79, ObjectAnd 70, CardFilter 66, Subject 48, IsArtifact 42, IsPlayer 42, ObjectNot 42, IsLand 40, ObjectOr 37, IsPlaneswalker 35, IsBattle 21, ControlledByYou 16, IsEnchantment 16, ControlledByOpponent 12, IsPermanent 12, IsSource 10, StateBound 10, HasSubtype 9, Source 9, Not 8, IsPermanent 5, IsThisSpell 5, WasEvoked 5, HasPowerAtMost 4, HasPrintedCardType 4, IsAttacking 4, IsLegendary 4, IsNontoken 4, Or 4, Always 3, CompletedADungeon 3, IsBasic 3, IsTapped 3, CardTypeOrSubtypeFilter 2, CardsInGraveyardAtLeast 2, CastOrCopy 2, DrawIsFirstInOwnDrawStep 2, EnteringWithXAtLeast 2, HasColor 2, HasInitiative 2, HasManaValueAtMost 2, HasPowerAtLeast 2, OneOrMoreBatch 2, PutIntoGraveyardFromAnywhere 2, ThisSpellWasKicked 2, CardsExiledFromLibraryOrGraveyard 1, CastAnotherSpellThisTurn 1, ControlsAtMostNamed 1, Delirium 1, EnteredNotFromZones 1, EnteredViaCast 1, FirstAttackThisTurn 1, GainedLifeThisTurn 1, HasCitysBlessing 1, HasDrawnNCardsThisTurn 1, HasKeyword 1, HasManaValueAtLeast 1, HasManaValueAtMost 1, HasManaValueAtMostColorsSpent 1, HasPowerToughness 1, HasTotalPowerAndToughnessAtMost 1, HasToughnessAtMost 1, HostAttacksWithAnother 1, IsColorless 1, IsFormerHost 1, IsHarnessed 1, IsMonarch 1, LandsPlayedThisTurnAtLeast 1, ManaValueAtMostSourceX 1, NoncreatureSpellsCastThisTurn 1, NthCardDrawnThisTurn 1, OffspringCostPaid 1, OpponentHasMoreCardsInHand 1, SpellMVHasSourceParity 1, SpellTargetsMatching 1, SpellsCastThisTurn 1, SquadCostPaid 1, YouControlAtLeast 1

**condition** (25 classes): ControlsAtMost 10, PerControlled 6, AlternativeCostWasPaid 3, ControlPermanent 3, DeliriumActive 3, SpellWasKicked 3, HandSizeAtMost 2, CardTypesAmongExiledWithSourceAtLeast 1, CardsInYourGraveyardAtLeast 1, ControlOtherPermanentOfColor 1, FirstSpellYouCastThisGame 1, FormulaCoversLibrary 1, HandSizeExactly 1, LevelCountersInRange 1, LifeAtMostHalfStartingLife 1, NameStickerGate 1, OptionalAdditionalCostPaid 1, PerBasicLandTypeAmongLandsYouControl 1, PermanentLeftYourControlThisTurn 1, SourceHasCounter 1, SpellCastFromGraveyard 1, SpellCastFromOutsideHand 1, StateCondition 1, YourLibraryHasNoCards 1, YourTurnOfGameAtMost 1

**formula** (11 classes): CountControlledMatching 6, ResolvingX 4, ResolvingDamageAmount 2, Scaled 2, CountAttackingCreatures 1, CountPermanentsOfPlayer 1, DevotionTo 1, EventSubjectManaValue 1, EventSubjectPower 1, ExperienceCounters 1, MVOfCostPaidObjectPlus 1

**cost** (8 classes): AlternativeCost 9, CostReduction 4, AdditionalCostOption 2, ExileSelfFromGraveyardCost 2, ExternalCostReduction 2, RemoveCountersFromSelfCost 1, SacrificePermanentsCost 1, TapUntappedPermanentsCost 1

**target** (2 classes): TargetSpec 129, OneOf 21

**modifier** (5 classes): EndOfTurn 4, ContinuousModifier 3, GrantKeyword 2, CantBeBlocked 1, PowerToughnessBonus 1

**transform** (7 classes): EventPattern 6, ReplacementEffect 6, Replace 2, AddToCount 1, InsteadForSourceController 1, MultiplyCount 1, WinTheGameInstead 1

**enum** (2 classes): Event 220, _Controller 79

**other** (8 classes): ControllerSearchesForBasicLand 3, ControllerGainsLifeEqualToPower 2, BeginGameOnBattlefield 1, ControllerCreatesTokens 1, IfYouControlledItDraw 1, LevelerPT 1, OtherPlayersCreateTokenCopy 1, PowerEqualsCount 1

**CardPrinting flag fields** (140): mana_only_from_produces 74, color_indicator 32, enters_tapped 27, hand_activated_abilities 24, loyalty 23, enters_tapped_unless 14, shock_land_life_cost 14, transform_back_face 12, face_colors 10, alternative_costs 9, equipment_bonuses 9, enters_with_named_counters 7, kicker_cost 6, kicker_cost_str 6, dynamic_pt_formula 5, evoke_exile_from_hand 5, landcycling 5, mdfc_back_face 5, produces_life_cost 5, doesnt_untap_during_untap_step 4, escape_cost 4, escape_cost_str 4, escape_exile_count 4, spell_is_uncounterable 4, additional_cost_pay_x_life 3, additional_cost_sacrifice_filter 3, enters_with_counters_from_x 3, flashback_cost 3, flashback_cost_str 3, grants_play_lands_from_graveyard 3, graveyard_activated_abilities 3, watched_in_graveyard 3, additional_cost_options 2, adventure_face 2, can_only_block_creatures_with_flying 2, chapter_count 2, cost_reductions 2, dash_cost 2, dash_cost_str 2, enters_as_copy_types 2, external_cost_reductions 2, grants_additional_land_per_turn 2, has_delve 2, opponents_cant_cast_spells_during_your_turn 2, opponents_cant_draw_more_than_one_card_each_turn 2, splice_cost 2, splice_cost_str 2, splice_onto 2, ward_cost 2, ward_cost_str 2, ward_life_cost 2, warp_cost 2, warp_cost_str 2, additional_cost_exile_from_graveyard 1, alt_cost_exile_from_hand 1, artifact_ability_cost_reduction 1, aura_enchant_type 1, bestow_cost 1, bestow_cost_str 1, bestow_enchanted_bonus 1, blocks_only_with_subtype 1, buyback_cost 1, buyback_cost_str 1, cant_be_blocked_by 1, chooses_card_name_as_enters 1, chooses_parity_as_enters 1, chooses_player_as_enters 1, class_level 1, class_levels 1, creature_during_your_turn 1, crew_power_bonus 1, daybound 1, each_player_one_spell_per_turn 1, emerge_cost 1, emerge_cost_str 1, emerge_sacrifice_type 1, enters_as_copy_add_subtypes 1, enters_as_copy_add_type 1, enters_as_copy_base_pt 1, enters_as_copy_exiles_card 1, enters_as_copy_keep_name 1, enters_as_copy_zone 1, enters_choosing_basic_land_type 1, enters_only_by_discarding_land 1, enters_prepared 1, enters_with_counter_per_kick 1, equipment_bonus_per_controlled 1, equipment_bonus_per_own_counter 1, equipment_granted_triggers 1, exiles_uncast_nontoken_creatures_entering 1, extra_mana_when_you_tap_creature_for_mana 1, extra_mana_when_you_tap_land_subtype 1, free_cast_condition 1, grants_graveyard_escape_exile_count 1, grants_look_at_top_of_library 1, grants_mayhem_to_graveyard 1, grants_once_per_turn_graveyard_play_mv 1, grants_play_from_top_paying_life 1, grants_retrace_to_nonland_permanents_during_your_turn 1, has_graveyard_triggers 1, has_improvise 1, has_storm 1, heals_other_damage_when_dealt_damage 1, is_chosen_basic_land_type 1, locks_activated_abilities_of 1, loyalty_abilities_at_instant_speed_if_entered_this_turn 1, may_skip_turn_to_untap 1, min_blockers 1, miracle_cost 1, miracle_cost_str 1, multikicker 1, named_ability_cost_increase 1, named_spell_cost_increase 1, nightbound 1, offspring_cost 1, offspring_cost_str 1, opening_hand_action 1, opponents_cant_make_you_discard_or_sacrifice 1, opponents_cast_spells_only_at_sorcery_speed 1, opponents_nonbasic_lands_enter_tapped 1, paints_all_objects_chosen_color_at_etb 1, plot_cost 1, plot_cost_str 1, prepare_spell 1, produces_with_counter 1, reconfigure_cost 1, reconfigure_cost_str 1, replicate_cost 1, sacrifice_at_cleanup_if_cast_without_sorcery_timing 1, spell_cost_increases 1, split_faces 1, squad_cost 1, squad_cost_str 1, sunburst 1, teamwork_power 1, void_exile_replacement 1, ward_discard_chosen 1, watches_library_graveyard_exile 1, x_min 1, your_creatures_combat_damage_cant_be_prevented 1

**ActivationCost fields** (30): tap_self 122, sacrifice_self 59, loyalty 22, discard_self 20, pay_life 14, extra_costs 5, remove_named_counter_from_self 5, discard_a_card 4, sacrifice_a_land 4, crew 3, requires_control_of_subtype 3, exile_self 2, remove_counters_from_self_amount 2, remove_counters_from_self_kind 2, return_unblocked_attacker 2, sacrifice_a_permanent_type 2, sacrifice_subtype 2, discard_hand 1, exert_self 1, exile_cards_from_graveyard 1, requires_own_artifact_count_at_least 1, requires_own_creature_power_at_least 1, sacrifice_another_artifact 1, sacrifice_another_creature_or_artifact 1, sacrifice_n_nonland_permanents 1, sacrifice_nontoken_artifact 1, saddle 1, tap_permanent_type 1, tap_permanent_you_control 1, waterbend 1

**StaticAbility fields** (13): power_bonus 8, toughness_bonus 7, granted_keywords 4, bonus_per_condition_count 3, activation_cost_reduction 1, granted_becomes_target_effect 1, granted_trigger_limit_per_turn 1, grants_no_max_hand_size 1, grants_spells_uncounterable 1, grants_ward_life_cost 1, graveyard_permanent_cast_mv_max 1, must_attack_each_combat 1, opponents_enter_tapped_types 1

**ActivatedAbility fields** (8): is_mana_ability 75, sorcery_speed 44, activate_only_if 2, cost_reductions 2, activate_only_as_instant 1, activate_only_during_your_turn 1, boast 1, exhaust 1

**TriggeredAbility fields** (2): once_per_turn 3, functions_in_graveyard 1

**Keywords (CardPrinting.keywords strings)** (26): flying 41, flash 23, trample 21, haste 16, lifelink 15, menace 12, vigilance 11, deathtouch 7, reach 7, indestructible 5, double strike 3, defender 2, exalted 2, first strike 2, annihilator 6 1, ascend 1, battle cry 1, cascade 1, devoid 1, hexproof 1, infect 1, persist 1, protection from spells that are one or more colors 1, protection from the chosen player 1, prowess 1, shadow 1

**Trigger/replacement Event values** (34): ENTERS 125, ATTACKS 33, IS_CAST 20, DIES 14, DEALS_COMBAT_DAMAGE 13, BEGINNING_OF_UPKEEP 12, LEAVES_BATTLEFIELD 10, BEGINNING_OF_COMBAT 7, YOU_ATTACK 7, BEGINNING_OF_END_STEP 6, DRAWS_CARD 5, BECOMES_TARGET 2, CARD_DISCARDED 2, DEALT_DAMAGE 2, PLAYS_LAND 2, WOULD_GO_TO_GRAVEYARD 2, ATTACKERS_UNBLOCKED 1, BECOMES_MONSTROUS 1, BECOMES_TAPPED 1, BEGINNING_OF_DRAW_STEP 1, BEGINNING_OF_PRECOMBAT_MAIN 1, BLOCKS 1, CARDS_PUT_INTO_LIBRARY 1, COUNTERS_ADDED 1, CREATURE_TOKENS_CREATED 1, DEALS_COMBAT_DAMAGE_TO_CREATURE 1, DEALS_DAMAGE 1, DISCARDS_CARD 1, IS_SACRIFICED 1, TRANSFORMS 1, WOULD_CREATE_TOKEN 1, WOULD_DRAW_A_CARD 1, WOULD_DRAW_CARDS 1, WOULD_DRAW_FROM_EMPTY_LIBRARY 1

**StaticAbility.scope strings** (6): self 19, other_creatures_you_control 2, card_type_you_control:Land 1, creatures_you_control 1, subtype_you_control:all:Ninja 1, subtype_you_control:other:Dwarf 1

**TriggeredAbility.trigger string keys** (4): chapter_1 2, chapter_2 2, chapter_3 2, class_level_2 1
