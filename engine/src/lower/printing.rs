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
    AltCost, ManaAbility, ManaProd, Mod, Prot, SacCost, Scope, Slot, StaticDef, TrigPred, TriggerDef,
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
            attached_cant_be_blocked_by_more_than_one,
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
            enters_with_plus_one_counters_per,
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
            companion,
            enters_with_counter_per_card_exiled_with_it,
        } = p;
        // Which face this printing is (set by the caller for a face); the
        // printings lowered inside it (tokens, emblems) are cards.
        let face = std::mem::take(&mut l.face);
        let _ = warp_cost_str; // display form of `warp_cost`
        l.deny("CardPrinting.impending_cost", impending_cost);
        l.deny("CardPrinting.impending_cost_str", impending_cost_str);
        l.deny("CardPrinting.impending_count", impending_count);
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
        l.deny("CardPrinting.grants_cast_artifact_from_top_of_library", grants_cast_artifact_from_top_of_library);
        l.deny("CardPrinting.grants_blue_mana_ability_to_your_artifacts", grants_blue_mana_ability_to_your_artifacts);
            // Sunburst (CR 702.44a): counters by the colours of mana spent to
        // cast it (the payment records them, as for converge).
        let sunburst_counter = sunburst.then(|| {
            l.converge = true;
            super::effect_ext::counter_by_name(l, "charge")
        });
        l.deny("CardPrinting.enters_tapped_unless_any_player_life_at_most", enters_tapped_unless_any_player_life_at_most);
        l.deny("CardPrinting.additional_cost_sacrifice_creature_or_enchantment", additional_cost_sacrifice_creature_or_enchantment);
        let enters_x_counters = match enters_with_counters_from_x {
            None => false,
            Some(enums::XCounterMode::X) => true,
            // "... equal to the amount of mana spent to cast it" (EoE W3:
            // `W3Def::counters_mana_spent`).
            Some(enums::XCounterMode::ManaSpent) => false,
            Some(other) => {
                l.unsupported(format!("CardPrinting.enters_with_counters_from_x({other:?})"));
                false
            }
        };
        l.deny("CardPrinting.enters_with_plus_one_counter_if_creature_died_this_turn", enters_with_plus_one_counter_if_creature_died_this_turn);
        // ENGINE.md 99.11 (W1).
        l.deny("CardPrinting.enters_with_minus_one_counters_from_hand_cast", enters_with_minus_one_counters_from_hand_cast);
        // "Enchant creature card in a graveyard" (Animate Dead): an Aura
        // spell's target; an Aura's other enchant types (on the battlefield)
        // below, with the card types (ENGINE.md 99.2).
        let aura_gy = *aura_enchant_type == enums::EnchantKind::CreatureCardInGraveyard;
        l.deny("CardPrinting.grants_all_basic_land_types_to_lands_you_control", grants_all_basic_land_types_to_lands_you_control);
        l.deny("CardPrinting.grants_basic_land_type_to_all_lands", grants_basic_land_type_to_all_lands);
        l.deny("CardPrinting.all_permanents_gain_card_type", all_permanents_gain_card_type);
        l.deny("CardPrinting.all_objects_colorless", all_objects_colorless);
        l.deny("CardPrinting.mana_spendable_as_any_color", mana_spendable_as_any_color);
        let chooses_player = *chooses_player_as_enters;
        l.deny("CardPrinting.looks_at_opponents_face_down_creatures", looks_at_opponents_face_down_creatures);
        l.deny("CardPrinting.wipes_all_creature_abilities", wipes_all_creature_abilities);
        l.deny("CardPrinting.sets_all_creature_pt_to_1_1", sets_all_creature_pt_to_1_1);
        l.deny("CardPrinting.must_be_blocked_if_able", must_be_blocked_if_able);
        let _ = flashback_cost_str; // display form of `flashback_cost`
        let _ = buyback_cost_str; // display form of `buyback_cost`
        l.deny_ne("CardPrinting.alt_cost_exile_from_hand_count", alt_cost_exile_from_hand_count, &generated::defaults::d_CardPrinting_alt_cost_exile_from_hand_count());
        let _ = alt_cost_exile_from_hand_count; // 1 (checked above)
        // Splice onto <subtype> (CR 702.47): an instant or sorcery's.
        let splice = match (splice_onto.as_deref(), splice_cost.as_deref()) {
            (None, None) => {
                l.deny("CardPrinting.splice_cost_str", splice_cost_str);
                None
            }
            (Some(onto), Some(m)) => {
                let mana = super::effect_copies::checked_cost(l, "CardPrinting.splice_cost", m, splice_cost_str);
                Some((l.subtype(onto), mana))
            }
            _ => {
                l.unsupported("CardPrinting.splice_onto(without its cost)");
                None
            }
        };
        // Faces (`mdfc_back_face`, `transform_back_face`, `face_colors`,
        // `daybound`, `nightbound`, `adventure_face`): below.
        let _ = escape_cost_str; // display form of `escape_cost`
        let _ = dash_cost_str; // display form of `dash_cost`
        l.deny("CardPrinting.evoke_cost", evoke_cost);
        l.deny("CardPrinting.evoke_cost_str", evoke_cost_str);
        l.deny("CardPrinting.locks_opponents_activated_abilities_of", locks_opponents_activated_abilities_of);
        l.deny("CardPrinting.grants_retrace_to", grants_retrace_to);
        l.deny("CardPrinting.zone_abilities", zone_abilities);
        let _ = miracle_cost_str; // display form of `miracle_cost`
        l.deny("CardPrinting.enters_as_copy_add_triggers", enters_as_copy_add_triggers);
        l.deny("CardPrinting.enters_as_copy_max_mv_mana_spent", enters_as_copy_max_mv_mana_spent);
        l.deny("CardPrinting.rebound", rebound);
        l.deny("CardPrinting.room_halves", room_halves);
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
    // A nonmodal back face's colours are its colour indicator's (CR 204,
    // 712.8e: it has no mana cost); the IR gives them as `face_colors`.
    match (face, face_colors) {
        (super::Face::Back, Some(v)) => {
            for c in v {
                match letter_index(c) {
                    Some(i) if i < 5 => indicator |= 1 << i,
                    _ => l.unsupported(format!("CardPrinting.face_colors({c})")),
                }
            }
        }
        (super::Face::Back, None) => l.unsupported("CardPrinting.face_colors(missing on a back face)"),
        (_, Some(_)) => l.unsupported("CardPrinting.face_colors(not a nonmodal back face)"),
        (_, None) => {}
    }
    let subs: Vec<u16> = subtypes.iter().map(|s| l.subtype(s)).collect();
    // An Aura that enchants a permanent (CR 303.4a, 702.5a): its enchant
    // ability is its spell's target and what it may stay attached to.
    let aura_bf = super::eoe_a::aura_enchant(
        l,
        ty,
        subtypes.iter().any(|s| s == "Aura"),
        aura_enchant_type,
        *aura_yours_only,
        aura_gy,
        spell_effect.is_some(),
    );
    // {X} in the card's own mana cost (CR 107.3): announced on casting.
    let x_pips = {
        let cats::ManaCost::ManaCost(m) = &**mana_cost;
        l.int("CardPrinting.mana_cost.x_pips", m.x_pips, 0, 3) as u8
    };
    let cost = {
        let cats::ManaCost::ManaCost(m) = &**mana_cost;
        let plain = cats::ManaCost::ManaCost(nodes::ManaCost { x_pips: 0, ..m.clone() });
        lower_mana_cost(l, "CardPrinting.mana_cost", &plain)
    };

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
    // Station (CR 702.184, 722; ENGINE.md 99.2): the station ability, and the
    // static ability of the striation with the stationed keywords and the
    // power/toughness box, which alone gives the card its P/T (722.2c).
    let station = match station_threshold {
        Some(n) => Some(super::eoe_a::station(l, ty, *n, stationed_keywords, pw.zip(tg))),
        None => {
            l.deny("CardPrinting.stationed_keywords(without station_threshold)", stationed_keywords);
            None
        }
    };
    let (pw, tg) = if station.is_some() { (None, None) } else { (pw, tg) };

    let mut kws = KwSet::default();
    let mut protection = Prot::default();
    let mut keyword_triggers: Vec<TriggerDef> = Vec::new();
    let mut cascade_kw = false;
    let mut cascade_kw_ascend = false;
    let mut annihilator: Option<u8> = None;
    let mut devoid = false;
    for k in keywords {
        match lower_keyword_full(l, k) {
            Some(KwOrProt::Kw(kw)) => kws.insert(kw),
            Some(KwOrProt::Prot(p)) => protection.union(&p),
            Some(KwOrProt::Exalted) => keyword_triggers.push(exalted()),
            Some(KwOrProt::BattleCry) => keyword_triggers.push(battle_cry()),
            Some(KwOrProt::Prowess) => keyword_triggers.push(prowess()),
            Some(KwOrProt::Persist) => keyword_triggers.push(persist()),
            Some(KwOrProt::Cascade) => cascade_kw = true,
            Some(KwOrProt::Ascend) => cascade_kw_ascend = true,
            Some(KwOrProt::Annihilator(n)) => annihilator = Some(n),
            Some(KwOrProt::Devoid) => devoid = true,
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
    // The card's spell has an X (its mana cost's, or "pay X life").
    let saved_spell_x = std::mem::replace(&mut l.spell_x, x_pips > 0 || *additional_cost_pay_x_life);
    let saved_delayed = std::mem::take(&mut l.delayed);
    // Necromancy: "if you cast it any time a sorcery couldn't have been
    // cast, the controller of the permanent it becomes sacrifices it at the
    // beginning of the next cleanup step" (a delayed ability of the card).
    // "You may cast this spell as though it had flash": the IR writes the
    // flash keyword beside the cleanup sacrifice (ir_defects 8); it is a
    // permission (CR 702.8a: Necromancy has no flash).
    let flash_permission = *sacrifice_at_cleanup_if_cast_without_sorcery_timing && kws.has(Kw::Flash);
    if flash_permission {
        kws.remove(Kw::Flash);
    } else if *sacrifice_at_cleanup_if_cast_without_sorcery_timing {
        l.unsupported("CardPrinting.sacrifice_at_cleanup_if_cast_without_sorcery_timing(without \"as though it had flash\")");
    }
    let cleanup_sac = sacrifice_at_cleanup_if_cast_without_sorcery_timing.then(|| {
        let mut d = crate::program::Builder::default();
        d.op(crate::program::Op::Ext(crate::program::Ext::SacrificeEventObj));
        l.delayed_ability(d.finish())
    });
    let saved_latent = std::mem::take(&mut l.latent);
    l.land_colors = prod & crate::mana::COLORS;
    if prod != 0 && ty & types::LAND == 0 {
        // `produces` describes lands (CR 305.6 and land rules text).
        l.unsupported("CardPrinting.produces(non-land)");
    }

    // Activated abilities. The IR's `produces` is the land's "{T}: Add"
    // ability, except for colours an explicit mana ability whose whole
    // cost is {T} already makes (Wasteland's {C}, the Verges): there it
    // is a summary (ENGINE.md 18).
    // Planeswalkers (CR 306): printed loyalty (306.5a); their printed
    // activated abilities are loyalty abilities (606.2).
    let walker = ty & types::PLANESWALKER != 0;
    let printed_loyalty = match (walker, loyalty) {
        (true, Some(n)) => Some(l.int("CardPrinting.loyalty", *n, 0, 30) as u8),
        (true, None) => {
            l.unsupported("CardPrinting.loyalty(missing for a planeswalker)");
            None
        }
        (false, Some(_)) => {
            l.unsupported("CardPrinting.loyalty(not a planeswalker)");
            None
        }
        (false, None) => None,
    };
    let walker_flag = |l: &mut Lowerer, what: &str, v: bool| {
        if v && !walker {
            l.unsupported(format!("{what}(not a planeswalker)"));
        }
        v
    };
    let loyalty_instant_if_entered = walker_flag(
        l,
        "CardPrinting.loyalty_abilities_at_instant_speed_if_entered_this_turn",
        *loyalty_abilities_at_instant_speed_if_entered_this_turn,
    );
    let mut activated: Vec<ActivatedDef> = Vec::new();
    for a in activated_abilities {
        if let Some(ad) = super::eoe_a::lower_activated_tiered(l, a, walker) {
            activated.push(ad);
        }
    }
    let (station_ability, station_static) = match station {
        Some(st) => (Some(st.ability), st.statik),
        None => (None, None),
    };
    activated.extend(station_ability);
    // Reconfigure (CR 702.151; Lion Sash): its two abilities are in the
    // IR's list; the cost agrees with them.
    let reconfigure = match reconfigure_cost.as_deref() {
        Some(m) => {
            let mana = super::effect_copies::checked_cost(l, "CardPrinting.reconfigure_cost", m, reconfigure_cost_str);
            super::effect_m7d::reconfigure_abilities(l, &mut activated, mana);
            if !(subs.contains(&l.subtype("Equipment")) && is_creature) {
                l.unsupported("CardPrinting.reconfigure_cost(not an Equipment creature)");
            }
            true
        }
        None => {
            l.deny("CardPrinting.reconfigure_cost_str", reconfigure_cost_str);
            false
        }
    };
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
                effect: ActEffect::Mana(ManaAbility { prod: ManaProd::One(intrinsic), damage_to_you: 0, sac_if_no: None, only: 0, uncounterable: false, haste: false, opp_life: 0 }),
                sorcery: false,
                instant_only: false,
                exhaust: false,
                only_if: None,
                your_turn: false,
                loyalty: None,
                discounts: Vec::new(),
                boast: false,
                x: false,
                special: None,
            },
        );
    } else if life > 0 {
        l.unsupported("CardPrinting.produces_life_cost(no produces ability)");
    }
    // Multiversal Passage: "As this land enters, choose a basic land type
    // ... This land is the chosen type" (CR 305.6: it has that type's mana
    // ability).
    let chooses_land_type = *enters_choosing_basic_land_type;
    if chooses_land_type != *is_chosen_basic_land_type || chooses_land_type && ty & types::LAND == 0 {
        l.unsupported("CardPrinting.enters_choosing_basic_land_type(without is_chosen_basic_land_type)");
    }
    if chooses_land_type {
        activated.insert(
            0,
            ActivatedDef {
                cost: Cost { tap: true, ..Cost::default() },
                effect: ActEffect::Mana(ManaAbility { prod: ManaProd::Chosen, damage_to_you: 0, sac_if_no: None, only: 0, uncounterable: false, haste: false, opp_life: 0 }),
                sorcery: false,
                instant_only: false,
                exhaust: false,
                only_if: None,
                your_turn: false,
                loyalty: None,
                discounts: Vec::new(),
                boast: false,
                x: false,
                special: None,
            },
        );
    }
    // (Checked again with the latent abilities, below.)
    // Lands pay only through their mana abilities here. The Python engine
    // lets a land without `mana_only_from_produces` pay one generic mana
    // anyway; a land with no mana ability in the IR relies on that (Strip
    // Mine's missing "{T}: Add {C}"), which would play wrong (ENGINE.md 14).
    if ty & types::LAND != 0 && !*mana_only_from_produces && !activated.iter().any(|a| a.mana().is_some()) {
        l.unsupported("CardPrinting.mana_only_from_produces(land without a mana ability)");
    }
    let mut hand_abilities = Vec::new();
    for a in hand_activated_abilities {
        if let Some(ad) = lower_activated(l, a, From::Hand, false) {
            hand_abilities.push(ad);
        }
    }
    // Plot (CR 702.170a): a special action from hand (116.2k), offered as
    // the card's hand ability: "any time you have priority during your main
    // phase while the stack is empty, you may exile this card from your hand
    // and pay [cost]". Cast later "as a sorcery": a sorcery's own timing
    // (an instant with plot would need its own; none in the cube).
    match plot_cost.as_deref() {
        Some(m) => {
            let mana = super::effect_copies::checked_cost(l, "CardPrinting.plot_cost", m, plot_cost_str);
            if ty & types::SORCERY == 0 {
                l.unsupported("CardPrinting.plot_cost(not a sorcery)");
            }
            hand_abilities.push(ActivatedDef {
                cost: Cost { mana, ..Cost::default() },
                effect: ActEffect::Stack(crate::db::AbilityDef::default()),
                sorcery: true,
                instant_only: false,
                exhaust: false,
                only_if: None,
                your_turn: false,
                loyalty: None,
                discounts: Vec::new(),
                boast: false,
                x: false,
                special: Some(crate::db::Special::Plot),
            });
        }
        None => l.deny("CardPrinting.plot_cost_str", plot_cost_str),
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
    // "As an additional cost to cast this spell, sacrifice an artifact or
    // creature" (CR 601.2b, 118.8; ENGINE.md 99.2).
    if *additional_cost_sacrifice_creature_or_artifact {
        if !additional.is_empty() {
            l.unsupported("CardPrinting.additional_cost_sacrifice_creature_or_artifact(with other additional costs)");
        }
        let filter = Filter::Type(types::CREATURE | types::ARTIFACT);
        additional.push(Cost { sacrifice: Some(SacCost { filter, n: 1, another: false }), ..Cost::default() });
    }
    // "As an additional cost to cast this spell, exile n cards from your
    // graveyard" (Abhorrent Oculus; CR 601.2b, 118.8).
    if *additional_cost_exile_from_graveyard != 0 {
        let n = l.int("CardPrinting.additional_cost_exile_from_graveyard", *additional_cost_exile_from_graveyard, 1, 20) as u8;
        if !additional.is_empty() {
            l.unsupported("CardPrinting.additional_cost_exile_from_graveyard(with other additional costs)");
        }
        additional.push(Cost { exile_graveyard: n, ..Cost::default() });
    }
    // (On a permanent card only the graveyard exile, a cost paid like a
    // spell's: Abhorrent Oculus.)
    let gy_only = *additional_cost_exile_from_graveyard != 0 && additional.len() == 1;
    if !additional.is_empty() && !is_spell_card_any(ty) && !gy_only {
        l.unsupported("CardPrinting.additional_cost(on a permanent card)");
    }
    if shock_life > 0 && entry != EntersTapped::No {
        l.unsupported("CardPrinting.shock_land_life_cost(with enters_tapped)");
    }

    let is_spell_card = ty & (types::INSTANT | types::SORCERY) != 0;
    // Kicker (CR 702.33): an optional additional cost; "if this spell was
    // kicked" picks the effect, as a mode given by the cost (its targets
    // follow, CR 601.2c after 601.2b).
    let mut perm_kicker: Option<(ManaCost, bool)> = None;
    let kicker = match kicker_cost {
        None => None,
        // Kicker on a permanent card (CR 702.33): an optional additional
        // cost, paid once (multikicker, 702.33c: any number of times);
        // "if it was kicked" is how it was cast (the `Enters` event's note,
        // or the spell's for a cast trigger). Kicker {X} announces X.
        Some(m) if !is_spell_card && spell_effect.is_none() && ty & types::PERMANENT != 0 => {
            perm_kicker = kicker_mana(l, m, kicker_cost_str);
            None
        }
        Some(m) => {
            let mana = lower_mana_cost(l, "CardPrinting.kicker_cost", m);
            match spell_effect.as_deref() {
                Some(cats::Effect::IfConditionElse(nodes::IfConditionElse { condition, then_effect, else_effect: Some(e) }))
                    if matches!(**condition, cats::ConditionOrStatePredicate::SpellWasKicked(_)) && is_spell_card && *teamwork_power == 0 =>
                {
                    if !additional.is_empty() {
                        l.unsupported("CardPrinting.kicker_cost(with additional costs)");
                    }
                    additional = vec![Cost::default(), Cost { mana, ..Cost::default() }];
                    let plain = lower_ability(l, e);
                    let kicked = lower_ability(l, then_effect);
                    if plain.modes.is_some() || kicked.modes.is_some() || plain.divide.is_some() || kicked.divide.is_some() {
                        l.unsupported("CardPrinting.kicker_cost(modal effect)");
                    }
                    Some(crate::db::AbilityDef {
                        modes: Some(Box::new(crate::db::Modes { list: vec![plain, kicked], min: 1, max: 1, repeat: false, escalate_discard: false, costs: Vec::new(), once_each: false })),
                        ..Default::default()
                    })
                }
                _ => {
                    l.unsupported("CardPrinting.kicker_cost(not \"if kicked ... else\")");
                    None
                }
            }
        }
    };
    // Teamwork n: "as an additional cost to cast this spell, you may tap any
    // number of creatures you control with total power n or more" (crew's
    // cost, CR 702.122a); "if this spell was cast using teamwork" picks the
    // effect, as kicker's does.
    let kicker = if *teamwork_power != 0 {
        let n = l.int("CardPrinting.teamwork_power", *teamwork_power, 1, 20) as u8;
        match spell_effect.as_deref() {
            Some(cats::Effect::IfConditionElse(nodes::IfConditionElse { condition, then_effect, else_effect: Some(e) }))
                if matches!(**condition, cats::ConditionOrStatePredicate::OptionalAdditionalCostPaid(_))
                    && is_spell_card
                    && kicker.is_none()
                    && additional.is_empty() =>
            {
                additional = vec![Cost::default(), Cost { crew: n, ..Cost::default() }];
                let plain = lower_ability(l, e);
                let teamed = lower_ability(l, then_effect);
                if plain.modes.is_some() || teamed.modes.is_some() || plain.divide.is_some() || teamed.divide.is_some() {
                    l.unsupported("CardPrinting.teamwork_power(modal effect)");
                }
                Some(crate::db::AbilityDef {
                    modes: Some(Box::new(crate::db::Modes { list: vec![plain, teamed], min: 1, max: 1, repeat: false, escalate_discard: false, costs: Vec::new(), once_each: false })),
                    ..Default::default()
                })
            }
            _ => {
                l.unsupported("CardPrinting.teamwork_power(not \"if cast using teamwork ... else\")");
                kicker
            }
        }
    } else {
        kicker
    };
    // Overload (CR 702.96): the alternative cost changes "target" to
    // "each": the two branches of "if the alternative cost was paid" are
    // the modes, the mode given by the cost.
    let overload = alternative_costs.iter().any(|a| {
        let cats::AlternativeCost::AlternativeCost(n) = a;
        n.overload
    });
    let overloaded = if overload {
        match spell_effect.as_deref() {
            Some(cats::Effect::IfConditionElse(nodes::IfConditionElse { condition, then_effect, else_effect: Some(e) }))
                if matches!(**condition, cats::ConditionOrStatePredicate::AlternativeCostWasPaid(_)) && is_spell_card =>
            {
                let plain = lower_ability(l, e);
                let each = lower_ability(l, then_effect);
                if plain.modes.is_some() || each.modes.is_some() || alternative_costs.len() != 1 {
                    l.unsupported("AlternativeCost.overload(shape)");
                }
                Some(crate::db::AbilityDef {
                    modes: Some(Box::new(crate::db::Modes { list: vec![plain, each], min: 1, max: 1, repeat: false, escalate_discard: false, costs: Vec::new(), once_each: false })),
                    ..Default::default()
                })
            }
            _ => {
                l.unsupported("AlternativeCost.overload(not \"if overloaded ... else\")");
                None
            }
        }
    } else {
        None
    };
    // Buyback (CR 702.27): an optional additional cost.
    let buyback = match buyback_cost {
        None => false,
        Some(m) => {
            let mana = lower_mana_cost(l, "CardPrinting.buyback_cost", m);
            if !additional.is_empty() || kicker.is_some() {
                l.unsupported("CardPrinting.buyback_cost(with other additional costs)");
            }
            additional = vec![Cost::default(), Cost { mana, ..Cost::default() }];
            true
        }
    };
    let spell = match spell_effect {
        // An Aura spell targets what it will enchant (CR 303.4a).
        None if aura_gy => {
            let f = Filter::InGraveyard(Box::new(Filter::Type(types::CREATURE)));
            Some(crate::db::AbilityDef { targets: vec![crate::db::TargetReq { filter: f, min: 1, max: 1, distinct: 0, pair: 0, total_mv: None }], ..Default::default() })
        }
        None if aura_bf.is_some() => {
            let f = aura_bf.clone().unwrap();
            Some(crate::db::AbilityDef { targets: vec![crate::db::TargetReq { filter: f, min: 1, max: 1, distinct: 0, pair: 0, total_mv: None }], ..Default::default() })
        }
        _ if kicker.is_some() => kicker.clone(),
        _ if overloaded.is_some() => overloaded.clone(),
        Some(e) if is_spell_card => Some(lower_ability(l, e)),
        Some(_) => {
            l.unsupported("CardPrinting.spell_effect(on a permanent card)");
            None
        }
        // A split card's spells are its halves' (CR 709.3).
        None if is_spell_card && split_faces.is_empty() => {
            l.unsupported("CardPrinting.spell_effect(missing)");
            None
        }
        None => None,
    };

    // Dash (CR 702.109): an alternative cost; the dashed permanent's
    // delayed "return to hand" ability.
    let dash = dash_cost.as_ref().map(|m| {
        let mana = lower_mana_cost(l, "CardPrinting.dash_cost", m);
        let mut d = crate::program::Builder::default();
        d.op(crate::program::Op::Ext(crate::program::Ext::ReturnEventObjToHand));
        let k = l.delayed_ability(d.finish());
        AltCost { mana, dash: Some(k), ..AltCost::default() }
    });
    // Warp (CR 702.185): an alternative cost from hand; the permanent is
    // exiled at the beginning of the next end step, and its owner may cast
    // it from exile on a later turn.
    let warp = warp_cost.as_ref().map(|m| {
        // "Warp {X}{G}" on an {X} card: the X announced as it is cast
        // (CR 107.3, 601.2b) adds to the warp cost as to its mana cost
        // (`total_mana`), so the warp cost's {X} must be the card's.
        let cats::ManaCost::ManaCost(n) = &**m;
        if n.x_pips != x_pips as i64 {
            l.unsupported("CardPrinting.warp_cost.x_pips(not the mana cost's)");
        }
        let plain = cats::ManaCost::ManaCost(nodes::ManaCost { x_pips: 0, ..n.clone() });
        let mana = lower_mana_cost(l, "CardPrinting.warp_cost", &plain);
        let mut d = crate::program::Builder::default();
        d.op(crate::program::Op::Ext(crate::program::Ext::WarpExile));
        let k = l.delayed_ability(d.finish());
        // "Warp-{B}, Pay 2 life" (CR 119.4; ENGINE.md 99.4).
        let life = l.int("CardPrinting.warp_additional_life_cost", *warp_additional_life_cost, 0, 20) as u8;
        let cost = crate::db::Cost { life, ..Default::default() };
        AltCost { mana, cost, dash: Some(k), warp: true, ..AltCost::default() }
    });
    if warp.is_none() {
        l.deny("CardPrinting.warp_additional_life_cost(without warp)", warp_additional_life_cost);
        l.deny("CardPrinting.warp_castable_from_graveyard(without warp)", warp_castable_from_graveyard);
    }
    if warp.is_some() && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.warp_cost(not a permanent card)");
    }
    // "<Cards> in your hand have warp <cost>": each grant's warp exile is a
    // delayed ability of this definition (ENGINE.md 99.15).
    let hand_alt_grants = super::eoe_w5::hand_alt_grants(l, hand_ability_grants);
    // Miracle (CR 702.94): "when you reveal this card this way, you may cast
    // it by paying [cost] rather than its mana cost".
    let miracle = miracle_cost.as_ref().map(|m| {
        let mana = lower_mana_cost(l, "CardPrinting.miracle_cost", m);
        let mut d = crate::program::Builder::default();
        d.op(crate::program::Op::Ext(crate::program::Ext::MiracleCast));
        (mana, l.delayed_ability(d.finish()))
    });
    if dash.is_some() && ty & types::CREATURE == 0 {
        l.unsupported("CardPrinting.dash_cost(not a creature)");
    }
    let flashback = flashback_cost.as_ref().map(|m| lower_mana_cost(l, "CardPrinting.flashback_cost", m));
    let escape = match (escape_cost, escape_exile_count) {
        (None, 0) => None,
        (Some(m), n) if *n > 0 => {
            let mana = lower_mana_cost(l, "CardPrinting.escape_cost", m);
            let n = l.int("CardPrinting.escape_exile_count", *n, 1, 20) as u8;
            Some((mana, Cost { exile_graveyard: n, ..Cost::default() }))
        }
        _ => {
            l.unsupported("CardPrinting.escape_cost(without an exile count)");
            None
        }
    };
    // ---- copies (M7b part 3; ENGINE.md 26) ----
    // "You may have this enter as a copy of ..." (CR 707.5, 707.9).
    let enters_as_copy = lower_enters_as_copy(
        l,
        EnterCopyFields {
            types: enters_as_copy_types,
            zone: enters_as_copy_zone,
            add_type: enters_as_copy_add_type,
            keep_name: *enters_as_copy_keep_name,
            base_pt: enters_as_copy_base_pt,
            add_subtypes: enters_as_copy_add_subtypes,
            exiles_card: *enters_as_copy_exiles_card,
            add_keywords: enters_as_copy_add_keywords,
        },
        ty,
    );
    // "You may have this land enter tapped as a copy of any land card in a
    // graveyard, except it's a Cave in addition to its other types" (the
    // flag's meaning, Echoing Deeps; CR 707.5, 707.9b; ENGINE.md 99.12).
    let enters_as_copy = if *optional_copy_land_from_graveyard {
        if enters_as_copy.is_some() || ty & types::LAND == 0 {
            l.unsupported("CardPrinting.optional_copy_land_from_graveyard(not a land, or with another copy)");
        }
        let mut exc = crate::copies::Except::default();
        exc.add_subtype(l.subtype("Cave"));
        Some(Box::new(crate::db::EntersAsCopy { filter: Filter::Type(types::LAND), from_graveyard: true, exc, keep_name: false, exile_k: None, tapped: true }))
    } else {
        enters_as_copy
    };
    // Additional costs paid a number of times (CR 601.2b): squad
    // (702.157a, creatures), offspring (702.175a, once), replicate
    // (702.56a, instants and sorceries: its "when you cast this spell, copy
    // it for each time" trigger is the keyword's own).
    let mut times_cost: Option<crate::db::TimesCost> = None;
    let mut set_times = |l: &mut Lowerer, what: &str, mana: ManaCost, max: u8, ok: bool| {
        if !ok {
            l.unsupported(format!("CardPrinting.{what}(on this card type)"));
        }
        if times_cost.is_some() {
            l.unsupported(format!("CardPrinting.{what}(with another repeatable cost)"));
        }
        times_cost = Some(crate::db::TimesCost { mana, max, x: false });
    };
    match squad_cost.as_deref() {
        Some(m) => {
            let mana = super::effect_copies::checked_cost(l, "CardPrinting.squad_cost", m, squad_cost_str);
            set_times(l, "squad_cost", mana, crate::decision::act::NUMBER_N as u8 - 1, is_creature);
        }
        None => l.deny("CardPrinting.squad_cost_str", squad_cost_str),
    }
    match offspring_cost.as_deref() {
        Some(m) => {
            let mana = super::effect_copies::checked_cost(l, "CardPrinting.offspring_cost", m, offspring_cost_str);
            set_times(l, "offspring_cost", mana, 1, ty & types::PERMANENT != 0);
        }
        None => l.deny("CardPrinting.offspring_cost_str", offspring_cost_str),
    }
    if !replicate_cost.is_empty() {
        match ManaCost::parse(replicate_cost) {
            Some(mana) => {
                set_times(l, "replicate_cost", mana, crate::decision::act::NUMBER_N as u8 - 1, ty & (types::INSTANT | types::SORCERY) != 0);
                keyword_triggers.push(replicate());
            }
            None => l.unsupported(format!("CardPrinting.replicate_cost({replicate_cost})")),
        }
    }
    // Storm (CR 702.40a): a triggered ability of an instant or sorcery
    // spell.
    if *has_storm {
        if ty & (types::INSTANT | types::SORCERY) == 0 {
            l.unsupported("CardPrinting.has_storm(not an instant or sorcery)");
        }
        keyword_triggers.push(storm());
    }
    if let Some((mana, x)) = perm_kicker {
        let max = if *multikicker { crate::decision::act::NUMBER_N as u8 - 1 } else { 1 };
        if x && *multikicker {
            l.unsupported("CardPrinting.multikicker(with {X})");
        }
        set_times(l, "kicker_cost", mana, max, true);
        if let Some(t) = times_cost.as_mut() {
            t.x = x;
        }
    } else if *multikicker {
        l.unsupported("CardPrinting.multikicker(without a permanent's kicker)");
    }
    if *enters_with_counter_per_kick && !*multikicker {
        l.unsupported("CardPrinting.enters_with_counter_per_kick(without multikicker)");
    }
    // "X can't be 0" (kicker {X}).
    let x_least = match (*x_min, times_cost.as_ref().is_some_and(|t| t.x)) {
        (0, _) => 0,
        (n, true) => l.int("CardPrinting.x_min", n, 0, 9) as u8,
        (_, false) => {
            l.unsupported("CardPrinting.x_min(without kicker {X})");
            0
        }
    };
    if splice.is_some() && (ty & (types::INSTANT | types::SORCERY) == 0 || spell_effect.is_none() || kicker.is_some() || overloaded.is_some()) {
        l.unsupported("CardPrinting.splice_onto(not a plain instant or sorcery)");
    }
    if times_cost.is_some() && (!additional.is_empty() || x_pips > 0) {
        l.unsupported("CardPrinting(a repeatable cost with other additional costs or X)");
    }
    // Bestow (CR 702.103): an alternative cost; the bestowed face is built
    // below from the card (702.103b).
    let bestow = match bestow_cost.as_deref() {
        Some(m) => {
            let mana = super::effect_copies::checked_cost(l, "CardPrinting.bestow_cost", m, bestow_cost_str);
            if !is_creature || face != super::Face::Card {
                l.unsupported("CardPrinting.bestow_cost(not a creature card)");
            }
            match bestow_enchanted_bonus[..] {
                [p, t] => Some((mana, l.int("CardPrinting.bestow_enchanted_bonus", p, -9, 9), l.int("CardPrinting.bestow_enchanted_bonus", t, -9, 9))),
                _ => {
                    l.unsupported("CardPrinting.bestow_enchanted_bonus(not p/t)");
                    None
                }
            }
        }
        None => {
            l.deny("CardPrinting.bestow_cost_str", bestow_cost_str);
            l.deny("CardPrinting.bestow_enchanted_bonus", bestow_enchanted_bonus);
            None
        }
    };
    // Ward (CR 702.21a): a triggered ability of the permanent, "whenever this
    // permanent becomes the target of a spell or ability an opponent
    // controls, counter that spell or ability unless that player pays
    // <cost>". The IR states one cost: mana, life or a chosen discard.
    let mut wards: Vec<crate::db::WardCost> = Vec::new();
    match ward_cost.as_deref() {
        Some(m) => wards.push(crate::db::WardCost::Mana(super::effect_copies::checked_cost(l, "CardPrinting.ward_cost", m, ward_cost_str))),
        None => l.deny("CardPrinting.ward_cost_str", ward_cost_str),
    }
    if *ward_life_cost != 0 {
        wards.push(crate::db::WardCost::Life(l.int("CardPrinting.ward_life_cost", *ward_life_cost, 1, 20) as u8));
    }
    if *ward_discard_chosen {
        wards.push(crate::db::WardCost::Discard);
    }
    if *ward_discard_random {
        wards.push(crate::db::WardCost::DiscardRandom);
    }
    if wards.len() > 1 {
        l.unsupported("CardPrinting.ward(more than one cost)");
    }
    let ward = wards.first().copied();
    if ward.is_some() && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.ward(not a permanent card)");
    }
    if let Some(w) = ward {
        keyword_triggers.push(super::effect_stack::ward_trigger(w, false));
    }
    // "Enchanted permanent has ward {n}" (ENGINE.md 99.2).
    match attached_grants_ward_cost.as_deref() {
        Some(m) => {
            let cost = super::effect_copies::checked_cost(l, "CardPrinting.attached_grants_ward_cost", m, attached_grants_ward_str);
            keyword_triggers.push(super::eoe_a::attached_ward(l, *aura_yours_only && aura_bf.is_some(), cost));
        }
        None => l.deny("CardPrinting.attached_grants_ward_str", attached_grants_ward_str),
    }
    // Abilities activated from the graveyard (CR 602.2: "exile this card
    // from your graveyard", eternalize; ENGINE.md 27.5).
    let mut gy_abilities: Vec<ActivatedDef> = Vec::new();
    for a in graveyard_activated_abilities {
        if let Some(ad) = lower_activated(l, a, From::Graveyard, false) {
            if ad.mana().is_some() {
                l.unsupported("CardPrinting.graveyard_activated_abilities(a mana ability)");
            }
            gy_abilities.push(ad);
        }
    }
    if gy_abilities.len() > 1 {
        l.unsupported("CardPrinting.graveyard_activated_abilities(more than one)");
    }
    // Improvise (CR 702.126): on a spell.
    if *has_improvise && ty & types::LAND != 0 {
        l.unsupported("CardPrinting.has_improvise(a land)");
    }
    // Convoke (CR 702.51a): on a spell; with improvise too, an artifact
    // creature would be two sources of one tap (no card has both).
    if *has_convoke && (ty & types::LAND != 0 || *has_improvise) {
        l.unsupported("CardPrinting.has_convoke(a land, or with improvise)");
    }
    // Static abilities (CR 604), before the triggered abilities: a granted
    // ward's trigger is this card's (ENGINE.md 27.3).
    let mut statics = Vec::new();
    let mut repl = super::statics::Repl::default();
    for s in static_abilities {
        if let Some(st) = super::eoe_a::lower_static_tiered(l, s, &mut repl) {
            statics.push(st);
        }
    }
    statics.extend(station_static);
    if let Some(w) = repl.grants_ward {
        keyword_triggers.push(super::effect_stack::ward_trigger(w, true));
    }
    if let Some((ability, n)) = repl.grants_target_trigger.take() {
        keyword_triggers.push(super::effect_m7d::granted_target_trigger(ability, n));
    }
    let mut triggers = Vec::new();
    // Triggered abilities an Equipment grants (Kaldra Compleat).
    for t in equipment_granted_triggers {
        if !subs.contains(&l.subtype("Equipment")) {
            l.unsupported("CardPrinting.equipment_granted_triggers(not an Equipment)");
        }
        if let Some(mut td) = super::trigger::lower_trigger(l, t, ty) {
            td.pred = super::trigger::granted_to_host(td.pred);
            triggers.push(td);
        }
    }
    // "Whenever one or more cards are put into exile from your library and/or
    // your graveyard": the flag and the trigger's predicate say it together.
    l.watches_exile = *watches_library_graveyard_exile;
    let watched = triggered_abilities.iter().any(|t| {
        let cats::TriggeredAbility::TriggeredAbility(n) = t;
        matches!(n.predicate.as_deref(), Some(cats::TriggerPredicate::CardsExiledFromLibraryOrGraveyard(_)))
    });
    if watched != *watches_library_graveyard_exile {
        l.unsupported("CardPrinting.watches_library_graveyard_exile(without its trigger)");
    }
    for t in triggered_abilities {
        if let Some(td) = super::eoe_a::lower_trigger_tiered(l, t, ty) {
            triggers.push(td);
        }
    }
    l.watches_exile = false;
    // Cascade: the keyword and its "when you cast this spell" trigger.
    let cascade_trigger = triggers.iter().any(|t| {
        t.event == EventKind::IsCast
            && t.pred == TrigPred::IsSelf(Slot::Spell)
            && t.ability.program.contains(&crate::program::Op::Ext(crate::program::Ext::Cascade))
    });
    if cascade_kw != cascade_trigger || l.cascade != cascade_kw {
        l.unsupported("Keyword:Cascade(keyword and trigger disagree)");
    }
    // Annihilator n: the keyword and its "whenever this creature attacks,
    // defending player sacrifices n permanents" trigger (CR 702.86a).
    let annihilator_trigger = triggers.iter().find_map(|t| match t.ability.program.as_slice() {
        [crate::program::Op::Ext(crate::program::Ext::SacrificeN { who: crate::program::Who::Opponent, filter: Filter::Type(types::PERMANENT), n })]
            if t.event == EventKind::Attacks && t.pred == TrigPred::IsSelf(Slot::Default) =>
        {
            Some(*n)
        }
        _ => None,
    });
    if annihilator != annihilator_trigger {
        l.unsupported("Keyword:Annihilator(keyword and trigger disagree)");
    }
    // "Put into a graveyard from anywhere" (a trigger, or a replacement):
    // the printing says it watches the card there (`watched_in_graveyard`).
    let gy_watch = triggers.iter().any(|t| t.event == EventKind::ToGraveyard) || repl.gy_to_library;
    // Triggered abilities that function in the graveyard: the flag and the
    // triggers say it together.
    let gy_triggers = triggers.iter().any(|t| crate::triggers::in_graveyard(&t.pred));
    if gy_triggers != *has_graveyard_triggers {
        l.unsupported("CardPrinting.has_graveyard_triggers(without its trigger)");
    }
    if gy_watch != *watched_in_graveyard {
        l.unsupported("CardPrinting.watched_in_graveyard(without a \"from anywhere\" ability)");
    }
    triggers.extend(keyword_triggers);
    let trig_mask = triggers.iter().fold(0u64, |m, t| m | 1 << t.event as u32);
    // Sagas (CR 714): the final chapter number is the greatest of its
    // chapter abilities' (714.2d); the IR also states it (`chapter_count`),
    // which must agree (ENGINE.md 3, rule 4a).
    let chapters = {
        let n = l.int("CardPrinting.chapter_count", *chapter_count, 0, 20) as u8;
        let mut last = 0u8;
        for t in &triggers {
            let ch = chapter_of(&t.pred);
            if ch.is_some() != (t.event == EventKind::Chapter) {
                l.unsupported("TriggerPredicate:ChapterReached(on another event)");
            }
            last = last.max(ch.unwrap_or(0));
        }
        if n != last {
            l.unsupported("CardPrinting.chapter_count(not its last chapter)");
        }
        if n > 0 && !subs.contains(&l.subtype("Saga")) {
            l.unsupported("CardPrinting.chapter_count(not a Saga)");
        }
        n
    };
    // Serra Paragon: what a permanent played or cast from the graveyard
    // this way gains: "When this permanent is put into a graveyard from the
    // battlefield, exile it and you gain 2 life" (a delayed ability of this
    // card that the permanent's leaving triggers, ENGINE.md 28).
    let gy_play_mv = l.int("CardPrinting.grants_once_per_turn_graveyard_play_mv", *grants_once_per_turn_graveyard_play_mv, 0, 20) as u8;
    let gy_play_k = if gy_play_mv > 0 {
        let mut b = crate::program::Builder::default();
        b.op(crate::program::Op::Ext(crate::program::Ext::ExileEventCard));
        b.op(crate::program::Op::GainLife { who: crate::program::Who::You, n: crate::program::Amount::Fixed(2) });
        l.delayed_ability(b.finish())
    } else {
        0
    };
    // "Whenever you tap a creature for mana, add an additional {G}."
    let mut extra_mana_creature = [0u8; 6];
    for m in extra_mana_when_you_tap_creature_for_mana {
        match letter_index(m) {
            Some(i) => extra_mana_creature[i] += 1,
            None => l.unsupported(format!("CardPrinting.extra_mana_when_you_tap_creature_for_mana({m})")),
        }
    }
    // Delayed triggered abilities come last (`delayed_base`).
    let delayed_base = triggers.len() as u8;
    triggers.extend(std::mem::replace(&mut l.delayed, saved_delayed));
    // Latent activated abilities its effects grant it come last
    // (`CardDef::latent`).
    let latent = std::mem::replace(&mut l.latent, saved_latent);
    let n_latent = latent.len() as u8;
    activated.extend(latent);
    if activated.len() > 4 {
        l.unsupported("CardPrinting.activated_abilities(more than 4)");
    }
    if triggers.len() > u8::MAX as usize {
        l.unsupported("CardPrinting.triggered_abilities(too many)");
    }

    // Static abilities (CR 604; lowered above) and equipment bonuses
    // (301.5).
    // "During your turn, as long as this has one or more loyalty counters
    // on it, it's a p/t <subtypes> creature [and has <keywords>]" (a
    // static ability; Kaito).
    if let Some((p, t, subs, kws)) = creature_during_your_turn {
        if !walker {
            l.unsupported("CardPrinting.creature_during_your_turn(not a planeswalker)");
        }
        let subtypes = subs.iter().map(|s| l.subtype(s)).collect();
        let mut mods = vec![
            // "it's a 3/4 Ninja creature": its card types and subtypes
            // become exactly these (CR 205.1a-b), so it isn't a planeswalker
            // then: damage to it is marked, not loyalty lost (120.3; Kaito's
            // ruling). Its loyalty abilities stay (606.3).
            Mod::Types { add: types::CREATURE, subtypes, replace: true },
            Mod::SetPt(l.int("creature_during_your_turn.p", *p, 0, 99), l.int("creature_during_your_turn.t", *t, 0, 99)),
        ];
        mods.extend(super::effect_ext::keyword_mods(l, kws));
        let cond = crate::db::Cond::All(vec![
            crate::db::Cond::YourTurn,
            crate::db::Cond::SourceCountersIn(crate::db::Counter::Loyalty, 1, None),
        ]);
        statics.push(StaticDef { scope: Scope::This, cond: Some(cond), mods });
    }
    // "Whenever you tap a <land subtype> for mana, add an additional
    // <mana>" (a triggered mana ability, CR 605.1b).
    let mut extra_mana_on_tap = Vec::new();
    for pair in extra_mana_when_you_tap_land_subtype {
        match &pair[..] {
            [sub, mana] => match letter_index(mana) {
                Some(i) => extra_mana_on_tap.push((l.subtype(sub), i as u8)),
                None => l.unsupported(format!("CardPrinting.extra_mana_when_you_tap_land_subtype({mana})")),
            },
            _ => l.unsupported("CardPrinting.extra_mana_when_you_tap_land_subtype(shape)"),
        }
    }
    if let Some((tys, p, t)) = equipment_bonus_per_controlled {
        // "Equipped creature gets +p/+t for each <type> you control."
        if !subs.contains(&l.subtype("Equipment")) {
            l.unsupported("CardPrinting.equipment_bonus_per_controlled(not an Equipment)");
        }
        let mut bits = 0u16;
        for name in tys {
            match types::from_name(name) {
                Some(b) => bits |= b,
                None => l.unsupported(format!("CardPrinting.equipment_bonus_per_controlled({name})")),
            }
        }
        let filter = Filter::And(Box::new(Filter::Type(bits)), Box::new(Filter::Controller(true)));
        let (p, t) = (l.int("equipment_bonus_per_controlled.p", *p, -9, 9), l.int("equipment_bonus_per_controlled.t", *t, -9, 9));
        statics.push(StaticDef { scope: Scope::Equipped, cond: None, mods: vec![Mod::PtPer { filter, p, t }] });
    }
    // "Equipped creature gets +1/+1 for each +1/+1 counter on this
    // Equipment" (Lion Sash): [counter, p, t].
    if !equipment_bonus_per_own_counter.is_empty() {
        use card_ir::unions::IntOrStr as V;
        match &equipment_bonus_per_own_counter[..] {
            [V::Str(k), V::Int(p), V::Int(t)] if subs.contains(&l.subtype("Equipment")) => {
                let counter = super::effect_ext::counter_by_name(l, k);
                let (p, t) = (l.int("equipment_bonus_per_own_counter.p", *p, -9, 9), l.int("equipment_bonus_per_own_counter.t", *t, -9, 9));
                statics.push(StaticDef { scope: Scope::Equipped, cond: None, mods: vec![Mod::PtPerSourceCounter { counter, p, t }] });
            }
            _ => l.unsupported("CardPrinting.equipment_bonus_per_own_counter(shape)"),
        }
    }
    if let Some((p, t, names)) = equipment_bonuses {
        // (An Aura's "enchanted creature gets ...": the IR reuses the
        // Equipment field, docs/ir_defects.md 7; the Aura's attachment is
        // the same `attached`.)
        if !subs.contains(&l.subtype("Equipment")) && !(subs.contains(&l.subtype("Aura")) && (aura_gy || aura_bf.is_some())) {
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
        // "Gets +X/+0" (a static ability, ENGINE.md 99.2).
        Some(f) => match super::eoe_a::pt_formula_static(f) {
            Some(st) if is_creature => {
                statics.push(st);
                None
            }
            Some(_) => {
                l.unsupported("CardPrinting.dynamic_pt_formula(not a creature)");
                None
            }
            None => super::statics::lower_pt_formula(l, f),
        },
    };
    if pt_cda.is_some() && !is_creature {
        l.unsupported("CardPrinting.dynamic_pt_formula(not a creature)");
    }
    let unblockable_by = match cant_be_blocked_by.as_deref() {
        None => None,
        Some(p) => super::filter::lower_pred(l, p),
    };
    // Alternative costs (CR 118.9): the IR's list, "exile a card from your
    // hand rather than pay" (Pyrokinesis), evoke (702.74).
    let mut alt_costs = Vec::new();
    for a in alternative_costs {
        if let Some(a) = lower_alt_cost(l, a) {
            alt_costs.push(a);
        }
    }
    if let Some(f) = alt_cost_exile_from_hand {
        if let Some(f) = lower_card_filter(l, f) {
            alt_costs.push(AltCost { cost: Cost { exile_from_hand: Some(f), ..Cost::default() }, ..AltCost::default() });
        }
    }
    if let Some(f) = evoke_exile_from_hand {
        if ty & types::CREATURE == 0 {
            l.unsupported("CardPrinting.evoke_exile_from_hand(not a creature)");
        }
        if let Some(f) = lower_card_filter(l, f) {
            alt_costs.push(AltCost {
                cost: Cost { exile_from_hand: Some(f), ..Cost::default() },
                evoke: true,
                ..AltCost::default()
            });
        }
    }
    alt_costs.extend(dash);
    alt_costs.extend(warp);
    // Emerge (CR 702.119a): "you may cast this spell by paying [cost] and
    // sacrificing a <type> rather than paying its mana cost"; the cost is
    // reduced by its mana value (702.119b, 601.2f).
    match emerge_cost.as_deref() {
        Some(m) => {
            let mana = super::effect_copies::checked_cost(l, "CardPrinting.emerge_cost", m, emerge_cost_str);
            let bits = types::from_name(emerge_sacrifice_type).unwrap_or_else(|| {
                l.unsupported(format!("CardPrinting.emerge_sacrifice_type({emerge_sacrifice_type})"));
                0
            });
            if ty & types::CREATURE == 0 || bits & types::PERMANENT == 0 {
                l.unsupported("CardPrinting.emerge_cost(not a creature, or not a permanent type)");
            }
            alt_costs.push(AltCost { mana, emerge: Some(Filter::Type(bits)), ..AltCost::default() });
        }
        None => {
            l.deny("CardPrinting.emerge_cost_str", emerge_cost_str);
            l.deny_ne("CardPrinting.emerge_sacrifice_type", emerge_sacrifice_type, &generated::defaults::d_CardPrinting_emerge_sacrifice_type());
        }
    }
    if let Some((mana, _, _)) = &bestow {
        alt_costs.push(AltCost { mana: *mana, bestow: true, ..AltCost::default() });
    }
    // "If this spell is the first spell you've cast this game, you may cast
    // it without paying its mana cost" (an alternative cost, CR 118.9).
    match free_cast_condition.as_deref() {
        None => {}
        Some(cats::Condition::FirstSpellYouCastThisGame(_)) => {
            alt_costs.push(AltCost { only_if: Some(crate::db::Cond::FirstSpellThisGame), ..AltCost::default() });
        }
        Some(other) => l.unsupported(format!("CardPrinting.free_cast_condition({})", other.tag())),
    }
    // The cast pays an alternative cost's rest alone (`Game::alt_payable`,
    // `cast_pay`, `item_cost`), and a printed escape cost's exile alone
    // (`Game::route_extra`): with additional costs of the card's own, the
    // kicker's, buyback's or spree's mode costs (CR 601.2b, 601.2f, 702.172a:
    // the total cost is both), not supported. No cube card has either
    // (ENGINE.md 40.9).
    let spree = spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| !m.costs.is_empty());
    if !alt_costs.is_empty() && (!additional.is_empty() || spree) {
        l.unsupported("CardPrinting.alternative_costs(with additional costs)");
    }
    if escape.is_some() && !additional.is_empty() {
        l.unsupported("CardPrinting.escape_cost(with additional costs)");
    }
    let discounts = lower_discounts(l, cost_reductions);
    // (A reduction applies to an alternative cost too, CR 601.2f, 118.9d:
    // `Game::alt_mana`.)
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
    l.spell_x = saved_spell_x;

    // The card's other faces (CR 712, 715; ENGINE.md 25): each is lowered
    // like a printing into a definition of its own (`l.faces`).
    let (mut back, mut modal, mut adventure) = (None, false, None);
    let mut back_nightbound = false;
    // A Class (CR 716): the IR's `class_levels` are the printings of its
    // levels 2, 3, ..., each with every ability the Class has at that level;
    // each is lowered as a face (gaining a level turns it up).
    let mut levels: Vec<crate::ids::DefId> = Vec::new();
    let class_lv = l.int("CardPrinting.class_level", *class_level, 0, 9) as u8;
    let class_sub = l.subtype("Class");
    match face {
        super::Face::Level => {
            if class_lv < 2 || !class_levels.is_empty() {
                l.unsupported("CardPrinting.class_levels(a level's shape)");
            }
        }
        _ if class_lv == 0 && class_levels.is_empty() => {}
        super::Face::Card if class_lv == 1 && !class_levels.is_empty() && subs.contains(&class_sub) && ty == types::ENCHANTMENT => {
            for (i, lp) in class_levels.iter().enumerate() {
                let cats::CardPrinting::CardPrinting(lp) = lp;
                if lp.class_level != i as i64 + 2 || lp.name != *name || lp.mana_cost != *mana_cost || lp.card_types != *card_types || lp.subtypes != *subtypes {
                    l.unsupported("CardPrinting.class_levels(a level unlike its card)");
                }
                l.face = super::Face::Level; // (taken by the face's lower_printing)
                let d = lower_printing(l, lp);
                l.faces.push(d);
                levels.push((l.faces.len() - 1) as crate::ids::DefId);
            }
        }
        _ => l.unsupported("CardPrinting.class_level(not a Class with levels)"),
    }
    if face != super::Face::Card {
        l.deny("CardPrinting.transform_back_face(on a face)", transform_back_face);
        l.deny("CardPrinting.mdfc_back_face(on a face)", mdfc_back_face);
        l.deny("CardPrinting.adventure_face(on a face)", adventure_face);
    } else {
        let lower_face = |l: &mut Lowerer, f: &nodes::CardPrinting, face: super::Face| -> (u16, bool) {
            l.face = face; // (taken by the face's lower_printing)
            let d = lower_printing(l, f);
            let night = d.nightbound;
            l.faces.push(d);
            ((l.faces.len() - 1) as u16, night)
        };
        match (transform_back_face.as_deref(), mdfc_back_face.as_deref()) {
            (None, None) => {}
            // A nonmodal double-faced card (CR 712.2). The IR repeats the
            // front's mana cost on the back face (docs/ir_defects.md 9):
            // the back face has none (its mana value is the front's, CR
            // 712.8e).
            (Some(cats::CardPrinting::CardPrinting(t)), None) => {
                let mut t = t.clone();
                let cats::ManaCost::ManaCost(m) = &*t.mana_cost;
                let empty = nodes::ManaCost { generic: 0, colored: Vec::new(), x_pips: 0, hybrid: Vec::new(), phyrexian: Vec::new() };
                if t.mana_cost == *mana_cost && t.mana_cost_str.is_empty() || *m == empty {
                    t.mana_cost = Box::new(cats::ManaCost::ManaCost(empty));
                } else {
                    l.unsupported("CardPrinting.transform_back_face.mana_cost(not the front face's)");
                }
                let (k, night) = lower_face(l, &t, super::Face::Back);
                l.faces[k as usize].front_mv = Some(cost.mana_value());
                back = Some(k);
                back_nightbound = night;
            }
            // A modal double-faced card (CR 712.3), possibly one whose face
            // can transform: the IR then describes the back face twice,
            // which must agree (ENGINE.md 3, rule 4a).
            (t, Some(cats::CardPrinting::CardPrinting(m))) => {
                if let Some(cats::CardPrinting::CardPrinting(t)) = t {
                    if t != m {
                        l.unsupported("CardPrinting.mdfc_back_face(differs from transform_back_face)");
                    }
                }
                let (k, _) = lower_face(l, m, super::Face::ModalBack);
                back = Some(k);
                modal = true;
            }
        }
        if let Some(cats::CardPrinting::CardPrinting(a)) = adventure_face.as_deref() {
            // CR 715.2: an Adventure is an instant or sorcery spell.
            let (k, _) = lower_face(l, a, super::Face::Adventure);
            let ad = &l.faces[k as usize];
            if ad.types & (types::INSTANT | types::SORCERY) == 0 || ad.types & types::PERMANENT != 0 {
                l.unsupported("CardPrinting.adventure_face(not an instant or sorcery)");
            }
            if ty & types::PERMANENT == 0 {
                l.unsupported("CardPrinting.adventure_face(not on a permanent card)");
            }
            if back.is_some() {
                l.unsupported("CardPrinting.adventure_face(on a double-faced card)");
            }
            adventure = Some(k);
        }
    }
    // A preparation card (CR 722): its prepare spell is a face of its own
    // (an instant or sorcery) that copies of the card in exile have
    // (722.3c); "this creature enters prepared" (722.3a).
    let mut prepare = None;
    match prepare_spell.as_deref() {
        Some(cats::CardPrinting::CardPrinting(ps)) => {
            if face != super::Face::Card || ty & types::PERMANENT == 0 {
                l.unsupported("CardPrinting.prepare_spell(not on a permanent card)");
            }
            l.face = super::Face::Prepare;
            let d = lower_printing(l, ps);
            if d.types & (types::INSTANT | types::SORCERY) == 0 || d.types & types::PERMANENT != 0 {
                l.unsupported("CardPrinting.prepare_spell(not an instant or sorcery)");
            }
            l.faces.push(d);
            prepare = Some((l.faces.len() - 1) as u16);
        }
        None => l.deny("CardPrinting.enters_prepared(without a prepare spell)", enters_prepared),
    }
    // A split card (CR 709): its two halves, each a face of its own; the
    // card's characteristics are the halves' combined (709.4: the IR states
    // them, and they must agree).
    let mut halves = None;
    if !split_faces.is_empty() {
        if face != super::Face::Card || split_faces.len() != 2 || back.is_some() || adventure.is_some() || spell_effect.is_some() {
            l.unsupported("CardPrinting.split_faces(shape)");
        } else {
            let mut ks = [0u16; 2];
            let mut sum = ManaCost::default();
            let mut tys = 0u16;
            for (i, h) in split_faces.iter().enumerate() {
                let cats::CardPrinting::CardPrinting(h) = h;
                l.face = super::Face::Half;
                let d = lower_printing(l, h);
                if d.spell.is_none() {
                    l.unsupported("CardPrinting.split_faces(a half without a spell)");
                }
                sum = sum.plus(&d.cost);
                tys |= d.types;
                l.faces.push(d);
                ks[i] = (l.faces.len() - 1) as u16;
            }
            if sum != cost || tys != ty {
                l.unsupported("CardPrinting.split_faces(differ from the card's mana cost or types)");
            }
            halves = Some((ks[0], ks[1]));
        }
    }
    // "Spells you cast from your graveyard cost {n} less to cast" (an
    // effect on its controller's spells, CR 601.2f).
    let mut gy_spell_discount = 0u8;
    let mut outside_hand_discount = 0u8;
    let mut casting = crate::db::CastingRules {
        one_noncreature_spell: *restricts_noncreature_spells_to_one_per_turn,
        first_spell_free: *grants_first_spell_each_turn_free,
        warp_from_graveyard: *warp_castable_from_graveyard,
        convoke: *has_convoke,
        choose_creature_or_warped: *additional_cost_choose_your_creature,
        hand_alt_grants,
        ..Default::default()
    };
    for r in external_cost_reductions {
        let cats::ExternalCostReduction::ExternalCostReduction(nodes::ExternalCostReduction { amount, condition }) = r;
        match &**condition {
            cats::Condition::SpellCastFromGraveyard(nodes::SpellCastFromGraveyard {}) => {
                gy_spell_discount += l.int("ExternalCostReduction.amount", *amount, 1, 9) as u8;
            }
            // "Spells you cast from anywhere other than your hand cost {1}
            // less to cast" (Bilbo).
            cats::Condition::SpellCastFromOutsideHand(nodes::SpellCastFromOutsideHand {}) => {
                outside_hand_discount += l.int("ExternalCostReduction.amount", *amount, 1, 9) as u8;
            }
            // Lane C's (ENGINE.md 99.4).
            other => {
                let n = l.int("ExternalCostReduction.amount", *amount, 1, 9) as u8;
                if let Some(d) = super::eoe_c::spell_discount(l, other, n) {
                    casting.spell_discounts.push(d);
                }
            }
        }
    }
    // Daybound and nightbound (CR 702.145) are on the two faces of one
    // double-faced card.
    match face {
        super::Face::Card => {
            if *daybound != back_nightbound || (*daybound && modal) {
                l.unsupported("CardPrinting.daybound(without a nightbound back face)");
            }
            l.deny("CardPrinting.nightbound(on a front face)", nightbound);
        }
        super::Face::Back => l.deny("CardPrinting.daybound(on a back face)", daybound),
        _ => {
            l.deny("CardPrinting.daybound(on a modal face)", daybound);
            l.deny("CardPrinting.nightbound(on a modal face)", nightbound);
        }
    }
    // Gemstone Caverns (M7d): "if it has a luck counter on it, instead add
    // one mana of any color" summarises its mana ability (`IfCounter`).
    if let Some((kind, colors)) = produces_with_counter {
        let counter = super::effect_ext::counter_by_name(l, kind);
        let mut mask = 0;
        for c in colors {
            match letter_index(c) {
                Some(i) => mask |= 1 << i,
                None => l.unsupported(format!("CardPrinting.produces_with_counter({c})")),
            }
        }
        let ok = activated.iter().any(|a| {
            matches!(a.mana().map(|m| &m.prod), Some(ManaProd::IfCounter { counter: k, then, .. }) if *k == counter && *then == mask)
        });
        if !ok {
            l.unsupported("CardPrinting.produces_with_counter(without its mana ability)");
        }
    }
    // An opening-hand action (CR 103.6; Gemstone Caverns).
    let opening = opening_hand_action.as_deref().map(|o| {
        let cats::BeginGameOnBattlefield::BeginGameOnBattlefield(nodes::BeginGameOnBattlefield {
            only_if_not_starting_player,
            counters,
            exile_from_hand,
        }) = o;
        if ty & types::PERMANENT == 0 {
            l.unsupported("CardPrinting.opening_hand_action(not a permanent card)");
        }
        let counters = counters
            .iter()
            .map(|(k, n)| (super::effect_ext::counter_by_name(l, k), l.int("BeginGameOnBattlefield.counters", *n, 1, 9) as u8))
            .collect();
        crate::db::Opening {
            not_starting: *only_if_not_starting_player,
            counters,
            exile: l.int("BeginGameOnBattlefield.exile_from_hand", *exile_from_hand, 0, 1) as u8,
        }
    });
    let void_exile = void_exile_replacement.then(|| super::effect_ext::counter_by_name(l, "void"));
    // Anointed Peacekeeper: the name chosen as it enters, and its taxes.
    let named_spell_tax = l.int("CardPrinting.named_spell_cost_increase", *named_spell_cost_increase, 0, 9) as u8;
    let named_ability_tax = l.int("CardPrinting.named_ability_cost_increase", *named_ability_cost_increase, 0, 9) as u8;
    if (named_spell_tax > 0 || named_ability_tax > 0) && !*chooses_card_name_as_enters || *chooses_card_name_as_enters && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.named_spell_cost_increase(without a name chosen as it enters)");
    }
    // "Nonbasic lands are Mountains": a static ability of a permanent
    // (ENGINE.md 99.12).
    if *nonbasic_lands_are_mountains && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.nonbasic_lands_are_mountains(not a permanent)");
    }
    // Devour land n (CR 702.82a): a creature's.
    let devour_land = if *devour_land_multiplier != 0 {
        if ty & types::CREATURE == 0 {
            l.unsupported("CardPrinting.devour_land_multiplier(not a creature)");
        }
        l.int("CardPrinting.devour_land_multiplier", *devour_land_multiplier, 1, 9) as u8
    } else {
        0
    };
    if *grants_etb_counter_per_land_entered_this_turn && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.grants_etb_counter_per_land_entered_this_turn(not a permanent)");
    }
    if *grants_play_from_top_paying_life && !*grants_look_at_top_of_library || (*grants_look_at_top_of_library && ty & types::PERMANENT == 0) {
        l.unsupported("CardPrinting.grants_play_from_top_paying_life(without looking at the top card)");
    }
    // Companion (CR 702.139): its deck condition (ENGINE.md 35). A face
    // can't be a companion (the card's own characteristics are its front
    // face's outside the game).
    let companion = match companion.as_deref() {
        Some(c) if face == super::Face::Card => super::filter::lower_companion(l, c),
        Some(_) => {
            l.unsupported("CardPrinting.companion(on a face)");
            None
        }
        None => None,
    };
    // "This creature enters with a +1/+1 counter on it for each <filter>
    // card exiled with it" (Murktide Regent, ENGINE.md 54): linked to the
    // ability printed on the card that exiles cards while paying the cost
    // to cast it (CR 607.2q), here delve (a granted escape, Underworld
    // Breach's, isn't printed on it: its cards don't count); the filter
    // reads a card's own characteristics (the cards are in exile). The
    // card's own escape (also printed) isn't recorded: a blocker, never a
    // silent zero.
    let counters_per_exiled = match enters_with_counter_per_card_exiled_with_it.as_deref() {
        None => None,
        Some(x) => {
            let f = super::filter::lower_pred(l, x);
            if !*has_delve || ty & types::PERMANENT == 0 || escape.is_some() {
                l.unsupported("CardPrinting.enters_with_counter_per_card_exiled_with_it(without delve, or with escape)");
            }
            match f {
                Some(f) if crate::db::deck_filter_ok(&f) => Some(f),
                Some(f) => {
                    l.unsupported(format!("CardPrinting.enters_with_counter_per_card_exiled_with_it({f:?}: a filter that needs a game)"));
                    None
                }
                None => None,
            }
        }
    };
    // "Enters with a +1/+1 counter on it for each <filter> you control"
    // (Luxknight Breacher; ENGINE.md 99.11).
    let enters_counters_per = match enters_with_plus_one_counters_per.as_deref() {
        None => None,
        Some(x) => {
            if ty & types::PERMANENT == 0 {
                l.unsupported("CardPrinting.enters_with_plus_one_counters_per(not a permanent)");
            }
            super::filter::lower_pred(l, x)
        }
    };
    // "Enchanted creature can't be blocked by more than one creature"
    // (Meltstrider's Resolve; ENGINE.md 99.11): an Aura's.
    if *attached_cant_be_blocked_by_more_than_one && !subtypes.iter().any(|s| s == "Aura") {
        l.unsupported("CardPrinting.attached_cant_be_blocked_by_more_than_one(not an Aura)");
    }
    let mut def = CardDef {
        nonbasic_lands_are_mountains: *nonbasic_lands_are_mountains && ty & types::PERMANENT != 0,
        etb_counter_per_land_entered: *grants_etb_counter_per_land_entered_this_turn && ty & types::PERMANENT != 0,
        devour_land,
        counters_per_exiled,
        enters_counters_per,
        attached_max_one_blocker: *attached_cant_be_blocked_by_more_than_one,
        companion,
        look_top: *grants_look_at_top_of_library,
        play_top_life: *grants_play_from_top_paying_life,
        chooses_name: *chooses_card_name_as_enters,
        named_spell_tax,
        named_ability_tax,
        class_level: class_lv,
        levels,
        reconfigure,
        void_exile,
        opening,
        name: name.clone(),
        types: ty,
        supertypes: sup,
        subtypes: subs,
        colors: if devoid { 0 } else { cost.colors() | indicator },
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
        alt_costs,
        discounts,
        delayed_base,
        flashback,
        escape,
        lands_from_graveyard: *grants_play_lands_from_graveyard,
        combat_unpreventable: *your_creatures_combat_damage_cant_be_prevented,
        chooses_player,
        delve: *has_delve,
        kicker: kicker.is_some(),
        discard_land_to_enter: *enters_only_by_discarding_land,
        x_pips,
        enters_x_counters,
        // "You may play any number of lands on each of your turns"
        // (Fastbond): the IR's sentinel 1,000,000 (docs/ir_defects.md 10),
        // read as `ANY_LANDS`, more land drops than a game has lands.
        extra_lands: if *grants_additional_land_per_turn == 1_000_000 {
            crate::db::ANY_LANDS
        } else {
            l.int("CardPrinting.grants_additional_land_per_turn", *grants_additional_land_per_turn, 0, 3) as u8
        },
        buyback,
        overload: overloaded.is_some(),
        one_spell_each_turn: *each_player_one_spell_per_turn,
        crew_bonus: l.int("CardPrinting.crew_power_bonus", *crew_power_bonus, 0, 9),
        opp_enter_tapped: l.opp_enter_tapped,
        opp_nonbasic_enter_tapped: *opponents_nonbasic_lands_enter_tapped,
        grants_escape: (*grants_graveyard_escape_exile_count > 0).then(|| Cost {
            exile_graveyard: l.int("CardPrinting.grants_graveyard_escape_exile_count", *grants_graveyard_escape_exile_count, 1, 20) as u8,
            ..Cost::default()
        }),
        loyalty: printed_loyalty,
        loyalty_instant_if_entered,
        opp_draw_limit: *opponents_cant_draw_more_than_one_card_each_turn,
        opp_sorcery_speed: *opponents_cast_spells_only_at_sorcery_speed,
        no_forced_discard_sac: *opponents_cant_make_you_discard_or_sacrifice,
        token_factor: repl.token_factor,
        wins_on_empty_draw: repl.wins_on_empty_draw,
        gy_to_library: repl.gy_to_library,
        opp_draw_token: repl.opp_draw_token,
        draw_plus: repl.draw_plus,
        extra_mana_on_tap,
        emblem: false,
        // "During your turn, nonland permanent cards in your graveyard have
        // retrace" (Six; on a permanent, during its controller's turn).
        grants_retrace: grants_retrace_to_nonland_permanents_during_your_turn.then(|| {
            Filter::And(Box::new(Filter::Type(types::PERMANENT)), Box::new(Filter::Not(Box::new(Filter::Type(types::LAND)))))
        }),
        back,
        modal,
        adventure,
        front: None,
        front_mv: None,
        daybound: *daybound,
        nightbound: *nightbound,
        chapters,
        opp_no_spells_your_turn: *opponents_cant_cast_spells_during_your_turn,
        latent: n_latent,
        no_max_hand: repl.no_max_hand,
        halves,
        gy_spell_discount,
        grants_mayhem: *grants_mayhem_to_graveyard,
        enters_as_copy,
        times_cost,
        bestow: None,
        bestowed: false,
        prepare,
        grants_equipped: None,
        exiles_granter: false,
        enters_prepared: *enters_prepared,
        spells_uncounterable: repl.spells_uncounterable,
        ward,
        grants_ward: repl.grants_ward,
        improvise: *has_improvise,
        rules: false,
        gy_abilities,
        artifact_ability_discount: l.int("CardPrinting.artifact_ability_cost_reduction", *artifact_ability_cost_reduction, 0, 9) as u8,
        skip_turn_untap: *may_skip_turn_to_untap,
        converge: false,
        paints: *paints_all_objects_chosen_color_at_etb,
        extra_mana_creature,
        gy_play_mv: if repl.gy_cast_mv > 0 { repl.gy_cast_mv } else { gy_play_mv },
        gy_play_k: if repl.gy_cast_mv > 0 { crate::db::NO_GRANT } else { gy_play_k },
        gy_play_land: gy_play_mv > 0,
        ability_discount: repl.ability_discount,
        ascend: cascade_kw_ascend,
        outside_hand_discount,
        casting,
        toughness_assigns_damage: *grants_toughness_as_damage_for_creatures_you_control,
        toughness_stations: *grants_toughness_as_station_power_for_creatures_you_control,
        chooses_land_type,
        x_life: *additional_cost_pay_x_life,
        sunburst: sunburst_counter,
        enters_times_counters: *enters_with_counter_per_kick,
        aura_spell: aura_gy || aura_bf.is_some(),
        reanimates: l.reanimates,
        cleanup_sac,
        flash_permission,
        x_min: x_least,
        gy_triggers,
        chooses_parity: *chooses_parity_as_enters,
        blocks_only_subtype: (!blocks_only_with_subtype.is_empty()).then(|| l.subtype(blocks_only_with_subtype)),
        splice,
        miracle,
        w3: crate::eoe_w3::W3Def {
            // "Combat damage can't be prevented" (CR 615.12; Frenzied Baloth).
            combat_unpreventable_all: *combat_damage_cant_be_prevented,
            creature_spells_uncounterable: repl.creature_spells_uncounterable,
            seen: 0,
            mana_spent: false,
            counters_mana_spent: *enters_with_counters_from_x == Some(enums::XCounterMode::ManaSpent),
            grants_activated: Vec::new(),
            grants_subtype: Vec::new(),
        },
    };
    super::eoe_w3::printing_w3(l, &mut def, *grants_food_to_your_artifacts, *grants_food_ability_to_your_artifacts);
    def.w3.seen = crate::eoe_w3::seen_of(&def);
    def.w3.mana_spent = def.w3.counters_mana_spent || crate::eoe_w3::reads_mana_spent(&def);
    if *additional_cost_pay_x_life && (x_pips > 0 || !is_spell_card) {
        l.unsupported("CardPrinting.additional_cost_pay_x_life(with {X} or on a permanent)");
    }
    if *grants_retrace_to_nonland_permanents_during_your_turn && ty & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.grants_retrace_to_nonland_permanents_during_your_turn(not a permanent)");
    }
    if repl.gy_cast_mv > 0 && gy_play_mv > 0 {
        l.unsupported("StaticAbility.graveyard_permanent_cast_mv_max(with another graveyard permission)");
    }
    super::effect_m7d::check_self_shuffle(l, &def);
    // The bestowed face (CR 702.103b): an Aura enchantment with enchant
    // creature (an Aura spell targets a creature, 303.4a) whose enchanted
    // creature gets the bonus; no creature types, no power and toughness;
    // its other abilities are the card's.
    if let Some((_, p, t)) = bestow {
        let mut f = def.clone();
        f.types = types::ENCHANTMENT;
        f.subtypes = vec![l.subtype("Aura")];
        f.power = None;
        f.toughness = None;
        f.pt_cda = None;
        f.statics.push(StaticDef { scope: Scope::Equipped, cond: None, mods: vec![Mod::Pt(p, t)] });
        let req = crate::db::TargetReq { filter: Filter::Type(types::CREATURE), min: 1, max: 1, distinct: 0, pair: 0, total_mv: None };
        f.spell = Some(crate::db::AbilityDef { targets: vec![req], ..Default::default() });
        f.bestowed = true;
        l.faces.push(f);
        def.bestow = Some((l.faces.len() - 1) as u16);
    }
    if *grants_control_opponent_ability_when_equipped {
        super::eoe_w4::grant_control_ability(l, &mut def);
    }
    def
}

