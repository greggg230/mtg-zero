"""Filter-driven counterspells: "counter target <spell np> [unless its
controller pays {N}]", with the "If that spell is countered this way,
<put it somewhere else> instead" destination riders and the Pyroblast
"counter target spell if it's <color>" resolution condition.

The engine counters a spell by flagging its ``StackItem`` (``countered``)
and moving the card when the item is popped
(``actions.resolve_top_of_stack``). :class:`CounterSpell` additionally
records ``StackItem.countered_destination`` so Memory Lapse (top of
library), Remand (hand), and Force of Negation / No More Lies (exile)
send the card to the printed zone instead of the graveyard.

Targeting mirrors the legacy :class:`CounterTargetSpell`: targets are
``("stack", index)`` pairs over spell items (abilities excluded) that
aren't already countered and can be countered (CR 101.2), filtered by
``spell_filter`` (an ``ObjectPredicate`` tested against the spell's
card). Legality is re-checked on resolution (CR 608.2b).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import SpellDestination, plain
from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.effects.object_predicates import ObjectPredicate
    from mtgz_ir.game import GameState, StackItem


COUNTER_DESTINATIONS = ("graveyard", "exile", "library_top", "hand")


def _is_uncounterable(state: "GameState", item: "StackItem") -> bool:
    from mtgz_ir.effects.generic.stack_control import CounterTargetSpell

    return CounterTargetSpell._is_uncounterable(state, item)


def spell_mana_value(item: "StackItem") -> int:
    """CR 202.3e: a spell's mana value on the stack counts X as the
    chosen value."""
    cost = item.source.printing.mana_cost
    return cost.total + cost.x_pips * item.x_value


@dataclass(frozen=True)
class CounterSpell(Effect):
    """Counter target spell matching ``spell_filter`` (``None`` = any
    spell).

    ``unless_pays`` ("{3}") gives the spell's controller the option to
    pay that cost on resolution; if they can and do (``choose_yes_no``),
    the spell isn't countered. ``only_if`` is a resolution-time
    condition on the targeted spell ("if it's blue") — the target is
    legal regardless, but the counter only happens when it holds.
    ``destination`` is where the countered card goes.

    ``if_paid`` is the "If they do, <effect>" rider of an ``unless_pays``
    counter (Divert Disaster: "If they do, you create a Lander token."),
    resolved for this spell's controller when the tax is paid.
    ``if_mana_spent_below_mv`` is "If the amount of mana spent to cast
    that spell was less than its mana value, <effect>" (Unravel): the
    targeted spell's ``mana_spent_to_cast`` against its mana value on the
    stack (CR 202.3e), read after the counter (a spell that can't be
    countered still answers it).
    """

    spell_filter: "ObjectPredicate | None" = None
    unless_pays: str = ""
    only_if: "ObjectPredicate | None" = None
    destination: SpellDestination = SpellDestination.GRAVEYARD
    label: str = "spell"
    if_paid: "Effect | None" = None
    if_mana_spent_below_mv: "Effect | None" = None

    def __post_init__(self) -> None:
        if self.destination not in COUNTER_DESTINATIONS:
            raise ValueError(f"unknown counter destination: {self.destination!r}")
        object.__setattr__(self, "destination", SpellDestination(self.destination))

    def requires_target(self) -> bool:
        return True

    def _matches(self, state: "GameState", item: "StackItem") -> bool:
        if self.spell_filter is None:
            return True
        return self.spell_filter.test(item.source, state)

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        out: list[Target] = []
        for i, item in enumerate(state.stack):
            if item.ability is not None or item.countered:
                continue
            if _is_uncounterable(state, item):
                continue
            if not self._matches(state, item):
                continue
            out.append(("stack", i))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import auto_tap_for_cost, can_pay_cost
        from mtgz_ir.cards import parse_mana_cost

        if not targets:
            return
        kind, idx = targets[0]
        if kind != "stack" or not (0 <= idx < len(state.stack)):
            return
        item = state.stack[idx]
        name = item.source.printing.name
        if item.ability is not None or item.countered:
            return
        if not self._matches(state, item):
            state.log.append(
                f"counterspell fizzles: {name} is no longer a legal target"
            )
            return
        if _is_uncounterable(state, item):
            state.log.append(f"counterspell fizzles: {name} can't be countered")
            self._mana_spent_rider(state, controller_id, item)
            return
        if self.only_if is not None and not self.only_if.test(item.source, state):
            state.log.append(f"{name} doesn't meet the condition — not countered")
            return
        if self.unless_pays:
            payer_id = item.controller_id
            payer = state.player_by_id(payer_id)
            cost = parse_mana_cost(self.unless_pays)
            if can_pay_cost(state, payer_id, cost) and (
                payer.agent is None or payer.agent.choose_yes_no(
                    state, payer_id,
                    f"pay {self.unless_pays} so {name} isn't countered",
                )
            ):
                auto_tap_for_cost(state, payer_id, cost)
                state.log.append(
                    f"P{payer_id} pays {self.unless_pays}; {name} "
                    f"isn't countered"
                )
                if self.if_paid is not None:
                    self.if_paid.resolve(state, controller_id, ())
                return
        item.countered = True
        item.countered_destination = plain(self.destination)
        state.log.append(f"counterspell: {name} is countered")
        self._mana_spent_rider(state, controller_id, item)

    def _mana_spent_rider(
        self, state: "GameState", controller_id: int, item: "StackItem",
    ) -> None:
        if self.if_mana_spent_below_mv is None:
            return
        spent = getattr(item.source, "mana_spent_to_cast", 0)
        if spent < spell_mana_value(item):
            self.if_mana_spent_below_mv.resolve(state, controller_id, ())

    def describe(self) -> str:
        text = f"counter target {self.label}"
        if self.unless_pays:
            text += f" unless its controller pays {self.unless_pays}"
        if self.destination != "graveyard":
            text += f"; it goes to {self.destination.replace('_', ' ')} instead"
        if self.if_paid is not None:
            text += f". If they do, {self.if_paid.describe()}"
        if self.if_mana_spent_below_mv is not None:
            text += (
                ". If the amount of mana spent to cast that spell was less "
                f"than its mana value, {self.if_mana_spent_below_mv.describe()}"
            )
        return text


__all__ = [
    "COUNTER_DESTINATIONS",
    "CounterSpell",
    "spell_mana_value",
]
