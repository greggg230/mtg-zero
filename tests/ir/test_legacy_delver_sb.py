"""The legacy pool's Delver sideboard cards (mtg-zero's Legacy Izzet Delver
75, MTGGoldfish 8016541: Consign to Memory, Hydroblast, Meltdown, Price of
Progress, Rough // Tumble): their printings, the one recognizer of
``cards/parser/legacy_delver_sb.py`` ("deals N damage to each creature with
/ without <keyword>") and the Python semantics of
``effects/generic/legacy_delver_sb.DealNDamageToEachCreatureMatching``."""

# mtg-zero (docs/ENGINE.md 170): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_the_damage_reaches_only_the_matching_creatures

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_delver_sb import parse_legacy_delver_sb_effect
from mtgz_ir.effects.generic.legacy_delver_sb import DealNDamageToEachCreatureMatching
from mtgz_ir.effects.object_predicates import HasKeyword

DELVER_SB = ("Consign to Memory", "Hydroblast", "Meltdown", "Price of Progress", "Rough // Tumble")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_five_cards_are_claimed(legacy):
    assert set(DELVER_SB) <= set(legacy)


def test_consign_to_memory(legacy):
    from mtgz_ir.effects.generic.roto_stack import CounterTargetStackObjectFiltered, IsColorless
    from mtgz_ir.effects.vocab import StackObjectKind

    c = legacy["Consign to Memory"]
    assert (c.mana_cost_str, c.card_types, c.replicate_cost) == ("{U}", ("Instant",), "{1}")
    eff = c.spell_effect
    assert isinstance(eff, CounterTargetStackObjectFiltered)
    # "Counter target triggered ability or colorless spell."
    assert eff.kinds == (StackObjectKind.TRIGGERED, StackObjectKind.SPELL)
    assert eff.spell_filter == IsColorless() and not eff.up_to_one


def test_hydroblast_is_pyroblasts_shape_with_red(legacy):
    from mtgz_ir.effects import ModalChooseOne
    from mtgz_ir.effects.value_predicates import HasColor

    h = legacy["Hydroblast"]
    assert (h.mana_cost_str, h.card_types) == ("{U}", ("Instant",))
    eff = h.spell_effect
    assert isinstance(eff, ModalChooseOne) and not eff.or_both
    counter, destroy = eff.modes
    assert type(counter).__name__ == "CounterSpell" and counter.only_if == HasColor("R")
    assert type(destroy).__name__ == "RemoveTargetPermanents" and destroy.only_if == HasColor("R")


def test_meltdown(legacy):
    m = legacy["Meltdown"]
    assert (m.mana_cost_str, m.card_types, m.mana_cost.x_pips) == ("{X}{R}", ("Sorcery",), 1)
    eff = m.spell_effect
    assert type(eff).__name__ == "RemoveAllMatching"
    assert eff.max_mana_value_from_x and type(eff.permanent_filter).__name__ == "IsArtifact"


def test_price_of_progress(legacy):
    from mtgz_ir.effects.generic.roto_board import DamageEachPlayerPerControlled

    p = legacy["Price of Progress"]
    assert (p.mana_cost_str, p.card_types) == ("{1}{R}", ("Instant",))
    eff = p.spell_effect
    assert isinstance(eff, DamageEachPlayerPerControlled) and eff.multiplier == 2
    assert eff.per_filter.left.__class__.__name__ == "IsLand"
    assert eff.per_filter.right.inner.__class__.__name__ == "IsBasic"


def test_rough_tumble_is_a_split_card(legacy):
    r = legacy["Rough // Tumble"]
    # CR 709.4: both names, the combined mana cost (mana value 8), both
    # halves' types, no effect of its own.
    assert (r.mana_cost_str, r.type_line, r.card_types) == ("{1}{R} // {5}{R}", "Sorcery // Sorcery", ("Sorcery",))
    assert r.mana_cost.generic == 6 and r.mana_cost.colored == (("R", 2),)
    assert r.spell_effect is None
    rough, tumble = r.split_faces
    assert (rough.name, rough.mana_cost_str) == ("Rough", "{1}{R}")
    assert rough.spell_effect == DealNDamageToEachCreatureMatching(amount=2, creature_filter=~HasKeyword("flying"))
    assert (tumble.name, tumble.mana_cost_str) == ("Tumble", "{5}{R}")
    assert tumble.spell_effect == DealNDamageToEachCreatureMatching(amount=6, creature_filter=HasKeyword("flying"))


@pytest.mark.parametrize("phrase, n, pred", [
    ("deals 2 damage to each creature without flying", 2, ~HasKeyword("flying")),
    ("deals 6 damage to each creature with flying", 6, HasKeyword("flying")),
    ("deals 1 damage to each creature with flying or reach", 1, HasKeyword("flying") | HasKeyword("reach")),
    ("deals 3 damage to each creature without flying or reach", 3, ~(HasKeyword("flying") | HasKeyword("reach"))),
])
def test_the_recognizer(phrase, n, pred):
    assert parse_legacy_delver_sb_effect(phrase) == DealNDamageToEachCreatureMatching(amount=n, creature_filter=pred)


def test_with_takes_the_target_phrase_qualifiers():
    from mtgz_ir.effects.generic.cube_walkers import HasManaValueAtMost

    eff = parse_legacy_delver_sb_effect("deals 2 damage to each creature with mana value 3 or less")
    assert eff == DealNDamageToEachCreatureMatching(amount=2, creature_filter=HasManaValueAtMost(3))


@pytest.mark.parametrize("phrase", [
    "deals 2 damage to each creature",
    "deals X damage to each creature without flying",
    "deals 2 damage to each creature without mana value 3 or less",
    "deals 2 damage to each creature with frobbing",
    "deals 2 damage to each creature without frobbing",
    "deals 2 damage to each creature and each player without flying",
    "deals 2 damage to each creature without flying and each player",
])
def test_phrases_it_does_not_claim(phrase):
    assert parse_legacy_delver_sb_effect(phrase) is None


def test_the_halves_parse_through_the_unified_grammar():
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    assert _parse_spell_effect_phrase("Rough deals 2 damage to each creature without flying.", "Rough") == (
        DealNDamageToEachCreatureMatching(amount=2, creature_filter=~HasKeyword("flying"))
    )
    assert _parse_spell_effect_phrase("Tumble deals 6 damage to each creature with flying.", "Tumble") == (
        DealNDamageToEachCreatureMatching(amount=6, creature_filter=HasKeyword("flying"))
    )


