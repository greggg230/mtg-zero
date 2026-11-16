"""Enumerate legal actions for the player who currently has priority."""

from __future__ import annotations

from mtgz_ir.actions import (
    Action,
    ActivateAbility,
    ActivateStation,
    CastSpell,
    PassPriority,
    PlayLand,
    TurnFaceUp,
    UnlockRoomHalf,
    can_pay_cost,
)
from mtgz_ir.cards import ManaCost
from mtgz_ir.game import (
    GameObject, GameState, Zone, effective_colors, has_card_type,
    is_artifact_now,
)
from mtgz_ir.turn import MAIN_STEPS


def loyalty_activation_available(state, player_id: int, obj) -> bool:
    """CR 606.3's once-per-turn loyalty gate, widened by "you may
    activate <this>'s loyalty abilities N more times this turn" (Comet,
    Stellar Pup: ``GameObject.extra_loyalty_activations``)."""
    if (player_id, obj.id) not in state.loyalty_activated_this_turn:
        return True
    extra = obj.extra_loyalty_activations
    return bool(extra) and extra[0] == state.turn_number and extra[1] > 0


def _mayhem_castable(state: GameState, player_id: int, card) -> bool:
    from mtgz_ir.layouts import mayhem_castable

    return mayhem_castable(state, player_id, card)


def _has_priority(state: GameState, player_id: int) -> bool:
    return state.players[state.priority_player_idx].id == player_id


def _is_sorcery_timing(state: GameState, player_id: int) -> bool:
    """Sorcery timing: your turn, main phase, empty stack, you have priority."""
    return (
        state.players[state.active_player_idx].id == player_id
        and state.step in MAIN_STEPS
        and not state.stack
        and _has_priority(state, player_id)
    )


def _restricted_to_sorcery_speed_casts(
    state: GameState, player_id: int,
) -> bool:
    """True iff an opponent of ``player_id`` controls a permanent with
    "Each opponent can cast spells only any time they could cast a
    sorcery." (Teferi, Time Raveler)."""
    for o in state.battlefield:
        if (
            o.controller_id != player_id
            and o.printing.opponents_cast_spells_only_at_sorcery_speed
        ):
            return True
    return False


CAST_SORCERIES_AS_FLASH_KIND = "cast_sorceries_as_flash"


def _may_cast_sorceries_as_flash(state: GameState, player_id: int) -> bool:
    """True iff a live "you may cast sorcery spells as though they had
    flash" grant (Teferi, Time Raveler's +1; a
    ``cast_sorceries_as_flash`` continuous-effect registry entry whose
    ``params[0]`` is the granted player) covers ``player_id``."""
    for ce in state.continuous_effects:
        if (
            ce.kind == CAST_SORCERIES_AS_FLASH_KIND
            and ce.params
            and ce.params[0] == player_id
        ):
            return True
    return False


def _loyalty_abilities_at_instant_speed(
    state: GameState, obj: GameObject,
) -> bool:
    """The Wandering Emperor: "As long as ~ entered this turn, you may
    activate her loyalty abilities any time you could cast an
    instant." True iff ``obj`` carries that static and entered the
    battlefield this turn."""
    return (
        obj.printing.is_planeswalker
        and obj.printing.loyalty_abilities_at_instant_speed_if_entered_this_turn
        and obj.entered_battlefield_turn == state.turn_number
    )


def _spell_kinds_for_card(
    state: GameState, card: GameObject,
) -> "frozenset[str]":
    """Build the set of spend-restriction context tags for casting
    ``card``. Mirrors the logic in ``actions._cast_spell``: a card may
    satisfy multiple coin restrictions at once (e.g., a legendary
    artifact spell satisfies both "artifact_spell" and "legendary_spell"
    coin tags). Reads ``card.zone`` to detect non-hand origins
    (graveyard / exile / library top — Mm'menon's grant).
    """
    from mtgz_ir.game import Zone

    kinds: set[str] = {"spell"}
    if is_artifact_now(state, card):
        kinds.add("artifact_spell")
    if (
        "Eldrazi" in card.printing.subtypes
        and not effective_colors(state, card)
    ):
        kinds.add("colorless_eldrazi_spell")
    if card.printing.is_legendary:
        kinds.add("legendary_spell")
    if card.zone != Zone.HAND:
        kinds.add("cast_from_outside_hand")
    # Vintage Roto P3 tags (Cavern of Souls' creature-type mana).
    from mtgz_ir.effects.generic.roto_mana import roto_spell_kinds

    kinds |= roto_spell_kinds(card)
    return frozenset(kinds)


def _citadel_top_card(state: GameState, player_id: int):
    """The top card of ``player_id``'s library when they control a
    permanent granting Bolas's Citadel's play-from-top, else None."""
    p = state.player_by_id(player_id)
    if not p.library:
        return None
    if not any(
        o.controller_id == player_id
        and o.printing.grants_play_from_top_paying_life
        for o in state.battlefield
    ):
        return None
    return p.library[0]


def _can_cast_paying_life(
    state: GameState, player_id: int, card: GameObject,
) -> bool:
    """Bolas's Citadel cast legality: life to pay the mana value (CR
    119.4 — a player can pay life only up to their life total), plus the
    usual target / additional-cost gates."""
    p = state.player_by_id(player_id)
    return (
        p.life >= card.printing.mana_cost.total
        and _targets_are_satisfiable(state, player_id, card)
        and _additional_costs_payable(state, player_id, card)
    )


def unblocked_attackers(state: GameState, player_id: int) -> list:
    """Attacking creatures ``player_id`` controls that no creature
    blocked, once blockers have been declared (the declare blockers
    step) -- Ninjutsu's cost (CR 702.49a)."""
    from mtgz_ir.turn import Step

    if state.step != Step.DECLARE_BLOCKERS:
        return []
    return [
        o for o in state.battlefield
        if o.attacking and o.controller_id == player_id and not o.blocked_by
    ]


def _can_cast(
    state: GameState, player_id: int, card: GameObject, cost: ManaCost,
    *, spell_cost: bool = True,
) -> bool:
    """Check whether ``player_id`` can pay ``cost`` to cast ``card``.

    Bundles spend-restriction context (spell_kinds from the card type and
    zone), convoke eligibility, and sac-for-mana eligibility so call sites
    don't have to reconstruct those three arguments on every check.
    ``sac_for_mana`` is always ``True`` — every spell-cast site permits
    Treasure-style sacrifice during cost payment. ``spell_cost`` (default)
    marks ``cost`` as a spell's total cost, so a Trinisphere floor
    (``roto_restrictions``) applies; the plot special action passes False.

    A card printed with no mana cost (Lotus Bloom) can't be cast by
    paying a cost (CR 202.1b / 601.2f); its free casts don't come here.
    """
    if card.printing.has_no_mana_cost:
        return False
    if spell_cost:
        from mtgz_ir.roto_restrictions import apply_minimum_spell_cost

        cost = apply_minimum_spell_cost(state, cost)
    return can_pay_cost(
        state, player_id, cost,
        spell_kind=_spell_kinds_for_card(state, card),
        convoke=card.printing.has_convoke,
        improvise=card.printing.has_improvise,
        sac_for_mana=True,
    )


def _targets_are_satisfiable(
    state: GameState, player_id: int, card: GameObject,
    *, kicked: bool = False,
) -> bool:
    """For spells with targeted spell_effects, check that at least
    ``min_t`` legal targets exist (CR 601.2c — a spell with N
    required targets needs N legal targets to be cast at all). When
    ``target_slot_kinds`` is set, each kind's slot count must be
    backed by enough legal entries of that kind, since duplicates
    can't be picked across slots (CR 115.1b "different targets" rule).

    Non-targeting spells and creatures without spell_effect trivially
    satisfy this.
    """
    from mtgz_ir.actions import cast_time_effect

    effect = card.printing.spell_effect
    if effect is not None:
        # Kicker-dependent target shapes (Tear Asunder).
        effect = cast_time_effect(effect, kicked=kicked)
    if effect is None or not effect.requires_target():
        return True
    from mtgz_ir.game import drop_protected_players
    from mtgz_ir.keyword_rules import filter_targetable

    legal = drop_protected_players(state, filter_targetable(
        state, effect.legal_targets(state, player_id), card, player_id,
    ))
    if not legal:
        return False
    min_t, _max_t = effect.target_count_range()
    if min_t <= 1:
        return True
    slot_kinds = effect.target_slot_kinds()
    if slot_kinds:
        # Per-slot kind enforcement: each unique kind needs as many
        # legal entries as it has slots.
        from collections import Counter
        slot_counts = Counter(slot_kinds[:min_t])
        legal_counts = Counter(t[0] for t in legal)
        for kind, needed in slot_counts.items():
            if legal_counts.get(kind, 0) < needed:
                return False
        return True
    # No per-slot kind enforcement: just ensure at least min_t
    # distinct legal targets exist (different-targets rule).
    return len(legal) >= min_t


