"""Tests for newly-added keyword-line entries.

This batch surveyed ``data/edge_of_eternities.json`` for one-line
keyword-style oracle text to extend the parser's keyword grammar.
The set's parametric keyword surface is already covered by the
Warp / Ward / Devour recognizers in ``printing_keywords.py`` —
no additional parametric keyword on a creature card was found
that the engine supports and that isn't already wired (Equip
appears only on Equipment, which has no parser builder yet;
Cycling / Aftermath / Disturb don't appear in EoE; Kicker /
Convoke are spell-side concerns handled by the spell builder).

What this batch does add are the two missing single-word
evergreens that *do* appear in EoE: ``Hexproof`` (Survey Mechan)
and ``Indestructible`` (Nutrient Block). Nutrient Block is an
artifact (no artifact builder yet at the time of this batch), so
the addition is defensive — it keeps ``_KEYWORD_LINES`` aligned
with the evergreen list so any future creature printing with
indestructible is recognized without further parser work.
Survey Mechan has since been fully migrated to the oracle-text
grammar (bespoke-survey-mechan): the activated ability body and
PerDifferentlyNamedLand cost-reduction rider now parse via grammar
primitives, so the hexproof keyword addition is no longer the only
claim avenue for this card.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.creature_builder import _build_creature_printing
from mtgz_ir.cards.parser.keywords import (
    _KEYWORD_LINES,
    _parse_keyword_line,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource


# ---------- _parse_keyword_line: new evergreens ----------


@pytest.mark.parametrize(
    "line,expected",
    [
        ("Hexproof", ("hexproof",)),
        ("Indestructible", ("indestructible",)),
    ],
)
def test_parse_keyword_line_new_evergreens_single(line, expected):
    """The two newly-added evergreens parse as bare single-keyword
    lines, returning their lowercase engine spellings."""
    assert _parse_keyword_line(line) == expected


def test_keyword_lines_table_contains_new_evergreens():
    """Direct sanity check against ``_KEYWORD_LINES`` so a future
    accidental removal is caught even if the per-line parametric
    tests above were skipped or refactored."""
    assert _KEYWORD_LINES["Hexproof"] == "hexproof"
    assert _KEYWORD_LINES["Indestructible"] == "indestructible"


@pytest.mark.parametrize(
    "line,expected",
    [
        ("Flying, Hexproof", ("flying", "hexproof")),
        ("Hexproof, Flying", ("hexproof", "flying")),
        ("Indestructible, Trample", ("indestructible", "trample")),
        (
            "Hexproof, Indestructible",
            ("hexproof", "indestructible"),
        ),
        (
            "Flying, Hexproof, Indestructible",
            ("flying", "hexproof", "indestructible"),
        ),
    ],
)
def test_parse_keyword_line_new_evergreens_in_multi(line, expected):
    """The new evergreens compose with the existing multi-keyword
    grammar: any comma-separated mix of recognized keywords parses,
    and order is preserved in the returned tuple."""
    assert _parse_keyword_line(line) == expected


# ---------- negative cases ----------


def test_parse_keyword_line_rejects_hexproof_with_qualifier():
    """``Hexproof from <color>`` is a different (parameterized)
    variant; the bare-evergreen entry must not silently swallow it.
    The existing keyword grammar fails the line because no piece
    matches the table — exactly the right behavior so the caller
    falls through and the enclosing card is skipped."""
    assert _parse_keyword_line("Hexproof from black") is None
    assert _parse_keyword_line("Hexproof from creatures") is None


def test_parse_keyword_line_rejects_indestructible_with_rider():
    """A trailing rider (``Indestructible. <something>``) is not a
    bare keyword line and must not match — same fall-through
    contract as above."""
    assert _parse_keyword_line(
        "Indestructible. When this dies, draw a card."
    ) is None


def test_parse_keyword_line_is_case_sensitive_for_new_evergreens():
    """The keyword table is keyed on the printed (capitalized)
    spelling. Lowercase variants don't match — lowercase ``hexproof``
    only appears mid-sentence, not as a printed keyword line."""
    assert _parse_keyword_line("hexproof") is None
    assert _parse_keyword_line("indestructible") is None


# ---------- _parse_clause integration ----------


@pytest.mark.parametrize(
    "line,keyword",
    [
        ("Hexproof", "hexproof"),
        ("Indestructible", "indestructible"),
    ],
)
def test_parse_clause_recognizes_new_evergreens(line, keyword):
    """The clause-level dispatcher must see the new keywords too —
    creature builder dispatches every non-printing-keyword line via
    ``_parse_clause`` and reads ``parsed.keywords``."""
    parsed = _parse_clause(line)
    assert parsed is not None
    assert parsed.keywords == (keyword,)
    assert parsed.trigger is None
    assert parsed.activated is None
    assert parsed.static is None


# ---------- creature builder integration ----------


def test_build_creature_printing_handles_bare_hexproof():
    """A synthetic vanilla creature whose only oracle-text line is
    ``Hexproof`` is fully claimed and the keyword round-trips into
    ``CardPrinting.keywords``."""
    card = {
        "name": "Test Hex Beast",
        "mana_cost": "{2}{G}",
        "type_line": "Creature — Beast",
        "power": "2",
        "toughness": "2",
        "oracle_text": "Hexproof",
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"hexproof"})


def test_build_creature_printing_handles_bare_indestructible():
    card = {
        "name": "Test Indy Beast",
        "mana_cost": "{4}",
        "type_line": "Creature — Beast",
        "power": "3",
        "toughness": "3",
        "oracle_text": "Indestructible",
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"indestructible"})


def test_build_creature_printing_handles_mixed_evergreen_line():
    """Multi-keyword line mixing existing and new evergreens parses
    end-to-end and contributes every keyword to the resulting
    printing's ``keywords`` set."""
    card = {
        "name": "Test Mixed Beast",
        "mana_cost": "{3}{G}",
        "type_line": "Creature — Beast",
        "power": "3",
        "toughness": "3",
        "oracle_text": "Flying, Hexproof, Indestructible",
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset(
        {"flying", "hexproof", "indestructible"}
    )


def test_build_creature_printing_handles_new_evergreens_with_reminder():
    """Reminder-text stripping happens before clause dispatch, so a
    printed ``Hexproof (...)`` line still resolves to a bare keyword
    line and the printing claims the keyword."""
    card = {
        "name": "Test Hex Reminder",
        "mana_cost": "{2}{U}",
        "type_line": "Creature — Wizard",
        "power": "1",
        "toughness": "3",
        "oracle_text": (
            "Hexproof (This creature can't be the target of spells "
            "or abilities your opponents control.)"
        ),
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"hexproof"})