/// The `enters_as_copy_*` fields of a printing.
struct EnterCopyFields<'a> {
    types: &'a [String],
    zone: &'a enums::ZoneName,
    add_type: &'a str,
    keep_name: bool,
    base_pt: &'a Option<(i64, i64)>,
    add_subtypes: &'a [String],
    exiles_card: bool,
    add_keywords: &'a [cats::Keyword],
}

/// "You may have this enter as a copy of any <types> on the battlefield /
/// any <types> card in a graveyard, except ..." (CR 707.5, 707.9b) -> its
/// [`EntersAsCopy`](crate::db::EntersAsCopy); "when you do, exile that card"
/// is a delayed ability of the card (603.11).
fn lower_enters_as_copy(l: &mut Lowerer, x: EnterCopyFields, card_types: u16) -> Option<Box<crate::db::EntersAsCopy>> {
    if x.types.is_empty() {
        l.deny_ne("CardPrinting.enters_as_copy_zone", x.zone, &generated::defaults::d_CardPrinting_enters_as_copy_zone());
        l.deny("CardPrinting.enters_as_copy_add_type", &x.add_type.to_string());
        l.deny("CardPrinting.enters_as_copy_keep_name", &x.keep_name);
        l.deny("CardPrinting.enters_as_copy_base_pt", x.base_pt);
        l.deny("CardPrinting.enters_as_copy_add_subtypes", &x.add_subtypes.to_vec());
        l.deny("CardPrinting.enters_as_copy_exiles_card", &x.exiles_card);
        if !x.add_keywords.is_empty() {
            l.unsupported("CardPrinting.enters_as_copy_add_keywords");
        }
        return None;
    }
    if card_types & types::PERMANENT == 0 {
        l.unsupported("CardPrinting.enters_as_copy_types(not a permanent card)");
    }
    let mut bits = 0u16;
    for t in x.types {
        match types::from_name(t) {
            Some(b) => bits |= b,
            None => l.unsupported(format!("CardPrinting.enters_as_copy_types({t})")),
        }
    }
    let from_graveyard = match x.zone {
        enums::ZoneName::Battlefield => false,
        enums::ZoneName::Graveyard => true,
        other => {
            l.unsupported(format!("CardPrinting.enters_as_copy_zone({other:?})"));
            false
        }
    };
    let mut exc = crate::copies::Except::default();
    if !x.add_type.is_empty() {
        match types::from_name(x.add_type) {
            Some(b) => exc.types |= b,
            None => l.unsupported(format!("CardPrinting.enters_as_copy_add_type({})", x.add_type)),
        }
    }
    if let Some((p, t)) = x.base_pt {
        exc.pt = Some((l.int("CardPrinting.enters_as_copy_base_pt", *p, 0, 99) as i8, l.int("CardPrinting.enters_as_copy_base_pt", *t, 0, 99) as i8));
    }
    for s in x.add_subtypes {
        let id = l.subtype(s);
        if !exc.add_subtype(id) {
            l.unsupported("CardPrinting.enters_as_copy_add_subtypes(too many)");
        }
    }
    for k in x.add_keywords {
        match lower_keyword_full(l, k) {
            Some(KwOrProt::Kw(kw)) => exc.kws.insert(kw),
            Some(_) => l.unsupported("CardPrinting.enters_as_copy_add_keywords(not a plain keyword)"),
            None => {}
        }
    }
    let exile_k = if x.exiles_card {
        if !from_graveyard {
            l.unsupported("CardPrinting.enters_as_copy_exiles_card(not a graveyard card)");
        }
        let mut d = crate::program::Builder::default();
        d.op(crate::program::Op::Ext(crate::program::Ext::ExileEventCard));
        Some(l.delayed_ability(d.finish()))
    } else {
        None
    };
    Some(Box::new(crate::db::EntersAsCopy { filter: Filter::Type(bits), from_graveyard, exc, keep_name: x.keep_name, exile_k, tapped: false }))
}

