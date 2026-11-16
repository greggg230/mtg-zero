"""Multi-piece (specifically 3-piece) activation-cost parsing.

The activated-ability cost grammar previously rejected any cost with
more than two comma-separated pieces. EoE prints many ``{cost}:
<effect>.`` lines whose cost is exactly three pieces, e.g.

  * ``{2}, {T}, Sacrifice this artifact`` — Nutrient Block,
    Dubious Delicacy, All-Fates Scroll, Thaumaton Torpedo.
  * ``{1}, {T}, Sacrifice a Food`` — Ragost, Deft Gastronaut.
  * ``{2}, {T}, Sacrifice another artifact or land`` —
    Slagdrill Scrapper.
  * ``{1}{R}, {T}, Sacrifice a land`` — Kavaron, Memorial World
    (back face).

The parser now classifies each comma-separated piece into one of a
small set of atoms (mana / tap / sacrifice-self / pay-life / one of
the named-sacrifice flavors) and merges the bag of classified pieces
into a single ``ActivationCost``. The merge is order-insensitive and
rejects any bag with a duplicate kind, an unrecognized atom, or
4-or-more pieces (so 4-piece costs like Susur Secundi's
``{1}{B}, {T}, Pay 2 life, Sacrifice a creature`` still skip).

This file covers the new 3-piece behavior end-to-end. The
single- and two-piece paths still have their own dedicated test
files (``test_parser_activated_sacrifice.py``, ``test_parser_pay_life.py``,
``test_parser_mana_activations.py``) — only smoke-checked here when a
2-piece test exercises a code path that the 3-piece refactor changed.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    ActivationCost,
    DrawCards,
    GainLife,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import (
    _parse_activated_ability_line,
    _parse_activation_cost,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


# --- Three-piece costs: mana + tap + sacrifice-self -------------------------

def test_three_piece_mana_tap_sacrifice_self_creature():
    """``{2}, {T}, Sacrifice this creature`` — the canonical EoE shape
    with the source described as a creature (e.g. activated abilities
    on creatures whose oracle text references ``Sacrifice this
    creature``)."""
    cost = _parse_activation_cost("{2}, {T}, Sacrifice this creature")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_self=True,
    )


def test_three_piece_mana_tap_sacrifice_self_artifact():
    """``{2}, {T}, Sacrifice this artifact`` — the artifact wording
    variant, used by Nutrient Block, Dubious Delicacy, All-Fates
    Scroll, Thaumaton Torpedo. Same engine field
    (``sacrifice_self``)."""
    cost = _parse_activation_cost("{2}, {T}, Sacrifice this artifact")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_self=True,
    )


def test_three_piece_high_generic_mana_tap_sacrifice_self():
    """``{7}, {T}, Sacrifice this artifact`` — All-Fates Scroll's
    high-generic activation. Confirms the mana piece accepts
    multi-digit generic values."""
    cost = _parse_activation_cost("{7}, {T}, Sacrifice this artifact")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{7}"),
        mana_str="{7}",
        tap_self=True,
        sacrifice_self=True,
    )


def test_three_piece_colored_mana_tap_sacrifice_self():
    """``{1}{R}, {T}, Sacrifice this artifact`` — colored mana piece
    survives parse_mana_cost validation and lands in the resulting
    cost intact."""
    cost = _parse_activation_cost("{1}{R}, {T}, Sacrifice this artifact")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}{R}"),
        mana_str="{1}{R}",
        tap_self=True,
        sacrifice_self=True,
    )


# --- Three-piece costs: mana + tap + named-sacrifice ------------------------

def test_three_piece_mana_tap_sacrifice_a_food():
    """Ragost: ``{1}, {T}, Sacrifice a Food: Ragost deals 3 damage to
    each opponent.`` — exercises the ``sacrifice_food`` named-sac
    atom in a 3-piece bag."""
    cost = _parse_activation_cost("{1}, {T}, Sacrifice a Food")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}"),
        mana_str="{1}",
        tap_self=True,
        sacrifice_food=True,
    )


def test_three_piece_mana_tap_sacrifice_another_artifact_or_land():
    """Slagdrill Scrapper: ``{2}, {T}, Sacrifice another artifact or
    land: Draw a card.`` — exercises the
    ``sacrifice_another_artifact_or_land`` named-sac atom."""
    cost = _parse_activation_cost(
        "{2}, {T}, Sacrifice another artifact or land"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_another_artifact_or_land=True,
    )


def test_three_piece_mana_tap_sacrifice_another_artifact():
    """``{N}, {T}, Sacrifice another artifact`` — exercises the
    artifact-only named-sac atom in a 3-piece bag. EoE prints this
    shape on Memorial Vault's 2-piece cost; the 3-piece form is
    accepted by the bag merge for symmetry."""
    cost = _parse_activation_cost("{2}, {T}, Sacrifice another artifact")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_another_artifact=True,
    )


def test_three_piece_mana_tap_sacrifice_a_land():
    """Kavaron back face: ``{1}{R}, {T}, Sacrifice a land: Create a 2/2
    colorless Robot artifact creature token, then …`` — exercises the
    ``sacrifice_a_land`` named-sac atom."""
    cost = _parse_activation_cost("{1}{R}, {T}, Sacrifice a land")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}{R}"),
        mana_str="{1}{R}",
        tap_self=True,
        sacrifice_a_land=True,
    )


def test_three_piece_mana_tap_sacrifice_another_creature_or_artifact():
    """The Umbral-Collar-Zealot piece (``Sacrifice another creature or
    artifact``) composes cleanly with mana + tap into a 3-piece cost."""
    cost = _parse_activation_cost(
        "{1}, {T}, Sacrifice another creature or artifact"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}"),
        mana_str="{1}",
        tap_self=True,
        sacrifice_another_creature_or_artifact=True,
    )


def test_three_piece_mana_tap_sacrifice_a_desert():
    """``{N}, {T}, Sacrifice a Desert`` — Scavenger-Grounds-style atom.
    EoE doesn't print this 3-piece shape on a real card today; covered
    here to keep the named-sac classifier table fully exercised."""
    cost = _parse_activation_cost("{2}, {T}, Sacrifice a Desert")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_a_desert=True,
    )


# --- Three-piece costs: tap + pay-life + sacrifice (no mana) ----------------

def test_three_piece_tap_pay_life_sacrifice_self():
    """``{T}, Pay 2 life, Sacrifice this creature`` — the prompt-
    named hypothetical. No EoE card prints this exact 3-piece shape
    today (Susur Secundi's similar text has a mana piece too,
    bringing it to 4 pieces), but the bag-merge classifier accepts it
    for free."""
    cost = _parse_activation_cost(
        "{T}, Pay 2 life, Sacrifice this creature"
    )
    assert cost == ActivationCost(
        tap_self=True, pay_life=2, sacrifice_self=True
    )


def test_three_piece_mana_pay_life_sacrifice_self():
    """``{1}, Pay 1 life, Sacrifice this creature`` — synthetic mix of
    all three non-tap atoms; covered for grammar symmetry."""
    cost = _parse_activation_cost(
        "{1}, Pay 1 life, Sacrifice this creature"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}"),
        mana_str="{1}",
        pay_life=1,
        sacrifice_self=True,
    )


def test_three_piece_mana_tap_pay_life():
    """``{2}, {T}, Pay 1 life`` — three-piece without any sacrifice
    atom; mana + tap + pay-life."""
    cost = _parse_activation_cost("{2}, {T}, Pay 1 life")
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        pay_life=1,
    )


# --- Order-insensitivity ----------------------------------------------------

@pytest.mark.parametrize(
    "ordered",
    [
        "{2}, {T}, Sacrifice this artifact",
        "{2}, Sacrifice this artifact, {T}",
        "{T}, {2}, Sacrifice this artifact",
        "{T}, Sacrifice this artifact, {2}",
        "Sacrifice this artifact, {2}, {T}",
        "Sacrifice this artifact, {T}, {2}",
    ],
)
def test_three_piece_order_insensitive(ordered):
    """All six permutations of ``{2}`` + ``{T}`` + ``Sacrifice this
    artifact`` parse to the identical ``ActivationCost`` — bag merge
    is order-insensitive (matches MTG's cost-payment semantics, where
    the order pieces are listed in oracle text doesn't affect what
    you pay)."""
    expected = ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_self=True,
    )
    assert _parse_activation_cost(ordered) == expected


def test_three_piece_named_sac_order_insensitive():
    """Same order-insensitivity holds when the sacrifice piece is one
    of the named-sac flavors (Slagdrill Scrapper's exact shape)."""
    expected = ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_another_artifact_or_land=True,
    )
    assert _parse_activation_cost(
        "Sacrifice another artifact or land, {T}, {2}"
    ) == expected
    assert _parse_activation_cost(
        "{T}, Sacrifice another artifact or land, {2}"
    ) == expected


