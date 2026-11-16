"""Dispatch rules of the compositional phrase grammar
(:mod:`mtgz_ir.cards.parser.precise_grammar`) and the parse shapes of
the removal recognizers in :mod:`mtgz_ir.cards.parser.removal_phrases`."""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    CounterTargetSpell,
    DealDamageToTargetCreature,
    DestroyTargetCreature,
    ExileTargetCreature,
)
from mtgz_ir.cards.parser.effects import _parse_effect, _parse_spell_effect_phrase
from mtgz_ir.cards.parser.precise_grammar import (
    RUST_PARITY_PINNED_SPELLS,
    legacy_parity_pinned,
    parse_precise_fallback,
    parse_precise_override,
    precise_grammar_active,
)
from mtgz_ir.cards.parser.removal_phrases import (
    parse_damage_to_target,
    parse_destroy_all,
    parse_each_opponent_sacrifices,
    parse_targeted_removal,
    split_sentences,
)
from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
from mtgz_ir.effects.generic.damage import DealDamage
from mtgz_ir.effects.generic.targeted_removal import (
    ControllerCreatesTokens,
    ControllerGainsLifeEqualToPower,
    ControllerSearchesForBasicLand,
    RemoveAllMatching,
    OtherPlayersCreateTokenCopy,
    RemoveTargetPermanents,
)
from mtgz_ir.effects.object_predicates import (
    IsArtifact, IsCreature, IsEnchantment, IsLand, IsPlaneswalker,
)
from mtgz_ir.effects.value_predicates import HasColor, IsPermanent

import functools


# mtg-zero: mtg-bot's tests/_spells_a_helpers.cube_printings, inlined (the helper module also builds
# Python-engine games, which stayed in mtg-bot; docs/ENGINE.md 169).
@functools.lru_cache(maxsize=1)
def cube_printings():
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    return OracleParserCardSource(pool="cube").get_printings()


# ---------- dispatch ----------


def test_legacy_faithful_shapes_keep_their_legacy_class():
    # "destroy target creature" is modeled faithfully by the legacy
    # grammar; the compositional fallback never sees it.
    assert _parse_effect("destroy target creature") == DestroyTargetCreature()


def test_lossy_surface_is_overridden_faithfully():
    eff = _parse_effect("exile target creature or planeswalker")
    assert isinstance(eff, RemoveTargetPermanents)
    assert eff.target_filter == IsCreature() | IsPlaneswalker()
    dmg = _parse_effect("deals 2 damage to target creature or planeswalker")
    assert isinstance(dmg, DealDamage)
    assert dmg.targets[0].filter == IsCreature() | IsPlaneswalker()


def test_override_only_fires_on_lossy_surfaces():
    assert parse_precise_override("destroy target permanent") is None
    assert parse_precise_fallback("destroy target permanent") is not None


def test_pinned_legacy_spells_keep_lossy_models():
    assert "Scorching Dragonfire" in RUST_PARITY_PINNED_SPELLS
    eff = _parse_spell_effect_phrase(
        "Scorching Dragonfire deals 3 damage to target creature or "
        "planeswalker.",
        "Scorching Dragonfire",
    )
    assert eff == DealDamageToTargetCreature(amount=3)
    # Archenemy's Charm was un-pinned in feature/eoe-ir-fixes: it now
    # parses faithfully (exile target creature OR planeswalker).
    assert "Archenemy's Charm" not in RUST_PARITY_PINNED_SPELLS
    charm = _parse_spell_effect_phrase(
        "Exile target creature or planeswalker.", "Archenemy's Charm",
    )
    assert isinstance(charm, RemoveTargetPermanents)
    assert charm.target_filter == IsCreature() | IsPlaneswalker()
    assert _parse_spell_oracle_text(
        "Counter target spell unless its controller pays {2}. "
        "If they do, surveil 2.",
        "Don't Make a Sound",
    ) == CounterTargetSpell()


def test_pin_context_manager_scopes_and_restores():
    assert precise_grammar_active()
    with legacy_parity_pinned("Don't Make a Sound"):
        assert not precise_grammar_active()
        assert parse_precise_fallback("destroy target permanent") is None
        assert parse_precise_override(
            "exile target creature or planeswalker",
        ) is None
    assert precise_grammar_active()
    with legacy_parity_pinned("Vindicate"):
        assert precise_grammar_active()


def test_suplex_parses_both_modes():
    # Suplex must never be claimed as just one of its modes.
    text = (
        "Choose one —\n"
        "• Suplex deals 3 damage to target creature. If that creature "
        "would die this turn, exile it instead.\n"
        "• Exile target artifact."
    )
    assert len(_parse_spell_oracle_text(text, "Suplex").modes) == 2


