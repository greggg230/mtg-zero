"""Tests for the leading-spell-keyword-preamble branch in
``mtgz_ir.cards.parser.spell_builder``.

The branch recognizes a leading clause that's just an unimplemented
spell-only keyword followed by its parenthesized reminder text (today
only ``Convoke (...)``) and drops it before parsing the rest of the
spell. This keeps the parser in parity with the static implementation,
which omits the keyword's behavior entirely. See the module docstring
on ``spell_builder.py`` for the rationale.

Coverage targets:
  - The new pattern recognizer accepts the supported keywords and
    rejects similarly-shaped lines that don't match.
  - End-to-end parse of Vote Out (the EoE spell now claimed by this
    branch) matches the static printing exactly.
  - Honor and Plasma Bolt — the previously-claimed multi-line spells
    that DON'T have a keyword preamble — still parse to their original
    combinator shapes (Sequence and ConditionalIfVoid respectively),
    so the new branch hasn't perturbed the existing dispatch.
  - A spell whose preamble looks similar but isn't a recognized
    keyword (e.g. a Kicker preamble) does NOT get its preamble
    dropped, falling through to the unsupported path.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mtgz_ir.cards.base import (
    ConditionalIfVoid,
    DealDamageAnyTarget,
    DestroyTargetCreature,
    DrawCards,
    PutPlusOneCounterOnTargetCreature,
    Sequence,
    make_sorcery,
)
from mtgz_ir.cards.parser.spell_builder import (
    _build_spell_printing,
    _is_spell_keyword_preamble,
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


# ---------- _is_spell_keyword_preamble ----------

def test_preamble_recognizer_accepts_convoke_with_full_reminder():
    # Vote Out's actual leading clause from EoE.
    line = (
        "Convoke (Your creatures can help cast this spell. Each "
        "creature you tap while casting this spell pays for {1} or "
        "one mana of that creature's color.)"
    )
    assert _is_spell_keyword_preamble(line)


def test_preamble_recognizer_accepts_convoke_with_short_reminder():
    # The recognizer doesn't care about the reminder text content,
    # only that it's parenthesized after the keyword.
    assert _is_spell_keyword_preamble("Convoke (anything goes here.)")


def test_preamble_recognizer_strips_surrounding_whitespace():
    # The caller passes already-stripped clauses, but defensive
    # whitespace handling means a stray trailing space won't hide a
    # recognized preamble.
    assert _is_spell_keyword_preamble(
        "  Convoke (some reminder text.)  "
    )


def test_preamble_recognizer_rejects_kicker_preamble():
    # Kicker isn't on the whitelist (its inner spell text isn't
    # broadly parseable yet, and stripping it would silently desync
    # from a static side that DOES model Kicker as a separate cost).
    line = (
        "Kicker {1}{U} (You may pay an additional {1}{U} as you "
        "cast this spell.)"
    )
    assert not _is_spell_keyword_preamble(line)


def test_preamble_recognizer_rejects_unknown_keyword():
    assert not _is_spell_keyword_preamble("Bogusword (some reminder.)")


def test_preamble_recognizer_rejects_non_keyword_clauses():
    # A normal spell-effect sentence must NOT be recognized as a
    # preamble — that would cause the parser to silently drop a real
    # effect line.
    assert not _is_spell_keyword_preamble("Destroy target creature.")
    # Convoke-without-reminder-text is not the recognized printed
    # shape; reject it (Scryfall always pairs the keyword with its
    # reminder text on standalone-keyword preamble lines).
    assert not _is_spell_keyword_preamble("Convoke")
    # Convoke with the parenthetical missing trailing text isn't a
    # printed shape; reject it.
    assert not _is_spell_keyword_preamble("Convoke ()")


# ---------- end-to-end on the EoE pool ----------



def test_vote_out_oracle_text_drops_convoke_and_returns_destroy():
    # Direct exercise of _parse_spell_oracle_text on Vote Out's text:
    # the Convoke preamble is stripped, leaving a single-clause
    # destroy effect.
    text = (
        "Convoke (Your creatures can help cast this spell. Each "
        "creature you tap while casting this spell pays for {1} or "
        "one mana of that creature's color.)\n"
        "Destroy target creature."
    )
    assert _parse_spell_oracle_text(text, "Vote Out") == \
        DestroyTargetCreature()


# ---------- existing combinator branches still work ----------

def test_honor_still_parses_to_sequence():
    # Honor is a plain two-clause Sequence; it has no preamble, so the
    # new branch must be a no-op for it.
    card = _eoe_card("Honor")
    parsed = _build_spell_printing(card)
    assert parsed is not None
    assert parsed.spell_effect == Sequence(effects=(
        PutPlusOneCounterOnTargetCreature(amount=1),
        DrawCards(amount=1),
    ))


def test_plasma_bolt_still_parses_to_conditional_if_void():
    # Plasma Bolt's two-clause Void shape is unaffected by the new
    # preamble-stripping pass.
    card = _eoe_card("Plasma Bolt")
    parsed = _build_spell_printing(card)
    assert parsed is not None
    assert parsed.spell_effect == ConditionalIfVoid(
        then_effect=DealDamageAnyTarget(amount=3),
        else_effect=DealDamageAnyTarget(amount=2),
    )


# ---------- negative end-to-end ----------

def test_consult_the_star_charts_claimed_with_kicker_threaded():
    # Consult the Star Charts has a Kicker preamble plus a 4-sentence
    # body with a bespoke effect (``ConsultTheStarChartsEffect``).
    # The Kicker preamble parser captures the cost ("{1}{U}") and
    # threads it onto the printing via ``kicker``; the body lands as
    # a whole-spell exact-text dispatch entry. End-to-end, the parser
    # produces a printing matching the static side exactly.
    card = _eoe_card("Consult the Star Charts")
    printing = _build_spell_printing(card)
    assert printing is not None
    assert printing.kicker_cost_str == "{1}{U}"


def test_synthetic_spell_with_only_a_preamble_returns_none():
    # If a spell text consists ENTIRELY of preamble clauses with no
    # surviving effect line, the parser must return None — there's
    # nothing to parse into an Effect.
    text = "Convoke (Your creatures can help cast this spell.)"
    assert _parse_spell_oracle_text(text, "Bogus Convoker") is None


def test_synthetic_spell_with_unparseable_tail_returns_none():
    # Preamble strips cleanly, but the surviving clause has no
    # grammar match — the spell must fall through to None rather
    # than silently parse to a partial effect.
    text = (
        "Convoke (reminder.)\n"
        "Do something completely unsupported by the grammar."
    )
    assert _parse_spell_oracle_text(text, "Bogus Spell") is None


def test_synthetic_spell_with_two_preambles_then_supported_effect():
    # Multiple leading preambles all get stripped; the surviving
    # effect parses normally. Today only Convoke is on the whitelist,
    # so the easiest two-preamble probe stacks two Convokes — which
    # is nonsensical printed text but exercises the loop in
    # _parse_spell_oracle_text. The parser must accept it.
    text = (
        "Convoke (one reminder.)\n"
        "Convoke (another reminder.)\n"
        "Destroy target creature."
    )
    assert _parse_spell_oracle_text(text, "Bogus Stacker") == \
        DestroyTargetCreature()


def test_preamble_appearing_mid_text_is_not_stripped():
    # Only LEADING preamble clauses get dropped. A Convoke-shaped
    # line appearing AFTER a real effect must NOT be silently
    # dropped — that would change the spell's resolution. The
    # surviving 2-clause shape doesn't fit Sequence (the Convoke
    # line doesn't parse as a spell effect), so the spell falls
    # through to None.
    text = (
        "Destroy target creature.\n"
        "Convoke (reminder.)"
    )
    assert _parse_spell_oracle_text(text, "Bogus Trailer") is None


# ---------- structural parity sanity ----------



def test_make_sorcery_round_trip_for_vote_out():
    # Sanity: the parser produces the exact CardPrinting that
    # `make_sorcery("Vote Out", "{3}{B}", DestroyTargetCreature(),
    # has_convoke=True)` would produce. The Convoke preamble in the
    # oracle text drives ``has_convoke=True`` via _detect_preamble_flags.
    card = _eoe_card("Vote Out")
    parsed = _build_spell_printing(card)
    expected = make_sorcery(
        "Vote Out", "{3}{B}", effect=DestroyTargetCreature(),
        has_convoke=True,
    )
    assert parsed == expected
