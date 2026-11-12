# The interaction matrix (M8d part 2)

ENGINE.md 38. Every cube card has its own Oracle test (sections 23-32) and the cube's official
rulings are tracked in `docs/rulings.md` (section 37); this is the list of **pairs of mechanics**
that interact under the Comprehensive Rules and are reachable with cube cards, and what tests
each pair. The pairs most likely to hide a fault are those whose two subsystems were written in
different milestones; part 2 wrote its tests (`engine/tests/interactions_*.rs`, `rulings_*2.rs`,
`rulings_casts.rs`) for them first, a few deep tests per risky cell rather than one per cell.
`FUZZ_POOL=interactions` (fuzz.rs, ENGINE.md 38.5) plays random games and matches with decks
built from groups of these cards (one group per family of cells below).

A cell names its tests (`file::test`; **new** = part 2, **fixed** = a fault part 2 found and
fixed, with the test that failed before the fix), or says `n/a` (no cube card reaches it, or
nothing in the rules joins the two) or `untested` (what is left open). Cells already covered by
earlier milestones name their tests too.

## Mechanics

| | Mechanic | CR | Milestone |
|---|---|---|---|
| A | Copies: copy effects, token copies, copies of spells, entering as a copy | 707, 111.10 | M7b part 3 |
| B | Layers: type-, colour-, ability- and P/T-changing effects, loss of abilities | 613 | M7a |
| C | Replacement effects and their order | 614, 616 | M7a, M7e part 1 |
| D | Protection, hexproof, ward, "becomes the target" | 702.11, 702.16, 702.21, 115, 603.2e | M7a, M7b part 3b |
| E | Face-down permanents and cards (manifest dread, cloak, face-down exile) | 708, 701.40, 701.62 | M7c, M7d |
| F | Cards with more than one face: split, MDFC, transforming DFC, Adventure, prepare | 709, 712, 715, 722 | M7b part 2, part 3 |
| G | Planeswalkers | 306, 606, 120.3c | M7b part 1 |
| H | The stack: counterspells, modal, X and divided spells, copies of spells, stealing a spell | 405, 700.2, 601.2d, 707.10, 115.7 | M4, M7b part 3b |
| I | Triggers whose source is gone; last known information | 603.10, 608.2h, 610.3c, 603.7 | M7a, M7e part 1 |
| J | The new object: flicker, reanimation, linked abilities, "until" exile | 400.7, 607, 610.3 | M7a |
| K | Costs: additional, alternative, increases, reductions; free casts | 601.2f-h, 118.9, 107.3b | M7a, M7e |
| L | Mana abilities while paying | 605.3, 601.2g | M7e part 2 |
| M | Combat: first and double strike, trample, deathtouch, protection, damage assignment | 510, 702.2, 702.4, 702.7, 702.19 | M4, M7a |
| N | State-based actions racing triggers | 704, 603.3 | M4 |
| O | Control changes and "you", "its controller", "its owner" | 110.2, 302.6, 603.3a | M7b part 1, part 3 |
| P | Day and night; transforming | 726, 701.27 | M7b part 2 |

## Cells

