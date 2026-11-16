"""Inline ``Activate only as a sorcery.`` rider parsing.

EoE always glues the sorcery-speed rider to the same line as the
activation rather than putting it on its own line, e.g.::

    {3}: Put a +1/+1 counter on this creature. Activate only as a sorcery.

These tests exercise the inline-rider branch of
``_parse_activated_ability_line`` for the supported cost shapes
(pure mana, tap, tap+mana) and confirm the negative cases — malformed
rider, unsupported effect — still return ``None``.

The standalone-rider machinery (``_is_sorcery_rider_line``) is
unchanged and tested elsewhere; this file only covers the inline shape.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivationCost,
    DrawCards,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line


def test_inline_rider_pure_mana_cost_marks_sorcery_speed():
    ab = _parse_activated_ability_line(
        "{1}: Draw a card. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.sorcery_speed is True
    assert ab.effect == DrawCards(amount=1)
    assert ab.cost.mana_str == "{1}"
    assert ab.cost.mana == parse_mana_cost("{1}")
    assert ab.cost.tap_self is False


def test_inline_rider_effect_uses_unified_grammar():
    """The inline-rider branch must delegate to ``_parse_effect`` for
    the effect — same grammar as the no-rider branch — so any effect
    the unified grammar supports is automatically supported with the
    rider too."""
    without = _parse_activated_ability_line("{1}: Draw a card.")
    with_rider = _parse_activated_ability_line(
        "{1}: Draw a card. Activate only as a sorcery."
    )
    assert without is not None and with_rider is not None
    # Same effect, same cost — only the sorcery_speed flag differs.
    assert without.effect == with_rider.effect
    assert without.cost == with_rider.cost
    assert without.sorcery_speed is False
    assert with_rider.sorcery_speed is True


def test_inline_rider_tap_only_cost():
    ab = _parse_activated_ability_line(
        "{T}: Draw a card. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.sorcery_speed is True
    assert ab.effect == DrawCards(amount=1)
    assert ab.cost == ActivationCost(tap_self=True)


def test_inline_rider_tap_plus_mana_cost():
    ab = _parse_activated_ability_line(
        "{T}, {1}: Draw a card. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.sorcery_speed is True
    assert ab.effect == DrawCards(amount=1)
    assert ab.cost.tap_self is True
    assert ab.cost.mana_str == "{1}"
    assert ab.cost.mana == parse_mana_cost("{1}")


def test_inline_rider_mana_then_tap_cost():
    """Mana-then-tap order is also valid for the cost segment."""
    ab = _parse_activated_ability_line(
        "{1}, {T}: Draw a card. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.sorcery_speed is True
    assert ab.cost.tap_self is True
    assert ab.cost.mana_str == "{1}"


def test_malformed_rider_typo_does_not_match():
    """A typoed rider is treated as part of the effect text, so the
    line falls through the unified effect grammar and returns ``None``
    rather than a sorcery-speed ability."""
    ab = _parse_activated_ability_line(
        "{1}: Draw a card. Activate only as a sorecry."
    )
    assert ab is None


def test_malformed_rider_missing_period_does_not_match():
    """Without the trailing period the suffix doesn't match, so the
    rider isn't stripped. The remaining ``{cost}: <effect>.`` shape
    still won't parse because the effect text now includes the bare
    ``Activate only as a sorcery`` clause."""
    ab = _parse_activated_ability_line(
        "{1}: Draw a card. Activate only as a sorcery"
    )
    assert ab is None


def test_inline_rider_with_unsupported_effect_returns_none():
    """An inline rider over an effect the unified grammar doesn't
    recognize must still return ``None`` — we don't synthesize a
    sorcery-speed ability around an unparseable effect."""
    ab = _parse_activated_ability_line(
        "{1}: Bend the rules of physics. Activate only as a sorcery."
    )
    assert ab is None


def test_inline_rider_with_unsupported_cost_returns_none():
    """Cost-side failures still bail out even when the rider is
    present — an unrecognized cost atom rejects the line regardless of
    the rider."""
    ab = _parse_activated_ability_line(
        "Sacrifice a blue thing: Draw a card. Activate only as a sorcery."
    )
    assert ab is None


def test_inline_rider_does_not_break_no_rider_line():
    """Sanity check: a plain ``{cost}: <effect>.`` line with no rider
    still parses to a non-sorcery-speed ability (the rider branch is
    truly opt-in based on the trailing suffix)."""
    ab = _parse_activated_ability_line("{1}: Draw a card.")
    assert ab is not None
    assert ab.sorcery_speed is False
