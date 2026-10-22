//! `CardPrinting` -> `CardDef`. Every field of the generated struct is named
//! below; fields the engine does not implement yet are `deny`-ed (a blocker
//! when not at their default). Supporting a flag = moving it out of the
//! deny list and implementing it.

use super::activated::{lower_activated, prod_mask, From};
use super::effect::lower_ability;
use super::filter::{lower_card_filter, lower_condition};
use super::Lowerer;
use crate::db::{
    supertypes, types, ActEffect, ActivatedDef, CardDef, Cost, EntersTapped, EventKind, Filter, Kw, KwSet,
    ManaAbility, ManaProd, Mod, Prot, SacCost, Scope, Slot, StaticDef, TrigPred, TriggerDef,
};
use crate::mana::{letter_index, pair_index, ManaCost, ManaMask};
use card_ir::{cats, enums, generated, nodes};

pub(crate) fn lower_printing(l: &mut Lowerer, p: &nodes::CardPrinting) -> CardDef {
        let nodes::CardPrinting {
            name,
            mana_cost_str,
            mana_cost,
            type_line,
            supertypes,
            card_types,
            subtypes,
            power,
            toughness,
            produces,
            keywords,
            triggered_abilities,
            spell_effect,
            warp_cost,
            warp_cost_str,
            warp_additional_life_cost,
            warp_castable_from_graveyard,
            impending_cost,
            impending_cost_str,
            impending_count,
            kicker_cost,
            kicker_cost_str,
            loyalty,
            devour_land_multiplier,
            station_threshold,
            stationed_keywords,
            chapter_count,
            ward_cost,
            ward_cost_str,
            attached_grants_ward_cost,
            attached_grants_ward_str,
            grants_additional_land_per_turn,
            grants_play_lands_from_graveyard,
            attached_prevents_untap,
            attached_grants_control,
            attached_sets_base_pt,
            attached_grants_lose_all_abilities,
            attached_grants_subtype,
            attached_grants_card_types,
            attached_prevents_turning_face_up,
            attached_grants_activated_abilities,
            enters_as_copy_of_creature,
            replaces_first_tokens_with_copies,
            grants_look_at_top_of_library,
            grants_cast_artifact_from_top_of_library,
            grants_first_spell_each_turn_free,
            grants_food_to_your_artifacts,
            grants_food_ability_to_your_artifacts,
            grants_blue_mana_ability_to_your_artifacts,
            hand_ability_grants,
            grants_control_opponent_ability_when_equipped,
            ward_discard_random,
            ward_discard_chosen,
            ward_life_cost,
            equipment_bonuses,
            shock_land_life_cost,
            enters_tapped,
            doesnt_untap_during_untap_step,
            enters_only_by_discarding_land,
            may_skip_turn_to_untap,
            equipment_bonus_per_controlled,
            crew_power_bonus,
            has_graveyard_triggers,
            sunburst,
            grants_play_from_top_paying_life,
            equipment_granted_triggers,
            enters_tapped_unless_any_player_life_at_most,
            optional_copy_land_from_graveyard,
            enters_tapped_unless,
            mana_only_from_produces,
            produces_life_cost,
            produces_with_counter,
            opening_hand_action,
            hand_activated_abilities,
            enters_choosing_basic_land_type,
            is_chosen_basic_land_type,
            enters_with_named_counters,
            additional_cost_sacrifice_creature_or_artifact,
            additional_cost_sacrifice_creature_or_enchantment,
            additional_cost_choose_your_creature,
            cost_reductions,
            external_cost_reductions,
            static_abilities,
            activated_abilities,
            enters_with_counters_from_x,
            grants_etb_counter_per_land_entered_this_turn,
            enters_with_plus_one_counter_if_creature_died_this_turn,
            enters_with_minus_one_counters_from_hand_cast,
            aura_enchant_type,
            aura_yours_only,
            dynamic_pt_formula,
            restricts_noncreature_spells_to_one_per_turn,
            opponents_cant_draw_more_than_one_card_each_turn,
            opponents_cast_spells_only_at_sorcery_speed,
            loyalty_abilities_at_instant_speed_if_entered_this_turn,
            nonbasic_lands_are_mountains,
            grants_all_basic_land_types_to_lands_you_control,
            grants_basic_land_type_to_all_lands,
            paints_all_objects_chosen_color_at_etb,
            all_permanents_gain_card_type,
            all_objects_colorless,
            mana_spendable_as_any_color,
            chooses_player_as_enters,
            looks_at_opponents_face_down_creatures,
            wipes_all_creature_abilities,
            sets_all_creature_pt_to_1_1,
            must_be_blocked_if_able,
            spell_is_uncounterable,
            combat_damage_cant_be_prevented,
            grants_toughness_as_damage_for_creatures_you_control,
            grants_toughness_as_station_power_for_creatures_you_control,
            protection_from_color,
            color_indicator,
            has_convoke,
            can_only_block_creatures_with_flying,
            landcycling,
            flashback_cost,
            flashback_cost_str,
            has_delve,
            buyback_cost,
            buyback_cost_str,
            additional_cost_sacrifice_filter,
            additional_cost_pay_x_life,
            free_cast_condition,
            has_storm,
            alt_cost_exile_from_hand,
            alt_cost_exile_from_hand_count,
            splice_onto,
            splice_cost,
            splice_cost_str,
            mdfc_back_face,
            transform_back_face,
            face_colors,
            daybound,
            nightbound,
            class_level,
            class_levels,
            split_faces,
            grants_mayhem_to_graveyard,
            adventure_face,
            prepare_spell,
            enters_prepared,
            plot_cost,
            plot_cost_str,
            escape_cost,
            escape_cost_str,
            escape_exile_count,
            grants_graveyard_escape_exile_count,
            sacrifice_at_cleanup_if_cast_without_sorcery_timing,
            graveyard_activated_abilities,
            dash_cost,
            dash_cost_str,
            heals_other_damage_when_dealt_damage,
            watched_in_graveyard,
            watches_library_graveyard_exile,
            multikicker,
            enters_with_counter_per_kick,
            evoke_cost,
            evoke_cost_str,
            evoke_exile_from_hand,
            opponents_cant_cast_spells_during_your_turn,
            each_player_one_spell_per_turn,
            opponents_nonbasic_lands_enter_tapped,
            spell_cost_increases,
            cant_be_blocked_by,
            your_creatures_combat_damage_cant_be_prevented,
            locks_activated_abilities_of,
            locks_opponents_activated_abilities_of,
            exiles_uncast_nontoken_creatures_entering,
            grants_retrace_to_nonland_permanents_during_your_turn,
            grants_once_per_turn_graveyard_play_mv,
            grants_retrace_to,
            opponents_cant_make_you_discard_or_sacrifice,
            extra_mana_when_you_tap_land_subtype,
            creature_during_your_turn,
            chooses_card_name_as_enters,
            named_spell_cost_increase,
            named_ability_cost_increase,
            reconfigure_cost,
            reconfigure_cost_str,
            equipment_bonus_per_own_counter,
            bestow_cost,
            bestow_cost_str,
            bestow_enchanted_bonus,
            extra_mana_when_you_tap_creature_for_mana,
            min_blockers,
            squad_cost,
            squad_cost_str,
            offspring_cost,
            offspring_cost_str,
            has_improvise,
            void_exile_replacement,
            zone_abilities,
            emerge_cost,
            emerge_cost_str,
            emerge_sacrifice_type,
            miracle_cost,
            miracle_cost_str,
            additional_cost_exile_from_graveyard,
            artifact_ability_cost_reduction,
            chooses_parity_as_enters,
            x_min,
            blocks_only_with_subtype,
            enters_as_copy_types,
            enters_as_copy_zone,
            enters_as_copy_add_type,
            enters_as_copy_keep_name,
            enters_as_copy_base_pt,
            enters_as_copy_add_subtypes,
            enters_as_copy_exiles_card,
            enters_as_copy_add_triggers,
            enters_as_copy_add_keywords,
            enters_as_copy_max_mv_mana_spent,
            rebound,
            room_halves,
            alternative_costs,
            additional_cost_options,
            replicate_cost,
            teamwork_power,
            roto_statics,
            enters_choosing_creature_type,
            cast_from_top_of_library_kinds,
            suspend_count,
            suspend_cost,
            suspend_cost_str,
            has_no_mana_cost,
            exile_activated_abilities,
            aura_enchant_subtype,
            enters_choosing_color,
            enchanted_land_adds_chosen_color,
            additional_cost_discard_x,
            optional_sacrifice_any_number,
            grants_free_casts_from_hand,
            free_flash_creature_casts_max_mv,
            grants_discard_alternative_cost,
            tapped_for_mana_triggers,
            extra_mana_when_you_tap_nonland_for_mana,
            board_rules,
            escape_exile_card_types,
            conspiracy_rules,
            replacement_statics,
        } = p;
        l.deny("CardPrinting.warp_cost", warp_cost);
        l.deny("CardPrinting.warp_cost_str", warp_cost_str);
        l.deny("CardPrinting.warp_additional_life_cost", warp_additional_life_cost);
        l.deny("CardPrinting.warp_castable_from_graveyard", warp_castable_from_graveyard);
        l.deny("CardPrinting.impending_cost", impending_cost);
        l.deny("CardPrinting.impending_cost_str", impending_cost_str);
        l.deny("CardPrinting.impending_count", impending_count);
        l.deny("CardPrinting.kicker_cost", kicker_cost);
        l.deny("CardPrinting.kicker_cost_str", kicker_cost_str);
        l.deny("CardPrinting.loyalty", loyalty);
        l.deny("CardPrinting.devour_land_multiplier", devour_land_multiplier);
        l.deny("CardPrinting.station_threshold", station_threshold);
        l.deny("CardPrinting.stationed_keywords", stationed_keywords);
        l.deny("CardPrinting.chapter_count", chapter_count);
        l.deny("CardPrinting.ward_cost", ward_cost);
        l.deny("CardPrinting.ward_cost_str", ward_cost_str);
        l.deny("CardPrinting.attached_grants_ward_cost", attached_grants_ward_cost);
        l.deny("CardPrinting.attached_grants_ward_str", attached_grants_ward_str);
        l.deny("CardPrinting.grants_additional_land_per_turn", grants_additional_land_per_turn);
        l.deny("CardPrinting.grants_play_lands_from_graveyard", grants_play_lands_from_graveyard);
        l.deny("CardPrinting.attached_prevents_untap", attached_prevents_untap);
        l.deny("CardPrinting.attached_grants_control", attached_grants_control);
        l.deny("CardPrinting.attached_sets_base_pt", attached_sets_base_pt);
        l.deny("CardPrinting.attached_grants_lose_all_abilities", attached_grants_lose_all_abilities);
        l.deny("CardPrinting.attached_grants_subtype", attached_grants_subtype);
        l.deny("CardPrinting.attached_grants_card_types", attached_grants_card_types);
        l.deny("CardPrinting.attached_prevents_turning_face_up", attached_prevents_turning_face_up);
        l.deny("CardPrinting.attached_grants_activated_abilities", attached_grants_activated_abilities);
        l.deny("CardPrinting.enters_as_copy_of_creature", enters_as_copy_of_creature);
        l.deny("CardPrinting.replaces_first_tokens_with_copies", replaces_first_tokens_with_copies);
        l.deny("CardPrinting.grants_look_at_top_of_library", grants_look_at_top_of_library);
        l.deny("CardPrinting.grants_cast_artifact_from_top_of_library", grants_cast_artifact_from_top_of_library);
        l.deny("CardPrinting.grants_first_spell_each_turn_free", grants_first_spell_each_turn_free);
        l.deny("CardPrinting.grants_food_to_your_artifacts", grants_food_to_your_artifacts);
        l.deny("CardPrinting.grants_food_ability_to_your_artifacts", grants_food_ability_to_your_artifacts);
        l.deny("CardPrinting.grants_blue_mana_ability_to_your_artifacts", grants_blue_mana_ability_to_your_artifacts);
        l.deny("CardPrinting.hand_ability_grants", hand_ability_grants);
        l.deny("CardPrinting.grants_control_opponent_ability_when_equipped", grants_control_opponent_ability_when_equipped);
        l.deny("CardPrinting.ward_discard_random", ward_discard_random);
        l.deny("CardPrinting.ward_discard_chosen", ward_discard_chosen);
        l.deny("CardPrinting.ward_life_cost", ward_life_cost);
        l.deny("CardPrinting.enters_only_by_discarding_land", enters_only_by_discarding_land);
        l.deny("CardPrinting.may_skip_turn_to_untap", may_skip_turn_to_untap);
        l.deny("CardPrinting.equipment_bonus_per_controlled", equipment_bonus_per_controlled);
        l.deny("CardPrinting.crew_power_bonus", crew_power_bonus);
        l.deny("CardPrinting.has_graveyard_triggers", has_graveyard_triggers);
        l.deny("CardPrinting.sunburst", sunburst);
        l.deny("CardPrinting.grants_play_from_top_paying_life", grants_play_from_top_paying_life);
        l.deny("CardPrinting.equipment_granted_triggers", equipment_granted_triggers);
        l.deny("CardPrinting.enters_tapped_unless_any_player_life_at_most", enters_tapped_unless_any_player_life_at_most);
        l.deny("CardPrinting.optional_copy_land_from_graveyard", optional_copy_land_from_graveyard);
        l.deny("CardPrinting.produces_with_counter", produces_with_counter);
        l.deny("CardPrinting.opening_hand_action", opening_hand_action);
        l.deny("CardPrinting.enters_choosing_basic_land_type", enters_choosing_basic_land_type);
        l.deny("CardPrinting.is_chosen_basic_land_type", is_chosen_basic_land_type);
        l.deny("CardPrinting.additional_cost_sacrifice_creature_or_artifact", additional_cost_sacrifice_creature_or_artifact);
        l.deny("CardPrinting.additional_cost_sacrifice_creature_or_enchantment", additional_cost_sacrifice_creature_or_enchantment);
        l.deny("CardPrinting.additional_cost_choose_your_creature", additional_cost_choose_your_creature);
        l.deny("CardPrinting.cost_reductions", cost_reductions);
        l.deny("CardPrinting.external_cost_reductions", external_cost_reductions);
        l.deny("CardPrinting.enters_with_counters_from_x", enters_with_counters_from_x);
        l.deny("CardPrinting.grants_etb_counter_per_land_entered_this_turn", grants_etb_counter_per_land_entered_this_turn);
        l.deny("CardPrinting.enters_with_plus_one_counter_if_creature_died_this_turn", enters_with_plus_one_counter_if_creature_died_this_turn);
        l.deny("CardPrinting.enters_with_minus_one_counters_from_hand_cast", enters_with_minus_one_counters_from_hand_cast);
        l.deny_ne("CardPrinting.aura_enchant_type", aura_enchant_type, &generated::defaults::d_CardPrinting_aura_enchant_type());
        l.deny("CardPrinting.aura_yours_only", aura_yours_only);
        l.deny("CardPrinting.restricts_noncreature_spells_to_one_per_turn", restricts_noncreature_spells_to_one_per_turn);
        l.deny("CardPrinting.opponents_cant_draw_more_than_one_card_each_turn", opponents_cant_draw_more_than_one_card_each_turn);
        l.deny("CardPrinting.opponents_cast_spells_only_at_sorcery_speed", opponents_cast_spells_only_at_sorcery_speed);
        l.deny("CardPrinting.loyalty_abilities_at_instant_speed_if_entered_this_turn", loyalty_abilities_at_instant_speed_if_entered_this_turn);
        l.deny("CardPrinting.nonbasic_lands_are_mountains", nonbasic_lands_are_mountains);
        l.deny("CardPrinting.grants_all_basic_land_types_to_lands_you_control", grants_all_basic_land_types_to_lands_you_control);
        l.deny("CardPrinting.grants_basic_land_type_to_all_lands", grants_basic_land_type_to_all_lands);
        l.deny("CardPrinting.paints_all_objects_chosen_color_at_etb", paints_all_objects_chosen_color_at_etb);
        l.deny("CardPrinting.all_permanents_gain_card_type", all_permanents_gain_card_type);
        l.deny("CardPrinting.all_objects_colorless", all_objects_colorless);
        l.deny("CardPrinting.mana_spendable_as_any_color", mana_spendable_as_any_color);
        l.deny("CardPrinting.chooses_player_as_enters", chooses_player_as_enters);
        l.deny("CardPrinting.looks_at_opponents_face_down_creatures", looks_at_opponents_face_down_creatures);
        l.deny("CardPrinting.wipes_all_creature_abilities", wipes_all_creature_abilities);
        l.deny("CardPrinting.sets_all_creature_pt_to_1_1", sets_all_creature_pt_to_1_1);
        l.deny("CardPrinting.must_be_blocked_if_able", must_be_blocked_if_able);
        l.deny("CardPrinting.combat_damage_cant_be_prevented", combat_damage_cant_be_prevented);
        l.deny("CardPrinting.grants_toughness_as_damage_for_creatures_you_control", grants_toughness_as_damage_for_creatures_you_control);
        l.deny("CardPrinting.grants_toughness_as_station_power_for_creatures_you_control", grants_toughness_as_station_power_for_creatures_you_control);
        l.deny("CardPrinting.has_convoke", has_convoke);
        l.deny("CardPrinting.flashback_cost", flashback_cost);
        l.deny("CardPrinting.flashback_cost_str", flashback_cost_str);
        l.deny("CardPrinting.has_delve", has_delve);
        l.deny("CardPrinting.buyback_cost", buyback_cost);
        l.deny("CardPrinting.buyback_cost_str", buyback_cost_str);
        l.deny("CardPrinting.additional_cost_pay_x_life", additional_cost_pay_x_life);
        l.deny("CardPrinting.free_cast_condition", free_cast_condition);
        l.deny("CardPrinting.has_storm", has_storm);
        l.deny("CardPrinting.alt_cost_exile_from_hand", alt_cost_exile_from_hand);
        l.deny_ne("CardPrinting.alt_cost_exile_from_hand_count", alt_cost_exile_from_hand_count, &generated::defaults::d_CardPrinting_alt_cost_exile_from_hand_count());
        l.deny("CardPrinting.splice_onto", splice_onto);
        l.deny("CardPrinting.splice_cost", splice_cost);
        l.deny("CardPrinting.splice_cost_str", splice_cost_str);
        l.deny("CardPrinting.mdfc_back_face", mdfc_back_face);
        l.deny("CardPrinting.transform_back_face", transform_back_face);
        l.deny("CardPrinting.face_colors", face_colors);
        l.deny("CardPrinting.daybound", daybound);
        l.deny("CardPrinting.nightbound", nightbound);
        l.deny("CardPrinting.class_level", class_level);
        l.deny("CardPrinting.class_levels", class_levels);
        l.deny("CardPrinting.split_faces", split_faces);
        l.deny("CardPrinting.grants_mayhem_to_graveyard", grants_mayhem_to_graveyard);
        l.deny("CardPrinting.adventure_face", adventure_face);
        l.deny("CardPrinting.prepare_spell", prepare_spell);
        l.deny("CardPrinting.enters_prepared", enters_prepared);
        l.deny("CardPrinting.plot_cost", plot_cost);
        l.deny("CardPrinting.plot_cost_str", plot_cost_str);
        l.deny("CardPrinting.escape_cost", escape_cost);
        l.deny("CardPrinting.escape_cost_str", escape_cost_str);
        l.deny("CardPrinting.escape_exile_count", escape_exile_count);
        l.deny("CardPrinting.grants_graveyard_escape_exile_count", grants_graveyard_escape_exile_count);
        l.deny("CardPrinting.sacrifice_at_cleanup_if_cast_without_sorcery_timing", sacrifice_at_cleanup_if_cast_without_sorcery_timing);
        l.deny("CardPrinting.graveyard_activated_abilities", graveyard_activated_abilities);
        l.deny("CardPrinting.dash_cost", dash_cost);
        l.deny("CardPrinting.dash_cost_str", dash_cost_str);
        l.deny("CardPrinting.watched_in_graveyard", watched_in_graveyard);
        l.deny("CardPrinting.watches_library_graveyard_exile", watches_library_graveyard_exile);
        l.deny("CardPrinting.multikicker", multikicker);
        l.deny("CardPrinting.enters_with_counter_per_kick", enters_with_counter_per_kick);
        l.deny("CardPrinting.evoke_cost", evoke_cost);
        l.deny("CardPrinting.evoke_cost_str", evoke_cost_str);
        l.deny("CardPrinting.evoke_exile_from_hand", evoke_exile_from_hand);
        l.deny("CardPrinting.opponents_cant_cast_spells_during_your_turn", opponents_cant_cast_spells_during_your_turn);
        l.deny("CardPrinting.each_player_one_spell_per_turn", each_player_one_spell_per_turn);
        l.deny("CardPrinting.opponents_nonbasic_lands_enter_tapped", opponents_nonbasic_lands_enter_tapped);
        l.deny("CardPrinting.your_creatures_combat_damage_cant_be_prevented", your_creatures_combat_damage_cant_be_prevented);
        l.deny("CardPrinting.locks_opponents_activated_abilities_of", locks_opponents_activated_abilities_of);
        l.deny("CardPrinting.grants_retrace_to_nonland_permanents_during_your_turn", grants_retrace_to_nonland_permanents_during_your_turn);
        l.deny("CardPrinting.grants_once_per_turn_graveyard_play_mv", grants_once_per_turn_graveyard_play_mv);
        l.deny("CardPrinting.grants_retrace_to", grants_retrace_to);
        l.deny("CardPrinting.opponents_cant_make_you_discard_or_sacrifice", opponents_cant_make_you_discard_or_sacrifice);
        l.deny("CardPrinting.extra_mana_when_you_tap_land_subtype", extra_mana_when_you_tap_land_subtype);
        l.deny("CardPrinting.creature_during_your_turn", creature_during_your_turn);
        l.deny("CardPrinting.chooses_card_name_as_enters", chooses_card_name_as_enters);
        l.deny("CardPrinting.named_spell_cost_increase", named_spell_cost_increase);
        l.deny("CardPrinting.named_ability_cost_increase", named_ability_cost_increase);
        l.deny("CardPrinting.reconfigure_cost", reconfigure_cost);
        l.deny("CardPrinting.reconfigure_cost_str", reconfigure_cost_str);
        l.deny("CardPrinting.equipment_bonus_per_own_counter", equipment_bonus_per_own_counter);
        l.deny("CardPrinting.bestow_cost", bestow_cost);
        l.deny("CardPrinting.bestow_cost_str", bestow_cost_str);
        l.deny("CardPrinting.bestow_enchanted_bonus", bestow_enchanted_bonus);
        l.deny("CardPrinting.extra_mana_when_you_tap_creature_for_mana", extra_mana_when_you_tap_creature_for_mana);
        l.deny("CardPrinting.squad_cost", squad_cost);
        l.deny("CardPrinting.squad_cost_str", squad_cost_str);
        l.deny("CardPrinting.offspring_cost", offspring_cost);
        l.deny("CardPrinting.offspring_cost_str", offspring_cost_str);
        l.deny("CardPrinting.has_improvise", has_improvise);
        l.deny("CardPrinting.void_exile_replacement", void_exile_replacement);
        l.deny("CardPrinting.zone_abilities", zone_abilities);
        l.deny("CardPrinting.emerge_cost", emerge_cost);
        l.deny("CardPrinting.emerge_cost_str", emerge_cost_str);
        l.deny_ne("CardPrinting.emerge_sacrifice_type", emerge_sacrifice_type, &generated::defaults::d_CardPrinting_emerge_sacrifice_type());
        l.deny("CardPrinting.miracle_cost", miracle_cost);
        l.deny("CardPrinting.miracle_cost_str", miracle_cost_str);
        l.deny("CardPrinting.additional_cost_exile_from_graveyard", additional_cost_exile_from_graveyard);
        l.deny("CardPrinting.artifact_ability_cost_reduction", artifact_ability_cost_reduction);
        l.deny("CardPrinting.chooses_parity_as_enters", chooses_parity_as_enters);
        l.deny("CardPrinting.x_min", x_min);
        l.deny("CardPrinting.blocks_only_with_subtype", blocks_only_with_subtype);
        l.deny("CardPrinting.enters_as_copy_types", enters_as_copy_types);
        l.deny_ne("CardPrinting.enters_as_copy_zone", enters_as_copy_zone, &generated::defaults::d_CardPrinting_enters_as_copy_zone());
        l.deny("CardPrinting.enters_as_copy_add_type", enters_as_copy_add_type);
        l.deny("CardPrinting.enters_as_copy_keep_name", enters_as_copy_keep_name);
        l.deny("CardPrinting.enters_as_copy_base_pt", enters_as_copy_base_pt);
        l.deny("CardPrinting.enters_as_copy_add_subtypes", enters_as_copy_add_subtypes);
        l.deny("CardPrinting.enters_as_copy_exiles_card", enters_as_copy_exiles_card);
        l.deny("CardPrinting.enters_as_copy_add_triggers", enters_as_copy_add_triggers);
        l.deny("CardPrinting.enters_as_copy_add_keywords", enters_as_copy_add_keywords);
        l.deny("CardPrinting.enters_as_copy_max_mv_mana_spent", enters_as_copy_max_mv_mana_spent);
        l.deny("CardPrinting.rebound", rebound);
        l.deny("CardPrinting.room_halves", room_halves);
        l.deny("CardPrinting.alternative_costs", alternative_costs);
        l.deny("CardPrinting.replicate_cost", replicate_cost);
        l.deny("CardPrinting.teamwork_power", teamwork_power);
        l.deny("CardPrinting.roto_statics", roto_statics);
        l.deny("CardPrinting.enters_choosing_creature_type", enters_choosing_creature_type);
        l.deny("CardPrinting.cast_from_top_of_library_kinds", cast_from_top_of_library_kinds);
        l.deny("CardPrinting.suspend_count", suspend_count);
        l.deny("CardPrinting.suspend_cost", suspend_cost);
        l.deny("CardPrinting.suspend_cost_str", suspend_cost_str);
        l.deny("CardPrinting.has_no_mana_cost", has_no_mana_cost);
        l.deny("CardPrinting.exile_activated_abilities", exile_activated_abilities);
        l.deny("CardPrinting.aura_enchant_subtype", aura_enchant_subtype);
        l.deny("CardPrinting.enters_choosing_color", enters_choosing_color);
        l.deny("CardPrinting.enchanted_land_adds_chosen_color", enchanted_land_adds_chosen_color);
        l.deny("CardPrinting.additional_cost_discard_x", additional_cost_discard_x);
        l.deny("CardPrinting.optional_sacrifice_any_number", optional_sacrifice_any_number);
        l.deny("CardPrinting.grants_free_casts_from_hand", grants_free_casts_from_hand);
        l.deny("CardPrinting.free_flash_creature_casts_max_mv", free_flash_creature_casts_max_mv);
        l.deny("CardPrinting.grants_discard_alternative_cost", grants_discard_alternative_cost);
        l.deny("CardPrinting.tapped_for_mana_triggers", tapped_for_mana_triggers);
        l.deny("CardPrinting.extra_mana_when_you_tap_nonland_for_mana", extra_mana_when_you_tap_nonland_for_mana);
        l.deny("CardPrinting.board_rules", board_rules);
        l.deny("CardPrinting.escape_exile_card_types", escape_exile_card_types);
        l.deny("CardPrinting.conspiracy_rules", conspiracy_rules);
        l.deny("CardPrinting.replacement_statics", replacement_statics);

    // Informational duplicates of `mana_cost` and the type fields.
    let _ = (mana_cost_str, type_line);

    let mut ty = 0u16;
    for t in card_types {
        match types::from_name(t) {
            Some(b) => ty |= b,
            None => l.unsupported(format!("CardType:{t}")),
        }
    }
    let mut sup = 0u8;
    let mut token = false;
    for s in supertypes {
        match supertypes::from_name(s) {
            Some(b) => sup |= b,
            // CR 111: "Token" marks a token's definition (on a type line
            // it is not a supertype).
            None if s == "Token" => token = true,
            None => l.unsupported(format!("Supertype:{s}")),
        }
    }
    // A colour indicator (CR 204) adds its colours to the mana cost's.
    let mut indicator: ManaMask = 0;
    for c in color_indicator {
        match letter_index(c) {
            Some(i) if i < 5 => indicator |= 1 << i,
            _ => l.unsupported(format!("CardPrinting.color_indicator({c})")),
        }
    }
    let subs: Vec<u16> = subtypes.iter().map(|s| l.subtype(s)).collect();
    let cost = lower_mana_cost(l, "CardPrinting.mana_cost", mana_cost);

    let is_creature = ty & types::CREATURE != 0;
    let (pw, tg) = match (power, toughness) {
        (Some(pw), Some(tg)) => (
            Some(l.int("CardPrinting.power", *pw, -99, 99)),
            Some(l.int("CardPrinting.toughness", *tg, -99, 99)),
        ),
        (None, None) => (None, None),
        _ => {
            l.unsupported("CardPrinting.power/toughness(partial)");
            (None, None)
        }
    };
    if is_creature && pw.is_none() {
        l.unsupported("CardPrinting.power(creature without P/T)");
    }

    let mut kws = KwSet::default();
    let mut protection = Prot::default();
    let mut keyword_triggers: Vec<TriggerDef> = Vec::new();
    for k in keywords {
        match lower_keyword_full(l, k) {
            Some(KwOrProt::Kw(kw)) => kws.insert(kw),
            Some(KwOrProt::Prot(p)) => protection.union(&p),
            Some(KwOrProt::Exalted) => keyword_triggers.push(exalted()),
            Some(KwOrProt::BattleCry) => keyword_triggers.push(battle_cry()),
            Some(KwOrProt::Prowess) => keyword_triggers.push(prowess()),
            None => {}
        }
    }
    if let Some(c) = protection_from_color {
        match letter_index(c) {
            Some(i) if i < 5 => protection.colors |= 1 << i,
            _ => l.unsupported(format!("CardPrinting.protection_from_color({c})")),
        }
    }

    let mut prod: ManaMask = 0;
    for s in produces {
        match letter_index(s) {
            Some(i) => prod |= 1 << i,
            None => l.unsupported(format!("CardPrinting.produces({s})")),
        }
    }
    let saved_land_colors = l.land_colors;
    l.land_colors = prod & crate::mana::COLORS;
    if prod != 0 && ty & types::LAND == 0 {
        // `produces` describes lands (CR 305.6 and land rules text).
        l.unsupported("CardPrinting.produces(non-land)");
    }

    // Activated abilities. The IR's `produces` is the land's "{T}: Add"
    // ability, except for colours an explicit mana ability whose whole
    // cost is {T} already makes (Wasteland's {C}, the Verges): there it
    // is a summary (ENGINE.md 18).
    let mut activated: Vec<ActivatedDef> = Vec::new();
    for a in activated_abilities {
        if let Some(ad) = lower_activated(l, a, From::Battlefield) {
            activated.push(ad);
        }
    }
    let covered = activated
        .iter()
        .filter(|a| a.cost == Cost { tap: true, ..Cost::default() })
        .filter_map(|a| a.mana().map(|m| prod_mask(&m.prod)))
        .fold(0, |m, x| m | x);
    let intrinsic = prod & !covered;
    let life = l.int("CardPrinting.produces_life_cost", *produces_life_cost, 0, 20) as u8;
    if intrinsic != 0 {
        activated.insert(
            0,
            ActivatedDef {
                cost: Cost { tap: true, life, ..Cost::default() },
                effect: ActEffect::Mana(ManaAbility { prod: ManaProd::One(intrinsic), damage_to_you: 0 }),
                sorcery: false,
                instant_only: false,
                only_if: None,
                your_turn: false,
            },
        );
    } else if life > 0 {
        l.unsupported("CardPrinting.produces_life_cost(no produces ability)");
    }
    if activated.len() > 4 {
        l.unsupported("CardPrinting.activated_abilities(more than 4)");
    }
    // Lands pay only through their mana abilities here. The Python engine
    // lets a land without `mana_only_from_produces` pay one generic mana
    // anyway; a land with no mana ability in the IR relies on that (Strip
    // Mine's missing "{T}: Add {C}"), which would play wrong (ENGINE.md 14).
    if ty & types::LAND != 0 && !*mana_only_from_produces && !activated.iter().any(|a| a.mana().is_some()) {
        l.unsupported("CardPrinting.mana_only_from_produces(land without a mana ability)");
    }
    let mut hand_abilities = Vec::new();
    for a in hand_activated_abilities {
        if let Some(ad) = lower_activated(l, a, From::Hand) {
            hand_abilities.push(ad);
        }
    }
    if hand_abilities.len() > 1 {
        l.unsupported("CardPrinting.hand_activated_abilities(more than 1)");
    }
    // Landcycling (CR 702.29) is a hand ability the IR also spells out in
    // `hand_activated_abilities`: the summary must match it.
    if let Some((kind, cost_str)) = landcycling {
        let ok = ManaCost::parse(cost_str).is_some_and(|mana| {
            hand_abilities.iter().any(|a| {
                a.cost == Cost { mana, discard_self: true, ..Cost::default() }
                    && match &a.effect {
                        ActEffect::Stack(ab) => ab.program.iter().any(|op| {
                            matches!(op, crate::program::Op::Search { filter: crate::db::Filter::Or(v), .. }
                                if v.len() == 1 && v[0] == crate::db::Filter::Subtype(l.subtype(kind)))
                        }),
                        ActEffect::Mana(_) => false,
                    }
            })
        });
        if !ok {
            l.unsupported("CardPrinting.landcycling(without its ability)");
        }
    }

    // Entering the battlefield (CR 614.1c, 614.12).
    let entry = match (enters_tapped, enters_tapped_unless) {
        (false, None) => EntersTapped::No,
        (true, None) => EntersTapped::Always,
        (false, Some(c)) => match lower_condition(l, c) {
            Some(c) => EntersTapped::Unless(c),
            None => EntersTapped::No,
        },
        (true, Some(_)) => {
            l.unsupported("CardPrinting.enters_tapped(with enters_tapped_unless)");
            EntersTapped::No
        }
    };
    let shock_life = l.int("CardPrinting.shock_land_life_cost", *shock_land_life_cost, 0, 20) as u8;

    // Additional costs (CR 601.2b, 118.8): pay one of the options.
    let mut additional: Vec<Cost> = Vec::new();
    for o in additional_cost_options {
        if let Some(c) = lower_additional_option(l, o) {
            additional.push(c);
        }
    }
    if let Some(f) = additional_cost_sacrifice_filter {
        if !additional.is_empty() {
            l.unsupported("CardPrinting.additional_cost_sacrifice_filter(with options)");
        }
        if let Some(filter) = lower_card_filter(l, f) {
            additional.push(Cost { sacrifice: Some(SacCost { filter, n: 1, another: false }), ..Cost::default() });
        }
    }
    if !additional.is_empty() && !is_spell_card_any(ty) {
        l.unsupported("CardPrinting.additional_cost(on a permanent card)");
    }
    if shock_life > 0 && entry != EntersTapped::No {
        l.unsupported("CardPrinting.shock_land_life_cost(with enters_tapped)");
    }

    let is_spell_card = ty & (types::INSTANT | types::SORCERY) != 0;
    let spell = match spell_effect {
        Some(e) if is_spell_card => Some(lower_ability(l, e)),
        Some(_) => {
            l.unsupported("CardPrinting.spell_effect(on a permanent card)");
            None
        }
        None if is_spell_card => {
            l.unsupported("CardPrinting.spell_effect(missing)");
            None
        }
        None => None,
    };

    let mut triggers = Vec::new();
    for t in triggered_abilities {
        if let Some(td) = super::trigger::lower_trigger(l, t, ty) {
            triggers.push(td);
        }
    }
    triggers.extend(keyword_triggers);
    let trig_mask = triggers.iter().fold(0u32, |m, t| m | 1 << t.event as u32);

    // Static abilities (CR 604) and equipment bonuses (301.5).
    let mut statics = Vec::new();
    for s in static_abilities {
        if let Some(st) = super::statics::lower_static(l, s) {
            statics.push(st);
        }
    }
    if let Some((p, t, names)) = equipment_bonuses {
        if !subs.contains(&l.subtype("Equipment")) {
            l.unsupported("CardPrinting.equipment_bonuses(not an Equipment)");
        }
        let mut mods = Vec::new();
        if *p != 0 || *t != 0 {
            mods.push(Mod::Pt(l.int("equipment_bonuses.power", *p, -99, 99), l.int("equipment_bonuses.toughness", *t, -99, 99)));
        }
        let mut set = KwSet::default();
        for n in names {
            match super::effect_ext::kw_by_name(n) {
                Some(k) => set.insert(k),
                None => l.unsupported(format!("CardPrinting.equipment_bonuses({n})")),
            }
        }
        if !set.is_empty() {
            mods.push(Mod::Keywords(set));
        }
        if !mods.is_empty() {
            statics.push(StaticDef { scope: Scope::Equipped, cond: None, mods });
        }
    }
    let pt_cda = match dynamic_pt_formula.as_deref() {
        None => None,
        Some(f) => super::statics::lower_pt_formula(l, f),
    };
    if pt_cda.is_some() && !is_creature {
        l.unsupported("CardPrinting.dynamic_pt_formula(not a creature)");
    }
    let unblockable_by = match cant_be_blocked_by.as_deref() {
        None => None,
        Some(p) => super::filter::lower_pred(l, p),
    };
    let mut spell_tax = Vec::new();
    for (f, n) in spell_cost_increases {
        if let Some(f) = lower_card_filter(l, f) {
            spell_tax.push((f, l.int("CardPrinting.spell_cost_increases", *n, 1, 9) as u8));
        }
    }
    let mut locks_activated = 0u16;
    for t in locks_activated_abilities_of {
        match types::from_name(t) {
            Some(b) => locks_activated |= b,
            None => l.unsupported(format!("CardPrinting.locks_activated_abilities_of({t})")),
        }
    }
    let mut enters_counters = Vec::new();
    for (name, n) in enters_with_named_counters {
        let k = super::effect_ext::counter_by_name(l, name);
        enters_counters.push((k, l.int("CardPrinting.enters_with_named_counters", *n, 1, 99) as u8));
    }

    l.land_colors = saved_land_colors;
    CardDef {
        name: name.clone(),
        types: ty,
        supertypes: sup,
        subtypes: subs,
        colors: cost.colors() | indicator,
        cost,
        power: pw,
        toughness: tg,
        keywords: kws,
        spell,
        triggers,
        activated,
        hand_abilities,
        enters_tapped: entry,
        shock_life,
        uncounterable: *spell_is_uncounterable,
        token,
        additional,
        statics,
        pt_cda,
        min_blockers: l.int("CardPrinting.min_blockers", *min_blockers, 0, 9) as u8,
        blocks_only_flyers: *can_only_block_creatures_with_flying,
        unblockable_by,
        no_untap: *doesnt_untap_during_untap_step,
        enters_counters,
        trig_mask,
        protection,
        spell_tax,
        locks_activated,
        exiles_uncast_creatures: *exiles_uncast_nontoken_creatures_entering,
        heals_on_damage: *heals_other_damage_when_dealt_damage,
    }
}

