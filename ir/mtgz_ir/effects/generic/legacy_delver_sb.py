"""Effects of the Legacy cards outside the cube (pool ``legacy``) that no
earlier class expressed: the Legacy Izzet Delver 75's sideboard (mtg-zero's
Delver mirror, MTGGoldfish 8016541). Consign to Memory, Hydroblast,
Meltdown and Price of Progress reuse existing classes; Rough // Tumble's
halves needed one:

* :class:`DealNDamageToEachCreatureMatching` -- "<This> deals N damage to
  each creature with <keyword>" / "... without <keyword>" (Rough //
  Tumble's halves: "Rough deals 2 damage to each creature without flying.",
  "Tumble deals 6 damage to each creature with flying."; also the shape of
  Hurricane's and Whirlwind's creature halves).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.object_predicates import ObjectPredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class DealNDamageToEachCreatureMatching(Effect):
    """"<This> deals ``amount`` damage to each creature <qualifier>": each
    creature on the battlefield for which ``creature_filter`` holds as the
    effect resolves ("with flying": ``HasKeyword("flying")``; "without
    flying": its negation). The creatures are the permanents that are
    creatures now (``is_creature_now``: an animated land too) and the filter
    reads their current characteristics (granted keywords count). No player
    is dealt damage.

    Snapshot semantics, as :class:`DealNDamageToEachCreature`: the victims
    are listed before any damage is dealt (CR 120.3: the damage is dealt
    at once)."""

    amount: int
    creature_filter: ObjectPredicate

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.game import is_creature_now

        victims = [
            o for o in state.battlefield
            if is_creature_now(o) and self.creature_filter.test(o, state)
        ]
        for o in victims:
            _apply_damage(state, ("creature", o.id), self.amount)

    def describe(self) -> str:
        return f"deals {self.amount} damage to each matching creature"
