"""Library-search hooks for the Vintage Roto P2 package (F18: Opposition
Agent, Wan Shi Tong, Librarian, Archive Trap).

Every effect that searches a library wraps its ``resolve`` with
:func:`library_search` (the searcher is the resolving controller) and
calls :func:`searched_card_taken` for each card it finds, right after
taking it out of the library and before moving it anywhere. The hooks:

* record ``Player.library_searched_turn`` ("if an opponent searched their
  library this turn" -- Archive Trap's alternative cost);
* while an opponent of the searcher controls an Opposition Agent, swap
  the searcher's agent for one whose search picks
  (``choose_card_from_library``) are made by that opponent ("You control
  your opponents while they're searching their libraries"), and exile
  each found card with a play permission for that opponent ("they exile
  each card they find. You may play those cards for as long as they
  remain exiled, and you may spend mana as though it were mana of any
  color to cast them");
* after the search, put "whenever an opponent searches their library"
  triggers on the stack (Wan Shi Tong).

Kept free of module-level engine imports so the base search modules can
import it at load time without an import cycle.
"""

from __future__ import annotations

import functools
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Mapping

from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.predicates import TriggerPredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


@dataclass(frozen=True)
class ControlSearchingOpponents:
    """``CardPrinting.zone_abilities`` spec: "You control your opponents
    while they're searching their libraries." (Opposition Agent.)"""

    def describe(self) -> str:
        return "you control your opponents while they're searching their libraries"


@dataclass(frozen=True)
class ExileFoundCardsForYou:
    """``CardPrinting.zone_abilities`` spec: "While an opponent is
    searching their library, they exile each card they find. You may
    play those cards for as long as they remain exiled, and you may spend
    mana as though it were mana of any color to cast them." (Opposition
    Agent.)"""

    def describe(self) -> str:
        return "opponents exile the cards they find; you may play them"


class OpponentSearchedLibrary(TriggerPredicate):
    """Marker predicate of "Whenever an opponent searches their library"
    (Wan Shi Tong). Never matches a regular dispatch; :func:`library_
    search` stacks the ability after each search by an opponent."""

    def matches(self, state, subjects: Mapping[str, Any], source) -> bool:
        return False

    def __eq__(self, other):
        return isinstance(other, OpponentSearchedLibrary)

    def __hash__(self):
        return hash("OpponentSearchedLibrary")

    def __repr__(self):
        return "OpponentSearchedLibrary()"


@dataclass(frozen=True)
class OpponentSearchedLibraryThisTurn(Condition):
    """"If an opponent searched their library this turn" (Archive
    Trap)."""

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return any(
            p.id != viewer_id and p.library_searched_turn == state.turn_number
            for p in state.players
        )


def _opposing_sources(state, searcher_id: int, spec_type) -> list:
    from mtgz_ir.game import _source_layer6_wiped

    out = []
    for o in state.battlefield:
        if o.controller_id == searcher_id:
            continue
        if any(isinstance(s, spec_type) for s in o.printing.zone_abilities):
            if state.continuous_effects and _source_layer6_wiped(state, o):
                continue
            out.append(o)
    return out


class _ControlledSearchAgent:
    """The searcher's agent while an opponent controls their search:
    search picks are the controlling player's; everything else is the
    searcher's own agent."""

    def __init__(self, own, controller_agent, controller_id: int) -> None:
        self._own = own
        self._controller_agent = controller_agent
        self._controller_id = controller_id

    def choose_card_from_library(self, state, player_id, option_ids):
        return self._controller_agent.choose_card_from_library(
            state, self._controller_id, option_ids,
        )

    def __getattr__(self, name):
        return getattr(self._own, name)


def run_library_search(state, searcher_id: int, search):
    """Run ``search()`` -- ``searcher_id`` searching their own library --
    under the hooks in the module doc; returns its result."""
    searcher = state.player_by_id(searcher_id)
    searcher.library_searched_turn = state.turn_number
    own = searcher.agent
    controllers = _opposing_sources(state, searcher_id, ControlSearchingOpponents)
    if controllers and own is not None:
        ctl = state.player_by_id(controllers[-1].controller_id)
        if ctl.agent is not None:
            searcher.agent = _ControlledSearchAgent(own, ctl.agent, ctl.id)
            state.log.append(f"P{ctl.id} controls P{searcher_id} while they search")
    try:
        result = search()
    finally:
        searcher.agent = own
    _fire_search_triggers(state, searcher_id)
    return result


def library_search(resolve):
    """Decorator for a library-searching effect's ``resolve(self, state,
    controller_id, targets=())``: the resolving controller searches their
    own library (see the module doc)."""

    @functools.wraps(resolve)
    def wrapper(self, state, controller_id, targets=()):
        return run_library_search(
            state, controller_id,
            lambda: resolve(self, state, controller_id, targets),
        )

    return wrapper


def searched_card_taken(state, searcher_id: int, card) -> bool:
    """``card`` (just taken out of ``searcher_id``'s library by a search)
    is exiled instead of going where the search puts it when an opponent
    controls an Opposition Agent; the agent's controller may play it while
    it stays exiled. Returns ``True`` when the card was taken (the caller
    skips its own move)."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        WHILE_EXILED,
        grant_exile_play,
    )
    from mtgz_ir.game import Zone, reveal_to_all

    exilers = _opposing_sources(state, searcher_id, ExileFoundCardsForYou)
    if not exilers:
        return False
    owner = state.player_by_id(card.owner_id)
    card.zone = Zone.EXILE
    owner.exile.append(card)
    reveal_to_all(state, card)
    grant_exile_play(
        state, card, exilers[-1].controller_id,
        duration=WHILE_EXILED, any_color_mana=True,
    )
    state.log.append(
        f"P{searcher_id} exiles {card.printing.name} (found while searching); "
        f"P{exilers[-1].controller_id} may play it"
    )
    return True


def _fire_search_triggers(state, searcher_id: int) -> None:
    from mtgz_ir.actions import push_triggered_ability

    for o in list(state.battlefield):
        if o.controller_id == searcher_id:
            continue
        for idx, ability in enumerate(o.printing.triggered_abilities):
            if isinstance(ability.predicate, OpponentSearchedLibrary):
                push_triggered_ability(
                    state, source=o, ability=ability, ability_index=idx,
                    controller_id=o.controller_id,
                    label="opponent searched their library",
                )
