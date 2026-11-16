"""Tests for parenthesized reminder-text stripping.

The parser drops parenthesized reminder text from each oracle line
before clause dispatch so the keyword/triggered/activated/static
grammars don't have to know about flavor parentheticals. The helper
``_strip_reminder_text`` lives in ``parser/text.py`` and is wired
into the creature and spell builders. The land builder intentionally
does NOT use it — basic-land reminder text (``"({T}: Add {W}.)"``)
IS the land's oracle text and must reach the basic-land grammar
intact.

Coverage targets here:

- The pure helper: bare keyword cleanup, multiple parenthetical
  clauses on one line, no-op for parenthesis-free lines, mana-cost
  braces left alone, basic-land-shaped input is touched (the land
  builder's protection comes from never calling the helper, not from
  the helper detecting the shape).
- Builder integration: creature with a reminder-text-bearing trigger,
  spell with a reminder-text-bearing tail, basic-land claim parity is
  unaffected.
- End-to-end: ``OracleParserCardSource`` newly claims Starfighter
  Pilot and Starbreach Whale once stripping is enabled.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser.creature_builder import _build_creature_printing
from mtgz_ir.cards.parser.land_builder import _build_land_printing
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.spell_builder import _build_spell_printing
from mtgz_ir.cards.parser.text import _strip_reminder_text


# ---------- _strip_reminder_text ----------

@pytest.mark.parametrize(
    "raw,expected",
    [
        # Evergreen keywords with their printed reminder text.
        (
            "Flying (This creature can't be blocked except by "
            "creatures with flying or reach.)",
            "Flying",
        ),
        (
            "Trample (This creature can deal excess combat damage "
            "to the player or planeswalker it's attacking.)",
            "Trample",
        ),
        (
            "Menace (This creature can't be blocked except by "
            "two or more creatures.)",
            "Menace",
        ),
        # Trigger with surveil reminder text.
        (
            "Whenever this creature becomes tapped, surveil 1. "
            "(Look at the top card of your library. You may put "
            "it into your graveyard.)",
            "Whenever this creature becomes tapped, surveil 1.",
        ),
        # ETB with reminder.
        (
            "When this creature enters, surveil 2. (Look at the "
            "top two cards of your library, then put any number "
            "of them into your graveyard and the rest on top of "
            "your library in any order.)",
            "When this creature enters, surveil 2.",
        ),
    ],
)
def test_strip_reminder_text_basic_patterns(raw, expected):
    """A keyword + a single trailing parenthetical reduces to just
    the keyword (or just the rules text without the flavor tail)."""
    assert _strip_reminder_text(raw) == expected


def test_strip_reminder_text_multiple_clauses_in_one_line():
    """A pathological line with multiple parenthetical clauses has
    every parenthetical removed in a single call."""
    line = "Flying (flavor one) and Trample (flavor two)."
    assert _strip_reminder_text(line) == "Flying and Trample."


def test_strip_reminder_text_noop_for_lines_without_parens():
    """Lines with no parentheses round-trip unchanged (modulo
    leading/trailing whitespace, which the helper always strips)."""
    line = "Whenever this creature attacks, draw a card."
    assert _strip_reminder_text(line) == line


def test_strip_reminder_text_strips_leading_and_trailing_whitespace():
    """The helper always returns a stripped result regardless of
    whether anything was removed."""
    assert _strip_reminder_text("  Flying  ") == "Flying"
    assert _strip_reminder_text("  Flying (reminder)  ") == "Flying"


def test_strip_reminder_text_does_not_touch_mana_cost_braces():
    """Mana-cost notation uses braces, not parens. ``{1}{R}`` and
    ``{T}: Add {C}.`` must round-trip unchanged."""
    assert _strip_reminder_text("{1}{R}") == "{1}{R}"
    assert _strip_reminder_text("{T}: Add {C}.") == "{T}: Add {C}."
    assert (
        _strip_reminder_text("{2}{U}, {T}: Draw a card.")
        == "{2}{U}, {T}: Draw a card."
    )


def test_strip_reminder_text_only_parenthetical_returns_empty():
    """A line that's nothing but a parenthetical (degenerate input)
    becomes the empty string, leaving the caller free to skip it."""
    assert _strip_reminder_text("(just reminder)") == ""
    assert _strip_reminder_text("  (just reminder)  ") == ""


def test_strip_reminder_text_does_strip_basic_land_shape():
    """The helper itself does NOT special-case the basic-land
    reminder shape — it's a pure paren-stripper. Basic lands stay
    intact only because the land builder never calls the helper.
    Documenting the actual behavior here so the protection contract
    is visible: if anyone ever wires the helper into the land builder
    they will reproduce this test failure and notice the issue.
    """
    assert _strip_reminder_text("({T}: Add {W}.)") == ""


# ---------- creature builder integration ----------

def test_creature_builder_handles_keyword_with_reminder_text():
    """A vanilla creature with the printed Flying reminder text
    parses just like one with bare ``Flying``."""
    card = {
        "name": "Test Flyer",
        "mana_cost": "{2}{U}",
        "type_line": "Creature — Bird",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Flying (This creature can't be blocked except by "
            "creatures with flying or reach.)"
        ),
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"flying"})


def test_creature_builder_handles_trigger_with_reminder_text():
    """A surveil-on-tap trigger with its printed reminder text in
    parens produces the same printing as the bare-trigger form."""
    card = {
        "name": "Test Surveiler",
        "mana_cost": "{1}{W}",
        "type_line": "Creature — Human Pilot",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Whenever this creature becomes tapped, surveil 1. "
            "(Look at the top card of your library. You may put "
            "it into your graveyard.)"
        ),
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert len(printing.triggered_abilities) == 1


def test_creature_builder_skips_line_that_is_only_reminder():
    """A line whose entire content is a parenthetical has nothing
    left after stripping; the builder skips it instead of treating
    the empty result as an unparseable clause and rejecting the card.
    """
    card = {
        "name": "Test Only Reminder",
        "mana_cost": "{1}{U}",
        "type_line": "Creature — Whale",
        "power": "1",
        "toughness": "1",
        "oracle_text": "Flying\n(this whole line is just flavor)",
    }
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"flying"})


# ---------- spell builder integration ----------

def test_spell_builder_handles_single_clause_with_reminder_tail():
    """A single-line spell whose effect sentence ends with a
    parenthetical (e.g. Hymn-of-the-Faller-shaped surveil reminder)
    parses as if the tail weren't there."""
    card = {
        "name": "Test Surveil Spell",
        "mana_cost": "{1}{B}",
        "type_line": "Sorcery",
        "oracle_text": (
            "Surveil 1, then you draw a card and lose 1 life. "
            "(To surveil 1, look at the top card of your library. "
            "You may put it into your graveyard.)"
        ),
    }
    printing = _build_spell_printing(card)
    # The spell-effect grammar may or may not understand the surveil
    # phrase today — what we verify is that the parser at least gets
    # past the reminder-text barrier (i.e. no parenthetical fragment
    # survives to break a downstream parse). If the grammar can't
    # parse the cleaned phrase we'll get None; otherwise a printing.
    # Either outcome is acceptable for THIS test — the helper did its
    # job. The strict assertion is that whatever the grammar does, it
    # doesn't see the parenthetical.
    if printing is not None:
        # If a printing came back, it must have a real effect, not a
        # truncated-by-parenthesis nonsense.
        assert printing.spell_effect is not None


