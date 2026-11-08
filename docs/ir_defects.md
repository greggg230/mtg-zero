# Card-IR defects for mtg-bot's parser (M7b)

Status: 2026-10-22, engine at main 7c014ca + the M7b card tests (branch `m7b-tests`); IR
`data/cards/powerlsv.ir.json` (546 cube cards, 360 supported). Updated 2026-10-23 for M7b part 2
(cards with more than one face, branch `m7b-faces`, 394 supported): entries 9, 13 and 14; mtg-bot read at commit
`3073a633` (branch main). Updated 2026-10-23 for M7b part 3 (copies, branch `m7b-copies`, 410 supported): no new
defect; entry 11 gains two classes, and the copy cards' Python-only switches are listed under "Checked and not
defects". Updated 2026-10-23 for M7b part 3b (the stack, ward, the initiative, graveyard abilities, branch
`m7b-stack`, 438 supported): no new defect; the 28 cards' notes are under "Checked and not defects". Updated 2026-10-23 for M7c tail part 1 (branch `m7c-tail1`, 467 supported): entry 15 (Chromatic Star). Updated 2026-10-24 for M7c tail part 2 (branch `m7c-tail2`, 491 supported): entries 16 (Forth Eorlingas!) and 17 (Green Sun's Zenith); entry 14's second card is now supported; Laelia's placeholder event under "Checked and not defects". Updated 2026-10-24 for M7c tail part 3 (branch `m7c-tail3`, 524 supported): entry 18 (Wizard's Rockets); entries 5, 6, 7, 8 and 10's cards (Sowing Mycospawn, Thieving Skydiver, Wan Shi Tong, Animate Dead, Necromancy, Fastbond, Pest Infestation) supported, the IR read faithfully with consistency checks; the tail's notes under "Checked and not defects". Updated 2026-10-25 (branch `m7d-ir`, 546 supported): the IR now in use is mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca (schema `66ba3e33fe5b`), which fixes entries 1, 2, 4, 12, 13, 15, 16, 17 and 18 (each entry's last bullet); the lowering reads the fixed forms and `lower::IR_DEFECTS` is empty: every cube card is supported. The IR is exported by mtg-bot (`mtg_bot.cards.ir`, `python -m mtg_bot
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
| 1 | Strip Mine | ability dropped | only `{T}, sac: DestroyTargetLand`; no mana ability, no `produces` | also `{T}: Add {C}` (as Wasteland) | `cards/parser/land_builder.py::_try_build_tap_sac_utility_land` | fixed (8ea9fcca), supported | confirmed |
| 2 | Celestial Colonnade, Creeping Tar Pit | fields dropped | `BecomeLandCreatureUEOT` without `set_colors`, `granted_subtype` | `set_colors: [W, U]` / `[U, B]`, `granted_subtype: "Elemental"` | `cards/parser/land_builder.py::_try_build_manland` | fixed (8ea9fcca), supported | confirmed |
| 3 | Get Lost | value resolved late | `ControllerCreatesTokens { token_name: "Map", count: 2 }` | the Map token's `CardPrinting` inline | `effects/generic/targeted_removal.py::ControllerCreatesTokens`, `cards/parser/removal_phrases.py` | worked around | confirmed |
| 4 | Robber of the Rich | default left in | `ExileTopCardsWithPlayPermission.duration` = default `this_turn` | `while_exiled` | `cards/parser/creature_rmc_grammar.py::_permission_kwargs` | fixed (8ea9fcca), supported | confirmed |
| 5 | Sowing Mycospawn | lossy schema | `kicker_cost: { generic: 2 }` for "Kicker {1}{C}" | a colorless-only pip | `cards/base.py::parse_mana_cost`, `ManaCost` | not detected (card blocked elsewhere) | confirmed |
| 6 | Thieving Skydiver (and 6 more effect classes, 7 more cards) | target spec not exported | `GainControlOfTargetThenAttachIfEquipment {}` | its `TargetSpec` (artifact with mana value X or less) | `effects/generic/cube_creatures_ub.py` (`targets` set in `__post_init__`) | blocked / re-derived in the lowering | confirmed |
| 7 | Animate Dead | misnamed field | Aura bonus "-1/-0" in `equipment_bonuses` | an Aura (or attachment) bonus field | `cards/base.py::make_aura` | blocked | suspected |
| 8 | Necromancy | permission as keyword | `keywords: [flash]` | "may cast as though it had flash" | `cards/parser/cube_enchantments.py::_reanimation_aura_lines` | blocked elsewhere | suspected |
| 9 | transform DFCs (Aang, Ajani, Cecil, Concealing Curtains, Etali, Fable, Jace VP, Outland Liberator, Tamiyo IS, Ulvenwald Oddity) | value copied | back face `mana_cost` = the front face's | no mana cost (mana value from the front, CR 712.8e) | `cards/parser/dfc_builder.py::transform_back` | worked around (read as no mana cost) | suspected |
| 10 | Fastbond, Pest Infestation, Wizard's Rockets | sentinel values | `1000000`; `count_max: 0` + `count_from_x`; `amount: 0` + `amount_is_x` | explicit "any number" / "X" | `cards/parser/cube_enchantments.py`, `cards/parser/removal_phrases.py`, `effects/generic/artifact_mechanics.py` | blocked | suspected |
| 11 | many (Agent Bishop, Cryogen Relic, Kolaghan's Command, ...) | targets only in code | effect classes with no `targets` | target requirements as data | effect classes' `requires_target` / `legal_targets` | re-derived per class in the lowering | suspected (systemic) |
| 12 | Minsc & Boo, Timeless Heroes | colour dropped | Boo's token printing with no `color_indicator` (a colourless Boo) | `color_indicator: [R]` | `cards/parser/effects_ext.py::_CREATE_NAMED_TOKEN`, `_token_printing` | fixed (8ea9fcca), supported | confirmed |
| 13 | Ajani, Nacatl Pariah // Ajani, Nacatl Avenger | reflexive trigger flattened | the 0: `Sequence [CreateToken, IfCondition(ControlOtherPermanentOfColor, DealDamageFromEvent → any target)]` | a reflexive triggered ability (CR 603.12) after the token, its target chosen when it triggers | `cards/parser/layout_face_grammar.py` (`_WHEN_YOU_DO`) | fixed (8ea9fcca), supported | confirmed |
| 14 | The Sensational She-Hulk; Enduring Innocence | one flag, two rules | `TriggeredAbility.once_per_turn: true` for both | "do this only once each turn" (603.2h) apart from "this ability triggers only once each turn" | `TriggeredAbility.once_per_turn` (`turn.py::should_fire_trigger`) | told apart by the effect's shape | suspected |
| 15 | Chromatic Star | event widened | `event: leaves_battlefield` for "When this artifact is put into a graveyard from the battlefield" | `event: dies` (the IR's "put into a graveyard from the battlefield", as Titania's) | `cards/parser/triggers.py` (the "this artifact is put into a graveyard from the battlefield" entry, ~line 744) | fixed (8ea9fcca), supported | confirmed |
| 16 | Forth Eorlingas! | "one or more" as once per turn | the this-turn trigger "whenever one or more creatures you control deal combat damage to one or more players" with `once_per_turn: true` | a trigger per combat damage step ("one or more": `OneOrMoreBatch`), no once-per-turn limit | `cards/parser/triggers.py` (the "one or more creatures you control deal combat damage to one or more players" entry, ~line 1050) | fixed (8ea9fcca), supported | confirmed |
| 17 | Green Sun's Zenith | instruction as a replacement | "Shuffle Green Sun's Zenith into its owner's library" as a self static `ReplacementEffect(would_go_to_graveyard, Replace(library))` (from anywhere) | the last instruction of the spell's effect (only as it resolves) | `cards/parser/spell_builder.py` (`_SELF_SHUFFLE_GSZ_RE`, `_detect_shuffle_self_statics`, ~line 430) | fixed (8ea9fcca), supported | confirmed |
| 18 | Wizard's Rockets | event widened | `event: leaves_battlefield` for "When this artifact is put into a graveyard from the battlefield" | `event: dies` (as entry 15) | `cards/parser/triggers.py` (the same phrase-table entry as entry 15) | fixed (8ea9fcca), supported | confirmed |
| 19 | Gut, True Soul Zealot | qualifier widened | `SacrificeAPermanent { another: true, permanent_filter: ObjectOr(IsCreature, IsArtifact) }` for "sacrifice another creature or an artifact" | "another" on the creature only (an artifact may be Gut itself) | the "sacrifice another ..." phrase parser (`IfYouDo` / `MayDo`) | worked around (the label read) | confirmed |
| 20 | Adagia, Windswept Bastion; Evendo, Waking Haven; Kavaron, Memorial World; Susur Secundi, Void Altar; Uthros, Titanic Godcore; The Eternity Elevator (Edge of Eternities, ENGINE.md 99) | ability dropped | the planets: a land with `enters_tapped` and `produces` only; The Eternity Elevator: no `station_threshold` / `stationed_*` | Station (CR 702.184) and each threshold ability ("12+ ..."), as the set's Spacecraft carry them | `cards/parser/land_builder.py` (planets) and the Spacecraft builder (`cards/parser/spacecraft_builder.py`) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 60 | All-Fates Stalker (Edge of Eternities, ENGINE.md 99.4) | target count dropped | `ExileTargetCreatureUntilSourceLeaves { excluded_subtypes: [Assassin] }`, a class with one required target (Hardlight Containment's too) | "up to one target" (a target count, minimum 0) | `effects/generic/exile.py::ExileTargetCreatureUntilSourceLeaves` (`requires_target`) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 61 | Endless Sands (Edge of Eternities, ENGINE.md 99.4) | effect misread, ability dropped | `{2},{T}: ExileTargetCreatureUntilSourceLeaves { yours_only }`; `{4},{T}, sac: Sequence { effects: [] }` | a plain linked exile (CR 607.2a), and "return each creature card exiled with this land to the battlefield under its owner's control" | the land-ability parser (`cards/parser/land_builder.py`) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 62 | Gravkill; Archenemy's Charm (Edge of Eternities, ENGINE.md 99.4) | target types dropped | `ExileTargetCreature { targets: [IsCreature] }` for "exile target creature or Spacecraft" / "... or planeswalker"; the Charm's second mode `ReturnUpToNCreaturesFromYourGraveyardToHand { creature }` for "one or two target creature and/or planeswalker cards" | `ObjectOr(IsCreature, HasSubtype(Spacecraft))` / `ObjectOr(IsCreature, IsPlaneswalker)`; the planeswalker cards and the count | the "exile target X" phrase parser | blocked (`lower::IR_DEFECTS`) | confirmed |
| 63 | Elegy Acolyte (Edge of Eternities, ENGINE.md 99.4) | "one or more" as once per turn | "whenever one or more creatures you control deal combat damage to a player" with `once_per_turn: true` | one trigger per combat damage step (`OneOrMoreBatch`), no once-each-turn limit (as entry 16) | `cards/parser/triggers.py` (the "one or more creatures you control deal combat damage to a player" entry) | blocked (`lower::IR_DEFECTS`) | confirmed |

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
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: the tap-and-sacrifice builder folds "{T}: Add {C}" into `produces: [C]` with `mana_only_from_produces` in every branch (Dust Bowl, Petrified Field, High Market, Scavenger Grounds outside the cube too). Strip Mine is supported (`tests/rules_lands.rs::strip_mine_taps_for_colorless_and_destroys_any_land`).

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
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: `_try_build_manland` passes `set_colors` (the two colour words) and `granted_subtype: "Elemental"` (every manland of that shape, the Stellar Sights ones too). Both cards are supported (`tests/rules_layers.rs`).

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
* Engine: the lowering re-derives each target from the class name and the Oracle text
  (`engine/src/lower/effect_ext.rs::lower_effect_ext`, `effect_tail3.rs`): Mishra's Bauble,
  Urza's Bauble, Grindstone, Displacer Kitten, and since M7c tail part 3 Thieving Skydiver
  ("target artifact with mana value X or less", X its kicker's: `Filter::MvAtMostSourceX`) and Wan
  Shi Tong ("target nonland permanent").

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
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: `duration: "while_exiled"`. The lowering now reads it (a `this_turn` duration with `requires_attacked_with_subtype` is a blocker, `tests/support.rs`); the lowered card is unchanged.

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
* Engine (M7c tail part 3): worked around. A permanent's kicker is read from `kicker_cost_str`
  when it agrees with the structured cost but for the {C} pips counted as generic there
  (`engine/src/lower/printing.rs::kicker_mana`, rule 4a of ENGINE.md 3), so Sowing Mycospawn's
  kicker is {1}{C} and needs colorless mana
  (`tests/cards_m7c_tail3.rs::sowing_mycospawn_kicker_needs_colorless_mana_cr107_4c`); any other
  difference is a blocker.

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
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: the token phrases capture the colour (`color_indicator: [R]` on both of Boo's printings; Dark Depths' Marit Lage outside the cube is black too). Minsc & Boo is supported; its tests run on the cube card (`tests/cards_m7b_walkers2.rs`). The red Boo is a new token definition for the network's token registry.

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
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: the 0 is `DoThenReflexive { first: CreateToken, then: IfCondition(...) }` (a new class, the non-optional twin of `MayDoThenReflexive`); the lowering makes `then` a reflexive triggered ability with the `IfCondition` as its intervening "if" (ENGINE.md 32). Ajani is supported (`tests/cards_m7b_faces4.rs`, the real card).

### G. A trigger event widened (M7c tail part 1)

**15. Chromatic Star.** Oracle: "{1}, {T}, Sacrifice this artifact: Add one mana of any color.
When this artifact is put into a graveyard from the battlefield, draw a card."

* IR: the triggered ability's `event` is `leaves_battlefield` (with `IsSelf`): it would also draw
  when the Star is exiled, bounced or put into a library.
* Should be: `event: dies`, which the IR uses for "put into a graveyard from the battlefield" of
  any permanent (Titania, Protector of Argoth's lands; the engine's `EventKind::Dies`).
* Source: `cards/parser/triggers.py`, the phrase table entry `"this artifact is put into a
  graveyard from the battlefield": (Event.LEAVES_BATTLEFIELD, IsSelf())` (~line 744); its comment
  says it mirrors a static-registry simplification ("slightly looser - also fires if exiled",
  Nutrient Block) for per-card parity.
* Fix: map the phrase to `Event.DIES` with `IsSelf()` (and fix the registry entry it mirrors).
* Engine: blocked, `IR:TriggeredAbility.event(leaves_battlefield for "put into a graveyard")`
  (`lower::IR_DEFECTS`); `tests/cards_m7c_tail.rs::chromatic_star_is_blocked_by_its_ir_event`.
  The other 29 cards of the batch: their IR matches their Oracle text.
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: `event: dies`. Chromatic Star is supported (`tests/cards_m7c_tail.rs`).

### H. "One or more" and a resolution instruction rewritten (M7c tail part 2)

**16. Forth Eorlingas!** Oracle: "Create X 2/2 red Human Knight creature tokens with trample and
haste. Whenever one or more creatures you control deal combat damage to one or more players this
turn, you become the monarch."

* IR: the this-turn trigger (`CreateThisTurnTriggeredAbility`) has `event: deals_combat_damage`,
  a per-source predicate, and `once_per_turn: true`.
* Should be: "one or more" (`OneOrMoreBatch`): one trigger per combat damage step in which
  creatures of yours deal combat damage to players, with no once-per-turn limit. With a
  first-strike damage step, an additional combat (Fear of Missing Out) or the monarchy taken away
  between them, the Oracle text makes the player the monarch again; the IR doesn't.
* Source: `cards/parser/triggers.py`, the phrase table entry `"one or more creatures you control
  deal combat damage to one or more players"` (~line 1050) threads `once_per_turn=True`; its
  comment says "with one combat per turn, once per turn is once per combat" (the Python engine's
  single-combat model). Locker Room's entry above it has the same shape.
* Fix: emit `OneOrMoreBatch` in the predicate and no `once_per_turn` (or a separate
  "once per combat damage step" flag if the Python dispatcher needs one).
* Engine: blocked, `IR:TriggeredAbility.once_per_turn("one or more" written as once each turn)`
  (`lower::IR_DEFECTS`): the engine now reads `once_per_turn` on a non-"you may" effect as "this
  ability triggers only once each turn" (entry 14), which this is not;
  `tests/cards_m7c_tail2.rs::forth_eorlingas_is_blocked_by_its_ir_once_per_turn`. Its two effect
  classes (`CreateTokensScaledByX`, `CreateThisTurnTriggeredAbility`) are not lowered yet either.
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: `OneOrMoreBatch` in the predicate and no `once_per_turn`. Forth Eorlingas! is supported (`CreateThisTurnTriggeredAbility` lowered, ENGINE.md 32; `tests/cards_m7c_tail2.rs`).

**17. Green Sun's Zenith.** Oracle: "Search your library for a green creature card with mana value
X or less, put it onto the battlefield, then shuffle. Shuffle Green Sun's Zenith into its owner's
library."

* IR: `spell_effect: SearchLibraryForGreenCreatureWithMVAtMostX`, and the last sentence as a
  self-scoped static `ReplacementEffect { event_pattern: would_go_to_graveyard, transform:
  Replace { new_destination: library } }`, with no zone: "if this card would be put into a
  graveyard from anywhere, put it into its library instead" (Nexus of Fate's wording, which the
  same parser path also produces).
