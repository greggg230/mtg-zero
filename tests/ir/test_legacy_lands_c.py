"""The legacy pool's Legacy Lands cards of lane C (mtg-zero's deck MTGGoldfish
8027955: Pithing Needle, Cursed Totem, Torpor Orb, Damping Sphere, Choke, The
Tabernacle at Pendrell Vale; docs/ENGINE.md 270.3): their printings and the
recognizers ``cards/parser/legacy_lands_c.py`` adds: plural basic land types as
permanent phrases (Choke's "Islands"), "<permanents> entering don't cause
abilities to trigger", Damping Sphere's two lines."""

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_lands_c import (
    LEGACY_LANDS_C_ENCHANTMENTS,
    basic_land_type_phrase,
    lands_c_board_line,
)
from mtgz_ir.cards.parser.roto_board import parse_roto_board_line, permanent_phrase
from mtgz_ir.effects.generic import legacy_lands_c as lc
from mtgz_ir.effects.predicates import Event

LANE_C = ("The Tabernacle at Pendrell Vale", "Pithing Needle", "Choke", "Cursed Totem", "Damping Sphere", "Torpor Orb")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_six_cards_are_claimed(legacy):
    assert set(LANE_C) <= set(legacy)


def test_pithing_needle(legacy):
    p = legacy["Pithing Needle"]
    assert (p.mana_cost_str, p.card_types) == ("{1}", ("Artifact",))
    kinds = [type(r).__name__ for r in p.roto_statics]
    assert kinds == ["ChooseCardNameAsEnters", "ActivationProhibition"]
    assert p.roto_statics[0].nonland is False and p.roto_statics[1].except_mana_abilities is True
    assert not p.keywords, "no flash, unlike Disruptor Flute"


def test_cursed_totem(legacy):
    t = legacy["Cursed Totem"]
    assert (t.mana_cost_str, t.card_types, t.locks_activated_abilities_of) == ("{2}", ("Artifact",), ("Creature",))


def test_torpor_orb(legacy):
    t = legacy["Torpor Orb"]
    assert (t.mana_cost_str, t.card_types) == ("{2}", ("Artifact",))
    (r,) = t.board_rules
    assert isinstance(r, lc.EnteringDoesntCauseTriggers)
    assert type(r.permanent_filter).__name__ == "IsCreature" and r.label == "creatures"
    assert r.describe() == "Creatures entering don't cause abilities to trigger."


def test_damping_sphere(legacy):
    d = legacy["Damping Sphere"]
    assert (d.mana_cost_str, d.card_types) == ("{2}", ("Artifact",))
    assert d.board_rules == (lc.MultiManaLandsProduceColorless(), lc.SpellTaxPerOtherSpellThisTurn(amount=1))


def test_choke(legacy):
    c = legacy["Choke"]
    assert (c.mana_cost_str, c.card_types) == ("{2}{G}", ("Enchantment",))
    (r,) = c.board_rules
    assert type(r).__name__ == "DoesntUntap"
    assert type(r.permanent_filter).__name__ == "HasSubtype" and r.permanent_filter.name == "Island"
    assert "Choke" in LEGACY_LANDS_C_ENCHANTMENTS


def test_the_tabernacle(legacy):
    t = legacy["The Tabernacle at Pendrell Vale"]
    assert (t.mana_cost_str, t.card_types, t.supertypes, t.produces) == ("", ("Land",), ("Legendary",), frozenset())
    (r,) = t.board_rules
    assert type(r).__name__ == "GrantsTriggerToCreatures"
    a = r.ability
    assert a.event == Event.BEGINNING_OF_UPKEEP
    assert type(a.effect).__name__ == "DestroySourceUnlessPay" and a.effect.cost == "{1}"
    assert type(a.predicate).__name__ == "Controlled"


@pytest.mark.parametrize("np, sub", [("islands", "Island"), ("forests", "Forest"), ("swamps", "Swamp"), ("mountains", "Mountain"), ("plains", "Plains")])
def test_plural_basic_land_types(np, sub):
    f = basic_land_type_phrase(np, plural=True)
    assert type(f).__name__ == "HasSubtype" and f.name == sub
    assert permanent_phrase(np, plural=True) == f


@pytest.mark.parametrize("np, plural", [("island", True), ("island", False), ("wastes", True), ("deserts", True)])
def test_not_plural_basic_land_types(np, plural):
    assert basic_land_type_phrase(np, plural=plural) is None


def test_the_board_lines():
    assert lands_c_board_line("Creatures entering don't cause abilities to trigger.").fields["board_rules"] == (
        lc.EnteringDoesntCauseTriggers(permanent_filter=permanent_phrase("creatures", plural=True), label="creatures"),
    )
    assert lands_c_board_line("Each spell a player casts costs {2} more to cast for each other spell that player has cast this turn.").fields[
        "board_rules"
    ] == (lc.SpellTaxPerOtherSpellThisTurn(amount=2),)
    for no in [
        "Artifacts and creatures entering don't cause abilities to trigger.",
        "If a land is tapped for mana, it produces {C} instead of any other type and amount.",
        "Each spell a player casts costs {1} more to cast.",
        "Each spell a player casts costs {X} more to cast for each other spell that player has cast this turn.",
    ]:
        assert lands_c_board_line(no) is None, no
    # Through the board-line chain (last of it), the same.
    assert parse_roto_board_line("Islands don't untap during their controllers' untap steps.", {"name": "Choke"}) is not None
