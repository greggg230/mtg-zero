"""Tests for the sixth Duskmourn creature parser batch.

This batch claims a pair of common-rarity Duskmourn (set code ``dsk``)
creatures whose oracle lines need only printing-level ETB-replacement
fields plus the line recognizers that flip them:

* **Cackling Slasher** ``{3}{B}`` 3/3 Common Human Assassin —
  ``Deathtouch`` plus
  ``This creature enters with a +1/+1 counter on it if a creature died
  this turn.`` The new
  ``CardPrinting.enters_with_plus_one_counter_if_creature_died_this_turn``
  field fires from ``actions.put_into_battlefield`` when the global
  ``GameState.creatures_died_this_turn`` tracker is positive. The
  tracker is incremented in ``turn.move_off_battlefield`` for every
  creature dies event the engine fires (regardless of controller —
  Cackling Slasher's printed clause doesn't qualify "you control") and
  reset by ``run_cleanup_step``.

* **Patched Plaything** ``{2}{W}`` 4/3 Uncommon Artifact Creature — Toy
  — ``Double strike`` plus
  ``This creature enters with two -1/-1 counters on it if you cast it
  from your hand.`` The new
  ``CardPrinting.enters_with_minus_one_counters_from_hand_cast``
  field fires from ``actions.put_into_battlefield`` only when the
  GameObject's ``cast_via_hand`` flag is set. ``cast_via_hand`` is
  flipped in ``actions._cast_spell``'s cast-from-hand branch and
  cleared on every other cast path (warp-from-graveyard, cast-from-
  exile, cast-from-top-of-library), so the rider stays inert on
  non-hand-origin resolutions exactly as the printed text gates.

The shared parametrized parity test in the existing parser test
corpus enforces full ``CardPrinting`` parity, so this file focuses on
the new printing-level fields, the line recognizers, the engine
hooks, and the end-to-end claim assertions.
"""

# mtg-zero (docs/ENGINE.md 169): 11 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_cast_via_hand_cleared_when_returning_via_blink, test_cast_via_hand_default_false_on_new_object, test_creatures_died_this_turn_tracker_clones_via_state_clone, test_creatures_died_this_turn_tracker_increments_on_death, test_creatures_died_this_turn_tracker_no_increment_on_exile, test_creatures_died_this_turn_tracker_no_increment_on_noncreature, test_creatures_died_this_turn_tracker_resets_at_cleanup, test_etb_replacement_minus_one_counters_from_hand_cast_only, test_etb_replacement_minus_one_counters_inactive_when_not_from_hand, test_etb_replacement_plus_one_counter_active_when_creature_died, test_etb_replacement_plus_one_counter_inactive_with_no_deaths

from __future__ import annotations

