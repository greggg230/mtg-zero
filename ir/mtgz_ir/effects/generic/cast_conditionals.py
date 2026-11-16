"""Resolution-time spell conditionals: "... instead if <condition>" /
"If <condition>, <effect>" riders whose condition is a fact about how
the spell was cast or about the game state.

- :class:`SpellWasKicked` — "If this spell was kicked" (reads
  ``state.resolving_kicked``).
- :class:`AlternativeCostWasPaid` — "If the {1}{B} cost was paid"
  (Baleful Mastery; reads ``state.resolving_alt_cost_paid``).
- :class:`PermanentLeftYourControlThisTurn` — Revolt: "if a permanent
  left the battlefield under your control this turn" (reads
  ``Player.permanent_left_battlefield_this_turn``).
- Delirium reuses :class:`mtgz_ir.effects.conditions.DeliriumActive`.

They are consumed by the shared
:class:`mtgz_ir.effects.combinators.IfConditionElse` (which also takes
``StatePredicate`` conditions such as Threshold's
``CardsInGraveyardAtLeast``).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.conditions import Condition

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class SpellWasKicked(Condition):
    def matches(self, state, viewer_id, chosen_targets=None):
        return bool(state.resolving_kicked)


@dataclass(frozen=True)
class AlternativeCostWasPaid(Condition):
    def matches(self, state, viewer_id, chosen_targets=None):
        return bool(state.resolving_alt_cost_paid)


@dataclass(frozen=True)
class OptionalAdditionalCostPaid(Condition):
    """"If this spell was cast using teamwork" (the resolving spell's
    optional additional cost was paid)."""

    def matches(self, state, viewer_id, chosen_targets=None):
        spell = state.resolving_spell
        return bool(spell is not None and spell.optional_additional_cost_paid)


@dataclass(frozen=True)
class PermanentLeftYourControlThisTurn(Condition):
    """Revolt (CR 207.2c ability word): a permanent left the
    battlefield under the viewer's control this turn."""

    def matches(self, state, viewer_id, chosen_targets=None):
        return bool(
            state.player_by_id(viewer_id).permanent_left_battlefield_this_turn
        )


@dataclass(frozen=True)
class AnOpponentDrawsCards(Effect):
    """"An opponent draws N cards." — the controller chooses an
    opponent (``choose_target`` over the living opponents; with one
    opponent there is no choice)."""

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        opponents = [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]
        if not opponents:
            return
        chosen = opponents[0]
        agent = state.player_by_id(controller_id).agent
        if len(opponents) > 1 and agent is not None:
            pick = agent.choose_target(state, controller_id, opponents)
            if pick in opponents:
                chosen = pick
        draw_n_cards(state, chosen[1], self.amount)
        state.log.append(f"P{chosen[1]} draws {self.amount} card(s)")

    def describe(self) -> str:
        return f"an opponent draws {self.amount} card(s)"


__all__ = [
    "AlternativeCostWasPaid",
    "AnOpponentDrawsCards",
    "OptionalAdditionalCostPaid",
    "PermanentLeftYourControlThisTurn",
    "SpellWasKicked",
]
