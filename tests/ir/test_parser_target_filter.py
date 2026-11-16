"""Tests for the parser-side target-phrase phrasebook
(``cards/parser/target_filter.py``).
"""

from __future__ import annotations

from mtgz_ir.cards.parser.target_filter import (
    _TARGET_PHRASE_TABLE,
    parse_target_phrase,
)
from mtgz_ir.effects.object_predicates import (
    IsAttacking,
    IsBlocking,
    IsCreature,
    IsTapped,
)
from mtgz_ir.effects.targets import any_target_filter


def test_parse_any_target():
    """``"any target"`` expands to the four-arm OneOf — CR 115.4."""
    assert parse_target_phrase("any target") == any_target_filter()


def test_parse_target_creature():
    assert parse_target_phrase("creature") == IsCreature()


def test_parse_target_tapped_creature():
    assert (
        parse_target_phrase("tapped creature")
        == IsCreature() & IsTapped()
    )


def test_parse_target_attacking_or_blocking_creature():
    """Focus Fire's filter: a creature that is currently attacking
    OR currently blocking."""
    assert parse_target_phrase("attacking or blocking creature") == (
        IsCreature() & (IsAttacking() | IsBlocking())
    )


def test_parse_unknown_phrase_returns_none():
    assert parse_target_phrase("dragon you control") is None


def test_parse_strips_whitespace():
    """Leading/trailing whitespace is tolerated — the parser
    pipeline strips before lookup but the helper is defensive."""
    assert parse_target_phrase("  creature  ") == IsCreature()


def test_phrasebook_table_is_dict():
    """Sanity: the table is a plain dict for parser-test
    introspection — no metaclass / property magic that'd surprise a
    caller iterating it."""
    assert isinstance(_TARGET_PHRASE_TABLE, dict)
    assert "creature" in _TARGET_PHRASE_TABLE
    assert "any target" in _TARGET_PHRASE_TABLE
