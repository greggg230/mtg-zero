"""Damage variants for spells: X damage and damage with a "dies this
turn -> exile instead" rider.

- :class:`DealXDamage` — "<Name> deals X damage to <target>" where X is
  the resolving spell's X (Traumatic Critique).
- :class:`DealDamageExileIfDiesThisTurn` — "<Name> deals N damage to
  target creature. If that creature would die this turn, exile it
  instead." (Suplex mode 1): marks the target
  (``GameObject.exile_if_dies_this_turn``, consumed by
  ``turn.move_off_battlefield``) and then deals the damage.
- :class:`DealDamagePaidInEnergy` — "Choose target <np>. You get
  {E}{E}{E}, then you may pay any amount of {E}. <Name> deals that much
  damage to that permanent." (Galvanic Discharge).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Target, _apply_damage
from mtgz_ir.effects.generic.damage import DealDamage
from mtgz_ir.effects.generic.targets_helpers import (
    _describe_targets,
    _fizzle_message,
    _slot_target_still_legal,
)

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


def _legal_chosen(state, slots, targets, controller_id):
    prior = state.resolving_controller_id
    try:
        state.resolving_controller_id = controller_id
        out = []
        for slot, chosen in zip(slots, targets):
            if _slot_target_still_legal(state, slot, chosen):
                out.append(chosen)
            else:
                state.log.append(_fizzle_message(state, chosen))
        return out
    finally:
        state.resolving_controller_id = prior


@dataclass(frozen=True)
class DealXDamage(DealDamage):
    """``DealDamage`` whose amount is the resolving spell's X."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        x = state.resolving_x_value
        for chosen in _legal_chosen(state, self.targets, targets, controller_id):
            _apply_damage(state, chosen, x)

    def describe(self) -> str:
        return f"deals X damage to {_describe_targets(self.targets)}"


@dataclass(frozen=True)
class DealDamageExileIfDiesThisTurn(DealDamage):
    """``DealDamage`` plus "If that creature would die this turn, exile
    it instead." for each damaged creature."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        for chosen in _legal_chosen(state, self.targets, targets, controller_id):
            obj = next(
                (o for o in state.battlefield if o.id == chosen[1]), None,
            )
            if obj is not None and chosen[0] != "player":
                obj.exile_if_dies_this_turn = True
            _apply_damage(state, chosen, self.amount)

    def describe(self) -> str:
        return (
            f"deals {self.amount} damage to "
            f"{_describe_targets(self.targets)}; if it would die this "
            f"turn, exile it instead"
        )


@dataclass(frozen=True)
class DealDamagePaidInEnergy(DealDamage):
    """"Choose target <np>. You get {E}xN, then you may pay any amount
    of {E}. <Name> deals that much damage to that permanent."

    The controller gets ``energy`` energy counters (CR 122.1), then
    chooses how many of ALL their energy counters to pay (``choose_x``
    over ``[0, energy total]``); the target is dealt that much damage.
    ``amount`` is unused (the damage is the energy paid). If the target
    is illegal on resolution the spell doesn't resolve (CR 608.2b): no
    energy is gained."""

    energy: int = 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        legal = _legal_chosen(state, self.targets, targets, controller_id)
        if not legal:
            return
        p = state.player_by_id(controller_id)
        p.energy += self.energy
        state.log.append(
            f"P{controller_id} gets {self.energy} energy (now {p.energy})"
        )
        paid = p.energy
        if p.agent is not None:
            paid = p.agent.choose_x(
                state, controller_id, "energy to pay", p.energy,
            )
        paid = max(0, min(paid, p.energy))
        p.energy -= paid
        state.log.append(
            f"P{controller_id} pays {paid} energy (now {p.energy})"
        )
        if paid:
            _apply_damage(state, legal[0], paid)

    def describe(self) -> str:
        return (
            f"choose {_describe_targets(self.targets)}; you get "
            + "{E}" * self.energy
            + ", then pay any amount of {E} to deal that much damage to it"
        )


__all__ = [
    "DealDamageExileIfDiesThisTurn",
    "DealDamagePaidInEnergy",
    "DealXDamage",
]
