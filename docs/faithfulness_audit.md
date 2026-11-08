# Faithfulness audit of the rules engine (branch `eoe-f0-audit`, 2026-11-08)

The user's rule (2026-11-08): **"We can't have any approximations. Need fully faithful
implementation."** The engine is the ground truth that self-play trains on, so every place where
it differs from the Comprehensive Rules (CR) or the Oracle text has to go. This document is the
audit and the fix design only. It changes no engine code. Other agents are editing the engine in
parallel (EoE W2 `eoe-w2-mana`, W3 `eoe-w3-combat`, W5 `eoe-w5-tannuk`), so the fixes come after
those merges.

* **Base.** `eoe-base` 7cc45b2: main plus the Edge of Eternities port so far (W1 and W4 merged).
  The three open EoE branches were read too (`git show <branch>:...`). Their new admitted
  divergences are listed as rows (section 4), and their fixes are taken as given where they close
  a row (W3's ability page, section 5.1).
* **Sources.** `docs/shortcuts.md`, every row; `docs/ENGINE.md` 10-11, 58, 60 and 99 (99.12, 99.13
  and 99.15 from their branches); the code each row points at. There was also a sweep of
  `engine/src` for comments that admit a limit, and for `.take(`, `.min(` and `NUMBER_N - 1` clamps
  (section 4).

## 0. Classes, and what counts as an argument

Each row gets exactly one class. A row with several parts gets the class of its worst part, and
its parts are named.

1. **Faithful by the rules in every reachable state.** A representation or an optimisation that
   can never change an outcome or remove a legal option. The argument is a rules argument
   ("every search shuffles, and identical library cards differ in nothing a shuffle keeps"). An
   argument about the card pool ("no cube card does X") does not count.
2. **A bound no legal game can reach.** It is proved from the formats the engine plays (40-card
   limited decks, 60-card constructed decks, the 60-turn cap), or from a **loud refusal**: the
   lowering denies every card that would need the missing feature, so no game the engine accepts
   reaches it (AGENTS.md: "cards are data, with no silent drops"). Refusals are listed so a
   future port knows where they are. They are not approximations.
3. **Must be fixed.** An approximation; a truncation (a legal option not offered, a value
   clamped); an equivalence that holds only for the current card pool; a forced order that a
   player could choose differently with a different outcome. A "dominated option left out" also
   lands here unless the two results are identical. The CR gives no way to turn the better
   resource into the worse one, so card text can tell them apart (13.1k, 39.1).

"Loud" limits (an assert or a panic) are class 3 when a legal game can reach them. They never
play a game wrongly, but they end a legal game the engine can't finish.

## 1. Summary

**Counts.** There are 84 rows in docs/shortcuts.md that are not fully fixed: 79 not marked fixed,
plus the kept parts of five fixed rows (13.1, 13.2, 24.7.5, 28.8.1 and 28.8.2's kept part):

| Class | Rows |
|---|---:|
| 1. Faithful | 33 |
| 2. Unreachable (format bound or loud refusal) | 3 |
| 3. Must be fixed | 47 |
| Not a rule (24.7.6, the heuristic bot) | 1 |

There are also 4 known items outside the log (N1-N4), 10 class-3 rows from the comment sweep
(N5-N15, without N13), 1 class-2 row (N13), 3 notes (N16 is not a rule; N17 and N18 are class 1), 5 admitted divergences
on the open EoE branches (N19-N23), and 17 public-state gaps (N2).

**Must fix, in priority order** (one line each; section 5 has the designs):

1. **58.1**: stack targets beyond the 31 nearest items. The Delver mirror reached a stack of 39
   (ENGINE.md 56.11). Fix: page the `STACK` range.
2. **13.12, 13.2k, 13.4, 13.13, 13.14**: mana abilities are not offered at every priority (CR
   117.1d, 605.3a). Example: Balance with floating mana. Fix: one priority action that opens a
   source loop; payments may activate extra sources.
3. **13.9, 23.11.3, N5, N9**: numbers are clamped to `NUMBER` 0..31. This covers X, kicker X,
   "pay X life", X targets, squad, replicate, multikicker, divided damage, the energy amount and
   combat damage of 32 or more. Fix: a big-number decision (exact up to 31, then base-32 digits).
4. **N1 (W1)**: a permanent sits "exiled for now" during a 616.1 replacement choice, so zone
   watchers see exile. Fix: hold it in no zone, with its leave events deferred until the choice.
5. **N4**: granted graveyard casting abilities are judged by the front face, against Lier's
   2025-10-02 ruling. Example: Wrenn and Six's emblem with Brazen Borrower or Embereth
   Shieldbreaker. Fix: judge them by the face being cast.
6. **29.6.5**: the miracle reveal is asked after the whole draw instruction. With Brainstorm the
   player sees the later cards first. Fix: ask at the draw.
7. **13.8 and 31.7.8**: waiting triggers of one definition from different objects are merged,
   and more than 16 distinct triggers cut the order. Fix: compare the source state, and page the
   `TRIGGER` range.
8. **N15**: the timestamps of objects entering at the same time are never chosen by the active
   player (CR 613.7m). Fix: an ordering decision when the order can matter.
9. **Payment model (13.3, 13.15, 13.16, 13.17, 27.13.4, N8, N10, N11, 28.8.2k, 13.1k, N19-N21)**:
   sources excluded too broadly, need matching, Black Lotus with delve, Lumberjack with teamwork,
   creatures with mana abilities as tap costs, colours of mana spent, `PayPool` cut at 32, and
   restrictions on floated mana.
10. **28.8.4, 26.11.3, 26.11.5, 31.7.7k, N22, 13.10 (battles)**: characteristics. Painter's
    Servant against a "becomes a colour" effect (Celestial Colonnade), a copying DFC that won't
    transform, copy exceptions dropped in release, a copy of a Class, the basic land type's mana
    ability, and battle cards that are not denied.
11. **32.5.2, 29.6.4, 39.1, N6**: timing and replacements. "This turn" triggers that outlive
    514.2, an additional combat phase that does nothing outside combat, Bilbo against an
    Adventure, and Hullbreacher against Quantum Riddler.
12. **Slot ranges (58.4, 99.14.1, 27.13.5, 65.10, 24.7.4, 26.11.6, 25.10.5, 99.14.3, 31.7.2k,
    31.7.5, N12, N14)**: the `EXILE` 32, `GY_ABILITY` 12, graveyard list 64, 96 permanents, 64
    tokens a creation, 4 `ACTIVATE` slots, Anointed Peacekeeper's names, 16 counter kinds. Fix:
    paging everywhere (W3's page covers 25.10.5 and 99.14.3).
13. **24.7.5k, 40.1**: retrace together with another hand-card or permanent cost. Fix: costs that
    reserve several classes.
14. **N3 and 99.14.2**: The Dominion Bracelet is blocked on the encoding, and the controller
    under-shares what it sees. Fix: an encoding-only frame flip within layout v5.
15. **Numbers and loops (46.1-46.6, 29.6.6, 26.11.7)**: saturating counters, P/T, life, mana and
    loop counts, and the 31-a-turn activation and copy caps. Grim Monolith-style mana loops and
    infinite storm loops reach these caps. Fix: exact 64-bit arithmetic or a loud abort, plus
    CR 732.2a loop shortcuts.

**Workstreams** (section 6). There is a shared A0 commit first, with the paging and number
helpers and two context floats. Then four workstreams run on mostly disjoint files:

* **A, choice ranges** (F1, F2): `decision.rs`, `combat.rs`, the range code in `cast.rs`,
  `triggers.rs` (paging), the exile, graveyard and name choices.
* **B, mana and payment** (F3, F4): `pay.rs`, `mana.rs`, the mana parts of `activate.rs`,
  `eoe_w2.rs`, and the parts of `db.rs` that merge costs.
* **C, rules timing and characteristics** (F5, F6): `game.rs` (moves, replacement effects,
  draws, timestamps), `flow.rs`, `sba.rs`, `layers.rs`, `faces.rs`, the cost-free parts of
  `copies.rs`, `resolve_m7d.rs`, `lower/printing.rs`.
* **D, encoding** (F8): `az/encode.rs`, `public_state.rs`, `eoe_w4.rs`, layout v6.

F7 (numbers and loops) comes last. It needs A's number decision and a small zero-core opt-in.

**Layout.** Almost everything fits inside layout v5. Paging and digit decisions reuse the
existing ranges. Their page and digit context goes in v5's spare context floats 246-255. The
Bracelet's frame flip is encoding-only. The scalar gaps fit v5's spare floats: 8 permanent status
floats, 14 game globals and 8 stack floats.

**Only three facts need layout v6.** Each is an identity, which no spare float can hold: the
exiled cards a stack item targets or noted, and each player's phased-out permanents. That is 3
bag rows. v6 is an exact widen like v4 -> v5 (`widen` already supports added bag rows, ENGINE.md
60.2). It should also add headroom, because filling every v5 spare leaves the next port nothing
(section 5.8).

## 2. Every row of docs/shortcuts.md

"k" after an id means the kept part of a row otherwise marked fixed. "F*n*" points to the fix
design in section 5.

| Id | Class | Argument, or the divergence | Fix |
|---|---|---|---|
| 13.1k | 3 | The pool's spend prunes remainders on a "never worse" claim: artifact-only {C} is spent before other {C}, and haste {R} after other {R} on a noncreature spell. The kept pool differs (a tag), and the CR gives no way to turn plain {C} back into the restricted kind, so card text could tell them apart. "Two units of one type and tag are one choice" is faithful: the units are identical. Low priority. | F4 (`PayPool` offers every distinct remainder) |
| 13.2k | 3 | Mana floated at priority (the kept part). See 13.12. | F3 |
| 13.3 | 3 | `activate.rs::mana_exclude` removes **all** mana abilities of a cost's source when the cost taps, sacrifices, exiles or discards it. CR 601.2g-h allows a mana ability whose cost doesn't conflict: tap the source for mana and then sacrifice it for "{1}, Sacrifice this", or remove a charge counter for mana and then tap it for "{1}, {T}". "No supported card has such a pair" is a pool argument. | F4 (exclusion per ability, by conflict) |
| 13.4 | 3 | A Spirit Guide's mana ability is not offered at priority. Exiling it changes the hand's size (hellbent, Ensnaring Bridge counts), and its mana empties at the step's end. CR 117.1d and 605.3a allow it at any priority. | F3 |
| 13.5 | 1 | `Op::Search` always shuffles by construction (`program.rs`: "then shuffles"; `resolve.rs` calls `shuffle_library`). `EoeX::SearchUpTo...` shuffles in `finish_search_up_to`. Library cards of one definition differ only in position and knowledge, and a shuffle erases both (701.20: what was known becomes a cloud over the library, and a cloud of identity d is unchanged by which d left). A future non-shuffling search must name cards; it would be a new op, which the lowering doesn't have. | none |
| 13.6 | 1 | Modes are chosen as a set, or as a multiset where repeats are allowed (700.2). The spell performs them in printed order whatever order they were picked in (700.2a, 608.2c). The instances of a repeated mode are interchangeable, each with its own targets, so each multiset is reachable once and loses nothing. | none |
| 13.7 | 1 | `trig_mask` per definition: an event is matched only against observers whose definition has a trigger of that kind (`triggers.rs::event`: battlefield, command zone, graveyards with `gy_triggers`, the leaving look-back, the card itself, the cast spell). An event no ability can match has no effect. The `SEEN_*` bits come from every card in the game, so a card that arrives later is covered. | none |
| 13.8 | 3 | The code's test (`put_triggers::same`) is the same source definition and slot, and the same event or a program that ignores the event. It **never compares the source objects**. Two triggers from different objects of one definition whose effects read the source's own state (its power, counters or attachments as it last existed) are merged, and their order is forced (603.3b gives it to the controller). The row's "their effects ignore the source's per-object state" is not checked. | F5 (`uses_source` beside `uses_event`) |
| 13.9 | 3 | Combat damage assignment with 32 or more to divide offers 0..29, "lethal" (30) and "all the rest" (31) (`combat.rs::assign_damage`). The other splits are missing. They matter for damage triggers that read the amount (Pain for All, EoE W3: "it deals that much damage"), for damage marked on an indestructible blocker, and for how much tramples over. CR 510.1c-d allows any split. | F2 |
| 13.10 | 3 | **Battles:** `types::from_name("Battle")` lowers, but there are no defense counters, no protector and no attack on a battle (CR 310). A battle card in a table would be played wrongly and silently. Not a refusal. **Starting player:** fixed. **Open decklists in `determinize`:** class 1. This is the information model of the format played (best-of-one with open decklists); best-of-three hides decks (`az/sample.rs`, ENGINE.md 36.11). | F6 (deny battle cards until CR 310 is built) |
| 13.11 | 1 | More options than needed (identical permanents' targets are not merged); nothing is removed. | none |
| 23.11.2 | 1 | `UntapUpToNPermanents` and `UntapChosenPermanents` are not targeted (`lower/effect_ext.rs` 645, `lower/effect_stack.rs` 212). Untapping an untapped permanent does nothing and makes no event (701.21), so leaving it out equals choosing fewer. Targeted untaps (`Op::Untap`) keep untapped candidates. | none |
| 23.11.3 | 3 | Divided combat damage above 31: as 13.9. | F2 |
| 23.11.8 | 1 | A graveyard card's castability is checked on a clone with the card on the stack. That is the order of 601.2a-i (the characteristics as a spell); the clone is an implementation of the same check. | none |
| 23.11.9 | 1 | True-Name Nemesis: choosing yourself is offered, so nothing is removed. (It is not dominated, either: protection from yourself saves it from your own Fiery Confluence.) | none |
| 23.11.10 | 1 | Representation only: the same choices under other decision kinds. | none |
| 24.7.1 | 1 | `AttackTarget` is asked right after each attacker. CR 508.1 checks the whole declaration. The engine's restrictions and requirements (`can_attack`, `must_attack`, goad, `must_attack_player`, `combat.rs` 46-230) don't depend on attack targets, and there are no attack costs. Goad and "must attack a player" are settled before the question. So the sequential choice reaches exactly the legal declarations. A restriction or cost that depends on the target (Propaganda-style) would be a new `Mod` that the lowering doesn't have. | none |
| 24.7.2 | 1 | Naming a card (Demonic Consultation, Tamiyo) offers the library's names and one "other name". The named name has no rules effect beyond matching revealed library cards, so names outside the library give identical results. Which outside name was spoken is only public cheap talk; in a two-player zero-sum game it can't change any outcome. | none |
| 24.7.3 | 1 | A triggered mana ability resolves at once (605.4a), so it joins the mana ability's production. | none |
| 24.7.4 | 3 | At most 64 tokens from one creation after doubling (`game.rs::token_count`). CR 111 has no maximum, and token doublers make it reachable. | F1 (permanent paging, no token cap) |
| 24.7.5k | 3 | Retrace with a card's own discard cost isn't offered: two discards can't merge into one cost. CR 601.2b, f-h make them one cost. "None in the cube" is a pool argument. | F4 (costs) |
| 24.7.6 | n/a | Bot policy, not a rule. | none |
| 25.10.1 | 1 | `face_castable` evaluates a face on a clone: an implementation of 601.2a. | none |
| 25.10.2 | 1 | Both kinds are built. Not a shortcut. | none |
| 25.10.3 | 1 | Mayhem is tied to the graveyard object discarded this turn. That is 400.7 (a new object). | none |
| 25.10.4 | 2 | Meld, flip, convert, "cast transformed", read ahead and rooms are not built. `room_halves` and the other IR fields of these mechanisms deny, so they are loud refusals. Split cards are lowered generally (`lower/printing.rs` 1605). Workstream C must confirm that each unbuilt mechanism denies (13.10 shows battles don't). | F6 (deny audit) |
| 25.10.5 | 3 | At most four activated abilities per permanent. Printed abilities past four are denied (`lower/printing.rs` 1276, a refusal), but granted abilities (Equipment, 99.14.3; Ragost on W3) exceed four at run time and are cut. **Fixed on `eoe-w3-combat`** (706ad43: the fourth slot opens a page). | F1 (merge W3) |
| 26.11.1 | 1 | A copy of a spell leaving the stack goes to no zone at once (`game.rs` 1946). It isn't a card (707.10), so no "card" query can see it in a graveyard before the next state-based check (704.5e). No supported trigger predicate matches a non-card object entering a graveyard from the stack (only `ToGraveyard` on the card itself). A predicate that did would be new IR. | none |
| 26.11.2k | 1 | The copy choice is made right after the move with the `Enters` event held back. Nothing happens in between, and 614.12a's group check keeps the choices legal. | none |
| 26.11.3 | 3 | A double-faced permanent that is copying something doesn't transform. 701.28c stops only permanents that aren't transforming DFCs. A DFC copying something transforms. While the copy effect lasts, the effect still sets its characteristics (layer 1, 707.2). When the effect ends, the permanent shows the face that is now up. Reachable: Saheeli, Sublime Artificer's -2 on a daybound permanent across a day/night change. | F6 |
| 26.11.4 | 2 | "Another target" with more than one target per requirement: `lower/effect_copies.rs::two_targets` denies any count but one, and other `distinct_from` uses deny (`lower/effect.rs` 95, 342). A loud refusal. | none |
| 26.11.5 | 3 | Copy exceptions hold four subtypes (`copies.rs::Except::subtypes`). Printed exceptions are checked at lowering, but a copy of a copy merges two lists (`Except::over`). In release, a fifth subtype is **silently dropped** (`debug_assert!` only). Copy chains have no depth limit. | F6 |
| 26.11.6 | 3 | 96 permanents a player (`perm_room`: control isn't gained, tokens and copies cease to exist). Tokens are unbounded by deck size. | F1 |
| 26.11.7 | 3 | At most 31 copies a turn from "may copy this spell" (`MAY_COPY_CAP`). CR 732.2a lets the player choose any number of iterations of an optional loop. More than 31 matters whenever each copy does something. | F7 |
| 26.11.9 | 1 | The engine follows the Oracle text (no hand condition); the IR flag is wrong (an IR defect). | none |
| 27.13.3 | 1 | White Plume Adventurer's "untap a creature you control" is not targeted. As 23.11.2. | none |
| 27.13.4 | 3 | "Tap n untapped X you control" counts only candidates without mana abilities when mana is also owed (`activate.rs` 562-569). The player may tap a creature with a mana ability for the cost and pay the mana with other sources (601.2g-h), so the activation is wrongly refused. "No Rebel in the cube has one" is a pool argument, and the code applies to every such cost. | F4 (W3's reservation, `Keep::sac_taps`) |
| 27.13.5 | 3 | One graveyard ability per card (refused at lowering: class 2). `GY_ABILITY` names the first 12 cards with one, so the 13th and later can't be activated while 12 earlier ones are there (`activate.rs` 201-213). A 40-card deck can put 13 such cards in its graveyard. | F1 |
| 27.13.6 | 1 | The initiative ventures into Undercity (725.2: "venture into Undercity"). "Venture into the dungeon" has no lowering arm, so a card that has it is unsupported, which is a refusal. | none |
| 27.13.7 | 1 | The initiative's abilities have no source and are controlled by the player named (725.2), as built. | none |
| 28.8.1k | 1 | Lion's Eye Diamond is with priority only. That is its ruling ("activate only as an instant": not while casting a spell). Chromatic Star and Wizard's Rockets are offered both with priority and while paying. | none |
| 28.8.2k | 3 | Converge counts the most colours each dual source can show, and single-mana sources "count as their best colours". The player can't choose to spend fewer colours, which matters when more colours hurt (a converge sweeper like Radiant Flames, or "if {R} was spent" cards). "More is never worse for Prismatic Ending" is a pool argument. Also `pay.rs` 1978: hybrid or Phyrexian pips with many units give "an upper bound". | F4 (`ChooseColorsSpent`) |
| 28.8.3 | 1 | Lim-Dul's Vault: the order on the bottom is lost to the final shuffle (701.20). The cards put on top are ordered by the player. | none |
| 28.8.4 | 3 | Painter's Servant's colour is applied before every `Mod::Colors` (`layers.rs` 323). Layer 5 applies effects in timestamp order (613.7). A "becomes [colours]" effect with an earlier timestamp than Painter's must be added to, not overwrite it. Reachable in the cube: animate Celestial Colonnade ("becomes a white and blue Elemental"), then cast Painter's Servant in the second main phase. `Mod::Colors` is lowered (`lower/effect_ext.rs` 498, `lower/effect_walkers.rs` 408), so the comment's "none of those exists" is wrong. | F6 |
| 28.8.7 | 1 | Goblin Welder against a shielded artifact: the impossible part is ignored (101.3). The return still happens. | none |
| 29.6.4 | 3 | "After this phase, there is an additional combat phase" does nothing unless it resolves during a combat phase (`resolve_tail2.rs` 153). CR 500.8 adds the phase after whatever phase it is. The IR class `AdditionalCombatPhaseAfterThisPhase` isn't limited to combat. | F5 |
| 29.6.5 | 3 | The miracle reveal (702.94a: "as you draw it") is asked at the next state-based check (`sba.rs` 239), after the whole draw instruction. With Brainstorm (cube) drawing a first card of the turn, the player has seen the other two cards, and has put two back, before deciding. That is information the rules don't give. The cube's miracle card is Metamorphosis Fanatic. | F5 |
| 29.6.6 | 3 | At most 31 activations of one permanent's non-mana ability a turn (`ACTIVATION_CAP`). 732.2a allows any number. Mana loops are limited by it, and a value of X is limited by 13.9 as well. | F7 |
| 29.6.7 | 1 | An extra turn is a turn (500.7). | none |
| 30.7.5 | 1 | Shelldock Isle's land play follows 305.2-305.3. | none |
| 30.7.6 | 1 | As the CR (611.2b, the controller's turns). | none |
| 30.7.7 | 1 | Splice: the card being cast is on the stack, not in the hand (702.47a). Each copy in hand is a card of its own. Each spliced text's targets are requirements of their own (115.3: one per instance of "target"). A "distinct targets" requirement can't be expressed: `distinct_from` is denied outside `two_targets`, which is a refusal. | none |
| 30.7.8 | 1 | `sba.rs::bad_reanimating_aura` checks the Oracle's enchant restriction exactly: a creature card in a graveyard, or the creature it reanimated that is still a creature, plus protection. | none |
| 31.7.2k | 3 | A permanent's sacrifice sources (Rain of Filth, Squandered Resources) are `ACTIVATE` slots after its own abilities, and none is offered past the fourth. W3's page covers priority, not `PayMana`. | F1 (a "which source of this permanent" follow-up, as W2's `PayMana::which`) |
| 31.7.3 | 1 | Emerge's permanent is chosen right after the alternative cost (601.2b) and only noted. It is sacrificed after the mana (601.2h), so targets (601.2c) see the same game either way. The two choices are independent, so the set of reachable pairs is the same in either order. | none |
| 31.7.4k | 1 | Bolas's Citadel: "you may look at the top card of your library any time" is the same as knowing it before every decision. | none |
| 31.7.5 | 3 | Anointed Peacekeeper names among the opponent's cards (`resolve_m7d.rs::choose_name_as_enters`), then `take(64)`. The second sentence ("activated abilities of sources with the chosen name cost {2} more") taxes **every** source, so naming one of your own cards is a different, legal choice. It taxes your own abilities and the opponent's if they steal that permanent (Dack Fayden). "Dominated" is a pool argument. With hidden decks, the known pool (84 cards in sealed) can exceed 64 names. | F1 (all cards' names, paged) |
| 31.7.6 | 1 | **Fixed since ENGINE.md 71.2**, so the row is stale. `Mark::Triggered` keys the count by the granting object (`src: ObjRef`): a new Nadu object is a new ability (400.7, ruling 96e9a886). | doc: update the row |
| 31.7.7k | 3 | A copy of a Class gains no levels (`resolve_m7d.rs` 350). The level is not one of the copiable values (707.2), so a copy starts at level 1. Its copied level abilities can be activated and raise its own level (716). "No such copy in the cube" is a pool argument. Morph, disguise and cloak are class 2 if their IR fields deny (part of the deny audit). | F6 |
| 31.7.8 | 3 | More than 16 distinct waiting triggers: only the first 16 are offered, so the 17th can never be put on the stack first (`triggers.rs` 499). The order is the controller's (603.3b). A wrath on 17 different creatures with dies triggers reaches it. | F1 |
| 32.5.1 | 1 | Representation only (one `ChooseMana` per mana). Its X is clamped like any X (N5). | F2 for X |
| 32.5.2 | 3 | "This turn" triggered abilities (`M7d::TriggerThisTurn*`) are removed as the next turn begins (`flow.rs` 467). CR 514.2 ends "this turn" effects in the cleanup step. A creature that dies to a state-based check right after 514.2 still triggers them: an until-end-of-turn +X/+X ending on a creature with -1/-1 counters. Lane D's own grant is removed at cleanup (`layers.rs` 795), so the two are inconsistent. | F5 |
| 13.12 | 3 | With priority, mana abilities are offered only for an untapped source an object on the stack targets (86). Floating otherwise isn't possible, though 117.1d allows it. Reachable in the cube: tap every land, cast Balance, and cast a spell from the floating mana after the lands are sacrificed. Also before a non-targeting wipe, or with painlands to lower your own life at instant speed. | F3 |
| 13.13 | 3 | A payment spends the most pool mana it can, so "keep the pool's {R} for a later Lightning Bolt and tap a land for the generic" is left out (its own row). | F3 (payments may add sources) |
| 13.14 | 3 | Wizard's Rockets is offered in a payment only when nothing else pays. Using it to keep a land untapped (a counterspell held up) is a real choice. | F3 |
| 13.15 | 3 | `MADE` and `NOT_MADE` are one bit each, so the mana of a mana ability with a mana cost can't pay another such ability's cost in one payment (Loot's mana for Arena of Glory's {R}). | F4 (exact needs) |
| 13.16 | 3 | Black Lotus sacrificed for mana is in the graveyard before 601.2h, so it can be one of delve's or an escape's cards in the payment it helps. The engine forces single-mana sources only. The order part ("delve's commitments come before its graveyard cards") is class 1, because nothing happens between them. | F4 |
| 13.17 | 3 | Teamwork tapping a creature while an Orcish Lumberjack is a source: the Lumberjack's sacrifices are not offered in that payment. | F4 |
| 38.1 | 1 | Containment Priest and a card entering as a copy, without casting. The Priest-first order (exiled) is reached by the copy-first order by copying a creature or nothing ("you may"), with the identical result: the card in exile, nothing entered. Copying a noncreature artifact is the copy-first order's only other result. The two orders reach the same set of states. | none |
| 39.1 | 3 | Bilbo, Thief in the Night against an Adventure (616.1). The order that exiles without the Adventure's permission is not offered. "Dominated (the permission need not be used)" assumes nothing reads the permission. The states differ, and no action turns one into the other. Low priority. | F5 (a `ChooseReplacement` code) |
| 40.1 | 3 | Retrace with an alternative cost whose rest takes a hand card, returns a permanent or sacrifices one is not offered (`merge_costs` reserves one class). It is reachable (Six's retrace on an evoke creature, per the row). | F4 (costs) |
| 46.1 | 3 | Counters saturate at 2^32 - 1, which the fuzz reaches (Bristly Bill). Saturation changes outcomes when two huge values are compared (a fight, combat, life). | F7 |
| 46.2 | 3 | P/T, life and amounts saturate at +-2^31 - 1. As 46.1. | F7 |
| 46.3 | 3 | At most 255 mana of one type in a pool or payment (`u8`, saturating). | F7 |
| 46.4 | 3 | A frame's `u8` loop counts: 255 copies, a connive of 255, 60 cards looked at (below a library's 64), 254 mana from Coalition Relic. Counters grow without bound (doublings, proliferate), and storm counts grow with loops (Underworld Breach, Brain Freeze). | F2 (storage) |
| 46.5 | 3 | Spells cast and cards drawn a turn saturate at 255. Breach loops and other recursion pass 255 with no bound from the deck's size. | F2 (storage) |
| 46.6 | 3 | **Turn numbers** (`u16`, the cap is 60, 180 a match): class 2, the format's turn cap. **Objects:** 65,534 slots, and tokens and copies take new ones, ending in a panic (loud). A legal game with loops of tokens or copies can pass it. | F7 |
| 58.1 | 3 | A target on the stack must be among the 31 items below (`stack_in_reach`). Reached in self-play: a stack of 39 (56.11). | F1 |
| 58.3 | 2 | At most 64 cards a player. This is a format bound: every deck the engine plays is 40 (limited, drafted, random, evaluation) or 60 (constructed), with at most one companion. `check_size` refuses larger lists loudly. A hand, library or graveyard holds only its owner's cards (400.3). There are no Wish effects (cards from outside the game) beyond a revealed companion. | none |
| 58.4 | 3 | At priority and in effect choices, the first 32 exiled cards the player may play or choose are offered (`take(EXILE_N)`, 6 sites). Impulse draws pile up past 32. | F1 |
| 99.14.1 | 3 | A target in exile is among the first 32 offered. As 58.4. | F1 |
| 99.14.2 | 3 | During a controlled turn the controller is not shown a look that names no decision ("look at the top card"), or the searched library's contents. CR 722.3 lets the controller see everything the controlled player can see during that turn. Under-sharing is information the rules give and the decider lacks. | F8 (`sync_controller_view` mirrors every private reveal to the controlled player) |
| 99.14.3 | 3 | Equipment-granted activated abilities past the fourth `ACTIVATE` slot are not offered. **Fixed on W3** (its page includes W4's grants, `permanent_activations`). | F1 (merge W3) |
| 65.10 | 3 | A graveyard choice over both graveyards with more than 64 cards offered fails an assert (loud). Lion Sash with 33 + 32 cards in the two graveyards reaches it in a long or milling game. | F1 (page the list block) |

## 3. Known items not in the log

### N1 (W1, ENGINE.md 99.11): "exiled for now" during a 616.1 choice. Class 3.

`game.rs::move_card_ctl` (1891-1900) handles `Dest::Choose(results, k)` for a card whose
replacement orders give different results (library, exile, exile with a void counter). It moves
the card to **exile** right away, and `Task::ChooseReplacement` asks its controller (or owner)
next. So, while the choice is pending and as the move's events are matched:

* `leaving_to` reads exile, so `TrigPred::LeftTo` ("dies or is put into exile", Syr Vondam)
  triggers even if the library is chosen next.
* Anything that counts exile sees the card there: Cosmogoyf's P/T (`PtCda::OwnedInExile`), "cards
  exiled" counts, and the decision's own encoding (the card in the exile bag).

CR 616.1 makes the choice before the event happens, so the card never touches exile on the way to
a library. **Reachable:** Blightsteel Colossus (shuffled into the library instead) and an
opponent's Dauthi Voidwalker (exiled with a void counter instead) are both in the cube; when the
Colossus dies, its controller chooses. With Syr Vondam (EoE) watching, or a Cosmogoyf, the
difference shows. The cube and EoE tables are played separately, but that is a pool argument.

**Fix (workstream C).** Use the Mox Diamond pattern (`Zone::Nowhere`, 23.11.5):

1. Put the card in no zone while `ChooseReplacement` is pending. The invariants already allow
   `Nowhere` with a pending task.
2. Record the move's leave-the-battlefield and dies events with their look-back snapshot
   (`leaving`, the LKI vectors are already taken before the move), but **match them after the
   choice**, with `leaving_to` set to the chosen zone.
3. Then do the move: shuffle the card in, exile it, or exile it with a void counter.

Nothing else happens in between, because the choice is "right after the move, before anything
else". So the deferred matching is the same trigger timing as matching in place (603.2, 603.10a).
**Files:** `game.rs` (`move_card_ctl`, `choose_replacement`, `shuffle_in_instead`),
`triggers.rs` (a held event list), `eoe_w1.rs` (`LeftTo` reads the final zone).
**Decisions:** none new. **Digests:** the full digest changes for the digest games that reach a
`ChooseReplacement` (the decision's encoding no longer shows the card in exile).
`DIGEST_ENCODING=0` changes only if one of them had a watcher. Both are intended.

### N2: the public-state gate's next-layout gaps. Class 3 (encoding).

The network can't see these public facts, so it decides with less than the player has. The
rules are unaffected. On this branch, `public_state::limits()` has 9 gaps with kind `NextLayout`.
The open branches add 8 more, for 17 in all, plus two `Design` limits that a card's text reads.
For each, where it goes (section 5.8):

| # | Gap (perturbation) | Branch | Where it goes |
|---|---|---|---|
| 1 | Two graveyard cards swap places (Priority's `GRAVEYARD` ids are positions) | base | **Decomposition**: Priority's graveyard casts become one id plus a `ChooseGraveyardCard` over the list block (65.10's mechanism), so no id depends on order |
| 2 | Void holds | base | game spare float |
| 3 | A creature was cast for its warp cost | base | permanent status spare float |
| 4 | A copy is legendary by its exception | base | code 47 becomes "legendary as it is" (as 58.8 made types and colours "as they are") |
| 5 | A stack item targets another exiled card | base | **v6 bag row** (identity) plus a stack status float (a count) |
| 6 | A stack item noted another object | base | permanents: permanent status float "noted by a stack item"; exiled cards: the v6 bag row of #5 |
| 7 | A player will control the other's next turn | base | game spare pair |
| 8 | The active player is controlled this turn | base | game spare float, and the decision frame (N3) |
| 9 | An artifact entered under a player's control this turn | base | game spare pair |
| 10 | Pool mana restricted beyond the Workshop's | W2 | game spare pair (a count) |
| 11 | A land entered this turn (`lands_entered`) | W2 | game spare pair |
| 12 | A regeneration shield | W3 | permanent status float (count) |
| 13 | The mana spent to cast a permanent | W3 | permanent status float |
| 14 | A permanent is phased out | W3 | **v6 bag rows** (mine, the opponent's) plus counts |
| 15 | Phased out indirectly | W3 | in the counts of #14 |
| 16 | Granted triggered abilities (W3: no perturbation, "should be encoded"); the ability page's options name places the network can't see | W3 | permanent status floats: granted triggered and granted activated counts. The page decision uses the list block (F1) |
| 17 | A spell's granted alternative cost came from another permanent | W5 | permanent status float "grants the alternative cost of a spell on the stack" |
| D1 | More life gained this turn (a `Design` limit; Haliya reads the amount) | base | game spare pair (amount / 4) |
| D2 | A permanent entered this turn (`Design`; two cards read it) | base | permanent status float |

The other three `Design` limits concern what the opponent knows about the viewer's own cards.
They are the encoding's information model, not game state a card reads, so they stay.

**Fit in layout v5's spare floats:**

* Permanent status 112-119 takes #3, 12, 13, 16 (two floats), 17, 6, D2: 8 of 8.
* The game's 50-63 take #2, 7 (two), 8, 9 (two), 10 (two), 11 (two), D1 (two): 12 of 14.
* Stack status 64-71 takes #5's count: 1 of 8.

Only #5, the exiled half of #6, and #14-15 need identities, which means v6 bag rows.

### N3: The Dominion Bracelet (CR 722, ENGINE.md 99.14). Class 3 (blocked on the encoding).

The rules are built. During a controlled turn, a decision keeps its player (the controlled
player: `HAND`, `MY_PERM`, `LIBRARY` and `TRIGGER` ids are that player's). The decider
(`to_move`, the controller) is encoded in its own seat's frame, so the ids point at slots it has
no tokens for (EoE fuzz seed 12441, `check_list_options`).

**Decomposition can't help:** the ids are already right, and the frame is wrong. **No layout
change is needed** either, as 99.14 says ("No action id changes"):

1. `encode(viewer)` becomes `encode(seat, knower)`. The seat is the decision's player: its hand
   in the hand slots with the legal flags, its permanents as `MY_PERM`, its list block, its
   triggers. The knowledge is the decider's. Its own hand appears as the opponent's hand, known
   exactly, and the controlled player's hand is known through 722.3's sync. Hidden-information
   rule: everything is encoded from the decider's knowledge. `the_v5_facts_encode_no_hidden_information`
   gets a controlled-turn case.
2. A context float (v5 spare 248, "this decision is made for the other seat") tells the network
   that the value it must estimate is the decider's (the opposite seat's) while the frame is the
   controlled player's.
3. Gaps #7 and #8 in game spare floats.
4. 99.14.2's under-sharing is fixed in `eoe_w4.rs::sync_controller_view`. Every private reveal
   emitted to the controlled player during the turn (`KEY_LOOK`, a search's library contents as a
   cloud over the library) is mirrored to the controller.
5. Remove the lowering's block and add the card to `SUPPORTED_EOE_EXILE`.

**Files:** `az/encode.rs`, `az/mod.rs` (the encode call), `eoe_w4.rs`, `lower/eoe_w4.rs`,
`tests/az.rs`, `tests/cards_eoe_w4.rs` (the four ignored tests). **Tier:** encoding, plus
engine for the card. **Digests:** the cube is unchanged (no Bracelet); EoE fuzz only.

### N4: granted graveyard casting abilities judged by the front face. Class 3.

`faces.rs::face_castable` (468-472) has the comment "How the card may be cast from where it is
comes from the card as it is there (its front: 'instant and sorcery cards in your graveyard have
retrace' doesn't reach an adventurer creature card, CR 715.4)". `cast.rs::retrace_granted`
(685-695) matches the grant's filter against the card, which is its front face. Lier, Disciple
of the Drowned's ruling of 2025-10-02 (Scryfall, verbatim):

> Lier's last ability allows you to cast adventurer cards in your graveyard as Adventures and
> omen cards in your graveyard as Omens using the flashback it grants. This is because the
> alternative characteristics of those cards are considered when determining whether or not
> those spells can legally be cast. Note that only the Adventure or Omen can be cast this way.
> For example, if you control Lier and you have Bonecrusher Giant in your graveyard, you can cast
> Stomp with flashback, but not Bonecrusher Giant.

So a grant of a casting ability to "[type] cards in your graveyard" is judged by the
characteristics of the spell being cast, in both directions. The W5 branch reads the same
principle for granted alternative costs (CR 601.3e; Henzie's ruling for a modal back face).

**Divergences, both reachable with cube cards:**

* Wrenn and Six's emblem ("Instant and sorcery cards in your graveyard have retrace"), with
  Brazen Borrower // Petty Theft or Embereth Shieldbreaker // Battle Display in the graveyard.
  The Adventure should be castable by retrace; the engine doesn't offer it.
* Six ("nonland permanent cards in your graveyard have retrace", during your turn), with the
  same cards. Only the creature may be cast by retrace, not the Adventure. The engine computes
  the route from the front (it matches) and then checks the Adventure's castability through that
  route.

Underworld Breach's escape ("each nonland card") gives the same answer for either face. Targeted
grants (Snapcaster Mage) are right: targeting reads the graveyard card's own characteristics
(Brazen Borrower's 2025-06-06 ruling: Jidoor can't be targeted by Sorceress's Schemes).

**Fix (workstream C).**

* `gy_routes_esc` gets the face's definition `fd`, as the Paragon/Lurrus branch already uses.
  It evaluates every granted route's filter (`retrace_granted(p, card, fd)`, the escape grant's
  "nonland") against `fd`, with the card's other state.
* `castable_gy_routes` and `face_castable` ask per face.
* A card in the graveyard keeps its front characteristics for every other purpose (targets,
  counts, filters).

**Files:** `cast.rs`, `faces.rs` (and the comment). **Decisions:** none new; `ChooseFace` and
`ChooseRoute` already exist. **Digests:** change only if a digest game has the emblem or Six with
an adventurer in the graveyard.

## 4. New rows: the comment sweep and the open branches

The sweep covered comments with "at most", "first N", "only", "approximat", "assume", "not
modelled", "kept:", "cap", "saturat", "clamp", "truncat", "simplif", "none in the cube", "no
card", "for now" and similar, plus uncommented `.take(`, `.min(` and `NUMBER_N - 1` clamps.
Hits already logged are omitted.

| Id | Where | Class | The divergence | Fix |
|---|---|---|---|---|
| N5 | `cast.rs` 855, 900, 916, 935, 1821, 1890; `activate.rs` 1268; `resolve_ext.rs` 1192; `lower/printing.rs` 1008, 1022, 1037 | 3 | X (abilities, spells, kicker X, "pay X life"), the number of "X targets", squad, replicate and multikicker counts, divided damage outside combat, and the energy amount paid are clamped to 0..31 (`NUMBER_N - 1`). 13.9 and 23.11.3 cover only combat. Gaea's Cradle or a mana loop gives 32 or more mana. Life can be above 31. | F2 |
| N6 | `game.rs` 2402-2415 | 3 | Hullbreacher and Quantum Riddler on one draw: the drawer gets the first order (616.1 gives the drawer the choice). The other order gives the opponent one more Treasure, which is "dominated" only by assumption (card text could reward giving it). Low. | F5 |
| N7 | `resolve_ext.rs` 1017 | 3 | `CreateTokenAttachSource` with a doubler attaches to the first token. The tokens are identical, but their relative timestamps are N15's choice. Low. | F5 (with N15: "which token") |
| N8 | `activate.rs` 731 | 3 | Crew and saddle payability enumerates subsets of only the first 10 creatures with mana abilities. With more, a payable crew can be refused. | F4 |
| N9 | `game.rs` 1503 | 3 | The X a ravenous permanent enters with is stored in 6 bits (`x.min(63)`). Once N5 is fixed, X above 63 is clamped. | F2 |
| N10 | `pay.rs` 670 | 3 | A source without {T} that removes counters for mana (Pentad Prism) is reusable at most 32 times in one payment (`.min(32)`). Counters can be proliferated or doubled. | F4 |
| N11 | `pay.rs` 1796 | 3 | `PayPool` offers at most 32 distinct pools left (`truncate(NUMBER_N)`) after a fixed ranking. More are reachable with three or more types in the pool. | F1 |
| N12 | `resolve_ext.rs` 866 | 3 | "You may remove a counter" offers the first 16 counter kinds. Keyword counters and named counters exceed 16 kinds. | F1 |
| N13 | `companion.rs` 102 | 2 | `take(64)` over companion definitions. Bounded by the table's companion cards. Add a load-time check that a table has at most 64, so the bound is loud. | none (assert) |
| N14 | `resolve_m7d.rs` 629 | 3 | The names Anointed Peacekeeper offers are cut at 64 (hidden-deck pools are larger). Part of 31.7.5. | F1 |
| N15 | `game.rs` (`next_ts`, `group_ts`) | 3 | Objects that get timestamps at the same time (entering together, becoming attached together) get them in processing order. CR 613.7m: "the active player determines their relative timestamp order". The order matters in layers: Magus of the Moon and a land-type adder returned together by a mass reanimation; two static P/T or type setters. | F5 |
| N16 | `lower/effect_ext.rs` 463 | not a rule | A card-specific repair of IR data (Celestial Colonnade's colours and type). It belongs in docs/ir_defects.md. | doc |
| N17 | `lower/effect.rs` 731 | 1 | The IR's `bottom_choice: false` is ignored, and scry always offers the player's choice. This is the Oracle (an IR defect, not a shortcut). | doc (ir_defects) |
| N18 | `lower/effect.rs` 325, 451 | 1 | "Can't be regenerated" is dropped. Faithful on this branch: no regeneration op exists, so no shield can exist. W3 builds regeneration and honours the flag (`W3State::no_regen`). | merge W3 |
| N19 | W2 99.12 | 3 | Mana floated with priority loses its spending restriction in the cube table (Mishra's Workshop's {C}{C}{C} floated in response pays anything). W2 kept the cube's behaviour "a known fault" to keep digests. | F4 (every table; 12 of the 2,500 digest games change) |
| N20 | W2 99.12 | 3 | Delighted Halfling's "can't be countered" rider is lost on floated mana (the pool has no such tag). | F4 |
| N21 | W2 99.12 | 3 | `mana::fits` ignores restrictions on needs. Workshop's {C} pays Chromatic Star's {1} while an artifact is cast (a cube fault since 34). Sunken Citadel's land-ability mana isn't offered for Cascading Cataracts' {5}. | F4 (with 13.15) |
| N22 | W2 99.12 | 3 | A basic land type an effect gives a permanent doesn't give it that type's mana ability (CR 305.6-305.7). This is engine-wide. | F6 |
| N23 | W2 99.12 | 3 (loud) | Gene Pollinators of which only some are creatures, together with a "whenever you tap a creature for mana" bonus, fail loudly. | F4 |

W3 and W5 report no remaining approximation. W3's four first-version shortcuts were fixed before
review, and W5 adds no docs/shortcuts.md entry. Their gaps are in N2.

## 5. Fix designs

General rules for every fix: a failing-first test per behaviour, named after its CR rule; no new
action-id layout; the 20k fuzz runs (default and `FUZZ_POOL=eoe`); docs/shortcuts.md rows marked
**fixed**. A fix meant to change play re-records the digests on purpose (`DIGEST_WRITE=1`),
stating old -> new. That is acceptable here: the old behaviour was a rules error, so the new
games are the right ones. Runs fork onto the new build (a policy trained on the wrong rules is
retrained, not resumed blindly).

### 5.1 F1: paged choices (one helper for every slot range)

The precedents are W3's ability page (`Task::ChooseAbility`, a `ChooseMode` with `NUMBER` +
place - 3 and `NUMBER` + 31 for a further page, ENGINE.md 99.13) and W1's linked-creature choice
(31 exile slots, then `PASS` for the next page, 99.11).

**Helper** (`decision.rs`, commit A0): `Game::ask_paged(task, player, kind, base, n, items,
extra)`.

* With `items.len() <= n`, the ids are `base + k` exactly as today, so behaviour and digests are
  unchanged.
* Otherwise page `p` offers items `p*(n-1) .. p*(n-1)+n-2` at `base + 0 .. base + n - 2`, and
  `base + n - 1` is "the next page" (`Choice::Page(p + 1)`). The last page has no "next".
* The page number lives in the task, so a `determinize` refresh re-asks the same page.
* Canonical order is kept, so every item is reached by exactly one sequence of decisions (each
  item lies on one page, and the pages are visited in order).
* Encoding: context floats 246 ("paged") and 247 (page / 4), from v5's spare context floats. This
  is encoding-only (tier encoding), and `az/encode.rs` is workstream D's file, so the two floats
  land in A0.

**Applications:**

| Range | Limit removed | Rows | Notes |
|---|---|---|---|
| `STACK` 32 | targets in reach | 58.1 | `stack_in_reach` and `targeting` become "every item". The page holds items 31 and deeper. A copy's `PASS` to keep a target stays. The checks before a cast (`has_candidates`) stop excluding deep targets. |
| `TRIGGER` 16 | first 16 distinct | 31.7.8 | Drop the "firsts then rest" reordering. |
| `GY_ABILITY` 12 | first 12 cards | 27.13.5 | One ability per card stays a refusal. |
| `EXILE` 32 (priority, `ChooseExiled`, exile targets) | first 32 | 58.4, 99.14.1 | Unify W1's `PASS`-paging with the helper. |
| `LIBRARY` list 64 (graveyard choices and targets, names) | assert at 64 | 65.10, 31.7.5, N14 | The list block shows the current page's cards, so each id still has its token (the meaning 65.10 gave the list). Peacekeeper's names become every card's in the game, both owners, plus `PASS` = "a name of no card". |
| `MY_PERM` / `OPP_PERM` 96 | `perm_room`, the token cap | 26.11.6, 24.7.4 | Permanents past slot 94 are reached through slot 95 = "more" (follow-up decisions in the same range). `ACTIVATE` for them goes through W3's ability page, which lists (permanent, ability) pairs past the boundary. Remove `token_count`'s 64 and `perm_room`. The 34 callers of `perm_action` go through one function, `Game::perm_option`, which also decides the page. |
| `ACTIVATE` 4 per permanent | granted abilities | 25.10.5, 99.14.3 | Merge W3's page. Change its options from `NUMBER` ids to `LIBRARY` ids over the list block. The token is the definition that holds the ability (the card's own, or the granting face) and a float gives the ability's index, so the network sees what each option is (gap #16) inside v5. |
| Payment sources of one permanent | 4 `ACTIVATE` slots | 31.7.2k | `PayMana` picks the permanent by its `MY_PERM` id, then W2's `PayMana::which` (`ChooseMode`) asks which of its sources, paged. |
| `NUMBER` for `PayPool` and counter kinds | 32 and 16 | N11, N12 | `NUMBER` + 31 = next page. |

**Digests:** unchanged unless a digest game exceeded a range. The full digest's encodings change
only for paged decisions.

### 5.2 F2: big numbers

The helper is `Game::ask_number(task, player, kind, lo, hi)`, in `decision.rs` (A0):

* With `hi - lo <= 31`, `NUMBER + (v - lo)` as today, so nothing changes.
* Otherwise `NUMBER + k` for `k <= 30` means `v = lo + k`, and `NUMBER + 31` means "`lo + 31` or
  more". The rest `d = v - lo - 31` is then chosen in base-32 digits, most significant first. Each
  digit decision offers only digits with a legal completion (prefix feasibility). Every value is
  reached exactly once, in at most 7 extra decisions for a `u32`.
* The task keeps the prefix and the digits left.
* Context floats 249 (the digit position) and 250 (the prefix, squashed): v5 spares,
  encoding-only.

**Uses:**

* X of spells and abilities, kicker X, "pay X life" (`hi` = life), "X targets" (`hi` = the
  candidates, no 255).
* Squad, replicate, multikicker, divided damage, the energy paid.
* Combat damage: `assign_damage` offers any amount in `lo..=hi`. The special codes 30 (lethal) and
  31 (all) go. The search reaches lethal through the digits.
* Wizard's Rockets' X.

**Storage widened (exact):** `StackItem::x` and `times` (u8 -> u32); the ravenous X kept apart
from `enter_note` (N9); frame loop counters (46.4) and `spells_cast`, `drawn` and storm counts
(46.5) to u32. **Files:** `decision.rs`, `cast.rs`, `activate.rs` (the X loop), `combat.rs`,
`resolve_ext.rs`, `copies.rs` (loop counts), `game.rs` (the note, counters), `resolve.rs`.
**Digests:** unchanged unless a digest game had 32 or more to divide (combat) or a larger X.

### 5.3 F3: mana abilities at any time (13.12, 13.2k, 13.4, 13.13, 13.14)

* **Priority.** One new option, `act::YES` ("activate mana abilities"). YES is not among
  Priority's ids today (`cast.rs::priority_options_all`). It opens a new decision kind,
  `ActivateMana` (the 47th of the context block's 64, no layout change). Its options:
  * every mana ability the player may activate now (`ACTIVATE` ids, through F1's page past four);
  * Spirit Guides (`HAND_ABILITY`);
  * `PASS` = done.

  Each pick runs the ability (`ChooseMana` and the rest as in a payment). The mana goes to the
  pool with its tags and restrictions (W2's `pool_restr`), and the decision repeats. The player
  keeps priority (mana abilities don't use the stack, 605.3b). Lion's Eye Diamond stays
  priority-only by its ruling, and is offered here. 86's targeted-source `ACTIVATE` options at
  priority become redundant. Keep them for a first commit (fewer digest changes), then fold them
  in.
* **Payments.** When the virtual pool alone could pay the rest, `PayMana` no longer stops. It
  offers the remaining sources, with `PASS` = "spend from the pool", so a land can be tapped while
  the pool's {R} is kept (13.13). `PayPool` then offers every remainder (13.1k). Wizard's Rockets
  is offered like any source (13.14, the `convert_ok` gate removed).
* **Why not a relevance filter.** Offering the float only "when it can matter" would need a rules
  proof for each condition. The known reasons are many: losing the source, painlands' damage,
  hand size, tapped status, tap triggers, a cost determined before 601.2g. The single option costs
  one id per priority decision and a sub-decision only when chosen.

**Files:** `activate.rs` (`mana_with_priority`, `mana_at_priority`, the new option), `pay.rs`
(`PayMana`'s stop rule, `convert_ok`), `flow.rs` (the task), `decision.rs` (the kind), `az/bots.rs`
(the heuristic bots answer the new kind: they pass), `az/encode.rs` (the kind's one-hot).
**Digests:** **every** game changes (Priority's legal actions gain YES wherever a source is
available): both digests are re-recorded. This is the largest behaviour change in the audit, and
it is meant.

### 5.4 F4: the payment model, exact (13.1k, 13.3, 13.15, 13.16, 13.17, 27.13.4, 28.8.2k, N8, N10, N19-N21, N23, 24.7.5k, 40.1)

* **13.3: exclusion by conflict.** `mana_exclude` returns per ability, not per card. A mana
  ability of the cost's own source is excluded only if:
  * both it and the cost tap the source;
  * or it moves the source (sacrifice, exile, return, discard) and the cost needs the source
    (taps, sacrifices, exiles or discards it);
  * or both remove more counters than the source has.

  `Keep::exclude` becomes a set of (card, ability) pairs.
* **13.15, N21: exact needs.** A unit carries its producing source's index and its restriction
  purposes, and a need carries "not payable by its own source" and its allowance. `mana::fits`
  matches on those. The mask is internal: widen `ManaMask` to a struct (no encoding effect).
* **13.16.** `PayMana::forced` admits several-mana self-sacrificing sources (Black Lotus), and
  `delve_sources` includes them.
* **13.17.** Teamwork's tapped creature is reserved (`Keep`), instead of removing the Lumberjack's
  classes.
* **27.13.4, N8: tap costs as a matching.** "Tap n untapped X" and crew/saddle use W3's
  reservation (`Keep::sac_taps`, `tap_x_reserve`): every candidate is counted, and the payment
  reserves n of them. No 10-creature enumeration.
* **28.8.2k: colours spent.** When the spell reads the colours of the mana spent (converge,
  sunburst, "if {C} was spent"), and after the sources are chosen the colour sets the player can
  produce differ, ask `ChooseColorsSpent`. Its ids are `NUMBER` + the bitmask of WUBRG spent:
  exactly the 32 subsets, so no paging. Single-mana dual sources no longer "count as their best
  colours". The hybrid/Phyrexian "upper bound" (`pay.rs` 1978) becomes exact.
* **N10.** `reuse` is counted exactly (u32).
* **N19, N20.** Keep restrictions on mana floated with priority in every table (W2's
  `add_restricted_mana` gate removed), and add a pool tag for "can't be countered" mana. This
  changes 12 of the 2,500 cube digest games (W2's measurement).
* **N23.** The bonus per Pollinator that activates: which one activates is a choice (W2's
  `which`).
* **24.7.5k, 40.1: costs with several classes.** `merge_costs` and `Game::keep_for` reserve
  several classes of hand cards or permanents (retrace's land and the card's own discard; retrace
  and a pitch, return or sacrifice cost).

**Files:** `pay.rs`, `mana.rs`, `activate.rs` (the mana and cost parts), `eoe_w2.rs`, `db.rs`
(`merge_costs`), `cast.rs` (`delve_sources`, `alt_rest`). **Digests:** change where a payment
had one of these situations: few games, except N19 (12 measured).

### 5.5 F5: replacement effects, timing, order (N1, 29.6.4, 29.6.5, 32.5.2, 39.1, 13.8, N6, N7, N15)

* **N1:** section 3.
* **29.6.5.** The draw instruction's loop (`Op::Draw` and the draw-n ops in `resolve.rs`; the
  frame keeps its count) suspends right after the card that sets `miracle_check`, asks
  `MiracleReveal` there, and resumes. Draws in the draw step are already single. The deferred
  path in `sba.rs` goes.
* **32.5.2.** Remove `this_turn` temporary triggers in `end_turn_effects` (514.2, `layers.rs`),
  not as the next turn starts.
* **29.6.4.** `Turn::extra_phases`: "after this phase" adds a combat phase after the current
  phase, whatever it is (500.8). `flow.rs` runs the queue. `extra_combats` stays for the combat
  case's encoding.
* **39.1, N6.** Two new `ChooseReplacement` codes (NUMBER 4: Bilbo's exile before the
  Adventure's; NUMBER 5: the second draw-replacement order), asked only when both apply.
* **13.8.** Add `uses_source` beside `uses_event`, set by the lowering when the program reads its
  source's per-object state (power, toughness, counters, attachments, or its LKI). `same` then
  also requires the same source object unless `!uses_source`. Files: `triggers.rs`, `db.rs`
  (`TriggerDef`), `lower/mod.rs`, `program.rs` (`Op::uses_source`).
* **N15, N7.** When two or more objects get timestamps in one event and at least one of them has
  an effect whose order matters, ask the active player for the order (`OrderTimestamps`). An
  effect whose order matters means a static ability applying in layers 1-7, or an Aura or
  Equipment attaching. The decision is a `ChoosePermanent` sequence over their `MY_PERM` /
  `OPP_PERM` ids, earliest first, with `PASS` = "the rest in canonical order", offered when the
  rest are interchangeable. N7's attach target among several created tokens is a
  `ChoosePermanent` among them. Files: `game.rs` (`group_ts`), `layers.rs` (the fast-path test).

**Digests:** 29.6.5 changes games with a miracle deck and multi-card draws. 13.8 changes games
with simultaneous same-definition triggers from different objects. The rest are rare.

### 5.6 F6: characteristics, layers, faces (28.8.4, 26.11.3, 26.11.5, 31.7.7k, N4, N22, 13.10, 25.10.4)

* **28.8.4.** Painter's colour joins the layer-5 effects in timestamp order (its permanent's
  timestamp), so a "becomes" colour effect overwrites it only when later (`layers.rs::colors_of`).
* **26.11.3.** `faces.rs`: a transforming DFC that is copying something transforms. Its face
  status flips, and the copy effect keeps its characteristics until it ends.
* **26.11.5.** `Except::subtypes` becomes a bitset over the subtypes any copy exception of the
  table names, computed at load. Exact, with a load-time check (at most 64) that is loud.
* **31.7.7k.** Class levels per object: a copy starts at level 1 and may level up with its copied
  level abilities (707.2, 716).
* **N4:** section 3.
* **N22.** CR 305.6-305.7: a permanent with a basic land type from an effect has that type's
  intrinsic mana ability (`abilities()`, as W2's moon definitions do).
* **13.10, 25.10.4 (deny audit).** `lower/printing.rs` denies the card type "Battle" until CR 310
  is built. The audit confirms that meld, flip, convert, "cast transformed", read ahead, morph,
  disguise and cloak deny.

**Digests:** 28.8.4 changes cube games with Painter's Servant and an animated manland (rare). The
rest change nothing in the cube.

### 5.7 F7: numbers and loops (46.1-46.3, 46.6, 29.6.6, 26.11.7)

* **Exact or abort.** Counters, P/T sums, life, damage and mana counts become 64-bit with checked
  arithmetic. On overflow, the game ends **aborted**. That is a new terminal kind which the
  self-play driver drops, never a draw or a win: a small opt-in in zero-core (an episode without a
  result), or a Python-side filter on `Game::aborted`. The engine stays exact in every game it
  completes. 46.6's object cap: recycle the slots of objects that ceased to exist and are no
  longer referenced, or widen `CardIdx`; past it, abort.
* **Loops (732.2a).** Replace `ACTIVATION_CAP` and `MAY_COPY_CAP`:
  * an iteration that would return the game to a state already seen at this point of the turn is
    not offered (no progress, so equal to stopping);
  * a detected cycle (the same action sequence with the state equal except for the quantities it
    changes by the same amounts) offers "repeat it N more times" through F2's number decision;
  * after that the cycle's manual repetition is not offered.

  This is the heaviest design. It needs a state hash (`testkit`'s hash, made a real one) and
  careful tests (Grim Monolith or Basalt Monolith loops, Time Vault + Voltaic Key's turns, which
  the turn cap ends). Last in order.

### 5.8 F8: the encoding, and layout v6

* **v5, encoding-only (tier encoding):**
  * the page and digit context floats (246, 247, 249, 250: A0);
  * the Bracelet's frame flag (248) and frame flip (N3);
  * the scalar gaps of N2 in v5's spare floats (permanent status 112-119 full, game 50-61);
  * gap #4's meaning change (legendary as it is);
  * gap #1 by decomposition: Priority's graveyard casts become `GRAVEYARD` + 0, "cast or play
    from your graveyard", then a `ChooseGraveyardCard` over the list block. The `GRAVEYARD` ids
    stop depending on order, and the `NextLayout` limit goes.

  Gate: `public_state_reaches_the_encoding` asserts only `Design` limits again (as after 60),
  and the hidden-information tests cover the flipped frame.
* **Layout v6 (minimal, additive, exact widen):** three bag rows.
  * Row 19: exiled cards a stack item targets or noted (gaps #5 and #6), both players' items.
  * Row 20: the viewer's phased-out permanents (gaps #14 and #15).
  * Row 21: the opponent's phased-out permanents.

  No segment, slot, present flag, identity or action id changes, so zero-core's `widen` follows
  it exactly (new bag rows go at the end, ENGINE.md 60.2). It needs `layout_v6.json`,
  `v5_projection()` and the `DIGEST_ENCODING=v5` check that the v5 floats are unchanged.
  **Recommended with it:** a headroom block, because the v5 spares will be full after the above:
  a v6 globals block (per player 16, the game's 16), permanent status +16, stack status +8. Move
  the scalar gaps there instead of into the v5 spares if v6 comes first. Either order works.

## 6. Workstreams and order

**First, merge the open branches:** W2, W3 and W5 into `eoe-base`, then main. F1 builds on W3's
page (and fixes the `granted_activated` reconciliation it already did), and F4 builds on W2's
restricted pool and `PayMana::which`.

| Order | Workstream | Fixes | Files (owner) | Tier |
|---|---|---|---|---|
| 0 | **A0**: helpers | `ask_paged`, `ask_number`, context floats 246-250 | `decision.rs`, `az/encode.rs` (four floats) | engine (no behaviour change: digests unchanged) |
| 1 | **A**: choice ranges | F1, F2 (13.9, 23.11.3, 24.7.4, 25.10.5, 26.11.6, 27.13.5, 31.7.2k, 31.7.5, 31.7.8, 46.4, 46.5, 58.1, 58.4, 65.10, 99.14.1, 99.14.3, N5, N9, N11, N12, N14) | `combat.rs`, `triggers.rs` (`put_triggers` paging only), the range and X code in `cast.rs`, `copies.rs` (loop counters), `resolve_ext.rs`, `resolve_m7d.rs` (names), `resolve_tail2.rs`, `resolve_tail3.rs`, `resolve_walkers.rs`, `eoe_w4.rs` (exile targets), `eoe_w1.rs` (paging), `game.rs` (`perm_option`, the token cap), `activate.rs` (pages) | engine |
| 1 | **B**: mana and payment | F3, F4 (13.1k, 13.2k, 13.3, 13.4, 13.12-13.17, 24.7.5k, 27.13.4, 28.8.2k, 40.1, N8, N10, N19-N21, N23) | `pay.rs`, `mana.rs`, `activate.rs` (mana and cost functions), `eoe_w2.rs`, `db.rs` (`merge_costs`), `cast.rs` (`delve_sources`, `alt_rest`), `flow.rs` (the float task), `az/bots.rs` | engine |
| 1 | **C**: rules timing and characteristics | F5, F6 (N1, N4, N6, N7, N15, N22, 13.8, 13.10, 25.10.4, 26.11.3, 26.11.5, 28.8.4, 29.6.4, 29.6.5, 31.7.7k, 32.5.2, 39.1) | `game.rs` (moves, replacement effects, draws, timestamps), `triggers.rs` (`same`, held events), `flow.rs` (phases), `sba.rs`, `resolve.rs` (draw loop), `layers.rs`, `faces.rs`, `copies.rs` (`Except`), `resolve_m7d.rs` (Class levels), `resolve_tail2.rs` (extra combat), `lower/printing.rs`, `lower/mod.rs`, `eoe_w1.rs` | engine |
| 1 | **D**: encoding | F8 (N2, N3, 99.14.2), then layout v6 | `az/encode.rs`, `az/mod.rs`, `public_state.rs`, `eoe_w4.rs` (`sync_controller_view`), `lower/eoe_w4.rs`, `tests/az.rs`, `mtgz/az/layout_v6.json` | encoding (+ engine for the Bracelet) |
| 2 | **F7**: numbers and loops | 46.1-46.3, 46.6, 26.11.7, 29.6.6 | `num.rs`, `layers.rs` (P/T), `game.rs` (life, objects), `activate.rs` and `copies.rs` (caps), a zero-core opt-in or a Python filter | engine + optin-core |

**Where the files overlap.** The four streams share four files at the function level:

* `activate.rs`: A has the pages, B the mana and cost functions.
* `cast.rs`: A has the ranges and X, B the costs and delve, C nothing.
* `triggers.rs`: A has `put_triggers`' paging, C `same` and held events.
* `game.rs`: A has `perm_option`, C moves and timestamps.

Merging A first, then B and C (in either order), keeps the conflicts small. D is independent
after A0. Recommended merge order:

1. A0;
2. A (unblocks the deep-stack panic class and every clamp);
3. C (N1, N4, the miracle: rules the cube plays now);
4. B (F3 re-records every digest, so it should land once, after the smaller behaviour changes
   are measured);
5. D (with or after B, so runs fork once onto F3 plus the encoding);
6. F7.

**Digests in summary.** A0, A and D (v5 part): unchanged, except games that exceed an old range,
and the full digest where a decision becomes paged. C: a small set of games (miracle, same-source
triggers, Painter, replacement choices). B: all games (YES at priority), plus W2's 12 restriction
games. F7: games that hit the caps. Every re-record is a rules fix and goes in its commit message
and ENGINE.md section.

## 7. Follow-ups for docs (not done here: this branch edits this file only)

* docs/shortcuts.md: 31.7.6 is fixed (ENGINE.md 71.2). 25.10.5 and 99.14.3 are fixed when W3
  merges. N13 needs a load-time assert. Add rows N1, N4-N15 and N19-N23 until they are fixed.
* docs/ir_defects.md: N16 (Celestial Colonnade's lost colours and type, repaired in the
  lowering), N17 (scry's `bottom_choice`), 26.11.9 (Chain of Smog's flag).
* `faces.rs`: the comment at 468-472 states the opposite of Lier's ruling (N4).
* `layers.rs` 323: the comment "none of those exists" is wrong (28.8.4: animated manlands set
  colours).
