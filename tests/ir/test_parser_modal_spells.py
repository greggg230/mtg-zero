"""Tests for the modal-spell ``Choose one —`` branch in
``mtgz_ir.cards.parser.spell_builder``.

The branch recognizes Scryfall's printed shape of a modal spell:

  ``Choose one —``  (em-dash, U+2014)
  ``• <mode 1 sentence>``
  ``• <mode 2 sentence>``
  ``• <mode 3 sentence>``  (optional, more allowed)

When every mode parses via the unified spell-effect grammar, the
clauses combine into a ``ModalChooseOne`` effect. If any mode line
fails to parse, the whole spell is skipped (returns ``None``) — a
partial modal effect would silently change the spell's playable
behavior.

Coverage targets:
  - Synthetic modal spell with two recognized modes parses to a
    ``ModalChooseOne`` over the right ordered modes.
  - Synthetic three-mode modal spell parses correctly (parser must
    not be hard-coded to two modes).
  - Synthetic modal spell with at least one unrecognized mode
    returns ``None``.
  - The ``Choose one or both`` shape (Scour for Scrap) is recognized
    (extended in a follow-up batch) and produces a ``ModalChooseOne``
    with ``or_both=True`` so the engine routes mode-pick through
    the agent's ``choose_modes`` (plural) hook. See
    ``tests/test_parser_modal_extensions.py`` for happy-path coverage.
  - The ``Choose two`` shape is NOT claimed by this branch — out of
    scope this batch.
  - A header without bullet lines (or with only one mode) is rejected.
  - A bullet line without a leading header is NOT silently treated as
    a Sequence clause and parsed in isolation — the existing Sequence
    fallback would reject it because the bullet prefix isn't a
    recognized spell-effect surface.
  - The end-to-end EoE pool: every real EoE ``Choose one`` modal
    spell either parses to a ``ModalChooseOne`` matching the static
    printing, or stays unclaimed (the unified grammar doesn't yet
    cover its modes). Today every real EoE modal stays unclaimed;
    this test acts as a tripwire when a future grammar expansion
    starts claiming one of them.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mtgz_ir.cards.base import (
    DestroyTargetCreature,
    DrawCards,
    ExileTargetCreature,
    GainLife,
    ModalChooseOne,
    PutPlusOneCounterOnTargetCreature,
)
from mtgz_ir.cards.parser.spell_builder import (
    _build_spell_printing,
    _parse_modal_choose_one,
    _parse_spell_oracle_text,
)


_DATA_PATH = (
    (Path(__file__).resolve().parents[2] / "ir")
    / "data"
    / "edge_of_eternities.json"
)


def _eoe_card(name: str) -> dict:
    """Look up a Scryfall record by ``name`` from the EoE data file."""
    with _DATA_PATH.open("r", encoding="utf-8") as f:
        for card in json.load(f):
            if card["name"] == name:
                return card
    raise LookupError(f"no EoE card named {name!r}")


# ---------- happy path: all modes parseable ----------

def test_two_mode_modal_with_recognized_modes_parses_to_modal_choose_one():
    # Synthetic two-mode modal where both modes use shapes already in
    # the unified grammar (`destroy target creature` and
    # `draw a card`).
    text = (
        "Choose one —\n"
        "• Destroy target creature.\n"
        "• Draw a card."
    )
    parsed = _parse_spell_oracle_text(text, "Bogus Modal")
    assert parsed == ModalChooseOne(modes=(
        DestroyTargetCreature(),
        DrawCards(amount=1),
    ))


def test_three_mode_modal_with_recognized_modes_parses_to_modal_choose_one():
    # Three modes — verifies the parser isn't hard-coded to exactly
    # two modes. Order is preserved (the engine resolves the
    # agent-picked index, so the tuple's positional order matters).
    text = (
        "Choose one —\n"
        "• Exile target creature.\n"
        "• Put a +1/+1 counter on target creature.\n"
        "• You gain 3 life."
    )
    parsed = _parse_spell_oracle_text(text, "Bogus Trimodal")
    assert parsed == ModalChooseOne(modes=(
        ExileTargetCreature(),
        PutPlusOneCounterOnTargetCreature(amount=1),
        GainLife(amount=3),
    ))


def test_helper_returns_modal_choose_one_directly():
    # Direct exercise of the helper — bypassing _parse_spell_oracle_text
    # so the helper's contract is tested in isolation.
    clauses = [
        "Choose one —",
        "• Destroy target creature.",
        "• Draw a card.",
    ]
    parsed = _parse_modal_choose_one(clauses, "Bogus Modal")
    assert parsed == ModalChooseOne(modes=(
        DestroyTargetCreature(),
        DrawCards(amount=1),
    ))


# ---------- partial-mode behavior (parser-is-right fallback) ----------

def test_modal_with_exactly_one_parseable_mode_returns_that_mode():
    # When EXACTLY ONE mode parses and all others are unrecognized, the
    # "parser-is-right" fallback returns that one mode's effect directly
    # -- but only for the grandfathered printings that claimed through
    # it before the no-silent-no-op rule (Get Out, Untimely
    # Malfunction, Sheoldred's Edict). Any other modal spell with an
    # unparsed mode stays unclaimed.
    text = (
        "Choose one —\n"
        "• Destroy target creature.\n"
        "• Do something completely unsupported by the grammar."
    )
    assert _parse_spell_oracle_text(text, "Get Out") == DestroyTargetCreature()
    assert _parse_spell_oracle_text(text, "Bogus Half-Modal") is None


def test_modal_with_first_mode_unrecognized_and_second_parses_returns_second():
    # Symmetric: when the SECOND mode parses and the first doesn't,
    # the parser returns the second mode's effect directly (same
    # exactly-one fallback).
    text = (
        "Choose one —\n"
        "• Do something completely unsupported by the grammar.\n"
        "• Draw a card."
    )
    assert _parse_spell_oracle_text(
        text, "Untimely Malfunction",
    ) == DrawCards(amount=1)
    assert _parse_spell_oracle_text(text, "Bogus Half-Modal 2") is None


def test_modal_with_two_parseable_and_one_unparseable_returns_none():
    # When TWO modes parse but one doesn't, no unambiguous "best"
    # choice exists — the parser returns None (spell stays unclaimed)
    # rather than picking one arbitrarily.
    text = (
        "Choose one —\n"
        "• Destroy target creature.\n"
        "• Draw a card.\n"
        "• Do something completely unsupported by the grammar."
    )
    assert _parse_spell_oracle_text(text, "Bogus Triple-Modal") is None


def test_modal_with_zero_parseable_modes_returns_none():
    # Zero parseable modes → unclaimed.
    text = (
        "Choose one —\n"
        "• Do something completely unsupported.\n"
        "• Do something else completely unsupported."
    )
    assert _parse_spell_oracle_text(text, "Bogus All-None Modal") is None


# ---------- shape rejections (variants out of scope) ----------

def test_choose_one_or_both_header_is_claimed_with_or_both_flag():
    # `Choose one or both` is supported via the same modal helper —
    # the resulting ``ModalChooseOne`` carries ``or_both=True`` so
    # the engine routes mode-pick through ``choose_modes`` (plural).
    # See tests/test_parser_modal_extensions.py for additional
    # coverage of the happy-path and reject paths.
    text = (
        "Choose one or both —\n"
        "• Destroy target creature.\n"
        "• Draw a card."
    )
    assert _parse_spell_oracle_text(text, "Bogus Both") == ModalChooseOne(
        modes=(DestroyTargetCreature(), DrawCards(amount=1)),
        or_both=True,
    )


def test_choose_two_header_is_claimed_by_choose_modes():
    # `Choose two` requires the agent to pick TWO modes — not this
    # ModalChooseOne branch; the multi-mode ``ChooseModes`` shape
    # (``modal_spells``) claims it.
    from mtgz_ir.effects.generic.multi_modal import ChooseModes

    text = (
        "Choose two —\n"
        "• Destroy target creature.\n"
        "• Draw a card.\n"
        "• You gain 3 life."
    )
    eff = _parse_spell_oracle_text(text, "Bogus Choose Two")
    assert isinstance(eff, ChooseModes)
    assert (eff.min_modes, eff.max_modes, eff.repeats) == (2, 2, False)


def test_modal_header_with_no_modes_returns_none():
    # A bare "Choose one —" with no bullet lines isn't a printed
    # spell shape; the parser must decline it.
    text = "Choose one —"
    assert _parse_spell_oracle_text(text, "Bogus Empty Modal") is None


def test_modal_header_with_single_mode_returns_none():
    # Modal spells always have ≥2 modes printed. A single-mode
    # "modal" wouldn't be modal at all and must not be accepted —
    # accepting it would surface a one-mode `ModalChooseOne` whose
    # static-side equivalent doesn't exist.
    text = (
        "Choose one —\n"
        "• Destroy target creature."
    )
    assert _parse_spell_oracle_text(text, "Bogus One-Mode") is None


def test_helper_returns_none_for_empty_clauses():
    # Defensive: the helper must tolerate an empty clause list
    # (in practice the caller filters empty input, but the helper
    # is a public-ish module function and shouldn't crash).
    assert _parse_modal_choose_one([], "Bogus Empty") is None


def test_helper_returns_none_when_first_clause_isnt_modal_header():
    # Helper called with a non-modal first clause must decline so
    # the dispatch in _parse_spell_oracle_text falls through to the
    # next branch.
    clauses = [
        "Destroy target creature.",
        "• Draw a card.",
    ]
    assert _parse_modal_choose_one(clauses, "Bogus") is None


def test_helper_returns_none_when_a_mode_line_lacks_bullet():
    # Bullet prefix is mandatory. A non-bulleted line after the
    # header is malformed and the helper must decline.
    clauses = [
        "Choose one —",
        "• Destroy target creature.",
        "Draw a card.",  # no bullet
    ]
    assert _parse_modal_choose_one(clauses, "Bogus") is None


def test_bare_bullet_line_doesnt_become_a_sequence_clause():
    # A bullet line WITHOUT the leading "Choose one —" header is
    # malformed printed text; the parser must not accept it as a
    # Sequence (the bullet prefix isn't a recognized spell-effect
    # surface, so the unified grammar would reject it anyway, but
    # this test pins that down).
    text = (
        "• Destroy target creature.\n"
        "• Draw a card."
    )
    assert _parse_spell_oracle_text(text, "Bogus Bare Bullets") is None


# ---------- preamble interaction ----------

def test_modal_after_convoke_preamble_still_parses():
    # If a modal spell ever ships with a Convoke preamble, the
    # preamble-strip pass must run first so the modal-detection
    # sees the header at index 0.
    text = (
        "Convoke (reminder.)\n"
        "Choose one —\n"
        "• Destroy target creature.\n"
        "• Draw a card."
    )
    assert _parse_spell_oracle_text(text, "Bogus Convoke Modal") == \
        ModalChooseOne(modes=(
            DestroyTargetCreature(),
            DrawCards(amount=1),
        ))


# ---------- end-to-end on the EoE pool ----------

# Every EoE printing whose oracle text contains "Choose one " (modal
# trigger). Today none of them have all-grammar-recognized modes, so
# every entry stays unclaimed by the parser. When the grammar grows
# enough to claim one, this list becomes the tripwire: the new claim
# is checked structurally against the static printing.
_EOE_CHOOSE_ONE_NAMES = (
    "Reroute Systems",
    "Scour for Scrap",       # Choose one or both — header doesn't match
    "Archenemy's Charm",
    "Drill Too Deep",
    "Ruinous Rampage",
)


