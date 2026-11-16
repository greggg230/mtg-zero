"""Effects of the Legacy cards outside the cube (pool ``legacy``) that no
earlier class expressed: mtg-zero's Legacy Sneak and Show deck
(MTGGoldfish 8014220). Show and Tell, Omniscience, Mistrise Village,
Disruptor Flute and Magus of the Moon reuse existing classes; Brotherhood's
End's first mode needed one:

* :class:`DealNDamageToEachCreatureAndPlaneswalker` -- "<This> deals N
  damage to each creature and each planeswalker." (Brotherhood's End,
  Storm's Wrath).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class DealNDamageToEachCreatureAndPlaneswalker(Effect):
    """"<This> deals ``amount`` damage to each creature and each
    planeswalker." Battles are not included. A permanent that is both a
    creature and a planeswalker is one object of the set and is dealt the
    damage once (CR 120.3: one damage event, with both results: damage
    marked, loyalty counters removed).

    Snapshot semantics, as :class:`DealNDamageToEachCreature`: the victims
    are listed before any damage is dealt."""

    amount: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage

        victims = [
            ("creature", o.id) if o.printing.is_creature else ("planeswalker", o.id)
            for o in state.battlefield
            if o.printing.is_creature or o.printing.is_planeswalker
        ]
        for v in victims:
            _apply_damage(state, v, self.amount)

    def describe(self) -> str:
        return f"deals {self.amount} damage to each creature and each planeswalker"
