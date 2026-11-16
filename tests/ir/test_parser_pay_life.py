"""Pay-life-cost activated-ability parsing.

Mana Confluence-style activations such as ``{T}, Pay 1 life: <effect>.``
include a literal ``Pay N life`` cost piece. This file covers the new
pay-life cost shapes added to ``_parse_activation_cost``:

  * Pure pay-life.
  * Tap + pay-life (either order).
  * Mana + pay-life (either order).

The unified effect grammar is reused unchanged via ``_parse_effect``,
so the effect-side coverage here just confirms the new cost branches
combine with an existing-grammar effect end-to-end. EoE doesn't have
any cards that bring a pure pay-life activation cost online today
(the only ``Pay N life`` lines are a warp cost and a 4-piece station
cost — both out of scope for this slice), so there's no card-level
"is now claimed" test in this file. Keeping the parser branches in
place anyway since the engine field already exists and pay-life is a
common cost shape we want supported the moment a real card lands.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivationCost,
    DrawCards,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import (
    _parse_activated_ability_line,
    _parse_activation_cost,
)


# --- _parse_activation_cost: pure pay-life ----------------------------------

def test_pure_pay_life_cost_parses():
    cost = _parse_activation_cost("Pay 1 life")
    assert cost == ActivationCost(pay_life=1)


def test_pure_pay_life_cost_parses_with_larger_amount():
    cost = _parse_activation_cost("Pay 2 life")
    assert cost == ActivationCost(pay_life=2)


def test_pure_pay_life_cost_parses_with_surrounding_whitespace():
    """The dispatcher hands us already-trimmed pieces, but the helper
    is documented to accept either form — exercise the strip path."""
    cost = _parse_activation_cost("  Pay 3 life  ")
    assert cost == ActivationCost(pay_life=3)


# --- _parse_activation_cost: tap + pay-life (either order) ------------------

def test_tap_then_pay_life_cost_parses():
    cost = _parse_activation_cost("{T}, Pay 1 life")
    assert cost == ActivationCost(tap_self=True, pay_life=1)


def test_pay_life_then_tap_cost_parses():
    cost = _parse_activation_cost("Pay 1 life, {T}")
    assert cost == ActivationCost(tap_self=True, pay_life=1)


# --- _parse_activation_cost: mana + pay-life (either order) -----------------

def test_mana_then_pay_life_cost_parses():
    cost = _parse_activation_cost("{2}, Pay 1 life")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        pay_life=1,
    )


def test_pay_life_then_mana_cost_parses():
    cost = _parse_activation_cost("Pay 2 life, {1}{R}")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}{R}"),
        mana_str="{1}{R}",
        pay_life=2,
    )


def test_colored_mana_plus_pay_life_cost_parses():
    cost = _parse_activation_cost("{B}{B}, Pay 3 life")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{B}{B}"),
        mana_str="{B}{B}",
        pay_life=3,
    )


# --- Negative / out-of-scope cases ------------------------------------------

def test_three_piece_tap_mana_pay_life_cost_now_parses():
    """Triple-cost lines (tap + mana + pay-life) are supported by the
    bag-merge classifier added in the multi-piece-costs slice; cross-
    link to ``tests/test_parser_multi_piece_costs.py`` for the full
    3-piece coverage."""
    assert _parse_activation_cost("{T}, {2}, Pay 1 life") == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        pay_life=1,
    )


def test_three_piece_tap_pay_life_sacrifice_cost_now_parses():
    """Tap + pay-life + sacrifice-self (the EoE Planet's 12+ shape
    minus the mana piece) is now supported by the bag-merge
    classifier."""
    assert _parse_activation_cost(
        "{T}, Pay 2 life, Sacrifice this creature"
    ) == ActivationCost(
        tap_self=True, pay_life=2, sacrifice_self=True
    )


def test_pay_life_zero_letters_returns_none():
    """``Pay life`` (no integer) doesn't match the literal recognizer
    and bails out."""
    assert _parse_activation_cost("Pay life") is None


def test_pay_life_with_word_amount_returns_none():
    """The integer must be a digit run — ``Pay one life`` is not the
    canonical oracle phrasing and must not match."""
    assert _parse_activation_cost("Pay one life") is None


def test_pay_life_typo_returns_none():
    """``Pay 1 lyfe`` isn't ``Pay 1 life`` and must not match."""
    assert _parse_activation_cost("Pay 1 lyfe") is None


