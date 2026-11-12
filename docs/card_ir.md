# Card IR in mtg-zero (`card_ir` crate)

The Rust rules engine reads cards as data: mtg-bot's oracle parser
exports every parsed card of a pool as the **card IR** (JSON), and the
`card_ir` crate deserializes it into typed Rust values. No per-card or
per-class bridge: the Rust types are generated from the schema mtg-bot
exports alongside the IR (DESIGN.md section 3.2). The format itself is
specified in mtg-bot's `docs/CARD_IR.md`.

## What is checked in

| Path | What |
|---|---|
| `data/cards/powerlsv.ir.json` | The PowerLSV cube (546 printings), IR v1, exported by mtg-bot `feature/legacy-delver-ir` 140eafa9 with the legacy pool (schema `181ba4b8a1e7...`, ENGINE.md 54; the cards byte for byte those of `feature/companion-ir` 62210e89, schema `7100f8436556...`: companion conditions, M8a; before it `feature/ir-fixes-mtgzero` 8ea9fcca, schema `66ba3e33fe5b...`, the IR defects of docs/ir_defects.md 1, 2, 4, 12, 13, 15-18 fixed) |
| `data/cards/legacy.ir.json` | The legacy cards of constructed play (ENGINE.md 54: Murktide Regent, Prismari Charm; ENGINE.md 110.1: Doomsday, Pact of Negation, Personal Tutor, Flow State; mtg-bot's pool `legacy`), the same export and schema |
| `data/cards/eoe.ir.json` | The Edge of Eternities sealed cards (ENGINE.md 99.1: mtg-bot's pool `eoe` filtered by `tools/codegen/filter_eoe_ir.py` to the set's 320 cards with IR), the same export and schema; since ENGINE.md 99.16 the export of mtg-bot `feature/eoe-ir-fixes` f4f2d663 (before it 7c02e5c9, ENGINE.md 99.11; 5362f5ed, ENGINE.md 99.9) |
| `data/cards/powerlsv.schema.json` | The schema the three documents were encoded with |
| `tools/codegen/gen_card_ir.py` | Schema -> Rust generator (stdlib Python) |
| `card_ir/src/generated.rs` | Generated types (do not edit) |
| `card_ir/src/support.rs` | Hand-written `Count` (int or `"inf"`) and `Never` |
| `card_ir/src/lib.rs` | `Document` / `Card` and `load_document` |
| `card_ir/tests/cube.rs` | Loads every cube card, prints per-category counts |

## The generated types

* Each Python class becomes a struct in `card_ir::nodes`
  (`#[serde(deny_unknown_fields)]`, so an unknown field is an error).
  Fields the IR omits (sparse encoding) take the schema default:
  `#[serde(default)]` when it equals Rust's `Default`, else a generated
  function in `card_ir::generated::defaults`.
* Each **category** becomes a tagged enum in `card_ir::cats`
  (`#[serde(tag = "$t")]`), one newtype variant per class:
  `cats::Effect`, `cats::ObjectPredicate`, `cats::TriggerPredicate`,
  `cats::StatePredicate`, `cats::Condition`, `cats::Formula`,
  `cats::PTFormula`, `cats::CostAtom`, `cats::LayerPayload`,
  `cats::EventTransform`, and one single-variant enum per record class
  (`cats::CardPrinting`, `cats::ManaCost`, `cats::TargetSpec`,
  `cats::Keyword`, ...). An unknown `$t` is a deserialization error. A
  slot typed with several categories gets its own enum over all their
  classes (e.g. `cats::ObjectPredicateOrOneOf`).
* Python enums become `card_ir::enums` (variants renamed to the Python
  string values): `Event`, `KeywordKind`, `ProtectionQuality`,
  `ScopeKind`, `RemovalAction`, `ZoneName`, ...
* `int | <object>` slots become untagged enums in `card_ir::unions`.
* Object-valued fields are boxed (`Box<cats::Effect>`), sequences are
  `Vec`, optional slots `Option`.
* `support::Never` types a slot no encoded card fills (an empty tuple
  default, an always-`None` field): deserializing a value for it fails,
  so a parser change that starts using it fails loudly here until the
  schema is regenerated.

`card_ir::category_of(tag)` maps a `$t` tag to its category
(`TAG_CATEGORY`). `load_document` checks `format`, `ir_version` and that
the document's `schema_sha256` equals the hash the types were generated
from (`card_ir::SCHEMA_SHA256`), then decodes card by card so an error
names the card.

## Refreshing when the parser changes

Any parser or card-primitive change in mtg-bot can change the schema
(new classes, fields, enum members). Refresh all three files together:

```bash
# 1. In the mtg-bot checkout (a worktree: PYTHONPATH=src, shared venv):
PYTHONPATH=src ../../mtg-bot/.venv-py313/Scripts/python.exe -m mtg_bot cards roundtrip --pool cube legacy eoe
# (three pools, one schema: powerlsv.ir.json, legacy.ir.json, eoe.ir.json, powerlsv.schema.json;
# ENGINE.md 54, 99.1)
PYTHONPATH=src ../../mtg-bot/.venv-py313/Scripts/python.exe -m mtg_bot cards export-ir \
    --pool cube legacy eoe --out "C:/Users/Gregg Keithley/mtg-zero/data/cards"

# 2. In mtg-zero:
python tools/codegen/filter_eoe_ir.py --mtg-bot-data <mtg-bot>/data --ir data/cards/eoe.ir.json
python tools/codegen/gen_card_ir.py --schema data/cards/powerlsv.schema.json \
    --out card_ir/src/generated.rs
CARGO_TARGET_DIR=E:/cargo-target/mtg-zero/<branch> cargo test -p card_ir -- --nocapture
python -m pytest tools/codegen   # generated.rs is current (--check)
```

`export-ir` prints the schema hash; the regenerated `generated.rs`
carries the same hash in `SCHEMA_SHA256`. The codegen is deterministic,
so an unchanged schema leaves `generated.rs` byte-identical. Commit the
IR, the schema and `generated.rs` in one commit, noting the mtg-bot
commit they came from.

When mtg-bot bumps `ir_version` (a change to the encoding rules, not to
the card vocabulary), `support.rs`, `lib.rs` and the generator may need
changes too; see mtg-bot `docs/CARD_IR.md`, "Versioning".

## Current numbers (mtg-bot `feature/doomsday-a-ir` a544a075)

ENGINE.md 110.1 (Legacy Doomsday, lane A): exported with `cards export-ir --pool cube legacy eoe` from mtg-bot
`feature/doomsday-a-ir` a544a075 (on `feature/eoe-ir-fixes` f4f2d663), then `filter_eoe_ir.py`; schema sha256
`506c54f1e63e53718129009c43624e31f7b2c6af847dc9cd5faae701a4e73807` (was `1cbe33073b1b...`). 713 classes, four more:
`DoomsdayPile` and `LookAtTopPutIntoHandMoreIfInstantAndSorcery` (mtg-bot's roto classes, now used by Doomsday and Flow
State), `LoseTheGame` (roto), and `PayManaOrElse` (new: "pay <cost>. If you don't, <effect>"); the enum `DelayedStep`
gains `your_upkeep` ("at the beginning of your next upkeep"). The legacy pool gains four cards (Doomsday, Pact of
Negation, Personal Tutor, Flow State); docs/ir_defects.md 200 (a single-faced card's colour indicator dropped: Pact of
Negation's blue) is fixed. `powerlsv.ir.json` (546 cards, 402,200 bytes) and `eoe.ir.json` (320 cards, 214,200 bytes)
are the previous files byte for byte once the schema hash is replaced (checked card by card and as bytes);
`legacy.ir.json` holds 6 cards (3,489 bytes).

### Before ENGINE.md 110.1 (mtg-bot `feature/eoe-ir-fixes` f4f2d663)

ENGINE.md 99.16 (W6): exported with `cards export-ir --pool cube legacy eoe` from mtg-bot
`feature/eoe-ir-fixes` f4f2d663 (the third IR-fix batch, on aa323a3b), then `filter_eoe_ir.py`; schema
sha256 `1cbe33073b1b4c28528a98b7541fff5faa654c132627d86c15e4eeb403517543` (was `663b44095dbe...`).
709 classes, one more: the effect `MoveCounterBetweenTargets` (Nesting Grounds) and the trigger
predicate `CountersPutBy` (Terrasymbiosis) are new, `MovePlusOneCounterBetweenPermanentsYouControl`
(no card uses it now) is gone; the enum `EventSubject` gains `entering_obj` (Starfield Vocalist's
subject filter). The fixed defects are docs/ir_defects.md 27, 31, 66, 160 (Plaza of Heroes: the
third ability is `AddOneManaOfColorAmongControlled`) and 161 (Contested War Zone: combat damage to
a player). `powerlsv.ir.json` (546 cards, 402,200 bytes) and `legacy.ir.json` (2 cards) are the
previous files card for card, `schema_sha256` aside (checked card by card); `eoe.ir.json` holds the
same 320 cards, 5 of them changed (exactly the cards of those entries), 214,200 bytes.

### Before ENGINE.md 99.16 (mtg-bot `feature/eoe-ir-fixes` 20bea9f4)

ENGINE.md 99.11 (W1): exported with `cards export-ir --pool cube legacy eoe` from mtg-bot
`feature/eoe-ir-fixes` 20bea9f4 (the second IR-fix batch, on 7af12b20), then `filter_eoe_ir.py`;
schema sha256 `663b44095dbe1f30560a1ea30aecc00a1edf3c5c9bcefb4f4f4db47351fbcc3b` (was
`675cfc50fc4e...`). 708 classes: six new (`ReturnTargetCardsFromYourGraveyard` and its record
`GraveyardReturnGroup`, `ReturnTargetCardFromYourGraveyardToBattlefieldAs`,
`ReturnCardsExiledWithSourceToBattlefield`, the trigger predicate `LeftBattlefieldTo`, the layer
payload `CanAttackAsThoughNoDefender`), five no longer used by any card
(`ConditionalIfSourcePowerAtLeast`, `ReturnUpToNCreaturesFromYourGraveyardToHand`,
`ReturnUpToNNonlandPermanentCardsFromGraveyardToHand`,
`PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact`, `RemoveKeyword`); new fields
`CounterSpell.if_paid` / `if_mana_spent_below_mv`, `ReturnTargetCreatureToOwnersHand.if_was_attacking`
(it replaces `create_drone_if_was_attacking`), `CreateNTokensFromLastExileRecall.token_printing`,
`BecomeLandCreatureUEOT.all_creature_types`, `CardPrinting.enters_with_plus_one_counters_per` /
`attached_cant_be_blocked_by_more_than_one`; new enum members `EnchantKind.creature_or_spacecraft` /
`artifact_or_creature`, `KeywordKind.modular`, `SpendRestriction.artifact_spell_or_activation`;
`ReturnChosenGraveyardCardToHand.card_filter` takes a `CardFilterAnyOf` too. The fixed defects are
docs/ir_defects.md 21 (Luxknight Breacher), 24, 25, 28, 29, 30, 41 (Mutavault), 43, 44, 45, 47, 49,
50, 61, 62 (Archenemy's Charm), 65, 67, 80, 100, 101. `powerlsv.ir.json` (546 cards, 402,200 bytes)
and `legacy.ir.json` (2 cards) are the previous files card for card, `schema_sha256` aside (checked
card by card); `eoe.ir.json` holds the same 320 cards, 20 of them changed (exactly the cards of
those entries), 213,719 bytes. mtg-bot 7c02e5c9 (Sothera, the Supervoid's sacrifice before its return, W4's entry 180) changes that card's printing only, the schema and the other files unchanged.

### Before ENGINE.md 99.11 (mtg-bot `feature/eoe-ir-fixes` 5362f5ed)

ENGINE.md 99.9: exported with `cards export-ir --pool cube legacy eoe` from mtg-bot
`feature/eoe-ir-fixes` 5362f5ed (from main 2cb79f77), then `filter_eoe_ir.py`; schema sha256
`675cfc50fc4efd23b2fdb07a27ef999e67c06e86be409daee57b1c0d237eb259` (was `70c85fff4b77...`, the EoE
import of ENGINE.md 99.1 from mtg-bot main 2cb79f77, three pools; before it `181ba4b8a1e7...`).
707 classes, five more: `CardFilterAnyOf` (a library search's "<a ... card> or <a ... card>"),
`PowerOfCostPaidObject` (a formula), `DrawCardsEqualTo`, `CreateTokenCopyOfTargetWithExceptions` and
`CopyExceptions` (with `add_supertypes`); new fields `BecomeLandCreatureUEOT.granted_card_types` /
`granted_subtypes`, `ExileTargetCreatureUntilSourceLeaves.up_to_one`,
`OneOrMoreBatch.per_target_player`. The fixed defects are docs/ir_defects.md 20-23, 26, 40-42, 46,
48, 51, 60, 62-64, 81, 82 (some for part of their cards; each entry's last bullet).
`powerlsv.ir.json` (546 cards, 402,200 bytes) and `legacy.ir.json` (2 cards) are the previous files
card for card, `schema_sha256` aside (checked card by card); `eoe.ir.json` holds the same 320 cards,
53 of them changed (62 of the export's 483: the other 9 are Duskmourn cards the filter drops, whose
tokens gained their printed colour), 210,970 bytes.

### Before ENGINE.md 99 (mtg-bot `feature/legacy-delver-ir` 140eafa9)

D1 (ENGINE.md 54): exported with `cards export-ir --pool cube legacy`, one schema over both pools
(sha256 `181ba4b8a1e7b13c1dd77d7529ed4ecd32bbe27ca88d62bf98e6d02c9b5d31b6`): 514 classes (one more,
`DealDamageToEachTarget`), `CardPrinting.enters_with_counter_per_card_exiled_with_it` (an optional
object predicate) and `Event.leaves_graveyard`. The cube's 546 cards are those of 62210e89, byte for
byte (402,200 bytes, `schema_sha256` aside); `legacy.ir.json` holds 2 cards (1,905 bytes). Refresh both
files together (above: a cube-only export would carry another schema, which `card_ir` rejects for
the legacy file).

### Before D1 (mtg-bot `feature/companion-ir` 62210e89)

M8a (ENGINE.md 35): exported from mtg-bot branch `feature/companion-ir` at 62210e89 (from main
8ea9fcca); schema sha256
`7100f8436556f92a62df96f2ea7f01a22ee0d114ffa5086930b6705373be6026` (before: `66ba3e33...`).
Five classes more: `CardPrinting.companion` (an optional `Companion { condition }`, category
`Companion`), the category `DeckCondition` with `EachCardInStartingDeck { cards, must }` and
`StartingDeckDifferentNames { cards }`, and two object predicates, `IsPermanentCard` (CR 110.4a)
and `HasActivatedAbility`. Only the three companions' printings changed (checked card by card
against the previous export): Lurrus of the Dream-Den (`EachCardInStartingDeck(IsPermanentCard,
HasManaValueAtMost(2))`), Lutri, the Spellchaser (`StartingDeckDifferentNames(ObjectNot(IsLand))`),
Zirda, the Dawnwaker (`EachCardInStartingDeck(IsPermanentCard, HasActivatedAbility)`). 546 cards,
513 classes, 402,226 bytes (62,375 gzipped).

### Before M8a (mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca)

The IR in use since M7d's IR fixes (ENGINE.md 32): exported from mtg-bot
branch `feature/ir-fixes-mtgzero` at 8ea9fcca (from main 3073a633, the
commit of the first export); schema sha256
`66ba3e33fe5ba01a6f255a1ddb1a6234cb34e331574b9f2ee6699d39859c0fd0` (before:
`dcc39f8f...`, mtg-bot 3073a633). Two classes more than that schema:
`DoThenReflexive { first, then }` and `ShuffleThisSpellIntoOwnersLibrary {}`
(category Effect); no other class, field or enum changed.

546 cards; 508 classes in 32 categories; 29 enums; 5,471 tagged objects
(1,121 effects, 757 object predicates, 521 trigger predicates, 50
conditions, 24 formulas, 5 P/T formulas, 5 cost atoms, 231 keywords,
641 printings including faces and tokens). The IR is 402 KB (62 KB
gzipped); `generated.rs` is about 9,100 lines and builds in about 30 s.
