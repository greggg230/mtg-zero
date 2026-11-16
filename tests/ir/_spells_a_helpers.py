"""Shared fixtures for the PowerLSV cube "spells A" (removal, counters,
burn, tricks) engine tests: a deterministic scripted agent plus small
state builders that cast real parsed cube printings through the
engine's ``apply_action`` / ``resolve_top_of_stack`` pipeline."""

from __future__ import annotations

from functools import lru_cache

from mtgz_ir.actions import CastSpell, apply_action, resolve_top_of_stack
from mtgz_ir.agents import Agent
from mtgz_ir.cards import BASIC_LANDS
from mtgz_ir.cards.base import CardPrinting
from mtgz_ir.game import GameObject, GameState, Player, Zone
from mtgz_ir.turn import Step, state_based_actions


@lru_cache(maxsize=1)
def cube_printings() -> dict[str, CardPrinting]:
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    return OracleParserCardSource(pool="cube").get_printings()


def cube(name: str) -> CardPrinting:
    printing = cube_printings().get(name)
    assert printing is not None, f"{name} is not claimed by the parser"
    return printing


class ScriptedAgent(Agent):
    """Deterministic agent: first legal option unless told otherwise.

    ``yes`` answers every ``choose_yes_no`` (a list is consumed in
    order). ``prefer`` is a list of object ids / target tuples tried in
    order by the target / sacrifice / card pickers. ``mode`` /
    ``modes`` drive modal picks; ``x`` caps ``choose_x``.
    """

    name = "scripted"

    def __init__(
        self, *, yes=True, prefer=(), mode=0, modes=None, x=None,
    ) -> None:
        self.yes = yes
        self.prefer = list(prefer)
        self.mode = mode
        self.modes = modes
        self.x = x
        self.prompts: list[str] = []

    def _pick(self, options):
        for want in self.prefer:
            if want in options:
                return want
        return options[0] if options else None

    def choose_action(self, state, player_id, legal):  # pragma: no cover
        return legal[0]

    def declare_attackers(self, state, player_id, eligible):  # pragma: no cover
        return []

    def declare_blockers(self, state, player_id, eligible, attackers):  # pragma: no cover
        return {}

    def choose_target(self, state, player_id, legal_targets):
        if not legal_targets:
            return None
        for want in self.prefer:
            for t in legal_targets:
                if t == want or (isinstance(want, int) and t[1] == want):
                    return t
        return legal_targets[0]

    def choose_discard(self, state, player_id, hand_card_ids):
        return self._pick(hand_card_ids)

    def choose_sacrifice(self, state, player_id, legal_object_ids):
        return self._pick(legal_object_ids)

    def choose_card_from_hand(self, state, player_id, option_ids):
        return self._pick(option_ids)

    def choose_card_from_library(self, state, player_id, option_ids):
        return self._pick(option_ids)

    def choose_card_from_opponent_hand(self, state, player_id, ids):
        return self._pick(ids)

    def choose_mode(self, state, player_id, mode_descriptions):
        return self.mode

    def choose_modes(self, state, player_id, mode_descriptions):
        return list(self.modes) if self.modes is not None else [self.mode]

    def choose_x(self, state, player_id, card_name, upper_bound):
        return upper_bound if self.x is None else min(self.x, upper_bound)

    def choose_yes_no(self, state, player_id, prompt):
        self.prompts.append(prompt)
        if isinstance(self.yes, list):
            return self.yes.pop(0) if self.yes else True
        return self.yes

    def choose_surveil(self, state, player_id, top_card_id):  # pragma: no cover
        return False

    def choose_pay_shock_life(self, state, player_id, life_cost):  # pragma: no cover
        return False


def new_game(agent0: Agent | None = None, agent1: Agent | None = None) -> GameState:
    state = GameState()
    state.players.append(Player(id=0, agent=agent0 or ScriptedAgent()))
    state.players.append(Player(id=1, agent=agent1 or ScriptedAgent()))
    state.active_player_idx = 0
    state.priority_player_idx = 0
    state.step = Step.PRECOMBAT_MAIN
    return state


def put(state: GameState, printing: CardPrinting | str, controller: int = 0) -> GameObject:
    """Put a permanent straight onto the battlefield (no ETB)."""
    if isinstance(printing, str):
        printing = BASIC_LANDS.get(printing) or cube(printing)
    obj = state.new_object(printing, controller, Zone.BATTLEFIELD)
    obj.summoning_sick = False
    if printing.loyalty is not None:
        obj.loyalty_counters = printing.loyalty
    state.battlefield.append(obj)
    return obj


def lands(state: GameState, controller: int, name: str, n: int) -> list[GameObject]:
    return [put(state, BASIC_LANDS[name], controller) for _ in range(n)]


def to_hand(state: GameState, printing: CardPrinting | str, owner: int = 0) -> GameObject:
    if isinstance(printing, str):
        printing = BASIC_LANDS.get(printing) or cube(printing)
    obj = state.new_object(printing, owner, Zone.HAND)
    state.player_by_id(owner).hand.append(obj)
    return obj


def to_library(state: GameState, printing: CardPrinting | str, owner: int = 0) -> GameObject:
    if isinstance(printing, str):
        printing = BASIC_LANDS.get(printing) or cube(printing)
    obj = state.new_object(printing, owner, Zone.LIBRARY)
    state.player_by_id(owner).library.append(obj)
    return obj


def cast(state: GameState, player_id: int, card: GameObject, **kwargs) -> None:
    apply_action(state, player_id, CastSpell(card.id, **kwargs))


def resolve(state: GameState) -> None:
    resolve_top_of_stack(state)
    state_based_actions(state)


def cast_and_resolve(state: GameState, player_id: int, card: GameObject, **kwargs) -> None:
    cast(state, player_id, card, **kwargs)
    resolve(state)