def test_pay_life_with_trailing_text_returns_none():
    """``Pay 1 life or whatever`` isn't a bare pay-life piece — the
    anchored regex rejects extra trailing text."""
    assert _parse_activation_cost("Pay 1 life or whatever") is None


def test_pay_life_paired_with_unknown_piece_returns_none():
    """``Pay 1 life, <something-not-tap-or-mana>`` falls through every
    two-piece branch (tap-pair, sac-pair, and mana+pay-life)."""
    assert (
        _parse_activation_cost("Pay 1 life, Mill a card") is None
    )


def test_pay_life_paired_with_invalid_mana_returns_none():
    """The mana piece in mana+pay-life must be a pure brace sequence —
    anything else (here a malformed brace expression) bails out."""
    assert _parse_activation_cost("Pay 1 life, {2 R}") is None


def test_tap_paired_with_invalid_mana_still_returns_none():
    """Regression: keep the existing tap+mana negative branch working —
    the new pay-life branch must not change the tap+mana fall-through."""
    assert _parse_activation_cost("{T}, {2 R}") is None


def test_two_piece_with_neither_tap_nor_sac_nor_pay_life_returns_none():
    """Two unrelated pieces that hit none of the recognized branches
    fall through the whole two-piece dispatcher and return ``None``.
    Exercises the final ``return None`` after the mana+pay-life branch."""
    assert (
        _parse_activation_cost("Discard a card, Reveal your hand")
        is None
    )


# --- Full activated-ability lines (cost + effect) ---------------------------

def test_pure_pay_life_line_with_draw_card_effect():
    ab = _parse_activated_ability_line("Pay 1 life: Draw a card.")
    assert ab is not None
    assert ab.cost == ActivationCost(pay_life=1)
    assert ab.effect == DrawCards(amount=1)
    assert ab.sorcery_speed is False


def test_tap_plus_pay_life_line_with_draw_card_effect():
    ab = _parse_activated_ability_line(
        "{T}, Pay 1 life: Draw a card."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(tap_self=True, pay_life=1)
    assert ab.effect == DrawCards(amount=1)


def test_mana_plus_pay_life_line_with_draw_card_effect():
    ab = _parse_activated_ability_line(
        "{2}, Pay 2 life: Draw a card."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        pay_life=2,
    )
    assert ab.effect == DrawCards(amount=1)


def test_pay_life_line_combines_with_inline_sorcery_rider():
    """The inline ``Activate only as a sorcery.`` rider must compose
    with pay-life costs the same way it composes with mana/tap/sac
    costs — strip the rider, parse the remainder, mark sorcery_speed."""
    ab = _parse_activated_ability_line(
        "{2}, Pay 1 life: Draw a card. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        pay_life=1,
    )
    assert ab.effect == DrawCards(amount=1)
    assert ab.sorcery_speed is True


def test_malformed_pay_life_line_does_not_match():
    """A pay-life cost paired with an effect the unified grammar
    doesn't support still bails out — no synthetic ability."""
    ab = _parse_activated_ability_line(
        "Pay 1 life: Bend the rules of physics."
    )
    assert ab is None


def test_pay_life_phrasing_typo_does_not_match():
    """A typoed pay-life phrase doesn't match the literal recognizer
    and bails out at the cost layer (returns ``None``)."""
    ab = _parse_activated_ability_line(
        "Pay 1 lyfe: Draw a card."
    )
    assert ab is None
