"""Incubate (CR 701.53) and the transform it enables.

"Incubate N" creates an Incubator token — an artifact with N +1/+1
counters and "{2}: Transform this artifact." — whose back face is a 0/0
Phyrexian artifact creature (the counters make it an N/N). The token
printings live in ``mtgz_ir.cards.tokens`` ("Incubator", "Phyrexian");
transforming swaps the permanent's printing to the back face in place
(same object: counters, damage and control carry over).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Union

from mtgz_ir.effects.base import Effect

if TYPE_CHECKING:
    from mtgz_ir.effects.formulas import Formula


@dataclass(frozen=True)
class TransformSelfIntoPrinting(Effect):
    """"Transform this <permanent>." for a one-way double-faced token:
    the resolving source (if still on the battlefield) becomes
    ``back_printing``."""

    back_printing: "object" = None

    def resolve(self, state, controller_id, targets=()):
        src = state.resolving_source
        if src is None or self.back_printing is None:
            return
        obj = next((o for o in state.battlefield if o.id == src.id), None)
        if obj is None or obj.printing is self.back_printing:
            return
        obj.printing = self.back_printing
        state._invalidate_layered_cache()
        state.log.append(f"{src.printing.name} transforms into {self.back_printing.name}")

    def describe(self) -> str:
        return "transform this artifact"


def incubate(state, controller_id: int, n: int):
    """Create an Incubator token with ``n`` +1/+1 counters."""
    from mtgz_ir.actions import put_into_battlefield
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    from mtgz_ir.game import Zone, add_plus_one_counters

    token = state.new_object(
        TOKEN_PRINTINGS["Incubator"], owner_id=controller_id, zone=Zone.BATTLEFIELD,
    )
    saved_x = state.resolving_x_value
    state.resolving_x_value = 0
    try:
        put_into_battlefield(state, token, controller_id)
    finally:
        state.resolving_x_value = saved_x
    if n > 0:
        add_plus_one_counters(state, token, n)
    state.log.append(f"P{controller_id} incubates {n}")
    return token


@dataclass(frozen=True)
class Incubate(Effect):
    """"Incubate N." ``amount`` is an int or a ``Formula``."""

    amount: "Union[int, Formula]" = 1

    def resolve(self, state, controller_id, targets=()):
        n = self.amount if isinstance(self.amount, int) else self.amount.value(state, controller_id)
        incubate(state, controller_id, max(0, n))

    def describe(self) -> str:
        amt = self.amount if isinstance(self.amount, int) else "X"
        return f"incubate {amt}"


@dataclass(frozen=True)
class ExileAllCreaturesThenIncubate(Effect):
    """Sunfall: "Exile all creatures. Incubate X, where X is the number of
    creatures exiled this way." Tokens that leave count as exiled (they
    were exiled, then ceased to exist)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import is_creature_now
        from mtgz_ir.turn import move_off_battlefield

        creatures = [o for o in state.battlefield if is_creature_now(o)]
        exiled = 0
        for o in creatures:
            if o in state.battlefield:
                move_off_battlefield(state, o, destination="exile")
                exiled += 1
        state.log.append(f"{exiled} creature(s) exiled")
        incubate(state, controller_id, exiled)

    def describe(self) -> str:
        return (
            "exile all creatures; incubate X, where X is the number of "
            "creatures exiled this way"
        )