### A: copies

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| A x A | Metamorph, Reflection of Kiki-Jiki, Saheeli, Fractured Identity | `rules_copies::a_copy_of_a_copy_copies_its_copiable_values_cr_707_3`, `::a_token_copy_of_a_token_copy_keeps_its_exceptions_cr_707_9b`; **new** `rulings_copies2::reflection_copying_a_metamorph_copying_a_bear_makes_that_bear_cr_707_9b`, `::reflection_copying_a_token_uses_what_made_it_cr_707_2_111_4` | tested |
| A x B | Oko's Elk, Saheeli, Tidebinder, animated manlands | `rules_copies::copiable_values_have_no_counters_or_other_effects_cr_707_2`; `rulings_copies::saheeli_copy_of_an_animated_manland_is_a_land_not_a_creature_cr_707_2`, `::saheeli_an_effect_applied_before_the_copy_keeps_applying_cr_613_1`; **new** `interactions_copies::a_copy_of_an_elk_is_the_original_card_not_okos_effect_cr_707_2_613_1a` | tested |
| A x C | Metamorph, Containment Priest, Exhume, Reanimate | `rules_shortcuts::cr_614_12a_metamorph_cant_copy_a_permanent_entering_at_the_same_time`; `rulings_copies::metamorph_copying_walking_ballista_has_x_zero_and_its_enters_with_applies_cr_107_3m_614_1c`; **fixed** `interactions_copies::containment_priest_and_a_reanimated_metamorph_entering_as_an_artifact_cr_614_12_616_1` | tested |
| A x D | True-Name Nemesis, Kappa Cannoneer, Lutri, Chain Lightning | `rules_stack::copies_of_a_spell_become_targets_as_they_are_put_on_the_stack_cr_707_10c`; **new** `interactions_copies::a_metamorph_copying_true_name_nemesis_chooses_its_own_player_cr_707_5_614_12`, `::a_spell_copy_retargeted_at_a_ward_creature_triggers_ward_cr_707_10c_702_21a` | tested |
| A x E | Metamorph, Reflection, Fractured Identity, manifest dread | **new** `interactions_copies::a_copy_of_a_face_down_creature_is_a_face_up_nameless_2_2_cr_707_2_708_2`, `::a_token_copy_of_a_face_down_creature_is_a_face_up_nameless_2_2_cr_707_2_708_2` | tested |
| A x F | Reflection, Metamorph, Emeritus, Ulvenwald Oddity | `rules_copies::a_token_copy_of_a_transformed_permanent_has_its_back_face_up_cr_707_8a`; `cards_m7b_copies2::a_metamorph_copying_emeritus_enters_prepared_cr_722_2b`; **new** `rulings_copies2::being_prepared_isnt_copiable_cr_722_2b_707_2` | tested |
| A x G | Fractured Identity, Saheeli | **new** `interactions_zones::fractured_identity_on_a_planeswalker_gives_a_token_with_its_printed_loyalty_cr_707_2_306_5b` | tested |
| A x H | Lutri, Chain Lightning, Lose Focus, Brain Freeze, Sage of the Skies, Three Steps Ahead | `rules_copies::a_copy_of_a_spell_*` (4); **new** `rulings_stack2::lutris_copy_of_a_modal_spell_has_the_same_modes_cr_707_10`, `::mystic_confluences_copy_may_get_new_targets_not_new_modes_cr_707_10c`, `::three_steps_ahead_copied_keeps_its_modes_and_may_target_anew_cr_702_172_707_10c`, `::lutris_copy_keeps_x_the_division_and_the_cost_paid_new_targets_allowed_cr_707_10_601_2d`, `::lutri_copies_a_spell_without_targets_cr_707_10` | tested |
| A x I | Reflection, Fractured Identity, Superior Spider-Man | `rules_copies::a_token_copy_of_an_exiled_permanent_uses_its_last_known_values_cr_608_2h`; `rulings_copies::superior_spider_man_copys_when_enters_ability_triggers_cr_707_5`; **new** `rulings_copies2::reflections_token_has_the_copied_creatures_enter_ability_cr_707_4_603_6a` | tested |
| A x J | Reflection token, Flickerwisp, Phelia | `rulings_faces::a_copy_of_the_reflection_token_has_haste_and_isnt_sacrificed_cr_707_9b`; `cards_m7b_tests_edges` 111.7 (a flickered token) | tested |
| A x K | Prismatic Ending, Fire Covenant, Lutri | **new** `rulings_casts::prismatic_ending_copied_counts_no_colours_cr_707_10_601_2h`; `rulings_stack2::lutris_copy_keeps_x_the_division_and_the_cost_paid_*` | tested |
| A x L | - | n/a: a copy of a spell is not cast and pays nothing (707.10) | n/a |
| A x M | Reflection (haste), Metamorph | `cards_m7b_copies::reflection_of_kiki_jiki_makes_a_hasty_token_copy_sacrificed_at_end_step_cr_707_9a` | tested |
| A x N | Metamorph or Reflection copying a legend or a 0/0 | `rules_copies::a_token_copy_of_a_legend_meets_the_legend_rule_cr_704_5j`; `rulings_faces::reflection_of_kiki_jiki_copying_walking_ballista_has_x_zero_cr_107_3m` | tested |
| A x O | Fractured Identity, Oko's exchange of a copy | **new** `interactions_zones::fractured_identity_on_a_planeswalker_*` (the token's controller) | tested |
| A x P | Reflection copying a daybound creature at night | **new** (part 3) `interactions_copies::a_token_copy_of_a_nightbound_permanent_is_double_faced_and_turns_at_day_cr_707_8a_702_145f`. Reading (CR 2026-09-25): a token copy of a double-faced permanent is a double-faced token entering with the copied face up (707.8a); a permanent represented by a double-faced token can transform (712.9); nightbound turns it as it becomes day (702.145e-f). 702.145b's "enters transformed" needs a double-faced *card*, but at night a daybound permanent is never front face up to be copied (702.145c). (712.10 in this CR is about transforming into an instant or sorcery face: no cube card reaches it.) | tested |

