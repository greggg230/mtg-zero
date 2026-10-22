"""Map engine subsystems to the top-level functions/classes implementing
them and sum their line counts from ``_engine_outline.json`` (produced by
outline_engine.py).  Whole files are listed as ``file:*``.

Read-only.  Run from the mtg-bot repo's ``src/mtg_bot`` directory after
outline_engine.py, or anywhere (it only reads the outline JSON + files).
"""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUTLINE = json.loads((HERE / "_engine_outline.json").read_text(encoding="utf-8"))
SRC = Path(r"C:/Users/Gregg Keithley/Documents/Claude/Projects/mtg-bot/src/mtg_bot")

SUBSYSTEMS = {
    "Game-state model + copy-on-write cloning": {
        "game.py": ["Zone", "_CowList", "_make_shallow_cloner", "GameObject", "StackItem",
                     "Player", "ManaCoin", "LogBuffer", "GameState"],
    },
    "Turn structure, steps, priority loop": {
        "simulate.py": ["priority_loop", "run_step", "_advance_to_next_active_player",
                         "run_turn", "_next_alive"],
        "turn.py": ["Step", "next_step", "run_untap_step", "run_draw_step", "draw_card",
                     "draw_n_cards", "run_cleanup_step"],
        "roto_turns.py": ["turn_structure_next_step", "end_the_turn", "skips_draw_step",
                           "schedule_at_your_next_end_step"],
    },
    "Legal-action generation (priority, timing, castability)": {"legal.py": "*"},
    "Casting spells + alternative/additional costs": {
        "actions.py": ["CastSpell", "_cast_spell", "_select_spell_targets", "_choose_modes_at_cast",
                        "_effective_spell_effect_for_targets", "cast_time_effect",
                        "_targets_depend_on_x", "_plot_card", "_pay_teamwork",
                        "_choose_replicate_times", "_roto_top_of_library_cast_allowed",
                        "apply_cost_reductions", "_evaluate_cost_reduction",
                        "_apply_external_cost_reductions", "_x_upper_bound", "_choose_and_apply_x",
                        "effective_warp_cost", "_mayhem_permission", "_lurrus_permission_for"],
        "alt_casting.py": "*", "evoke.py": "*", "creature_casting_ub.py": "*",
        "graveyard_casting_wg.py": "*", "cards/spell_costs.py": "*",
    },
    "Mana pool, mana abilities, auto-payment (convoke/improvise/delve/phyrexian)": {
        "actions.py": ["_untapped_lands", "_land_auto_pays", "_land_tap_life_cost",
                        "_coin_matches_color", "mana_spendable_as_any_color", "_pip_passes",
                        "_coin_satisfies_context", "_phyrexian_split_affordable",
                        "_settle_phyrexian_pips", "can_pay_cost", "extra_mana_grants_for_tapped_lands",
                        "_extra_mana_lands", "_extra_coins_for_land", "_virtual_pretap",
                        "_snow_mana_lands", "_can_pay_from", "_convoke_candidates",
                        "_improvise_candidates", "_improvise_max_contribution", "_apply_improvise",
                        "_cost_is_zero", "_apply_convoke_pip", "_convoke_max_contribution",
                        "_apply_convoke", "_classify_sac_for_mana", "_sac_for_mana_sources",
                        "_can_fire_sac_for_mana", "_produces_satisfies", "_produces_total",
                        "_sac_for_mana_max_contribution", "_activate_sac_for_mana_for_cost",
                        "auto_tap_for_cost", "_pretap_lands_for_extra_mana", "max_distinct_colors"],
        "game.py": ["effective_land_produces", "_granted_tap_mana"],
    },
    "Activated abilities (incl. loyalty, hand/graveyard abilities, station)": {
        "actions.py": ["ActivateAbility", "ActivateStation", "_effective_ability_mana_cost",
                        "_choose_ability_targets", "_activate_hand_ability",
                        "_activate_graveyard_ability", "_activate_ability", "_activate_station"],
        "game.py": ["effective_activated_abilities"],
    },
    "Stack + resolution (fizzle, copies on stack, counters)": {
        "actions.py": ["move_spell_off_stack", "drain_stack", "resolve_top_of_stack",
                        "_resolve_top_of_stack_item", "push_spell_copy",
                        "_send_to_graveyard_or_library", "apply_action"],
        "keyword_rules.py": ["targets_all_illegal_on_resolution"],
    },
    "Triggered abilities (dispatch, delayed, state-triggers)": {
        "actions.py": ["push_triggered_ability", "dispatch_event", "_dispatch_graveyard_triggers",
                        "_trigger_etb_abilities", "_prowess_ability", "fire_prowess_triggers",
                        "push_persist_trigger", "_fire_unlock_door_triggers",
                        "_flush_creature_token_creations"],
        "turn.py": ["fire_counters_added_to_your_creature_triggers", "_fire_leaves_battlefield_triggers",
                     "fire_opponent_discards_triggers", "should_fire_trigger", "_dispatch_is_sacrificed",
                     "fire_landfall_triggers", "fire_opponent_plays_land_triggers", "_fire_dies_triggers",
                     "run_beginning_of_upkeep_triggers", "run_beginning_of_combat_triggers",
                     "run_delayed_end_step_triggers", "run_delayed_upkeep_triggers",
                     "_push_delayed_entries", "run_beginning_of_postcombat_main_triggers",
                     "run_beginning_of_precombat_main_triggers", "run_delayed_main_phase_triggers",
                     "run_beginning_of_end_step_triggers", "run_impending_decrement_turn_based_action"],
        "game.py": ["effective_triggered_abilities", "etb_trigger_multiplier"],
        "roto_board_statics.py": ["GrantsTriggerToCreatures", "_wiped_after", "granted_triggers",
                                   "_registry_granted", "fire_granted_triggers", "_observed",
                                   "fire_combat_damage_dealt", "fire_becomes_blocked", "StateTrigger",
                                   "_pending", "apply_board_state_checks"],
    },
    "Layers 1-7 / continuous effects / characteristic queries": {
        "game.py": ["is_creature_now", "normalize_added_card_type", "_layer4_added_card_types",
                     "effective_card_types", "has_card_type", "is_artifact_now",
                     "snapshot_departure_card_types", "clear_departure_card_types",
                     "sync_creature_type_cache", "_source_layer6_wiped", "_static_bonuses",
                     "_layer6_entries_for", "effective_keywords", "is_cant_be_blocked",
                     "effective_ward_cost", "_dynamic_base_pt", "effective_subtypes",
                     "effective_colors", "effective_protection_from_colors",
                     "magus_of_the_moon_active", "revert_printing_if_no_layer1_copies",
                     "_has_basic_land_type_set_effect", "_registry_set_base_pt",
                     "_compute_effective_pt", "effective_power", "effective_toughness",
                     "ContinuousEffect"],
        "effects/modifiers.py": ["active_static_abilities", "walk_continuous_modifiers"],
        "roto_board_statics.py": ["abilities_suppressed", "lands_without_rules_text",
                                   "land_enters_without_rules_text", "rules_in_play", "any_rules"],
        "cube_glue_statics.py": "*", "creature_statics_wg.py": ["apply_ascend"],
    },
    "Replacement + prevention effects": {
        "effects/modifiers.py": ["_apply_transform", "dispatch_replacement", "_source_by_id",
                                  "_collect_replacement_effects", "_subject_filters_match"],
        "turn.py": ["_draw_limited_to_one_per_turn", "_draw_from_empty_library_wins", "_draw_replaced",
                     "apply_void_exile_replacement", "apply_graveyard_exile_replacement", "win_the_game"],
        "actions.py": ["apply_land_etb_replacements", "_discard_land_to_enter"],
        "game.py": ["effective_draw_count", "_counter_doubling_factor", "player_damage_prevented",
                     "drop_protected_players", "void_exile_replacement_active", "exile_with_void_counter"],
        "creature_statics_wg.py": ["exile_instead_of_entering", "combat_damage_unpreventable"],
    },
    "State-based actions": {"turn.py": ["state_based_actions", "_state_based_actions_pass"]},
    "Zone changes (ETB/LTB, sacrifice, destroy, land play)": {
        "actions.py": ["PlayLand", "_play_land", "put_into_battlefield", "_apply_devour",
                        "apply_aura_static_grants_on_attach"],
        "turn.py": ["move_off_battlefield", "_reset_left_battlefield_object", "_end_departure",
                     "_release_exiled_until_leaves", "sacrifice", "destroy", "tap_permanent",
                     "note_cards_put_into_library", "note_life_losses"],
        "game.py": ["gain_life", "add_plus_one_counters", "add_minus_one_counters",
                     "add_charge_counters", "add_counters", "remove_counters", "add_stun_counters",
                     "counter_count", "named_counter_count", "set_named_counters", "_set_named_counter"],
    },
    "Combat (attack/block legality, damage, combat triggers)": {
        "combat.py": "*",
        "actions.py": ["DeclareAttackers", "DeclareBlockers", "_declare_attackers", "_declare_blockers"],
        "roto_board_statics.py": ["WithoutKeyword", "PowerGreaterThanControllersHandSize",
                                   "CantAttack", "CantBlock", "_forbidden", "filter_attackers",
                                   "filter_blockers"],
        "designations.py": ["goad", "clear_goad_for", "forced_attackers"],
        "game.py": ["crew_candidates", "crew_power"],
    },
    "Copy effects (clones, token copies, spell copies)": {
        "actions.py": ["apply_clone_enters_as_copy_replacement", "_offer_echoing_deeps_copy"],
        "creature_casting_ub.py": ["apply_copy_on_entry", "push_exile_copied_card"],
        "effects/generic/spell_copies.py": "*",
    },
    "Control-changing effects": {
        "game.py": ["effective_controller"], "effects/generic/control.py": "*",
    },
    "Face-down (manifest / morph-like), turn face up": {
        "actions.py": ["TurnFaceUp", "_turn_face_up"],
        "effects/generic/roto_layers.py": ["turning_face_up_prevented"],
    },
    "Multi-face layouts (transform/MDFC/adventure/split/class/saga/room/prepare/day-night)": {
        "layouts.py": "*", "game.py": ["restore_mdfc_front"],
        "actions.py": ["UnlockRoomHalf", "_unlock_room_half"],
        "effects/generic/layout_effects.py": "*",
    },
    "Designations: monarch / initiative / Undercity dungeon": {"designations.py": "*"},
    "Extra turns / extra combats / skip turn": {
        "simulate.py": ["_maybe_skip_turn", "_pick_next_active_player"],
        "effects/generic/stack_control.py": ["TakeExtraTurnAfterThisOne"],
    },
    "Outside-the-game (wish/sideboard), conspiracies, pregame": {
        "roto_turns.py": ["sideboard_printings", "take_from_outside_the_game"],
        "conspiracies.py": "*", "opening_hand.py": "*", "mulligan.py": "*",
        "simulate.py": ["setup_game"],
    },
    "Tokens (predefined token printings)": {"cards/tokens.py": "*"},
    "Emblems": {"effects/generic/emblem.py": "*"},
    "Restrictions, protection, ward, hexproof, taxes": {
        "keyword_rules.py": "*", "roto_restrictions.py": "*", "player_protections.py": "*",
        "creature_statics_wg.py": ["cast_forbidden", "filter_cast_restrictions",
                                    "forced_to_enter_tapped", "spell_cost_increase",
                                    "ability_cost_increase", "choose_card_name_as_enters",
                                    "blocking_forbidden", "extra_mana_for_creature_tap",
                                    "any_activation_locks", "activation_locked",
                                    "filter_unusable_abilities", "filter_activation_locks"],
        "actions.py": ["_apply_ward_check"],
        "roto_board_statics.py": ["untargetable_by", "CastOnlyBeforeStep", "filter_cast_timing",
                                   "NoCreaturesOnBattlefield", "spells_uncounterable_now",
                                   "player_hexproof_from_source", "DoesntUntap", "UntapLimit",
                                   "untap_step_held", "note_spell_cast", "spell_colors_cast_this_turn",
                                   "_drop_stale_grants", "_apply_role_uniqueness"],
    },
    "Hidden information + determinization": {
        "game.py": ["reveal_to", "reveal_to_all", "reset_public_knowledge",
                     "hide_hand_moved_into_library", "reset_library_public_knowledge", "shuffle_library"],
        "search/determinize.py": "*",
    },
}


