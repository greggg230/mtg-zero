# Card-IR defects for mtg-bot's parser (M7b)

Status: 2026-10-22, engine at main 7c014ca + the M7b card tests (branch `m7b-tests`); IR
`data/cards/powerlsv.ir.json` (546 cube cards, 360 supported). Updated 2026-10-23 for M7b part 2
(cards with more than one face, branch `m7b-faces`, 394 supported): entries 9, 13 and 14; mtg-bot read at commit
`3073a633` (branch main). The IR is exported by mtg-bot (`mtg_bot.cards.ir`, `python -m mtg_bot
cards export-ir`) from the card printings its oracle-text parser builds (`src/mtg_bot/cards/parser/`)
and the effect dataclasses (`src/mtg_bot/effects/`). This file lists where the IR says something
other than the Oracle text, or leaves out what a consumer needs, with the parser code that produces
it and a suggested fix. File references below are relative to `mtg-bot/src/mtg_bot/`.

How the list was made:

* the support report's blockers (`SUPPORT_DUMP=<file> cargo test --release -p engine --test
  support -- --nocapture`), keeping those that point at an inconsistent or wrong IR rather than a
  missing engine feature (`...(out of range 0)`, `...(missing for a coloured land)`,
  `...(not an Equipment)`, `...(land without a mana ability)`, ...), each checked against the
  Oracle text;
* a cross-check of every card's IR against its Scryfall object (mana cost, every `X` / `X_str`
  cost pair, type line, power and toughness, evergreen keywords, the mana a land makes,
  planeswalker loyalty costs, integer sentinels);
* reading the IR of the 118 supported cards the M7b card tests cover (`engine/tests/
  cards_m7b_tests*.rs`) next to their Oracle text. Those tests found no IR defect in a supported
  card: every one of the 118 behaves as its Oracle text says.

Each entry: the card and its Oracle text, what the IR says, what it should say, where in mtg-bot it
comes from, a suggested parser fix, and the engine-side status (blocked, or worked around in the
lowering). "Confirmed" means the IR contradicts or drops Oracle text; "suspected" means the IR is
representable but misleading (a misnamed field, a sentinel, a keyword standing in for a
permission) and a consumer can go wrong with it.

## Summary

| # | Card(s) | Kind | IR says | Should say | mtg-bot source | Engine status | |
|---|---|---|---|---|---|---|---|
| 1 | Strip Mine | ability dropped | only `{T}, sac: DestroyTargetLand`; no mana ability, no `produces` | also `{T}: Add {C}` (as Wasteland) | `cards/parser/land_builder.py::_try_build_tap_sac_utility_land` | blocked | confirmed |
| 2 | Celestial Colonnade, Creeping Tar Pit | fields dropped | `BecomeLandCreatureUEOT` without `set_colors`, `granted_subtype` | `set_colors: [W, U]` / `[U, B]`, `granted_subtype: "Elemental"` | `cards/parser/land_builder.py::_try_build_manland` | blocked | confirmed |
| 3 | Get Lost | value resolved late | `ControllerCreatesTokens { token_name: "Map", count: 2 }` | the Map token's `CardPrinting` inline | `effects/generic/targeted_removal.py::ControllerCreatesTokens`, `cards/parser/removal_phrases.py` | worked around | confirmed |
| 4 | Robber of the Rich | default left in | `ExileTopCardsWithPlayPermission.duration` = default `this_turn` | `while_exiled` | `cards/parser/creature_rmc_grammar.py::_permission_kwargs` | worked around | confirmed |
| 5 | Sowing Mycospawn | lossy schema | `kicker_cost: { generic: 2 }` for "Kicker {1}{C}" | a colorless-only pip | `cards/base.py::parse_mana_cost`, `ManaCost` | not detected (card blocked elsewhere) | confirmed |
| 6 | Thieving Skydiver (and 6 more effect classes, 7 more cards) | target spec not exported | `GainControlOfTargetThenAttachIfEquipment {}` | its `TargetSpec` (artifact with mana value X or less) | `effects/generic/cube_creatures_ub.py` (`targets` set in `__post_init__`) | blocked / re-derived in the lowering | confirmed |
| 7 | Animate Dead | misnamed field | Aura bonus "-1/-0" in `equipment_bonuses` | an Aura (or attachment) bonus field | `cards/base.py::make_aura` | blocked | suspected |
| 8 | Necromancy | permission as keyword | `keywords: [flash]` | "may cast as though it had flash" | `cards/parser/cube_enchantments.py::_reanimation_aura_lines` | blocked elsewhere | suspected |
| 9 | transform DFCs (Aang, Ajani, Cecil, Concealing Curtains, Etali, Fable, Jace VP, Outland Liberator, Tamiyo IS, Ulvenwald Oddity) | value copied | back face `mana_cost` = the front face's | no mana cost (mana value from the front, CR 712.8e) | `cards/parser/dfc_builder.py::transform_back` | worked around (read as no mana cost) | suspected |
| 10 | Fastbond, Pest Infestation, Wizard's Rockets | sentinel values | `1000000`; `count_max: 0` + `count_from_x`; `amount: 0` + `amount_is_x` | explicit "any number" / "X" | `cards/parser/cube_enchantments.py`, `cards/parser/removal_phrases.py`, `effects/generic/artifact_mechanics.py` | blocked | suspected |
| 11 | many (Agent Bishop, Cryogen Relic, Kolaghan's Command, ...) | targets only in code | effect classes with no `targets` | target requirements as data | effect classes' `requires_target` / `legal_targets` | re-derived per class in the lowering | suspected (systemic) |
| 12 | Minsc & Boo, Timeless Heroes | colour dropped | Boo's token printing with no `color_indicator` (a colourless Boo) | `color_indicator: [R]` | `cards/parser/effects_ext.py::_CREATE_NAMED_TOKEN`, `_token_printing` | blocked (`lower::IR_DEFECTS`) | confirmed |
| 13 | Ajani, Nacatl Pariah // Ajani, Nacatl Avenger | reflexive trigger flattened | the 0: `Sequence [CreateToken, IfCondition(ControlOtherPermanentOfColor, DealDamageFromEvent → any target)]` | a reflexive triggered ability (CR 603.12) after the token, its target chosen when it triggers | `cards/parser/layout_face_grammar.py` (`_WHEN_YOU_DO`) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 14 | The Sensational She-Hulk; Enduring Innocence | one flag, two rules | `TriggeredAbility.once_per_turn: true` for both | "do this only once each turn" (603.2h) apart from "this ability triggers only once each turn" | `TriggeredAbility.once_per_turn` (`turn.py::should_fire_trigger`) | told apart by the effect's shape | suspected |

## Confirmed defects

### A. Abilities or fields dropped by a whole-card builder

**1. Strip Mine.** Oracle: "{T}: Add {C}. / {T}, Sacrifice this land: Destroy target land."

* IR: `activated_abilities` holds only `{T}, sacrifice: DestroyTargetLand`; no mana ability, no
  `produces`, no `mana_only_from_produces`. Python plays it anyway through its "any land pays one
  generic" fallback.
* Should be: a `{T}: AddManaToPool [C, 1]` mana ability (or `produces: ["C"]` with
  `mana_only_from_produces`), as Wasteland's IR has.
* Source: `cards/parser/land_builder.py::_try_build_tap_sac_utility_land` (lines ~1170-1212,
  called from line 883). It claims two-line lands whose first line is "{T}: Add {C}." and whose
  second line is in `_TAP_SAC_SELF_BODIES` (line 1137: "Destroy target land", ...), and builds
  `make_utility_land(name, activated_abilities=(ActivatedAbility(cost=ActivationCost(tap_self=True,
  sacrifice_self=True), effect=factory()),))`: the first line is matched and then dropped
  (`make_utility_land`'s `produces` defaults to empty, `cards/base.py:2658`).
* Fix: delete `_try_build_tap_sac_utility_land` (and its call): Wasteland's body is not in the
  table and falls through to the line-composed land grammar (`cards/parser/land_lines.py`), which
  builds both abilities correctly; Strip Mine would take the same path. Minimal alternative: pass
  `produces=frozenset({"C"})` / the `{T}: Add {C}` ability in every branch of the function. Same
  table, same bug, outside the cube: Petrified Field, Dust Bowl, High Market.
* Engine: blocked, `CardPrinting.mana_only_from_produces(land without a mana ability)`
  (`engine/src/lower/printing.rs::lower_printing`); `tests/rules_lands.rs::
  strip_mine_is_blocked_by_its_missing_mana_ability`.

**2. Celestial Colonnade, Creeping Tar Pit.** Oracle: "{3}{W}{U}: Until end of turn, this land
becomes a 4/4 white and blue Elemental creature with flying and vigilance. It's still a land." /
"{1}{U}{B}: Until end of turn, this land becomes a 3/2 blue and black Elemental creature. It's
still a land. It can't be blocked this turn."

* IR: `BecomeLandCreatureUEOT { power, toughness, granted_keywords | unblockable }`, no
  `set_colors`, no `granted_subtype`: the animated land would be a colourless creature with no
  creature type.
* Should be: `set_colors: ["U", "W"]` / `["B", "U"]` and `granted_subtype: "Elemental"`. The class
  has both fields (`effects/generic/become_type.py:165, 180`); Restless Vinestalk, which goes
  through the generic grammar, has them in its IR.
* Source: `cards/parser/land_builder.py::_try_build_manland` (lines 1019-1100). Its regexes
  (`_MANLAND_ANIMATE_RE` etc., lines ~129-161) capture the colour words `c1word` / `c2word` only to
  check them against the mana ability's colours (lines 1074-1078) and spell "Elemental" as literal
  text; the constructor call (lines 1090-1097) passes neither `set_colors` nor `granted_subtype`.
* Fix: pass `set_colors=frozenset({c1_animate, c2_animate})` and `granted_subtype` (capture the
  creature type instead of matching the literal), or drop `_try_build_manland` so these lines reach
  the generic recognizer `_THIS_LAND_BECOMES_CREATURE_UEOT` (`cards/parser/effects.py:5632-5656`),
  which already fills both.
* Engine: blocked, `BecomeLandCreatureUEOT.set_colors(missing for a coloured land)`
  (`engine/src/lower/effect_ext.rs::lower_effect_ext`, the `BecomeLandCreatureUEOT` arm: a land
  that makes coloured mana and animates without colours is refused rather than made colourless).
  The missing `granted_subtype` is not detected by the engine.

### B. Values the IR leaves to Python

**3. Get Lost.** Oracle: "Destroy target creature, enchantment, or planeswalker. Its controller
creates two Map tokens."

* IR: rider `ControllerCreatesTokens { token_name: "Map", count: 2 }` with no token printing;
  the other token makers in the IR carry one (`CreateToken.token_printing`,
  `CreateTokensScaledByX.token_printing`).
* Should be: the Map printing inline (an artifact token with "{1}, {T}, Sacrifice this token: Target
  creature you control explores. Activate only as a sorcery.").
* Source: `effects/generic/targeted_removal.py:172-185` (`ControllerCreatesTokens` stores
  `token_name` and looks up `TOKEN_PRINTINGS[self.token_name]` in `apply()`), built by
  `cards/parser/removal_phrases.py:116-125`, which already checks `name in TOKEN_PRINTINGS`.
* Fix: give the rider a `token_printing: CardPrinting` field set to `TOKEN_PRINTINGS[name]` at
  parse time (and use it in `apply()`), as `CreateToken` does.
* Engine: worked around: `engine/src/lower/effect_ext.rs::named_token` holds a built-in Map
  printing (`MAP_TOKEN`); `tests/cards_m7a_threats.rs::get_lost_its_controller_creates_two_map_tokens`.

**6. Target specifications set in `__post_init__` (Thieving Skydiver and others).** Oracle
(Skydiver): "When this creature enters, if it was kicked, gain control of target artifact with
mana value X or less. If that artifact is an Equipment, attach it to this creature."

* IR: `{"$t": "GainControlOfTargetThenAttachIfEquipment"}`, empty: the target requirement (an
  artifact with mana value at most the kicker's X) appears nowhere.
* Source: `effects/generic/cube_creatures_ub.py:937-950`: the class declares no fields and sets
  `object.__setattr__(self, "targets", (TargetSpec(IsArtifact() & ManaValueAtMostSourceX()),))`
  in `__post_init__`; the exporter serializes dataclass fields only. Classes that declare a
  `targets` field and fill it the same way (`RemoveTargetPermanents`, `DealDamageAnyTarget`, ...)
  export correctly.
* The same pattern, with the IR node exported without `targets`: `ThenTargetControllerResolves`
  (`effects/combinators.py:41`; Boseiju), `LookAtTopCardOfTargetPlayersLibrary`
  (`effects/generic/artifact_mechanics.py:470`; Mishra's Bauble), `LookAtRandomCardInTargetPlayersHand`
  (`:498`; Urza's Bauble), `MillTwoRepeatWhileShareColor` (`:657`; Grindstone),
  `FlickerUpToOneOtherTargetNonlandPermanentYouControl` (`:1217`; Displacer Kitten, The Mind
  Stone), `OwnerPutsTargetSecondFromTopOrBottom` (`effects/generic/cube_creatures_ub.py:994`; Wan
  Shi Tong).
* Fix: declare `targets: tuple = ()` as a dataclass field on these classes (keeping the
  `__post_init__` assignment), or make the exporter emit a non-empty `targets` attribute whether or
  not it is a declared field. The Skydiver's filter also needs an exportable predicate for "mana
  value at most the source's X".
* Engine: Thieving Skydiver is blocked (`Effect:GainControlOfTargetThenAttachIfEquipment`, plus its
  kicker); the supported ones (Mishra's Bauble, Urza's Bauble, Grindstone, Displacer Kitten) work
  because the lowering re-derives each target from the class name (`engine/src/lower/
  effect_ext.rs::lower_effect_ext`, the arms of those classes).

### C. A default left in place of the Oracle text

**4. Robber of the Rich.** Oracle: "Whenever this creature attacks, if defending player has more
cards in hand than you, exile the top card of their library. During any turn you attacked with a
Rogue, you may cast that card and you may spend mana as though it were mana of any color to cast
that spell."

* IR: `ExileTopCardsWithPlayPermission { any_color_mana, cast_only, requires_attacked_with_subtype:
  "Rogue", whose: "defending_player" }` with the default `duration` (`this_turn`): read literally,
  the permission ends with the turn.
* Should be: `duration: "while_exiled"` (the enum has it, `effects/vocab.py:200`): the card stays
  castable on any later turn you attack with a Rogue.
* Source: `cards/parser/creature_rmc_grammar.py:279-288` (`_permission_kwargs`): the
  `_ATTACKED_WITH_SUBTYPE_PERMISSION` match returns `cast_only`, `requires_attacked_with_subtype`,
  `any_color_mana` and no `duration`, so `ExileTopCardsWithPlayPermission.duration` keeps its
  default `THIS_TURN` (`effects/generic/cube_creatures_rmc.py:474`). Python plays it right only
  because `ExilePlayPermission.expired_after_cleanup` special-cases `requires_attacked_with_subtype`.
* Fix: add `"duration": "while_exiled"` to that kwargs dict.
* Engine: worked around in `engine/src/lower/effect_ext.rs::lower_effect_ext`
  (`ExileTopCardsWithPlayPermission` arm: `this_turn` is false when `requires_attacked_with_subtype`
  is set); `tests/cards_m7a_zones2.rs::robber_of_the_rich_cast_with_any_mana_during_a_turn_you_attacked_with_a_rogue`.

### D. A schema that can't say it

**5. Sowing Mycospawn.** Oracle: "Kicker {1}{C}".

* IR: `kicker_cost: { generic: 2 }` while `kicker_cost_str` is `"{1}{C}"`. `{C}` can be paid only
  with colorless mana (CR 107.4c, 106.1b); as generic 2 it could be paid with any mana.
* Should be: one generic and one colorless-only pip.
* Source: `cards/base.py:378` (`parse_mana_cost`): `elif sym == "C": generic += 1` (line 391-392);
  `ManaCost` (`cards/base.py:266`) has `generic`, `colored`, `x_pips`, `hybrid`, `phyrexian` and no
  colorless field. Every cost (casting, kicker, activation) goes through this function; in the cube
  only Sowing Mycospawn has a `{C}` cost (Mana Drain's `{C}` is mana it adds).
* Fix: add `colorless: int = 0` to `ManaCost`, count `{C}` there in `parse_mana_cost`, export it
  (the IR's `ManaCost` gains the field), and pay it with colorless mana only.
* Engine: not detected. The card is blocked for other reasons (`CardPrinting.kicker_cost(not "if
  kicked ... else")`, `Keyword:Devoid`, `TriggerPredicate:ThisSpellWasKicked`); once those are done
  the kicker would lower as `{2}`. The engine's `ManaCost` has a `{C}` pip, but
  `engine/src/lower/printing.rs::lower_printing` ignores the `*_str` fields ("display form").
  Suggested engine-side guard (rule 4a of ENGINE.md 3): parse each `*_cost_str` / `mana_str` and
  record a blocker when it differs from the structured cost.

### E. A token's colour dropped by the token-phrase parser

**12. Minsc & Boo, Timeless Heroes.** Oracle: "When Minsc & Boo enters and at the beginning of
your upkeep, you may create Boo, a legendary 1/1 red Hamster creature token with trample and
haste."

* IR: both `CreateToken.token_printing`s (the entering and the upkeep trigger) name Boo, legendary,
  1/1, Hamster, trample and haste, but have no `color_indicator`: Boo would be colourless.
* Should be: `color_indicator: ["R"]`.
* Source: `cards/parser/effects_ext.py`: `_CREATE_NAMED_TOKEN` (line ~343) matches the colour with
  a non-capturing group `(?:{_COLOR})`, and `_token_printing` (line ~449) passes no colours to
  `cards/base.py::make_token_creature` (which takes `colors=`). `_CREATE_STAT_TOKEN` (line ~338)
  drops the colour the same way; a scan of every token printing in the cube IR against its card's
  Oracle text finds Boo as the only coloured creature token that comes out colourless (the other
  tokens are built elsewhere, e.g. from `TOKEN_PRINTINGS`).
* Fix: capture the colour words in both patterns and pass them through `_token_printing` to
  `make_token_creature(colors=...)`.
* Engine: blocked by `lower::IR_DEFECTS` (`IR:CreateToken.token_printing.color_indicator(Boo is
  red)`, ENGINE.md 24.8); the card's tests run on a corrected copy of its IR
  (`engine/tests/cards_m7b_walkers2.rs::minsc_db`), so it becomes supported once the IR is fixed
  (and the registry gets a red Boo line).

### F. A reflexive trigger flattened into the ability (M7b part 2)

**13. Ajani, Nacatl Pariah // Ajani, Nacatl Avenger.** Oracle (Ajani, Nacatl Avenger): "0: Create
a 2/1 white Cat Warrior creature token. When you do, if you control a red permanent other than
Ajani, he deals damage equal to the number of creatures you control to any target."

* IR: the loyalty ability's effect is `Sequence [CreateToken (Cat Warrior), IfCondition {
  condition: ControlOtherPermanentOfColor, then: DealDamageFromEvent { amount:
  CountControlledMatching(IsCreature), targets: [any target] } }]`. The "any target" is then a
  target of the loyalty ability, chosen as it is activated (before the token exists), and the
  ability can't be activated without a legal target for it; the condition is checked only as
  the ability resolves.
* Should be: a reflexive triggered ability (CR 603.12): after the token is created, "when you do"
  triggers; its intervening "if" (603.4) is checked as it triggers and as it resolves, and its
  target is chosen when it is put on the stack. With no red permanent the 0 still makes a Cat
  Warrior and asks for no target.
* Source: `cards/parser/layout_face_grammar.py` (`_WHEN_YOU_DO`, line ~140): "`<first>`. When
  you do, `<then>`." returns `Sequence(effects=(first, then))`, with the comment "The reflexive
  ability's targets are chosen with the enclosing ability (the MayDoThenReflexive precedent;
  see docs/SEARCH_LIMITATIONS.md)", a Python engine shortcut written into the IR.
* Fix: export the reflexive form as data, as `ReflexiveWhenYouDo` / `MayDoThenReflexive` already
  do for "you may ... When you do" (a non-optional variant: `first`, the reflexive effect, its
  targets), and keep the Python shortcut in the Python engine.
* Engine: blocked by `lower::IR_DEFECTS` (`IR:IfCondition(a reflexive trigger flattened: Ajani's
  0)`, ENGINE.md 25.11); every ability of the card is tested on a renamed copy of its IR
  (`engine/tests/cards_m7b_faces4.rs`, "Test Ajani"), so it becomes supported once the IR is
  fixed and the lowering reads the reflexive form (the engine has reflexive triggers, ENGINE.md
  23.2).

## Suspected defects (representational)

**7. Animate Dead.** Oracle: "... Enchanted creature gets -1/-0."

* IR: `equipment_bonuses: [-1, 0, []]` on an Aura.
* Source: `cards/base.py::make_aura` (lines ~2246-2269) reuses the Equipment bonus machinery on
  purpose ("we reuse the Equipment bonus machinery (equipment_bonuses) so effective_* helpers pick
  them up automatically"); the field's own comment says "None on non-Equipment cards"
  (`cards/base.py:843-846`). The line is parsed by `cards/parser/cube_enchantments.py:258`
  (`_enchanted_creature_gets`).
* Fix: rename the field to something attachment-neutral (`attached_bonuses`) or export Auras'
  bonus under its own field (as `bestow_enchanted_bonus` already is). Every Aura built with
  `make_aura(power_bonus=...)` is affected.
* Engine: blocked, `CardPrinting.equipment_bonuses(not an Equipment)` (`engine/src/lower/
  printing.rs::lower_printing`), with the Aura features (`aura_enchant_type`, the reanimation
  effects).

**8. Necromancy.** Oracle: "You may cast this spell as though it had flash. If you cast it any time
a sorcery couldn't have been cast, the controller of the permanent it becomes sacrifices it at the
beginning of the next cleanup step."

* IR: `keywords: [{kind: "flash"}]` plus `sacrifice_at_cleanup_if_cast_without_sorcery_timing`.
  A permission to cast it as though it had flash is not the flash keyword (CR 702.8a: flash is a
  keyword ability, which Necromancy doesn't have; anything that asks whether a card has flash
  would see it).
* Source: `cards/parser/enchantment_phrases.py::parse_reanimation_aura_line` (lines ~143-174)
  returns `{"flash": True, ...}` for the literal line; `cards/parser/cube_enchantments.py:277-279`
  (`_reanimation_aura_lines`) turns it into `acc.keywords.add("flash")`.
* Fix: a printing flag such as `cast_as_though_flash: bool`, not a keyword.
* Engine: blocked by other features (Aura reanimation, the cleanup sacrifice); lowered as is, it
  would have flash.

**9. Transforming double-faced cards: the back face's mana cost.** Scryfall back faces have no mana
cost; the IR's `transform_back_face.mana_cost` is the front face's (with `mana_cost_str: ""`):
Aang, Swift Savior // Aang and La, Ocean's Fury; Ajani, Nacatl Pariah // Ajani, Nacatl Avenger;
Cecil, Dark Knight; Concealing Curtains; Etali, Primal Conqueror; Fable of the Mirror-Breaker;
Jace, Vryn's Prodigy; Outland Liberator; Tamiyo, Inquisitive Student; Ulvenwald Oddity.

* Source: `cards/parser/dfc_builder.py:89-98` (`transform_back`): `mana_cost=parse_mana_cost(
  front_face["mana_cost"])`, on purpose for the mana value (CR 712.8e).
* Why it misleads: the back face has no mana cost; only its mana value is computed from the front
  face's mana cost (CR 712.8e, the rule the code comment cites). A `mana_cost` of `{2}{R}` on
  Reflection of Kiki-Jiki reads as a cost it can be cast for and gives the face a mana cost for
  anything that inspects mana costs.
* Fix: keep the back face's `mana_cost` empty and export the mana value separately (or let
  consumers take it from the front face).
* Engine (M7b part 2, ENGINE.md 25.1): worked around. The lowering reads a back-face mana cost
  equal to the front's with an empty `mana_cost_str` (or an empty cost) as "no mana cost" and
  takes the mana value from the front face; any other back-face cost is a blocker
  (`CardPrinting.transform_back_face.mana_cost(not the front face's)`). All ten cards have the
  copied shape; seven of them are supported (Cecil, Concealing Curtains, Etali, Jace VP,
  Outland Liberator, Tamiyo IS, Ulvenwald Oddity). Modal double-faced back faces carry their
  own mana cost and are not affected.

**10. Sentinel values.**

* Fastbond ("You may play any number of lands on each of your turns."):
  `grants_additional_land_per_turn: 1000000` (`cards/parser/cube_enchantments.py:121`,
  `ANY_NUMBER_OF_LAND_PLAYS = 1_000_000`). Fix: an explicit `unlimited_land_plays: bool`. Engine:
  blocked, `CardPrinting.grants_additional_land_per_turn(out of range 1000000)`.
* Pest Infestation ("Destroy up to X target artifacts and/or enchantments. ..."):
  `RemoveTargetPermanents { count_from_x: true, count_min: 0, count_max: 0 }` and its `TargetSpec
  { count_min: 0, count_max: 0 }` (`cards/parser/removal_phrases.py:206-214`: `count_max = 0` as
  a placeholder "fixed at cast time"). A `TargetSpec` with `count_max: 0` reads as "no targets".
  Fix: a `count_max_from_x` flag on the `TargetSpec` and no numeric 0. Engine: blocked,
  `TargetSpec.count_max(out of range 0)`, `RemoveTargetPermanents.count(differs from targets)`,
  `RemoveTargetPermanents.count_from_x`, `Effect:CreateTokensScaledByX`.
* Wizard's Rockets ("{X}, {T}, Sacrifice this artifact: Add X mana in any combination of
  colors."): `AddManaInAnyCombination { amount_is_x: true }` with the default `amount: 0`
  (`effects/generic/artifact_mechanics.py:109-111`; built at `cards/parser/artifact_effects.py:137`).
  Fix: omit `amount` (or export it as null) when `amount_is_x`. Engine: blocked,
  `AddManaInAnyCombination.amount(out of range 0)`, `AddManaInAnyCombination.amount_is_x`,
  `ActivationCost.mana.x_pips`.

**11. Target requirements that live only in code (systemic).** Many effect classes carry no
`targets` in the IR because Python computes them in methods (`requires_target`,
`target_count_range`, `legal_targets`), e.g. `PutPlusOneCounterOnEachOfUpToNTargetCreatures`
(Agent Bishop: "each of up to two target creatures", `effects/generic/counters.py:181-205`),
`PutStunCounterOnTappedCreature` (Cryogen Relic: "up to one target tapped creature",
`effects/generic/counters.py:596`), `ExileTargetCardFromGraveyard` (Soul-Guide Lantern),
`ReturnCreatureFromYourGraveyardToHand` and `TargetPlayerDiscards` (Kolaghan's Command),
`GrantFlashbackToTargetCardUEOT` (Flashback), `ReturnTargetSpellToOwnersHand` (Reprieve),
`CounterSpell` (Mana Tithe). The IR is then not self-describing: a consumer has to know each
class's targets. The engine re-derives them per class in `engine/src/lower/effect.rs` and
`effect_ext.rs` (for the cards above the M7b tests check the targets against the Oracle text).
Suggested direction: export a `TargetSpec` list for every targeting effect (computed from the same
methods at export time), so new classes don't need a hand-written lowering of their targets.

**14. `TriggeredAbility.once_per_turn` stands for two rules.** The Sensational She-Hulk: "Whenever
a creature you control is dealt damage, you may have The Sensational She-Hulk deal that much
damage to any target. Do this only once each turn." Enduring Innocence: "Whenever one or more
other creatures you control with power 2 or less enter, draw a card. This ability triggers only
once each turn."

* IR: `once_per_turn: true` on both triggered abilities.
* The rules differ: "do this only once each turn" (CR 603.2h) limits the action: the ability
  triggers again after a turn's trigger the player declined, and stops triggering only once the
  player has done it. "This ability triggers only once each turn" limits the triggering itself.
  Python implements the second for both (`turn.py::should_fire_trigger` records the first
  trigger), so a She-Hulk trigger declined once can't be taken later that turn.
* Fix: two fields (`once_per_turn_action`, `triggers_once_per_turn`), or an enum.
* Engine: told apart by the effect's shape. A `once_per_turn` trigger whose effect starts with
  "you may" (`Op::May { You }`) is lowered as "do this only once each turn" (`Ext::OnceMay`, CR
  603.2h; She-Hulk is supported); any other is a blocker
  (`TriggeredAbility.once_per_turn(not "you may ... only once each turn")`, Enduring Innocence).

## Checked and not defects

* `CardPrinting.kicker_cost(not "if kicked ... else")` (Batroc the Leaper, Sowing Mycospawn,
  Thieving Skydiver): the IR states kicker through the `ThisSpellWasKicked` trigger predicate and
  multikicker, which the engine doesn't lower yet; the IR matches the Oracle text (apart from #5
  and #6).
* `CardPrinting.spell_effect(missing)` (Life // Death): a split card; the faces carry the effects
  in `split_faces` (Death's `ReturnCardFromGraveyardToBattlefield` omits `card_filter` because the
  schema default is a creature card). Supported since M7b part 2: the card-level mana cost and
  types are the halves' sum and union, as CR 709.4 says.
* Modal double-faced cards that can also transform (Jennifer Walters // The Sensational She-Hulk,
  Norman Osborn // Green Goblin): the back face is described twice, as `mdfc_back_face` and
  `transform_back_face`; the two are equal (the lowering checks it) and each carries the back
  face's own mana cost, as it should for a modal face (CR 712.8f).
* `ActivationCost.sacrifice_n_nonland_permanents(out of range 10)` (Bolas's Citadel): 10 is the
  Oracle text; the range is the engine's.
* Planeswalker `ActivationCost.loyalty` is the signed loyalty *cost* (a "+1" ability is
  `loyalty: -1`); consistent over all 23 planeswalker faces and documented in
  `effects/base.py:~540-560`.
* Inti's `MayDoThenReflexive { first: DiscardSelf }`: `DiscardSelf` means "you discard a card"
  (`effects/generic/discard.py:30`), not "discard this card".
* Fastbond's `LandsPlayedThisTurnAtLeast` without `n`: the schema default is 2 ("if it wasn't the
  first land you played this turn").
* `TriggerPredicate.subject(Land / Defender / TargetSpell)` (City of Traitors, Coveted Jewel,
  Surrak), `ExileTopCardsWithPlayPermission.duration(UntilYourNextEndStep)` (Inti),
  `LinkExiledCardsToSource.inner(...)` (Keen-Eyed Curator), `RemoveAllMatching.action(Exile)`
  (Winds of Abandon), `Keyword.quality(SpellsThatAreOneOrMoreColors)` (Emrakul): the IR matches
  the Oracle text; these are engine features not built yet.
* Fast lands' `enters_tapped_unless: ControlsAtMost(IsLand, 2)` says "lands", not "other lands";
  it is evaluated before the land enters, so the two agree (ENGINE.md 19).
* Hybrid pips are stored in alphabetical order (`{W/B}` as `["B", "W"]`); same meaning.
