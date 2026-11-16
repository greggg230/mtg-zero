"""Canonical rules-engine interface.

Two entry points cover everything a caller (test, agent, or UI) needs:

- ``legal_moves(state, player_id=None)`` — enumerate the legal priority-level
  actions available to ``player_id`` (defaulting to whoever has priority right
  now). Priority-level means pass-priority, play-land, and cast-spell actions.
- ``apply(state, action, player_id=None)`` — transform ``state`` by applying
  ``action``. Mutates in place; returns the same state so callers can chain.
  Handles every ``Action`` subclass, including the combat-declaration actions
  (``DeclareAttackers``, ``DeclareBlockers``) that aren't enumerable as legal
  moves because their decision space is combinatorial.

For combat declarations, construct the action directly with whichever
attacker/blocker ids you want, then apply it — ``apply`` silently filters out
ineligible ids (summoning-sick attackers, blockers that can't block a flier,
etc.). The helpers ``mtgz_ir.combat.eligible_attackers`` /
``mtgz_ir.combat.eligible_blockers`` are useful for telling you *which* ids are
valid in the current state.
"""

from __future__ import annotations

from typing import Optional

from mtgz_ir.actions import Action, apply_action
from mtgz_ir.game import GameState
from mtgz_ir.legal import legal_actions


def legal_moves(state: GameState, player_id: Optional[int] = None) -> list[Action]:
    """Priority-level legal actions for ``player_id`` (default: priority holder)."""
    if player_id is None:
        player_id = state.players[state.priority_player_idx].id
    return legal_actions(state, player_id)


def apply(
    state: GameState, action: Action, player_id: Optional[int] = None
) -> GameState:
    """Apply ``action`` to ``state`` in place. Returns ``state`` for chaining."""
    if player_id is None:
        player_id = state.players[state.priority_player_idx].id
    apply_action(state, player_id, action)
    return state
