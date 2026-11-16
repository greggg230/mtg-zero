"""The EoE card-IR fixes for mtg-zero (``feature/eoe-ir-fixes``; mtg-zero
``docs/ir_defects.md`` 20-82): the new primitives and grammar pieces, each
against the Oracle text that needed it."""

from __future__ import annotations

import dataclasses

from mtgz_ir.cards.parser.activated import _parse_activation_cost
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.cards.source import get_active_source
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.combinators import Sequence
from mtgz_ir.effects.formulas import PowerOfCostPaidObject
from mtgz_ir.effects.generic.become_type import BecomeLandCreatureUEOT
from mtgz_ir.effects.generic.card_flow import CardFilter, CardFilterAnyOf
from mtgz_ir.effects.generic.cube_creatures_rmc import OneOrMoreBatch
from mtgz_ir.effects.generic.roto_walkers import CopyExceptions
from mtgz_ir.game import (
    GameState,
    Player,
    Zone,
    effective_card_types,
    effective_subtypes,
    is_stationed,
)

_PRINTINGS = get_active_source().get_printings()


def _state() -> GameState:
    state = GameState()
    state.players.append(Player(id=0))
    state.players.append(Player(id=1))
    state.players.append(Player(id=2))
    return state


def _on_battlefield(state: GameState, name: str, controller: int = 0):
    obj = state.new_object(_PRINTINGS[name], controller, Zone.BATTLEFIELD)
    state.battlefield.append(obj)
    return obj


# -- formulas / filters / copy exceptions ---------------------------------


def test_power_of_cost_paid_object_reads_the_sacrificed_creature():
    """Susur Secundi's "draw cards equal to the sacrificed creature's
    power": the first cost-paid object's power, 0 with none."""
    state = _state()
    f = PowerOfCostPaidObject()
    assert f.value(state, 0) == 0
    giant = _on_battlefield(state, "Galactic Wayfarer")  # a 3/3
    state.resolving_cost_paid_objects = (giant,)
    assert f.value(state, 0) == 3


def test_card_filter_any_of_matches_either_kind():
    """Starfield Shepherd: "a basic Plains card or a creature card with
    mana value 1 or less" -- one filter, a card of either kind."""
    f = CardFilterAnyOf(filters=(
        CardFilter(supertype="Basic", subtype="Plains"),
        CardFilter(any_types=("Creature",), max_mv=1),
    ))
    assert f.has_quality
    assert f.matches(_PRINTINGS["Plains"])
    assert not f.matches(_PRINTINGS["Island"])
    assert not f.matches(_PRINTINGS["Galactic Wayfarer"])  # mana value 3
    assert "or" in f.describe()


def test_copy_exceptions_add_supertypes():
    """Adagia's "a token that's a copy ..., except it's legendary" (CR
    707.9b): the Legendary supertype in addition, once."""
    base = TOKEN_PRINTINGS["Robot"]
    copy = CopyExceptions(add_supertypes=("Legendary",)).apply(base)
    assert "Legendary" in copy.supertypes and "Token" in copy.supertypes
    again = CopyExceptions(add_supertypes=("Legendary",)).apply(copy)
    assert again.supertypes.count("Legendary") == 1


# -- triggers ---------------------------------------------------------------


def test_one_or_more_batch_per_target_player():
    """"Whenever one or more creatures you control deal combat damage to a
    player" triggers once per damaged player: a waiting instance about
    one player doesn't block another about a different player."""
    from mtgz_ir.game import StackItem
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.draw_mill import DrawCards
    from mtgz_ir.effects.predicates import Event

    state = _state()
    haliya = _on_battlefield(state, "Haliya, Ascendant Cadet")
    batch = OneOrMoreBatch(per_target_player=True)
    ability = TriggeredAbility(
        event=Event.DEALS_COMBAT_DAMAGE, predicate=batch,
        effect=DrawCards(amount=1),
    )
    p1, p2 = state.players[1], state.players[2]
    assert batch.matches(state, {"target": p1}, haliya)
    state.stack.append(StackItem(
        source=haliya, controller_id=0, ability=ability,
        targets=(("player", p1.id),),
    ))
    assert not batch.matches(state, {"target": p1}, haliya)
    assert batch.matches(state, {"target": p2}, haliya)
    # Without the flag (Forth Eorlingas!'s "one or more players"): one.
    plain = OneOrMoreBatch()
    state.stack[-1] = dataclasses.replace(
        state.stack[-1],
        ability=dataclasses.replace(ability, predicate=plain),
    )
    assert not plain.matches(state, {"target": p2}, haliya)


# -- station without a power/toughness box --------------------------------


def test_planets_have_station_and_never_become_creatures():
    """Evendo, Waking Haven: "Station ... 12+ | {G}, {T}: Add {G} for each
    creature you control." A planet has station (CR 702.184a) and no
    power/toughness box (CR 722.1)."""
    state = _state()
    evendo = _on_battlefield(state, "Evendo, Waking Haven")
    assert evendo.printing.has_station
    assert evendo.printing.station_threshold == 12
    assert [a.tier_threshold for a in evendo.printing.activated_abilities] == [12]
    evendo.charge_counters = 40
    assert not is_stationed(evendo)


def test_the_eternity_elevator_has_station_and_no_power():
    elevator = _PRINTINGS["The Eternity Elevator"]
    assert elevator.has_station and elevator.station_threshold == 20
    assert elevator.power is None and elevator.toughness is None


# -- the animated Nexus lands ----------------------------------------------


def test_animated_land_gains_card_types_and_creature_types():
    """Inkmoth Nexus: "This land becomes a 1/1 Phyrexian Blinkmoth artifact
    creature with flying and infect until end of turn. It's still a land.\""""
    state = _state()
    nexus = _on_battlefield(state, "Inkmoth Nexus")
    effect = nexus.printing.activated_abilities[0].effect
    assert isinstance(effect, BecomeLandCreatureUEOT)
    assert effect.granted_card_types == ("Artifact",)
    assert effect.granted_subtypes == ("Phyrexian", "Blinkmoth")
    state.resolving_source = nexus
    effect.resolve(state, 0)
    types = {t.lower() for t in effective_card_types(state, nexus)}
    assert {"artifact", "creature", "land"} <= types
    assert {"Phyrexian", "Blinkmoth"} <= set(effective_subtypes(state, nexus))


# -- grammar ----------------------------------------------------------------


def test_four_piece_activation_cost():
    """Susur Secundi: "{1}{B}, {T}, Pay 2 life, Sacrifice a creature"."""
    cost = _parse_activation_cost("{1}{B}, {T}, Pay 2 life, Sacrifice a creature")
    assert cost is not None
    assert cost.tap_self and cost.pay_life == 2
    assert cost.sacrifice_a_permanent_type == "creature"


def test_then_split_keeps_the_and_in_the_second_piece():
    """Kavaron: "create a ... Robot ... token, then creatures you control
    get +1/+0 and gain haste until end of turn"."""
    eff = _parse_effect(
        "create a 2/2 colorless Robot artifact creature token, then creatures "
        "you control get +1/+0 and gain haste until end of turn"
    )
    assert isinstance(eff, Sequence) and len(eff.effects) == 2


def test_registry_token_of_another_colour_is_built_from_its_printed_text():
    """CR 111.4: a token has the colours its effect names. The registry's
    Human Soldier is white; a printed white one keeps the registry entry."""
    eff = _parse_effect("create a 1/1 white Human Soldier creature token")
    assert eff.token_printing.color_indicator == frozenset({"W"})
    assert TOKEN_PRINTINGS["Human Soldier"].color_indicator == frozenset({"W"})