/// Spells with additional costs here are instants and sorceries (the cast
/// path pays them; permanent spells would need the same, untested).
fn is_spell_card_any(ty: u16) -> bool {
    ty & (types::INSTANT | types::SORCERY) != 0
}

fn lower_additional_option(l: &mut Lowerer, o: &cats::AdditionalCostOption) -> Option<Cost> {
    let cats::AdditionalCostOption::AdditionalCostOption(nodes::AdditionalCostOption {
        kind,
        amount,
        permanent_filter,
        label,
    }) = o;
    let _ = label; // display text
    let n = l.int("AdditionalCostOption.amount", *amount, 1, 20) as u8;
    let mut c = Cost::default();
    match kind {
        enums::AdditionalCostKind::Discard => c.discard = n,
        enums::AdditionalCostKind::PayLife => c.life = n,
        enums::AdditionalCostKind::Sacrifice => {
            let Some(p) = permanent_filter else {
                l.unsupported("AdditionalCostOption.permanent_filter(missing)");
                return None;
            };
            let filter = super::filter::lower_pred(l, p)?;
            c.sacrifice = Some(SacCost { filter, n, another: false });
        }
    }
    if *kind != enums::AdditionalCostKind::Sacrifice && permanent_filter.is_some() {
        l.unsupported("AdditionalCostOption.permanent_filter(not a sacrifice)");
    }
    Some(c)
}

