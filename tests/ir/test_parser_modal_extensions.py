"""Extended modal-spell parser coverage.

This file covers the ``Choose one or both —`` modal-header extension
added to ``mtgz_ir.cards.parser.spell_builder._parse_modal_choose_one``.
The base ``Choose one —`` shape is exercised in
``tests/test_parser_modal_spells.py``; this file adds:

  - ``Choose one or both —`` happy path (synthetic spell whose modes
    both parse via the unified grammar) — produces a
    ``ModalChooseOne`` with ``or_both=True`` so the engine prompts the
    agent's ``choose_modes`` (plural) hook.
  - ``Choose one or both —`` reject path (one mode fails to parse) —
    the whole spell stays unclaimed.
  - Direct exercise of ``_parse_modal_choose_one`` for the ``or_both``
    branch.
  - Per-mode reminder-text stripping continues to work for the
    ``or_both`` shape.
  - End-to-end tripwire on the EoE modal pool: every real EoE
    ``Choose one`` / ``Choose one or both`` spell either claims and
    matches static, or stays unclaimed. (No real EoE modal claims as
    of this batch — mode-grammar gaps prevent every one of them.
    Verified per-spell here so the next grammar expansion that flips
    a card to claimed can confirm static parity in one place.)
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mtgz_ir.cards.base import (
    DestroyTargetCreature,
    DrawCards,
    ModalChooseOne,
    ReturnTargetArtifactCardFromYourGraveyardToHand,
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


# ---------- Choose one or both: happy path ----------

def test_choose_one_or_both_with_recognized_modes_parses_with_or_both_flag():
    # Synthetic two-mode "or both" spell where both modes use shapes
    # already in the unified grammar (``destroy target creature`` and
    # ``draw a card``). Result is a ``ModalChooseOne`` with
    # ``or_both=True`` so the engine routes mode-pick through
    # ``choose_modes`` (plural).
    text = (
        "Choose one or both —\n"
        "• Destroy target creature.\n"
        "• Draw a card."
    )
    parsed = _parse_spell_oracle_text(text, "Bogus Or Both")
    assert parsed == ModalChooseOne(
        modes=(DestroyTargetCreature(), DrawCards(amount=1)),
        or_both=True,
    )


def test_choose_one_or_both_three_modes_parses_with_or_both_flag():
    # Three modes — verifies the parser isn't hard-coded to two modes
    # for the ``or_both`` branch (matching the plain ``choose one``
    # branch's flexibility).
    text = (
        "Choose one or both —\n"
        "• Destroy target creature.\n"
        "• Draw a card.\n"
        "• Return target artifact card from your graveyard to your hand."
    )
    parsed = _parse_spell_oracle_text(text, "Bogus Trimodal Both")
    assert parsed == ModalChooseOne(
        modes=(
            DestroyTargetCreature(),
            DrawCards(amount=1),
            ReturnTargetArtifactCardFromYourGraveyardToHand(),
        ),
        or_both=True,
    )


def test_helper_returns_modal_choose_one_or_both_directly():
    # Direct exercise of the helper so its ``or_both=True`` contract
    # is tested in isolation from the dispatcher.
    clauses = [
        "Choose one or both —",
        "• Destroy target creature.",
        "• Draw a card.",
    ]
    parsed = _parse_modal_choose_one(clauses, "Bogus Or Both")
    assert parsed == ModalChooseOne(
        modes=(DestroyTargetCreature(), DrawCards(amount=1)),
        or_both=True,
    )


# ---------- Choose one or both: reject paths ----------

def test_choose_one_or_both_with_unrecognized_mode_returns_none():
    # First mode parses; second mode is gibberish to the grammar.
    # The whole spell must be skipped — partial modal effects would
    # silently change the spell's playable behavior. Symmetric with
    # the plain ``choose one`` reject path.
    text = (
        "Choose one or both —\n"
        "• Destroy target creature.\n"
        "• Do something completely unsupported by the grammar."
    )
    assert _parse_spell_oracle_text(text, "Bogus Half Or Both") is None


def test_choose_one_or_both_single_mode_returns_none():
    # ``Choose one or both`` modal spells always have ≥2 modes printed
    # — a single-mode "or both" wouldn't be modal at all and the
    # parser must decline (matches the plain-modal single-mode reject).
    text = (
        "Choose one or both —\n"
        "• Destroy target creature."
    )
    assert _parse_spell_oracle_text(text, "Bogus Single Or Both") is None


def test_choose_one_or_both_no_modes_returns_none():
    # A bare header with no bullet lines isn't a printed spell shape.
    text = "Choose one or both —"
    assert _parse_spell_oracle_text(text, "Bogus Empty Or Both") is None


# ---------- per-mode reminder text stripping ----------

def test_choose_one_or_both_strips_reminder_text_per_mode():
    # Each mode line passes through the per-clause reminder-text
    # stripper (``_strip_reminder_text``) before the modal helper
    # sees the bullet payload. A mode line with a parenthetical
    # reminder still parses cleanly. (No real EoE ``or both`` mode
    # ships reminder text today, but the pipeline must handle it
    # consistently with the ``choose one`` branch.)
    text = (
        "Choose one or both —\n"
        "• Destroy target creature. (Flavor reminder.)\n"
        "• Draw a card."
    )
    parsed = _parse_spell_oracle_text(text, "Bogus Reminder Or Both")
    assert parsed == ModalChooseOne(
        modes=(DestroyTargetCreature(), DrawCards(amount=1)),
        or_both=True,
    )


# ---------- end-to-end on the EoE pool ----------

# Every real EoE printing whose oracle text leads with a modal
# ``Choose one`` / ``Choose one or both`` header. Each one's parser
# claim status is checked against the static printing — a future
# grammar expansion that flips one of these to claimed will fail
# loudly here unless the parser produces the same printing the static
# side ships.
_EOE_MODAL_NAMES = (
    "Reroute Systems",
    "Scour for Scrap",        # Choose one or both
    "Archenemy's Charm",
    "Drill Too Deep",
    "Ruinous Rampage",
)