def test_build_creature_printing_skips_qualified_hexproof():
    """A creature with ``Hexproof from <color>`` (variant the
    grammar intentionally doesn't support yet) must skip the whole
    card rather than silently drop the qualifier and claim plain
    Hexproof."""
    card = {
        "name": "Test Color-Hex Beast",
        "mana_cost": "{2}{W}",
        "type_line": "Creature — Beast",
        "power": "2",
        "toughness": "2",
        "oracle_text": "Hexproof from black",
    }
    assert _build_creature_printing(card) is None


# ---------- end-to-end EoE source claim ----------


def test_source_claims_survey_mechan():
    """Survey Mechan is now claimed by the oracle parser grammar
    (migrated from ``_BESPOKE_ARTIFACT_CREATURES`` in
    bespoke-survey-mechan). The printing faithfully includes both
    keyword lines (flying and hexproof) and the {10}/Sacrifice
    activated ability with SurveyMechanActivationEffect plus the
    PerDifferentlyNamedLand cost reduction.
    """
    from mtgz_ir.cards.parser.source import OracleParserCardSource
    source = OracleParserCardSource()
    assert "Survey Mechan" in source.claimed_names
    printings = source.get_printings()
    sm = printings["Survey Mechan"]
    assert "flying" in sm.keywords
    assert "hexproof" in sm.keywords
    assert len(sm.activated_abilities) == 1


def test_source_claims_nutrient_block():
    """Nutrient Block is now claimed by the artifact builder via the
    ``"this artifact is put into a graveyard from the battlefield"``
    trigger phrase (mapped to LEAVES_BATTLEFIELD per the static
    simplification) plus the indestructible keyword and the
    ``{2}, {T}, Sacrifice this artifact: You gain 3 life.`` activated
    ability — the unified clause grammar covers every line."""
    source = OracleParserCardSource()
    assert "Nutrient Block" in source.claimed_names
