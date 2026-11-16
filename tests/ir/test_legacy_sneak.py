"""The legacy pool's Sneak and Show cards (mtg-zero's Legacy Sneak and Show
deck, MTGGoldfish 8014220: Show and Tell, Omniscience, Mistrise Village,
Brotherhood's End, Disruptor Flute, Magus of the Moon): their printings, the
one recognizer of ``cards/parser/legacy_sneak.py`` ("deals N damage to each
creature and each planeswalker") and the Python semantics of
``effects/generic/legacy_sneak.DealNDamageToEachCreatureAndPlaneswalker``."""

# mtg-zero (docs/ENGINE.md 170): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_the_damage_reaches_each_creature_and_planeswalker_not_players

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_sneak import parse_legacy_sneak_effect
from mtgz_ir.effects.generic.legacy_sneak import DealNDamageToEachCreatureAndPlaneswalker

SNEAK_SHOW = ("Show and Tell", "Omniscience", "Mistrise Village", "Brotherhood's End", "Disruptor Flute", "Magus of the Moon")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_six_cards_are_claimed(legacy):
    assert set(SNEAK_SHOW) <= set(legacy)


def test_show_and_tell(legacy):
    from mtgz_ir.effects.generic.roto_zones import EachPlayerMayPutFromHand

    s = legacy["Show and Tell"]
    assert (s.mana_cost_str, s.card_types) == ("{2}{U}", ("Sorcery",))
    eff = s.spell_effect
    assert isinstance(eff, EachPlayerMayPutFromHand)
    assert eff.card_filter.any_types == ("Artifact", "Creature", "Enchantment", "Land")


def test_omniscience(legacy):
    o = legacy["Omniscience"]
    assert (o.mana_cost_str, o.card_types) == ("{7}{U}{U}{U}", ("Enchantment",))
    assert o.grants_free_casts_from_hand


def test_mistrise_village(legacy):
    from mtgz_ir.effects.generic.roto_board import NextSpellThisTurnCantBeCountered

    v = legacy["Mistrise Village"]
    assert v.card_types == ("Land",) and v.produces == frozenset({"U"})
    # "This land enters tapped unless you control a Mountain or a Forest."
    assert type(v.enters_tapped_unless).__name__ == "ControlPermanent"
    (ab,) = v.activated_abilities
    assert ab.cost.mana_str == "{U}" and ab.cost.tap_self
    assert ab.effect == NextSpellThisTurnCantBeCountered()


def test_brotherhoods_end(legacy):
    from mtgz_ir.effects import ModalChooseOne

    b = legacy["Brotherhood's End"]
    assert (b.mana_cost_str, b.card_types) == ("{1}{R}{R}", ("Sorcery",))
    eff = b.spell_effect
    assert isinstance(eff, ModalChooseOne) and not eff.or_both
    damage, destroy = eff.modes
    assert damage == DealNDamageToEachCreatureAndPlaneswalker(amount=3)
    assert type(destroy).__name__ == "RemoveAllMatching"
    assert destroy.label == "all artifacts with mana value 3 or less"


def test_disruptor_flute(legacy):
    from mtgz_ir.roto_restrictions import ActivationProhibition, ChooseCardNameAsEnters, SpellTax

    f = legacy["Disruptor Flute"]
    assert (f.mana_cost_str, f.card_types) == ("{2}", ("Artifact",)) and "flash" in f.keywords
    assert f.roto_statics == (
        ChooseCardNameAsEnters(),
        SpellTax(amount=3, named=True),
        ActivationProhibition(except_mana_abilities=True),
    )


def test_magus_of_the_moon(legacy):
    m = legacy["Magus of the Moon"]
    assert (m.mana_cost_str, m.power, m.toughness) == ("{2}{R}", 2, 2)
    assert m.nonbasic_lands_are_mountains


@pytest.mark.parametrize("phrase, n", [
    ("deals 3 damage to each creature and each planeswalker", 3),
    ("deals 4 damage to each creature and each planeswalker", 4),
    ("deals 10 damage to each creature and each planeswalker", 10),
])
def test_the_recognizer(phrase, n):
    assert parse_legacy_sneak_effect(phrase) == DealNDamageToEachCreatureAndPlaneswalker(amount=n)


@pytest.mark.parametrize("phrase", [
    "deals 3 damage to each creature",
    "deals 3 damage to each creature and each player",
    "deals X damage to each creature and each planeswalker",
    "deals 3 damage to each creature and each planeswalker you control",
])
def test_phrases_it_does_not_claim(phrase):
    assert parse_legacy_sneak_effect(phrase) is None


def test_the_spell_line_parses_through_the_unified_grammar():
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    eff = _parse_spell_effect_phrase(
        "Brotherhood's End deals 3 damage to each creature and each planeswalker.", "Brotherhood's End",
    )
    assert eff == DealNDamageToEachCreatureAndPlaneswalker(amount=3)


