"""Tests for the DSK Enduring cycle dies-trigger.

The Enduring cycle (Innocence W, Curiosity U, Tenacity B, Courage R,
Vitality G) prints a shared ``"When this creature dies, if it was a
creature, return it to the battlefield under its owner's control. It's
an enchantment."`` rider on each creature. This batch lands the cycle's
parser/engine plumbing:

- ``GameObject.enduring_returned_as_enchantment`` flag — sticky per-
  instance marker that suppresses the printed Creature card type at
  runtime via ``is_creature_now``. Mirrors the ``impending_active``
  precedent (printing keeps the type, GameObject suppresses it).
- ``EnduringDiesReturnAsEnchantmentEffect`` — bespoke parameterless
  Effect class that reads ``state.resolving_source`` to locate the
  dying card in its owner's graveyard, moves it back onto the
  battlefield under the owner's control, and sets the new flag.
  Mirrors the ``PerigeeBeckonerDiesReturnTappedEffect`` precedent.
- ``"it was a creature"`` intervening-if gate phrase → ``IsCreature()``
  predicate. Composes onto the ``IsSelf()`` base predicate of the
  ``"this creature dies"`` self-trigger via the existing
  intervening-if path.
- ``return it to the battlefield under its owner's control. It's an
  enchantment`` effect-phrase regex → the new bespoke effect class.

Of the five Enduring printings, ``Enduring Curiosity`` claims end-to-
end via these additions alone — its non-dies trigger
(``"Whenever a creature you control deals combat damage to a player,
draw a card."``) was already supported. The four other cards
(Innocence, Tenacity, Courage, Vitality) carry additional printed
clauses (aggregate "one or more" enters trigger, gain-life trigger,
"another creature you control enters" trigger with subject pump,
mana-ability anthem static) that need grammar/event additions outside
the Enduring cycle's scope — those land in the parallel
creatures-batch-4 / spells-batch-4 work and the four cards stay
unclaimed end-to-end on this branch.
"""

# mtg-zero (docs/ENGINE.md 169): 9 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_enduring_dies_via_engine_path_returns_as_enchantment, test_enduring_effect_no_op_for_token_source, test_enduring_effect_no_op_if_no_resolving_source, test_enduring_effect_no_op_if_source_not_in_graveyard, test_enduring_effect_returns_card_to_owners_battlefield, test_enduring_effect_routes_to_owner_not_resolver, test_enduring_second_death_does_not_re_fire, test_is_creature_now_face_down_takes_precedence_over_enduring, test_is_creature_now_false_with_enduring_flag

from __future__ import annotations