# --- Negative: 4+ piece costs are still out of scope ------------------------

def test_four_piece_cost_parses():
    """4-piece costs are accepted (Susur Secundi's
    ``{1}{B}, {T}, Pay 2 life, Sacrifice a creature`` is the EoE
    example); 5+ pieces stay out of scope."""
    c = _parse_activation_cost(
        "{1}{B}, {T}, Pay 2 life, Sacrifice this creature"
    )
    assert c is not None
    assert c.mana_str == "{1}{B}"
    assert c.tap_self is True
    assert c.pay_life == 2
    assert c.sacrifice_self is True
    s = _parse_activation_cost("{1}{B}, {T}, Pay 2 life, Sacrifice a creature")
    assert s is not None
    assert s.tap_self is True and s.pay_life == 2
    assert s.sacrifice_self is False
    assert s.sacrifice_a_permanent_type == "creature"


def test_five_piece_cost_returns_none():
    """5+ pieces — the cap is enforced by an explicit length check
    before classification, so even an all-otherwise-valid bag bails
    out."""
    assert _parse_activation_cost(
        "{1}, {T}, Pay 1 life, Sacrifice this creature, "
        "Sacrifice another artifact"
    ) is None


# --- Negative: unknown / duplicate / malformed pieces -----------------------

def test_three_piece_with_unknown_atom_returns_none():
    """One unrecognized piece (here ``Mill a card``, not yet a
    classified atom) makes the whole cost return None — partial
    parses would silently drop a real cost requirement and let the
    ability fire for free."""
    assert _parse_activation_cost(
        "{2}, {T}, Mill a card"
    ) is None


