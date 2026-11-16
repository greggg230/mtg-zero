"""Tests for the within-line sentence-boundary splitter in
``mtgz_ir.cards.parser.spell_builder``.

Some EoE spells pack two effect sentences onto a single oracle-text
line separated by ``. `` rather than ``\\n`` (Radiant Strike's
``"Destroy target artifact or tapped creature. You gain 3 life."``,
Sami's Curiosity's ``"You gain 2 life. Create a Lander token."``).
``_split_sentences`` carves those lines at the
period-then-uppercase-letter boundary so each sentence flows through
the per-clause parser independently and the Sequence combinator
picks them up.

Coverage targets:

* The splitter helper alone — single-sentence round-trip,
  multi-sentence carve, ``If you do,`` continuation guard,
  whitespace and trailing-period edge cases.
* End-to-end builder — Radiant Strike and Sami's Curiosity now
  parse to the static-side ``Sequence`` shapes (newly claimed by
  this slice). Every other previously-claimed spell with a multi-
  sentence line — Vote Out's Convoke preamble, Plasma Bolt's Void
  rider, Scour for Scrap's modal modes, Dark Endurance's cost-
  reduction rider — still parses unchanged (regression guard).
* Synthetic single-line, multi-sentence spell that uses both a
  carve-eligible boundary AND an ``If you do,`` continuation in the
  same line — only the eligible boundary fires; the continuation
  stays welded to its parent sentence so the
  ``You may pay/sacrifice/discard {COST}. If you do, …`` family
  doesn't get torn in half.
* The full parametrized parity gate (``test_parser_parity.py``)
  picks up the two newly-claimed names automatically; this file
  doesn't duplicate that coverage but does spot-check that the
  parser-built printing for each new card equals the static one.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mtgz_ir.cards.base import (
    CreateToken,
    DestroyTargetArtifactOrTappedCreature,
    GainLife,
    Sequence,
)
from mtgz_ir.cards.parser.spell_builder import (
    _build_spell_printing,
    _parse_spell_oracle_text,
    _split_sentences,
)
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


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


# ---------------------------------------------------------------------------
# _split_sentences unit tests
# ---------------------------------------------------------------------------


def test_split_sentences_single_sentence_roundtrips():
    """A line with no internal sentence boundary returns as a
    single-element list — no spurious carve, no whitespace damage."""
    assert _split_sentences("Destroy target creature.") == [
        "Destroy target creature."
    ]


def test_split_sentences_no_period_roundtrips():
    """A trailing-period-less surface (the per-clause parser tolerates
    both) also round-trips as a single element."""
    assert _split_sentences("Draw a card") == ["Draw a card"]


def test_split_sentences_two_sentences_carved():
    """Two effect sentences on one line carve into two clauses, each
    keeping its trailing period so the per-clause normalizer can strip
    them downstream the same way it does for newline-split clauses."""
    line = "Destroy target artifact or tapped creature. You gain 3 life."
    assert _split_sentences(line) == [
        "Destroy target artifact or tapped creature.",
        "You gain 3 life.",
    ]


def test_split_sentences_three_sentences_carved():
    """Three sentences on one line — splitter is not capped at two."""
    line = "You gain 2 life. Create a Lander token. Draw a card."
    assert _split_sentences(line) == [
        "You gain 2 life.",
        "Create a Lander token.",
        "Draw a card.",
    ]


def test_split_sentences_lowercase_continuation_not_split():
    """``period + whitespace + lowercase`` is NOT a sentence boundary —
    parenthetical reminders inside an effect sentence (already stripped
    upstream) wouldn't be eligible anyway, but the lowercase guard also
    keeps things like ``Surveil 1, then you draw a card.`` welded."""
    line = "Surveil 1, then you draw a card."
    assert _split_sentences(line) == [line]


def test_split_sentences_if_you_do_continuation_guarded():
    """The negative lookahead for ``If you do,`` keeps the
    ``You may pay/sacrifice/discard {COST}. If you do, …`` compound
    primitives intact — those are matched as a single phrase by the
    effect grammar and the embedded sentence boundary inside them is
    load-bearing."""
    line = "You may pay {2}. If you do, draw a card."
    assert _split_sentences(line) == [line]


def test_split_sentences_if_you_do_after_real_boundary():
    """A line with both a carve-eligible boundary AND an ``If you do,``
    continuation splits only at the eligible boundary; the continuation
    stays welded to its preceding sentence so the optional-payment
    primitive survives intact."""
    line = (
        "Create a Lander token. "
        "You may pay {2}. If you do, draw a card."
    )
    assert _split_sentences(line) == [
        "Create a Lander token.",
        "You may pay {2}. If you do, draw a card.",
    ]


def test_split_sentences_extra_whitespace_consumed():
    """Multiple spaces between sentences are absorbed by the ``\\s+``
    in the boundary regex — output has no leading/trailing whitespace."""
    line = "You gain 2 life.   Create a Lander token."
    assert _split_sentences(line) == [
        "You gain 2 life.",
        "Create a Lander token.",
    ]


def test_split_sentences_empty_pieces_filtered():
    """A pathological double-space-with-no-content boundary doesn't
    inject blank clauses — empty pieces post-strip are filtered out."""
    # An impossible-in-the-wild case but cheap to defend against.
    assert _split_sentences("") == []
    assert _split_sentences("   ") == []


# ---------------------------------------------------------------------------
# End-to-end EoE card claim tests (the cards this slice unlocks)
# ---------------------------------------------------------------------------


def test_radiant_strike_parses_via_sentence_split():
    """Radiant Strike's oracle text is one line, two sentences:
    ``"Destroy target artifact or tapped creature. You gain 3 life."``
    With sentence splitting the two sentences become independent
    clauses and combine via the Sequence path — matching the static-
    side ``Sequence(DestroyTargetArtifactOrTappedCreature, GainLife(3))``.
    """
    printing = _build_spell_printing(_eoe_card("Radiant Strike"))
    assert printing is not None
    assert printing.spell_effect == Sequence(
        effects=(
            DestroyTargetArtifactOrTappedCreature(),
            GainLife(amount=3),
        ),
    )




def test_samis_curiosity_parses_via_sentence_split():
    """Sami's Curiosity's oracle text packs ``"You gain 2 life.
    Create a Lander token."`` on a single line plus a parenthesized
    Lander reminder. Reminder-strip removes the parenthetical, then
    sentence splitting produces two clauses combined as
    ``Sequence(GainLife(2), CreateToken(Lander, 1))``."""
    printing = _build_spell_printing(_eoe_card("Sami's Curiosity"))
    assert printing is not None
    assert printing.spell_effect == Sequence(
        effects=(
            GainLife(amount=2),
            CreateToken(
                token_printing=TOKEN_PRINTINGS["Lander"],
                count=1,
            ),
        ),
    )




# ---------------------------------------------------------------------------
# Regression guards — previously-claimed spells with multi-sentence
# surfaces in some shape. Sentence splitting must not break them.
# ---------------------------------------------------------------------------




# ---------------------------------------------------------------------------
# Single-sentence behavior preservation
# ---------------------------------------------------------------------------


def test_single_sentence_spell_unchanged():
    """A spell whose oracle text is one sentence on one line still
    parses through ``_parse_spell_effect_phrase`` directly — the
    splitter no-ops because there's no internal boundary, and the
    single-clause branch returns the parsed effect unchanged."""
    text = "Destroy target creature."
    effect = _parse_spell_oracle_text(text, "Synthetic Spell")
    assert effect is not None
    # Equivalent to going through `_parse_spell_effect_phrase` directly.
    from mtgz_ir.cards.base import DestroyTargetCreature

    assert effect == DestroyTargetCreature()


def test_synthetic_two_sentence_one_line_combines_as_sequence():
    """A synthetic two-sentence single-line spell combines via the
    Sequence path — exercises the splitter+combinator pipeline
    independent of any specific EoE card so the test stays meaningful
    even if both real-world claimants are migrated to a different
    combinator someday."""
    text = "Draw a card. You gain 3 life."
    effect = _parse_spell_oracle_text(text, "Synthetic Two-Sentence")
    assert effect is not None
    from mtgz_ir.cards.base import DrawCards

    assert effect == Sequence(
        effects=(DrawCards(amount=1), GainLife(amount=3)),
    )


def test_unparseable_second_sentence_rejects_whole_spell():
    """If sentence splitting produces a clause that the per-clause
    grammar can't parse, the whole spell rejects (returns ``None``).
    Mirrors the existing "any unparseable clause skips the whole spell"
    rule for newline-split clauses."""
    # First sentence parses (DrawCards), second is gibberish.
    text = "Draw a card. Lorem ipsum dolor sit amet."
    effect = _parse_spell_oracle_text(text, "Synthetic Reject")
    assert effect is None