/// Replicate's triggered ability (CR 702.56a): "When you cast this spell,
/// if a replicate cost was paid for it, copy it for each time its replicate
/// cost was paid. If the spell has any targets, you may choose new targets
/// for any of the copies."
fn replicate() -> TriggerDef {
    let mut b = crate::program::Builder::default();
    b.op(crate::program::Op::Ext(crate::program::Ext::CopySpell {
        of: crate::program::SpellOf::EventSpell,
        who: crate::program::Who::You,
        n: crate::program::Amount::EventSpellTimes,
        may: false,
        new_targets: true,
    }));
    TriggerDef {
        event: EventKind::IsCast,
        pred: TrigPred::And(Box::new(TrigPred::IsSelf(Slot::Spell)), Box::new(TrigPred::SpellPaid)),
        ability: b.finish(),
        uses_event: true,
    }
}

/// "This spell / ability costs {n} less ... for each ..." (CR 601.2f).
pub(crate) fn lower_discounts(l: &mut Lowerer, cost_reductions: &[cats::CostReduction]) -> Vec<crate::db::Discount> {
    let mut discounts = Vec::new();
    for r in cost_reductions {
        let cats::CostReduction::CostReduction(nodes::CostReduction { amount, condition }) = r;
        let n = l.int("CostReduction.amount", *amount, 1, 9) as u8;
        match &**condition {
            cats::Condition::PerBasicLandTypeAmongLandsYouControl(nodes::PerBasicLandTypeAmongLandsYouControl {}) => {
                discounts.push(crate::db::Discount::PerBasicLandType(n))
            }
            // Affinity-like: "{1} less for each <permanent> you control".
            cats::Condition::PerControlled(nodes::PerControlled { filter }) => {
                if let Some(f) = super::filter::lower_pred(l, filter) {
                    discounts.push(crate::db::Discount::PerControlled(f, n));
                }
            }
            // (Edge of Eternities lane B's conditions, ENGINE.md 99.3.)
            c @ cats::Condition::PerDifferentlyNamedLand(_) => discounts.extend(super::eoe_b::lower_discount_eoe_b(l, n, c)),
            // Lane C's (ENGINE.md 99.4).
            other => discounts.extend(super::eoe_c::cost_reduction(l, other, n)),
        }
    }
    discounts
}