# ---------- removal shapes ----------


def test_split_sentences_lowercases_followers():
    assert split_sentences("exile target creature. Its controller gains 2.") == [
        "exile target creature", "its controller gains 2",
    ]


@pytest.mark.parametrize("phrase, verb, target_filter", [
    ("destroy target permanent", "destroy", IsPermanent()),
    ("exile target nonland permanent", "exile", IsPermanent() & ~IsLand()),
    (
        "destroy target creature, enchantment, or planeswalker",
        "destroy",
        (IsCreature() | IsEnchantment()) | IsPlaneswalker(),
    ),
    ("destroy target nonblack creature", "destroy", IsCreature() & ~HasColor("B")),
])
def test_targeted_removal_shapes(phrase, verb, target_filter):
    eff = parse_targeted_removal(phrase)
    assert eff == RemoveTargetPermanents(action=verb, target_filter=target_filter)


def test_targeted_removal_riders():
    eff = parse_targeted_removal(
        "destroy target nonblack creature. It can't be regenerated",
    )
    assert eff.can_be_regenerated is False
    eff = parse_targeted_removal(
        "exile target creature. Its controller gains life equal to its power",
    )
    assert eff.rider == ControllerGainsLifeEqualToPower()
    eff = parse_targeted_removal(
        "exile target creature. Its controller may search their library for "
        "a basic land card, put that card onto the battlefield tapped, then "
        "shuffle",
    )
    assert eff.rider == ControllerSearchesForBasicLand(optional=True)
    eff = parse_targeted_removal(
        "destroy target creature. Its controller creates a Treasure token",
    )
    assert eff.rider == ControllerCreatesTokens(token_name="Treasure", count=1)
    eff = parse_targeted_removal(
        "exile target nonland permanent. Each player other than its "
        "controller creates a token that's a copy of it",
    )
    assert eff.rider == OtherPlayersCreateTokenCopy()


def test_up_to_two_targets_uses_plural_grammar():
    eff = parse_targeted_removal(
        "destroy up to two target artifacts and/or enchantments",
    )
    assert eff.count_min == 0 and eff.count_max == 2
    assert eff.target_filter == IsArtifact() | IsEnchantment()


@pytest.mark.parametrize("phrase", [
    "destroy target wizard",
    "destroy each creature",
    "sacrifice target creature",
    "destroy up to X target wizards",
    "exile target creature. It can't be regenerated",
    "destroy target creature. It can't be regenerated. They can't be regenerated",
    "destroy target creature. Its controller creates two Blorp tokens",
    "destroy target creature. Its controller gains life equal to its power. "
    "Its controller gains life equal to its power",
    "destroy target creature. Draw a card",
])
def test_targeted_removal_rejects(phrase):
    assert parse_targeted_removal(phrase) is None


def test_destroy_all_shapes():
    eff = parse_destroy_all("destroy all creatures. They can't be regenerated")
    assert eff == RemoveAllMatching(
        permanent_filter=IsCreature(), can_be_regenerated=False,
        label="all creatures",
    )
    eff = parse_destroy_all(
        "destroy all artifacts and creatures with mana value X or less",
    )
    assert eff.max_mana_value_from_x
    assert eff.permanent_filter == IsArtifact() | IsCreature()
    assert parse_destroy_all("destroy all wizards") is None
    assert parse_destroy_all("destroy all creatures. Draw a card") is None
    assert parse_destroy_all("sacrifice all creatures") is None
    assert parse_destroy_all("exile all creatures").action == "exile"


def test_damage_to_target_shapes():
    eff = parse_damage_to_target("deals 5 damage to target creature or planeswalker")
    assert eff.amount == 5
    assert parse_damage_to_target("deals 5 damage to two target creatures") is None
    assert parse_damage_to_target("deals 5 damage to target wizard") is None
    assert parse_damage_to_target("deals X damage to each creature") is None


def test_each_opponent_sacrifices_shape():
    assert parse_each_opponent_sacrifices(
        "each opponent sacrifices a planeswalker of their choice",
    ).permanent_filter == IsPlaneswalker()
    assert parse_each_opponent_sacrifices(
        "each opponent sacrifices a wizard of their choice",
    ) is None
    assert parse_each_opponent_sacrifices("each opponent draws a card") is None


# ---------- claims ----------


@pytest.mark.parametrize("name", [
    "Vindicate",
    "Swords to Plowshares",
    "Path to Exile",
    "Erode",
    "Cut Down",
    "Wrath of God",
    "Fractured Identity",
    "Sheoldred's Edict",
    "Sear",
])
def test_cube_removal_cards_claimed(name):
    assert name in cube_printings()
