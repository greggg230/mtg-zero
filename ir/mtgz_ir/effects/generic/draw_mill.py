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
class EachOpponentMillsCards(Effect):
    """Each opponent puts the top ``amount`` cards of their library into
    their graveyard. Used as the 2-player simplification of "defending
    player mills N" attack triggers (Specimen Freighter).
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            milled = 0
            for _ in range(self.amount):
                if not p.library:
                    break
                card = p.library.pop(0)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
                milled += 1
            state.log.append(f"P{p.id} mills {milled} cards")

    def describe(self) -> str:
        return f"defending player mills {self.amount} cards"



@dataclass(frozen=True)
class DrawCardsEqualToResolvingCounterAmount(Effect):
    """Draw a number of cards equal to
    ``state.resolving_counter_amount``. Matches Terrasymbiosis's "draw
    that many cards" observer. No-op if amount is 0.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        amount = state.resolving_counter_amount
        if amount <= 0:
            return
        drawn = draw_n_cards(state, controller_id, amount)
        state.log.append(
            f"P{controller_id} draws {drawn} (counter-added trigger)"
        )

    def describe(self) -> str:
        return "you may draw that many cards"



@dataclass(frozen=True)
class DrawCardsEqualToResolvingCombatDamage(Effect):
    """Draw a number of cards equal to
    ``state.resolving_combat_damage_amount``. Matches Starwinder's
    "you may draw that many cards" observer trigger (the "may" is
    simplified to always yes, consistent with other OptionalXyz
    simplifications in this engine).

    No-op if the resolving amount is 0 (e.g. accidentally invoked
    outside a combat-damage trigger).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        amount = state.resolving_combat_damage_amount
        if amount <= 0:
            return
        drawn = draw_n_cards(state, controller_id, amount)
        state.log.append(
            f"P{controller_id} draws {drawn} cards (combat-damage trigger)"
        )

    def describe(self) -> str:
        return "you may draw that many cards"



@dataclass(frozen=True)
class DrawCardPerMulticoloredPermanent(Effect):
    """Draw one card for each multicolored permanent you control.
    Matches Infinite Guideline Station's attack trigger."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        n = _multicolored_permanents_count(state, controller_id)
        p = state.player_by_id(controller_id)
        drew = 0
        for _ in range(n):
            if not p.library:
                break
            card = p.library.pop(0)
            card.zone = Zone.HAND
            p.hand.append(card)
            drew += 1
        state.log.append(
            f"P{controller_id} draws {drew} cards "
            f"(multicolored-permanent count)"
        )

    def describe(self) -> str:
        return "draw a card for each multicolored permanent you control"



@dataclass(frozen=True)
class MillTargetPlayer(Effect):
    """Target player puts the top N cards of their library into their
    graveyard.

    If ``use_controller_life`` is True, N is the controller's current life
    total (minimum 0) instead of ``amount`` — matches Space-Time Anomaly.
    Running out of cards mid-mill simply stops; empty-library loss is
    handled separately by the draw step.
    """

    amount: int = 1
    use_controller_life: bool = False

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players if not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        target = state.player_by_id(target_id)
        if self.use_controller_life:
            n = max(0, state.player_by_id(controller_id).life)
        else:
            n = self.amount
        milled = 0
        for _ in range(n):
            if not target.library:
                break
            card = target.library.pop(0)
            card.zone = Zone.GRAVEYARD
            target.graveyard.append(card)
            milled += 1
        state.log.append(f"P{target_id} mills {milled} cards")

    def describe(self) -> str:
        if self.use_controller_life:
            return (
                "target player mills cards equal to your life total"
            )
        return f"target player mills {self.amount} cards"



@dataclass(frozen=True)
class DrawCards(Effect):
    amount: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        drawn = draw_n_cards(state, controller_id, self.amount)
        state.log.append(f"P{controller_id} draws {drawn} card(s)")

    def describe(self) -> str:
        if self.amount == 1:
            return "you draw a card"
        return f"you draw {self.amount} cards"



@dataclass(frozen=True)
class EachPlayerDraws(Effect):
    amount: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        for i in range(len(state.players)):
            idx = (state.active_player_idx + i) % len(state.players)
            p = state.players[idx]
            if p.has_lost:
                continue
            draw_n_cards(state, p.id, self.amount)
        state.log.append(
            f"each player draws {self.amount} card(s) "
            f"(from {state.players[controller_id].id}'s trigger)"
        )

    def describe(self) -> str:
        if self.amount == 1:
            return "each player draws a card"
        return f"each player draws {self.amount} cards"



@dataclass(frozen=True)
class DrawCardsByConditionMultiplier(Effect):
    """Draw X cards, where X is the ``multiplier`` of a structural
    ``Condition``.

    Generic primitive for the "draw X cards, where X is <count>" oracle
    shape. Reuses the existing ``Condition`` algebra (the same one
    ``CostReduction.condition`` uses): the resolve site reads
    ``condition.multiplier(state, controller_id)`` at fire time, which
    is the exact count semantics each ``Condition`` already encodes
    (``PerDifferentlyNamedLand`` counts distinctly-named lands the
    viewer controls, ``PerControlled`` counts permanents matching the
    inner ``ObjectPredicate``, etc.).

    The ``description`` argument is a short oracle-text fragment used
    by ``describe()`` so log output reads naturally — e.g. for
    ``PerDifferentlyNamedLand`` we want "draw X cards, where X is the
    number of differently named lands you control" rather than the
    generic ``Condition.__class__.__name__``. The fragment is parser-
    supplied (one per claimed phrase shape), keeping the runtime engine
    class oblivious to oracle-text wording while still producing
    readable logs.

    Replaces the bespoke ``AllFatesScrollDrawEffect`` for the
    differently-named-lands surface; reusable for any future "draw X
    where X is <Condition.multiplier>" printing without further engine
    work.
    """

    condition: "Condition"  # noqa: F821 - forward ref via TYPE_CHECKING
    description_fragment: str = "where X is determined at resolution"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        x = self.condition.multiplier(state, controller_id)
        if x <= 0:
            state.log.append(
                f"P{controller_id} draws 0 cards "
                f"({self.description_fragment})"
            )
            return
        drawn = draw_n_cards(state, controller_id, x)
        state.log.append(
            f"P{controller_id} draws {drawn} cards "
            f"({self.description_fragment})"
        )

    def describe(self) -> str:
        return f"draw X cards, {self.description_fragment}"
