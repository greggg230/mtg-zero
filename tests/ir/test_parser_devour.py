"""Tests for the Devour printing-level keyword recognizer.

Devour is the third printing-level parametric keyword (after Warp and
Ward) recognized in ``mtgz_ir.cards.parser.printing_keywords``. The
EoE form is ``Devour land N`` (Famished Worldsire's
``Devour land 3``); the recognizer extracts the integer N and the
creature builder plumbs it through to ``make_creature`` as
``devour_land_multiplier=N``.

Riders / em-dash variants intentionally don't match — the enclosing
card falls through and is skipped, the same way Warp/Ward riders do.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.creature_builder import _build_creature_printing
from mtgz_ir.cards.parser.printing_keywords import _parse_devour_line
from mtgz_ir.cards.parser.source import OracleParserCardSource


# ---------- _parse_devour_line ----------

@pytest.mark.parametrize(
    "line,expected_n",
    [
        ("Devour land 1", 1),
        ("Devour land 2", 2),
        ("Devour land 3", 3),
        ("Devour land 5", 5),
        ("Devour land 10", 10),
    ],
)
def test_parse_devour_line_simple(line, expected_n):
    """Bare ``Devour land N`` lines (no reminder text) extract N."""
    assert _parse_devour_line(line) == expected_n


def test_parse_devour_line_with_reminder_text():
    """The printed line includes parenthesized reminder text — the
    recognizer ignores it and just returns N."""
    line = (
        "Devour land 3 (As this creature enters, you may sacrifice "
        "any number of lands. It enters with three times that many "
        "+1/+1 counters on it.)"
    )
    assert _parse_devour_line(line) == 3


def test_parse_devour_line_strips_whitespace():
    assert _parse_devour_line("  Devour land 3  ") == 3


def test_parse_devour_line_rejects_em_dash_form():
    """``Devour land—N`` (em-dash separator) intentionally doesn't
    match — actual EoE printings use a space separator."""
    assert _parse_devour_line("Devour land—3") is None


def test_parse_devour_line_rejects_em_dash_rider():
    """A hypothetical em-dash + extra cost rider (parallel to
    ``Warp—{B}, Pay 2 life.``) is out of scope and must not match."""
    assert _parse_devour_line("Devour land 3—Pay 2 life.") is None


def test_parse_devour_line_rejects_plain_devour():
    """``Devour N`` without the ``land`` qualifier (the older,
    non-EoE Devour variant that eats creatures, not lands) is out of
    scope this batch — only ``Devour land N`` matches."""
    assert _parse_devour_line("Devour 3") is None


def test_parse_devour_line_rejects_non_devour_lines():
    assert _parse_devour_line("Flying") is None
    assert _parse_devour_line("Devour land") is None
    assert _parse_devour_line("Devour land 3 extra junk") is None
    assert _parse_devour_line("") is None


def test_parse_devour_line_rejects_non_integer_n():
    """N must be an integer literal."""
    assert _parse_devour_line("Devour land X") is None
    assert _parse_devour_line("Devour land {3}") is None


# ---------- creature builder integration ----------

def test_parse_clause_does_not_claim_devour_line():
    """Devour is handled by its own recognizer in
    ``_build_creature_printing``, not by ``_parse_clause``. A devour
    line passed directly to ``_parse_clause`` must return ``None``
    so the clause grammar stays orthogonal."""
    assert _parse_clause("Devour land 3") is None


def test_build_creature_printing_handles_simple_devour():
    """Synthetic vanilla creature with only a Devour clause —
    exercises the devour passthrough without depending on any other
    clause."""
    card = {
        "name": "Test Devourer",
        "mana_cost": "{6}",
        "type_line": "Creature — Beast",
        "power": "4",
        "toughness": "4",
        "oracle_text": (
            "Devour land 2 (As this creature enters, you may "
            "sacrifice any number of lands. It enters with two times "
            "that many +1/+1 counters on it.)"
        ),
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.devour_land_multiplier == 2


def test_build_creature_printing_handles_devour_with_keyword():
    """Devour combined with a normal vanilla keyword line."""
    card = {
        "name": "Test Devour Flyer",
        "mana_cost": "{5}{G}",
        "type_line": "Creature — Beast",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Flying\n"
            "Devour land 2"
        ),
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"flying"})
    assert printing.devour_land_multiplier == 2


def test_build_creature_printing_skips_devour_with_em_dash_rider():
    """A creature whose Devour clause has a hypothetical em-dash
    rider falls through and the whole card must be skipped, not
    silently parsed as plain Devour."""
    card = {
        "name": "Test Rider",
        "mana_cost": "{4}{G}",
        "type_line": "Creature — Beast",
        "power": "2",
        "toughness": "2",
        "oracle_text": "Devour land 3—Pay 2 life.",
    }
    assert _build_creature_printing(card) is None


def test_build_creature_printing_devour_default_zero():
    """A creature with no Devour clause produces a printing with the
    default ``devour_land_multiplier=0`` — the parser doesn't fabricate
    Devour out of nothing."""
    card = {
        "name": "Plain Beast",
        "mana_cost": "{3}",
        "type_line": "Creature — Beast",
        "power": "3",
        "toughness": "3",
        "oracle_text": "",
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.devour_land_multiplier == 0


# ---------- end-to-end EoE source claim ----------

def test_source_claims_famished_worldsire():
    """Famished Worldsire is now claimed by the grammar
    (engine-devour-land). The ETB body ``look at the top X cards …``
    is registered as a literal in ``_parse_effect`` mapping to
    ``FamishedWorldsireETBEffect``; Ward and Devour land 3 parse via
    their own per-keyword recognizers.
    """
    source = OracleParserCardSource()
    assert "Famished Worldsire" in source.claimed_names


def test_build_creature_printing_famished_worldsire_grammar_attributes():
    """Grammar-claimed Famished Worldsire has the correct oracle
    attributes: 0/0 (real card), devour_land_multiplier=3, Ward {3},
    and an ETB trigger that fires FamishedWorldsireETBEffect.
    Only EoE card with this exact ETB body; broader grammar deferred
    until a second card with the same shape exists.
    """
    from mtgz_ir.effects import FamishedWorldsireETBEffect as _Effect
    card = {
        "name": "Famished Worldsire",
        "mana_cost": "{5}{G}{G}{G}",
        "type_line": "Creature — Leviathan",
        "power": "0",
        "toughness": "0",
        "oracle_text": (
            "Ward {3}\n"
            "Devour land 3 (As this creature enters, you may sacrifice "
            "any number of lands. It enters with three times that many "
            "+1/+1 counters on it.)\n"
            "When this creature enters, look at the top X cards of your "
            "library, where X is this creature's power. Put any number "
            "of land cards from among them onto the battlefield tapped, "
            "then shuffle."
        ),
    }
    p = _build_creature_printing(card)
    assert p is not None
    assert p.power == 0
    assert p.toughness == 0
    assert p.devour_land_multiplier == 3
    assert p.ward_cost_str == "{3}"
    assert len(p.triggered_abilities) == 1
    assert isinstance(p.triggered_abilities[0].effect, _Effect)


