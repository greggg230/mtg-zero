"""Companion (CR 702.139) in the parser and the card IR: the deck
conditions of the cube's companions (``effects.deck_conditions``)."""

from __future__ import annotations

import json

import pytest

from mtgz_ir.cards import BASIC_LANDS, ir
from mtgz_ir.cards.base import make_artifact, make_creature, make_instant
from mtgz_ir.cards.parser.creature_rmc_grammar import (
    parse_companion_line,
    parse_rmc_creature_line_post,
)
from mtgz_ir.cards.source import OracleParserCardSource
from mtgz_ir.effects.deck_conditions import (
    Companion,
    DeckCondition,
    EachCardInStartingDeck,
    StartingDeckDifferentNames,
)
from mtgz_ir.effects.object_predicates import IsCreature, IsLand, ObjectNot
from mtgz_ir.effects.value_predicates import (
    HasActivatedAbility,
    HasManaValueAtMost,
    IsPermanentCard,
)
from mtgz_ir.game import GameState, Player, Zone

LURRUS = "Companion — Each permanent card in your starting deck has mana value 2 or less."
LUTRI = "Companion — Each nonland card in your starting deck has a different name."
ZIRDA = "Companion — Each permanent card in your starting deck has an activated ability."


def test_parse_the_three_cube_conditions():
    assert parse_companion_line(LURRUS) == Companion(
        EachCardInStartingDeck(cards=IsPermanentCard(), must=HasManaValueAtMost(2))
    )
    assert parse_companion_line(LUTRI) == Companion(
        StartingDeckDifferentNames(cards=ObjectNot(IsLand()))
    )
    assert parse_companion_line(ZIRDA) == Companion(
        EachCardInStartingDeck(cards=IsPermanentCard(), must=HasActivatedAbility())
    )
    # "creature card" is a noun the grammar knows (Keruga-style shapes).
    assert parse_companion_line(
        "Companion — Each creature card in your starting deck has mana value 3 or less."
    ) == Companion(EachCardInStartingDeck(cards=IsCreature(), must=HasManaValueAtMost(3)))


@pytest.mark.parametrize("line", [
    # Conditions not modeled yet (Kaheera, Jegantha): no companion, and the
    # line stays inert as before.
    "Companion — Each creature card in your starting deck is a Cat, Elemental, "
    "Nightmare, Dinosaur, or Beast card.",
    "Companion — No card in your starting deck has more than one of the same "
    "mana symbol in its mana cost.",
    "Companion — Each permanent card in your starting deck has flying.",
    "Flying",
])
def test_unmodeled_conditions_are_not_companions(line):
    assert parse_companion_line(line) is None


def test_creature_line_routes_companion_to_the_printing_field():
    rmc = parse_rmc_creature_line_post(LURRUS, "Lurrus of the Dream-Den")
    assert rmc.printing_fields == {"companion": parse_companion_line(LURRUS)}
    # An unmodeled companion clause is still claimed, contributing nothing.
    kaheera = (
        "Companion — Each creature card in your starting deck is a Cat, "
        "Elemental, Nightmare, Dinosaur, or Beast card."
    )
    rmc = parse_rmc_creature_line_post(kaheera, "Kaheera, the Orphanguard")
    assert rmc is not None and rmc.printing_fields == {}


def test_cube_printings_carry_the_companion_and_nothing_else_does():
    printings = OracleParserCardSource(pool="cube").get_printings()
    have = {n for n, p in printings.items() if p.companion is not None}
    assert have == {"Lurrus of the Dream-Den", "Lutri, the Spellchaser", "Zirda, the Dawnwaker"}
    assert printings["Lurrus of the Dream-Den"].companion == parse_companion_line(LURRUS)
    assert printings["Lutri, the Spellchaser"].companion == parse_companion_line(LUTRI)
    assert printings["Zirda, the Dawnwaker"].companion == parse_companion_line(ZIRDA)
    # Its in-game abilities are unchanged (Lurrus keeps its permission).
    assert printings["Lurrus of the Dream-Den"].static_abilities


