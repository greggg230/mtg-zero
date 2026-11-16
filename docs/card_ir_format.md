# Card IR v1: the format

The card IR is a lossless, versioned JSON form of the oracle parser's
output: every parsed `CardPrinting` of a pool, as data. It exists so
that the Rust rules engine (crate `card_ir`, `docs/card_ir.md`) can load
the parsed cards without a per-class bridge. Code: `ir/mtgz_ir/cards/ir.py`.
Tests: `tests/ir/test_card_ir.py`.

This is mtg-bot's `docs/CARD_IR.md`, moved with the pipeline (docs/ENGINE.md 169).
The format's names (`mtg-bot-card-ir`, `mtg-bot-card-ir-schema`) and the
schema's module labels (`mtg_bot.<module>` for the package now called
`mtgz_ir`, `ir.SCHEMA_MODULE_PACKAGE`) are the first exporter's, kept so
the schema hash and every checked-in file stay as they were. Below,
`mtg_bot.cards` / `mtg_bot.effects` in the encoding rules mean today's
`mtgz_ir.cards` / `mtgz_ir.effects`.

## Exporting

```bash
python tools/card_ir.py export      # the three pools, one schema, generated.rs (docs/card_ir.md)
python tools/card_ir.py check       # a regeneration equals the checked-in files, byte for byte
python tools/card_ir.py roundtrip   # every printing decodes back to an identical object
```

`mtgz_ir.export.export_ir(pools, out)` writes `<stem>.ir.json` per pool
and the schema once (`powerlsv.schema.json`) and returns a summary
(cards, classes, bytes, gzip bytes, schema hash). The pools are `cube`,
`legacy`, `eoe`, `roto` and `all` (`ir/mtgz_ir/cards/pools.py`); the stem
is `powerlsv` for the cube, `vintage_roto` for roto, else the pool name.
`roundtrip` checks, for each pool, that every printing decodes back to an
identical object (types included) and that re-encoding is a fixpoint.

For the PowerLSV cube (546 printings) the document is about 400 KB of
JSON (about 62 KB gzipped).

## Document

```json
{
  "format": "mtg-bot-card-ir",
  "ir_version": 1,
  "pool": "cube",
  "schema_sha256": "<sha256 of the canonical schema JSON>",
  "cards": [
    {"oracle_id": "<Scryfall oracle_id>", "name": "Ancestral Recall", "printing": {"$t": "CardPrinting", ...}},
    ...
  ]
}
```

