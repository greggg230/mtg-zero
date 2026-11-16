"""Shared fixtures for the PowerLSV cube artifact-bucket tests.

``cube_card`` returns the raw Scryfall record from
``data/cube_power_lsv.json``; ``build`` runs it through the same
builder chain ``OracleParserCardSource`` uses; ``arena`` lays out a
two-player main-phase state with scripted agents.
"""

from __future__ import annotations

import json
import random
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path

from mtgz_ir.agents import RandomAgent
from mtgz_ir.cards.base import CardPrinting
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.creature_builder import _build_creature_printing
from mtgz_ir.cards.parser.enchantment_builder import (
    _build_enchantment_printing,
)
from mtgz_ir.cards.parser.equipment_builder import _build_equipment_printing
from mtgz_ir.cards.parser.land_builder import _build_land_printing
from mtgz_ir.cards.parser.spacecraft_builder import _build_spacecraft_printing
from mtgz_ir.cards.parser.spell_builder import _build_spell_printing
from mtgz_ir.game import GameState, Player, Zone
from mtgz_ir.turn import Step

_DATA = (Path(__file__).resolve().parents[2] / "ir") / "data" / "cube_power_lsv.json"


@lru_cache(maxsize=1)
def _cube_index() -> dict[str, dict]:
    with _DATA.open(encoding="utf-8") as fh:
        return {c["name"]: c for c in json.load(fh)}


def cube_card(name: str) -> dict:
    return _cube_index()[name]


_BUILDERS = (
    _build_enchantment_printing,
    _build_creature_printing,
    _build_spell_printing,
    _build_land_printing,
    _build_equipment_printing,
    _build_spacecraft_printing,
    _build_artifact_printing,
)


def build(name: str) -> CardPrinting | None:
    card = cube_card(name)
    for builder in _BUILDERS:
        built = builder(card)
        if built is not None:
            return built
    return None


def build_ok(name: str) -> CardPrinting:
    printing = build(name)
    assert printing is not None, f"{name} should be claimed by the parser"
    return printing


@dataclass
class ScriptedAgent(RandomAgent):
    """RandomAgent with deterministic, overridable answers."""

    colors: list[str] = field(default_factory=list)
    yes: bool = True
    x_value: int | None = None
    hand_pick: object = "first"
    library_pick: object = "first"
    targets: list = field(default_factory=list)
    sacrifices: list[int] = field(default_factory=list)
    discards: list[int] = field(default_factory=list)

    def __post_init__(self) -> None:
        self.rng = random.Random(0)

    def choose_mana_color(self, state, player_id, options):
        if self.colors:
            c = self.colors.pop(0)
            if c in options:
                return c
        return options[0]

    def choose_yes_no(self, state, player_id, prompt):
        return self.yes

    def choose_x(self, state, player_id, card_name, upper_bound):
        if self.x_value is None:
            return upper_bound
        return min(self.x_value, upper_bound)

    def choose_card_from_hand(self, state, player_id, option_ids):
        if not option_ids or self.hand_pick is None:
            return None
        if self.hand_pick == "first":
            return option_ids[0]
        return self.hand_pick if self.hand_pick in option_ids else None

    def choose_card_from_library(self, state, player_id, option_ids):
        if not option_ids or self.library_pick is None:
            return None
        if self.library_pick == "first":
            return option_ids[0]
        return self.library_pick if self.library_pick in option_ids else None

    def choose_target(self, state, player_id, legal_targets):
        while self.targets:
            t = self.targets.pop(0)
            if t in legal_targets:
                return t
        return legal_targets[0] if legal_targets else None

    def choose_sacrifice(self, state, player_id, legal_object_ids):
        while self.sacrifices:
            s = self.sacrifices.pop(0)
            if s in legal_object_ids:
                return s
        return legal_object_ids[0]

    def choose_discard(self, state, player_id, hand_card_ids):
        while self.discards:
            d = self.discards.pop(0)
            if d in hand_card_ids:
                return d
        return hand_card_ids[0]


def arena(active: int = 0, seed: int = 0) -> GameState:
    state = GameState()
    state.players.append(Player(id=0, agent=ScriptedAgent()))
    state.players.append(Player(id=1, agent=ScriptedAgent()))
    state.active_player_idx = active
    state.priority_player_idx = active
    state.step = Step.PRECOMBAT_MAIN
    state.rng = random.Random(seed)
    return state


def put(state: GameState, printing: CardPrinting, controller: int = 0,
        *, tapped: bool = False, sick: bool = False):
    obj = state.new_object(printing, owner_id=controller, zone=Zone.BATTLEFIELD)
    obj.controller_id = controller
    obj.summoning_sick = sick
    obj.tapped = tapped
    state.battlefield.append(obj)
    return obj


def to_hand(state: GameState, printing: CardPrinting, owner: int = 0):
    obj = state.new_object(printing, owner_id=owner, zone=Zone.HAND)
    state.players[owner].hand.append(obj)
    return obj


def to_library(state: GameState, printing: CardPrinting, owner: int = 0):
    obj = state.new_object(printing, owner_id=owner, zone=Zone.LIBRARY)
    state.players[owner].library.append(obj)
    return obj


def to_graveyard(state: GameState, printing: CardPrinting, owner: int = 0):
    obj = state.new_object(printing, owner_id=owner, zone=Zone.GRAVEYARD)
    state.players[owner].graveyard.append(obj)
    return obj


def agent(state: GameState, player_id: int = 0) -> ScriptedAgent:
    return state.players[player_id].agent


def pool_colors(state: GameState, player_id: int = 0) -> list[str]:
    return sorted(c.color for c in state.players[player_id].mana_pool)