def _additional_costs_payable(
    state: GameState, player_id: int, card: GameObject
) -> bool:
    """Check that any cast-time additional costs on ``card`` can be paid.
    Currently: additional_cost_sacrifice_creature_or_artifact requires a
    creature or artifact the caster controls; the casting-substrate
    costs (``additional_cost_sacrifice_filter`` etc.) are checked by
    ``alt_casting.additional_costs_payable``.
    """
    from mtgz_ir.alt_casting import additional_costs_payable as _alt_payable

    if not _alt_payable(state, player_id, card):
        return False
    if card.printing.additional_cost_options:
        # One-of additional costs ("discard a card or pay 3 life"): at
        # least one arm must be payable.
        from mtgz_ir.cards.spell_costs import additional_costs_payable

        if not additional_costs_payable(state, player_id, card):
            return False
    if card.printing.additional_cost_sacrifice_creature_or_artifact:
        have_sac = any(
            o for o in state.battlefield
            if o.controller_id == player_id
            and (o.printing.is_creature or is_artifact_now(state, o))
        )
        if not have_sac:
            return False
    if card.printing.additional_cost_sacrifice_creature_or_enchantment:
        # Final Vengeance: "As an additional cost to cast this spell,
        # sacrifice a creature or enchantment." Eligible permanents are
        # any creature or enchantment the caster controls. Auras count
        # (an Aura is an Enchantment subtype); the same controller-
        # filter as the artifact-or-creature flag applies.
        have_sac = any(
            o for o in state.battlefield
            if o.controller_id == player_id
            and (
                o.printing.is_creature
                or "Enchantment" in o.printing.card_types
            )
        )
        if not have_sac:
            return False
    if card.printing.additional_cost_choose_your_creature:
        # "Choose a creature you control or a warped creature card you
        # own in exile." Either source satisfies the cost.
        have_battlefield = any(
            o for o in state.battlefield
            if o.controller_id == player_id and o.printing.is_creature
        )
        if not have_battlefield:
            p = state.player_by_id(player_id)
            have_exiled = any(
                o for o in p.exile
                if o.owner_id == player_id
                and o.warped
                and o.printing.is_creature
            )
            if not have_exiled:
                return False
    return True


# ---------------------------------------------------------------------------
# Cube creatures (R / multicolor / colorless bucket) legality helpers
# ---------------------------------------------------------------------------


