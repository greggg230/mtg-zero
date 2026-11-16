"""Tests for ability-word-prefixed triggered abilities.

Ability words (Landfall, Magecraft, Constellation, Domain, …) are
italicized labels with no rules consequence — the substance is the
``Whenever …`` clause that follows the em-dash. The parser strips
any ``<Capitalized Words> — `` prefix before parsing the body
through the normal triggered-ability grammar; we never enumerate
ability words individually. This file covers the stripping behavior
plus the Landfall trigger-key mapping that lets Eumidian
Terrabotanist claim end-to-end.
"""

from __future__ import annotations

from mtgz_ir.cards.base import GainLife, TriggeredAbility
from mtgz_ir.cards.parser.triggers import (
    _parse_triggered_ability_line,
    _strip_ability_word_prefix,
)
from mtgz_ir.effects.object_predicates import (
    IsLand,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    Subject,
    YOU,
)
from _etb_helpers import _legacy_or_inferred_key, _matches_legacy_key


_LANDFALL_PRED = Subject(None, IsLand()) & Controlled(YOU)


# ---------- Happy path: Landfall-prefixed gain-life trigger ----------

def test_landfall_prefix_parses_to_landfall_trigger():
    parsed = _parse_triggered_ability_line(
        "Landfall — Whenever a land you control enters, "
        "you gain 1 life."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_LANDFALL_PRED,
        effect=GainLife(amount=1),
    )


def test_landfall_prefix_uses_engine_landfall_trigger_key():
    parsed = _parse_triggered_ability_line(
        "Landfall — Whenever a land you control enters, "
        "you gain 1 life."
    )
    assert parsed is not None
    # Phase-2 migration: every landfall trigger now sits on the
    # predicate path (``event=ENTERS, predicate=IsLand() & Controlled
    # (YOU)``). The legacy string-key inference helper recognizes the
    # equivalence.
    assert _matches_legacy_key(parsed, "landfall")


def test_landfall_prefix_extracts_effect_via_unified_grammar():
    # The effect side is parsed by the existing unified effect grammar
    # — varying the gain-life amount must flow through unchanged.
    parsed = _parse_triggered_ability_line(
        "Landfall — Whenever a land you control enters, "
        "you gain 3 life."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_LANDFALL_PRED,
        effect=GainLife(amount=3),
    )


# ---------- Negative: malformed Landfall lines return None ----------

def test_landfall_prefix_with_unsupported_inner_effect_returns_none():
    # The inner effect ``you frobnicate the orb`` is not a recognized
    # effect phrase. The Landfall prefix must not paper over an
    # unrecognized body — the line still returns ``None`` so the
    # enclosing card is skipped.
    parsed = _parse_triggered_ability_line(
        "Landfall — Whenever a land you control enters, "
        "you frobnicate the orb."
    )
    assert parsed is None


def test_landfall_prefix_without_trigger_body_returns_none():
    # A bare ``Landfall — `` prefix with no ``Whenever …`` body is
    # malformed. After stripping the prefix the remainder doesn't match
    # the triggered-ability line pattern, so the parser returns
    # ``None`` and skips the card.
    parsed = _parse_triggered_ability_line(
        "Landfall — you gain 1 life."
    )
    assert parsed is None


# ---------- Generic ability-word stripping ----------

def test_any_ability_word_prefix_is_stripped_generically():
    # Ability words are labels with no rules consequence, so the
    # parser doesn't enumerate them individually — any
    # ``<Capitalized Words> — Whenever <body>`` strips cleanly. Saga,
    # Magecraft, Constellation, etc. all flow through the same path.
    for label in ("Saga", "Magecraft", "Constellation", "Domain"):
        parsed = _parse_triggered_ability_line(
            f"{label} — Whenever a land you control enters, "
            "you gain 1 life."
        )
        assert parsed == TriggeredAbility(
            event=Event.ENTERS, predicate=_LANDFALL_PRED,
            effect=GainLife(amount=1),
        )


# ---------- _strip_ability_word_prefix unit tests ----------

def test_strip_ability_word_prefix_strips_landfall_label():
    stripped = _strip_ability_word_prefix(
        "Landfall — Whenever a land you control enters, "
        "you gain 1 life."
    )
    assert stripped == (
        "Whenever a land you control enters, you gain 1 life."
    )


def test_strip_ability_word_prefix_strips_any_label_before_whenever():
    # Generic strip — ``<Capitalized Words> — `` followed by
    # ``Whenever`` is removed regardless of the label content.
    assert _strip_ability_word_prefix(
        "Magecraft — Whenever a land you control enters, "
        "you gain 1 life."
    ) == "Whenever a land you control enters, you gain 1 life."


def test_strip_ability_word_prefix_passes_through_plain_line():
    # Lines without any ability-word-prefix label are returned unchanged.
    line = "When this creature enters, it deals 3 damage to any target."
    assert _strip_ability_word_prefix(line) == line


def test_strip_ability_word_prefix_does_not_strip_type_line_dash():
    # Type lines use the same em-dash separator (``Creature — Dragon``)
    # but never have ``When``/``Whenever`` after the dash, so the
    # lookahead in the strip pattern keeps them untouched.
    assert _strip_ability_word_prefix("Creature — Dragon") == \
        "Creature — Dragon"


def test_strip_ability_word_prefix_does_not_strip_non_when_bodies():
    # Saga chapters use roman-numeral notation (``Saga — I — Draw a
    # card.``) — no ``Whenever`` body, so the prefix is preserved.
    line = "Saga — I, II, III — Draw a card."
    assert _strip_ability_word_prefix(line) == line


# ---------- Bare condition (no prefix) is also recognized ----------

def test_bare_landfall_condition_still_parses():
    # The ``a land you control enters`` condition phrase maps directly
    # to the ``landfall`` trigger key. Even without the keyword-prefix
    # label, a line like ``Whenever a land you control enters, …``
    # parses to a landfall trigger. (Engine semantics are identical;
    # the prefix is reminder text.) No real EoE card uses this surface
    # form, but the recognizer is consistent with the unified
    # condition-phrase mechanism.
    parsed = _parse_triggered_ability_line(
        "Whenever a land you control enters, you gain 1 life."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_LANDFALL_PRED,
        effect=GainLife(amount=1),
    )


# ---------- End-to-end: Eumidian Terrabotanist is claimed ----------

