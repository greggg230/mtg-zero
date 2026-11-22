"""The legacy pool's Legacy Hogaak Tempo cards of lane B (mtg-zero's ENGINE.md 220.2; MTGGoldfish
8027954): Mockingbird and Massacre. Mockingbird parses through the U/B cube's enter-as-copy grammar
(``cube_creatures_ub._ENTER_AS_COPY``, the roto pool's "with mana value less than or equal to the
amount of mana spent to cast this creature" rider and the Roto P4 exception); Massacre needs the
free-cast line "If <condition>, you may cast this spell without paying its mana cost." with the
parameterized control condition of ``spell_builder._free_cast_condition``."""

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.spell_builder import _casting_keyword_fields, _free_cast_condition
from mtgz_ir.effects.conditions import (
    AndCondition,
    ControlPermanent,
    FirstSpellYouCastThisGame,
    OpponentsControlAtLeast,
)
from mtgz_ir.effects.object_predicates import HasSubtype

LANE_B = ("Mockingbird", "Massacre")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_two_cards_are_claimed(legacy):
    assert set(LANE_B) <= set(legacy)


def test_mockingbird(legacy):
    m = legacy["Mockingbird"]
    assert (m.mana_cost_str, m.card_types, m.subtypes) == ("{X}{U}", ("Creature",), ("Bird", "Bard"))
    assert (m.power, m.toughness) == (1, 1) and m.keywords == frozenset({"flying"})
    # "You may have this creature enter as a copy of any creature on the battlefield with mana
    # value less than or equal to the amount of mana spent to cast this creature, except it's a
    # Bird in addition to its other types and it has flying."
    assert m.enters_as_copy_types == ("Creature",)
    assert m.enters_as_copy_max_mv_mana_spent
    assert m.enters_as_copy_add_subtypes == ("Bird",)
    assert m.enters_as_copy_add_keywords == frozenset({"flying"})
    assert not m.enters_as_copy_keep_name and m.enters_as_copy_base_pt is None and not m.enters_as_copy_add_type


def test_massacre(legacy):
    from mtgz_ir.effects.generic.mass_effects import PumpAllCreaturesUEOT

    s = legacy["Massacre"]
    assert (s.mana_cost_str, s.card_types) == ("{2}{B}{B}", ("Sorcery",))
    assert s.free_cast_condition == AndCondition(
        OpponentsControlAtLeast(HasSubtype("Plains"), 1), ControlPermanent(HasSubtype("Swamp"))
    )
    assert s.alternative_costs == ()
    assert s.spell_effect == PumpAllCreaturesUEOT(power=-2, toughness=-2)


def test_the_free_cast_line_takes_it_or_this_spell():
    # Once Upon a Time's wording ("cast it") still parses to its fixed condition.
    once = "If this spell is the first spell you've cast this game, you may cast it without paying its mana cost."
    assert _casting_keyword_fields(once) == {"free_cast_condition": FirstSpellYouCastThisGame()}
    massacre = "If an opponent controls a Plains and you control a Swamp, you may cast this spell without paying its mana cost."
    assert _casting_keyword_fields(massacre)["free_cast_condition"] == AndCondition(
        OpponentsControlAtLeast(HasSubtype("Plains"), 1), ControlPermanent(HasSubtype("Swamp"))
    )


@pytest.mark.parametrize(
    "text, expected",
    [
        # the Nemesis cycle's other shapes (one clause, either side)
        ("you control an Island", ControlPermanent(HasSubtype("Island"))),
        ("an opponent controls a Forest", OpponentsControlAtLeast(HasSubtype("Forest"), 1)),
        (
            "an opponent controls a Mountain and you control a Swamp",
            AndCondition(OpponentsControlAtLeast(HasSubtype("Mountain"), 1), ControlPermanent(HasSubtype("Swamp"))),
        ),
    ],
)
def test_the_control_condition_is_parameterized(text, expected):
    assert _free_cast_condition(text) == expected


@pytest.mark.parametrize(
    "text",
    [
        "an opponent controls two Plains",
        "you control a Swamp or an Island",
        "an opponent controls a Plains and",
        "a player controls a Plains",
    ],
)
def test_other_conditions_are_left_unclaimed(text):
    assert _free_cast_condition(text) is None
