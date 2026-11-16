"""Tests for the DSK creature ``This creature enters tapped.`` clause.

The engine has long carried the printing-level ``enters_tapped: bool``
field (consumed by ``actions.put_into_battlefield`` and
``effects/combinators.py``). What was missing was the parser side —
the ``creature_builder`` ``_parse_clause`` pipeline didn't recognize
the ``This creature enters tapped.`` line so any creature whose oracle
text included it stayed unclaimed. This batch adds the recognizer and
threads the flag through ``make_creature``.

Surfaces on Piranha Fly's two-line oracle text (``Flying`` + ``This
creature enters tapped.``); a future creature whose oracle text only
prints the line without other clauses would claim via the same path
without further grammar work.
"""

# mtg-zero (docs/ENGINE.md 169): 2 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.creature_statics_wg, not carried into mtgz_ir):
# test_creature_with_enters_tapped_true_etbs_tapped, test_synthetic_vanilla_etbs_untapped_unchanged

from __future__ import annotations

from mtgz_ir.cards import SYNTHETIC_CREATURES
from mtgz_ir.cards.base import make_creature
from mtgz_ir.cards.parser.printing_keywords import _parse_enters_tapped_line
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.game import GameState, Player, Zone


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- line recognizer ----------


def test_parse_enters_tapped_line_recognizes_canonical_form():
    # The verbatim Scryfall surface form. Whitespace tolerant — the
    # caller passes lines through ``_strip_reminder_text`` which
    # already strips trailing spaces, but the recognizer's own
    # ``.strip()`` is a belt-and-suspenders for hand-fed input.
    assert _parse_enters_tapped_line("This creature enters tapped.") is True


def test_parse_enters_tapped_line_rejects_other_forms():
    # Variants we DON'T match: missing period, the broader "This
    # permanent enters tapped" wording, or the printed-with-counters
    # variant. Each of those would land its own recognizer rather
    # than parameterizing this one — the strict 1:1 mapping mirrors
    # the ``Ward—Discard`` precedent.
    assert _parse_enters_tapped_line("This creature enters tapped") is False
    assert _parse_enters_tapped_line(
        "This permanent enters tapped."
    ) is False
    assert _parse_enters_tapped_line(
        "This creature enters tapped with a +1/+1 counter on it."
    ) is False
    assert _parse_enters_tapped_line("") is False


# ---------- make_creature kwarg threading ----------


def test_make_creature_threads_enters_tapped_flag():
    # Direct sanity check that the new ``enters_tapped=True`` kwarg
    # reaches the produced ``CardPrinting`` field. The default stays
    # ``False`` for every creature that doesn't pass the kwarg.
    p = make_creature("Test ETB Tapped", "{1}", 1, 1, enters_tapped=True)
    assert p.enters_tapped is True
    p_default = make_creature("Test Default", "{1}", 1, 1)
    assert p_default.enters_tapped is False


# ---------- end-to-end claim ----------


def test_piranha_fly_claimed_by_parser():
    # Piranha Fly ``{1}{U}`` 2/1 Fish Insect. Two oracle lines:
    # ``Flying`` (existing keyword) and ``This creature enters
    # tapped.`` (new printing-keyword recognizer). Claims via the
    # existing flying keyword grammar plus the new
    # ``_parse_enters_tapped_line`` recognizer flipping the
    # ``enters_tapped=True`` kwarg on ``make_creature``.
    assert "Piranha Fly" in _claimed_names()


def test_piranha_fly_carries_enters_tapped_flag():
    # The claimed printing must actually carry the field — without
    # the threading from the builder to ``make_creature`` to the
    # printing's ``enters_tapped`` field, the engine wouldn't tap
    # the creature on ETB even though the line was recognized.
    src = OracleParserCardSource()
    printings = {p.name: p for p in src.get_printings().values()}
    p = printings["Piranha Fly"]
    assert p.enters_tapped is True
    assert "flying" in p.keywords


# ---------- engine ETB-tap behavior (sanity, not coverage gain) ----------