* Should be: an instruction of the effect, done as the spell resolves (CR 608.2c). Countered, it
  goes to its owner's graveyard; discarded or milled, likewise. The IR would put it into the
  library in all three cases.
* Source: `cards/parser/spell_builder.py`: `_SELF_SHUFFLE_GSZ_RE` and `_SELF_SHUFFLE_NEXUS_RE`
  (~line 459) drop the sentence from the effect stream, and `_detect_shuffle_self_statics` wires
  both forms as the same static replacement.
* Fix: emit the GSZ form as an effect step (a `ShuffleSourceIntoOwnersLibrary` at the end of the
  `Sequence`, as Worldspine Wurm's IR already has the class), and keep the replacement for the
  Nexus form only.
* Engine: blocked, `IR:StaticAbility.replacement_effect("shuffle it into its library" as a
  replacement from anywhere)` (`lower::IR_DEFECTS`); also `ReplacementEffect(WouldGoToGraveyard,
  Replace)` (not built). Its search is built (`Filter::MvAtMostX`) and tested on a custom card
  without the last sentence (`Test Green Sun`, `tests/cards_m7c_tail2.rs`).
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: `spell_effect: Sequence [SearchLibraryForGreenCreatureWithMVAtMostX, ShuffleThisSpellIntoOwnersLibrary]` (a new class) and no static; Nexus of Fate's form keeps the replacement. Green Sun's Zenith is supported (`tests/cards_m7c_tail2.rs`; the custom `Test Green Sun` is gone).

### I. The same widened event (M7c tail part 3)

**18. Wizard's Rockets.** Oracle: "This artifact enters tapped. {X}, {T}, Sacrifice this artifact:
Add X mana in any combination of colors. When this artifact is put into a graveyard from the
battlefield, draw a card."

* IR: the triggered ability's `event` is `leaves_battlefield` with `IsSelf`, as Chromatic Star's
  (entry 15): it would also draw when the Rockets are exiled, bounced or put into a library.
* Should be: `event: dies`.
* Source: the phrase table entry of entry 15 (`cards/parser/triggers.py`, "this artifact is put
  into a graveyard from the battlefield").
* Fix: entry 15's.
* Engine: blocked, `IR:TriggeredAbility.event(leaves_battlefield for "put into a graveyard")`
  (`lower::IR_DEFECTS`); `tests/cards_m7c_tail3.rs::wizards_rockets_is_blocked_by_its_ir_event`.
  Its mana ability's `amount: 0` with `amount_is_x` (entry 10) and the {X} in a mana ability's
  cost are not lowered either.
* Fixed in mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca: `event: dies` (entry 15's fix). Wizard's Rockets is supported: its {X} mana ability is lowered too, reading entry 10's `amount: 0` beside `amount_is_x` (ENGINE.md 32; `tests/cards_m7c_tail3.rs`).

### J. "Another" spread over an "or" (M8d part 4)

**19. Gut, True Soul Zealot.** Oracle: "Whenever you attack, you may sacrifice another creature or
an artifact. If you do, create a 4/1 black Skeleton creature token with menace that's tapped and
attacking."

* IR: `SacrificeAPermanent { another: true, label: "another creature or an artifact",
  permanent_filter: ObjectOr(IsCreature, IsArtifact) }`: read as written, `another` excludes Gut
  from both kinds.
* Should be: "another" qualifies "creature" only; "an artifact" may be Gut itself when it is one
  (ruling 06cb2c7b: "If Gut somehow becomes an artifact, you may sacrifice it to its own
  ability"). E.g. `permanent_filter: ObjectOr(ObjectAnd(IsCreature, Another), IsArtifact)` and
  `another: false`.
* Source: the parser of "sacrifice another <phrase>" (the `another` flag set for the whole
  phrase).
* Fix: scope `another` to the noun phrase it precedes when the phrase is "another X or a/an Y".
* Engine: worked around in the lowering (`lower/effect_ext.rs`, `SacrificeAPermanent`): with
  `another` and a label "... or a ..." / "... or an ...", only the first part of the `Or` excludes
  the source (M8d part 4; `tests/rulings_other.rs::gut_made_an_artifact_may_sacrifice_itself_to_its_own_ability_cr_701_21a`,
  a Phyrexian Metamorph copying Gut). Gut is the only cube card with this shape.


### K. Station dropped (Edge of Eternities, ENGINE.md 99)

**20. The five planets and The Eternity Elevator.** Oracle (Evendo, Waking Haven): "This land
enters tapped. / {T}: Add {G}. / Station (Tap another creature you control: Put charge counters
equal to its power on this Planet. Station only as a sorcery.) / 12+ | {G}, {T}: Add {G} for each
creature you control." Adagia, Kavaron, Susur Secundi and Uthros have the same shape; The Eternity
Elevator is a Spacecraft with Station.

* IR (mtg-bot main 2cb79f77, `cards export-ir --pool cube legacy eoe`): each planet is
  `CardPrinting { card_types: [Land], enters_tapped: true, produces: [<colour>] }`, nothing else;
  The Eternity Elevator has no `station_threshold` or `stationed_*` field. The set's other 21
  Spacecraft carry `station_threshold` and `stationed_keywords`.
* Should be: the Station keyword ability and each "N+ | ..." ability gated on the charge
  counters, as the Spacecraft builder writes them.
* Source: the planets go through the land builder, which keeps only the enters-tapped and mana
  lines; The Eternity Elevator's builder misses its station lines.
* Fix: build the planets' and The Eternity Elevator's Station and threshold abilities like the
  Spacecraft's.
* Engine: blocked, `ir_defect(station dropped, docs/ir_defects.md 20)` (`lower::IR_DEFECTS`).
  Without the entry the five planets lowered as plain taplands and the support report counted them
  supported (98 cards instead of 93): a silent drop the report could not see. Found by checking
  every EoE card whose Oracle text has Station for a `station_threshold` in its IR.

### L. Edge of Eternities lane C (ENGINE.md 99.4)

**60. All-Fates Stalker.** Oracle: "When this creature enters, exile up to one target non-Assassin
creature until this creature leaves the battlefield. / Warp {1}{W}"

* IR: `TriggeredAbility { event: enters, effect: ExileTargetCreatureUntilSourceLeaves {
  excluded_subtypes: [Assassin] } }`. The class has no target spec or count; its Python
  `requires_target` is true and it is also Hardlight Containment's "exile target creature an
  opponent controls until this Aura leaves the battlefield" (one required target).
* Should be: up to one target (a `TargetSpec` with `count_min: 0`): the ability can do nothing,
  and with no other creature on the battlefield it must not target the Stalker itself.
* Source: `effects/generic/exile.py::ExileTargetCreatureUntilSourceLeaves` (no count field).
* Fix: export the target spec (`targets: [TargetSpec { filter, count_min: 0 }]`), or a
  `count_min` field on the class.
* Engine: the class lowers (one target, `Ext::ExileLinked { until: true }`); the card is
  blocked, `ir_defect(up to one target dropped, docs/ir_defects.md 60)`.

**61. Endless Sands.** Oracle: "{T}: Add {C}. / {2}, {T}: Exile target creature you control. /
{4}, {T}, Sacrifice this land: Return each creature card exiled with this land to the battlefield
under its owner's control."

* IR: the second ability is `ExileTargetCreatureUntilSourceLeaves { yours_only: true }` (the
  creature would return when the land leaves, so the third ability's sacrifice would return it
  anyway); the third ability's effect is `Sequence { effects: [] }`, nothing.
* Should be: a linked exile without a duration (CR 607.2a, "exiled with this land"), and the
  third ability's "return each creature card exiled with this land to the battlefield under its
  owner's control" (as the cube's linked-exile returns).
* Source: the land builder (`cards/parser/land_builder.py`), which matched the exile line to the
  until-it-leaves class and dropped the return.
* Fix: a linked exile class for the second ability and a "return each card exiled with this"
  effect for the third.
* Engine: blocked, `ir_defect(linked exile and its return dropped, docs/ir_defects.md 61)`
  (the card is also lane B's: `mana_only_from_produces`).

**62. Gravkill; Archenemy's Charm.** Oracle (Gravkill): "Exile target creature or Spacecraft."
(Archenemy's Charm): "Choose one - / * Exile target creature or planeswalker. / * Return one or
two target creature and/or planeswalker cards from your graveyard to your hand. / * Put two
+1/+1 counters on target creature you control. It gains lifelink until end of turn."

* IR: `ExileTargetCreature { targets: [TargetSpec { filter: IsCreature }] }` for both exile
  modes; the Charm's second mode is `ReturnUpToNCreaturesFromYourGraveyardToHand { creature:
  true }`.
* Should be: `ObjectOr(IsCreature, HasSubtype(Spacecraft))` (a Spacecraft that isn't a
  creature, below its station threshold, is a legal target) and `ObjectOr(IsCreature,
  IsPlaneswalker)`; the second mode's planeswalker cards and its "one or two" count.
* Source: the "exile target creature ..." phrase parser keeps the first noun only.
* Fix: parse "creature or <type>" into an `ObjectOr` target filter.
* Engine: `ExileTargetCreature` lowers its target spec as written; both cards are blocked,
  `ir_defect(target types dropped, docs/ir_defects.md 62)`.

**63. Elegy Acolyte.** Oracle: "Lifelink / Whenever one or more creatures you control deal combat
damage to a player, you draw a card and lose 1 life. / Void - At the beginning of your end step,
if a nonland permanent left the battlefield this turn or a spell was warped this turn, create a
2/2 colorless Robot artifact creature token."

* IR: the combat damage trigger is `TriggeredAbility { event: deals_combat_damage, predicate:
  creature you control -> player, once_per_turn: true }`.
* Should be: one trigger per combat damage step in which one or more of them deal combat damage
  to a player (`OneOrMoreBatch`), with no once-each-turn limit: a first-strike step and a regular
  step, or an extra combat, trigger it again (the shape of entry 16, Forth Eorlingas!).
* Source: `cards/parser/triggers.py`, the "one or more creatures you control deal combat damage
  to a player" phrase entry (entry 16's fix covered another phrase).
* Fix: `OneOrMoreBatch` in the predicate, `once_per_turn: false`.
* Engine: blocked, `ir_defect(one or more as once per turn, docs/ir_defects.md 63)`; its void
  trigger lowers (ENGINE.md 99.4).


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
* Engine (M7c tail part 3): read as the Aura's bonus to what it enchants (the same `attached`)
  on an Aura with "enchant creature card in a graveyard", the only Aura shape lowered; Animate
  Dead is supported (`tests/cards_m7c_tail3.rs::animate_dead_reanimates_and_its_leaving_
  sacrifices_the_creature_cr303_4a`). On any other card that isn't an Equipment it stays a
  blocker.

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
* Engine (M7c tail part 3): lowered as is (the flash keyword), Necromancy supported: no card in the
  cube asks whether a card has flash, so the keyword and the permission behave alike here; the
  difference is noted (ENGINE.md 30.7).

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
  `RemoveTargetPermanents.count_from_x`, `Effect:CreateTokensScaledByX`. Since M7c tail part 3:
  read in exactly that shape (`count_from_x` with the four zero counts, on a spell with X) as "up
  to X targets"; Pest Infestation supported. Fastbond's 1,000,000 is read as any number of land
  plays (that exact value); Fastbond supported.
* Wizard's Rockets ("{X}, {T}, Sacrifice this artifact: Add X mana in any combination of
  colors."): `AddManaInAnyCombination { amount_is_x: true }` with the default `amount: 0`
  (`effects/generic/artifact_mechanics.py:109-111`; built at `cards/parser/artifact_effects.py:137`).
  Fix: omit `amount` (or export it as null) when `amount_is_x`. Engine: blocked,
  `AddManaInAnyCombination.amount(out of range 0)`, `AddManaInAnyCombination.amount_is_x`,
  `ActivationCost.mana.x_pips`. Since the IR fixes of M7d (ENGINE.md 32): read in exactly that
  shape (`amount: 0` beside `amount_is_x`, with an {X} in the mana ability's cost) as X mana;
  Wizard's Rockets supported (entry 18). The sentinel is still in the IR.

**11. Target requirements that live only in code (systemic).** Many effect classes carry no
`targets` in the IR because Python computes them in methods (`requires_target`,
`target_count_range`, `legal_targets`), e.g. `PutPlusOneCounterOnEachOfUpToNTargetCreatures`
(Agent Bishop: "each of up to two target creatures", `effects/generic/counters.py:181-205`),
`PutStunCounterOnTappedCreature` (Cryogen Relic: "up to one target tapped creature",
`effects/generic/counters.py:596`), `ExileTargetCardFromGraveyard` (Soul-Guide Lantern),
`ReturnCreatureFromYourGraveyardToHand` and `TargetPlayerDiscards` (Kolaghan's Command),
`GrantFlashbackToTargetCardUEOT` (Flashback), `ReturnTargetSpellToOwnersHand` (Reprieve),
`CounterSpell` (Mana Tithe); M7b part 3: `SourceBecomesCopyOfTargetGraveyardCardUEOT` (Shifting
Woodland: "target permanent card in your graveyard") and `CopyTargetInstantOrSorcerySpellYouControl`
(Lutri: "target instant or sorcery spell you control"). The IR is then not self-describing: a
consumer has to know each class's targets. The engine re-derives them per class in `engine/src/lower/effect.rs` and
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
  603.2h; She-Hulk is supported); any other, since M7c tail part 2, as "this ability triggers
  only once each turn" (`TrigPred::Once`; Enduring Innocence is supported). Forth Eorlingas!'s
  flag means neither (entry 16) and is blocked by name.

## Checked and not defects

* M8d part 1, the official rulings (ENGINE.md 37): no ruling tested showed a wrong IR. Kaito,
  Bane of Nightmares' `creature_during_your_turn` (p, t, subtypes, keywords) carries no "in
  addition to its other types", which is right: "he's a 3/4 Ninja creature" replaces his card
  types (CR 205.1a, the card's ruling f0224bc7). The engine's lowering had read it as "in addition
  to"; fixed there, not in the IR.

* M7d, the 13 cards (ENGINE.md 31): the IR matches the Oracle text. Representational notes, each
  read together with what it summarises and checked to agree: Gemstone Caverns'
  `produces_with_counter` (its mana ability's `IfConditionElse` on `SourceHasCounter`); Lion
  Sash's `reconfigure_cost` (its two abilities, `ReconfigureAttach` and `ReconfigureUnattach`, both
  sorcery-speed) and `equipment_bonus_per_own_counter` as `[counter, p, t]`; Does Machines'
  `class_levels` (each level's whole printing, with the card's name, cost and types, its
  `GainClassLevel` default level 2); Abhorrent Oculus's `ManifestDread.count` default 1; Urza's
  "tap an untapped artifact you control" as `tap_permanent_you_control` with
  `tap_permanent_type: "Artifact"`; Nadu's grant as `granted_becomes_target_effect` with
  `granted_trigger_limit_per_turn: 2` on a "creatures you control" scope; Pillage the Bog's
  `Scaled { factor: 2 }` count; Arena of Glory's `exert_self` and
  `grants_haste_to_creature_spell`; Crabomination's `emerge_sacrifice_type: "Artifact"` and
  `TargetOpponentExilesThreeMayCastOneFree` (the three exiles and the free cast are the class's).

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
  Oracle text; the range was the engine's (10 since M7d).
* Planeswalker `ActivationCost.loyalty` is the signed loyalty *cost* (a "+1" ability is
  `loyalty: -1`); consistent over all 23 planeswalker faces and documented in
  `effects/base.py:~540-560`.
* Inti's `MayDoThenReflexive { first: DiscardSelf }`: `DiscardSelf` means "you discard a card"
  (`effects/generic/discard.py:30`), not "discard this card".
* Fastbond's `LandsPlayedThisTurnAtLeast` without `n`: the schema default is 2 ("if it wasn't the
  first land you played this turn").
* `TriggerPredicate.subject(Land / Defender)` (City of Traitors, Coveted Jewel; Surrak's
  `TargetSpell` is built since M7b part 3b), `ExileTopCardsWithPlayPermission.duration(UntilYourNextEndStep)` (Inti),
  `LinkExiledCardsToSource.inner(...)` (Keen-Eyed Curator), `RemoveAllMatching.action(Exile)`
  (Winds of Abandon), `Keyword.quality(SpellsThatAreOneOrMoreColors)` (Emrakul): the IR matches
  the Oracle text; these are engine features not built yet (Inti's, Winds of Abandon's and
  Emrakul's are built since M7c tail part 3).
* M7c tail part 3, the 33 cards (ENGINE.md 30): the IR matches the Oracle text, apart from entries
  5-8, 10 and 18. Representational notes: `ConditionalIfCastFromHand` (The One Ring) is "if you
  cast it" (its docstring; the class name says "from hand"); "put into a graveyard from anywhere"
  is the `dies` event with a `PutIntoGraveyardFromAnywhere` marker predicate plus
  `watched_in_graveyard` (Worldspine Wurm, Emrakul; Blightsteel's replacement), read together and
  checked to agree; Emrakul's annihilator is stated twice (keyword and trigger), they agree;
  Batroc's "up to X targets" is the class's, its `TargetSpec` giving the filter with the default
  counts; `discards_card` (Inti) and `card_discarded` (Currency Converter) are the same event;
  `HideawayLookExileOneFaceDown`, `PlayExiledCardFreeIfAnyLibraryAtMost` and
  `EachPlayerExilesHandDrawsNReturnsLater` carry their numbers as schema defaults (4, 20, 7), the
  Oracle text's. Abhorrent Oculus's `ManifestDread` and the other cards still blocked (ENGINE.md
  30.8) are engine features.
* Fast lands' `enters_tapped_unless: ControlsAtMost(IsLand, 2)` says "lands", not "other lands";
  it is evaluated before the land enters, so the two agree (ENGINE.md 19).
* Hybrid pips are stored in alphabetical order (`{W/B}` as `["B", "W"]`); same meaning.
* M7b part 3, the copy cards: `TargetPlayerMayCopyThisSpell.needs_target_with_cards_in_hand`
  (Chain of Smog) is a Python engine switch (offer the copy only if a player it could target has
  cards in hand, a guard against endless copying), not Oracle text; the engine ignores it
  (ENGINE.md 14, 26.11). Lutri's companion condition isn't in the IR; it works outside the game
  (no sideboards here). `CardPrinting.replicate_cost` is a string with no structured twin (the
  other new costs, squad, offspring, bestow and Springheart Nantuko's payment, agree with their
  `*_str` twins). Chain of Smog's "that player" is the spell's target player, found among its
  targets. Apart from these (and entry 11), the 16 cards' IR matches their Oracle text and
  rulings.
* M7b part 3b, the 28 cards of the stack, ward, the initiative and graveyard abilities (Tishana's
  Tidebinder, Ertai Resurrected, Invert Polarity, Leovold, Surrak, Sedgemoor Witch, Kappa
  Cannoneer, Raffine, Hexing Squelcher, Sunset Saboteur, Feywild Caretaker, Passageway Seer, White
  Plume Adventurer, Caves of Chaos Adventurer, Seasoned Dungeoneer, Undermountain Adventurer, From
  the Catacombs, Fanatic of Rhonas, Otharri, Seasoned Pyromancer; The Wondrous Wasp, Voice of
  Victory, Brain Freeze, Occult Epiphany, Fallen Shinobi, Otawara, Boseiju, Forensic Gadgeteer):
  the IR matches the Oracle text. Rules the IR leaves to the consumer, as the CR defines them:
  `TakeInitiative` alone (the initiative's three abilities, CR 725.2, and Undercity's rooms, 309,
  are built by the engine from the dungeon card; Python's `designations.py` does the same),
  `Mobilize { n }` (the Warrior token, 702.181a) and `has_storm` (702.40a).
  `TargetPlayerExilesTopNYouMayPlayThem` (Fallen Shinobi) has no target specification: its "that
  player" is the player the combat-damage trigger names (Python passes it as the trigger's fixed
  target), not a chosen target; the name misleads, the data is right. `TargetConnives` states
  "attacking" twice (`attacking_only` and its target filter); they agree. `ward_discard_random`
  (Alpharael, not in the cube) is unused here and a blocker.
* M7c tail part 2, the 24 cards (ENGINE.md 29): the IR matches the Oracle text. Representational
  notes: Laelia's "whenever one or more cards are put into exile from your library and/or your
  graveyard" is a marker predicate (`CardsExiledFromLibraryOrGraveyard`) on a placeholder
  `event: dies` plus the printing flag `watches_library_graveyard_exile` (the Python engine sweeps
  for it, `effects/generic/cube_creatures_rmc.py::sweep_library_graveyard_exile_watch`); the
  engine reads the three together and checks they agree. Bloodbraid Challenger states cascade
  twice (the keyword and its "when you cast this spell" trigger with `Cascade`); they agree.
  Enduring Innocence's `once_per_turn` is "this ability triggers only once each turn" (entry 14's
  second reading, now lowered). Multiversal Passage's `shock_land_life_cost` is its "then you may
  pay 2 life"; We Say Thee Nay!'s `teamwork_power` and `OptionalAdditionalCostPaid` describe
  teamwork as kicker-like modes; Fire Covenant's X is defined by `additional_cost_pay_x_life`
  (`DealDamageDividedAmongTargets.amount: ResolvingX`); Candelabra of Tawnos's
  `UntapXTargets.target_count_is_x` with `count_max: inf`. Still blocked by engine features, not
  the IR: Abhorrent Oculus (manifest dread, face-down permanents), Shelldock Isle, Memory Jar and
  Headliner Scarlett (face-down exile).
