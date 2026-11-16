"""Shared fixtures for the cube creatures (R / multicolor / colorless)
bucket tests (not collected)."""

from __future__ import annotations

import functools

from mtgz_ir.actions import (
    ActivateAbility,
    CastSpell,
    apply_action,
    resolve_top_of_stack,
)
from mtgz_ir.cards import BASIC_LANDS
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.game import GameState, Player, Zone
from mtgz_ir.legal import legal_actions
from mtgz_ir.turn import Step
from _spells_b_helpers import ScriptedAgent as _BaseScriptedAgent


@functools.lru_cache(maxsize=1)
def cube_printings() -> dict:
    return OracleParserCardSource(pool="cube").get_printings()


def printing(name: str):
    if name in BASIC_LANDS:
        return BASIC_LANDS[name]
    return cube_printings()[name]


class ScriptedAgent(_BaseScriptedAgent):
    """The spells-B scripted agent plus the tap-a-permanent and
    declare hooks."""

    def choose_permanent_to_tap(self, state, player_id, legal_object_ids):
        return self._next(
            "tap", legal_object_ids[0] if legal_object_ids else None,
            legal_object_ids,
        )

    def declare_attackers(self, state, player_id, eligible):
        return self._next("attackers", [], eligible)

    def declare_blockers(self, state, player_id, eligible, attackers):
        return self._next("blockers", {}, eligible)

    def choose_card_from_opponent_hand(self, state, player_id, opponent_hand_ids):
        return self._next("opp_hand", opponent_hand_ids[0], opponent_hand_ids)


def make_state(step=Step.PRECOMBAT_MAIN, agent0=None, agent1=None) -> GameState:
    state = GameState()
    state.players.append(Player(id=0, agent=agent0 or ScriptedAgent()))
    state.players.append(Player(id=1, agent=agent1 or ScriptedAgent()))
    state.step = step
    return state


def add(state: GameState, pid: int, name: str, zone: Zone = Zone.BATTLEFIELD, *, sick=False):
    obj = state.new_object(printing(name), owner_id=pid, zone=zone)
    p = state.player_by_id(pid)
    if zone == Zone.BATTLEFIELD:
        obj.summoning_sick = sick
        state.battlefield.append(obj)
    elif zone == Zone.HAND:
        p.hand.append(obj)
    elif zone == Zone.LIBRARY:
        p.library.append(obj)
    elif zone == Zone.GRAVEYARD:
        p.graveyard.append(obj)
    elif zone == Zone.EXILE:
        p.exile.append(obj)
    return obj


def add_lands(state: GameState, pid: int, name: str, n: int) -> list:
    return [add(state, pid, name) for _ in range(n)]


def resolve_all(state: GameState) -> None:
    """Resolve the whole stack, running state-based actions after each
    resolution (as the priority loop does)."""
    from mtgz_ir.turn import state_based_actions

    state_based_actions(state)
    while state.stack:
        resolve_top_of_stack(state)
        state_based_actions(state)


def cast(state: GameState, pid: int, card, resolve=True, **kwargs) -> None:
    action = CastSpell(card.id, **kwargs)
    assert action in legal_actions(state, pid), (action, legal_actions(state, pid))
    apply_action(state, pid, action)
    if resolve:
        resolve_all(state)


def activate(state: GameState, pid: int, source, idx: int = 0, resolve=True) -> None:
    action = ActivateAbility(source_id=source.id, ability_index=idx)
    assert action in legal_actions(state, pid), (action, legal_actions(state, pid))
    apply_action(state, pid, action)
    if resolve:
        resolve_all(state)


def can_activate(state: GameState, pid: int, source, idx: int = 0) -> bool:
    return ActivateAbility(source_id=source.id, ability_index=idx) in legal_actions(state, pid)