/// `ManaCost` without X (hybrid pairs and Phyrexian pips included).
pub(crate) fn lower_mana_cost(l: &mut Lowerer, what: &str, c: &cats::ManaCost) -> ManaCost {
    let cats::ManaCost::ManaCost(nodes::ManaCost { generic, colored, x_pips, hybrid, phyrexian }) = c;
    l.deny(&format!("{what}.x_pips"), x_pips);
    let mut out = ManaCost { generic: l.int("ManaCost.generic", *generic, 0, 30) as u8, ..ManaCost::default() };
    for (sym, n) in colored {
        match letter_index(sym) {
            Some(i) => out.pips[i] += l.int("ManaCost.colored", *n, 0, 30) as u8,
            None => l.unsupported(format!("ManaCost.colored({sym})")),
        }
    }
    for (a, b) in hybrid {
        match (letter_index(a), letter_index(b)) {
            (Some(x), Some(y)) if x < 5 && y < 5 && x != y => out.hybrid[pair_index(x, y).unwrap()] += 1,
            _ => l.unsupported(format!("{what}.hybrid({a}/{b})")),
        }
    }
    for sym in phyrexian {
        match letter_index(sym) {
            Some(i) if i < 5 => out.phyrexian[i] += 1,
            _ => l.unsupported(format!("{what}.phyrexian({sym})")),
        }
    }
    out
}

