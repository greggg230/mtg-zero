"""Generic effects: shared module-level helpers for legal-target checks,
fizzle messaging, target/filter descriptions, and the canonical
bounce-to-hand zone-move primitive. These helpers are imported by every
cluster module that handles targeted effects."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


def _eff_subtypes(state: "GameState", obj) -> tuple[str, ...]:
    """Local alias for ``mtgz_ir.game.effective_subtypes`` so
    subtype-keyed legal-target filters (anti-subtype, allowed-subtype,
    target-subtype) can read the layered-effects view of a battlefield
    permanent's subtypes without circular-import gymnastics. Lazy
    import — the function-level fallthrough avoids importing game at
    module load."""
    from mtgz_ir.game import effective_subtypes
    return effective_subtypes(state, obj)


def _slot_target_still_legal(state, slot, chosen):
    """Return True if ``chosen`` (engine ``("kind", id)`` tuple) still
    satisfies ``slot.filter`` against the current ``state``. Pure
    helper used by ``DealDamage.resolve`` and any future new-shape
    re-verification path."""
    kind, target_id = chosen
    if kind == "player":
        try:
            obj = state.player_by_id(target_id)
        except KeyError:
            return False
        if getattr(obj, "has_lost", False):
            return False
    else:
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return False
    return slot.filter.test(obj, state)


def _fizzle_message(state, chosen, action: str = "damage"):
    """Produce the user-visible log line for a target whose at-resolve
    legality re-check failed. Names the target where possible so the
    log entry reads like the legacy per-class fizzle messages without
    each effect having to compose its own string. ``action`` slots into
    the trailing "...fizzles" phrase ("damage fizzles", "destroy
    fizzles", "exile fizzles") so cross-family migrations can share
    the helper."""
    kind, target_id = chosen
    if kind == "player":
        return (
            f"P{target_id} is no longer a legal target — {action} fizzles"
        )
    obj = next(
        (o for o in state.battlefield if o.id == target_id), None
    )
    if obj is None:
        return (
            f"target is no longer on the battlefield — {action} fizzles"
        )
    name = obj.printing.name
    if action == "damage" and not getattr(obj, "tapped", False):
        # Common case for damage targeting tapped creatures: the slot's
        # filter was IsCreature() & IsTapped() and the creature
        # untapped between target-pick and resolve. Naming the specific
        # change reads cleanest in the log; falls back to the generic
        # phrasing for other re-check failures.
        return f"{name} is no longer tapped — damage fizzles"
    return f"{name} is no longer a legal target — {action} fizzles"


def _describe_targets(targets) -> str:
    """Coarse human-readable rendering of a target-slot tuple. PR #3
    only ships single-slot damage but the helper handles the multi-
    slot case for forward compat."""
    from mtgz_ir.effects.targets import SlotRef, TargetSpec
    if not targets:
        return "any target"
    parts: list[str] = []
    for slot in targets:
        if isinstance(slot, SlotRef):
            parts.append(f"that target (slot {slot.idx})")
        elif isinstance(slot, TargetSpec):
            parts.append(_describe_filter(slot.filter))
        else:
            parts.append("?")
    return " and ".join(parts)


def _describe_filter(target_filter) -> str:
    """Render a ``TargetFilter`` as a printed-style noun phrase for
    log lines / describe(). Coarse — exact wording is owned by the
    parser's phrasebook on the way in."""
    from mtgz_ir.effects.object_predicates import (
        BlockedBySource, ControlledByOpponent, ControlledByYou,
        IsArtifact, IsAttacking, IsBasic, IsBlocking, IsCreature,
        IsEnchantment, IsLand, IsPlayer, IsTapped, ObjectAnd,
        ObjectNot, ObjectOr,
    )
    from mtgz_ir.effects.target_filter import OneOf
    from mtgz_ir.effects.targets import any_target_filter
    if target_filter == any_target_filter():
        return "any target"
    if isinstance(target_filter, IsCreature):
        return "target creature"
    if isinstance(target_filter, IsPlayer):
        return "target player"
    if isinstance(target_filter, IsArtifact):
        return "target artifact"
    if isinstance(target_filter, IsLand):
        return "target land"
    if isinstance(target_filter, IsEnchantment):
        return "target enchantment"
    if isinstance(target_filter, ObjectAnd):
        l = target_filter.left
        r = target_filter.right
        if isinstance(l, IsCreature) and isinstance(r, IsTapped):
            return "target tapped creature"
        if isinstance(l, IsCreature) and isinstance(r, ControlledByOpponent):
            return "target creature an opponent controls"
        if isinstance(l, IsCreature) and isinstance(r, ControlledByYou):
            return "target creature you control"
        if isinstance(l, IsCreature) and isinstance(r, BlockedBySource):
            return "target creature it's blocking"
        if isinstance(l, IsPlayer) and isinstance(r, ControlledByOpponent):
            return "target opponent"
        if isinstance(l, IsLand) and isinstance(r, ObjectNot) and isinstance(
            r.inner, IsBasic
        ):
            return "target nonbasic land"
        if isinstance(l, IsCreature) and isinstance(r, ObjectOr):
            inner_l, inner_r = r.left, r.right
            if (isinstance(inner_l, IsAttacking)
                    and isinstance(inner_r, IsBlocking)):
                return "target attacking or blocking creature"
    if isinstance(target_filter, OneOf):
        return "any target"  # default rendering for OneOf disjuncts
    return "target"


def _bounce_to_hand(state: "GameState", obj) -> None:
    """Common bounce-side-effects helper. Removes ``obj`` from the
    battlefield, resets combat / counters / temp_* state, marks the
    Void-tracking flag, and either lets a token cease to exist or
    moves the card to its owner's hand and reveals it. Pure helper —
    used by every variant of the bounce family so each subclass
    needs only the per-card filter and any rider snapshot logic.
    """
    from mtgz_ir.game import Zone, reveal_to_all

    if obj.exile_instead_of_leaving:
        from mtgz_ir.turn import move_off_battlefield

        move_off_battlefield(state, obj, destination="exile")
        return
    state.battlefield.remove(obj)
    from mtgz_ir.game import restore_mdfc_front

    restore_mdfc_front(obj)
    obj.damage = 0
    obj.marked_lethal = False
    obj.damaged_this_turn = False
    obj.tapped = False
    obj.attacking = False
    obj.blocking = None
    obj.blocked_by = []
    obj.plus_one_counters = 0
    obj.charge_counters = 0
    obj.temp_power_bonus = 0
    obj.temp_toughness_bonus = 0
    if not obj.printing.is_land:
        state.permanent_left_battlefield_this_turn = True
    state.player_by_id(obj.controller_id).permanent_left_battlefield_this_turn = True
    if obj.printing.is_token:
        state.log.append(
            f"{obj.printing.name} token ceases to exist (bounced)"
        )
        return
    obj.zone = Zone.HAND
    state.player_by_id(obj.owner_id).hand.append(obj)
    reveal_to_all(state, obj)
    state.log.append(
        f"{obj.printing.name} returns to P{obj.owner_id}'s hand"
    )
