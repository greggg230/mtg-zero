"""Divided damage: "<Name> deals N damage divided as you choose among any
number of target creatures" (Pyrokinesis, Fire Covenant) and "among up to
two target creatures and/or planeswalkers" (Shatterskull Smashing).

CR 601.2d: the division is announced while casting, after targets are
chosen — each target gets at least 1. ``actions._cast_spell`` asks the
effect for the total (``division_total``) and records the split on the
StackItem (``StackItem.division``); resolution reads it back through
``state.resolving_division``. Damage assigned to a target that is no
longer legal on resolution is simply not dealt.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Union

from mtgz_ir.effects.base import Effect

if TYPE_CHECKING:
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


_ANY_NUMBER_CAP = 99


@dataclass(frozen=True)
class DealDamageDividedAmongTargets(Effect):
    """Deal ``amount`` damage divided among the chosen targets.

    * ``amount`` — int, or ``ResolvingX()`` for X spells / "pay X life".
    * ``max_targets`` — ``None`` for "any number of target ...",
      else the printed cap ("up to two").
    * ``include_planeswalkers`` — "creatures and/or planeswalkers".
    * ``double_if_x_at_least`` — Shatterskull Smashing's "If X is 6 or
      more, ... deals twice X damage ... instead" (0 = no rider).
    """

    amount: "Union[int, Formula]" = 1
    max_targets: "int | None" = None
    include_planeswalkers: bool = False
    double_if_x_at_least: int = 0

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (1, self.max_targets if self.max_targets is not None else _ANY_NUMBER_CAP)

    def _eligible(self, o) -> bool:
        from mtgz_ir.game import is_creature_now

        if o.phased_out:
            return False
        if is_creature_now(o):
            return True
        return self.include_planeswalkers and o.printing.is_planeswalker

    def legal_targets(self, state, controller_id):
        from mtgz_ir.game import is_creature_now

        out = [
            ("creature" if is_creature_now(o) else "planeswalker", o.id)
            for o in state.battlefield if self._eligible(o)
        ]
        # Opponents' permanents first: agents that take the first legal
        # target(s) aim the damage at the other side.
        out.sort(key=lambda t: next(
            o.controller_id == controller_id
            for o in state.battlefield if o.id == t[1]
        ))
        return out

    def division_total(self, state: "GameState", controller_id: int, x_value: int) -> int:
        if isinstance(self.amount, int):
            total = self.amount
        else:
            saved = state.resolving_x_value
            state.resolving_x_value = x_value
            try:
                total = self.amount.value(state, controller_id)
            finally:
                state.resolving_x_value = saved
        if self.double_if_x_at_least and x_value >= self.double_if_x_at_least:
            total *= 2
        return max(0, total)

    def useful_x_cap(self, state: "GameState", controller_id: int) -> "int | None":
        """X beyond the total toughness + loyalty of everything that could
        be hit only wastes life (Fire Covenant's pay-X-life cap)."""
        from mtgz_ir.game import effective_toughness, is_creature_now

        total = 0
        for o in state.battlefield:
            if o.controller_id == controller_id or not self._eligible(o):
                continue
            if is_creature_now(o):
                total += max(0, effective_toughness(state, o) - o.damage)
            else:
                total += o.loyalty_counters
        return total

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage

        division = tuple(state.resolving_division)
        if len(division) != len(targets):
            # No announced split (a copy, or a direct resolve): the whole
            # amount goes to the first target.
            total = self.division_total(state, controller_id, state.resolving_x_value)
            division = (total,) + (0,) * (len(targets) - 1) if targets else ()
        for (kind, tid), dmg in zip(targets, division):
            if dmg <= 0:
                continue
            obj = next((o for o in state.battlefield if o.id == tid), None)
            if obj is None or not self._eligible(obj):
                state.log.append(f"divided damage target #{tid} is gone")
                continue
            if obj.printing.is_planeswalker and kind == "planeswalker":
                obj.loyalty_counters = max(0, obj.loyalty_counters - dmg)
                state.log.append(
                    f"{obj.printing.name} takes {dmg} damage "
                    f"(loyalty {obj.loyalty_counters})"
                )
            else:
                _apply_damage(state, ("creature", tid), dmg)

    def describe(self) -> str:
        amt = self.amount if isinstance(self.amount, int) else "X"
        who = "any number of" if self.max_targets is None else f"up to {self.max_targets}"
        kinds = "creatures and/or planeswalkers" if self.include_planeswalkers else "creatures"
        return f"deals {amt} damage divided as you choose among {who} target {kinds}"


def choose_damage_division(
    state: "GameState", player_id: int, label: str, n_targets: int, total: int,
) -> tuple[int, ...]:
    """Split ``total`` among ``n_targets`` (each at least 1) by asking
    ``choose_x`` how much goes to each target in order; the last target
    gets the remainder."""
    if n_targets <= 0:
        return ()
    p = state.player_by_id(player_id)
    division: list[int] = []
    remaining = total
    for i in range(n_targets - 1):
        upper = remaining - (n_targets - i - 1)
        amt = upper
        if p.agent is not None:
            amt = p.agent.choose_x(state, player_id, f"{label} (target {i + 1})", upper)
        amt = max(1, min(upper, amt))
        division.append(amt)
        remaining -= amt
    division.append(remaining)
    return tuple(division)