### B: layers and ability-changing effects

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| B x C | Oko's Elk and "enters with" | n/a: the Elk applies only on the battlefield, after entering | n/a |
| B x D | Tidebinder and granted ward, Lightning Greaves | `rules_ward::a_creature_that_lost_its_abilities_has_no_granted_ward_cr_613_7`; `rulings_stack::tidebinder_an_ability_gained_after_the_loss_is_kept_cr_613_7` | tested |
| B x E | a face-down permanent losing abilities | `rules_shortcuts::cr_116_2b_a_face_down_permanent_that_lost_its_abilities_can_turn_face_up` | tested |
| B x F | Oko on a prepared Emeritus | **new** `rulings_copies2::emeritus_losing_all_abilities_stays_prepared_cr_722_3a_613_1f` | tested |
| B x G | Kaito (a creature on his turn) | `rules_walkers::cr_205_1a_kaito_is_a_creature_not_a_planeswalker_on_his_turn_and_damage_is_marked`; `rulings_walkers::kaito_as_a_creature_is_dealt_damage_not_loyalty_loss_cr_306_8` | tested |
| B x M | Oko's Elk losing trample, Thalia's first strike | `cards_m7b_walkers2::oko_food_elk_and_exchange`, `::oko_an_elked_thalia_taxes_nothing` | tested |
| B x O | Saheeli's artifact that entered this turn becoming a creature | `rulings_copies::saheeli_an_artifact_that_entered_this_turn_and_becomes_a_creature_cant_attack_cr_302_6` | tested |

### C: replacement effects

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| C x C (order) | Hullbreacher, Quantum Riddler, Yawgmoth's Will, Blightsteel, Dauthi | `rulings_replacement::hullbreacher_and_quantum_riddler_*`, `::quantum_riddler_*`; `rules_shortcuts::cr_616_1_*` (2) | tested |
| C x D | Blightsteel's infect and protection | `rulings_replacement::blightsteel_infect_damage_can_be_prevented_by_protection_from_colorless_cr_615_702_16e` | tested |
| C x E | Containment Priest and a manifested card | `rulings_replacement::containment_priest_exiles_a_manifested_card_cr_701_40_614_12` | tested |
| C x H | Green Sun's Zenith countered; Remand on flashback | **new** `rulings_triggers2::green_suns_zenith_countered_goes_to_the_graveyard_cr_701_6a`, `::a_flashback_spell_countered_or_remanded_is_exiled_cr_702_34a` | tested |
| C x I | Dauthi and dies triggers, Shallow Grave | `rulings_replacement::dauthi_an_opponents_creature_is_exiled_and_its_dies_ability_doesnt_trigger_cr_700_4`; **new** `rulings_triggers2::shallow_grave_exiles_only_the_creature_still_on_the_battlefield_cr_603_7_400_7` | tested |
| C x J | Dauthi and a discard; Laelia and "exile until" | **new** `rulings_casts::dauthi_a_discard_exiled_instead_still_triggers_discard_abilities_cr_614_1a`, `::dauthi_occult_epiphany_*`; **fixed** `rulings_triggers2::laelia_cascade_exiles_one_card_at_a_time_one_trigger_each_cr_702_85a_603_2c` | tested |
| C x K | Yawgmoth's Will and costs; a Lotus Petal it exiles | `rulings_replacement::yawgmoths_will_applies_to_costs_cr_614_1a`; **fixed** `interactions_costs::a_lotus_petal_exiled_by_yawgmoths_will_cant_feed_a_graveyard_exile_cost_cr_614_1a_601_2g` | tested |
| C x L | Lotus Petal sacrificed under the Will while paying | the C x K test | tested |
| C x O | Dauthi exiling an opponent's card; the Priest and a stolen creature | `rulings_replacement::dauthi_*` | tested |