from mtgz_ir.cards.base import (
    DrawCards,
    EnduringDiesReturnAsEnchantmentEffect,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.effects import _parse_effect_phrase
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.effects.object_predicates import (
    IsCreature,
    IsPlayer,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    StateBound,
    Subject,
    YOU,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- effect-phrase grammar ----------


def test_parse_enduring_dies_return_as_enchantment_body():
    # Bare effect-phrase parse for the Enduring cycle's shared body.
    # The phrase appears inside an intervening-if line and the
    # parenthesized ``(It's not a creature.)`` reminder is stripped
    # before this dispatcher runs, so the regex matches the two-
    # sentence body verbatim.
    phrase = (
        "return it to the battlefield under its owner's control. "
        "It's an enchantment"
    )
    assert _parse_effect_phrase(phrase) == (
        EnduringDiesReturnAsEnchantmentEffect()
    )


def test_parse_enduring_body_unrelated_text_rejects():
    # A near-miss phrase that drops the "It's an enchantment" tail must
    # NOT be claimed by the new regex — the bare graveyard-return
    # primitive is a different engine class. Belt-and-suspenders that
    # the new grammar entry didn't accidentally widen the dispatcher.
    phrase = (
        "return it to the battlefield under its owner's control"
    )
    assert _parse_effect_phrase(phrase) is None


# ---------- intervening-if trigger-line end-to-end ----------


def test_parse_enduring_dies_trigger_line():
    # End-to-end through the trigger-line parser. The intervening-if
    # path composes the gate predicate (``IsCreature()``) onto the
    # base predicate of ``"this creature dies"`` (``IsSelf()``) via
    # plain ``And``, mirroring the Delirium / Void gate precedents.
    # The ``Enduring Curiosity`` name is substituted with
    # ``"this creature"`` by ``_substitute_self_name`` before the
    # condition phrase is looked up.
    line = (
        "When Enduring Curiosity dies, if it was a creature, "
        "return it to the battlefield under its owner's control. "
        "It's an enchantment."
    )
    parsed = _parse_triggered_ability_line(line, name="Enduring Curiosity")
    assert parsed == TriggeredAbility(
        event=Event.DIES,
        predicate=IsSelf() & Subject(None, IsCreature()),
        effect=EnduringDiesReturnAsEnchantmentEffect(),
    )


def test_parse_enduring_dies_trigger_line_innocence():
    # Same shape against a different printing's name — the substitution
    # rewrites both the full name and the canonical surface, so every
    # Enduring printing's dies trigger maps to the same
    # ``TriggeredAbility``. Belt-and-suspenders that the
    # ``_substitute_self_name`` step generalizes across the cycle.
    line = (
        "When Enduring Innocence dies, if it was a creature, "
        "return it to the battlefield under its owner's control. "
        "It's an enchantment."
    )
    parsed = _parse_triggered_ability_line(line, name="Enduring Innocence")
    assert parsed == TriggeredAbility(
        event=Event.DIES,
        predicate=IsSelf() & Subject(None, IsCreature()),
        effect=EnduringDiesReturnAsEnchantmentEffect(),
    )


def test_parse_enduring_intervening_if_unknown_gate_rejects():
    # A near-miss intervening-if line whose gate phrase isn't in the
    # phrasebook rejects outright — the don't-silently-drop-the-gate
    # discipline is preserved. Belt-and-suspenders that adding the
    # ``"it was a creature"`` entry didn't widen the gate dispatcher
    # to accept arbitrary phrases.
    line = (
        "When this creature dies, if there's a moon, "
        "return it to the battlefield under its owner's control. "
        "It's an enchantment."
    )
    assert _parse_triggered_ability_line(line) is None


# ---------- Effect resolution semantics ----------










    # Token stays in the graveyard pile (the engine's separate token
    # cleanup will remove it; the Enduring effect just declines to
    # touch it).


def test_enduring_effect_describe():
    # The describe output spells out the card's printed return-as-
    # enchantment body so log lines are readable. Lowercase to match
    # the convention of every other effect's describe in this engine.
    eff = EnduringDiesReturnAsEnchantmentEffect()
    assert eff.describe() == (
        "return it to the battlefield under its owner's control. "
        "it's an enchantment"
    )


# ---------- ``enduring_returned_as_enchantment`` flag semantics ----------






# ---------- end-to-end claim ----------


def test_enduring_curiosity_claimed_by_parser():
    # Enduring Curiosity ``{2}{U}{U}`` 4/3 Enchantment Creature — Cat
    # Glimmer. Three oracle lines:
    #
    # 1. ``Flash`` (existing keyword grammar)
    # 2. ``Whenever a creature you control deals combat damage to a
    #    player, draw a card.`` (existing predicate-trigger phrase)
    # 3. ``When Enduring Curiosity dies, if it was a creature, return
    #    it to the battlefield under its owner's control. It's an
    #    enchantment. (It's not a creature.)`` (NEW intervening-if
    #    gate phrase + NEW effect-phrase regex feeding the new
    #    ``EnduringDiesReturnAsEnchantmentEffect`` engine class)
    #
    # Every Enduring printing carries the same dies trigger; Curiosity
    # is the one whose other lines are already supported by the
    # parser, so it's the cycle's end-to-end claim on this branch.
    # The other four cards (Innocence, Tenacity, Courage, Vitality)
    # stay unclaimed pending grammar additions outside the Enduring
    # cycle's scope.
    assert "Enduring Curiosity" in _claimed_names()


def test_enduring_curiosity_carries_dies_trigger():
    # Belt-and-suspenders that the claimed printing actually carries
    # the Enduring dies trigger with the gate predicate composed onto
    # the IsSelf base. Otherwise the engine wouldn't return the card
    # as an enchantment when it dies.
    src = OracleParserCardSource()
    p = src.get_printings()["Enduring Curiosity"]
    dies = [
        ab for ab in p.triggered_abilities
        if ab.event == Event.DIES
    ]
    assert len(dies) == 1
    assert dies[0] == TriggeredAbility(
        event=Event.DIES,
        predicate=IsSelf() & Subject(None, IsCreature()),
        effect=EnduringDiesReturnAsEnchantmentEffect(),
    )


def test_enduring_curiosity_carries_combat_damage_trigger():
    # Belt-and-suspenders that the claimed printing carries the
    # printed non-dies trigger (combat damage to a player → draw a
    # card). Otherwise the parser would have silently dropped a
    # printed clause, which would violate the parser-is-right rule.
    src = OracleParserCardSource()
    p = src.get_printings()["Enduring Curiosity"]
    cd = [
        ab for ab in p.triggered_abilities
        if ab.event == Event.DEALS_COMBAT_DAMAGE
    ]
    assert len(cd) == 1
    assert cd[0].effect == DrawCards(amount=1)
    # Predicate composes "creature you control source" + "player target".
    assert cd[0].predicate == (
        Subject("source", IsCreature())
        & Controlled(YOU, subject="source")
        & Subject("target", IsPlayer())
    )


def test_enduring_curiosity_carries_flash_keyword():
    # The Flash keyword line is a separate clause from the trigger
    # bodies; verify it survives end-to-end so the engine surfaces
    # the printing at instant speed.
    src = OracleParserCardSource()
    p = src.get_printings()["Enduring Curiosity"]
    assert "flash" in p.keywords


# ---------- engine path: dies dispatch fires the Enduring trigger ----------




