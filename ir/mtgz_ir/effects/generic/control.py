"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import (
    Effect, Target, TriggeredAbility,
    _apply_damage, _multicolored_permanents_count,
    _find_moonlit_replacement_source,
)
from mtgz_ir.effects.generic.targets_helpers import (
    _eff_subtypes,
    _slot_target_still_legal,
    _fizzle_message,
    _describe_targets,
    _describe_filter,
    _bounce_to_hand,
)

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class ControlTargetOpponentNextTurn(Effect):
    """The Dominion Bracelet: "You control target opponent during
    their next turn."

    Implementation: schedule the target player's ``scheduled_agent_
    override`` to the source-controller's agent, taking effect the
    next time it's the target's turn. On cleanup of that turn, the
    override is reverted automatically by run_cleanup_step.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id)
            for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        target = state.player_by_id(target_id)
        caster = state.player_by_id(controller_id)
        # Schedule the override: the caster's agent will be swapped
        # into the target at the start of the target's next turn.
        target.scheduled_agent_override = caster.agent
        target.scheduled_agent_override_by = controller_id
        state.log.append(
            f"P{controller_id} will control P{target_id} during "
            f"P{target_id}'s next turn"
        )

    def describe(self) -> str:
        return "you control target opponent during their next turn"



@dataclass(frozen=True)
class ChangeControllerToCombatDamageSource(Effect):
    """Contested War Zone: "Whenever a creature deals combat damage to
    you, that creature's controller gains control of this land."

    Phase-9 wired the predicate-style ``Event.DEALS_COMBAT_DAMAGE``
    dispatch to thread the attacker's controller through
    ``state.resolving_combat_damage_source_controller_id`` (mirrored
    onto the StackItem) rather than ``fixed_targets``. The slot
    works the same way as ``state.resolving_combat_damage_amount``:
    set on push, mirrored at resolve time, cleared after. Permanent
    continuous effect (not UEOT); the new controller keeps the land
    until another control-change effect overrides it.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        new_cid = state.resolving_combat_damage_source_controller_id
        if new_cid is None or new_cid == source.controller_id:
            return
        old_cid = source.controller_id
        source.controller_id = new_cid
        state.log.append(
            f"P{new_cid} gains control of {source.printing.name} "
            f"(was P{old_cid})"
        )

    def describe(self) -> str:
        return "that creature's controller gains control of this land"



@dataclass(frozen=True)
class GainControlOfTargetSliverPermanently(Effect):
    """"Gain control of target Sliver. (This effect lasts indefinitely.)"

    Matches Sliver Overlord's second activated ability. Unlike temporary
    steal effects (Mutinous Massacre, Control-target-opponent), this
    one reassigns controller_id without scheduling a revert — it is
    not reset at cleanup. The Sliver becomes permanently the
    activator's.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and "Sliver" in _eff_subtypes(state, o)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        if "Sliver" not in _eff_subtypes(state, obj):
            return
        if obj.controller_id == controller_id:
            # Already yours — no change, but log for visibility.
            state.log.append(
                f"P{controller_id} targets own Sliver "
                f"{obj.printing.name} — no change"
            )
            return
        obj.controller_id = controller_id
        # Permanent gain of control: no UEOT registry entry, no
        # cleanup revert. (Pre-registry, this branch contrasted itself
        # with a now-retired ``temp_original_controller_id`` UEOT
        # mirror; UEOT reverts now flow through the layer-2 registry
        # path.)
        state.log.append(
            f"P{controller_id} permanently gains control of "
            f"{obj.printing.name}"
        )

    def describe(self) -> str:
        return "gain control of target Sliver (indefinitely)"
