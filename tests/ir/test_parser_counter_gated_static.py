"""Tests for the counter-gated anthem static-ability grammar.

The counter-gated anthem matches lines of the shape ``Each creature
you control with a +1/+1 counter on it has <keyword>.`` and produces
a ``StaticAbility`` with
``StaticScope.CREATURES_YOU_CONTROL_WITH_COUNTER``. The new EoE card
unlocked end-to-end by this grammar is **Drix Fatemaker**, whose
single static line is exactly this shape.

The grammar is intentionally narrow: only the singular subject
``Each creature you control`` (with the fixed ``with a +1/+1 counter
on it`` filter) and a single keyword grant are accepted. P/T grants
and the plural-anthem subject would require their own surface forms,
neither of which appears on any EoE card today.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import StaticAbility, StaticScope
from mtgz_ir.cards.parser.statics import (
    _COUNTER_GATED_ANTHEM_LINE,
    _parse_counter_gated_anthem_line,
    _parse_static_ability_line,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.modifiers import ContinuousModifier, GrantKeyword


def _expected_counter_gated_static(keyword: str) -> tuple[StaticAbility, ...]:
    """Common expected-shape helper. Counter-gated anthems migrate to a
    layer-6 ContinuousModifier(GrantKeyword) carried on a single
    StaticAbility — same shape the parser returns for keyword-only
    anthem lines, just with a different scope."""
    return (
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL_WITH_COUNTER,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword(keyword),
            ),
        ),
    )


# ---------- Happy paths ----------


def test_drix_fatemaker_line_parses_to_counter_gated_static():
    # Drix's exact oracle line; the parsed tuple must match the new
    # ContinuousModifier shape for the structural parity gate to admit
    # the card.
    parsed = _parse_static_ability_line(
        "Each creature you control with a +1/+1 counter on it has trample."
    )
    assert parsed == _expected_counter_gated_static("trample")


def test_counter_gated_grammar_accepts_other_evergreens():
    # The grammar is keyword-agnostic: any evergreen in
    # ``_KEYWORD_LINES`` should round-trip. Pinned with a different
    # keyword to document that the regex doesn't hardcode "trample".
    parsed = _parse_static_ability_line(
        "Each creature you control with a +1/+1 counter on it has flying."
    )
    assert parsed == _expected_counter_gated_static("flying")


def test_counter_gated_grammar_accepts_multiword_keyword():
    # Multi-word evergreens (``first strike``, ``double strike``) round-
    # trip through ``str.capitalize()`` to find their ``_KEYWORD_LINES``
    # entry — same convention as the plural-anthem grammar.
    parsed = _parse_static_ability_line(
        "Each creature you control with a +1/+1 counter on it has "
        "first strike."
    )
    assert parsed == _expected_counter_gated_static("first strike")


def test_counter_gated_helper_callable_directly():
    # The dispatcher calls the helper as a final fallback after the
    # plural-anthem grammar; pinning the direct call documents that
    # the helper is a stable entry point inside the module.
    parsed = _parse_counter_gated_anthem_line(
        "Each creature you control with a +1/+1 counter on it has trample."
    )
    assert parsed == _expected_counter_gated_static("trample")


# ---------- Regex sanity ----------


def test_regex_extracts_keyword_named_group():
    # The compiled regex must expose the ``keyword`` group the parse
    # function reads; a regression in the regex shape would silently
    # change the ``StaticAbility`` produced.
    m = _COUNTER_GATED_ANTHEM_LINE.match(
        "Each creature you control with a +1/+1 counter on it has trample."
    )
    assert m is not None
    assert m.group("keyword") == "trample"


# ---------- Negatives ----------


def test_unknown_keyword_returns_empty_tuple():
    # A non-evergreen keyword fails the parse rather than silently
    # producing a static with no engine effect — same conservative
    # policy as the plural-anthem grammar.
    assert (
        _parse_static_ability_line(
            "Each creature you control with a +1/+1 counter on it has "
            "scry."
        )
        == ()
    )


def test_missing_period_returns_empty_tuple():
    # The line anchor requires a terminating period; an oracle line
    # without one is malformed and must not match.
    assert (
        _parse_static_ability_line(
            "Each creature you control with a +1/+1 counter on it has "
            "trample"
        )
        == ()
    )


def test_plural_subject_returns_empty_tuple():
    # The grammar is pinned to the singular ``Each creature`` subject.
    # The plural ``Creatures you control with a +1/+1 counter on them``
    # form does not appear on any EoE card today and is not handled.
    # Falling through here is intentional — adding it speculatively
    # without an example to anchor the surface spelling would risk
    # mis-modeling a future printing.
    assert (
        _parse_static_ability_line(
            "Creatures you control with a +1/+1 counter on them have "
            "trample."
        )
        == ()
    )


def test_different_counter_kind_returns_empty_tuple():
    # The regex hard-codes ``+1/+1`` counters. Other counter kinds
    # (charge, loyalty, stun, etc.) need their own scope and grammar;
    # a line about a different counter must not be silently mapped to
    # the +1/+1 scope.
    assert (
        _parse_static_ability_line(
            "Each creature you control with a charge counter on it has "
            "trample."
        )
        == ()
    )


def test_pt_grant_in_counter_gated_form_returns_empty_tuple():
    # P/T grants in the counter-gated shape would need their own
    # grammar branch; today only keyword grants are accepted. A line
    # that smuggles a ``get +N/+N`` body into the counter-gated subject
    # must therefore not match.
    assert (
        _parse_static_ability_line(
            "Each creature you control with a +1/+1 counter on it gets "
            "+1/+1."
        )
        == ()
    )


# ---------- End-to-end claim ----------


def test_drix_fatemaker_is_claimed_end_to_end():
    # Drix Fatemaker is the EoE card the counter-gated anthem grammar
    # unlocks. Parser must claim it through the full builder pipeline;
    # structural parity vs. the static registry is verified separately
    # by ``test_parser_parity.py``.
    claimed = OracleParserCardSource().claimed_names
    assert "Drix Fatemaker" in claimed
