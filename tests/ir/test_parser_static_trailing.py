"""Tests for the trailing-shape ``As long as`` static-ability grammar.

The static-ability parser supports two surface forms of the same
semantics:

* **Leading**: ``As long as <condition>, this creature gets +N/+N[ and
  has <keyword>].``
* **Trailing**: ``This creature gets +N/+N[ and has <keyword>] as long
  as <condition>.``

The trailing shape unlocks Brightspear Zealot in EoE, whose oracle
text reads ``Vigilance\\nThis creature gets +2/+0 as long as you've
cast two or more spells this turn.``. Both shapes route through the
same condition phrasebook and must produce identical ``StaticAbility``
outputs for the same condition.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import StaticAbility, StaticScope
from mtgz_ir.cards.parser.statics import (
    _STATIC_ABILITY_LINE_TRAILING,
    _parse_static_ability_line,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.conditions import (
    AtLeastNSpellsCastThisTurn,
    ControlPermanent,
    DuringYourTurn,
)
from mtgz_ir.effects.modifiers import (
    ContinuousModifier,
    GrantKeyword,
    PowerToughnessBonus,
)
from mtgz_ir.effects.object_predicates import IsArtifact


# ---------- Trailing-shape happy paths ----------


def test_trailing_shape_spells_cast_parses_to_static():
    # The exact Brightspear Zealot oracle line. PR-D continuous-effects
    # shape: P/T contribution rides on a layer-7c
    # ContinuousModifier(PowerToughnessBonus); the legacy
    # requires_spells_cast_this_turn gate stays as the surface today.
    parsed = _parse_static_ability_line(
        "This creature gets +2/+0 as long as you've cast two or more "
        "spells this turn."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=2, toughness_bonus=0),
        ),
        condition=AtLeastNSpellsCastThisTurn(2),
    ),)


def test_trailing_shape_artifact_parses_to_static():
    # Re-uses the original artifact condition under the new shape.
    parsed = _parse_static_ability_line(
        "This creature gets +1/+0 as long as you control an artifact."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=0),
        ),
        condition=ControlPermanent(IsArtifact()),
    ),)


def test_trailing_shape_with_keyword_rider():
    # The ``and has <keyword>`` tail sits between the P/T modifier and
    # the ``as long as`` clause in the trailing shape. Compound P/T +
    # keyword statics fan out: one StaticAbility for the layer-7c P/T
    # contribution + one per keyword for the layer-6 GrantKeyword.
    parsed = _parse_static_ability_line(
        "This creature gets +1/+0 and has deathtouch as long as you "
        "control an artifact."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(
                    power_bonus=1, toughness_bonus=0,
                ),
            ),
            condition=ControlPermanent(IsArtifact()),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("deathtouch"),
            ),
            condition=ControlPermanent(IsArtifact()),
        ),
    )


def test_trailing_shape_with_multiword_keyword_rider():
    # Multi-word evergreen keywords (e.g. "first strike") must
    # round-trip through ``str.capitalize()`` to find their
    # ``_KEYWORD_LINES`` entry.
    parsed = _parse_static_ability_line(
        "This creature gets +1/+1 and has first strike as long as "
        "you've cast two or more spells this turn."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(
                    power_bonus=1, toughness_bonus=1,
                ),
            ),
            condition=AtLeastNSpellsCastThisTurn(2),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("first strike"),
            ),
            condition=AtLeastNSpellsCastThisTurn(2),
        ),
    )


# ---------- Leading vs. trailing equivalence ----------


@pytest.mark.parametrize(
    "leading,trailing",
    [
        (
            "As long as you control an artifact, this creature gets "
            "+1/+0.",
            "This creature gets +1/+0 as long as you control an "
            "artifact.",
        ),
        (
            "As long as you've cast two or more spells this turn, this "
            "creature gets +2/+0.",
            "This creature gets +2/+0 as long as you've cast two or "
            "more spells this turn.",
        ),
        (
            "As long as you control an artifact, this creature gets "
            "+1/+0 and has deathtouch.",
            "This creature gets +1/+0 and has deathtouch as long as "
            "you control an artifact.",
        ),
        (
            "As long as you've cast two or more spells this turn, this "
            "creature gets +1/+1 and has flying.",
            "This creature gets +1/+1 and has flying as long as you've "
            "cast two or more spells this turn.",
        ),
    ],
)
def test_leading_and_trailing_shapes_are_equivalent(leading, trailing):
    # The two surface forms are semantically identical; the parser
    # must produce identical ``StaticAbility`` outputs regardless of
    # which shape the oracle text uses.
    a = _parse_static_ability_line(leading)
    b = _parse_static_ability_line(trailing)
    assert a != ()
    assert b != ()
    assert a == b


# ---------- Trailing-shape negatives ----------


def test_trailing_shape_active_turn_parses_to_static():
    # ``it's your turn`` maps to ``active_turn_only=True`` and rides
    # through the trailing shape the same way it rides through the
    # leading shape — both surface forms are semantically identical.
    parsed = _parse_static_ability_line(
        "This creature gets +1/+1 as long as it's your turn."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=1),
        ),
        condition=DuringYourTurn(),
    ),)


def test_trailing_shape_unknown_condition_returns_none():
    # Conditions with no matching ``StaticAbility`` gate field must
    # fall through even when the surface shape matches — better to
    # skip the card than silently mis-model it as always-on.
    assert (
        _parse_static_ability_line(
            "This creature gets +1/+1 as long as you control a creature."
        )
        == ()
    )


def test_trailing_shape_unknown_keyword_returns_none():
    # Same conservative behavior as the leading shape: a non-evergreen
    # keyword in the ``and has`` tail fails the parse rather than
    # silently dropping the rider.
    assert (
        _parse_static_ability_line(
            "This creature gets +1/+0 and has scry as long as you "
            "control an artifact."
        )
        == ()
    )


def test_trailing_shape_missing_period_returns_none():
    # The line anchor requires a terminating period; an oracle line
    # without one is malformed and must not match.
    assert (
        _parse_static_ability_line(
            "This creature gets +2/+0 as long as you've cast two or "
            "more spells this turn"
        )
        == ()
    )


def test_trailing_shape_missing_pt_returns_none():
    # The trailing regex requires ``+N/+N`` after ``gets``; a line
    # without it (e.g. just keyword grant) must not match the static
    # parser. A different parser may pick it up.
    assert (
        _parse_static_ability_line(
            "This creature gets flying as long as you control an "
            "artifact."
        )
        == ()
    )


def test_trailing_shape_with_negative_pt_returns_none():
    # The regex only accepts ``+N/+N`` modifiers; negative modifiers
    # (a different game effect) must not match.
    assert (
        _parse_static_ability_line(
            "This creature gets -1/-1 as long as you control an "
            "artifact."
        )
        == ()
    )


def test_trailing_shape_wrong_subject_returns_none():
    # The subject must be ``this creature`` (matching ``StaticScope.SELF``).
    # A different subject like ``target creature`` is not a static-self
    # ability and must not match.
    assert (
        _parse_static_ability_line(
            "Target creature gets +1/+0 as long as you control an "
            "artifact."
        )
        == ()
    )


# ---------- Direct regex sanity ----------


def test_trailing_regex_extracts_named_groups():
    # The compiled trailing regex must expose the same four named
    # groups (power, toughness, keyword, condition) as the leading
    # regex so the shared parse function can read either match by
    # name.
    m = _STATIC_ABILITY_LINE_TRAILING.match(
        "This creature gets +3/+2 and has trample as long as you "
        "control an artifact."
    )
    assert m is not None
    assert m.group("power") == "3"
    assert m.group("toughness") == "2"
    assert m.group("keyword") == "trample"
    assert m.group("condition") == "you control an artifact"


def test_trailing_regex_optional_keyword_group_is_none():
    # When the ``and has <keyword>`` tail is absent the keyword group
    # must be ``None`` so the parse function falls through to the
    # empty-frozenset default.
    m = _STATIC_ABILITY_LINE_TRAILING.match(
        "This creature gets +2/+0 as long as you've cast two or more "
        "spells this turn."
    )
    assert m is not None
    assert m.group("keyword") is None


# ---------- End-to-end claim ----------


def test_brightspear_zealot_is_claimed_end_to_end():
    # Brightspear Zealot is the EoE card the trailing shape unlocks.
    # The parser must claim it through the full builder pipeline and
    # the result must match the static registry's printing exactly
    # (parity is verified separately by ``test_parser_parity.py``,
    # but we pin the claim itself here to make a regression in the
    # trailing-shape regex loud).
    claimed = OracleParserCardSource().claimed_names
    assert "Brightspear Zealot" in claimed