/// A lowered keyword: a keyword the engine knows by name, protection (CR
/// 702.16), or a keyword that is a triggered ability (exalted, battle cry,
/// prowess).
pub(crate) enum KwOrProt {
    Kw(Kw),
    Prot(Prot),
    Exalted,
    BattleCry,
    Prowess,
}

pub(crate) fn lower_keyword_full(l: &mut Lowerer, k: &cats::Keyword) -> Option<KwOrProt> {
    let cats::Keyword::Keyword(nodes::Keyword { kind, n, quality }) = k;
    use enums::KeywordKind as K;
    match kind {
        K::ProtectionFrom => {
            l.deny("Keyword.n", n);
            let Some(q) = quality else {
                l.unsupported("Keyword.quality(missing)");
                return None;
            };
            use enums::ProtectionQuality as Q;
            let mut p = Prot::default();
            match q {
                Q::White => p.colors = 1,
                Q::Blue => p.colors = 2,
                Q::Black => p.colors = 4,
                Q::Red => p.colors = 8,
                Q::Green => p.colors = 16,
                Q::Colorless => p.colorless = true,
                Q::Everything => p.everything = true,
                Q::Creatures => p.types = types::CREATURE,
                Q::Artifacts => p.types = types::ARTIFACT,
                Q::Enchantments => p.types = types::ENCHANTMENT,
                Q::Instants => p.types = types::INSTANT,
                Q::Sorceries => p.types = types::SORCERY,
                Q::Planeswalkers => p.types = types::PLANESWALKER,
                Q::EachOfYourOpponents => p.opponents = true,
                other => {
                    l.unsupported(format!("Keyword.quality({other:?})"));
                    return None;
                }
            }
            return Some(KwOrProt::Prot(p));
        }
        K::Exalted | K::BattleCry | K::Prowess => {
            l.deny("Keyword.n", n);
            l.deny("Keyword.quality", quality);
            return Some(match kind {
                K::Exalted => KwOrProt::Exalted,
                K::BattleCry => KwOrProt::BattleCry,
                _ => KwOrProt::Prowess,
            });
        }
        _ => {}
    }
    lower_keyword(l, k).map(KwOrProt::Kw)
}

