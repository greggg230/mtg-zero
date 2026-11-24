"""The legacy pool's Legacy Lands cards of lane B (mtg-zero's deck MTGGoldfish
8027955: Life from the Loam, Exploration, Carnivorous Cultivator // Enroot,
Yavimaya, Cradle of Growth; docs/ENGINE.md 270.2). Every one parsed with the
parser as it was (the roto pool's dredge, the cube's additional land plays,
the preparation cards' grammar, the land-type grant): these pin the printings."""

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir

LANE_B = ("Life from the Loam", "Exploration", "Carnivorous Cultivator // Enroot", "Yavimaya, Cradle of Growth")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_four_cards_are_claimed(legacy):
    assert set(LANE_B) <= set(legacy)


def test_life_from_the_loam(legacy):
    from mtgz_ir.effects.generic.roto_zones import Dredge

    c = legacy["Life from the Loam"]
    assert (c.mana_cost_str, c.card_types, c.keywords) == ("{1}{G}", ("Sorcery",), frozenset())
    # "Return up to three target land cards from your graveyard to your hand."
    e = c.spell_effect
    assert type(e).__name__ == "ReturnFromGraveyard"
    assert (e.land, e.creature, e.artifact, e.nonland_permanent, e.min_n, e.max_n, e.destination.value, e.target_kind) == (
        True, False, False, False, 0, 3, "hand", "graveyard_land")
    # "Dredge 3" (CR 702.52): a zone ability of the card in a graveyard.
    assert c.zone_abilities == (Dredge(n=3),)


def test_exploration(legacy):
    c = legacy["Exploration"]
    assert (c.mana_cost_str, c.card_types) == ("{G}", ("Enchantment",))
    # "You may play an additional land on each of your turns." (CR 305.2)
    assert c.grants_additional_land_per_turn == 1
    assert c.static_abilities == () and c.triggered_abilities == ()


def test_carnivorous_cultivator(legacy):
    from mtgz_ir.effects.predicates import Event

    c = legacy["Carnivorous Cultivator // Enroot"]
    assert (c.mana_cost_str, c.card_types, c.subtypes, c.power, c.toughness) == ("{1}{G}", ("Creature",), ("Elf", "Warlock"), 2, 3)
    assert c.keywords == frozenset({"deathtouch"})
    # "This creature enters prepared." (CR 723.3a)
    assert c.enters_prepared
    # "Whenever this creature deals combat damage to a player, return target
    # land card from your graveyard to your hand."
    (t,) = c.triggered_abilities
    assert t.event == Event.DEALS_COMBAT_DAMAGE
    assert type(t.effect).__name__ == "ReturnTargetCardFromYourGraveyardToHand"
    assert t.effect.card_filter.any_types == ("Land",)
    # Enroot {G}, Sorcery: "Search your library for a land card, put it into
    # your graveyard, then shuffle."
    p = c.prepare_spell
    assert (p.name, p.mana_cost_str, p.card_types) == ("Enroot", "{G}", ("Sorcery",))
    s = p.spell_effect
    assert type(s).__name__ == "SearchLibrary"
    assert (s.card_filter.any_types, s.destination.value, s.reveal, s.tapped) == (("Land",), "graveyard", False, False)


def test_yavimaya_cradle_of_growth(legacy):
    c = legacy["Yavimaya, Cradle of Growth"]
    assert (c.card_types, c.supertypes, c.subtypes) == (("Land",), ("Legendary",), ())
    # "Each land is a Forest in addition to its other land types." (CR 305.7's
    # last sentence): its own "{T}: Add {G}" is the Forest type's intrinsic
    # ability (305.6), not a printed one.
    assert c.grants_basic_land_type_to_all_lands == "Forest"
    assert c.produces == frozenset() and c.mana_only_from_produces and c.activated_abilities == ()