/// Storm's triggered ability (CR 702.40a): "When you cast this spell, copy
/// it for each other spell that was cast before it this turn. If the spell
/// has any targets, you may choose new targets for any of the copies." (The
/// cast event's amount is that number, `Game::cast_done`.)
fn storm() -> TriggerDef {
    let mut b = crate::program::Builder::default();
    b.op(crate::program::Op::Ext(crate::program::Ext::CopySpell {
        of: crate::program::SpellOf::EventSpell,
        who: crate::program::Who::You,
        n: crate::program::Amount::EventAmount,
        may: false,
        new_targets: true,
    }));
    TriggerDef { event: EventKind::IsCast, pred: TrigPred::IsSelf(Slot::Spell), ability: b.finish(), uses_event: true }
}

/// The chapter number of a chapter ability's predicate (CR 714.2b).
fn chapter_of(p: &TrigPred) -> Option<u8> {
    match p {
        TrigPred::Chapter(n) => Some(*n),
        TrigPred::And(a, b) => chapter_of(a).or_else(|| chapter_of(b)),
        _ => None,
    }
}

/// Spells with additional costs here are instants and sorceries (the cast
/// path pays them; permanent spells would need the same, untested).
fn is_spell_card_any(ty: u16) -> bool {
    ty & (types::INSTANT | types::SORCERY) != 0
}

