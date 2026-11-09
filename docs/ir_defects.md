# Card-IR defects for mtg-bot's parser (M7b)

Status: 2026-10-22, engine at main 7c014ca + the M7b card tests (branch `m7b-tests`); IR
`data/cards/powerlsv.ir.json` (546 cube cards, 360 supported). Updated 2026-10-23 for M7b part 2
(cards with more than one face, branch `m7b-faces`, 394 supported): entries 9, 13 and 14; mtg-bot read at commit
`3073a633` (branch main). Updated 2026-10-23 for M7b part 3 (copies, branch `m7b-copies`, 410 supported): no new
defect; entry 11 gains two classes, and the copy cards' Python-only switches are listed under "Checked and not
defects". Updated 2026-10-23 for M7b part 3b (the stack, ward, the initiative, graveyard abilities, branch
`m7b-stack`, 438 supported): no new defect; the 28 cards' notes are under "Checked and not defects". Updated 2026-10-23 for M7c tail part 1 (branch `m7c-tail1`, 467 supported): entry 15 (Chromatic Star). Updated 2026-10-24 for M7c tail part 2 (branch `m7c-tail2`, 491 supported): entries 16 (Forth Eorlingas!) and 17 (Green Sun's Zenith); entry 14's second card is now supported; Laelia's placeholder event under "Checked and not defects". Updated 2026-10-24 for M7c tail part 3 (branch `m7c-tail3`, 524 supported): entry 18 (Wizard's Rockets); entries 5, 6, 7, 8 and 10's cards (Sowing Mycospawn, Thieving Skydiver, Wan Shi Tong, Animate Dead, Necromancy, Fastbond, Pest Infestation) supported, the IR read faithfully with consistency checks; the tail's notes under "Checked and not defects". Updated 2026-10-25 (branch `m7d-ir`, 546 supported): the IR now in use is mtg-bot `feature/ir-fixes-mtgzero` 8ea9fcca (schema `66ba3e33fe5b`), which fixes entries 1, 2, 4, 12, 13, 15, 16, 17 and 18 (each entry's last bullet); the lowering reads the fixed forms and `lower::IR_DEFECTS` is empty: every cube card is supported. Updated 2026-11-08 (branch `eoe-irfix`, ENGINE.md 99.9): the IR now in use is mtg-bot `feature/eoe-ir-fixes` 5362f5ed (schema `675cfc50fc4e`), which fixes entries 20-23, 26, 40-42, 46, 48, 51, 60, 62-64, 81 and 82 (for 21, 41 and 62 only some of their cards; each entry's last bullet); the other Edge of Eternities entries stand. Updated 2026-11-08 (branch `eoe-e-loose`, ENGINE.md 100.6, the faithfulness audit's follow-ups): entries 190 and 191 (Python engine switches the IR carries, section R) and a note on entry 2 (the audit's N16). The IR is exported by mtg-bot (`mtg_bot.cards.ir`, `python -m mtg_bot
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
| 20 | Adagia, Windswept Bastion; Evendo, Waking Haven; Kavaron, Memorial World; Susur Secundi, Void Altar; Uthros, Titanic Godcore; The Eternity Elevator (Edge of Eternities, ENGINE.md 99) | ability dropped | the planets: a land with `enters_tapped` and `produces` only; The Eternity Elevator: no `station_threshold` / `stationed_*` | Station (CR 702.184) and each threshold ability ("12+ ..."), as the set's Spacecraft carry them | `cards/parser/land_builder.py` (planets) and the Spacecraft builder (`cards/parser/spacecraft_builder.py`) | fixed (5362f5ed); Evendo, Kavaron, Uthros supported, the others blocked by engine features | confirmed |
| 21 | Dockworker Drone, Luxknight Breacher (and Blast Zone) (Edge of Eternities, ENGINE.md 99.2) | replacement as a trigger | "this creature enters with a +1/+1 counter on it [for each ...]" / "this land enters with a charge counter on it" as an `enters` triggered ability (`PutPlusOneCounterOnSelf`, `PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact`, `PutChargeCountersOnSelf`) | it enters with the counters (CR 614.1c, 122.6): `enters_with_named_counters`, and a per-permanent count the schema can't say yet | the creature parser's "enters with" lines (`cards/parser/creature_builder.py`, `cards/parser/effects.py`) | fixed for Dockworker Drone (supported) and Blast Zone (5362f5ed); Luxknight Breacher fixed (20bea9f4), supported | confirmed |
| 22 | Haliya, Ascendant Cadet (ENGINE.md 99.2) | "one or more" as once per turn | "whenever one or more creatures you control with +1/+1 counters on them deal combat damage to a player" with `once_per_turn: true` | a trigger per combat damage event ("one or more": `OneOrMoreBatch`), no once-per-turn limit (as entry 16) | `cards/parser/triggers.py` (the "one or more creatures you control with +1/+1 counters on them ..." entry, ~line 1015) | fixed (5362f5ed), supported | confirmed |
| 23 | (same defect as entry 51, found independently by lane A) Honored Knight-Captain, Rayblade Trooper; Cosmogrand Zenith, Knight Luminary (base list, lane B) (ENGINE.md 99.2) | colour dropped | the "1/1 white Human Soldier creature token" printing with no `color_indicator` (a colourless token) | `color_indicator: [W]` (as entry 12) | `cards/tokens.py` (`TOKEN_PRINTINGS["Human Soldier"]`, ~line 141) | fixed (5362f5ed), supported | confirmed |
| 24 | Mechan Shieldmate (ENGINE.md 99.2) | permission as a characteristic | "this creature can attack as though it didn't have defender" as a static `RemoveKeyword(defender)` | a rule that lets it attack while it keeps defender (CR 702.3b) | `cards/parser/creature_builder.py` (~line 672), `cards/parser/printing_keywords.py` (~line 473) | fixed (20bea9f4), supported | confirmed |
| 25 | Loading Zone (ENGINE.md 99.2) | scope dropped | "if one or more counters would be put on a creature, Spacecraft, or Planet you control, twice that many ... instead" as a `scope: self` `ReplacementEffect(would_place_counter, MultiplyCount(2))` with no recipient filter | the recipients: creatures, Spacecraft and Planets you control, every kind of counter | `cards/parser/enchantment_builder.py` (`_COUNTER_DOUBLER_LINE`, ~line 651; the Python engine hard-codes creatures and Spacecraft, +1/+1 counters only) | fixed (20bea9f4); blocked by an engine feature | confirmed |
| 26 | The Eternity Elevator (ENGINE.md 99.2) | value made up | `power: 0, toughness: 0` for a card with no power/toughness box | no power or toughness (CR 722.1, 722.2c) | `cards/parser/spacecraft_builder.py` (the missing-P/T fallback, ~line 334) | fixed (5362f5ed); blocked by an engine feature (entry 20) | confirmed |
| 27 | Nesting Grounds (ENGINE.md 99.2) | effect narrowed | "Move a counter from target permanent you control onto a second target permanent" as `MovePlusOneCounterBetweenPermanentsYouControl {}` (the class moves a +1/+1 counter between two permanents you control) | any kind of counter (the player's choice), and a second target permanent anyone controls | `effects/generic/counters.py::MovePlusOneCounterBetweenPermanentsYouControl` (its docstring: "Simplification") | blocked (`lower::IR_DEFECTS`) | confirmed |
| 28 | Meltstrider's Resolve (ENGINE.md 99.2) | keyword substituted | "Enchanted creature ... can't be blocked by more than one creature" as `equipment_bonuses: [0, 2, ["menace"]]` | a "can't be blocked by more than one creature" rule (menace is the opposite: by two or more only, CR 702.111b) | the Aura builder's bonus line (`cards/base.py::make_aura`, as entry 7) | fixed (20bea9f4), supported | confirmed |
| 29 | Tractor Beam (ENGINE.md 99.2) | enchant narrowed | "Enchant creature or Spacecraft" as the default `aura_enchant_type: creature` | creature or Spacecraft (a Spacecraft below its threshold is a noncreature artifact) | the Aura builder's enchant line (`EnchantKind` has no "or Spacecraft") | fixed (20bea9f4); blocked by engine features | confirmed |
| 30 | Moonlit Meditation (ENGINE.md 99.2) | enchant narrowed | "Enchant artifact or creature you control" as `aura_enchant_type: creature` (the default), `aura_yours_only` | artifact or creature you control | the Aura builder's enchant line (`EnchantKind` has no "artifact or creature") | fixed (20bea9f4); blocked by an engine feature | confirmed |
| 31 | Terrasymbiosis (ENGINE.md 99.2) | subject dropped | "whenever you put one or more +1/+1 counters on a creature you control" as `counters_added` with `Subject(IsCreature) & Controlled` (the creature's controller only) | who puts the counters is the trigger's subject too: you (not an opponent's effect putting them on your creature, Drix Fatemaker's or Rayblade Trooper's) | the trigger phrase table (`cards/parser/triggers.py`) and the Python engine's `fire_counters_added_to_your_creature_triggers` | blocked (`lower::IR_DEFECTS`) | confirmed |
| 40 | 19 lands of the EoE table: Bonders' Enclave, Cascading Cataracts, Cathedral of War, Crystal Quarry, Deserted Temple, Echoing Deeps, Inkmoth Nexus, Mutavault, Power Depot, Terrain Generator, Thespian's Stage; Blast Zone, Contested War Zone, Endless Sands, Inventors' Fair, Mirrorpool, Mystifying Maze, Nesting Grounds, Swarmyard (ENGINE.md 99.3) | ability dropped | no `{T}: Add {C}` ability, no `produces`, no `mana_only_from_produces` | `produces: [C]` with `mana_only_from_produces` (as Strip Mine since entry 1's fix) | `cards/parser/land_builder.py` (the builders that match `_DROPPED_TAP_FOR_C_LINE`, ~line 1452 on) | fixed (5362f5ed); 7 supported, the rest blocked elsewhere | confirmed |
| 41 | Blinkmoth Nexus, Inkmoth Nexus, Mutavault (ENGINE.md 99.3) | schema can't say it | `BecomeLandCreatureUEOT` with no card type and (Inkmoth, Mutavault) no creature types | "a 1/1 Blinkmoth **artifact** creature"; "Phyrexian Blinkmoth artifact creature"; "a 2/2 creature with all creature types" | `effects/generic/become_type.py::BecomeLandCreatureUEOT` (no card-type field), `cards/parser/land_builder.py::_try_build_manland` | fixed for Blinkmoth Nexus and Inkmoth Nexus (5362f5ed), supported; Mutavault fixed (20bea9f4), supported | confirmed |
| 42 | the 13 cards that create a Lander token through `CreateToken` (Beamsaw Prospector, Galactic Wayfarer, ...; ENGINE.md 99.3) | cost dropped | the token's ability costs `{2}`, sacrifice this token | "{2}, {T}, Sacrifice this token" (CR 111.10: the predefined Lander token) | the token-phrase parser's Lander printing (`TOKEN_PRINTINGS["Lander"]`) | fixed (5362f5ed); 12 supported | confirmed |
| 43 | Divert Disaster (ENGINE.md 99.3) | clause dropped | `CounterTargetSpell {}`: a hard counter | "Counter target spell unless its controller pays {2}. If they do, you create a Lander token." | the counterspell phrase parser | fixed (20bea9f4), supported | confirmed |
| 44 | Unravel (ENGINE.md 99.3) | clause dropped | `CounterTargetSpell {}` | "... If the amount of mana spent to cast that spell was less than its mana value, you draw a card." | the counterspell phrase parser | fixed (20bea9f4); blocked by an engine feature | confirmed |
| 45 | Xu-Ifit, Osteoharmonist (ENGINE.md 99.3) | clause dropped | `ReturnCreatureFromYourGraveyardToBattlefield` | "... It's a Skeleton in addition to its other types and has no abilities." | the reanimation phrase parser | fixed (20bea9f4), supported | confirmed |
| 46 | High Market (ENGINE.md 99.3) | cost widened | `sacrifice_another_creature_or_artifact: true` | "Sacrifice a creature" | `cards/parser/land_builder.py::_try_build_tap_sac_utility_land` (its docstring: "widens 'a creature' to 'a creature or artifact'") | fixed (5362f5ed), supported | confirmed |
| 47 | Power Depot (ENGINE.md 99.3) | ability dropped, restriction narrowed | one mana ability, `spend_restriction: artifact_spell`; no modular | also "{T}: Add {C}" (entry 40) and "Modular 1"; "spend this mana only to cast artifact spells **or activate abilities of artifacts**" | the artifact-land builder | fixed (20bea9f4); blocked (W2's spend restriction, modular) | confirmed |
| 48 | Starfield Shepherd (ENGINE.md 99.3) | choice reshaped | `ModalChooseOne [search basic Plains, search creature MV <= 1]` | one search for "a basic Plains card or a creature card with mana value 1 or less" | the library-search phrase parser | fixed (5362f5ed), supported | confirmed |
| 49 | Pull Through the Weft (ENGINE.md 99.3) | clause dropped | `ReturnUpToNNonlandPermanentCardsFromGraveyardToHand` only | "... then return up to two target land cards from your graveyard to the battlefield tapped" | `effects/generic/bounce.py` (its docstring: "the second ... clause is dropped at parse time") | fixed (20bea9f4), supported | confirmed |
| 50 | Sunken Citadel (ENGINE.md 99.3) | choice dropped | two mana abilities making `any` colour; no "as it enters, choose a color" | one mana / two mana "of the chosen color" | the land builder | fixed (20bea9f4); blocked (W2's spend restriction, chosen-colour mana) | confirmed |
| 51 | Knight Luminary, Cosmogrand Zenith, Honored Knight-Captain, Rayblade Trooper (ENGINE.md 99.3) | colour dropped | the Human Soldier token printing with no `color_indicator` (colourless) | `color_indicator: [W]` ("a 1/1 white Human Soldier creature token") | the token-phrase parser (`_token_printing`), as entry 12 | fixed (5362f5ed), supported | confirmed |
| 60 | All-Fates Stalker (Edge of Eternities, ENGINE.md 99.4) | target count dropped | `ExileTargetCreatureUntilSourceLeaves { excluded_subtypes: [Assassin] }`, a class with one required target (Hardlight Containment's too) | "up to one target" (a target count, minimum 0) | `effects/generic/exile.py::ExileTargetCreatureUntilSourceLeaves` (`requires_target`) | fixed (5362f5ed), supported | confirmed |
| 61 | Endless Sands (Edge of Eternities, ENGINE.md 99.4) | effect misread, ability dropped | `{2},{T}: ExileTargetCreatureUntilSourceLeaves { yours_only }`; `{4},{T}, sac: Sequence { effects: [] }` | a plain linked exile (CR 607.2a), and "return each creature card exiled with this land to the battlefield under its owner's control" | the land-ability parser (`cards/parser/land_builder.py`) | fixed (20bea9f4), supported | confirmed |
| 62 | Gravkill; Archenemy's Charm (Edge of Eternities, ENGINE.md 99.4) | target types dropped | `ExileTargetCreature { targets: [IsCreature] }` for "exile target creature or Spacecraft" / "... or planeswalker"; the Charm's second mode `ReturnUpToNCreaturesFromYourGraveyardToHand { creature }` for "one or two target creature and/or planeswalker cards" | `ObjectOr(IsCreature, HasSubtype(Spacecraft))` / `ObjectOr(IsCreature, IsPlaneswalker)`; the planeswalker cards and the count | the "exile target X" phrase parser | fixed for Gravkill (5362f5ed), supported; Archenemy's Charm fixed (20bea9f4), supported | confirmed |
| 63 | Elegy Acolyte (Edge of Eternities, ENGINE.md 99.4) | "one or more" as once per turn | "whenever one or more creatures you control deal combat damage to a player" with `once_per_turn: true` | one trigger per combat damage step (`OneOrMoreBatch`), no once-each-turn limit (as entry 16) | `cards/parser/triggers.py` (the "one or more creatures you control deal combat damage to a player" entry) | fixed (5362f5ed), supported | confirmed |
| 64 | Nexus of Fate (Edge of Eternities, ENGINE.md 99.4) | flag left unset | the self "would go to graveyard -> library" replacement with `watched_in_graveyard: false` | `watched_in_graveyard: true` (the printing's "put into a graveyard from anywhere" flag, which mtg-bot's move code reads before the replacement) | the instant builder that writes the replacement (`cards/parser/creature_rmc_grammar.py` sets the flag for the creatures only) | fixed (5362f5ed), supported | confirmed |
| 65 | Pinnacle Starcage (Edge of Eternities, ENGINE.md 99.4) | value resolved late | `CreateNTokensFromLastExileRecall { token_name: "Robot" }` | the 2/2 colorless Robot artifact creature token's `CardPrinting` inline (as entry 3) | `effects/` (the class resolves the token by name) | fixed (20bea9f4), supported | confirmed |
| 66 | Starfield Vocalist (Edge of Eternities, ENGINE.md 99.4) | event subjects dropped | `ReplacementEffect { event_pattern: EventPattern { event: triggered_ability_triggers }, transform: AddInstances { delta: 1 } }`, no subject filters (Strionic Resonator's shape, "triggered abilities trigger an additional time") | the triggering event a permanent entering the battlefield, the ability's source a permanent you control (`subject_filters`) | `cards/parser/creature_builder.py` (~line 627) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 67 | Mirrorpool (Edge of Eternities, ENGINE.md 99.4) | lossy schema, ability dropped | activation costs `mana: { generic: 3 }` for `mana_str: "{2}{C}"` and `{ generic: 5 }` for "{4}{C}"; no `produces`, no mana ability | a colorless pip (`{C}`, CR 107.4b) in each cost; "{T}: Add {C}" | `cards/base.py::parse_mana_cost` (as entry 5), the land builder | fixed (20bea9f4), supported | confirmed |
| 80 | Syr Vondam, Sunstar Exemplar (Edge of Eternities, lane D, ENGINE.md 99.5) | event widened, trigger condition moved | both triggers on `leaves_battlefield` for "dies or is put into exile"; "while its power is 4 or greater" as `ConditionalIfSourcePowerAtLeast` around the effect (read as the ability resolves) | a trigger on dying or being exiled only (not bounced, not put into a library); the power condition part of the trigger event, as the creature left (CR 603.10a, 603.6c) | `cards/parser/triggers.py` (the "dies or is put into exile" phrase-table entries, ~line 718; `_INTERVENING_WHILE_EFFECT_WRAPPERS`, ~line 1645) | fixed (20bea9f4), supported | confirmed |
| 81 | Dawnsire, Sunstar Dreadnought (lane D) | target narrowed | `DealDamageToTargetCreature` with a creature-only target for "up to one target creature or planeswalker" | a creature or planeswalker target | `cards/parser/effects.py` (`_DEAL_DAMAGE_UP_TO_ONE_TARGET_CREATURE`, ~line 390: "or planeswalker" accepted and dropped) | fixed (5362f5ed), supported | confirmed |
| 82 | Entropic Battlecruiser (lane D) | target invented | the 1+ trigger "Whenever an opponent discards a card, they lose 3 life" as `TargetPlayerLosesLife` (a targeted effect) | the discarding player, not targeted (the event's player: `BindEventPlayer`, as the cube's "that player loses" triggers) | `cards/parser/effects.py` (`_THEY_LOSE_N_LIFE`, ~line 1718; the Python engine pre-wires the player as a target) | fixed (5362f5ed), supported | confirmed |
| 100 | Fell Gravship (Edge of Eternities, cross-lane pass, ENGINE.md 99.8) | target invented | "mill three cards, then return a creature or Spacecraft card from your graveyard to your hand" as `Sequence [SelfMill 3, ReturnCreatureFromYourGraveyardToHand { creature, or_spacecraft }]`, a targeted class (its target chosen as the ability triggers, before the mill) | the card chosen as the ability resolves, after the mill, not targeted (a milled card may be returned; the mill happens with no such card in the graveyard) | `cards/parser/effects.py` (`_RETURN_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_HAND`, ~line 1911) | fixed (20bea9f4), supported | confirmed |
| 101 | Desculpting Blast (Edge of Eternities, cross-lane pass, ENGINE.md 99.8) | value resolved late | `ReturnTargetCreatureToOwnersHand { allow_nonland_permanent, create_drone_if_was_attacking: true }`: the token by a flag, its printing looked up by name in Python (`TOKEN_PRINTINGS["Drone"]`) | the token's `CardPrinting` inline (a 1/1 colorless Drone artifact creature with flying and "This token can block only creatures with flying"), as the set's other Drone makers carry it (as entries 3, 65) | `cards/parser/effects.py` (the Desculpting Blast literal, ~line 4324), `effects/generic/bounce.py` (~line 177) | fixed (20bea9f4), supported | confirmed |
| 160 | Plaza of Heroes (Edge of Eternities W2, ENGINE.md 99.12) | ability reshaped | its third mana ability as `AddManaToPool { produces: [any 1], spend_restriction: legendary_permanent_ability }` | "{T}: Add one mana of any color among legendary permanents you control" (no restriction: `AddOneManaOfColorAmongControlled { legendary: true }`) | `cards/parser/effects.py` (`_ADD_ANY_MANA_AMONG_GROUP`, `_ADD_MANA_AMONG_RESTRICTION`, ~line 1670) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 161 | Contested War Zone (Edge of Eternities W2, ENGINE.md 99.12) | event widened | `deals_combat_damage` with `And [Subject(IsCreature, source), Controlled(target)]` | "whenever a creature deals combat damage to **you**": also `Subject(IsPlayer, target)` | `cards/parser/land_builder.py` (the Contested War Zone builder, ~line 2897) | blocked (`lower::IR_DEFECTS`) | confirmed |
| 180 | (fixed by W1, mtg-bot feature/eoe-ir-fixes 7c02e5c9, ENGINE.md 99.11) Sothera, the Supervoid (Edge of Eternities, W4, ENGINE.md 99.14) | instructions reordered | the end-step ability as `Sequence [ReanimateOneExiledWithSourceWithCounters { counter_amount: 2 }, SacrificeSelfPreservingExileLink]`: the return first, then the sacrifice | "sacrifice Sothera, then put a creature card exiled with it onto the battlefield ...": the sacrifice first (the cards exiled with it stay linked to it, CR 607.2a) | `effects/generic/exile.py::SacrificeSelfPreservingExileLink` (its docstring: "the engine-level fix is to invert the order at composition time"), the Sothera parser line | blocked (`lower::IR_DEFECTS`) | confirmed |
| 190 | Palantír of Orthanc (faithfulness audit N17, ENGINE.md 100.6) | Python switch | `Scry { amount: 2 }` with `bottom_choice` left at its default `false` ("keep all on top", the Python class's first-slice shortcut) | a scry (CR 701.18a: any number on the bottom): `bottom_choice: true` | `cards/parser/artifact_effects.py::_parse_palantir_body`; `effects/generic/library_lookat.py::Scry` (the default) | supported; the field is ignored, every scry lowers as 701.18a | confirmed |
| 191 | Chain of Smog (docs/shortcuts.md 26.11.9, ENGINE.md 100.6) | Python switch | `TargetPlayerMayCopyThisSpell` with `needs_target_with_cards_in_hand` at its default `true` (the copy offered only while a player it could target has cards in hand) | no condition ("That player may copy this spell") | `effects/generic/spell_copies.py::TargetPlayerMayCopyThisSpell` (the default) | supported; the field is ignored, the copy is always offered | confirmed |

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
* The refusal stays in the lowering after the fix (the faithfulness audit's N16 calls it a repair:
  it repairs nothing): a land that makes coloured mana and animates with no `set_colors` is refused,
  never given colours or a creature type the IR doesn't state. It is a loud guard against this
  parser path coming back. A land that makes coloured mana and really animates into a colourless
  creature would be refused too (none is in the cube or the Edge of Eternities tables): a refusal,
  not a wrong game.
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
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: the planets carry `station_threshold: 12` (no
  power/toughness: CR 722.1) and their "12+ |" activated ability with `tier_threshold: 12`; The
  Eternity Elevator `station_threshold: 20`. New grammar for the striations: a four-piece activation
  cost (Susur Secundi), "X, then Y and Z" (Kavaron), `DrawCardsEqualTo(PowerOfCostPaidObject)`
  (Susur Secundi), a token copy "except it's legendary" (`CopyExceptions.add_supertypes`, Adagia).
  Evendo, Kavaron and Uthros are supported; Adagia (`Effect:CreateTokenCopyOfTargetWithExceptions`:
  the engine's copy exceptions have no supertype), Susur Secundi (`Effect:DrawCardsEqualTo`: no
  amount for the sacrificed creature's power) and The Eternity Elevator
  (`ManaAbility:AddChargeCounterManaOfChosenColor`) are blocked by engine features.


### L. Lane A of the Edge of Eternities port (ENGINE.md 99.2)

Found by reading every lane A card's IR against its Oracle text while porting (2026-11-07, branch
`eoe-a-station`).

**21. Dockworker Drone, Luxknight Breacher (and Blast Zone).** Oracle: "This creature enters with a
+1/+1 counter on it. / When this creature dies, put its counters on target creature you control."
(Dockworker Drone); "This creature enters with a +1/+1 counter on it for each other creature and/or
artifact you control." (Luxknight Breacher); "This land enters with a charge counter on it." (Blast
Zone).

* IR: an `enters` triggered ability, `IsSelf`, with `PutPlusOneCounterOnSelf`,
  `PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact` and `PutChargeCountersOnSelf`.
* Should be: a replacement effect, "enters with" (CR 614.1c, 122.6): the permanent is on the
  battlefield with the counters from the start (no trigger to respond to; the count read as it
  enters, before it is there: "each other creature" can't count it, and nothing sees a 1/1 Drone or
  a counterless Blast Zone). `enters_with_named_counters: [["+1/+1", 1]]` / `[["charge", 1]]`; the
  schema has no "for each <permanent> you control" count yet (Luxknight).
* Source: the creature parser's "enters with" lines (`cards/parser/creature_builder.py`,
  `cards/parser/effects.py`) and the land builder (Blast Zone).
* Fix: lower "enters with N <kind> counter(s)" to `enters_with_named_counters`; add a count field for
  "for each <filter> you control".
* Engine: blocked, `ir_defect(enters-with counters as a trigger, docs/ir_defects.md 21)`
  (`lower::IR_DEFECTS`).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: Dockworker Drone and Blast Zone carry
  `enters_with_named_counters` (`[["+1/+1", 1]]`, `[["charge", 1]]`), no enters trigger. Not fixed
  for Luxknight Breacher: the schema still has no "for each <permanent> you control" count (it stays
  in `lower::IR_DEFECTS`). Dockworker Drone is supported; Blast Zone is blocked by engine features
  (`{X}{X}` costs, its sweeper).
* Fixed for Luxknight Breacher in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `CardPrinting.enters_with_plus_one_counters_per` (an object predicate: other creatures and/or artifacts you control), no enters trigger; counted as it enters, not those entering at the same time (the card's ruling). Supported.

**22. Haliya, Ascendant Cadet.** Oracle: "Whenever Haliya enters or attacks, put a +1/+1 counter on
target creature you control. / Whenever one or more creatures you control with +1/+1 counters on
them deal combat damage to a player, draw a card."

* IR: the second ability is a `deals_combat_damage` trigger over each creature with a counter, with
  `once_per_turn: true` (the parser's marker for the Python engine's aggregated fire path).
* Should be: once per combat damage event ("one or more", `OneOrMoreBatch`): a first-strike step and
  the regular step are two events, an additional combat more (as entry 16, Forth Eorlingas!).
* Source: `cards/parser/triggers.py`, the phrase-table entry "one or more creatures you control with
  +1/+1 counters on them deal combat damage to a player" (~line 1015: "the 3-tuple value form threads
  `once_per_turn=True` so the aggregated path can distinguish this shape").
* Fix: export the batch predicate (as entry 16's fix) and no `once_per_turn`.
* Engine: blocked, `ir_defect(one or more as once per turn, docs/ir_defects.md 22)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: the predicate is `... & OneOrMoreBatch {
  per_target_player: true }`, no `once_per_turn` (a trigger per combat damage batch and damaged
  player; the engine is two-player, so one batch). Supported.

**23. Honored Knight-Captain, Rayblade Trooper (and Cosmogrand Zenith, Knight Luminary).** Oracle:
"When this creature enters, create a 1/1 white Human Soldier creature token." (Honored
Knight-Captain; Rayblade Trooper: "Whenever a nontoken creature you control with a +1/+1 counter on
it dies, create a 1/1 white Human Soldier creature token.")

* IR: the token printing `Human Soldier` (`card_types: [Creature]`, 1/1, no mana cost) has no
  `color_indicator`: a colourless token.
* Should be: `color_indicator: [W]` (CR 111.4: the token's colour is the one the effect states).
* Source: `cards/tokens.py`, `TOKEN_PRINTINGS["Human Soldier"]` (~line 141), built without a colour.
* Fix: give the registry entry its colour (as entry 12's Boo).
* Engine: Honored Knight-Captain and Rayblade Trooper blocked, `ir_defect(white token printed
  colourless, docs/ir_defects.md 23)`. Cosmogrand Zenith and Knight Luminary have the same token and
  are in `SUPPORTED_EOE_BASE` (lane B audits that list; lane A does not edit it).
* Lane B found the same defect independently (its entry 51, branch `eoe-b-lands`, with the four
  cards in its `IR_DEFECTS` section): at the merge, keep one entry and one `IR_DEFECTS` line per
  card.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: see entry 51 (the same token).

**24. Mechan Shieldmate.** Oracle: "Defender / As long as an artifact entered the battlefield under
your control this turn, this creature can attack as though it didn't have defender."

* IR: a self-scoped static with `condition: ControlledArtifactEnteredThisTurn` and the payload
  `RemoveKeyword(defender)`.
* Should be: a permission to attack (CR 702.3b: defender only stops attacking); the creature keeps
  defender (what counts or grants defender still sees it).
* Source: `cards/parser/creature_builder.py` (~line 672) and `cards/parser/printing_keywords.py`
  (~line 473).
* Fix: a "can attack as though it didn't have defender" flag (with its condition).
* Engine: blocked, `ir_defect(attack permission as losing defender, docs/ir_defects.md 24)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): the static's payload is `CanAttackAsThoughNoDefender` (it keeps defender; `Mod::AttackDespiteDefender`), with `ControlledArtifactEnteredThisTurn` (`Player::artifact_entered`). Supported.

**25. Loading Zone.** Oracle: "If one or more counters would be put on a creature, Spacecraft, or
Planet you control, twice that many of each of those kinds of counters are put on it instead. / Warp
{G}"

* IR: `StaticAbility { scope: self, replacement_effect: ReplacementEffect { event: would_place_counter,
  transform: MultiplyCount(2) } }`: read as written, counters put on Loading Zone itself.
* Should be: the recipients, creatures, Spacecraft and Planets you control, and every kind of
  counter (CR 614.1a).
* Source: `cards/parser/enchantment_builder.py`, `_COUNTER_DOUBLER_LINE` (~line 651); the Python
  engine hard-codes creatures and Spacecraft and +1/+1 counters only
  (`game.py::_counter_doubling_factor`).
* Fix: export the recipient filter (and the kinds) with the replacement.
* Engine: blocked, `ir_defect(counter doubler without its recipients, docs/ir_defects.md 25)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): the replacement's `EventPattern.subject_filters` names the recipients (`obj`: a creature, Spacecraft or Planet; the controller is the replacement's `affects`). Still blocked by the engine: no counter-multiplying replacement (`ReplacementEffect(WouldPlaceCounter, MultiplyCount)`).

**26. The Eternity Elevator.** Oracle: no power/toughness box ("{T}: Add {C}{C}{C}. / Station ... /
20+ | {T}: Add X mana of any one color, where X is the number of charge counters on The Eternity
Elevator.").

* IR: `power: 0, toughness: 0`.
* Should be: no power or toughness (CR 722.1: a station card "may have" a power/toughness box; this
  one has none, so it never becomes a creature, 722.2a).
* Source: `cards/parser/spacecraft_builder.py`, the missing-P/T fallback (~line 334: "the builder now
  falls back to `power=0, toughness=0`").
* Fix: leave power and toughness unset.
* Engine: blocked by entry 20 (`lower::IR_DEFECTS` names a card once). Once entry 20 is fixed this
  one matters: the engine reads a station card's power/toughness as the box of its
  `station_threshold` striation (ENGINE.md 99.2), so a 0/0 would make it a 0/0 creature at 20+.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: The Eternity Elevator has no power or toughness
  (mtg-bot's `game.is_stationed` is false for a station card without them).

**27. Nesting Grounds.** Oracle: "{T}: Add {C}. / {1}, {T}: Move a counter from target permanent you
control onto a second target permanent. Activate only as a sorcery."

* IR: the second ability's effect is `MovePlusOneCounterBetweenPermanentsYouControl {}`: the class
  (mtg-bot `effects/generic/counters.py`) moves exactly one +1/+1 counter, both targets permanents its
  controller controls ("Simplification: oracle says 'a counter' (any type ...). This implementation
  moves exactly one +1/+1 counter").
* Should be: a counter of any kind on the first target (its controller's choice of kind), onto a
  second target permanent anyone controls (CR 122.8-like "move": removed from one, put on the
  other).
* Source: the class and its parser entry.
* Fix: a "move a counter" effect with the kind chosen as it resolves and the second target's filter
  as printed.
* Engine: blocked, `ir_defect(move a counter narrowed, docs/ir_defects.md 27)` (a cross-lane card,
  lanes A and B).

**28. Meltstrider's Resolve.** Oracle: "Enchant creature you control / When this Aura enters,
enchanted creature fights up to one target creature an opponent controls. / Enchanted creature gets
+0/+2 and can't be blocked by more than one creature."

* IR: `equipment_bonuses: [0, 2, ["menace"]]`.
* Should be: +0/+2 and the rule "can't be blocked by more than one creature" (CR 509.1b: a blocking
  restriction). Menace (702.111b) says the opposite: "can't be blocked except by two or more
  creatures".
* Source: the Aura builder's bonus line (the misnamed field of entry 7; the phrase mapped to a
  keyword).
* Fix: export the restriction (a "max blockers 1" field, as `min_blockers` exists).
* Engine: blocked, `ir_defect(menace for "by more than one", docs/ir_defects.md 28)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `equipment_bonuses: [0, 2, []]` and `CardPrinting.attached_cant_be_blocked_by_more_than_one` (an Aura's blocking restriction, CR 509.1b). Supported.

**29. Tractor Beam.** Oracle: "Enchant creature or Spacecraft / When this Aura enters, tap enchanted
permanent. / You control enchanted permanent. / Enchanted permanent doesn't untap during its
controller's untap step."

* IR: no `aura_enchant_type` (the default `creature`).
* Should be: "creature or Spacecraft": a Spacecraft is an artifact that is a creature only at its
  threshold (CR 722.2b), so it may be enchanted (and stays enchanted) below it too (303.4c).
* Source: the Aura builder's enchant line; `EnchantKind` (creature, artifact, land, creature card in a
  graveyard) can't say "or Spacecraft".
* Fix: an enchant filter (a predicate) instead of the enum, or an `or_spacecraft` flag.
* Engine: blocked, `ir_defect(enchant creature or Spacecraft as creature, docs/ir_defects.md 29)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `aura_enchant_type: creature_or_spacecraft` (a new `EnchantKind`). Still blocked by the engine: an Aura's "you control enchanted permanent" (`attached_grants_control`), "doesn't untap" (`attached_prevents_untap`) and `TapEnchantedPermanent`.

**30. Moonlit Meditation.** Oracle: "Enchant artifact or creature you control / The first time you
would create one or more tokens each turn, you may instead create that many tokens that are copies of
enchanted permanent."

* IR: no `aura_enchant_type` (the default `creature`) and `aura_yours_only: true`: "creature you
  control".
* Should be: "artifact or creature you control".
* Source: as entry 29 (`EnchantKind` has no "artifact or creature").
* Fix: as entry 29.
* Engine: blocked, `ir_defect(enchant artifact or creature as creature, docs/ir_defects.md 30)` (a
  cross-lane card, lanes A and D).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `aura_enchant_type: artifact_or_creature` (a new `EnchantKind`) with `aura_yours_only`. Still blocked by the engine: the token replacement (`replaces_first_tokens_with_copies`).

**31. Terrasymbiosis.** Oracle: "Whenever you put one or more +1/+1 counters on a creature you
control, you may draw that many cards. Do this only once each turn."

* IR: `event: counters_added`, `predicate: And(Subject(IsCreature), Controlled)`: counters put on a
  creature you control, by anyone.
* Should be: counters you put (the controller of the spell or ability that puts them, or you as
  you pay a cost or as a permanent you control enters with them) on a creature you control. An
  opponent's "put a +1/+1 counter on target creature" (Drix Fatemaker, Rayblade Trooper, Haliya)
  aimed at your creature doesn't trigger it.
* Source: the trigger phrase table (`cards/parser/triggers.py`); the Python engine fires it for any
  counters added to its controller's creatures.
* Fix: a subject for the player who puts the counters (and the event's count, "that many").
* Engine: blocked, `ir_defect(who puts the counters dropped, docs/ir_defects.md 31)`. (The engine's
  `CountersAdded` event names neither the player nor the count yet; both come with the fix.)
### M. Edge of Eternities, lane B: lands, Landers and the base cards' audit (ENGINE.md 99.3)

Found by reading the IR of every card of `SUPPORTED_EOE_BASE` and of lane B's cards next to its
Oracle text (`data/cards/eoe.ir.json`, mtg-bot main 2cb79f77), after a scripted check of mana
costs, types, power and toughness, keywords and the mana each land makes.

**40. "{T}: Add {C}." dropped from 19 lands.** Oracle (Mutavault): "{T}: Add {C}. / {1}: This land
becomes a 2/2 creature with all creature types until end of turn. It's still a land." Every one of
the 19 has the line "{T}: Add {C}." (Cascading Cataracts, Crystal Quarry and Power Depot beside
another mana ability).

* IR: no `{T}: Add {C}` mana ability, `produces: []`, `mana_only_from_produces: false`. The Python
  engine pays one generic mana with such a land anyway; mtg-bot's own comment on the field says
  "pre-existing printings rely on it for their dropped `{T}: Add {C}.` lines" (`cards/base.py`).
* Should be: `produces: ["C"]` with `mana_only_from_produces: true`, as entry 1's fix gives Strip
  Mine (or an explicit `{T}: AddManaToPool [C, 1]`).
* Source: `cards/parser/land_builder.py`, the builders that match the "{T}: Add {C}." line and keep
  only the others (`_DROPPED_TAP_FOR_C_LINE`, ~line 1452: `_try_build_paid_fancy_mana_land`,
  `_try_build_keyword_colorless_utility_land`, `_try_build_paid_effect_utility_land`, ...).
* Fix: pass `produces=frozenset({"C"})` and `mana_only_from_produces=True` wherever the line is
  matched.
* Engine: blocked, `ir_defect({T}: Add {C} dropped, docs/ir_defects.md 40)`. It is not a missing
  engine feature: the generic fallback is not "{T}: Add {C}" (it can't pay a {C} pip and isn't a
  mana ability) and lands pay only through their mana abilities here (ENGINE.md 14, 19); reading an
  absent ability as "{T}: Add {C}" would invent data the IR doesn't hold. The 16 with no other mana
  ability also carry the engine's own consistency blocker
  `CardPrinting.mana_only_from_produces(land without a mana ability)`, which is what lane B's
  feature of that name (18 cards) turned out to be: 16 of the 18 are this defect; the other two,
  Meteor Crater and Reflecting Pool, have a mana ability the engine didn't lower yet.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: every one of the 19 has `produces: ["C"]` and
  `mana_only_from_produces: true`. Supported now: Bonders' Enclave, Cathedral of War, Deserted
  Temple, Inventors' Fair, Mystifying Maze, Terrain Generator; Inkmoth Nexus with entry 41. The
  others are blocked by engine features or other IR defects (each card's blockers in the support
  report).

**41. An animated land's types dropped.** Oracle: Blinkmoth Nexus "{1}: This land becomes a 1/1
Blinkmoth artifact creature with flying until end of turn. It's still a land."; Inkmoth Nexus "... a
1/1 Phyrexian Blinkmoth artifact creature with flying and infect ..."; Mutavault "... a 2/2 creature
with all creature types ...".

* IR: `BecomeLandCreatureUEOT { power, toughness, granted_keywords, granted_subtype }`: Blinkmoth
  Nexus's has `granted_subtype: "Blinkmoth"` and no artifact; Inkmoth's and Mutavault's have no
  subtype either. The class has no field for a card type, and one `granted_subtype` (a string) can't
  say "Phyrexian Blinkmoth" or "all creature types".
* Should be: the added card types (`Artifact`) and creature types (a list, or "all creature types",
  CR 205.3m) of the animation.
* Source: `effects/generic/become_type.py::BecomeLandCreatureUEOT` (schema) and
  `cards/parser/land_builder.py::_try_build_manland`.
* Fix: add `granted_types` and make `granted_subtype` a list (or an "all creature types" flag).
* Engine: blocked (`lower::IR_DEFECTS`). Blinkmoth Nexus was supported at the import (it lowered as
  a non-artifact Blinkmoth creature: wrong for every "artifact" rule of the set) and is removed from
  `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: `BecomeLandCreatureUEOT` gains `granted_card_types`
  and `granted_subtypes`: Blinkmoth Nexus `["Artifact"]` (with `granted_subtype: "Blinkmoth"`),
  Inkmoth Nexus `["Artifact"]`, `["Phyrexian", "Blinkmoth"]`; both supported. Not fixed for
  Mutavault: "all creature types" (CR 205.3m) has neither an IR field nor an engine feature (it
  stays in `lower::IR_DEFECTS`).
* Fixed for Mutavault in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `BecomeLandCreatureUEOT.all_creature_types` (CR 205.3m: `Mod::AllCreatureTypes`, the creature types of the rule's list). Supported.

**42. The Lander token without {T}.** Oracle (reminder text, CR 111.10's predefined token): "It's
an artifact with '{2}, {T}, Sacrifice this token: Search your library for a basic land card, put it
onto the battlefield tapped, then shuffle.'"

* IR: in each of the 13 `CreateToken` Lander printings, the ability's cost is
  `{ mana: {2}, sacrifice_self: true }`, without `tap_self`.
* Should be: `tap_self: true`. It matters: a tapped Lander (Secluded Starforge's "tap X untapped
  artifacts", Mental Modulation's tap) can't be sacrificed for its ability, and a Lander made a
  creature (Synthesizer Labship, Tezzeret's emblem) can't use it the turn it became one (CR 302.6).
* Source: the token-phrase parser's Lander printing (`TOKEN_PRINTINGS["Lander"]`, built from the
  reminder text).
* Fix: add the `{T}`.
* Engine: blocked (`lower::IR_DEFECTS`, the 13 cards). The engine has its own Lander token from CR
  111.10 (`lower/eoe_b.rs`), with the {T}, for the effect classes that create one without a printing
  (Kav Landseeker, Emergency Eject, Orbital Plunge); the IR's printing is not overridden by it.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: the Lander token's cost is `{ mana: {2}, tap_self,
  sacrifice_self }`. The 12 cards whose only blocker it was are supported; Bioengineered Future is
  blocked by an engine feature (`grants_etb_counter_per_land_entered_this_turn`).

**43. Divert Disaster.** Oracle: "Counter target spell unless its controller pays {2}. If they do,
you create a Lander token."

* IR: `spell_effect: CounterTargetSpell {}`, a hard counter for {1}{U}.
* Should be: a counter unless its controller pays {2}, then, if they paid, a Lander token for the
  spell's caster (the controller of Divert Disaster).
* Engine: blocked (`lower::IR_DEFECTS`); removed from `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `CounterSpell { unless_pays: "{2}", if_paid: CreateToken(<the Lander>) }`; the card left mtg-bot's `RUST_PARITY_PINNED_SPELLS`. Supported.

**44. Unravel.** Oracle: "Counter target spell. If the amount of mana spent to cast that spell was
less than its mana value, you draw a card."

* IR: `CounterTargetSpell {}`; the draw is dropped (a warp-cast or cost-reduced spell would draw).
* Engine: blocked (`lower::IR_DEFECTS`); removed from `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `CounterSpell { if_mana_spent_below_mv: DrawCards(1) }`. Still blocked by the engine: the mana spent to cast a spell isn't recorded (`CounterSpell.if_mana_spent_below_mv`).

**45. Xu-Ifit, Osteoharmonist.** Oracle: "{T}: Return target creature card from your graveyard to
the battlefield. It's a Skeleton in addition to its other types and has no abilities. Activate only
as a sorcery."

* IR: `ReturnCreatureFromYourGraveyardToBattlefield { creature: true, destination: battlefield }`;
  the returned creature keeps its abilities and types.
* Engine: blocked (`lower::IR_DEFECTS`); removed from `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `ReturnTargetCardFromYourGraveyardToBattlefieldAs { card_filter: creature, added_subtypes: [Skeleton], loses_all_abilities }`, applied as it enters (its ruling: its enters abilities and "enters with" replacements don't apply). Supported.

**46. High Market.** Oracle: "{T}: Add {C}. / {T}, Sacrifice a creature: You gain 1 life."

* IR: `ActivationCost { tap_self, sacrifice_another_creature_or_artifact: true }`: an artifact (a
  Lander, a Robot, Munitions) may be sacrificed.
* Source: `cards/parser/land_builder.py::_try_build_tap_sac_utility_land`, whose docstring says the
  flag "widens 'a creature' to 'a creature or artifact'".
* Fix: a "sacrifice a creature" cost.
* Engine: blocked (`lower::IR_DEFECTS`); removed from `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: High Market's cost is `sacrifice_a_permanent_type:
  "creature"`. Supported.

**47. Power Depot.** Oracle: "This land enters tapped. / {T}: Add {C}. / {T}: Add one mana of any
color. Spend this mana only to cast artifact spells or activate abilities of artifacts. / Modular
1".

* IR: one mana ability, `AddManaToPool [any, 1]` with `spend_restriction: "artifact_spell"`; no
  "{T}: Add {C}" (entry 40), no modular (CR 702.43).
* Should be: the {C} ability, a restriction that also allows artifacts' abilities, and modular 1.
* Engine: blocked (`lower::IR_DEFECTS`); removed from `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `keywords: [modular 1]` and `spend_restriction: artifact_spell_or_activation`. Still blocked by the engine: the spend restriction (W2's, ENGINE.md 99.12) and modular (CR 702.43).

**48. Starfield Shepherd.** Oracle: "When this creature enters, search your library for a basic
Plains card or a creature card with mana value 1 or less, reveal it, put it into your hand, then
shuffle."

* IR: `ModalChooseOne [SearchLibraryForCard basic_plains, SearchLibraryForCard creature_small
  (max_mv 1)]`: the player picks the kind of card when the ability is put on the stack (CR 603.3c),
  publicly and before the search; the card lets them pick any card of either kind during the search
  (a draw or mill in response changes which modes would find something).
* Should be: one `SearchLibraryForCard` with an `ObjectOr` filter.
* Engine: blocked (`lower::IR_DEFECTS`); removed from `SUPPORTED_EOE_BASE`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: one `SearchLibrary { card_filter: CardFilterAnyOf
  [basic Plains, creature with mana value 1 or less], reveal }` (a new card-filter class: a card
  matching any of its filters; the engine lowers it to `Filter::Or`). Supported.

**49. Pull Through the Weft.** Oracle: "Return up to two target nonland permanent cards from your
graveyard to your hand, then return up to two target land cards from your graveyard to the
battlefield tapped."

* IR: `ReturnUpToNNonlandPermanentCardsFromGraveyardToHand`; its docstring: "the second 'lands to
  battlefield tapped' clause is dropped at parse time".
* Engine: blocked (`lower::IR_DEFECTS`).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `ReturnTargetCardsFromYourGraveyard` with two target groups (nonland permanent cards 0-2 to the hand; land cards 0-2 onto the battlefield tapped). Supported.

**50. Sunken Citadel.** Oracle: "This land enters tapped. As it enters, choose a color. / {T}: Add
one mana of the chosen color. / {T}: Add two mana of the chosen color. Spend this mana only to
activate abilities of land sources."

* IR: `enters_tapped`, `AddManaToPool [any, 1]` and `AddManaToPool [any, 2]` (restricted); no colour
  chosen as it enters: any colour on every activation.
* Should be: the entering choice and mana of that colour.
* Engine: blocked (`lower::IR_DEFECTS`).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `enters_choosing_color` and `produces: [[chosen, 1]]` / `[[chosen, 2]]`. Still blocked by the engine: the spend restriction (W2's, ENGINE.md 99.12) and mana of the colour chosen as it entered.

**51. The Human Soldier token's white dropped.** Oracle (Knight Luminary): "When this creature
enters, create a 1/1 white Human Soldier creature token." Cosmogrand Zenith ("create two 1/1 white
Human Soldier creature tokens"), Honored Knight-Captain and Rayblade Trooper make the same token.

* IR: the `CreateToken` printing `{ name: "Human Soldier", card_types: [Creature], subtypes: [Human,
  Soldier], power: 1, toughness: 1 }` with no `color_indicator`: a colourless token.
* Should be: `color_indicator: ["W"]` (as entry 12's fix gives Boo; the set's other tokens, Robot,
  Drone, Sliver, Munitions, Eldrazi Scion, are colourless in the Oracle text too, and right).
* Source: the token-phrase parser (`_token_printing`), on the path these four cards take.
* Engine: blocked (`lower::IR_DEFECTS`). Knight Luminary and Cosmogrand Zenith were supported at
  the import and are removed from `SUPPORTED_EOE_BASE`; found by a card test
  (`tests/cards_eoe_b_base.rs`) asserting the token's colour, then a scan of every token printing
  of the EoE IR against its Oracle text.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: the registry's Human Soldier carries
  `color_indicator: ["W"]`, and the token grammar now builds a registry token from its printed
  characteristics when its printed colour differs (every token colour of the pools checked). The
  four cards are supported.
### N. Edge of Eternities lane C (ENGINE.md 99.4)

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
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: `ExileTargetCreatureUntilSourceLeaves.up_to_one:
  true` (zero or one target; the engine's target has minimum 0). Supported.

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
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `ExileLinkedToSource` (a creature you control) and `ReturnCardsExiledWithSourceToBattlefield { card_filter: creature }` (only creature cards return, the card's rulings). Supported.

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
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: Gravkill's effect is `RemoveTargetPermanents {
  action: exile, target_filter: ObjectOr(IsCreature, HasSubtype(Spacecraft)) }` (the precise
  grammar; "Spacecraft" is a target noun now). Supported. Not fixed for Archenemy's Charm: mtg-bot
  pins its parse to the lossy legacy grammar for its own cross-engine parity tests
  (`precise_grammar.RUST_PARITY_PINNED_SPELLS`); it stays in `lower::IR_DEFECTS`.
* Fixed for Archenemy's Charm in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): the first mode is `RemoveTargetPermanents(exile, creature or planeswalker)` (the precise grammar, the card no longer pinned), the second `ReturnTargetCardsFromYourGraveyard` (one or two creature and/or planeswalker cards). Supported.

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
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: as entry 22: `OneOrMoreBatch { per_target_player:
  true }`, no `once_per_turn`. Supported.


**64. Nexus of Fate.** Oracle: "Take an extra turn after this one. / If Nexus of Fate would be put
into a graveyard from anywhere, reveal Nexus of Fate and shuffle it into its owner's library
instead."

* IR: the replacement is there (`StaticAbility { scope: self, replacement_effect: { event:
  would_go_to_graveyard, transform: Replace(library) } }`) but `watched_in_graveyard` is false. The
  flag is what mtg-bot's zone-change code checks before applying a graveyard self-replacement
  (`turn.py`, "Blightsteel"), so as exported the replacement never applies there; the cube's cards
  with this replacement carry the flag.
* Should be: `watched_in_graveyard: true`.
* Source: the printing builder that writes the instant's replacement (the creature grammar sets
  the flag, `cards/parser/creature_rmc_grammar.py`).
* Fix: set the flag wherever the "would be put into a graveyard from anywhere" replacement is
  built.
* Engine: the lowering checks that the flag and the abilities agree (`printing.rs`, "watched in
  graveyard"); blocked, `ir_defect(watched_in_graveyard unset, docs/ir_defects.md 64)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: Nexus of Fate has `watched_in_graveyard: true`.
  Supported.

**65. Pinnacle Starcage.** Oracle: "When this artifact enters, exile all artifacts and creatures
with mana value 2 or less until this artifact leaves the battlefield. / {6}{W}{W}: Put each card
exiled with this artifact into its owner's graveyard, then create a 2/2 colorless Robot artifact
creature token for each card put into a graveyard this way. Sacrifice this artifact."

* IR: `CreateNTokensFromLastExileRecall { token_name: "Robot" }`: the token by name.
* Should be: the token's `CardPrinting` inline (a 2/2 colorless Robot artifact creature), as the
  set's other Robot makers carry it (`CreateToken { token_printing }`), entry 3's fix.
* Fix: export the token printing.
* Engine: blocked, `ir_defect(token printing by name only, docs/ir_defects.md 65)`; its other
  two classes (`ExileAllMatchingUntilSourceLeaves`, `MoveEachExiledWithSourceToOwnersGraveyard`)
  are not built (no other card uses them).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `CreateNTokensFromLastExileRecall.token_printing` (the 2/2 colorless Robot artifact creature). With the engine's `ExileAllMatchingUntilSourceLeaves` and `MoveEachExiledWithSourceToOwnersGraveyard` (ENGINE.md 99.11). Supported.

**66. Starfield Vocalist.** Oracle: "If a permanent entering the battlefield causes a triggered
ability of a permanent you control to trigger, that ability triggers an additional time. / Warp
{1}{U}"

* IR: `StaticAbility { scope: self, replacement_effect: ReplacementEffect { event_pattern:
  EventPattern { event: triggered_ability_triggers }, transform: AddInstances { delta: 1 } } }`
  with no `subject_filters`. mtg-bot's event docstring: the shape models both Strionic Resonator's
  "your triggered abilities trigger an additional time" and the Vocalist's narrower one, so the IR
  cannot tell them apart.
* Should be: subject filters for the event's two subjects (`triggering_obj`: a permanent entering
  the battlefield; `ability_source`: a permanent you control).
* Source: `cards/parser/creature_builder.py` (~line 627).
* Fix: write the two subject filters.
* Engine: blocked, `ir_defect(event subjects dropped, docs/ir_defects.md 66)`.


**67. Mirrorpool.** Oracle: "This land enters tapped. / {T}: Add {C}. / {2}{C}, {T}, Sacrifice this
land: Copy target instant or sorcery spell you control. You may choose new targets for the copy. /
{4}{C}, {T}, Sacrifice this land: Create a token that's a copy of target creature you control."

* IR: the two activated abilities' `mana` are `{ generic: 3 }` and `{ generic: 5 }` beside
  `mana_str` "{2}{C}" and "{4}{C}": the colorless pip (CR 107.4b, payable only with colorless mana)
  became generic, entry 5's lossy parse. The printing has no `produces` and no mana ability: "{T}:
  Add {C}" is dropped.
* Should be: a colorless pip in each cost; the mana ability (or `produces: [C]`).
* Source: `cards/base.py::parse_mana_cost` (entry 5) and the land builder.
* Engine: blocked, `ir_defect(colorless pips as generic, docs/ir_defects.md 67)`; its copy
  effects (`CopyTargetInstantOrSorceryYouControl`, `CreateTokenCopyOfTargetCreatureYouControl`)
  are not built (no other card uses them).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): the costs carry a colorless pip (`colored: [[C, 1]]`, mtg-bot's `parse_mana_cost(colorless_pips=True)`; the cube's lossy `{C}` costs, entry 5, are unchanged); `produces: [C]` was fixed by entry 40. Supported.

### O. Edge of Eternities, lane D (ENGINE.md 99.5)

**80. Syr Vondam, Sunstar Exemplar.** Oracle: "Vigilance, menace / Whenever another creature you
control dies or is put into exile, put a +1/+1 counter on Syr Vondam and you gain 1 life. / When Syr
Vondam dies or is put into exile while its power is 4 or greater, destroy up to one target nonland
permanent."

* IR (mtg-bot main 2cb79f77): both triggered abilities have `event: leaves_battlefield` (the first
  with `Subject(IsCreature) & Controlled & Not(IsSelf)`, the second with `IsSelf`); the second's
  effect is `ConditionalIfSourcePowerAtLeast { threshold: 4, effect: DestroyUpToOneTargetNonlandPermanent }`,
  which the Python engine checks against the source as the ability resolves.
* Should be: (1) the triggers fire only when the creature dies (CR 700.4) or is put into exile from
  the battlefield, not when it is returned to a hand or put into a library ("leaves the battlefield"
  is wider); (2) "while its power is 4 or greater" is part of the trigger condition, read as the
  creature last existed on the battlefield (CR 603.10a), not an "if" checked on resolution (a
  leave-the-battlefield ability looks back; the creature is gone by then).
* Source: `cards/parser/triggers.py`, the phrase-table entries "this creature dies or is put into
  exile" and "another creature you control dies or is put into exile" (~line 718, both mapped to
  `Event.LEAVES_BATTLEFIELD` "to match the static registry"), and `_INTERVENING_WHILE_EFFECT_WRAPPERS`
  (~line 1645) for the power condition.
* Fix: an event (or a destination predicate) for "dies or is put into exile"; the power condition as
  a trigger predicate on the leaving object (e.g. `Subject(HasPowerAtLeast(4))` on its last known
  characteristics).
* Engine: blocked, `ir_defect(dies-or-exiled widened to leaves the battlefield, docs/ir_defects.md
  80)` (`lower::IR_DEFECTS`). Without the entry the card would lower (the class
  `ConditionalIfSourcePowerAtLeast` aside) with triggers on bounces and library moves.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): both triggers carry `LeftBattlefieldTo(graveyard, exile)`; "while its power is 4 or greater" is `Subject(HasPowerAtLeast(4))` on the leave event (last known information); the effect is the plain `DestroyUpToOneTargetNonlandPermanent`. Supported.

**81. Dawnsire, Sunstar Dreadnought.** Oracle: "10+ | Whenever you attack, Dawnsire deals 100 damage
to up to one target creature or planeswalker."

* IR: `DealDamageToTargetCreature { amount: 100, optional: true, targets: [TargetSpec { count_min:
  0, filter: IsCreature }] }`.
* Should be: the target "creature or planeswalker" (`OneOf(IsCreature, IsPlaneswalker)`).
* Source: `cards/parser/effects.py`, `_DEAL_DAMAGE_UP_TO_ONE_TARGET_CREATURE` (~line 390): the
  "(?: or planeswalker)?" tail is accepted and dropped ("planeswalkers aren't modeled by the engine
  today").
* Fix: keep the planeswalker in the target spec.
* Engine: blocked, `ir_defect(planeswalker target dropped, docs/ir_defects.md 81)`. (The card also
  needs Station, lane A; and its 100 damage is above the lowering's 0..99 range for this class,
  `DealDamageToTargetCreature.amount(out of range 100)`, an engine limit to widen with the card.)
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: Dawnsire's effect is `DealDamage { amount: 100,
  targets: [TargetSpec { filter: ObjectOr(IsCreature, IsPlaneswalker), count_min: 0 }] }`; the
  lowering's bound for `DealDamage.amount` is widened to 100. Supported.

**82. Entropic Battlecruiser.** Oracle: "1+ | Whenever an opponent discards a card, they lose 3
life."

* IR: the triggered ability `event: discards_card, predicate: Controlled(by: opponent)` with effect
  `TargetPlayerLosesLife { amount: 3 }`, a targeted effect ("target player loses 3 life").
* Should be: the discarding player loses 3 life, not targeted (no target is chosen, so hexproof or
  shroud on a player doesn't stop it): as the cube's "that player loses N life" triggers,
  `BindEventPlayer { slot: player, inner: TargetPlayerLosesLife }`.
* Source: `cards/parser/effects.py`, `_THEY_LOSE_N_LIFE` (~line 1718): the Python engine pre-wires the
  discarding player as `targets[0]` (`fire_opponent_discards_triggers`).
* Fix: emit the `BindEventPlayer` wrapper for "they lose N life" on an event with a player subject.
* Engine: blocked, `ir_defect(the event's player written as a target, docs/ir_defects.md 82)`.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 5362f5ed: the effect is `BindEventPlayer { slot: player,
  inner: TargetPlayerLosesLife { amount: 3 } }`. Supported.

### P. Edge of Eternities, the cross-lane pass (ENGINE.md 99.8)

**100. Fell Gravship.** Oracle: "When this Spacecraft enters, mill three cards, then return a creature
or Spacecraft card from your graveyard to your hand."

* IR (mtg-bot main 2cb79f77): the enter trigger's effect is `Sequence [SelfMill { amount: 3 },
  ReturnCreatureFromYourGraveyardToHand { creature: true, or_spacecraft: true }]`. The class is a
  targeted one (`requires_target` unless `untargeted_all`, `min_n` = `max_n` = 1): the same node as
  "return target creature card from your graveyard to your hand" (Interceptor Mechan), with nothing
  that tells the two apart (`target_kind` is the Python target tuple's tag).
* Should be: "a creature or Spacecraft card" is not a target (CR 115.1d: only "target ..." is): the
  card is chosen as the ability resolves, after the three cards are milled. Read as a target, the
  card is chosen as the ability is put on the stack (CR 603.3d), so a card the mill puts into the
  graveyard can't be returned, and with no creature or Spacecraft card in the graveyard then, the
  ability has no legal target and is removed from the stack (CR 603.3d), so the three cards aren't
  milled either.
* Source: `cards/parser/effects.py`, `_RETURN_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_HAND` (~line 1911):
  its comment notes the article "a" ("Scryfall prints the choice as a non-target selection") and
  keeps the targeted class, relying on the Python engine's resolve-time choice.
* Fix: a non-targeted form of the class (for example a `chosen_on_resolution` flag, or the
  `ReturnFromGraveyard` node with a choice instead of targets), as "return a ... card" is elsewhere.
* Engine: blocked, `ir_defect(a card chosen on resolution written as a target, docs/ir_defects.md
  100)` (`lower::IR_DEFECTS`). Without the entry the card lowers (lanes A and C give it everything
  else) with the return targeted. Found by the cross-lane pass reading the card for its tests; it
  was in `SUPPORTED_EOE_CROSS` since lane A's merge (ENGINE.md 99.7) and is taken off.
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `ReturnChosenGraveyardCardToHand { card_filter: CardFilterAnyOf(creature, Spacecraft) }`, chosen as it resolves. Supported.

**101. Desculpting Blast.** Oracle: "Return target nonland permanent to its owner's hand. If it was
attacking, create a 1/1 colorless Drone artifact creature token with flying and \"This token can
block only creatures with flying.\""

* IR: `ReturnTargetCreatureToOwnersHand { allow_nonland_permanent: true, create_drone_if_was_attacking:
  true, targets: [nonland, not a player] }`: the token is a flag; the Python class creates
  `TOKEN_PRINTINGS["Drone"]` by name as it resolves.
* Should be: the token's `CardPrinting` inline (a 1/1 colorless Drone artifact creature token with
  flying and `can_only_block_creatures_with_flying`), as the set's other Drone makers (Pinnacle
  Emissary, Station Monitor) carry it in `CreateToken { token_printing }`. A Drone is not one of the
  rules' predefined tokens (CR 111.10), so the engine has no definition of its own to use (unlike the
  Lander, ENGINE.md 99.3, or entry 3's Map).
* Source: `cards/parser/effects.py` (the Desculpting Blast literal, ~line 4324) and
  `effects/generic/bounce.py::ReturnTargetCreatureToOwnersHand.resolve` (~line 177).
* Fix: a `token_printing` on the rider (or the rider as its own effect: "if it was attacking,
  `CreateToken`").
* Engine: blocked, `ir_defect(token printing by name only, docs/ir_defects.md 101)`. The rest of
  the card needs only `allow_nonland_permanent` (its target spec already says "nonland, not a
  player") and an "if it was attacking" check, both left with the card (ENGINE.md 99.8).
* Fixed in mtg-bot `feature/eoe-ir-fixes` 20bea9f4 (ENGINE.md 99.11): `ReturnTargetCreatureToOwnersHand.if_was_attacking` holds the Drone token's `CreateToken` (its printing inline). Supported.

### Q. Edge of Eternities, W4: exile, graveyard, copies and control (ENGINE.md 99.14)

**180. Sothera, the Supervoid.** Oracle: "Whenever a creature you control dies, each opponent
chooses a creature they control and exiles it. / At the beginning of your end step, if a player
controls no creatures, sacrifice Sothera, then put a creature card exiled with it onto the
battlefield under your control with two additional +1/+1 counters on it."

* IR: the end-step ability is `Sequence [ReanimateOneExiledWithSourceWithCounters { counter_amount:
  2 }, SacrificeSelfPreservingExileLink]`: the creature card is put onto the battlefield first,
  Sothera sacrificed after.
* Should be: the sacrifice first, then the return (the Oracle text's order; the cards exiled with
  Sothera stay linked to it after it leaves, CR 607.2a). The order is observable: the returned
  creature is on the battlefield as Sothera leaves (its "whenever a nonland permanent leaves the
  battlefield" or "whenever another creature dies"-style abilities see the sacrifice), and its
  enters abilities' intervening "if"s are checked before Sothera has left (void, CR 603.4: "if a
  nonland permanent left the battlefield this turn" may be false then and true in the Oracle
  order).
* Source: `effects/generic/exile.py::SacrificeSelfPreservingExileLink` (~line 1260; its docstring:
  a naive `Sequence(SacrificeSelf, recall)` loses the link in the Python engine, whose
  `_release_exiled_until_leaves` hook returns the cards as Sothera leaves, so "the engine-level fix
  is to invert the order at composition time") and the parser line that composes Sothera's ability.
* Fix: `Sequence [SacrificeSelf, ReanimateOneExiledWithSourceWithCounters]`, the link kept by the
  Python engine for a source that has left (Sothera's exile is not "until this leaves", CR 610.3).
* Engine: blocked, `ir_defect(sacrifice after the return, docs/ir_defects.md 180)`. Its classes
  `EachOpponentExilesOwnCreature`, `ReanimateOneExiledWithSourceWithCounters`,
  `SacrificeSelfPreservingExileLink` and the predicate `AnyPlayerControlsNoCreatures` are not
  lowered yet either (ENGINE.md 99.14).

### R. Edge of Eternities W2, mana and lands (ENGINE.md 99.12)

**160. Plaza of Heroes.** Oracle: "{T}: Add {C}. / {T}: Add one mana of any color. Spend this mana
only to cast a legendary spell. / {T}: Add one mana of any color among legendary permanents you
control. / {3}, {T}, Exile this land: Target legendary creature gains hexproof and indestructible
until end of turn."

* IR (mtg-bot main 2cb79f77): the third ability is `AddManaToPool { produces: [["any", 1]],
  spend_restriction: "legendary_permanent_ability" }`: one mana of any colour that may be spent
  only on abilities of legendary permanents.
* Should be: one mana of a colour among the legendary permanents its controller controls, spent on
  anything (no legendary permanent of a colour: none of that colour; none at all: no mana). The IR
  already has the class: `AddOneManaOfColorAmongControlled { card_types: [], legendary: true }`
  (lowered as Mox Amber's).
* Source: `cards/parser/effects.py`, `_ADD_ANY_MANA_AMONG_GROUP` and `_ADD_MANA_AMONG_RESTRICTION`
  (~line 1670): the phrase "add one mana of any color among <group> you control" is mapped to a
  spending restriction ("legendary permanents" -> `legendary_permanent_ability`).
* Fix: map the phrase to `AddOneManaOfColorAmongControlled` with the group's types and supertype.
* Engine: blocked, `ir_defect(a mana ability's colours written as a spending restriction,
  docs/ir_defects.md 160)` (`lower::IR_DEFECTS`); the restriction itself stays denied
  (`AddManaToPool.spend_restriction(LegendaryPermanentAbility)`: no Oracle text says it). Everything
  else of the card is in (the legendary-spell restriction, the colours among legendary permanents,
  the protection): tested on a corrected copy (`cards_eoe_w2_lands.rs`, "Test Plaza").

**161. Contested War Zone.** Oracle: "Whenever a creature deals combat damage to you, that
creature's controller gains control of this land."

* IR: the trigger is `event: deals_combat_damage` with `predicate: And [Subject(IsCreature, slot:
  source), Controlled(subject: target)]`: the damaged object or player is controlled by (is) the
  War Zone's controller.
* Should be: the damaged one is the War Zone's controller, a player: `Subject(IsPlayer, slot:
  target)` as well (as Starwinder's and Elegy Acolyte's "deals combat damage to a player"). As
  written it also triggers when a creature deals combat damage to a creature or planeswalker its
  controller controls (a blocker), and the land changes control.
* Source: `cards/parser/land_builder.py`, the Contested War Zone builder (~line 2897:
  `IsCreature(source) & Controlled(YOU, subject="target")`).
* Fix: add `Subject(IsPlayer, slot: target)` to the predicate.
* Engine: blocked, `ir_defect(combat damage to you widened to your permanents, docs/ir_defects.md
  161)`. The rest is in (`EoeW2::EventObjControllerGainsSource`; "attacking creatures get +1/+0" as
  the class's docstring says, every attacking creature): tested on a corrected copy ("Test War
  Zone").

### S. Python engine switches the IR carries (the faithfulness audit's follow-ups, ENGINE.md 100.6)

Two effect classes carry a field that turns a Python engine shortcut on, and its default is on. Read
literally, the IR then says something the Oracle text doesn't. The engine ignores both fields and
plays the Oracle text, so both cards are supported; the entries are here so that no consumer reads
the fields as rules.

**190. Palantír of Orthanc** (the audit's N17). Oracle: "At the beginning of your end step, put an
influence counter on Palantír of Orthanc and scry 2. ..."

* IR: the end-step ability's `Sequence` holds `Scry { amount: 2 }` with `bottom_choice` omitted, so
  its default `false`: the class's "first-slice deterministic shortcut: keep all on top" (its
  docstring), which mirrors the Rust bridge's `ScryN` arm. Preordain's scry has
  `bottom_choice: true`. These are the only two scry nodes in the cube and Edge of Eternities IR.
* Should be: the scry of CR 701.18a (any number of the cards on the bottom in any order, the rest on
  top in any order): `bottom_choice: true`, or no such field (a scry always lets the player choose).
* Source: `cards/parser/artifact_effects.py::_parse_palantir_body` (~line 598) builds
  `Scry(amount=...)` without the `bottom_choice=True` that `cards/parser/card_flow_grammar.py` and
  `cards/parser/keyword_actions.py` pass; the class `effects/generic/library_lookat.py::Scry` defaults
  it to `False`.
* Fix: pass `bottom_choice=True` in `_parse_palantir_body`, or make `True` the default.
* Engine: supported. The lowering ignores the field and lowers every scry as 701.18a
  (`engine/src/lower/effect.rs`, the `Scry` arm; ENGINE.md 14). The audit classes this as faithful
  (class 1).

**191. Chain of Smog** (docs/shortcuts.md 26.11.9). Oracle: "Target player discards two cards. That
player may copy this spell and may choose a new target for that copy."

* IR: `Sequence [TargetPlayerDiscards { amount: 2 }, TargetPlayerMayCopyThisSpell]`, the second with
  `needs_target_with_cards_in_hand` omitted, so its default `true`: the Python engine offers the
  copy only while a player the copy could target has cards in hand (a guard against endless
  copying).
* Should be: no condition. The Oracle text lets the player copy with any hands; a copy with empty
  hands still triggers "whenever you cast or copy" abilities (Witherbloom Apprentice).
* Source: `effects/generic/spell_copies.py::TargetPlayerMayCopyThisSpell` (the field and its
  default, ~line 95; read at ~line 108).
* Fix: default the field to `false` (or drop it) and bound the loop in the Python engine, as CR
  732.2a-732.3 do.
* Engine: supported. The lowering ignores the field (`engine/src/lower/effect_copies.rs`, the
  `TargetPlayerMayCopyThisSpell` arm) and always offers the copy, with at most 31 such copies a turn
  (`copies.rs::MAY_COPY_CAP`, docs/shortcuts.md 26.11.7; ENGINE.md 26.11).

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
  (ENGINE.md 14, 26.11). Listed as a defect since 2026-11-08: entry 191. Lutri's companion condition isn't in the IR; it works outside the game
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