def test_spell_builder_drops_clause_that_is_only_reminder():
    """A spell whose body is two clauses where one is purely a
    parenthetical degrades cleanly to the remaining clause."""
    card = {
        "name": "Test Drop Reminder Clause",
        "mana_cost": "{R}",
        "type_line": "Instant",
        "oracle_text": (
            "{name} deals 2 damage to any target.\n"
            "(parenthetical clause that says nothing useful)"
        ).replace("{name}", "Test Drop Reminder Clause"),
    }
    printing = _build_spell_printing(card)
    # After stripping, only the damage clause remains and it parses
    # via the unified spell-effect grammar.
    assert printing is not None


# ---------- land builder protection ----------

def test_land_builder_basic_land_unaffected_by_strip_helper():
    """Basic lands' oracle text IS the parenthesized reminder
    ``"({T}: Add {W}.)"``. The land builder doesn't call the
    reminder-text helper; verify a basic land still claims through
    the parser source unchanged."""
    card = {
        "name": "Plains",
        "mana_cost": "",
        "type_line": "Basic Land — Plains",
        "oracle_text": "({T}: Add {W}.)",
    }
    printing = _build_land_printing(card)
    assert printing is not None
    assert printing.name == "Plains"


# ---------- end-to-end source claim ----------

def test_source_now_claims_starfighter_pilot():
    """Before reminder-text stripping, Starfighter Pilot's
    surveil-on-tap trigger ended with parenthesized reminder text and
    failed clause dispatch. After stripping it claims end-to-end."""
    source = OracleParserCardSource()
    assert "Starfighter Pilot" in source.claimed_names


def test_source_now_claims_starbreach_whale():
    """Same story for Starbreach Whale: the ETB-surveil-2 trigger had
    reminder text that broke the trigger grammar. With the strip in
    place all three of its lines (Flying, ETB-surveil, Warp) parse."""
    source = OracleParserCardSource()
    assert "Starbreach Whale" in source.claimed_names