/// Exalted (CR 702.83a): "Whenever a creature you control attacks alone,
/// that creature gets +1/+1 until end of turn."
fn exalted() -> TriggerDef {
    let mut b = crate::program::Builder::default();
    b.op(crate::program::Op::Ext(crate::program::Ext::Apply {
        on: crate::program::On::EventObj,
        mods: vec![Mod::Pt(1, 1)],
        until: crate::layers::Until::EndOfTurn,
    }));
    TriggerDef {
        event: EventKind::AttacksAlone,
        pred: TrigPred::Controlled(Slot::Default, true),
        ability: b.finish(),
        uses_event: true,
    }
}

/// Prowess (CR 702.108a): "Whenever you cast a noncreature spell, this
/// creature gets +1/+1 until end of turn."
fn prowess() -> TriggerDef {
    let mut b = crate::program::Builder::default();
    b.op(crate::program::Op::Ext(crate::program::Ext::Apply {
        on: crate::program::On::Source,
        mods: vec![Mod::Pt(1, 1)],
        until: crate::layers::Until::EndOfTurn,
    }));
    let noncreature = Filter::Not(Box::new(Filter::Type(types::CREATURE)));
    TriggerDef {
        event: EventKind::IsCast,
        pred: TrigPred::And(
            Box::new(TrigPred::Subject(Slot::Spell, noncreature)),
            Box::new(TrigPred::Controlled(Slot::Controller, true)),
        ),
        ability: b.finish(),
        uses_event: false,
    }
}

