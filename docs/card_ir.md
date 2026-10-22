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
| `data/cards/powerlsv.ir.json` | The PowerLSV cube (546 printings), IR v1 |
| `data/cards/powerlsv.schema.json` | The schema that IR was encoded with |
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
PYTHONPATH=src ../../mtg-bot/.venv-py313/Scripts/python.exe -m mtg_bot cards roundtrip --pool cube
PYTHONPATH=src ../../mtg-bot/.venv-py313/Scripts/python.exe -m mtg_bot cards export-ir \
    --pool cube --out "C:/Users/Gregg Keithley/mtg-zero/data/cards"

# 2. In mtg-zero:
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

## Current numbers (mtg-bot `feature/card-ir-v1`)

546 cards; 506 classes in 32 categories; 29 enums; 5,472 tagged objects
(1,119 effects, 757 object predicates, 519 trigger predicates, 50
conditions, 24 formulas, 5 P/T formulas, 5 cost atoms, 231 keywords,
641 printings including faces and tokens). The IR is 402 KB (62 KB
gzipped); `generated.rs` is about 8,200 lines and builds in about 30 s.