def _extra_costs_payable(state: GameState, player_id: int, source, cost) -> bool:
    """Every ``ActivationCost.extra_costs`` atom can be paid."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import extra_costs_payable

    return extra_costs_payable(state, player_id, source, cost)


def _graveyard_ability_activations(
    state: GameState, player_id: int,
) -> list[Action]:
    """``ActivateAbility`` for each payable graveyard-activated ability
    of a card in ``player_id``'s graveyard (instant speed unless the
    ability is ``sorcery_speed``)."""
    out: list[Action] = []
    p = state.player_by_id(player_id)
    seen: set[tuple[str, int]] = set()
    for card in p.graveyard:
        for idx, ability in enumerate(card.printing.graveyard_activated_abilities):
            key = (card.printing.name, idx)
            if key in seen:
                continue
            if ability.sorcery_speed and not _is_sorcery_timing(state, player_id):
                continue
            if ability.cost.mana is not None:
                from mtgz_ir.actions import _effective_ability_mana_cost

                if not can_pay_cost(
                    state, player_id,
                    _effective_ability_mana_cost(state, card, ability),
                ):
                    continue
            if ability.cost.extra_costs and not _extra_costs_payable(
                state, player_id, card, ability.cost,
            ):
                continue
            if ability.effect.requires_target():
                state.resolving_source = card
                try:
                    has_legal = bool(ability.effect.legal_targets(state, player_id))
                finally:
                    state.resolving_source = None
                if not has_legal:
                    continue
            seen.add(key)
            out.append(ActivateAbility(source_id=card.id, ability_index=idx))
    return out


def _exile_permission_plays(
    state: GameState, player_id: int, *, sorcery_window: bool,
) -> list[Action]:
    """Casts / land plays granted by ``GameObject.exile_play_permission``.

    ``sorcery_window=False`` emits only instant-speed casts (instants and
    flash cards); ``True`` emits the sorcery-timing ones plus land plays.
    Free permissions are offered as ``cast_for_free`` casts.
    """
    from mtgz_ir.actions import apply_cost_reductions
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        exile_cards_playable_by,
        exile_permission_cast_cost,
    )

    out: list[Action] = []
    p = state.player_by_id(player_id)
    for card in exile_cards_playable_by(state, player_id):
        perm = card.exile_play_permission
        pr = card.printing
        if pr.is_land:
            if not sorcery_window or perm.cast_only:
                continue
            max_lands = 1 + sum(
                o.printing.grants_additional_land_per_turn
                for o in state.battlefield
                if o.controller_id == player_id
            )
            if p.lands_played_this_turn < max_lands:
                out.append(PlayLand(card.id))
            continue
        fast = pr.is_instant or "flash" in pr.keywords
        if fast == sorcery_window:
            continue
        cost = apply_cost_reductions(
            pr, state, player_id, exile_permission_cast_cost(card, perm),
            card=card,
        )
        if not (
            (perm.free or _can_cast(state, player_id, card, cost))
            and _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
        ):
            continue
        out.append(CastSpell(card.id, cast_for_free=perm.free))
    return out


def activation_timing_permits(
    state: GameState, obj: GameObject, ability, *, sorcery_timing: bool,
) -> bool:
    """Whether the timing rules let ``obj``'s controller activate
    ``ability`` now, given they hold priority (CR 117.1b: an activated
    ability may be activated any time its controller has priority unless
    the ability or a rule restricts it -- CR 602.5).

    In a sorcery window (``sorcery_timing``) every ability qualifies.
    Outside one:

    * a mana ability doesn't -- the engine-wide handling of explicit mana
      abilities (offered as actions in a sorcery window only; automatic
      payment otherwise), a documented simplification of CR 605.3a
      (cube-lands shortcut #1 in ``docs/SEARCH_LIMITATIONS.md``);
    * a planeswalker's abilities (loyalty abilities, CR 606.3 -- every
      walker ability is one by construction, flagged or not) don't,
      unless the walker's own static lets them be activated any time
      their controller could cast an instant (The Wandering Emperor the
      turn she entered);
    * a ``sorcery_speed`` ability ("Activate only as a sorcery.", equip,
      reconfigure, saddle, level up) doesn't.

    The other restrictions ("Activate only during your turn / if / once
    [each turn]", exhaust, boast, the loyalty once-per-turn gate) are
    window-independent and checked by ``battlefield_ability_activations``
    in every window."""
    if sorcery_timing:
        return True
    if ability.is_mana_ability:
        return False
    if obj.printing.is_planeswalker:
        return _loyalty_abilities_at_instant_speed(state, obj)
    return not ability.sorcery_speed


def battlefield_ability_activations(
    state: GameState, player_id: int, *, sorcery_timing: bool,
    first_only: bool = False,
) -> list[Action]:
    """``ActivateAbility`` for every activated ability of a permanent
    ``player_id`` controls that they may activate now (they hold
    priority; ``sorcery_timing`` says whether this is a sorcery window).

    The one enumerator for battlefield activations: both exits of
    ``_legal_actions_core`` call it, and so does the search capability
    check (``first_only=True`` stops at the first activation found).
    Timing is ``activation_timing_permits``; costs, targets and the
    printed riders are checked below in every window."""
    from mtgz_ir.game import effective_subtypes

    p = state.player_by_id(player_id)
    actions: list[Action] = []
    for obj in state.battlefield:
        if obj.controller_id != player_id:
            continue
        from mtgz_ir.game import effective_activated_abilities
        # CR 606.3 — once-per-turn-per-walker loyalty gate. If this
        # planeswalker has already had a loyalty ability activated this
        # turn, ALL of its activated abilities are inert for the rest of
        # the turn. Gate fires BEFORE the per-ability loop so none of the
        # walker's abilities emit. Per-walker (not per-ability) keyed on
        # (controller_id, source_id) — mirrors Rust's
        # state_ops_legal.rs walker-skip. Today every walker's activated
        # abilities are loyalty abilities by construction, so gating on
        # the Planeswalker card type maps 1:1 to "has a loyalty ability".
        if (
            obj.printing.is_planeswalker
            and not loyalty_activation_available(state, player_id, obj)
        ):
            continue
        for idx, ability in enumerate(
            effective_activated_abilities(state, obj)
        ):
            # CR 117.1b / 602.5 timing (the only window-dependent
            # gate; every other check below runs in every window).
            if not activation_timing_permits(
                state, obj, ability, sorcery_timing=sorcery_timing,
            ):
                continue
            # CR 606 — positive loyalty cost is a pay-loyalty cost: the
            # walker must have at least ``cost.loyalty`` loyalty counters
            # to afford the activation. Negative (add-loyalty) and zero
            # costs are always affordable. Mirrors Rust's emission gate
            # at state_ops_legal.rs. Fires early (before the other
            # cost-payability gates) — a walker that can't afford its
            # -N ability is outright unable regardless of other state.
            if (
                ability.cost.loyalty > 0
                and obj.loyalty_counters < ability.cost.loyalty
            ):
                continue
            # ``Activate only once.`` rider — DSK Haunted Screen's
            # third activation. Once the source's
            # ``has_activated_once_only_ability`` flag is latched at
            # activation time, every later activation of an ability
            # with this rider is filtered out — including a second
            # attempt within the same priority window before the
            # first activation has resolved.
            if ability.activate_only_once and obj.has_activated_once_only_ability:
                continue
            # Exhaust (CR 702.177): each exhaust ability once per object.
            if ability.exhaust and idx in obj.exhausted_abilities:
                continue
            # Boast (CR 702.142): only if it attacked this turn, once
            # each turn.
            if ability.boast and (
                obj.boasted_turn == state.turn_number
                or not any(
                    a.id == obj.id for a in p.attacked_this_turn
                )
            ):
                continue
            if ability.cost.extra_costs and not _extra_costs_payable(
                state, player_id, obj, ability.cost,
            ):
                continue
            # "Activate only if <condition>." (Library of Alexandria).
            if (
                ability.activate_only_if is not None
                and not ability.activate_only_if.matches(state, player_id)
            ):
                continue
            # ``Activate only during your turn.`` (Wishclaw Talisman).
            if (
                ability.activate_only_during_your_turn
                and state.active_player().id != player_id
            ):
                continue
            # Cost payability checks. Apply ability-level mana
            # reductions (Dominion Bracelet: {X} less where X is
            # equipped creature's power) before checking.
            from mtgz_ir.creature_statics_wg import ability_cost_increase

            if ability.cost.mana is not None or ability_cost_increase(
                state, obj, ability,
            ):
                from mtgz_ir.actions import _effective_ability_mana_cost
                effective_mana = _effective_ability_mana_cost(state, obj, ability)
                if ability.cost.waterbend:
                    from mtgz_ir.layouts import waterbend_helpers

                    effective_mana = effective_mana.reduced(
                        len(waterbend_helpers(state, player_id)),
                    )
                # Activation spend-restriction context: combine all the
                # tags this source qualifies for so coins with any of
                # those restrictions are usable. Mirrors
                # actions._activate_ability.
                ability_kinds: set[str] = {"spell", "activated_ability"}
                if (
                    "Eldrazi" in obj.printing.subtypes
                    and not effective_colors(state, obj)
                ):
                    ability_kinds.add("colorless_eldrazi_ability")
                if is_artifact_now(state, obj):
                    ability_kinds.add("artifact_activation")
                if obj.printing.is_legendary:
                    ability_kinds.add("legendary_permanent_ability")
                if obj.printing.is_land:
                    ability_kinds.add("land_activation")
                if not can_pay_cost(
                    state, player_id, effective_mana,
                    spell_kind=frozenset(ability_kinds),
                ):
                    continue
            if ability.cost.tap_self:
                if obj.tapped:
                    continue
                # Creatures tapping as part of a cost can't while summoning-sick.
                from mtgz_ir.game import is_creature_now
                if is_creature_now(obj) and obj.summoning_sick:
                    continue
            if ability.cost.sacrifice_another_creature_or_artifact:
                has_another = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.id != obj.id
                    and (o2.printing.is_creature or is_artifact_now(state, o2))
                )
                if not has_another:
                    continue
            if ability.cost.sacrifice_another_artifact_or_land:
                has_another = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.id != obj.id
                    and (is_artifact_now(state, o2) or o2.printing.is_land)
                )
                if not has_another:
                    continue
            if ability.cost.sacrifice_another_artifact:
                has_another = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.id != obj.id
                    and is_artifact_now(state, o2)
                )
                if not has_another:
                    continue
            if ability.cost.remove_plus_one_counter_from_your_creature:
                has_counter_creature = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.printing.is_creature
                    and o2.plus_one_counters > 0
                )
                if not has_counter_creature:
                    continue
            if ability.cost.sacrifice_food:
                has_food = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and "Food" in effective_subtypes(state, o2)
                )
                if not has_food:
                    continue
            if ability.cost.pay_energy and p.energy < ability.cost.pay_energy:
                continue
            if ability.cost.pay_life:
                if p.life <= ability.cost.pay_life:
                    # MTG lets you pay to 0 life (it kills you) but
                    # pragmatically our bot avoids that line — gate
                    # strictly to require surviving the payment.
                    continue
            if ability.cost.sacrifice_a_land:
                has_land = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.printing.is_land
                    and o2.id != obj.id
                )
                if not has_land:
                    continue
            if ability.cost.sacrifice_a_desert:
                has_desert = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and "Desert" in effective_subtypes(state, o2)
                )
                if not has_desert:
                    continue
            if ability.cost.sacrifice_n_nonland_permanents:
                n_nonland = sum(
                    1 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and not o2.printing.is_land
                )
                if n_nonland < ability.cost.sacrifice_n_nonland_permanents:
                    continue
            if ability.cost.saddle:
                from mtgz_ir.game import crew_candidates, effective_power

                if sum(
                    max(0, effective_power(state, o2))
                    for o2 in crew_candidates(state, player_id, obj)
                ) < ability.cost.saddle:
                    continue
            if ability.cost.crew:
                from mtgz_ir.game import crew_candidates, crew_power
                total = sum(
                    crew_power(state, o2)
                    for o2 in crew_candidates(state, player_id, obj)
                )
                if total < ability.cost.crew:
                    continue
            if ability.cost.sacrifice_subtype:
                wanted_sub = ability.cost.sacrifice_subtype
                if not any(
                    o2.controller_id == player_id
                    and wanted_sub in effective_subtypes(state, o2)
                    for o2 in state.battlefield
                ):
                    continue
            if ability.cost.sacrifice_nontoken_artifact:
                if not any(
                    o2.controller_id == player_id
                    and is_artifact_now(state, o2)
                    and not o2.printing.is_token
                    for o2 in state.battlefield
                ):
                    continue
            if ability.cost.sacrifice_a_permanent_type:
                from mtgz_ir.effects.generic.enchantment_mechanics import (
                    sacrifice_type_candidates,
                )
                if not sacrifice_type_candidates(
                    state, player_id, ability.cost.sacrifice_a_permanent_type,
                ):
                    continue
            if ability.cost.remove_counters_from_self_kind:
                from mtgz_ir.game import counter_count
                if (
                    counter_count(obj, ability.cost.remove_counters_from_self_kind)
                    < ability.cost.remove_counters_from_self_amount
                ):
                    continue
            if ability.cost.requires_own_creature_power_at_least:
                from mtgz_ir.game import effective_power
                threshold = ability.cost.requires_own_creature_power_at_least
                has_big = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.printing.is_creature
                    and effective_power(state, o2) >= threshold
                )
                if not has_big:
                    continue
            if ability.cost.requires_own_artifact_count_at_least:
                # Inventors' Fair: "Activate only if you control three
                # or more artifacts." Count artifacts the activator
                # controls; skip the activation if below the threshold.
                threshold = ability.cost.requires_own_artifact_count_at_least
                artifact_count = sum(
                    1 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and is_artifact_now(state, o2)
                )
                if artifact_count < threshold:
                    continue
            if ability.cost.requires_control_of_subtype:
                # Duskmourn Verge cycle: "Activate only if you control
                # a <T1> or a <T2>." The activator must control at
                # least one permanent whose effective subtypes include
                # any of the listed names.
                wanted = ability.cost.requires_control_of_subtype
                has_subtype = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and any(
                        st in effective_subtypes(state, o2) for st in wanted
                    )
                )
                if not has_subtype:
                    continue
            if ability.cost.exile_granting_equipment:
                # The granting equipment must still be attached to
                # the source. Legal when there's any equipment with
                # a matching grant-flag attached to obj.
                has_granter = any(
                    eq for eq in state.battlefield
                    if eq.attached_to == obj.id
                    and eq.printing.grants_control_opponent_ability_when_equipped
                )
                if not has_granter:
                    continue
            # exile_self — source must be on the battlefield to exile
            # it. The battlefield presence check at the top of this
            # loop already guarantees that, so there is no additional
            # gate beyond what sacrifice_self requires. The field is
            # checked here for explicitness and future-proofing (e.g.
            # if a card ever has an exile_self restriction beyond the
            # normal "must be on battlefield" one).
            # (no extra gate beyond battlefield presence for exile_self)
            if ability.cost.tap_permanent_you_control:
                # "Tap an untapped permanent you control" — Gene
                # Pollinator. Requires at least one OTHER untapped
                # permanent under the activator's control. The source
                # itself cannot be used (same ruling as crewing: the
                # source is the thing benefiting from the cost).
                has_untapped_other = any(
                    o2 for o2 in state.battlefield
                    if o2.controller_id == player_id
                    and o2.id != obj.id
                    and not o2.tapped
                    and (
                        not ability.cost.tap_permanent_type
                        or has_card_type(
                            state, o2, ability.cost.tap_permanent_type,
                        )
                    )
                )
                if not has_untapped_other:
                    continue
            if ability.cost.remove_named_counter_from_self:
                from mtgz_ir.game import named_counter_count
                if named_counter_count(
                    obj, ability.cost.remove_named_counter_from_self,
                ) < 1:
                    continue
            if ability.cost.discard_a_card and not p.hand:
                continue
            if (
                ability.cost.exile_cards_from_graveyard
                and len(p.graveyard) < ability.cost.exile_cards_from_graveyard
            ):
                continue
            if ability.cost.exile_artifact_from_graveyard:
                # "Exile an artifact card from your graveyard" — Rust
                # Harvester. Requires at least one artifact card in the
                # activating player's graveyard.
                has_artifact_in_gy = any(
                    card for card in p.graveyard
                    if is_artifact_now(state, card)
                )
                if not has_artifact_in_gy:
                    continue
            # If the effect requires a target, at least one must be legal.
            # Set resolving_source so effects with self-exclusion filters
            # (e.g. "another target creature") compute the correct legal
            # pool during legality-check. Cleared immediately afterward.
            if ability.effect.requires_target():
                state.resolving_source = obj
                try:
                    from mtgz_ir.game import drop_protected_players
                    from mtgz_ir.keyword_rules import filter_targetable

                    has_legal = bool(drop_protected_players(state, filter_targetable(
                        state,
                        ability.effect.legal_targets(state, player_id),
                        obj, player_id,
                    )
                    ))
                finally:
                    state.resolving_source = None
                if not has_legal:
                    continue
            actions.append(
                ActivateAbility(source_id=obj.id, ability_index=idx)
            )
            if first_only:
                return actions
    return actions


def legal_actions(state: GameState, player_id: int) -> list[Action]:
    """Return all legal actions for ``player_id`` right now (see
    ``_legal_actions_core``), plus casts of a card as one of its other
    faces -- a split card's halves, a modal DFC's castable back face
    (``mtgz_ir.layouts.castable_faces``)."""
    actions = _legal_actions_core(state, player_id)
    if len(actions) == 1 and not _has_priority(state, player_id):
        return actions
    from mtgz_ir.layouts import face_cast_actions

    return face_cast_actions(state, player_id, actions, _legal_actions_core)


def _legal_actions_core(state: GameState, player_id: int) -> list[Action]:
    """Return all legal actions for ``player_id`` right now.

    At any priority moment (on or off your turn), instants in hand whose cost
    is payable and whose targets are satisfiable are legal. At sorcery timing
    (your main phase, stack empty, your priority) creatures, sorceries, land
    plays, and warp-exile recasts are also legal. A card in hand with a warp
    cost can be cast either at its normal cost or for its warp alt-cost
    whenever a normal cast would be legal — proposed separately.
    """
    actions: list[Action] = [PassPriority()]

    if not _has_priority(state, player_id):
        return actions
    # Roto P4: the turn is being ended (CR 723.1d) -- nobody acts.
    if state.turn_ending:
        return actions

    p = state.player_by_id(player_id)

    from mtgz_ir.actions import apply_cost_reductions

    from mtgz_ir.alt_casting import (
        cost_plus as _cost_plus,
        delve_affordability_cost as _delve_affordability_cost,
        flashback_cost as _flashback_cost,
    )

    def _effective_cost(card, base_cost):
        # Delve (CR 702.66) only ever lowers the generic cost, so the
        # affordability probe assumes the whole graveyard is available.
        from mtgz_ir.creature_casting_ub import casting_origin

        with casting_origin(
            card.zone != Zone.HAND, from_graveyard=card.zone == Zone.GRAVEYARD,
            from_exile=card.zone == Zone.EXILE,
        ):
            reduced = apply_cost_reductions(
                card.printing, state, player_id, base_cost, card=card,
            )
        cost = _delve_affordability_cost(state, player_id, card, reduced)
        # Spree: every cast chooses at least one mode, each with its own
        # extra mana cost — the cast is affordable if base + SOME single
        # mode's cost is payable.
        mode_costs = getattr(card.printing.spell_effect, "single_mode_mana_costs", None)
        if mode_costs is not None:
            options = [_cost_plus(cost, extra) for extra in mode_costs()]
            payable = [c for c in options if _can_cast(state, player_id, card, c)]
            return (payable or options)[0]
        return cost

    def _cost_plus_kicker(card, base_cost):
        """Combined base + kicker cost (colored pips merged; a kicker
        {X} counts at its least legal X)."""
        kc = card.printing.kicker_cost
        if kc is None:
            return base_cost
        merged: dict[str, int] = dict(base_cost.colored)
        for color, n in kc.colored:
            merged[color] = merged.get(color, 0) + n
        return ManaCost(
            generic=base_cost.generic + kc.generic
            + kc.x_pips * card.printing.x_min,
            colored=tuple(sorted(merged.items())),
            x_pips=base_cost.x_pips,
        )

    # Weftwalking-style "first spell each player casts during each of
    # their turns may be cast without paying its mana cost." Only
    # offered to the active player when they haven't yet cast a spell
    # this turn AND any battlefield permanent grants it.
    free_first_cast_active = (
        state.players[state.active_player_idx].id == player_id
        and p.spells_cast_this_turn == 0
        and any(
            o.printing.grants_first_spell_each_turn_free
            for o in state.battlefield
        )
    )

    from mtgz_ir.alt_casting import pitch_options as _pitch_options

    def _free_cast_tax_payable(card) -> bool:
        # Cost increases (Thalia) still apply to free casts (CR 601.2f).
        from mtgz_ir.creature_statics_wg import spell_cost_increase

        from mtgz_ir.creature_casting_ub import casting_origin

        with casting_origin(
            card.zone != Zone.HAND, from_graveyard=card.zone == Zone.GRAVEYARD,
            from_exile=card.zone == Zone.EXILE,
        ):
            tax = spell_cost_increase(
                state, card.printing, player_id, card=card,
            )
        from mtgz_ir.roto_restrictions import minimum_spell_cost

        return (tax == 0 and not minimum_spell_cost(state)) or _can_cast(
            state, player_id, card, ManaCost(generic=tax),
        )

    def _own_free_cast(card) -> bool:
        cond = card.printing.free_cast_condition
        if cond is not None and cond.matches(state, player_id):
            return True
        # Printed alternative costs (Force of Will, Daze, Snuff Out,
        # Baleful Mastery, Overload, ...) and granted ones (Dream Halls)
        # share this action variant.
        from mtgz_ir.cards.spell_costs import alternative_costs_for

        if alternative_costs_for(state, player_id, card):
            from mtgz_ir.cards.spell_costs import payable_alternative_costs

            if payable_alternative_costs(state, player_id, card):
                return True
        # Pitch alternative cost (Pyrokinesis): exile another card.
        return bool(_pitch_options(state, player_id, card))

    # Instant-speed-castable: instants AND any permanent spell with flash.
    # Hand-zone PlayLand / CastSpell dedup: two same-printing copies in hand
    # reach identical end-states modulo card.id (which is a label with no
    # future bearing on play) — no per-object state on hand-resident
    # GameObjects diverges between copies; counters / tapped / attached_to /
    # warped only attach in non-hand zones. Equivalence key is
    # (printing.name, *modifier_tuple), mirroring the precedent at
    # _player_capabilities.py:557. MAINTENANCE INVARIANT: if a future
    # mechanic ever attaches per-object state to hand-resident cards
    # (counters on cards in hand, "next time you cast THIS card" memory,
    # etc.), this key MUST absorb that state — otherwise legal actions are
    # silently dropped. Same comment applies to the PlayLand and sorcery-
    # speed CastSpell dedup blocks below.
    seen_instant: set[tuple] = set()

    def _emit_hand_cast(card, **kwargs):
        key = (
            card.printing.name,
            kwargs.get("use_warp", False),
            kwargs.get("kicked", False),
            kwargs.get("cast_for_free", False),
            kwargs.get("use_impending", False),
            kwargs.get("cast_room_half", None),
            kwargs.get("use_buyback", False),
            kwargs.get("use_dash", False),
        )
        if key in seen_instant:
            return
        seen_instant.add(key)
        actions.append(CastSpell(card.id, **kwargs))

    sorcery_timing = _is_sorcery_timing(state, player_id)
    # Teferi, Time Raveler's +1: sorcery spells as though they had flash.
    sorceries_have_flash = _may_cast_sorceries_as_flash(state, player_id)
    for card in p.hand:
        instant_speed = (
            card.printing.is_instant or "flash" in card.printing.keywords
            or (sorceries_have_flash and card.printing.is_sorcery)
        )
        if not instant_speed:
            continue
        if _can_cast(
            state, player_id, card, _effective_cost(card, card.printing.mana_cost),
        ) and _targets_are_satisfiable(state, player_id, card) and _additional_costs_payable(state, player_id, card):
            _emit_hand_cast(card)
            # Kicked variant: can_pay the combined cost.
            if card.printing.kicker_cost is not None and _can_cast(
                state, player_id, card,
                _cost_plus_kicker(card, _effective_cost(card, card.printing.mana_cost)),
            ) and _targets_are_satisfiable(state, player_id, card, kicked=True):
                _emit_hand_cast(card, kicked=True)
            # Buyback variant (CR 702.27): base + buyback cost.
            if card.printing.buyback_cost is not None and _can_cast(
                state, player_id, card,
                _cost_plus(
                    _effective_cost(card, card.printing.mana_cost),
                    card.printing.buyback_cost,
                ),
            ):
                _emit_hand_cast(card, use_buyback=True)
        elif (
            # Only the kicked cast has a legal target (Tear Asunder's
            # kicked "exile target nonland permanent").
            card.printing.kicker_cost is not None
            and _can_cast(
                state, player_id, card,
                _cost_plus_kicker(card, _effective_cost(card, card.printing.mana_cost)),
            )
            and _targets_are_satisfiable(state, player_id, card, kicked=True)
            and _additional_costs_payable(state, player_id, card)
        ):
            _emit_hand_cast(card, kicked=True)
        if (
            card.printing.warp_cost is not None
            and _can_cast(
                state, player_id, card, _effective_cost(card, card.printing.warp_cost),
            )
            and _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
            and p.life > card.printing.warp_additional_life_cost
        ):
            _emit_hand_cast(card, use_warp=True)
        # Battlefield-granted warp: a permanent the controller controls
        # may grant warp to cards in hand matching a filter (Tannuk,
        # Steadfast Second). The grant only applies when the card has
        # no innate warp_cost — innate warp wins at the legal-actions
        # layer so the player isn't offered both options for the same
        # spell. ``effective_warp_cost`` looks up the grant; if it
        # returns a cost (and there's no innate warp already proposed
        # above), offer warp-cast.
        if card.printing.warp_cost is None:
            from mtgz_ir.actions import effective_warp_cost
            warp_lookup = effective_warp_cost(state, player_id, card)
            if (
                warp_lookup is not None
                and _can_cast(
                    state, player_id, card,
                    _effective_cost(card, warp_lookup[0]),
                )
                and _targets_are_satisfiable(state, player_id, card)
                and _additional_costs_payable(state, player_id, card)
            ):
                _emit_hand_cast(card, use_warp=True)
        # Weftwalking free-cast variant. Skips the mana-cost payable
        # check (it's free) but still gates on targets / additional
        # non-mana costs. Also covers the spell's own conditional free
        # cast (Once Upon a Time's ``free_cast_condition``).
        if (
            (free_first_cast_active or _own_free_cast(card))
            and _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
            and _free_cast_tax_payable(card)
        ):
            _emit_hand_cast(card, cast_for_free=True)

    # Hand-activated abilities (Cycling, Channel): the source is a card
    # in the activator's hand; ``ability_index`` indexes
    # ``printing.hand_activated_abilities``. Offered at instant speed
    # (CR 702.29a — cycling is an activated ability with no timing
    # restriction) unless the ability is ``sorcery_speed``, so this
    # block sits BEFORE the sorcery-window early return. Dedup same-name
    # hand copies exactly like the hand CastSpell / PlayLand blocks —
    # hand-resident GameObjects carry no per-object state.
    seen_hand_abilities: set[tuple[str, int]] = set()
    for card in p.hand:
        for idx, ability in enumerate(card.printing.hand_activated_abilities):
            key = (card.printing.name, idx)
            if key in seen_hand_abilities:
                continue
            if ability.cost.return_unblocked_attacker and not (
                unblocked_attackers(state, player_id)
            ):
                continue
            if ability.sorcery_speed and not _is_sorcery_timing(state, player_id):
                continue
            if (
                ability.activate_only_if is not None
                and not ability.activate_only_if.matches(state, player_id)
            ):
                continue
            from mtgz_ir.creature_statics_wg import ability_cost_increase

            if ability.cost.mana is not None or ability_cost_increase(
                state, card, ability,
            ):
                from mtgz_ir.actions import _effective_ability_mana_cost
                if not can_pay_cost(
                    state, player_id,
                    _effective_ability_mana_cost(state, card, ability),
                ):
                    continue
            if ability.cost.extra_costs and not _extra_costs_payable(
                state, player_id, card, ability.cost,
            ):
                continue
            if ability.effect.requires_target():
                state.resolving_source = card
                try:
                    has_legal = bool(
                        ability.effect.legal_targets(state, player_id)
                    )
                finally:
                    state.resolving_source = None
                if not has_legal:
                    continue
            seen_hand_abilities.add(key)
            actions.append(
                ActivateAbility(source_id=card.id, ability_index=idx)
            )

    # Graveyard-activated abilities (Seasoned Pyromancer's "{3}{R}{R},
    # Exile this card from your graveyard: ...", Otharri's "Return this
    # card from your graveyard to the battlefield"): the source is a
    # card in the activator's graveyard; ``ability_index`` indexes
    # ``printing.graveyard_activated_abilities``. Instant speed like the
    # hand abilities above (no timing restriction is printed).
    actions.extend(_graveyard_ability_activations(state, player_id))

    # Flashback (CR 702.34) from the graveyard. Instants are castable
    # at any priority; everything else waits for sorcery timing (the
    # second flashback block below).
    # Roto P2: flashback costs with a sacrifice (Cabal Therapy).
    from mtgz_ir.effects.generic.roto_zones import (
        flashback_extra_costs_payable as _flashback_extra_costs_payable,
    )

    def _emit_flashback_casts(instant_speed: bool) -> None:
        for gcard in p.graveyard:
            fb = _flashback_cost(gcard)
            if fb is None:
                continue
            is_instant_speed = (
                gcard.printing.is_instant
                or "flash" in gcard.printing.keywords
            )
            if is_instant_speed != instant_speed:
                continue
            if (
                _can_cast(state, player_id, gcard, _effective_cost(gcard, fb))
                and _targets_are_satisfiable(state, player_id, gcard)
                and _additional_costs_payable(state, player_id, gcard)
                and _flashback_extra_costs_payable(state, player_id, gcard)
            ):
                actions.append(CastSpell(gcard.id, use_flashback=True))

    _emit_flashback_casts(instant_speed=True)

    from mtgz_ir.alt_casting import escape_payable as _escape_payable
    from mtgz_ir.alt_casting import escape_terms as _escape_terms

    def _emit_escape_casts(instant_speed: bool) -> None:
        for gcard in p.graveyard:
            terms = _escape_terms(state, player_id, gcard)
            if terms is None:
                continue
            ec = terms[0]
            fast = gcard.printing.is_instant or "flash" in gcard.printing.keywords
            if fast != instant_speed:
                continue
            if (
                _escape_payable(state, player_id, gcard)
                and _can_cast(state, player_id, gcard, _effective_cost(gcard, ec))
                and _targets_are_satisfiable(state, player_id, gcard)
                and _additional_costs_payable(state, player_id, gcard)
            ):
                actions.append(CastSpell(gcard.id, use_escape=True))

    _emit_escape_casts(instant_speed=True)

    # Yawgmoth's Will: cast spells from the graveyard for their normal
    # cost (instant-speed ones now, the rest at sorcery timing below).
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        lurrus_casters as _lurrus_casters,
        lurrus_castable as _lurrus_castable,
    )

    def _emit_graveyard_permission_casts(instant_speed: bool) -> None:
        yawgmoth = p.may_play_from_graveyard_this_turn
        # Lurrus-style once-per-turn permanent casts from the graveyard;
        # per-card "you may cast that card this turn" (Emry).
        lurrus = bool(_lurrus_casters(state, player_id))
        for gcard in p.graveyard:
            pr = gcard.printing
            if pr.is_land:
                continue
            if not (
                yawgmoth
                or gcard.castable_from_graveyard_this_turn
                or (lurrus and _lurrus_castable(state, player_id, gcard) is not None)
                or _mayhem_castable(state, player_id, gcard)
            ):
                continue
            if (pr.is_instant or "flash" in pr.keywords) != instant_speed:
                continue
            if (
                _can_cast(state, player_id, gcard, _effective_cost(gcard, pr.mana_cost))
                and _targets_are_satisfiable(state, player_id, gcard)
                and _additional_costs_payable(state, player_id, gcard)
            ):
                actions.append(CastSpell(gcard.id))

    _emit_graveyard_permission_casts(instant_speed=True)

    # W/G graveyard permissions (Six's retrace, Serra Paragon;
    # ``mtgz_ir.graveyard_casting_wg``): instant-speed spells now, the
    # rest at sorcery timing below.
    from mtgz_ir.graveyard_casting_wg import graveyard_cast_options

    def _emit_wg_graveyard_casts(instant_speed: bool) -> None:
        for gcard, permission in graveyard_cast_options(state, player_id):
            pr = gcard.printing
            if (pr.is_instant or "flash" in pr.keywords) != instant_speed:
                continue
            if (
                _can_cast(state, player_id, gcard, _effective_cost(gcard, pr.mana_cost))
                and _targets_are_satisfiable(state, player_id, gcard)
                and _additional_costs_payable(state, player_id, gcard)
            ):
                actions.append(
                    CastSpell(gcard.id, graveyard_permission=permission)
                )

    _emit_wg_graveyard_casts(instant_speed=True)

    # Bolas's Citadel: the top card of the library may be cast paying
    # life equal to its mana value (instant-speed spells here; the rest
    # below at sorcery timing).
    citadel_top = _citadel_top_card(state, player_id)
    if citadel_top is not None and not citadel_top.printing.is_land and (
        citadel_top.printing.is_instant
        or "flash" in citadel_top.printing.keywords
    ):
        if _can_cast_paying_life(state, player_id, citadel_top):
            actions.append(CastSpell(citadel_top.id))
    # Exile play permissions — instants / flash cards at any priority.
    actions.extend(_exile_permission_plays(state, player_id, sorcery_window=False))

    # Evoke (CR 702.74; ``mtgz_ir.evoke``): cast from hand for the
    # evoke cost -- at instant speed for flash creatures, otherwise at
    # sorcery timing. Same-name hand copies dedup like the other hand
    # casts.
    from mtgz_ir.evoke import evoke_castable as _evoke_castable

    seen_evoke: set[str] = set()
    for card in p.hand:
        pr = card.printing
        if pr.name in seen_evoke:
            continue
        fast = pr.is_instant or "flash" in pr.keywords
        if not (fast or sorcery_timing):
            continue
        if (
            _evoke_castable(
                state, player_id, card,
                lambda cost, _c=card: _can_cast(
                    state, player_id, _c, _effective_cost(_c, cost),
                ),
            )
            and _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
        ):
            seen_evoke.add(pr.name)
            actions.append(CastSpell(card.id, use_evoke=True))

    # Bestow (CR 702.103): cast as an Aura for the bestow cost (the
    # permanent's own timing; a creature target must exist).
    seen_bestow: set[str] = set()
    for card in p.hand:
        pr = card.printing
        if pr.bestow_cost is None or pr.name in seen_bestow:
            continue
        if not (sorcery_timing or "flash" in pr.keywords):
            continue
        from mtgz_ir.game import is_creature_now as _icn

        if not any(_icn(o) for o in state.battlefield):
            continue
        if _can_cast(
            state, player_id, card, _effective_cost(card, pr.bestow_cost),
        ) and _additional_costs_payable(state, player_id, card):
            seen_bestow.add(pr.name)
            actions.append(CastSpell(card.id, use_bestow=True))

    # Vintage Roto P3 cast permissions (Mystic Forge, Omniscience, ...),
    # suspend and exile-zone activations: offered here, before the
    # instant-speed return, so every restriction filter below sees them.
    from mtgz_ir.effects.generic.roto_mana import roto_legal_actions

    actions.extend(roto_legal_actions(
        state, player_id, actions, sorcery_timing=sorcery_timing,
    ))

    if not sorcery_timing:
        # CR 117.1b / 602.5: activated abilities of permanents are
        # activatable whenever their controller has priority, unless
        # the ability or a rule restricts its timing
        # (``activation_timing_permits``).
        actions.extend(battlefield_ability_activations(
            state, player_id, sorcery_timing=False,
        ))
        # Teferi, Time Raveler: an opponent's static confines this
        # player's spell casts to their own sorcery timing.
        if _restricted_to_sorcery_speed_casts(state, player_id):
            actions = [a for a in actions if not isinstance(a, CastSpell)]
        return _filter_cast_restrictions_wg(state, player_id, actions)

    _emit_flashback_casts(instant_speed=False)
    _emit_escape_casts(instant_speed=False)
    # Emerge (CR 702.119) from hand, at the card's own timing.
    from mtgz_ir.creature_casting_ub import emerge_cast_actions

    actions.extend(emerge_cast_actions(state, player_id))
    _emit_graveyard_permission_casts(instant_speed=False)
    _emit_wg_graveyard_casts(instant_speed=False)

    # Plot (CR 702.170): the special action from hand (sorcery timing),
    # and casting a card plotted on an earlier turn from exile for free
    # (as a sorcery, whatever its type).
    seen_plot: set[str] = set()
    for card in p.hand:
        pc = card.printing.plot_cost
        if pc is None or card.printing.name in seen_plot:
            continue
        if _can_cast(state, player_id, card, pc, spell_cost=False):
            seen_plot.add(card.printing.name)
            actions.append(CastSpell(card.id, use_plot=True))
    for card in p.exile:
        if card.plotted_turn < 0 or card.plotted_turn >= state.turn_number:
            continue
        if (
            _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
            and _free_cast_tax_payable(card)
        ):
            actions.append(CastSpell(card.id, cast_for_free=True))

    # Land plays per turn at sorcery speed. Base = 1, increased by
    # permanents granting "you may play an additional land" (Icetill
    # Explorer).
    max_lands = 1 + sum(
        o.printing.grants_additional_land_per_turn
        for o in state.battlefield
        if o.controller_id == player_id
    )
    from mtgz_ir.graveyard_casting_wg import paragon_land_playable

    grants_play_from_graveyard = p.may_play_from_graveyard_this_turn or any(
        o.controller_id == player_id
        and o.printing.grants_play_lands_from_graveyard
        for o in state.battlefield
    ) or paragon_land_playable(state, player_id)
    if p.lands_played_this_turn < max_lands:
        # Hand-zone PlayLand dedup; see rationale + invariant on the
        # instant-speed dedup block above.
        seen_playland: set[tuple] = set()
        for card in p.hand:
            back = card.printing.mdfc_back_face
            if card.printing.is_land or (back is not None and back.is_land):
                key = (card.printing.name,)
                if key in seen_playland:
                    continue
                seen_playland.add(key)
                actions.append(PlayLand(card.id))
        if grants_play_from_graveyard:
            for card in p.graveyard:
                if card.printing.is_land:
                    actions.append(PlayLand(card.id))

    # Creature / sorcery / other permanent (artifact, spacecraft, ...) casts
    # at sorcery speed. Creatures/permanents with flash were already proposed
    # in the instant-speed block above, so skip them here to avoid duplicate
    # CastSpell actions.
    def _is_sorcery_speed_spell(card) -> bool:
        pr = card.printing
        return (
            pr.is_creature
            or pr.is_sorcery
            or (pr.is_permanent and not pr.is_instant and not pr.is_land)
        )

    # Hand-zone CastSpell dedup, sorcery-speed mirror; see rationale +
    # invariant on the instant-speed dedup block above.
    seen_sorcery: set[tuple] = set()

    def _emit_hand_sorcery_cast(card, **kwargs):
        key = (
            card.printing.name,
            kwargs.get("use_warp", False),
            kwargs.get("kicked", False),
            kwargs.get("cast_for_free", False),
            kwargs.get("use_impending", False),
            kwargs.get("cast_room_half", None),
            kwargs.get("use_buyback", False),
            kwargs.get("use_dash", False),
        )
        if key in seen_sorcery:
            return
        seen_sorcery.add(key)
        actions.append(CastSpell(card.id, **kwargs))

    for card in p.hand:
        if "flash" in card.printing.keywords:
            continue
        if card.printing.is_instant:
            continue
        if sorceries_have_flash and card.printing.is_sorcery:
            # Already proposed by the instant-speed block above.
            continue
        # DSK Rooms: a Room is cast for the chosen half's mana cost,
        # not the printing's combined cost. Surface one CastSpell per
        # half whose cost is payable; skip the generic cast block
        # below (the Room has no single "mana_cost" cast site).
        if card.printing.is_room:
            for half in card.printing.room_halves:
                if _can_cast(
                    state, player_id, card,
                    _effective_cost(card, half.mana_cost),
                ):
                    _emit_hand_sorcery_cast(card, cast_room_half=half.name)
            continue
        if _is_sorcery_speed_spell(card):
            if _can_cast(
                state, player_id, card, _effective_cost(card, card.printing.mana_cost),
            ) and _targets_are_satisfiable(state, player_id, card) and _additional_costs_payable(state, player_id, card):
                _emit_hand_sorcery_cast(card)
                if card.printing.kicker_cost is not None and _can_cast(
                    state, player_id, card,
                    _cost_plus_kicker(card, _effective_cost(card, card.printing.mana_cost)),
                ) and _targets_are_satisfiable(state, player_id, card, kicked=True):
                    _emit_hand_sorcery_cast(card, kicked=True)
                if card.printing.buyback_cost is not None and _can_cast(
                    state, player_id, card,
                    _cost_plus(
                        _effective_cost(card, card.printing.mana_cost),
                        card.printing.buyback_cost,
                    ),
                ):
                    _emit_hand_sorcery_cast(card, use_buyback=True)
            elif (
                card.printing.kicker_cost is not None
                and _can_cast(
                    state, player_id, card,
                    _cost_plus_kicker(card, _effective_cost(card, card.printing.mana_cost)),
                )
                and _targets_are_satisfiable(state, player_id, card, kicked=True)
                and _additional_costs_payable(state, player_id, card)
            ):
                _emit_hand_sorcery_cast(card, kicked=True)
            # Dash (CR 702.109): alternative cost from hand, sorcery
            # timing like the normal cast of the creature.
            if (
                card.printing.dash_cost is not None
                and _can_cast(
                    state, player_id, card,
                    _effective_cost(card, card.printing.dash_cost),
                )
                and _targets_are_satisfiable(state, player_id, card)
                and _additional_costs_payable(state, player_id, card)
            ):
                _emit_hand_sorcery_cast(card, use_dash=True)
            if (
                card.printing.warp_cost is not None
                and _can_cast(
                    state, player_id, card, _effective_cost(card, card.printing.warp_cost),
                )
                and _targets_are_satisfiable(state, player_id, card)
                and _additional_costs_payable(state, player_id, card)
                and p.life > card.printing.warp_additional_life_cost
            ):
                _emit_hand_sorcery_cast(card, use_warp=True)
            # Battlefield-granted warp (sorcery-speed mirror of the
            # instant-speed block above). Same priority: grant only
            # applies when no innate warp_cost is present.
            if card.printing.warp_cost is None:
                from mtgz_ir.actions import effective_warp_cost
                warp_lookup = effective_warp_cost(state, player_id, card)
                if (
                    warp_lookup is not None
                    and _can_cast(
                        state, player_id, card,
                        _effective_cost(card, warp_lookup[0]),
                    )
                    and _targets_are_satisfiable(state, player_id, card)
                    and _additional_costs_payable(state, player_id, card)
                ):
                    _emit_hand_sorcery_cast(card, use_warp=True)
            # Impending alt-cost (Duskmourn Overlord cycle). Sorcery-
            # speed only — mirrors the printed text; offered when
            # the card has an impending cost the caster can pay,
            # alongside the normal cast. ``_targets_are_satisfiable`` /
            # ``_additional_costs_payable`` checks aren't strictly
            # needed today (no Overlord targets at cast time / has
            # additional costs) but the gates mirror the warp pattern
            # so a future printing with both Impending and a target /
            # additional cost composes cleanly.
            if (
                card.printing.impending_cost is not None
                and _can_cast(
                    state, player_id, card,
                    _effective_cost(card, card.printing.impending_cost),
                )
                and _targets_are_satisfiable(state, player_id, card)
                and _additional_costs_payable(state, player_id, card)
            ):
                _emit_hand_sorcery_cast(card, use_impending=True)
            # Weftwalking free-cast variant for this sorcery-speed
            # spell. Skips the mana-cost payable check (it's free)
            # but still gates on targets / additional non-mana costs.
            if (
                (free_first_cast_active or _own_free_cast(card))
                and _targets_are_satisfiable(state, player_id, card)
                and _additional_costs_payable(state, player_id, card)
                and _free_cast_tax_payable(card)
            ):
                _emit_hand_sorcery_cast(card, cast_for_free=True)

    # Warp-cast from graveyard. Timeline Culler grants this; see
    # warp_castable_from_graveyard. Currently only sorcery-speed since
    # all such cards in the pool are creatures.
    for card in p.graveyard:
        if not card.printing.warp_castable_from_graveyard:
            continue
        if card.printing.warp_cost is None:
            continue
        if card.printing.is_instant:
            continue  # extend if a future card needs instant-speed grave warp
        if (
            _can_cast(state, player_id, card, _effective_cost(card, card.printing.warp_cost))
            and _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
            and p.life > card.printing.warp_additional_life_cost
        ):
            actions.append(CastSpell(card.id, use_warp=True))
        # NOTE: no Weftwalking free-cast variant from graveyard. The
        # warp-from-graveyard permission (Timeline Culler) is itself an
        # alternative cost (the warp cost), and CR 117.9b allows only
        # one alternative cost per cast. ``_cast_spell``'s graveyard
        # branch is gated on ``use_warp=True`` for that reason; emitting
        # a ``cast_for_free=True`` variant here would route through the
        # apply path's defensive raise (no graveyard route accepts
        # ``cast_for_free`` without ``use_warp``). Confirmed by a
        # 0.03%-rate ValueError surfaced in v16-vs-v13 eval (1/3263
        # games) when Weftwalking was on the battlefield + Timeline
        # Culler in the graveyard. See test_weftwalking_and_
        # possibility.py::test_legal_actions_does_not_offer_cast_for_
        # free_for_warp_castable_card_in_graveyard.

    # Warp-exile recasts at sorcery speed (for creatures / sorceries).
    # Instants in warp-exile could be recast at instant speed, but we don't
    # have any such cards yet — extend if/when we do.
    for card in p.exile:
        if not card.warped:
            continue
        if card.printing.is_instant:
            continue  # handled above at instant speed (none yet in the pool)
        if _can_cast(
            state, player_id, card, _effective_cost(card, card.printing.mana_cost),
        ) and _targets_are_satisfiable(state, player_id, card) and _additional_costs_payable(state, player_id, card):
            actions.append(CastSpell(card.id))

    # Exile play permissions (``GameObject.exile_play_permission``):
    # cards in ANY player's exile the player may play (Ragavan, Laelia,
    # Golos, Thor, ...). Sorcery-speed cards here; instants and flash
    # cards were offered by the instant-speed block.
    actions.extend(_exile_permission_plays(state, player_id, sorcery_window=True))

    # Impulse-cast: cards in exile marked playable_from_exile_this_turn
    # (Territorial Bruntar landfall, Memorial Vault activated) — or
    # marked play_from_exile_requires_kavu (Possibility Technician),
    # which persists until the card leaves exile but is gated on the
    # controller currently controlling a Kavu. Both pay normal mana
    # cost and respect sorcery-speed rules. Lands use PlayLand;
    # non-land permanents / sorceries use CastSpell.
    from mtgz_ir.game import effective_subtypes
    controls_kavu = any(
        o.controller_id == player_id
        and "Kavu" in effective_subtypes(state, o)
        for o in state.battlefield
    )
    for card in p.exile:
        impulse = card.playable_from_exile_this_turn
        kavu_gate = card.play_from_exile_requires_kavu and controls_kavu
        if not (impulse or kavu_gate):
            continue
        if card.warped:
            continue  # already handled above
        if card.printing.is_land:
            if p.lands_played_this_turn < (
                1 + sum(
                    o.printing.grants_additional_land_per_turn
                    for o in state.battlefield
                    if o.controller_id == player_id
                )
            ):
                actions.append(PlayLand(card.id))
            continue
        if card.printing.is_instant:
            # Instant speed — always castable if cost payable.
            if _can_cast(
                state, player_id, card, _effective_cost(card, card.printing.mana_cost),
            ) and _targets_are_satisfiable(state, player_id, card):
                actions.append(CastSpell(card.id))
            continue
        # Sorcery-speed cards: main phase, empty stack only.
        if not _is_sorcery_timing(state, player_id):
            continue
        if _can_cast(
            state, player_id, card, _effective_cost(card, card.printing.mana_cost),
        ) and _targets_are_satisfiable(state, player_id, card) and _additional_costs_payable(state, player_id, card):
            actions.append(CastSpell(card.id))

    # Lightstall Inquisitor exile-from-hand grant: the opponent may play
    # the exiled card for as long as it remains in exile, with a {1}
    # surcharge on spells and tapped-entry on lands. Sorcery-speed only
    # (oracle: "only if it's a land or they could cast it" — same timing
    # as a normal cast). Instants could technically be cast at instant
    # speed, but the oracle text's "could cast it" clause is interpreted
    # as "could cast it at sorcery timing" to keep the model simple; no
    # such card combination exists in the pool today.
    for card in p.exile:
        if not card.cast_from_exile_with_lightstall_surcharge:
            continue
        if card.warped:
            continue  # warp block above already handles this
        if card.printing.is_land:
            if _is_sorcery_timing(state, player_id) and p.lands_played_this_turn < (
                1 + sum(
                    o.printing.grants_additional_land_per_turn
                    for o in state.battlefield
                    if o.controller_id == player_id
                )
            ):
                actions.append(PlayLand(card.id))
            continue
        # Non-land: only castable at sorcery timing (mirrors the standard
        # "could cast it" restriction on the card's text).
        if not _is_sorcery_timing(state, player_id):
            continue
        # Apply +{1} surcharge on top of normal effective cost.
        base_with_surcharge = ManaCost(
            generic=(
                _effective_cost(card, card.printing.mana_cost).generic + 1
                + card.exile_cast_extra_surcharge
            ),
            colored=_effective_cost(card, card.printing.mana_cost).colored,
            x_pips=_effective_cost(card, card.printing.mana_cost).x_pips,
        )
        if (
            _can_cast(state, player_id, card, base_with_surcharge)
            and _targets_are_satisfiable(state, player_id, card)
            and _additional_costs_payable(state, player_id, card)
        ):
            actions.append(CastSpell(card.id))

    # Cast-from-library (top card only): Mm'menon, the Right Hand.
    # "You may cast artifact spells from the top of your library."
    # Offered when the controller has any permanent with the grant
    # flag AND the top card is an artifact at sorcery speed.
    has_cast_artifact_top = any(
        o.printing.grants_cast_artifact_from_top_of_library
        for o in state.battlefield
        if o.controller_id == player_id
    )
    if has_cast_artifact_top and p.library:
        top = p.library[0]
        if is_artifact_now(state, top):
            # Sorcery timing check (all artifact tops, creature or not).
            if _is_sorcery_timing(state, player_id) and _can_cast(
                state, player_id, top, _effective_cost(top, top.printing.mana_cost),
            ) and _targets_are_satisfiable(state, player_id, top):
                actions.append(CastSpell(top.id))

    # Bolas's Citadel at sorcery timing: the top card is a land (play it
    # if a land drop remains) or a sorcery-speed spell (cast it paying
    # life equal to its mana value).
    if citadel_top is not None:
        if citadel_top.printing.is_land:
            if p.lands_played_this_turn < max_lands:
                actions.append(PlayLand(citadel_top.id))
        elif not (
            citadel_top.printing.is_instant
            or "flash" in citadel_top.printing.keywords
        ) and _can_cast_paying_life(state, player_id, citadel_top):
            if CastSpell(citadel_top.id) not in actions:
                actions.append(CastSpell(citadel_top.id))

    # DSK Rooms: ``UnlockRoomHalf`` is sorcery-speed only (CR 715.4).
    # Surface one action per Room the controller controls + locked
    # half whose mana cost is currently payable. The unlock action is
    # NOT a spell (no spell_kinds context), but the same coin-pool
    # check applies via ``can_pay_cost`` with the default tag.
    for obj in state.battlefield:
        if obj.controller_id != player_id:
            continue
        if not obj.printing.is_room:
            continue
        for half in obj.printing.room_halves:
            if half.name in obj.unlocked_halves:
                continue
            if can_pay_cost(state, player_id, half.mana_cost):
                actions.append(
                    UnlockRoomHalf(card_id=obj.id, half_name=half.name)
                )

    # Activated abilities on permanents this player controls (sorcery
    # window: everything the timing rules allow, in the order this
    # block has always emitted them).
    actions.extend(battlefield_ability_activations(
        state, player_id, sorcery_timing=True,
    ))

    # Station activations: for each permanent with station (a Spacecraft,
    # a planet) this player controls, each untapped non-sick creature they
    # control (not the station permanent itself) is a legal crew choice.
    spacecraft = [
        o for o in state.battlefield
        if o.controller_id == player_id and o.printing.has_station
    ]
    if spacecraft:
        crew_candidates = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and o.printing.is_creature
            and not o.tapped
            and not o.summoning_sick
        ]
        for ship in spacecraft:
            for crew in crew_candidates:
                if crew.id == ship.id:
                    continue
                actions.append(
                    ActivateStation(spacecraft_id=ship.id, crew_id=crew.id)
                )

    # Turn-face-up special action (CR 701.34 / 707.4): for each
    # face-down permanent the player controls whose underlying
    # printing is a creature card whose mana cost they can pay, offer
    # the action. Special action — does not use the stack — but it's
    # a sorcery-speed timing in our simplified model (CR 707.4
    # actually permits "any time", but the only paths into this engine
    # that matter resolve in main phases anyway, and tying it to
    # sorcery timing avoids interactions with priority windows we
    # don't yet model).
    for obj in state.battlefield:
        if obj.controller_id != player_id:
            continue
        if not obj.face_down:
            continue
        if not obj.printing.is_creature:
            continue
        if not can_pay_cost(state, player_id, obj.printing.mana_cost):
            continue
        from mtgz_ir.effects.generic.roto_layers import (
            turning_face_up_prevented,
        )

        if turning_face_up_prevented(state, obj):
            # "As long as enchanted creature is face down, it can't be
            # turned face up." (Unable to Scream.)
            continue
        actions.append(TurnFaceUp(card_id=obj.id))

    # Deafening Silence gate: post-filter CastSpell actions so
    # noncreature casts beyond the per-turn limit of 1 are removed
    # when any permanent with the restriction is on the battlefield.
    # Kept as a post-filter because CastSpell is proposed from many
    # sites (hand, warp-exile, impulse-exile, cast-from-library) and
    # touching every site would risk drift.
    if any(
        o.printing.restricts_noncreature_spells_to_one_per_turn
        for o in state.battlefield
    ):
        p = state.player_by_id(player_id)
        if p.noncreature_spells_cast_this_turn >= 1:
            filtered: list[Action] = []
            for action in actions:
                if isinstance(action, CastSpell):
                    card = _find_card_by_id(state, player_id, action.card_id)
                    if card is not None and not card.printing.is_creature:
                        continue
                filtered.append(action)
            actions = filtered

    return _filter_cast_restrictions_wg(state, player_id, actions)


def _filter_cast_restrictions_wg(state, player_id, actions):
    """Cube W/G creature statics that forbid casting (Voice of Victory,
    Archon of Emeria) or activating (Clarion Conqueror) -- see
    ``mtgz_ir.creature_statics_wg``."""
    from mtgz_ir.creature_statics_wg import (
        filter_activation_locks,
        filter_cast_restrictions,
        filter_unusable_abilities,
    )

    actions = filter_cast_restrictions(state, player_id, actions)
    actions = filter_unusable_abilities(state, actions)
    actions = filter_activation_locks(state, actions)
    # Roto P1 prohibitions (named locks, "can't cast ... this turn").
    from mtgz_ir.roto_restrictions import filter_actions as _roto_filter

    actions = _roto_filter(state, player_id, actions)
    # "Cast this spell only before ..." (Berserk; Vintage Roto P5).
    from mtgz_ir.roto_board_statics import filter_cast_timing

    actions = filter_cast_timing(state, player_id, actions)
    # Split second (roto P6).
    from mtgz_ir.effects.generic.roto_keywords import filter_split_second

    return filter_split_second(state, actions)


def _find_card_by_id(state: GameState, player_id: int, card_id: int):
    """Look up a card by id across zones where CastSpell can source
    from: hand, exile (warp / impulse), and library (cast-from-top).
    Returns None if not found — defensive; CastSpell proposals should
    always reference a card the caster can see."""
    p = state.player_by_id(player_id)
    for zone in (p.hand, p.exile):
        for c in zone:
            if c.id == card_id:
                return c
    if p.library and p.library[0].id == card_id:
        return p.library[0]
    return None