def test_three_piece_with_two_mana_pieces_returns_none():
    """Duplicate kind: two mana pieces in one cost is nonsensical and
    rejected by the bag-merge dedup check."""
    assert _parse_activation_cost(
        "{1}, {2}, Sacrifice this creature"
    ) is None


def test_three_piece_with_two_tap_pieces_returns_none():
    """Duplicate kind: two ``{T}`` pieces is rejected. (No real card
    prints this; the dedup is a guardrail against malformed input.)"""
    assert _parse_activation_cost(
        "{T}, {T}, Sacrifice this creature"
    ) is None


def test_three_piece_with_two_sacrifice_pieces_returns_none():
    """Two sacrifice flavors in one cost — rejected. A single
    activation cost has at most one sacrifice clause."""
    assert _parse_activation_cost(
        "{T}, Sacrifice this creature, Sacrifice a Food"
    ) is None


def test_three_piece_with_self_sac_and_named_sac_returns_none():
    """Same exclusion holds when one piece is sacrifice-self and the
    other is a named-sac (different ``ActivationCost`` fields, but
    still two sacrifice clauses)."""
    assert _parse_activation_cost(
        "{T}, Sacrifice this artifact, Sacrifice another artifact"
    ) is None


def test_three_piece_with_two_self_sac_pieces_returns_none():
    """Two sacrifice-self pieces (here both the artifact and creature
    wordings) — both classify as the same ``sacrifice_self`` atom and
    the dedup check rejects the duplicate."""
    assert _parse_activation_cost(
        "{T}, Sacrifice this creature, Sacrifice this artifact"
    ) is None


def test_empty_cost_string_returns_none():
    """Defensive: the activated-ability line regex never hands the
    cost helper an empty string (the colon-split would fail first),
    but the helper still bails cleanly on one — important if a future
    caller invokes ``_parse_activation_cost`` directly with stripped
    user input."""
    assert _parse_activation_cost("") is None
    assert _parse_activation_cost("   ") is None


def test_three_piece_with_two_pay_life_pieces_returns_none():
    """Duplicate pay-life — the dedup check on ``pay_life`` triggers
    even though the second integer would technically overwrite the
    first; the rejection is intentional."""
    assert _parse_activation_cost(
        "{T}, Pay 1 life, Pay 2 life"
    ) is None


def test_three_piece_with_malformed_mana_returns_none():
    """A piece that looks like mana but fails ``parse_mana_cost``
    (here ``{2 R}`` — invalid because ``2 R`` isn't a valid symbol)
    causes the classifier to return None for that piece, which
    propagates to a None overall cost."""
    assert _parse_activation_cost(
        "{2 R}, {T}, Sacrifice this creature"
    ) is None


def test_three_piece_pieces_must_be_trimmed_consistently():
    """Pieces are stripped after splitting on ``,``, so extra
    whitespace between pieces doesn't break recognition."""
    cost = _parse_activation_cost(
        "  {2}  ,  {T}  ,  Sacrifice this artifact  "
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_self=True,
    )


# --- Full activated-ability lines (cost + effect) ---------------------------

def test_three_piece_line_with_gain_life_effect():
    """End-to-end: Nutrient Block's exact activated ability — the
    headline real-card case for the 3-piece slice (the artifact body
    itself isn't claimable yet — no artifact builder — but the
    activated-ability LINE parses cleanly)."""
    ab = _parse_activated_ability_line(
        "{2}, {T}, Sacrifice this artifact: You gain 3 life."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_self=True,
    )
    assert ab.effect == GainLife(amount=3)
    assert ab.sorcery_speed is False


def test_three_piece_line_with_inline_sorcery_rider():
    """The inline ``Activate only as a sorcery.`` rider must compose
    with 3-piece costs the same way it composes with 1- and 2-piece
    costs — strip the rider, parse the remainder, mark sorcery_speed."""
    ab = _parse_activated_ability_line(
        "{2}, {T}, Sacrifice this artifact: You gain 3 life. "
        "Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.cost.tap_self is True
    assert ab.cost.sacrifice_self is True
    assert ab.cost.mana_str == "{2}"
    assert ab.sorcery_speed is True


def test_three_piece_line_with_unparseable_effect_returns_none():
    """A valid 3-piece cost paired with an effect the unified grammar
    doesn't support still bails out — no synthetic ability."""
    ab = _parse_activated_ability_line(
        "{2}, {T}, Sacrifice this artifact: Bend the rules of physics."
    )
    assert ab is None


# --- Card-level: Slagdrill Scrapper is now claimed end-to-end --------------