### D: protection, ward, targeting

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| D x E | ward or protection on a face-down permanent | n/a: a face-down permanent has no abilities (708.2) | n/a |
| D x G | protection from a planeswalker's colour; targeting a planeswalker | `cards_m7b_walkers_audit` (any-target audit) | tested |
| D x H | protection gained in response; Emrakul on the stack; Leovold | `rulings_protection::a_target_made_illegal_by_protection_makes_the_ability_do_nothing_cr_608_2b`, `::emrakul_on_the_stack_*`; `rules_stack::becoming_the_target_triggers_once_per_spell_cr_603_2e`; **new** `rulings_triggers2::leovold_triggers_once_per_target_of_one_spell_cr_603_2e` | tested |
| D x M | Mother of Runes and trample; True-Name Nemesis blocking | `rulings_protection::protection_from_a_color_stops_blocks_by_that_color_and_its_damage_cr_702_16b_702_16e`, `::true_name_nemesis_*`; **new** `interactions_zones::trample_assigns_lethal_damage_to_a_protected_blocker_before_the_player_cr_702_19c_702_16e` | tested |
| D x O | True-Name's chosen player after a control change | **new** (part 3) `interactions_zones::true_name_nemesis_keeps_its_chosen_player_after_a_control_change_cr_702_16j_110_2` (Oko's exchange gives P0's Nemesis, P1 chosen, to P1: protection from its own controller) | tested |

### E: face-down

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| E x F | a manifested double-faced card | `rulings_facedown::a_manifested_double_faced_card_turns_face_up_front_face_up_cr_701_40e` | tested |
| E x H | a face-down spell | n/a: no cube card casts a card face down | n/a |
| E x J | Flickerwisp on a manifested creature and a manifested instant | **new** `interactions_zones::a_flickered_face_down_creature_returns_face_up_cr_708_9_400_7`; **fixed** `::a_flickered_face_down_instant_cant_return_and_stays_exiled_cr_708_9_400_4a` | tested |
| E x M | a manifested creature turned face up while attacking | `rulings_facedown::turning_face_up_keeps_it_tapped_and_attacking_the_same_permanent_cr_708_8` | tested |
| E x O | Fractured Identity on a face-down permanent | **new** `interactions_zones::fractured_identity_on_a_face_down_permanent_gives_a_nameless_2_2_cr_708_2_707_2` | tested |

### F: faces

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| F x G | a creature transformed into a planeswalker | `rules_faces::a_creature_transformed_into_a_planeswalker_has_no_loyalty_cr_306_5b_704_5i` | tested |
| F x H | split cards and graveyard permissions; Adventures countered; MDFCs | `cards_m7b_faces2` (Adventures); `rulings_faces::a_countered_adventure_*`; **fixed** `interactions_faces::a_split_card_with_two_graveyard_permissions_casts_its_left_half_cr_709_3_601_2b`; `rules_shortcuts::cr_709_3_citadel_casts_either_half_of_a_split_card` | tested |
| F x J | Flickerwisp on a transformed card; Shatterskull's divided damage | **new** `interactions_zones::a_flickered_transformed_card_returns_front_face_up_cr_712_14_400_7`; `rulings_stack2::shatterskull_smashing_*` | tested |
| F x K | an MDFC from the graveyard with Yawgmoth's Will; Paragon and a card with another face | `rulings_faces::yawgmoths_will_lets_you_play_a_modal_double_faced_card_with_either_face_cr_712_12`; `rules_shortcuts::cr_601_2a_paragon_casts_a_card_with_another_face` | tested |
| F x O | Emeritus's Regrowth copy after Oko's exchange | **new** `rulings_copies2::only_the_prepared_creatures_controller_may_cast_its_copy_cr_722_3c` | tested |

### G: planeswalkers

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| G x M | attacking planeswalkers, trample, Coveted Jewel | `rules_walkers::cr_508_1b_*`, `::cr_702_19b_trample_assigns_the_excess_to_the_attacked_planeswalker`, `::cr_510_3a_*`; **new** `rulings_triggers2::coveted_jewel_doesnt_trigger_on_an_attack_at_a_planeswalker_cr_508_1b` | tested |
| G x O | Fractured Identity on a planeswalker | **new** (A x G) | tested |
| G x H | divided damage among creatures and planeswalkers (Fury, Shatterskull) | `cards_m7b_walkers_audit` | tested |