def file_lines(rel):
    p = SRC / rel
    return sum(1 for _ in p.open(encoding="utf-8")) if p.exists() else 0


def main():
    rows = []
    claimed = {}
    for sub, spec in SUBSYSTEMS.items():
        total = 0
        parts = []
        for f, names in spec.items():
            if names == "*":
                n = file_lines(f)
                total += n
                parts.append(f"{f} (whole, {n})")
                continue
            items = {n: (l, s) for n, l, s, k in OUTLINE.get(f, [])}
            sub_total = 0
            missing = []
            for nm in names:
                if nm in items:
                    sub_total += items[nm][1]
                    claimed.setdefault((f, nm), sub)
                else:
                    missing.append(nm)
            total += sub_total
            biggest = sorted((items[n] + (n,) for n in names if n in items), key=lambda t: -t[1])[:3]
            parts.append(f"{f} ({sub_total}: " + ", ".join(f"{n}@{l}+{s}" for l, s, n in biggest) + ")"
                         + (f" MISSING {missing}" if missing else ""))
        rows.append((sub, total, parts))
    for sub, total, parts in rows:
        print(f"{total:6d}  {sub}")
        for p in parts:
            print("          ", p)
    print("TOTAL", sum(r[1] for r in rows))
    (HERE.parent / "scripts" / "_subsystems.json").write_text(
        json.dumps([{"subsystem": s, "lines": t, "parts": p} for s, t, p in rows], indent=1),
        encoding="utf-8")


if __name__ == "__main__":
    main()
