"""Sacrifice-cost activated-ability parsing.

Cards like Illvoi Galeblade have ``{2}, Sacrifice this creature: Draw
a card.`` — the cost segment is two pieces, one of which is the
literal phrase ``Sacrifice this creature``. This file covers the new
sacrifice-self cost shapes added to ``_parse_activation_cost``:

  * Pure sacrifice-self.
  * Tap + sacrifice-self (either order).
  * Mana + sacrifice-self (either order).

The unified effect grammar is reused unchanged via ``_parse_effect``,
so the effect-side coverage here just confirms the new cost branches
combine with an existing-grammar effect end-to-end.
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
from mtgz_ir.cards.parser_source import OracleParserCardSource


# --- _parse_activation_cost: pure sacrifice ---------------------------------

def test_pure_sacrifice_self_cost_parses():
    cost = _parse_activation_cost("Sacrifice this creature")
    assert cost == ActivationCost(sacrifice_self=True)


def test_pure_sacrifice_self_cost_parses_with_surrounding_whitespace():
    """The dispatcher hands us already-trimmed pieces, but the helper
    is documented to accept either form — exercise the strip path."""
    cost = _parse_activation_cost("  Sacrifice this creature  ")
    assert cost == ActivationCost(sacrifice_self=True)


# --- _parse_activation_cost: tap + sacrifice (either order) -----------------

def test_tap_then_sacrifice_self_cost_parses():
    cost = _parse_activation_cost("{T}, Sacrifice this creature")
    assert cost == ActivationCost(tap_self=True, sacrifice_self=True)


def test_sacrifice_self_then_tap_cost_parses():
    cost = _parse_activation_cost("Sacrifice this creature, {T}")
    assert cost == ActivationCost(tap_self=True, sacrifice_self=True)


# --- _parse_activation_cost: mana + sacrifice (either order) ----------------

def test_mana_then_sacrifice_self_cost_parses():
    cost = _parse_activation_cost("{2}, Sacrifice this creature")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        sacrifice_self=True,
    )


def test_sacrifice_self_then_mana_cost_parses():
    cost = _parse_activation_cost("Sacrifice this creature, {1}{R}")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}{R}"),
        mana_str="{1}{R}",
        sacrifice_self=True,
    )


def test_colored_mana_plus_sacrifice_self_cost_parses():
    cost = _parse_activation_cost("{B}{B}, Sacrifice this creature")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{B}{B}"),
        mana_str="{B}{B}",
        sacrifice_self=True,
    )


# --- Negative / out-of-scope cases ------------------------------------------

def test_three_piece_tap_mana_sacrifice_cost_now_parses():
    """Triple-cost lines (tap + mana + sacrifice-self) are supported by
    the bag-merge classifier added in the multi-piece-costs slice;
    cross-link to ``tests/test_parser_multi_piece_costs.py`` for the
    full 3-piece coverage."""
    assert _parse_activation_cost("{T}, {2}, Sacrifice this creature") == (
        ActivationCost(
            mana=parse_mana_cost("{2}"),
            mana_str="{2}",
            tap_self=True,
            sacrifice_self=True,
        )
    )


def test_sacrifice_other_phrasing_returns_none():
    """Sacrifice phrasings that aren't a literal sacrifice-self or one
    of the recognized named-sacrifice atoms are unsupported.
    ``Sacrifice a creature`` is the card-type atom
    (``sacrifice_a_permanent_type``, Goblin Bombardment);
    ``Sacrifice another creature`` is a composable ``extra_costs`` atom
    since the cube creatures bucket (``SacrificePermanentsCost``; see
    test_cube_creatures_rmc_costs)."""
    assert _parse_activation_cost("Sacrifice a creature") == ActivationCost(
        sacrifice_a_permanent_type="creature",
    )
    assert _parse_activation_cost("Sacrifice another creature").extra_costs
    assert _parse_activation_cost("Sacrifice a blue thing") is None
    # ``Sacrifice this token`` only appears inside parenthesized
    # reminder text on token-creating cards; it isn't a legal piece
    # of an activation cost on a real card and isn't recognized here.
    assert _parse_activation_cost("Sacrifice this token") is None


def test_sacrifice_this_artifact_now_parses_as_sacrifice_self():
    """``Sacrifice this artifact`` is a wording variant of
    sacrifice-self for artifact sources (e.g. Nutrient Block,
    All-Fates Scroll). Both literals map to the same engine field
    ``ActivationCost.sacrifice_self`` since the source's card type
    determines how the engine resolves the sac."""
    assert _parse_activation_cost("Sacrifice this artifact") == (
        ActivationCost(sacrifice_self=True)
    )


def test_sacrifice_paired_with_unknown_piece_returns_none():
    """``Sacrifice this creature, <piece-that-doesn't-classify>`` is
    rejected because the unknown atom can't be classified by
    ``_classify_cost_piece`` — the bag-merge bails immediately."""
    # ``Pay 1 life`` IS a known atom now (pay-life), so use a string
    # that genuinely doesn't classify as anything.
    assert (
        _parse_activation_cost(
            "Sacrifice this creature, Mill a card"
        )
        is None
    )


def test_sacrifice_paired_with_invalid_mana_returns_none():
    """The mana piece in mana+sac must be a pure brace sequence —
    anything else (here a malformed brace expression) bails out."""
    assert (
        _parse_activation_cost("Sacrifice this creature, {2 R}") is None
    )


# --- Full activated-ability lines (cost + effect) ---------------------------

def test_pure_sacrifice_line_with_draw_card_effect():
    ab = _parse_activated_ability_line("Sacrifice this creature: Draw a card.")
    assert ab is not None
    assert ab.cost == ActivationCost(sacrifice_self=True)
    assert ab.effect == DrawCards(amount=1)
    assert ab.sorcery_speed is False


def test_tap_plus_sacrifice_line_with_draw_card_effect():
    ab = _parse_activated_ability_line(
        "{T}, Sacrifice this creature: Draw a card."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(tap_self=True, sacrifice_self=True)
    assert ab.effect == DrawCards(amount=1)


def test_mana_plus_sacrifice_line_with_draw_card_effect():
    """The Illvoi Galeblade activated ability — the headline real-card
    case for this slice."""
    ab = _parse_activated_ability_line(
        "{2}, Sacrifice this creature: Draw a card."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        sacrifice_self=True,
    )
    assert ab.effect == DrawCards(amount=1)


def test_sacrifice_line_combines_with_inline_sorcery_rider():
    """The inline ``Activate only as a sorcery.`` rider must compose
    with sacrifice-self costs the same way it composes with mana/tap
    costs — strip the rider, parse the remainder, mark sorcery_speed."""
    ab = _parse_activated_ability_line(
        "{2}, Sacrifice this creature: Draw a card. "
        "Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        sacrifice_self=True,
    )
    assert ab.effect == DrawCards(amount=1)
    assert ab.sorcery_speed is True


def test_malformed_sacrifice_line_does_not_match():
    """A sacrifice-style cost paired with an effect the unified grammar
    doesn't support still bails out — no synthetic ability."""
    ab = _parse_activated_ability_line(
        "Sacrifice this creature: Bend the rules of physics."
    )
    assert ab is None


def test_sacrifice_phrasing_typo_does_not_match():
    """A typoed sacrifice phrase doesn't match the literal recognizer
    and bails out at the cost layer (returns ``None``)."""
    ab = _parse_activated_ability_line(
        "Sacrafice this creature: Draw a card."
    )
    assert ab is None


# --- Card-level: Illvoi Galeblade is now claimed end-to-end -----------------