### H: the stack

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| H x H (divided damage, a target gone) | Fury, Pyrokinesis, Fire Covenant, Shatterskull Smashing | **new** `rulings_stack2::fury_*` (2), `::pyrokinesis_*` (2), `::fire_covenant_*` (2), `::shatterskull_smashing_*` | tested |
| H x O (a stolen spell) | Invert Polarity, Green Sun's Zenith | `rules_stack::a_stolen_spell_may_get_new_targets_which_become_targets_cr_115_7`; **new** `rulings_stack2::invert_polarity_changes_targets_not_their_number_or_the_division_cr_115_7_601_2d`, `rulings_triggers2::green_suns_zenith_stolen_its_new_controller_searches_its_owners_library_gets_it_cr_608_2c` | tested |
| H x K | Etali's and the Citadel's casts during resolution and from the library | **new** `rulings_casts::etali_*` (6), `::citadel_*` (4) | tested |
| H x L | Loot paying Mana Leak during resolution | `rules_payment` (605.3a) | tested |
| H x I | a countered delayed trigger; triggers of countered spells | `rulings_stack::a_countered_next_end_step_delayed_trigger_doesnt_trigger_again_cr_603_7c`; `rulings_triggers::chandras_emblem_trigger_resolves_even_if_the_spell_is_countered_cr_603_3` | tested |

### I: sources gone, last known information

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| I x J | Palace Jailer, Leyline Binding, Animate Dead, Flickerwisp, Skyclave, Portable Hole | `rulings_triggers::*_gone_before_*` (4); **new** `rulings_triggers2::palace_jailer_gone_the_creature_returns_when_an_opponent_becomes_the_monarch_cr_610_3c`, `::leyline_binding_gone_before_its_enter_ability_resolves_exiles_nothing_cr_610_3c`, `::animate_dead_gone_before_its_trigger_resolves_returns_nothing_cr_608_2b`, `::flickerwisp_gone_its_exiled_permanent_still_returns_at_the_end_step_cr_603_7` | tested |
| I x M | Screaming Nemesis blocked by two creatures | **fixed** `rulings_triggers2::screaming_nemesis_dealt_damage_by_two_blockers_triggers_once_cr_603_2c_510_2` | tested |
| I x N | dies triggers and SBAs; Pyrogoyf | `rules_triggers::cr_603_3b_apnap_order_and_cr_603_10a_dies_triggers_look_back`; `rules_shortcuts::cr_608_2h_pyrogoyfs_creature_deals_damage_as_it_last_existed` | tested |
| I x O | Dack's control after Dack leaves; a stolen Laelia's triggers | `rulings_triggers::dacks_control_change_outlasts_dack_cr_611_2a` | tested |

### J: the new object

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| J x J | Leyline Binding flickered | **new** `interactions_zones::leyline_binding_flickered_returns_its_card_and_exiles_anew_cr_610_3_400_7` | tested |
| J x O | an Animate Dead creature flickered; Palace Jailer's token | **new** `interactions_zones::a_reanimated_creature_flickered_returns_under_its_owner_and_the_aura_is_gone_cr_400_7_704_5m`, `rulings_triggers2::palace_jailer_exiling_a_token_it_ceases_to_exist_cr_111_7` | tested |
| J x K | Uro reanimated (not escaped); flashback exile | **new** `rulings_triggers2::uro_put_onto_the_battlefield_is_sacrificed_and_its_enter_ability_triggers_cr_603_6a`, `::a_flashback_spell_countered_or_remanded_is_exiled_cr_702_34a` | tested |
| J x M | a creature flickered out of combat | `rules_combat::cr_506_4_blocked_creature_stays_blocked_after_its_blocker_leaves` | tested |

