"""Formula grammar — integer expressions evaluated at resolve time.

A ``Formula`` returns an ``int`` given the resolving game state and
the resolving controller. Effects that need a "where X is …" amount
that depends on runtime data (the mana value of a sacrificed
artifact, the power of a discarded creature, the number of cards in
a player's hand at resolve time, etc.) carry a ``Formula`` field
instead of a fixed ``int`` and call ``formula.value(state, controller_id)``
when they fire.

This module is deliberately small. The first concrete formula —
``MVOfCostPaidObjectPlus`` — reads the mana value of the first
GameObject in ``state.resolving_cost_paid_objects`` and adds a
constant. Future cards can compose new formulas (``PowerOfCostPaid
Object`` for "where X is the sacrificed creature's power",
``CountInZone(zone, predicate)`` for "where X is the number of
cards…", etc.) without disturbing the existing call sites.

Why a dedicated grammar rather than ``Condition.multiplier``: the
``Condition`` grammar is gated on a boolean ``matches`` and serves
cost-reduction rules; ``Formula`` is unconditional and serves
resolve-time amount calculation. Keeping them separate avoids
overloading either grammar with the other's semantics.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


class Formula(ABC):
    """A non-negative integer expression evaluated against the
    currently-resolving game state.

    Subclasses implement :meth:`value`. Effects that need a runtime
    amount (variable-X "exile X cards", "draw X cards where X is …")
    accept a ``Formula`` field and call its ``value`` at resolve time.
    """

    @abstractmethod
    def value(self, state: "GameState", controller_id: int) -> int:
        ...  # pragma: no cover


@dataclass(frozen=True)
class Constant(Formula):
    """A fixed integer. Useful as a default and in tests where a
    formula slot expects a ``Formula`` but the amount is known at
    parse time."""

    amount: int = 0

    def value(self, state: "GameState", controller_id: int) -> int:
        return self.amount


@dataclass(frozen=True)
class MVOfCostPaidObjectPlus(Formula):
    """``Mana value of the first cost-paid object, plus ``constant``.

    Reads ``state.resolving_cost_paid_objects[0]`` (the GameObject
    sacrificed / exiled / tapped as part of the resolving ability's
    cost) and returns its printing's mana value plus the configured
    constant. Memorial Vault uses ``MVOfCostPaidObjectPlus(constant=1)``
    for its "where X is one plus the mana value of the sacrificed
    artifact" body.

    If the cost-paid-objects tuple is empty (defensive — the cost
    grammar should have populated it before resolve), returns
    ``constant`` alone. The cost gate (``sacrifice_another_artifact``
    in Memorial Vault's case) guarantees at least one object is paid
    when the ability legally activates, so the empty branch only
    matters if a future call site invokes the formula without a paid
    object.
    """

    constant: int = 0

    def value(self, state: "GameState", controller_id: int) -> int:
        paid = state.resolving_cost_paid_objects
        if not paid:
            return self.constant
        return paid[0].printing.mana_cost.total + self.constant


@dataclass(frozen=True)
class PowerOfCostPaidObject(Formula):
    """The power of the first cost-paid object ("the sacrificed
    creature's power", Susur Secundi, Void Altar): read from
    ``state.resolving_cost_paid_objects[0]`` as it last existed on the
    battlefield (CR 608.2h). 0 when nothing was paid; never negative
    for a count (CR 107.1b)."""

    def value(self, state: "GameState", controller_id: int) -> int:
        paid = state.resolving_cost_paid_objects
        if not paid:
            return 0
        from mtgz_ir.game import effective_power

        return max(0, effective_power(state, paid[0]))


@dataclass(frozen=True)
class CountControlledMatching(Formula):
    """Number of permanents the controller controls that satisfy
    ``predicate``.

    Walks ``state.battlefield`` once at resolve time and counts every
    permanent whose ``controller_id`` matches the resolving controller
    AND whose object satisfies the wrapped
    :class:`mtgz_ir.effects.object_predicates.ObjectPredicate`.

    Used for "where X is the number of artifacts you control" /
    "the number of creatures you control" / "the number of <subtype>s
    you control" formulas. Composes with the predicate algebra (``&`` /
    ``|`` / ``~``) so cards layering filters (e.g. "the number of
    artifacts and creatures you control") drop in without a new formula
    class — wrap the combined predicate.
    """

    predicate: "object"  # ObjectPredicate; typed loosely to avoid an import cycle.

    def value(self, state: "GameState", controller_id: int) -> int:
        return sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id
            and self.predicate.test(o, state)
        )


@dataclass(frozen=True)
class ResolvingX(Formula):
    """The ``X`` chosen when the resolving spell / ability was put on
    the stack (``state.resolving_x_value``).

    Used by "Target player discards X cards at random" (Mind Twist) and
    "Draw X cards, then discard X cards" (Occult Epiphany) — any effect
    whose amount is the printed ``{X}`` of its own cost. Clamped at 0.
    """

    def value(self, state: "GameState", controller_id: int) -> int:
        return max(0, state.resolving_x_value)


@dataclass(frozen=True)
class Scaled(Formula):
    """``factor`` times another formula's value — "twice the number of
    lands you control" is ``Scaled(CountControlledMatching(IsLand()),
    factor=2)`` (Pillage the Bog)."""

    inner: Formula
    factor: int = 1

    def value(self, state: "GameState", controller_id: int) -> int:
        return self.factor * self.inner.value(state, controller_id)
