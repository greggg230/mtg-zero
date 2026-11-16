"""Tests for the first Duskmourn creature parser batch.

This batch grows the parser grammar to claim a cluster of unclaimed
Duskmourn (set code ``dsk``) creatures whose oracle text only needs
new effect-grammar / dispatch entries (no engine work):

* **``each player draws a card``** / **``each player draws N cards``**
  → ``EachPlayerDraws(amount=N)``. Friendly Teddy's
  ``When this creature dies, each player draws a card.`` is the
  Duskmourn surface form. The plural arm is forward-looking grammar
  coverage — no Duskmourn printing today uses N>1 but the engine
  class accepts any non-negative integer.

* **``draw that many cards``** → ``DrawCardsEqualToResolvingCombatDamage()``.
  Fear of Failed Tests's ``Whenever this creature deals combat damage
  to a player, draw that many cards.`` is the Duskmourn surface
  form. The engine class already exists (Starwinder uses it via the
  bespoke table); this is purely a parser-grammar addition.

* **``Treasure`` token printing** in ``cards/tokens.py`` —
  colorless artifact token with ``{T}, Sacrifice this token: Add
  one mana of any color.`` Piggy Bank's
  ``When this creature dies, create a Treasure token.`` lands via
  the existing bare-name token grammar once the registry entry
  exists.

The shared parametrized parity test in ``tests/test_parser_parity.py``
enforces full ``CardPrinting`` parity, so this file focuses on the
new grammar / dispatch entries in isolation plus the claim
assertions.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CreateToken,
    DrawCardsEqualToResolvingCombatDamage,
    EachPlayerDraws,
)
from mtgz_ir.cards.parser.effects import _parse_effect_phrase
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- ``each player draws`` effect grammar ----------


def test_parse_each_player_draws_a_card():
    # The bare ``a card`` form collapses to amount=1. The leading
    # ``it`` source-subject pronoun is NOT present because the phrase
    # is ``each player draws ...`` — the subject is ``each player``,
    # not the source — so ``_normalize_creature_effect_phrase`` is
    # a no-op and the regex sees the verbatim surface.
    assert _parse_effect_phrase("each player draws a card") == (
        EachPlayerDraws(amount=1)
    )


def test_parse_each_player_draws_n_cards_digit():
    # Forward-looking digit form. Maps to the same engine class with
    # the captured amount on the ``amount`` field.
    assert _parse_effect_phrase("each player draws 2 cards") == (
        EachPlayerDraws(amount=2)
    )


def test_parse_each_player_draws_n_cards_word():
    # Forward-looking spelled-out small-cardinal form. Same engine
    # class, same field — the parser just runs the captured token
    # through ``_parse_amount`` to convert it to an int.
    assert _parse_effect_phrase("each player draws three cards") == (
        EachPlayerDraws(amount=3)
    )


def test_parse_each_player_draws_rejects_unrelated_subjects():
    # A phrase that LOOKS similar but uses a different subject (e.g.
    # ``each opponent``) MUST NOT collapse to ``EachPlayerDraws``.
    # The grammar dispatches on the leading ``each player`` literal;
    # ``each opponent draws a card`` would need its own primitive
    # (no Duskmourn / EoE printing carries that surface today, so we
    # stay narrow).
    assert _parse_effect_phrase("each opponent draws a card") is None


# ---------- ``draw that many cards`` effect grammar ----------


def test_parse_draw_that_many_cards():
    # Surface form on Fear of Failed Tests's combat-damage trigger
    # body. Maps to the existing engine class — the trigger condition
    # latches the ``resolving_combat_damage_amount`` and the effect
    # reads it at resolve time. The leading ``it`` is stripped by
    # ``_normalize_creature_effect_phrase`` (Fear of Failed Tests's
    # printed body is ``draw that many cards`` with no source
    # subject — the trigger condition already names ``this creature``
    # — so the normalizer is a no-op here, but the regex shape
    # matches whether or not the prefix was present).
    assert _parse_effect_phrase("draw that many cards") == (
        DrawCardsEqualToResolvingCombatDamage()
    )


# ---------- ``Treasure`` token registry entry ----------


def test_treasure_token_registered():
    # ``Treasure`` must be present in the token registry so the
    # bare-name token grammar can claim ``create a Treasure token``.
    # The registered printing is an artifact (no creature type) with
    # the canonical ``{T}, Sacrifice this token: Add one mana of any
    # color.`` mana ability.
    assert "Treasure" in TOKEN_PRINTINGS


def test_treasure_token_is_artifact_not_creature():
    # Treasure is an artifact token, NOT a creature token — its only
    # ability is the sac-for-mana mana ability. Belt-and-suspenders
    # that future edits don't accidentally promote the printing to a
    # creature.
    treasure = TOKEN_PRINTINGS["Treasure"]
    assert "Artifact" in treasure.card_types
    assert "Creature" not in treasure.card_types


def test_treasure_token_has_sac_for_mana_ability():
    # The single activated ability mints one ``("any", 1)`` coin (the
    # at-resolution choose-color form mirrored from All-Fates Scroll
    # and Mana Confluence) and is flagged ``is_mana_ability=True`` so
    # the engine bypasses the stack at activation per CR 605.
    treasure = TOKEN_PRINTINGS["Treasure"]
    assert len(treasure.activated_abilities) == 1
    ability = treasure.activated_abilities[0]
    assert ability.is_mana_ability is True
    assert ability.cost.tap_self is True
    assert ability.cost.sacrifice_self is True


def test_parse_create_a_treasure_token():
    # End-to-end through the bare-name token grammar. Piggy Bank's
    # dies trigger body is exactly this phrase (modulo the trailing
    # period and surrounding ``When … create`` wrapper).
    eff = _parse_effect_phrase("create a Treasure token")
    assert eff == CreateToken(
        token_printing=TOKEN_PRINTINGS["Treasure"], count=1,
    )


# ---------- end-to-end claim assertions ----------


def test_friendly_teddy_claimed_by_parser():
    # Friendly Teddy ``{2}`` 2/2 Artifact Creature — Bear Toy. Single
    # oracle line: ``When this creature dies, each player draws a
    # card.`` Claims via the existing dies-trigger predicate and the
    # new ``each player draws a card`` effect primitive.
    assert "Friendly Teddy" in _claimed_names()


def test_fear_of_failed_tests_claimed_by_parser():
    # Fear of Failed Tests ``{4}{U}`` 4/3 Enchantment Creature —
    # Nightmare. Single oracle line: ``Whenever this creature deals
    # combat damage to a player, draw that many cards.`` Claims via
    # the existing combat-damage-to-player trigger predicate and the
    # new ``draw that many cards`` effect primitive.
    assert "Fear of Failed Tests" in _claimed_names()


def test_piggy_bank_claimed_by_parser():
    # Piggy Bank ``{1}{R}`` 1/1 Artifact Creature — Boar Toy. Single
    # oracle line: ``When this creature dies, create a Treasure
    # token.`` Claims via the existing dies-trigger predicate and the
    # bare-name token grammar resolving against the new Treasure
    # registry entry.
    assert "Piggy Bank" in _claimed_names()


# ---------- ``another target creature you control`` pump+grant ----------


def test_parse_another_target_creature_you_control_pump_and_grant():
    # The ``another`` qualifier on the existing pump+grant primitive
    # excludes the trigger source from the legal target pool. Pairs
    # with ``you control`` to gate targets to OTHER creatures the
    # controller controls; both flags are flipped on the produced
    # engine class.
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT
    phrase = (
        "another target creature you control gets +1/+0 and gains "
        "indestructible until end of turn"
    )
    parsed = _parse_effect_phrase(phrase)
    assert parsed == PumpTargetCreatureUntilEOT(
        power_bonus=1,
        toughness_bonus=0,
        granted_keywords=frozenset({"indestructible"}),
        you_control_only=True,
        exclude_self=True,
    )


def test_pump_and_grant_bare_form_unchanged():
    # Belt-and-suspenders that adding the optional ``another `` prefix
    # to the regex didn't accidentally widen or narrow the bare
    # ``target creature gets ...`` shape. The bare form must continue
    # to produce ``you_control_only=False, exclude_self=False``.
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT
    parsed = _parse_effect_phrase(
        "target creature gets +2/+2 and gains trample until end of turn"
    )
    assert parsed == PumpTargetCreatureUntilEOT(
        power_bonus=2,
        toughness_bonus=2,
        granted_keywords=frozenset({"trample"}),
        you_control_only=False,
        exclude_self=False,
    )


def test_hardened_escort_claimed_by_parser():
    # Hardened Escort ``{2}{W}`` 2/2 Human Soldier. Single oracle
    # line: ``Whenever this creature attacks, another target creature
    # you control gets +1/+0 and gains indestructible until end of
    # turn.`` Claims via the existing this-creature-attacks trigger
    # predicate and the new ``another``-qualified pump+grant primitive.
    assert "Hardened Escort" in _claimed_names()


# ---------- ``Glimmer`` enchantment-creature token ----------


def test_glimmer_token_registered():
    # ``Glimmer`` must be present in the token registry so the
    # stat-bearing creature-token grammar (extended for the
    # ``enchantment `` qualifier) can claim Tunnel Surveyor's ETB.
    assert "Glimmer" in TOKEN_PRINTINGS


def test_glimmer_token_is_both_enchantment_and_creature():
    # Glimmer is the first dual-type ``Enchantment Creature`` token
    # in the EoE/Duskmourn pool. Both type words MUST appear in
    # ``card_types`` so enchantment-targeting effects and
    # creature-targeting effects both interact with the token.
    glimmer = TOKEN_PRINTINGS["Glimmer"]
    assert "Enchantment" in glimmer.card_types
    assert "Creature" in glimmer.card_types


def test_glimmer_token_is_vanilla_one_one():
    # A Glimmer token is a 1/1 with no abilities and no flavor text —
    # the registered printing must be vanilla (no
    # triggered/activated/static abilities, no keywords).
    glimmer = TOKEN_PRINTINGS["Glimmer"]
    assert glimmer.power == 1
    assert glimmer.toughness == 1
    assert glimmer.keywords == frozenset()
    assert glimmer.triggered_abilities == ()
    assert glimmer.activated_abilities == ()
    assert glimmer.static_abilities == ()


def test_parse_create_glimmer_enchantment_creature_token():
    # End-to-end through the extended stat-bearing creature-token
    # grammar. Tunnel Surveyor's ETB body is ``create a 1/1 white
    # Glimmer enchantment creature token``; the regex collapses the
    # token to the ``Glimmer`` registry entry while consuming the
    # ``enchantment `` qualifier as part of the surface match.
    # The printed colour (white) is carried as the colour indicator.
    from dataclasses import replace
    eff = _parse_effect_phrase(
        "create a 1/1 white Glimmer enchantment creature token"
    )
    assert eff == CreateToken(
        token_printing=replace(TOKEN_PRINTINGS["Glimmer"], color_indicator=frozenset({"W"})), count=1,
    )


def test_token_grammar_artifact_qualifier_unchanged():
    # Belt-and-suspenders that adding the optional ``enchantment ``
    # tail didn't break the pre-existing ``artifact creature token``
    # surface (Robot, Drone). Checks the parse path returns the
    # registered Robot token printing for a 2/2 colorless artifact
    # creature.
    eff = _parse_effect_phrase(
        "create a 2/2 colorless Robot artifact creature token"
    )
    assert eff == CreateToken(
        token_printing=TOKEN_PRINTINGS["Robot"], count=1,
    )


def test_tunnel_surveyor_claimed_by_parser():
    # Tunnel Surveyor ``{2}{U}`` 2/3 Human Detective. Single oracle
    # line: ``When this creature enters, create a 1/1 white Glimmer
    # enchantment creature token.`` Claims via the existing
    # this-creature-enters trigger predicate and the extended
    # stat-bearing creature-token grammar resolving against the new
    # Glimmer registry entry.
    assert "Tunnel Surveyor" in _claimed_names()