/// An IR alternative cost -> [`AltCost`].
fn lower_alt_cost(l: &mut Lowerer, a: &cats::AlternativeCost) -> Option<AltCost> {
    let cats::AlternativeCost::AlternativeCost(nodes::AlternativeCost {
        mana,
        pay_life,
        exile_card_color,
        return_subtype,
        sacrifice_subtype,
        turn,
        requires_control_subtype,
        return_count,
        sacrifice_count,
        opponent_gains_life,
        discard_card_sharing_color,
        overload,
        condition,
    }) = a;
    l.deny("AlternativeCost.opponent_gains_life", opponent_gains_life);
    l.deny("AlternativeCost.discard_card_sharing_color", discard_card_sharing_color);
    // Overload changes "target" to "each" (CR 702.96): the spell's modes
    // (lower_printing).
    let _ = overload;
    l.deny("AlternativeCost.condition", condition);
    let mut out = AltCost::default();
    if !mana.is_empty() {
        match ManaCost::parse(mana) {
            Some(m) => out.mana = m,
            None => {
                l.unsupported(format!("AlternativeCost.mana({mana})"));
                return None;
            }
        }
    }
    out.cost.life = l.int("AlternativeCost.pay_life", *pay_life, 0, 99) as u8;
    if !exile_card_color.is_empty() {
        match letter_index(exile_card_color) {
            Some(i) if i < 5 => out.cost.exile_from_hand = Some(Filter::Color(1 << i)),
            _ => l.unsupported(format!("AlternativeCost.exile_card_color({exile_card_color})")),
        }
    }
    let n_ret = l.int("AlternativeCost.return_count", *return_count, 1, 9) as u8;
    if !return_subtype.is_empty() {
        let f = Filter::Subtype(l.subtype(return_subtype));
        out.cost.bounce = Some(SacCost { filter: f, n: n_ret, another: false });
    } else if n_ret != 1 {
        l.unsupported("AlternativeCost.return_count(without return_subtype)");
    }
    let n_sac = l.int("AlternativeCost.sacrifice_count", *sacrifice_count, 1, 9) as u8;
    if !sacrifice_subtype.is_empty() {
        let f = Filter::Subtype(l.subtype(sacrifice_subtype));
        out.cost.sacrifice = Some(SacCost { filter: f, n: n_sac, another: false });
    } else if n_sac != 1 {
        l.unsupported("AlternativeCost.sacrifice_count(without sacrifice_subtype)");
    }
    out.turn = turn.map(|t| t == enums::AlternativeCostTurn::Yours);
    if !requires_control_subtype.is_empty() {
        let f = Filter::Subtype(l.subtype(requires_control_subtype));
        out.only_if = Some(crate::db::Cond::ControlAtLeast(f, 1));
    }
    let parts = [out.cost.sacrifice.is_some(), out.cost.exile_from_hand.is_some()];
    if parts.iter().filter(|&&x| x).count() > 1 {
        l.unsupported("AlternativeCost(more than one reservation)");
    }
    Some(out)
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
/// A permanent's kicker cost -> (its mana, it is {X}). The IR's structured
/// cost counts {C} as generic (docs/ir_defects.md 5): its `kicker_cost_str`
/// says which pips are {C}, read when the two agree otherwise (ENGINE.md 3,
/// rule 4a).
fn kicker_mana(l: &mut Lowerer, m: &cats::ManaCost, s: &str) -> Option<(ManaCost, bool)> {
    let cats::ManaCost::ManaCost(n) = m;
    if n.x_pips != 0 {
        if (n.x_pips, n.generic) != (1, 0) || !n.colored.is_empty() || !n.hybrid.is_empty() || !n.phyrexian.is_empty() || s != "{X}" {
            l.unsupported("CardPrinting.kicker_cost(an X with more)");
            return None;
        }
        return Some((ManaCost::default(), true));
    }
    let plain = lower_mana_cost(l, "CardPrinting.kicker_cost", m);
    let Some(text) = ManaCost::parse(s) else {
        l.unsupported(format!("CardPrinting.kicker_cost_str({s})"));
        return None;
    };
    let mut same = text;
    same.generic = same.generic.saturating_add(same.pips[5]);
    same.pips[5] = 0;
    if same != plain {
        l.unsupported("CardPrinting.kicker_cost_str(differs from kicker_cost)");
        return None;
    }
    Some((text, false))
}

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
    Persist,
    /// Cascade (CR 702.85a): its triggered ability is in the IR too.
    Cascade,
    /// Ascend (CR 702.131a, on a permanent).
    Ascend,
    /// Annihilator n (CR 702.86a): its triggered ability is in the IR too.
    Annihilator(u8),
    /// Devoid (CR 702.114a): the card has no colour.
    Devoid,
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
                // (The player chosen as it entered; CR 614.12.)
                Q::TheChosenPlayer => p.chosen_player = true,
                // "Protection from spells that are one or more colors".
                Q::SpellsThatAreOneOrMoreColors => p.colored_spells = true,
                other => {
                    l.unsupported(format!("Keyword.quality({other:?})"));
                    return None;
                }
            }
            return Some(KwOrProt::Prot(p));
        }
        K::Exalted | K::BattleCry | K::Prowess | K::Persist => {
            l.deny("Keyword.n", n);
            l.deny("Keyword.quality", quality);
            return Some(match kind {
                K::Exalted => KwOrProt::Exalted,
                K::BattleCry => KwOrProt::BattleCry,
                K::Persist => KwOrProt::Persist,
                _ => KwOrProt::Prowess,
            });
        }
        K::Cascade => {
            l.deny("Keyword.n", n);
            l.deny("Keyword.quality", quality);
            return Some(KwOrProt::Cascade);
        }
        K::Ascend => {
            l.deny("Keyword.n", n);
            l.deny("Keyword.quality", quality);
            return Some(KwOrProt::Ascend);
        }
        // Annihilator n (CR 702.86a): its triggered ability is in the IR
        // too (checked to agree).
        K::Devoid => {
            l.deny("Keyword.n", n);
            l.deny("Keyword.quality", quality);
            return Some(KwOrProt::Devoid);
        }
        K::Annihilator => {
            l.deny("Keyword.quality", quality);
            return Some(KwOrProt::Annihilator(l.int("Keyword.n", *n, 1, 20) as u8));
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

/// Persist (CR 702.79a): "When this permanent is put into a graveyard from
/// the battlefield, if it had no -1/-1 counters on it, return it to the
/// battlefield under its owner's control with a -1/-1 counter on it" (the
/// counters seen as it left).
fn persist() -> TriggerDef {
    let mut b = crate::program::Builder::default();
    b.op(crate::program::Op::Ext(crate::program::Ext::ReturnDiedWithCounter { counter: crate::db::Counter::M1M1 }));
    TriggerDef {
        event: EventKind::Dies,
        pred: TrigPred::And(
            Box::new(TrigPred::IsSelf(Slot::Default)),
            Box::new(TrigPred::Subject(Slot::Default, Filter::NoCounters(crate::db::Counter::M1M1))),
        ),
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
        K::Infect => Kw::Infect,
        K::Shadow => Kw::Shadow,
        other => {
            l.unsupported(format!("Keyword:{other:?}"));
            return None;
        }
    })
}
