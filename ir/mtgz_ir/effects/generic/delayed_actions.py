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
class SacrificeSourceDelayed(Effect):
    """Delayed-triggered "sacrifice this permanent" effect.

    Scheduled with ``source_id`` set to the permanent that should be
    sacrificed. Used by Kav Landseeker (sacrifice the Lander token at
    P0's next end step) and Devastating Onslaught (sacrifice the haste-
    token copies at the next end step).

    On resolve, look up the source on the battlefield by id and
    sacrifice it. No-op if it's already gone (CR 603.6c) — the trigger
    still fires, it just has nothing to act on.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice

        ref = state.resolving_source
        if ref is None:
            return
        obj = next(
            (o for o in state.battlefield if o.id == ref.id), None,
        )
        if obj is None:
            return
        state.log.append(
            f"P{controller_id} sacrifices {obj.printing.name} "
            f"(delayed trigger)"
        )
        sacrifice(state, obj)

    def describe(self) -> str:
        return "sacrifice this permanent"



@dataclass(frozen=True)
class ScheduleSelfSacAtNextEndStep(Effect):
    """"Sacrifice this permanent at the beginning of the next end step."

    Reads ``state.resolving_source`` for the permanent to sacrifice and
    schedules a delayed ``SacrificeSourceDelayed`` trigger keyed at the
    current turn — the same "next end step on the current turn"
    interpretation Devastating Onslaught uses for its haste-token sac
    scheduling and Attack-in-the-Box's "if you do, sacrifice it at the
    beginning of the next end step" rider.

    On resolve, ``state.delayed_triggers`` gains an entry with
    ``fire_at_turn = state.turn_number`` (the current turn's upcoming
    end step), ``controller_id`` set to the controller, and
    ``source_id`` set to ``state.resolving_source.id``. The standard
    ``run_delayed_end_step_triggers`` machinery then pushes a synthetic
    ``BEGINNING_OF_END_STEP`` trigger that fires
    ``SacrificeSourceDelayed`` against the same source.

    Returns ``True`` when the schedule lands; ``False`` when there's
    no resolving source (defensive — this should never happen at a
    real callsite, but keeps composition with ``IfYouDo`` honest).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        source = state.resolving_source
        if source is None:
            return False
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number,
            "controller_id": controller_id,
            "source_id": source.id,
            "effect": SacrificeSourceDelayed(),
        })
        state.log.append(
            f"P{controller_id} schedules {source.printing.name} to be "
            f"sacrificed at the beginning of the next end step"
        )
        return True

    def describe(self) -> str:
        return (
            "sacrifice this permanent at the beginning of the next "
            "end step"
        )



@dataclass(frozen=True)
class RemoveChargeCountersFromSourceDelayed(Effect):
    """Delayed-triggered "remove N charge counters from this permanent"
    effect.

    Scheduled with ``source_id`` set to the Spacecraft to be reset.
    Used by Systems Override (add 10 charge counters now, remove 10
    at the next end step).

    On resolve, look up the source on the battlefield by id and
    decrement charge counters (clamped at 0). No-op if the source has
    left the battlefield.
    """

    amount: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        ref = state.resolving_source
        if ref is None:
            return
        obj = next(
            (o for o in state.battlefield if o.id == ref.id), None,
        )
        if obj is None:
            return
        obj.charge_counters = max(0, obj.charge_counters - self.amount)
        state.log.append(
            f"remove {self.amount} charge counters from "
            f"{obj.printing.name} (now {obj.charge_counters})"
        )

    def describe(self) -> str:
        return (
            f"remove {self.amount} charge counters from this permanent"
        )


def schedule_at_next_end_step(
    state: "GameState", controller_id: int, source_id: int, effect: Effect,
) -> None:
    """Schedule ``effect`` as a delayed trigger "at the beginning of the
    next end step" (CR 603.7) — whoever's turn that end step belongs to.

    ``source_id`` is the object the effect acts on (read back through
    ``state.resolving_source`` when the delayed trigger resolves). If the
    current turn's end step has already begun, the trigger waits for the
    following turn's end step.
    """
    from mtgz_ir.turn import Step

    fire_turn = state.turn_number
    if state.step in (Step.END, Step.CLEANUP):
        fire_turn += 1
    state.delayed_triggers.append({
        "fire_at_turn": fire_turn,
        "controller_id": controller_id,
        "source_id": source_id,
        "effect": effect,
        "any_player_end_step": True,
    })


@dataclass(frozen=True)
class ExileSourceDelayed(Effect):
    """Delayed-triggered "exile it" (Shallow Grave / Corpse Dance /
    Goryo's Vengeance: "Exile it at the beginning of the next end
    step."). Exiles the scheduled object if it is still on the
    battlefield; no-op otherwise (CR 603.6c)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        ref = state.resolving_source
        if ref is None:
            return
        obj = next((o for o in state.battlefield if o.id == ref.id), None)
        if obj is None:
            return
        state.log.append(f"{obj.printing.name} is exiled (delayed trigger)")
        move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return "exile it"
