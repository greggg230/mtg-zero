"""Batch 10: ``it's your turn`` static-ability condition.

Adds ``it's your turn`` → ``active_turn_only=True`` to the
``_STATIC_CONDITION_KWARGS`` phrasebook so static-ability lines like
``As long as it's your turn, this creature gets +N/+N.`` (and the
trailing-shape sibling) parse into a ``StaticAbility`` with the
``active_turn_only`` engine gate set.

The engine gate already exists (Memorial Team Leader uses it via the
hand-built ``During your turn`` rider on an anthem scope), so this batch
is forward-looking grammar coverage for the ``As long as`` surface
form. No new EoE creature unlocks today — the only EoE oracle text in
the leading/trailing static shape uses the two pre-existing conditions
(``you control an artifact`` and ``you've cast two or more spells this
turn``). Adding the phrase keeps the parser ready for reprints / future
sets without lying about engine semantics.
"""

from __future__ import annotations

from mtgz_ir.cards.base import StaticAbility, StaticScope
from mtgz_ir.cards.parser.statics import (
    _STATIC_CONDITION_KWARGS,
    _parse_static_ability_line,
)
from mtgz_ir.effects.conditions import DuringYourTurn
from mtgz_ir.effects.modifiers import (
    ContinuousModifier,
    GrantKeyword,
    PowerToughnessBonus,
)


# ---------- "it's your turn" → active_turn_only=True ----------


def test_active_turn_leading_shape_parses_to_static():
    # PR-D continuous-effects shape: P/T contribution rides on a
    # layer-7c ContinuousModifier(PowerToughnessBonus); the legacy
    # active_turn_only gate stays as the surface today.
    parsed = _parse_static_ability_line(
        "As long as it's your turn, this creature gets +2/+0."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=2, toughness_bonus=0),
        ),
        condition=DuringYourTurn(),
    ),)


def test_active_turn_trailing_shape_parses_to_static():
    parsed = _parse_static_ability_line(
        "This creature gets +2/+0 as long as it's your turn."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=2, toughness_bonus=0),
        ),
        condition=DuringYourTurn(),
    ),)


def test_active_turn_leading_and_trailing_are_equivalent():
    leading = _parse_static_ability_line(
        "As long as it's your turn, this creature gets +1/+1."
    )
    trailing = _parse_static_ability_line(
        "This creature gets +1/+1 as long as it's your turn."
    )
    assert leading is not None
    assert trailing is not None
    assert leading == trailing


def test_active_turn_with_keyword_rider_leading():
    # Compound P/T + keyword statics fan out into one StaticAbility
    # for the layer-7c P/T contribution + one per keyword for the
    # layer-6 GrantKeyword.
    parsed = _parse_static_ability_line(
        "As long as it's your turn, this creature gets +1/+0 and has "
        "flying."
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
            condition=DuringYourTurn(),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("flying"),
            ),
            condition=DuringYourTurn(),
        ),
    )


def test_active_turn_with_keyword_rider_trailing():
    parsed = _parse_static_ability_line(
        "This creature gets +1/+0 and has deathtouch as long as it's "
        "your turn."
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
            condition=DuringYourTurn(),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("deathtouch"),
            ),
            condition=DuringYourTurn(),
        ),
    )


def test_active_turn_with_multiword_keyword_rider():
    # Multi-word evergreen keyword (``first strike``) round-trips
    # through ``str.capitalize()`` to find the ``_KEYWORD_LINES`` entry,
    # exactly like the other condition phrases do.
    parsed = _parse_static_ability_line(
        "As long as it's your turn, this creature gets +1/+1 and has "
        "first strike."
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
            condition=DuringYourTurn(),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("first strike"),
            ),
            condition=DuringYourTurn(),
        ),
    )


def test_active_turn_does_not_set_other_gates():
    # A condition must set its own gate field and leave every other
    # gate at its default. Otherwise two conditions would conflict if
    # they ever appeared on the same printing.
    parsed = _parse_static_ability_line(
        "As long as it's your turn, this creature gets +1/+1."
    )
    assert len(parsed) == 1
    static = parsed[0]
    assert static.condition == DuringYourTurn()
    assert static.tier_threshold == 0


def test_active_turn_rejects_unknown_keyword_rider():
    # Same conservative behavior as the other conditions: a
    # non-evergreen keyword in the ``and has`` tail fails the parse
    # rather than silently dropping the rider.
    assert (
        _parse_static_ability_line(
            "As long as it's your turn, this creature gets +1/+0 and "
            "has scry."
        )
        == ()
    )


# ---------- Negative: similar-shaped lines that DON'T match ----------


def test_during_your_turn_surface_form_returns_none():
    # ``During your turn, ...`` is a different surface form (no
    # ``As long as``); the static-line regex does not match it. The
    # parser must fall through so the card builder either picks a
    # different path or skips the card.
    assert (
        _parse_static_ability_line(
            "During your turn, this creature gets +1/+0."
        )
        == ()
    )


def test_active_turn_opponent_form_returns_none():
    # ``it's an opponent's turn`` is a plausible English variant but
    # has no engine gate (and would be the negation of
    # ``active_turn_only``); it must not match.
    assert (
        _parse_static_ability_line(
            "As long as it's an opponent's turn, this creature gets "
            "+1/+0."
        )
        == ()
    )


def test_active_turn_capitalization_drift_returns_none():
    # The phrasebook is matched verbatim — capitalization drift
    # (``It's your turn`` instead of ``it's your turn``) inside an
    # ``As long as`` clause won't appear in actual oracle text but the
    # parser pinning the case-sensitive lookup is intentional.
    assert (
        _parse_static_ability_line(
            "As long as It's your turn, this creature gets +1/+0."
        )
        == ()
    )


# ---------- Phrasebook integrity ----------


def test_active_turn_phrase_pinned_in_phrasebook():
    # Pin the new entry so an accidental revert is loud.
    assert "it's your turn" in _STATIC_CONDITION_KWARGS
    assert _STATIC_CONDITION_KWARGS["it's your turn"] == {
        "condition": DuringYourTurn(),
    }
