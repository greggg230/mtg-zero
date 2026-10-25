# Rules shortcuts: the tracking table (M7e)

Every shortcut the engine documented in ENGINE.md's simplification lists (section 13's
"Deliberate simplifications" and 23.11, 24.7, 25.10, 26.11, 27.13, 28.8, 29.6, 30.7, 31.7,
32.5), one row each, with what became of it. ENGINE.md's lists now point here and keep only what
still applies.

Classes:

* **A**: a rules departure observable in cube play. **Fixed** (M7e part 1, branch `m7e-rules`)
  with a failing-first test in `engine/tests/rules_shortcuts.rs` that names the CR rule, or
  **deferred** to the payment rework (M7e part 2, "group 1": the pool's mana choice, mana
  abilities with costs during a payment) and left alone here.
* **B**: an engineering limit kept on purpose, mostly tied to the frozen action-id layout
  (layout v3: `NUMBER` 0..31, 96 permanent slots, 4 `ACTIVATE` slots, 16 `TRIGGER` slots, 64
  tokens) or to the search model.
* **C**: rules-equivalent (a representation that offers the same choices or results: definitions
  instead of cards in a search, modes as multisets, dominated options left out) or not reachable
  with the cube's cards.

| Id | Shortcut | Class | Status | Where |
|---|---|---|---|---|
| 13.1 | Pool mana is spent first and automatically (coloured pips by colour, hybrid by the more plentiful, generic colourless first) | A | deferred (payment rework) | `pay.rs` (`pay_into_for`) |
| 13.2 | Mana abilities aren't free-standing priority actions; a payment activates a minimal set of sources and never floats mana | A | deferred (payment rework) | `pay.rs`, ENGINE.md 5, 18 |
| 13.3 | A cost's source is never one of its own mana sources when the cost taps, sacrifices, exiles or discards it | C | kept: no supported card has such a pair | `pay.rs` (`Keep::exclude`) |
| 13.4 | A Spirit Guide's mana ability isn't offered at priority | C | kept (floating its mana first can't matter; the payment rework may revisit) | `activate.rs` |
| 13.5 | A searched library offers definitions, not cards | C | kept: every search shuffles | `resolve.rs` (`Op::Search`) |
| 13.6 | Modes: one decision per mode in printed order; repeats as multisets | C | kept: each set of modes reachable once | `cast.rs` (`ChooseModes`) |
| 13.7 | Events are recorded only for the kinds some permanent listens for (`trig_mask`) | C | kept: nothing else can see them | `triggers.rs` |
| 13.8 | Waiting triggers of the same definition and ability are offered once in the ordering decision | C | kept: interchangeable (their effects ignore the source's per-object state, or the events differ) | `triggers.rs` (`put_triggers`) |
| 13.9 | Combat damage amounts in the `NUMBER` range (0..31) | B | kept: the `NUMBER` ids are frozen | `combat.rs` |
| 13.10 | Battles can't be attacked; the starting player comes from the seed; determinize assumes open decklists | C / B | kept: no battle in the cube (C); no play/draw decision and open decklists are the self-play model (B) | `combat.rs`, `game.rs`, `hidden.rs` |
| 13.11 | Targets and activations of identical permanents are not deduplicated | C | kept: duplicates offer the same results (an optimisation) | `decision.rs` |
| 23.11.1 | Pyrogoyf-like "that creature deals damage" used the gone creature's characteristics as a card | A | **fixed**: the damage source as it last existed on the battlefield (608.2h, 113.7a; `Game::lki_damage`, `SEEN_LKI_DAMAGE`) | `resolve_ext.rs` (`Ext::DamageFrom`); `cr_608_2h_pyrogoyfs_creature_deals_damage_as_it_last_existed` |
| 23.11.2 | Untap-up-to effects offer only tapped permanents | C | kept: untapping an untapped permanent does nothing | `lower/effect_stack.rs` |
| 23.11.3 | Divided combat damage above 31 | B | kept (as 13.9) | `combat.rs` |
| 23.11.4 | An Equipment's timestamp wasn't refreshed when re-attached | A | **fixed**: an Aura or Equipment becoming attached gets a new timestamp (613.7e; `Game::attached_timestamp`) | `resolve_ext.rs`, `resolve_tail3.rs`; `cr_613_7e_equipment_reattached_gets_a_new_timestamp` |
| 23.11.5 | Mox Diamond's "if this would enter" applied only as the spell resolves | A | **fixed**: however it would enter (Tinker, Urza's Saga, Goblin Welder, ...): the card waits revealed in no zone while its would-be controller decides (`Task::EnterByDiscardAny`, a `Discard` decision) | `game.rs` (`put_onto_battlefield`), `cast.rs` (`enter_by_discard_any`); `cr_614_1a_mox_diamond_put_onto_the_battlefield_asks_for_a_land` |
| 23.11.6 | Underworld Breach's escape wasn't offered for a card with its own additional costs | A | **fixed**: the granted cost and each option are one cost (601.2b, 601.2f-h; `CardDb::merged_cost`, `merge_costs`) | `db.rs`, `cast.rs` (`route_extra`); `cr_601_2b_breach_escape_with_the_cards_own_additional_cost` |
| 23.11.7 | The monarch's end-step draw and "combat damage to the monarch", and Okoye's attack trigger, happened at once | A | **fixed**: the monarch's inherent triggered abilities (724.2) are sourceless triggers of a rules definition "The monarch" (controlled by the monarch, one per creature); Okoye's is a delayed triggered ability bound to the creature (603.7b-c, `M7d::TriggerThisTurnFor`, `TrigPred::AttacksMonarch`) | `dungeon.rs` (`monarch_trigger`), `flow.rs`, `combat.rs`, `lower/effect_stack.rs` (`monarch_def`); `cr_724_2_*`, `cr_603_7_okoyes_*` |
| 23.11.8 | A graveyard card's castability is checked on a clone with the card on the stack | C | kept: that is 601.2 (the spell's characteristics on the stack) | `faces.rs`, `cast.rs` |
| 23.11.9 | True-Name Nemesis: choosing yourself is offered | C | kept: legal, dominated | `lower` |
| 23.11.10 | Tap-or-untap, a counter kind, an energy amount and X use `ChooseMode`; "top or bottom" is a `YesNo` | C | kept: representation | `decision.rs` |
| 24.7.1 | `AttackTarget` asked right after each attacker | C | kept: no restriction depends on what a creature attacks | `combat.rs` |
| 24.7.2 | Naming a card offers the namer's library names and one "other name" | C | kept: names outside the library act alike | `resolve_walkers.rs` |
| 24.7.3 | Nissa's extra mana is part of the Forest's mana ability | C | kept: payments never float mana (the payment rework may revisit) | `lower/effect_walkers.rs` |
| 24.7.4 | At most 64 tokens per creation after doubling | B | kept | `game.rs` (`token_count`) |
| 24.7.5 | Retrace wasn't offered for a card with its own additional costs | A | **fixed** with 23.11.6 (granted retrace: `RETRACE_ROUTE`); a land discard with another discard can't be one cost and stays unoffered (none in the cube) | `db.rs`, `cast.rs` |
| 24.7.6 | (The heuristic bot's use of loyalty abilities) | — | not a rule | `az/bots.rs` |
| 25.10.1 | `face_castable` evaluates a face on a clone | C | kept: implementation | `faces.rs` |
| 25.10.2 | "Do this only once each turn" vs "this ability triggers only once each turn" | C | both built since M7c | `triggers.rs` |
| 25.10.3 | Mayhem is tied to the graveyard object discarded this turn | C | kept: that is 400.7 | `faces.rs` |
| 25.10.4 | Split cards: only Life // Death; no meld, flip, convert, "cast transformed", read ahead | C | kept: not in the cube | — |
| 25.10.5 | At most four activated abilities per permanent | B | kept: the `ACTIVATE` slots | `lower` |
| 26.11.1 | A copy of a spell leaving the stack ceases to exist at once | C | kept: nothing can see it elsewhere | `game.rs` |
| 26.11.2 | "Enter as a copy" could choose a permanent entering at the same time (Exhume) | A | **fixed**: not one that entered in the same event or since (`Game::group_ts`, 614.12a); the choice right after the move (its `Enters` event held back) stays (C) | `copies.rs` (`enter_as_copy`); `cr_614_12a_metamorph_cant_copy_a_permanent_entering_at_the_same_time` |
| 26.11.3 | A double-faced card that is copying something doesn't transform | C | kept: reachable only through Saheeli's -2 on a daybound creature over a day/night change | `faces.rs` |
| 26.11.4 | "Another target" requirements choose at most one target each | C | kept: the lowering checks | `lower` |
| 26.11.5 | A copy effect's exceptions hold at most four subtypes | B | kept | `copies.rs` |
| 26.11.6 | At most 96 permanents per player | B | kept: the `MY_PERM` / `OPP_PERM` slots | `game.rs` |
| 26.11.7 | At most 31 copies a turn by "may copy this spell" | B | kept (732.2a loop cap) | `copies.rs` |
| 26.11.8 | Kicker's additional costs added after reductions | A | fixed in M7b part 3b (27.6) | `cast.rs` |
| 26.11.9 | Chain of Smog's IR flag `needs_target_with_cards_in_hand` ignored | C | kept: the Oracle text has no such condition | `lower` |
| 27.13.1 | Granted ward was the granting permanent's ability (its source) | A | **fixed**: a granted triggered ability's source is the permanent that has it (113.7; `triggers.rs::granted_slot`): Tishana's Tidebinder countering a granted ward makes the warded creature lose its abilities | `triggers.rs`; `cr_113_7_a_granted_wards_source_is_the_warded_creature` |
| 27.13.2 | "With a counter on it" (From the Catacombs, Throne of the Dead Three) put the counters right after entering | A | **fixed**: counters and "tapped" from the effect, and the card's own "enters with" counters, are on it before its `Enters` event (614.1c, 122.6; `Game::entering_counters`, `entering_tapped`) | `game.rs` (`put_onto_battlefield`), `resolve_ext.rs`, `stack_ext.rs`; `cr_614_1c_a_creature_enters_with_the_counters_an_effect_gives_it` |
| 27.13.3 | White Plume Adventurer's untap offers only tapped creatures | C | kept (as 23.11.2) | `lower` |
| 27.13.4 | "Tap an untapped Rebel" counts only Rebels with no mana ability | C | kept: no Rebel in the cube has one | `activate.rs` |
| 27.13.5 | One graveyard ability per card; `GY_ABILITY` names the first 12 | B | kept | `decision.rs` |
| 27.13.6 | Undercity is the only dungeon | C | kept: not in the cube | `dungeon.rs` |
| 27.13.7 | The initiative's abilities: controlled by the player they name, a sourceless slot as source | C | kept: 725.2's "no source" (the monarch's now likewise, 724.2) | `dungeon.rs` |
| 28.8.1 | Mana abilities with a mana cost or a choice in their cost: with priority only | A | deferred (payment rework) | `activate.rs`, `pay.rs` |
| 28.8.2 | Converge: an upper bound with more than seven single-mana sources | A | **fixed**: exact for any number (a bipartite matching per colour set, `pay.rs::most_colors_matching`); counting the most colours the player's choices of each dual source's type can show stays (C: more colours is never worse for Prismatic Ending or Pentad Prism). Note for the payment rework: once a payment asks each source's type, `record_converge` should read the types chosen | `pay.rs` (`record_converge`); `cr_106_1b_converge_counts_exactly_with_many_sources` |
| 28.8.3 | Lim-Dul's Vault puts each five on the bottom in the order looked at | C | kept: the library is shuffled at the end | `resolve_tail.rs` |
| 28.8.4 | Painter's Servant's colour before any "becomes [colour]" effect | C | kept: no such effect in the cube | `layers.rs` |
| 28.8.5 | Serra Paragon: a card's own route came first; a card with another face wasn't cast with it | A | **fixed** with 29.6.2 | `cast.rs` (`gy_routes_esc`) |
| 28.8.6 | A permanent's granted leave trigger survived "loses all abilities" | A | **fixed**: not once it lost all abilities (613.1f; grants come as it enters, earlier than any loss) | `game.rs` (`move_card_ctl`); `cr_613_1f_a_granted_trigger_is_lost_with_all_abilities` |
| 28.8.7 | Goblin Welder: a shielded player's artifact isn't sacrificed but the card returns | C | kept: an impossible part of an instruction is ignored (101.3) | `resolve_tail.rs` |
| 29.6.1 | Laelia triggered at most once among the waiting triggers | A | **fixed**: event groups (`Game::ev_seq`: one instruction, one cost, one simultaneous move is one event, 603.2c); a "one or more" trigger fires once per group (`batch_once`) | `triggers.rs`, `resolve.rs` (`run_frame`), `activate.rs`; `cr_603_2c_laelia_triggers_once_per_exile_event` |
| 29.6.2 | Lurrus and Serra Paragon: the first permanent found granted the cast | A | **fixed**: the player chooses the permission (flashback, Yawgmoth's Will, escape, retrace, mayhem, Serra Paragon, Lurrus) when several can complete the cast (601.2a-b): decision `ChooseRoute` (`cast::route` codes); a card is offered if any route can complete | `cast.rs` (`gy_routes_esc`, `choose_route`); `cr_601_2a_*` |
| 29.6.3 | Teamwork excluded creatures with mana abilities; a crew bonus counted for it | A | **fixed**: teamwork's taps are paid before the mana and every choice leaves the mana payable (`crew_mana_ok`); the bonus counts for crew only | `activate.rs`, `cast.rs`; `cr_601_2h_teamwork_can_tap_creatures_with_mana_abilities`, `cr_702_171a_saddle_doesnt_count_a_crew_bonus` |
| 29.6.4 | "After this phase, there is an additional combat phase" during a combat phase only | C | kept: where the cube's card makes it | `resolve_tail2.rs` |
| 29.6.5 | Miracle's reveal asked before the next priority after the draw | C | kept: nothing can happen in between | `resolve_tail2.rs` |
| 29.6.6 | At most 31 activations of one permanent's non-mana ability a turn | B | kept (732.2a loop cap) | `activate.rs` |
| 29.6.7 | "Until the end of your next turn" and "a later turn" count the player's turns, extra turns included | C | kept: an extra turn is a turn | `game.rs` |
| 30.7.1 | Blightsteel Colossus's replacement applied before the others of a graveyard destination | A | **fixed** with 31.7.9: 616.1 | `game.rs` (`replaced_dest`, `choose_replacement`); `cr_616_1_blightsteel_and_yawgmoths_will_the_controller_chooses` |
| 30.7.2 | "Cards put into a library" once among the waiting triggers; discards once per batch | A | **fixed** with 29.6.1 | `triggers.rs`; `cr_603_2c_wan_shi_tong_*`, `cr_603_2c_inti_*` |
| 30.7.3 | Saddle counted a crew bonus | A | **fixed** with 29.6.3 (`Cost::saddle`) | `activate.rs` |
| 30.7.4 | Pentad Prism's mana made with priority only | A | deferred (payment rework) | `activate.rs` |
| 30.7.5 | Shelldock Isle plays a land only on its controller's turn with a land play left | C | kept: that is 305.2-305.3 | `resolve_tail3.rs` |
| 30.7.6 | "Until your next end step" ends as that end step begins; the permission counts the controller's turns, extra turns included | C | kept: as the CR | `game.rs` |
| 30.7.7 | Splice offers each spliced card once per copy in hand, never the spell itself; "distinct targets" not modelled | C | kept: equivalent; none in the cube | `cast.rs` |
| 30.7.8 | A reanimating Aura's attachment legality only for the creature it reanimated | C | kept: equivalent in the cube | `resolve_tail3.rs` |
| 30.7.9 | Necromancy's flash was the flash keyword (ir_defects 8) | A | **fixed**: "as though it had flash" is a permission (`CardDef::flash_permission`, 702.8a); the cleanup sacrifice when cast when a sorcery couldn't have been is unchanged | `lower/printing.rs`, `cast.rs` (`instant_speed`); `cr_702_8a_necromancy_*` |
| 31.7.1 | Arena of Glory's mana with priority only; spent by the pool's rule | A | deferred (payment rework) | `pay.rs` |
| 31.7.2 | Rain of Filth's granted ability while paying only; not with a land's conditioned ability; a land with four abilities gets no id | A / B | deferred (payment rework); the fourth-ability case is the `ACTIVATE` slots (B) | `resolve_m7d.rs`, `pay.rs` |
| 31.7.3 | Emerge's artifact chosen right after the alternative cost | C | kept: the cube's emerge card has no spell targets | `cast.rs` |
| 31.7.4 | Bolas's Citadel offered the top card's front face only | A | **fixed**: either half of a split card, an MDFC's or an Adventure's other face (709.3, 712.11b, 715.3; `ChooseFace`); the top card known before every decision stays (C) | `resolve_m7d.rs` (`top_options`); `cr_709_3_citadel_casts_either_half_of_a_split_card` |
| 31.7.5 | Anointed Peacekeeper names among its opponent's cards | C | kept: naming its controller's own cards is dominated | `resolve_m7d.rs` |
| 31.7.6 | Nadu's limit per creature and granting definition | C | kept: Nadu is legendary | `resolve_m7d.rs` |
| 31.7.7 | "Loses all abilities" also took the face-down turn-face-up action; a copy of a Class gains no levels; no morph, disguise, cloak, face-down spells | A / C | **fixed**: turning face up is a special action (116.2b, 708.7), offered after a loss; the Class copy and morph-likes are not reachable in the cube (C) | `activate.rs` (`activation_options`); `cr_116_2b_*` |
| 31.7.8 | More distinct waiting triggers than 16 offer the first 16 | B | kept: the `TRIGGER` slots | `triggers.rs` |
| 31.7.9 | Dauthi Voidwalker's replacement applied after Yawgmoth's Will's | A | **fixed**: replacement effects of a destination apply in the order the affected object's controller (owner) chooses, each once, until none applies (616.1, 616.1f); decision `ChooseReplacement` when orders give different results (library, exile, exile with a void counter) | `game.rs` (`replaced_dest`, `choose_replacement`); `cr_616_1_*` |
| 32.5.1 | Wizard's Rockets' X mana: one `ChooseMana` per mana; the bot announces X = 0 | C | kept: representation | `pay.rs` |
| 32.5.2 | A "this turn" triggered ability removed as the next turn begins | C | kept: nothing can trigger it in between | `game.rs` |

## Notes on the new mechanisms

* **Event groups** (`Game::ev_seq`, `new_event`): bumped as an instruction of a resolving spell or
  ability ends (`run_frame`), as a resolution begins (`resolve_top`), as a batch begins or ends
  (`begin_batch` / `end_batch`: SBAs, "destroy all", sacrifices, a combat damage step) and as a
  cost starts being paid (`pay_costs`). A "one or more" trigger (`TrigPred::Batch`, "cards put into
  a library") fires once per group. Several instructions of one resolution are separate events;
  one instruction's decisions (Brainstorm's two put-backs, one discard at a time) stay one.
* **616.1** (`replaced_dest`): the results every order of the applicable replacements reaches
  are computed (seven effects, each once); one result applies at once; several make the card go to
  exile for now (its arrival events held back) and `Task::ChooseReplacement` asks the affected
  object's controller (its owner in a hidden zone or graveyard), right after the move and before
  anything else happens; the library result then moves it and shuffles, the void result adds the
  counter. None of these is a self-replacement effect (616.1a).
* **Mox Diamond set aside**: while its controller decides, the card is revealed and in no zone
  (`Zone::Nowhere`, allowed by the invariants only with its pending task). Not given a land, it is
  put into its owner's graveyard from there.
* **Routes** (`cast::route`): codes 0 flashback, 1 Yawgmoth's Will, 2 escape, 3 retrace, 4
  mayhem, 5 Serra Paragon, 6 Lurrus, 7 an effect's permission (alone: the effect offered the card
  that way, a free or cheaper cast is never worse). Two permanents of one kind are offered once.
* **New decision kinds**: `ChooseReplacement` (39) and `ChooseRoute` (40), `NUMBER` ids; 41 kinds
  of the context block's 64. The heuristic bots answer both (`az/bots.rs`: `replacement_choice`,
  `route_choice`).

## Left for the payment rework (group 1)

13.1, 13.2, 28.8.1, 30.7.4, 31.7.1, 31.7.2 (and the converge note in 28.8.2): the pool's mana
choice and mana abilities with a cost or a choice activated during a payment. Nothing in this
branch changed `pay.rs`'s payment model; it added `classes_without` (teamwork's taps before the
mana) and the exact converge count.
