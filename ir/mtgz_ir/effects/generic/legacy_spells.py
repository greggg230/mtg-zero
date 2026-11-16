"""Effects of the Legacy cards outside the cube (pool ``legacy``) that no
earlier class expressed: mtg-zero's Legacy Doomsday deck, lane A
(Doomsday, Pact of Negation, Personal Tutor, Flow State; the other three
reuse existing classes).

* :class:`PayManaOrElse` -- "Pay <cost>. If you don't, <effect>." (Pact of
  Negation's delayed triggered ability).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class PayManaOrElse(Effect):
    """"Pay <cost>. If you don't, <otherwise>." (Pact of Negation: "At the
    beginning of your next upkeep, pay {3}{U}{U}. If you don't, you lose the
    game.")

    The controller of the resolving ability chooses whether to pay
    ``cost`` (a mana cost string, ``"{3}{U}{U}"``); they may decline even
    when they could pay, and may activate mana abilities during the payment
    (CR 605.3a, 117.1d). If they don't pay -- they declined or couldn't --
    ``otherwise`` happens (CR 118.12a reads "unless" the same way). The
    payment is offered mid-resolution like ``DestroySourceUnlessPay``'s
    (``pay_mana_during_resolution``)."""

    cost: str
    otherwise: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.cards.base import parse_mana_cost
        from mtgz_ir.effects.generic.cheat_into_play import (
            pay_mana_during_resolution,
        )

        if pay_mana_during_resolution(
            state, controller_id, parse_mana_cost(self.cost),
            f"Pay {self.cost}?",
        ):
            state.log.append(f"P{controller_id} pays {self.cost}")
            return
        state.log.append(f"P{controller_id} doesn't pay {self.cost}")
        self.otherwise.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"pay {self.cost}. If you don't, {self.otherwise.describe()}"