from mtgz_ir.cards.parser.printing_keywords import (
    _parse_enters_with_minus_one_counters_from_hand_cast_line,
    _parse_enters_with_plus_one_counter_if_creature_died_line,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- ``enters with +1/+1 counter if a creature died`` recognizer ----------


def test_parse_enters_with_plus_one_counter_if_creature_died_match():
    # Cackling Slasher's verbatim surface form. The recognizer is a
    # literal-string match — no card today varies the wording — so a
    # forward-looking surface variant ("if any creature died" / "if a
    # creature has died") would intentionally not match and would land
    # its own recognizer rather than parameterize this one.
    assert _parse_enters_with_plus_one_counter_if_creature_died_line(
        "This creature enters with a +1/+1 counter on it if a "
        "creature died this turn."
    ) is True


def test_parse_enters_with_plus_one_counter_if_creature_died_unrelated():
    # A near-miss surface that drops the death-condition tail must NOT
    # match — the bare ``This creature enters with a +1/+1 counter on
    # it.`` body is a different printed mechanic (no DSK card today,
    # but the recognizer must stay narrow). Belt-and-suspenders against
    # accidentally widening the literal-string recognizer.
    assert _parse_enters_with_plus_one_counter_if_creature_died_line(
        "This creature enters with a +1/+1 counter on it."
    ) is False


# ---------- ``enters with N -1/-1 counters from hand cast`` recognizer ----------


def test_parse_enters_with_minus_one_counters_from_hand_cast_two():
    # Patched Plaything's verbatim surface form. The cardinal ``two``
    # round-trips to ``int(2)`` via the spelled-out cardinal table so
    # the engine field stores a plain integer count.
    assert _parse_enters_with_minus_one_counters_from_hand_cast_line(
        "This creature enters with two -1/-1 counters on it if you "
        "cast it from your hand."
    ) == 2


def test_parse_enters_with_minus_one_counters_from_hand_cast_digit():
    # Forward-looking digit form. Same engine field; the recognizer's
    # ``\d+`` arm picks it up so a future printing that prints the
    # numeric ``2`` (or any other count) doesn't need a grammar
    # change. No DSK printing today uses this surface.
    assert _parse_enters_with_minus_one_counters_from_hand_cast_line(
        "This creature enters with 3 -1/-1 counters on it if you cast "
        "it from your hand."
    ) == 3


def test_parse_enters_with_minus_one_counters_from_hand_cast_singular():
    # Forward-looking ``one`` singular variant — the regex's
    # alternation includes the spelled-out cardinal and the ``counters?``
    # arm tolerates the singular "counter" tail. The engine accepts any
    # positive integer for the field; a 1-counter rider would resolve
    # the same way as the 2-counter one (Patched Plaything) at ETB.
    assert _parse_enters_with_minus_one_counters_from_hand_cast_line(
        "This creature enters with one -1/-1 counter on it if you cast "
        "it from your hand."
    ) == 1


def test_parse_enters_with_minus_one_counters_from_hand_cast_unrelated():
    # A near-miss line that swaps the cast-zone gate for an unrelated
    # phrase must NOT match — the recognizer is anchored on the
    # ``if you cast it from your hand`` tail. Belt-and-suspenders
    # against accidentally widening the regex to a generic ETB-counter
    # rider.
    assert _parse_enters_with_minus_one_counters_from_hand_cast_line(
        "This creature enters with two -1/-1 counters on it if it's "
        "your turn."
    ) is None


# ---------- engine hook: creatures_died_this_turn tracker ----------












# ---------- engine hook: cast_via_hand provenance flag ----------




# ---------- engine hook: ETB replacements ----------










# ---------- end-to-end claim assertions ----------


def test_cackling_slasher_claimed_by_parser():
    # Cackling Slasher ``{3}{B}`` 3/3 Common Human Assassin. Two oracle
    # lines: ``Deathtouch`` and ``This creature enters with a +1/+1
    # counter on it if a creature died this turn.`` Claims via the
    # existing keyword-line grammar for the first line plus the new
    # printing-level ETB-replacement field for the second.
    assert "Cackling Slasher" in _claimed_names()


def test_cackling_slasher_carries_etb_replacement_field():
    # Belt-and-suspenders that the claimed printing actually carries
    # ``enters_with_plus_one_counter_if_creature_died_this_turn=True``
    # — a regression that dropped the field would silently always-on
    # the rider (or never fire it), which Cackling Slasher's printed
    # text doesn't authorize.
    src = OracleParserCardSource()
    cs = src.get_printings()["Cackling Slasher"]
    assert "deathtouch" in cs.keywords
    assert (
        cs.enters_with_plus_one_counter_if_creature_died_this_turn is True
    )


def test_patched_plaything_claimed_by_parser():
    # Patched Plaything ``{2}{W}`` 4/3 Uncommon Artifact Creature — Toy.
    # Two oracle lines: ``Double strike`` and ``This creature enters
    # with two -1/-1 counters on it if you cast it from your hand.``
    # Claims via the existing keyword-line grammar for the first line
    # plus the new printing-level ETB-replacement field for the
    # second.
    assert "Patched Plaything" in _claimed_names()


def test_patched_plaything_carries_etb_replacement_field():
    # Belt-and-suspenders that the claimed printing actually carries
    # ``enters_with_minus_one_counters_from_hand_cast=2`` — the
    # printed two-counter count round-trips through the spelled-
    # cardinal table. A regression that dropped or zeroed the field
    # would silently leave Patched Plaything as a 4/3 with no -1/-1
    # rider regardless of cast zone.
    src = OracleParserCardSource()
    pp = src.get_printings()["Patched Plaything"]
    assert "double strike" in pp.keywords
    assert pp.enters_with_minus_one_counters_from_hand_cast == 2


# ---------- defensive: cast-paths clear cast_via_hand ----------