/// Battle cry (CR 702.91a): "Whenever this creature attacks, each other
/// attacking creature gets +1/+0 until end of turn."
fn battle_cry() -> TriggerDef {
    let mut b = crate::program::Builder::default();
    let others = Filter::And(
        Box::new(Filter::And(Box::new(Filter::Type(types::CREATURE)), Box::new(Filter::Attacking))),
        Box::new(Filter::Not(Box::new(Filter::Source))),
    );
    b.op(crate::program::Op::Ext(crate::program::Ext::Apply {
        on: crate::program::On::Each(others),
        mods: vec![Mod::Pt(1, 0)],
        until: crate::layers::Until::EndOfTurn,
    }));
    TriggerDef { event: EventKind::Attacks, pred: TrigPred::IsSelf(Slot::Default), ability: b.finish(), uses_event: false }
}

fn lower_keyword(l: &mut Lowerer, k: &cats::Keyword) -> Option<Kw> {
    let cats::Keyword::Keyword(nodes::Keyword { kind, n, quality }) = k;
    l.deny("Keyword.n", n);
    l.deny("Keyword.quality", quality);
    use enums::KeywordKind as K;
    Some(match kind {
        K::Flying => Kw::Flying,
        K::Reach => Kw::Reach,
        K::FirstStrike => Kw::FirstStrike,
        K::DoubleStrike => Kw::DoubleStrike,
        K::Deathtouch => Kw::Deathtouch,
        K::Trample => Kw::Trample,
        K::Lifelink => Kw::Lifelink,
        K::Vigilance => Kw::Vigilance,
        K::Haste => Kw::Haste,
        K::Defender => Kw::Defender,
        K::Menace => Kw::Menace,
        K::Flash => Kw::Flash,
        K::Hexproof => Kw::Hexproof,
        K::Shroud => Kw::Shroud,
        K::Indestructible => Kw::Indestructible,
        other => {
            l.unsupported(format!("Keyword:{other:?}"));
            return None;
        }
    })
}
