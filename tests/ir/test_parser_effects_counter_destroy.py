"""Tests for the ``put a +1/+1 counter on this creature`` and
``destroy target creature`` effects in the unified effect grammar.

Each effect is exercised three ways: directly via ``_parse_effect`` on
the normalized phrase, via ``_parse_effect_phrase`` (creature-trigger
surface form with ``it`` prefix), and via ``_parse_spell_effect_phrase``
(spell sentence form, capitalized first letter and trailing period).
This three-way coverage verifies the unified path works for both
contexts after a single grammar addition.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DestroyTargetCreature,
    PutPlusOneCounterOnSelf,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)


# ---------- put a +1/+1 counter on this creature ----------

def test_parse_effect_recognizes_put_plus_one_counter_on_this_creature():
    assert _parse_effect("put a +1/+1 counter on this creature") == \
        PutPlusOneCounterOnSelf(amount=1)


def test_parse_effect_phrase_recognizes_put_plus_one_counter_on_this_creature():
    # Creature-trigger surface: "put a +1/+1 counter on this creature"
    # is the second-half clause that follows e.g. "Whenever this
    # creature attacks," — no "it" subject (the controller does the
    # putting, not the creature itself).
    assert _parse_effect_phrase(
        "put a +1/+1 counter on this creature"
    ) == PutPlusOneCounterOnSelf(amount=1)


def test_parse_spell_effect_phrase_recognizes_put_plus_one_counter_on_this_creature():
    # Spell surface form: full sentence, leading capital, trailing period.
    assert _parse_spell_effect_phrase(
        "Put a +1/+1 counter on this creature.", "Bogus Spell"
    ) == PutPlusOneCounterOnSelf(amount=1)


# ---------- destroy target creature ----------

def test_parse_effect_recognizes_destroy_target_creature():
    assert _parse_effect("destroy target creature") == DestroyTargetCreature()


def test_parse_effect_phrase_recognizes_destroy_target_creature():
    # Creature-trigger surface form with the "it" pronoun stripped.
    assert _parse_effect_phrase("destroy target creature") == \
        DestroyTargetCreature()


def test_parse_spell_effect_phrase_recognizes_destroy_target_creature():
    # Spell surface form: capitalized + trailing period (Vote Out shape).
    assert _parse_spell_effect_phrase(
        "Destroy target creature.", "Vote Out"
    ) == DestroyTargetCreature()


def test_parse_spell_effect_phrase_destroy_strips_card_name_subject():
    # When the card-name subject prefix is present it must be stripped.
    assert _parse_spell_effect_phrase(
        "Vote Out destroy target creature.", "Vote Out"
    ) == DestroyTargetCreature()


# ---------- negative cases ----------

def test_parse_effect_returns_none_for_malformed_counter_phrase():
    # Wrong counter type, missing target qualifier, etc.
    assert _parse_effect("put a -1/-1 counter on this creature") is None
    assert _parse_effect("put a +1/+1 counter on") is None
    assert _parse_effect("put a +1/+1 counter on this") is None


def test_parse_effect_returns_none_for_malformed_destroy_phrase():
    # ``destroy target artifact`` was unsupported when this batch landed
    # but is recognized as of parser-effects-batch8 (extends ``destroy
    # target <type>`` to artifacts/lands/noncreature permanents). The
    # malformed-phrase shapes here keep the negative-case coverage
    # without overlapping the new grammar.
    assert _parse_effect("destroy target") is None
    assert _parse_effect("destroy target creature spell") is None
    assert _parse_effect("destroy target artifact creature") is None


def test_parse_effect_phrase_returns_none_for_malformed_phrase():
    assert _parse_effect_phrase("destroy something nonsense") is None


def test_parse_spell_effect_phrase_returns_none_for_malformed_phrase():
    assert _parse_spell_effect_phrase(
        "Destroy nothing in particular.", "Bogus"
    ) is None
