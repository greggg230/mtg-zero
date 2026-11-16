"""Evoke (CR 702.74) — cube W/G creature bucket.

"Evoke [cost]" means "You may cast this spell by paying [cost] rather
than paying its mana cost" and "When this permanent enters, if its
evoke cost was paid, its controller sacrifices it."

The printed cost is either mana (``CardPrinting.evoke_cost``) or the
Modern Horizons 2 Incarnation form "Exile a <filter> card from your
hand" (``CardPrinting.evoke_exile_from_hand``). The cast pipeline
(``actions._cast_spell(use_evoke=True)``) pays it, flags the spell
object ``evoked`` and the printing's :data:`EVOKE_SACRIFICE_TRIGGER`
sacrifices the permanent as it enters. ``legal.legal_actions`` offers
``CastSpell(id, use_evoke=True)`` through :func:`evoke_castable`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target, TriggeredAbility
from mtgz_ir.effects.object_predicates import ObjectPredicate
from mtgz_ir.effects.predicates import Event, IsSelf, Source

if TYPE_CHECKING:
    from mtgz_ir.cards import CardPrinting, ManaCost
    from mtgz_ir.game import GameObject, GameState


def has_evoke(printing: "CardPrinting") -> bool:
    return (
        printing.evoke_cost is not None
        or printing.evoke_exile_from_hand is not None
    )


def evoke_mana_cost(printing: "CardPrinting") -> "ManaCost":
    """The mana part of the evoke cost (zero for the exile-a-card form)."""
    from mtgz_ir.cards import ManaCost

    if printing.evoke_cost is not None:
        return printing.evoke_cost
    return ManaCost(generic=0, colored=(), x_pips=0)


def evoke_pitch_options(
    state: "GameState", player_id: int, card: "GameObject",
) -> list["GameObject"]:
    """Other hand cards that can pay an "Exile a <filter> card from your
    hand" evoke cost (empty for a mana evoke cost)."""
    f = card.printing.evoke_exile_from_hand
    if f is None:
        return []
    p = state.player_by_id(player_id)
    return [c for c in p.hand if c.id != card.id and f.matches_card(state, c)]


def evoke_castable(
    state: "GameState", player_id: int, card: "GameObject", can_pay,
) -> bool:
    """Whether ``card`` (in hand) can be cast for its evoke cost now.

    ``can_pay(cost)`` is the caller's mana-affordability probe (it
    applies cost increases / reductions); timing and targets are the
    caller's concern."""
    pr = card.printing
    if not has_evoke(pr):
        return False
    if pr.evoke_exile_from_hand is not None and not evoke_pitch_options(
        state, player_id, card,
    ):
        return False
    return can_pay(evoke_mana_cost(pr))


def pay_evoke_exile(
    state: "GameState", player_id: int, card: "GameObject",
) -> None:
    """Exile the card paying an "Exile a <filter> card from your hand"
    evoke cost, picked through ``choose_discard`` (the "which hand card
    do I give up" decision, as for pitch spells). No-op for a mana
    evoke cost."""
    if card.printing.evoke_exile_from_hand is None:
        return
    from mtgz_ir.game import Zone, reveal_to_all

    options = evoke_pitch_options(state, player_id, card)
    if not options:  # pragma: no cover - legal_actions gates
        raise ValueError(f"no card to exile for {card.printing.name}'s evoke")
    p = state.player_by_id(player_id)
    chosen = options[0]
    if p.agent is not None:
        pick = p.agent.choose_discard(state, player_id, [c.id for c in options])
        chosen = next((c for c in options if c.id == pick), options[0])
    p.hand.remove(chosen)
    chosen.zone = Zone.EXILE
    p.exile.append(chosen)
    reveal_to_all(state, chosen)
    state.log.append(
        f"P{player_id} exiles {chosen.printing.name} to evoke "
        f"{card.printing.name}"
    )


@dataclass(frozen=True)
class WasEvoked(ObjectPredicate):
    """The object was cast for its evoke cost (``GameObject.evoked``)."""

    def test(self, obj, state):
        return bool(getattr(obj, "evoked", False))


@dataclass(frozen=True)
class SacrificeSelfIfEvoked(Effect):
    """The evoke trigger's effect: sacrifice the source if it is still
    on the battlefield and still the evoked object (the intervening-if
    is rechecked on resolution, CR 603.4 — a flickered evoked creature
    is a new object and stays)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        if not source.evoked:
            return
        state.log.append(f"{source.printing.name} is sacrificed (evoke)")
        sacrifice(state, source)

    def describe(self) -> str:
        return "if its evoke cost was paid, sacrifice it"


EVOKE_SACRIFICE_TRIGGER = TriggeredAbility(
    event=Event.ENTERS,
    predicate=IsSelf() & Source(WasEvoked()),
    effect=SacrificeSelfIfEvoked(),
)
