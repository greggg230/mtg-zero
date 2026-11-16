"""Shared fixtures for the cube spells-B engine tests (not collected).

``ScriptedAgent`` answers every agent hook from per-hook FIFO scripts,
falling back to a deterministic default (first option / yes / the upper
bound) when a script runs dry. ``make_state`` builds a two-player game
in player 0's precombat main phase with player 0 holding priority.
"""

from __future__ import annotations

from mtgz_ir.actions import CastSpell, apply_action, resolve_top_of_stack
from mtgz_ir.agents import Agent
from mtgz_ir.cards import BASIC_LANDS
from mtgz_ir.cards.source import get_active_source
from mtgz_ir.game import GameState, Player, Zone
from mtgz_ir.turn import Step

PRINTINGS = get_active_source().get_printings()


def printing(name: str):
    if name in BASIC_LANDS:
        return BASIC_LANDS[name]
    return PRINTINGS[name]


class ScriptedAgent(Agent):
    """Agent whose answers come from FIFO lists keyed by hook name."""

    name = "scripted"

    def __init__(self, **scripts):
        self.scripts = {k: list(v) for k, v in scripts.items()}
        self.calls: list[tuple[str, object]] = []

    def _next(self, hook, default, options=None):
        self.calls.append((hook, options))
        queue = self.scripts.get(hook)
        if queue:
            return queue.pop(0)
        return default

    def choose_action(self, state, player_id, legal):  # pragma: no cover
        return legal[0]

    def declare_attackers(self, state, player_id, eligible):  # pragma: no cover
        return []

    def declare_blockers(self, state, player_id, eligible, attackers):  # pragma: no cover
        return {}

    def choose_target(self, state, player_id, legal_targets):
        return self._next("target", legal_targets[0] if legal_targets else None, legal_targets)

    def choose_discard(self, state, player_id, hand_card_ids):
        return self._next("discard", hand_card_ids[0], hand_card_ids)

    def choose_surveil(self, state, player_id, top_card_id):
        return self._next("surveil", False, top_card_id)

    def choose_sacrifice(self, state, player_id, legal_object_ids):
        return self._next("sacrifice", legal_object_ids[0], legal_object_ids)

    def choose_mode(self, state, player_id, mode_descriptions):
        return self._next("mode", 0, mode_descriptions)

    def choose_modes(self, state, player_id, mode_descriptions):
        return self._next("modes", [0], mode_descriptions)

    def choose_pay_shock_life(self, state, player_id, life_cost):  # pragma: no cover
        return False

    def choose_card_from_opponent_hand(self, state, player_id, opponent_hand_ids):
        return self._next("opp_hand", opponent_hand_ids[0], opponent_hand_ids)

    def choose_card_from_library(self, state, player_id, option_ids):
        return self._next("library", option_ids[0] if option_ids else None, option_ids)

    def choose_card_from_hand(self, state, player_id, option_ids):
        return self._next("hand", option_ids[0] if option_ids else None, option_ids)

    def choose_x(self, state, player_id, card_name, upper_bound):
        return self._next("x", upper_bound, (card_name, upper_bound))

    def choose_devour_lands(self, state, player_id, land_ids):  # pragma: no cover
        return []

    def choose_mana_color(self, state, player_id, options):
        return self._next("color", options[0], options)

    def choose_yes_no(self, state, player_id, prompt):
        return self._next("yes_no", True, prompt)


def make_state(agent0=None, agent1=None, step=Step.PRECOMBAT_MAIN) -> GameState:
    state = GameState()
    state.players.append(Player(id=0, agent=agent0 or ScriptedAgent()))
    state.players.append(Player(id=1, agent=agent1 or ScriptedAgent()))
    state.step = step
    return state


def add(state: GameState, pid: int, name: str, zone: Zone):
    """Create a card ``name`` owned by ``pid`` in ``zone``."""
    obj = state.new_object(printing(name), owner_id=pid, zone=zone)
    p = state.player_by_id(pid)
    if zone == Zone.BATTLEFIELD:
        obj.summoning_sick = False
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
    return [add(state, pid, name, Zone.BATTLEFIELD) for _ in range(n)]


def cast_and_resolve(state: GameState, pid: int, card, **kwargs) -> None:
    """Cast ``card`` through the real pipeline and resolve the stack."""
    apply_action(state, pid, CastSpell(card.id, **kwargs))
    while state.stack:
        resolve_top_of_stack(state)