Cards are sorted by name. `oracle_id` is the Scryfall oracle id; a
hand-authored custom card (roto's Meddling Wizard) has `oracle_id: null`
and `"custom": true`.

## Encoding rules

* **Objects.** A dataclass (or plain-class) instance is
  `{"$t": <tag>, <field>: <value>, ...}`. The tag is the class's
  `__qualname__`; a name that more than one class in `mtg_bot.cards` /
  `mtg_bot.effects` defines is qualified with its module's last
  component (`"cube_walkers.IsPermanent"` vs
  `"value_predicates.IsPermanent"`).
* **Sparse fields.** A field equal to its declared default is omitted;
  the schema records every default.
* **Tuples** are JSON arrays. **Frozensets** are arrays sorted by the
  canonical JSON of their elements. **Enum** members are their (string)
  values. **Infinity** is the string `"inf"` (only
  `TargetSpec.count_max`, "any number of targets").
* **Keywords.** Keyword-valued slots (annotated `KeywordSpelling`) hold
  the engine's keyword spelling in Python (`"annihilator 6"`) and a
  structured `Keyword` object in the IR:
  `{"$t": "Keyword", "kind": "annihilator", "n": 6}`,
  `{"$t": "Keyword", "kind": "protection from", "quality": "the chosen player"}`.
  The vocabulary is `mtg_bot.keyword_vocab`.
* **Derived attributes are not serialized.** Eight effects compute a
  `targets` attribute in `__post_init__` from their fields (for example
  `ThenTargetControllerResolves` copies `first.targets`). It is a pure
  function of the fields, so the IR leaves it out (`DERIVED_ATTRIBUTES`);
  the Python decoder re-runs `__post_init__`, and a consumer recomputes
  it in its own implementation of those classes.
* **Not in IR v1:** `TriggeredAbility.trigger`, the legacy string-key
  trigger path. No parsed card sets it (every trigger is `event` +
  `predicate`); encoding a non-empty value is an error.
* **Hard errors** (`IRError`): a callable, class, mutable container
  (`list` / `set` / `dict`), non-string enum, NaN, a class missing from
  the schema, an attribute that is neither a field nor derived, a value
  outside its slot's type, an unknown keyword spelling.

## Schema

`<name>.schema.json`:

```json
{
  "format": "mtg-bot-card-ir-schema",
  "ir_version": 1,
  "tag_key": "$t",
  "derived_attributes": ["targets"],
  "categories": {"Effect": ["AddManaToPool", ...], "CardPrinting": ["CardPrinting"], ...},
  "classes": {
    "DrawCards": {
      "module": "mtg_bot.effects.generic.draw_mill", "qualname": "DrawCards",
      "category": "Effect", "kind": "dataclass",
      "fields": [{"name": "amount", "annotation": "int", "type": {"t": "int"},
                  "type_source": "annotation", "required": false, "default": 1}, ...]
    }, ...
  },
  "enums": {"Event": {"module": "...", "qualname": "Event",
                      "members": [{"name": "ENTERS", "value": "enters"}, ...]}, ...}
}
```

The schema covers every class reachable from the encoded pool (plus
classes of object-valued field defaults). Field types:

| `t` | Meaning | JSON |
|---|---|---|
| `bool`, `int`, `float`, `str` | scalars | as is |
| `count` | int, or infinity | number or `"inf"` |
| `enum` (+ `enum`) | a member of that enum | its string value |
| `keyword` | a keyword spelling | a `Keyword` object |
| `ref` (+ `categories`) | an object of any class in those categories | `{"$t": ...}` |
| `opt` (+ `of`) | `of` or none | value or `null` |
| `list` / `set` (+ `of`) | tuple / frozenset | array |
| `tuple` (+ `items`) | fixed-arity tuple | array |
| `union` (+ `items`) | one of several JSON-distinguishable types | as the arm |
| `never` | a slot no encoded card fills | (no value can appear) |

Types come from the dataclass annotations. Where an annotation is vague
(`object`, a bare `tuple`) the builder completes it from the values
observed in the pool (`type_source`: `annotation+observed` or
`observed`). Every observed value is checked against the final type; an
annotation that rejects a value the parser produces is recorded as
`annotation_mismatch` (the test suite requires none), and a union whose
arms JSON cannot tell apart is an error.

**Categories.** A class's category is the first of these bases in its
MRO: `Effect`, `ObjectPredicate`, `StatePredicate`, `TriggerPredicate`,
`Condition`, `Formula`, `PTFormula`, `LayerPayload`, `EventTransform`;
else `CostAtom` for the duck-typed `ActivationCost.extra_costs` atoms
(`payable` + `pay`); else its topmost `mtg_bot` base (records such as
`CardPrinting`, `ManaCost`, `TargetSpec`, `StaticScope`, `Keyword` are
their own category). A `ref` slot names categories, so a consumer can
model each category as one tagged union.

## Versioning

* `ir_version` is the **format** version: the encoding rules above. A
  change to them (a new value encoding, a new header field) bumps
  `IR_VERSION` in `ir.py`, and decoders refuse other versions.
* `schema_sha256` identifies the **content** vocabulary: classes,
  fields, types, defaults, enum members. Any parser or primitive change
  that alters them changes the hash. The Python decoder refuses a
  document whose hash differs from the schema it is given; the Rust
  crate embeds the hash of the schema its types were generated from and
  refuses other documents. Refresh: `python tools/card_ir.py export`
  re-exports and regenerates the Rust types (`docs/card_ir.md`).

## Closed vocabularies

IR v1 was frozen after cleaning the string-keyed spots the card-IR
survey found (`docs/research/card_ir_survey.md` section 2.3):

* `CardPrinting.dynamic_pt_formula` is always a `PTFormula` object
  (`effects/pt_formulas.py`), never a string key.
* Tezzeret, Cruel Captain's emblem is an inline
  `CreateEmblemWithAbilities`; `CreateEmblem(emblem_kind=...)` is gone.
* `CreateEmblemWithAbilities.printing_fields` (a reflective
  `dataclasses.replace` patch) is the explicit field `grants_retrace_to`.
* Saga chapters and Class levels are events (`Event.SAGA_CHAPTER` +
  `ChapterReached(n)`, `Event.CLASS_LEVEL_GAINED` +
  `ClassLevelReached(n)`); Requiem Monolith's granted trigger is
  `Event.DEALT_DAMAGE` + `IsSelf("obj")`.
* `StaticAbility.scope` is a structured `StaticScope` (`ScopeKind` plus
  `subtype` / `other` / `card_type`).
* Keywords: `keyword_vocab.KeywordKind` + parameters (above).
* String discriminants are `StrEnum`s (`effects/vocab.py`), coerced in
  each class's `__post_init__`, so an unknown value is a construction
  error. A `StrEnum` member compares and hashes equal to its value, so
  engine comparisons are unchanged; values copied into runtime game
  state are stored as plain strings (`vocab.plain`).

* Companion (CR 702.139): `CardPrinting.companion` is a `Companion`
  record (category `Companion`) holding its deck condition, a
  `DeckCondition` (`effects/deck_conditions.py`):
  `EachCardInStartingDeck { cards, must }` (two object predicates: Lurrus
  `IsPermanentCard` / `HasManaValueAtMost(2)`, Zirda `IsPermanentCard` /
  `HasActivatedAbility`) or `StartingDeckDifferentNames { cards }` (Lutri:
  `ObjectNot(IsLand)`). The {3} of the special action (CR 116.2g) is a
  rule, not data. Other companion conditions still parse to nothing.

* Murktide Regent (pool `legacy`, the mtg-zero Delver mirror's maindeck
  cards outside the cube): `CardPrinting.enters_with_counter_per_card_exiled_with_it`
  (an object predicate: a +1/+1 counter for each matching card exiled to
  pay the cost of the spell that became the permanent, CR 607.2q, 614.1c)
  and `Event.LEAVES_GRAVEYARD` (subjects `player`, the graveyard's owner,
  and `card`, the card as it was there; CR 603.10a). Prismari Charm's
  "deals 1 damage to each of one or two targets" is the effect
  `DealDamageToEachTarget`. Data for mtg-zero; the Python engine refuses to
  play the two Murktide abilities (SEARCH_LIMITATIONS, "Legacy pool" 1).

* Several pools, one schema: `export_ir(["cube", "legacy", "eoe"], out)`
  builds the schema over the pools' printings and writes `powerlsv.ir.json`,
  `legacy.ir.json`, `eoe.ir.json` and `powerlsv.schema.json` (one generated
  consumer reads every document). The cube's cards encode exactly as in a cube-only export;
  only `schema_sha256` differs.

Strings that remain open by design: counter names (`"charge"`,
`"corpse"`), subtypes and card names, display labels, mana-cost strings
next to their parsed costs, and target-kind tags (`"graveyard_creature"`,
`"my_creature"`), which form one engine-wide namespace.