def test_companion_in_the_ir_round_trips():
    printing = OracleParserCardSource(pool="cube").get_printings()["Lutri, the Spellchaser"]
    schema = ir.build_schema([printing])
    enc = ir.Encoder(schema).encode_printing(printing)
    assert enc["companion"] == {
        "$t": "Companion",
        "condition": {
            "$t": "StartingDeckDifferentNames",
            "cards": {"$t": "ObjectNot", "inner": {"$t": "IsLand"}},
        },
    }
    assert ir.strict_equal(ir.Decoder(schema).decode_printing(json.loads(json.dumps(enc))), printing)
    cats = ir.schema_to_json(schema)["categories"]
    assert cats["DeckCondition"] == ["StartingDeckDifferentNames"]
    assert cats["Companion"] == ["Companion"]


def _state():
    s = GameState()
    s.players.append(Player(id=0, agent=None))
    s.players.append(Player(id=1, agent=None))
    return s


def _deck(s, printings):
    return [s.new_object(p, 0, Zone.LIBRARY) for p in printings]


def test_each_card_condition_over_a_deck():
    s = _state()
    bear = make_creature("Bear", "{1}{G}", 2, 2)
    ogre = make_creature("Ogre", "{2}{R}", 3, 3)
    bolt = make_instant("Bolt", "{R}", effect=None)
    lurrus = parse_companion_line(LURRUS).condition
    assert isinstance(lurrus, DeckCondition)
    # Lands are permanent cards of mana value 0; instants don't count.
    assert lurrus.fulfilled_by(_deck(s, [bear, bolt, BASIC_LANDS["Forest"]]), s)
    assert not lurrus.fulfilled_by(_deck(s, [bear, ogre]), s)
    assert lurrus.fulfilled_by([], s)  # vacuous


def test_different_names_condition_over_a_deck():
    s = _state()
    bear = make_creature("Bear", "{1}{G}", 2, 2)
    lutri = parse_companion_line(LUTRI).condition
    forest = BASIC_LANDS["Forest"]
    # Basic lands share names, but they are lands.
    assert lutri.fulfilled_by(_deck(s, [bear, forest, forest]), s)
    assert not lutri.fulfilled_by(_deck(s, [bear, bear, forest]), s)


def test_has_activated_ability():
    s = _state()
    zirda = parse_companion_line(ZIRDA).condition
    bear = make_creature("Bear", "{1}{G}", 2, 2)
    printings = OracleParserCardSource(pool="cube").get_printings()
    walker = printings["Jace, the Mind Sculptor"]
    pestle = printings["Lion Sash"]  # reconfigure + an activated ability
    forest = BASIC_LANDS["Forest"]  # an intrinsic mana ability (CR 305.6)
    assert HasActivatedAbility().test(s.new_object(forest, 0, Zone.LIBRARY), s)
    assert HasActivatedAbility().test(s.new_object(walker, 0, Zone.LIBRARY), s)
    assert HasActivatedAbility().test(s.new_object(pestle, 0, Zone.LIBRARY), s)
    assert not HasActivatedAbility().test(s.new_object(bear, 0, Zone.LIBRARY), s)
    assert not HasActivatedAbility().test(s.players[0], s)  # a player
    assert not IsPermanentCard().test(s.players[0], s)
    assert IsPermanentCard().test(s.new_object(forest, 0, Zone.LIBRARY), s)
    assert not IsPermanentCard().test(s.new_object(make_instant("Bolt", "{R}", effect=None), 0, Zone.LIBRARY), s)
    rock = make_artifact("Rock", "{2}", activated_abilities=walker.activated_abilities)
    assert zirda.fulfilled_by(_deck(s, [rock, forest, make_instant("Bolt", "{R}", effect=None)]), s)
    assert not zirda.fulfilled_by(_deck(s, [rock, bear]), s)