### K, L: costs

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| K x K (free casts, alternative costs, X) | Etali, Citadel, Dauthi, Force of Will, Solitude, Abhorrent Oculus, Walking Ballista | **new** `rulings_casts::etali_a_free_x_spell_has_x_zero_cr_107_3b`, `::etali_no_alternative_cost_but_mandatory_additional_costs_cr_118_9a_601_2f`, `::citadel_*`, `::prismatic_ending_cast_free_counts_no_colours_cr_601_2h` | tested |
| K x K (taxes) | Thalia with Etali's and the Citadel's casts | `rulings_mana::thalias_tax_*` (2); **new** `interactions_costs::etali_free_casts_pay_thalias_tax_cr_601_2f_118_9d`, `::citadel_casts_pay_life_and_thalias_tax_in_mana_cr_118_9d_601_2f` | tested |
| K x L | the Citadel's life and Mana Confluence's life for a spree cost | **fixed** `interactions_costs::citadel_spree_mode_costs_keep_the_life_the_citadel_takes_cr_601_2h_702_172a`; `rules_payment` (14) | tested |

### M, N: combat and state-based actions

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| M x M | first strike, double strike, trample, deathtouch | `rules_combat` (11) | tested |
| M x N | lethal damage, deathtouch, 0 toughness after combat | `rules_sba::cr_704_5g_*`, `::cr_704_5h_*` | tested |
| N x N | the legend rule, 0 loyalty | `rules_sba::cr_704_5j_*`, `rules_walkers::cr_704_5j_*` | tested |

### O, P

| Pair | Cube cards | Tests | Status |
|---|---|---|---|
| O x M | an exchanged creature's summoning sickness | **new** `interactions_zones::an_exchanged_creature_is_summoning_sick_and_its_controller_gains_the_life_cr_302_6_110_2` | tested |
| O x O | Oko's exchange, Coveted Jewel, Thieving Skydiver | `cards_m7b_walkers2::oko_food_elk_and_exchange`; `cards_m7c_tail3::thieving_skydiver_kicker_x_steals_and_attaches_an_equipment_cr702_33d` | tested |
| P x P | day and night, daybound cast or put at night | `rulings_faces::a_daybound_card_*` (2), `::it_becomes_night_*`; `rules_faces::a_daybound_permanent_transforms_only_through_day_and_night_cr_702_145b` | tested |
| P x J | a flickered transformed card | (F x J) | tested |

## Untested cells

None. Part 3 (ENGINE.md 39) tested the last two, A x P (a token copy of a daybound card at night)
and D x O (True-Name Nemesis's chosen player after a control change). All pairs above are tested
or not applicable.

## Legacy Sneak and Show (ENGINE.md 123)

Constructed play's table (`CardDb::constructed`); every test in `engine/tests/cards_sneak_show.rs`.

| Pair | Cards | Tests | Status |
|---|---|---|---|
| Show and Tell x a big creature, the other player answering | Show and Tell, Emrakul, Griselbrand, Atraxa | `cr_101_4_show_and_tell_the_active_player_chooses_then_the_other_and_both_enter`, `cr_603_3b_show_and_tell_enters_together_and_triggers_go_on_the_stack_in_apnap_order`, `cr_603_2_a_card_put_onto_the_battlefield_is_not_cast` | tested |
| Show and Tell x Omniscience | Show and Tell, Omniscience, Emrakul | `show_and_tell_into_omniscience_then_free_spells` | tested |
| Show and Tell x hidden information | the face-down choice | `cr_101_4a_the_other_player_knows_only_whether_a_card_was_chosen`, `cr_101_4a_the_other_players_worlds_deal_a_legal_face_down_choice`, `cr_101_4a_a_known_hand_is_one_pool_the_choice_among_its_cards_of_the_kind` | tested |
| Show and Tell x "as enters" | two Disruptor Flutes; Mistrise Village with the Magus | `show_and_tell_the_active_players_enters_choices_come_first`, `cr_614_12a_mistrise_village_entering_with_the_magus_enters_tapped` | tested |
| Sneak Attack x Emrakul | haste, the end step's sacrifice, the graveyard shuffle | `sneak_attack_emrakul_haste_end_step_sacrifice_and_the_graveyard_shuffle` | tested |
| Omniscience x Emrakul | the cast trigger's extra turn | `cr_603_2_emrakul_cast_with_omniscience_takes_an_extra_turn` | tested |
| Omniscience x Force of Will | two alternative costs, one at a time | `cr_118_9a_omniscience_and_force_of_will_are_alternative_costs_one_at_a_time` | tested |
| Omniscience x costs | X, kicker, Tinker's sacrifice, Thalia, the Flute | `cr_107_3b_omniscience_an_x_spell_has_x_0`, `cr_601_2f_omniscience_additional_costs_are_still_paid`, `cr_601_2f_omniscience_cost_increases_still_apply`, `cr_601_2f_disruptor_flute_naming_show_and_tell_under_omniscience_costs_3` | tested |
| Mistrise Village x counterspells | Force of Will, Daze | `cr_101_2_mistrise_village_the_next_spell_cant_be_countered`, `cr_118_12_daze_on_an_uncounterable_spell_its_payment_is_still_offered` | tested |
| Disruptor Flute x Show and Tell | the {3} with and without Omniscience | `cr_601_2f_disruptor_flute_spells_with_the_name_cost_3_more_every_players`, `cr_601_2f_disruptor_flute_naming_show_and_tell_under_omniscience_costs_3` | tested |
| Disruptor Flute x activated abilities | Sneak Attack, a fetch land, Lotus Petal, Simian Spirit Guide, Mistrise Village, Tamiyo's back face | `cr_602_5_*`, `cr_605_1a_*`, `cr_606_*` | tested |
| Magus of the Moon x the deck's mana | Volcanic Island, Thundering Falls, Mistrise Village, Ancient Tomb, City of Traitors, fetch lands, Multiversal Passage, Gemstone Mine | `cr_305_7_*`, `cr_614_12_*` | tested |
| Turn-one Show and Tell | Lotus Petal, City of Traitors, Ancient Tomb | `lotus_petal_spirit_guide_and_city_of_traitors_into_a_turn_one_show_and_tell` | tested |

## The Legacy Izzet Delver 75's sideboard (ENGINE.md 122)

Constructed play's table (`CardDb::constructed`); every test in `engine/tests/cards_delver_sb.rs`.

| Pair | Cards | Tests | Status |
|---|---|---|---|
| Consign to Memory x delayed triggers | Pact of Negation's upkeep payment | `cr_701_6a_consign_to_memory_counters_pact_of_negations_upkeep_trigger` | tested |
| Consign to Memory x an uncounterable spell | Emrakul, the Aeons Torn and its cast trigger (Omniscience) | `cr_101_2_consign_to_memory_and_emrakuls_cast_trigger` | tested |
| Consign to Memory x keyword triggers | ward (Kappa Cannoneer), storm (Flusterstorm) | `cr_702_21a_consign_to_memory_counters_a_ward_trigger`, `cr_702_40a_consign_to_memory_counters_a_storm_trigger` | tested |
| Consign to Memory's replicate x counterspells | Daze countering the original, the copy resolving | `cr_707_10_countering_consign_to_memory_leaves_its_copy`, `cr_702_56a_consign_to_memory_replicated_counters_two_triggers` | tested |
| Consign to Memory / Hydroblast x colour changes | Painter's Servant naming red | `cr_105_2c_a_spell_made_red_by_painters_servant_is_not_colorless`, `cr_613_1e_hydroblast_reads_painters_servants_red` | tested |
| Hydroblast x Misdirection | the mode kept on a redirect | `cr_700_2a_a_redirected_hydroblast_keeps_its_mode` | tested |
| Hydroblast x a split card | Rough on the stack (red) | `cr_709_3b_hydroblast_counters_rough` | tested |
| Meltdown x X | Omniscience (X 0), Walking Ballista, Kaldra Compleat | `cr_107_3b_meltdown_cast_for_free_has_x_zero`, `cr_202_3e_meltdown_for_zero_destroys_walking_ballista_and_the_zero_artifacts`, `cr_702_12b_meltdown_leaves_an_indestructible_artifact` | tested |
| Price of Progress x the lands | Magus of the Moon, a fetch land in response | `cr_305_7_nonbasic_lands_under_magus_of_the_moon_still_count`, `cr_608_2h_price_of_progress_counts_the_lands_as_it_resolves` | tested |
| Rough // Tumble x flying now | Dragon's Rage Channeler's delirium | `cr_613_1f_rough_and_tumble_read_flying_as_it_is_now` | tested |
| Rough // Tumble x copies, names, counts | Lutri, Disruptor Flute, Murktide Regent, Inquisition of Kozilek | `cr_707_10_a_copy_of_rough_is_rough`, `cr_709_4a_disruptor_flute_naming_rough_taxes_rough_only`, `cr_709_4_rough_tumble_is_one_card_for_murktide_regent`, `cr_202_3d_rough_tumble_in_hand_has_mana_value_eight` | tested |
