"""Tests for the bespoke-spells2 migration batch.

This batch migrates four entries out of ``_WHOLE_SPELL_EXACT_TEXT`` in
``parser.spell_builder`` by introducing five grammar extensions:

1. **Self-shuffle rider dropper** (``_is_self_shuffle_clause``) — recognizes
   the printed self-shuffle replacement and drops it from the
   effect-clause stream.  The behavior is wired via a SELF-scope
   StaticAbility on the spell printing carrying a
   ``ReplacementEffect(WOULD_GO_TO_GRAVEYARD,
   Replace(new_destination="library"))`` (built by
   ``_detect_shuffle_self_statics``); dropping the clause lets the
   remaining effect clause parse cleanly through the unified grammar.

   One shape recognized: ``If <Name> would be put into a graveyard from
   anywhere, reveal <Name> and shuffle it into its owner's library
   instead.`` (Nexus of Fate form). Green Sun's Zenith's ``Shuffle
   <Name> into its owner's library.`` is the last instruction of its
   effect (``ShuffleThisSpellIntoOwnersLibrary``, 2026-10-24), not
   dropped.

2. **``_TAKE_EXTRA_TURN_AFTER_THIS_ONE`` recognizer** (effects.py) — maps
   "take an extra turn after this one" to ``TakeExtraTurnAfterThisOne()``.
   Migrates: **Nexus of Fate** (main clause; Nexus rider dropped by #1).

3. **``_SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD`` recognizer**
   (effects.py) — maps "search your library for a green creature card with
   mana value X or less, put it onto the battlefield, then shuffle" to
   ``SearchLibraryForGreenCreatureWithMVAtMostX()``.
   Migrates: **Green Sun's Zenith** (main clause; its self-shuffle
   sentence is the ``Sequence``'s second step).

4. **``_SURVEIL_X_ARTIFACT_COUNT`` recognizer** (effects.py) — maps "surveil
   X, where X is the number of artifacts you control" to
   ``Surveil(amount=1, use_artifact_count=True)``.  Together with the
   existing ``_DRAW_N_CARDS`` recognizer (after ``_split_effect_sentences``
   strips the leading "Then "), the Sequence combinator produces the same
   ``Sequence(Surveil(...), DrawCards(3))`` the static side ships.
   Migrates: **Cerebral Download**.

5. **``_PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE`` recognizer** (effects.py) —
   maps "until end of turn, target creature gets +N/+M, gains <kw>, and
   becomes <type-change> in addition to its other types" to
   ``PumpTargetCreatureUntilEOT(power, toughness, granted_keywords)``,
   dropping the unimplemented type-change rider.
   Migrates: **Jump Scare**.

6. **``_DEPRESSURIZE`` recognizer** (effects.py) — a parameterized multi-
   sentence literal matching "target creature gets -N/-0 until end of turn.
   Then if that creature's power is 0 or less, destroy it" and returning
   ``DepressurizeEffect(power_penalty=N)``.
   Migrates: **Depressurize**.

Total migration: 5 entries removed from ``_WHOLE_SPELL_EXACT_TEXT``.
Table shrinks from 27 to 22.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    DepressurizeEffect,
    DrawCards,
    PumpTargetCreatureUntilEOT,
    SearchLibraryForGreenCreatureWithMVAtMostX,
    Sequence,
    Surveil,
    TakeExtraTurnAfterThisOne,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_spell_effect_phrase,
    _DEPRESSURIZE,
    _PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.spell_builder import _is_self_shuffle_clause, _SELF_SHUFFLE_NEXUS_RE, _parse_spell_oracle_text
from mtgz_ir.effects.generic.card_flow import ShuffleThisSpellIntoOwnersLibrary


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# Table-size regression covered comprehensively by
# tests/test_parser_bespoke_spells1.py::test_bespoke_table_count.

# ------------------------------------------------------------------ #
# Self-shuffle clause regexes                                         #
# ------------------------------------------------------------------ #

def test_self_shuffle_nexus_re_matches_nexus_form():
    assert _SELF_SHUFFLE_NEXUS_RE.match(
        "If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead."
    ) is not None


def test_self_shuffle_nexus_re_matches_generic_name():
    assert _SELF_SHUFFLE_NEXUS_RE.match(
        "If Widget would be put into a graveyard from anywhere, "
        "reveal Widget and shuffle it into its owner's library "
        "instead."
    ) is not None


def test_self_shuffle_nexus_re_no_false_positive_on_gsz_form():
    # The GSZ form starts with "Shuffle", not "If" — must not match
    # the Nexus regex.
    assert _SELF_SHUFFLE_NEXUS_RE.match(
        "Shuffle Green Sun's Zenith into its owner's library."
    ) is None


# ------------------------------------------------------------------ #
# _is_self_shuffle_clause                                             #
# ------------------------------------------------------------------ #

def test_is_self_shuffle_clause_keeps_the_gsz_instruction():
    # "Shuffle <Name> into its owner's library." is an instruction of
    # the spell's effect (done only as it resolves), not a replacement:
    # it stays in the clause stream.
    assert not _is_self_shuffle_clause(
        "Shuffle Green Sun's Zenith into its owner's library."
    )


def test_is_self_shuffle_clause_nexus_form():
    assert _is_self_shuffle_clause(
        "If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead."
    )


def test_is_self_shuffle_clause_strips_whitespace():
    # The helper must tolerate leading/trailing whitespace (mirrors the
    # other _is_* helpers in spell_builder).
    assert _is_self_shuffle_clause(
        "  If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead.  "
    )


def test_is_self_shuffle_clause_rejects_ordinary_clauses():
    assert not _is_self_shuffle_clause("Destroy target creature.")
    assert not _is_self_shuffle_clause("Counter target spell.")
    assert not _is_self_shuffle_clause(
        "Take an extra turn after this one."
    )
    assert not _is_self_shuffle_clause(
        "If Nexus of Fate would be put into a graveyard, shuffle it."
    )  # Slightly different wording — must not match (not an exact form).


# ------------------------------------------------------------------ #
# _TAKE_EXTRA_TURN_AFTER_THIS_ONE recognizer                          #
# ------------------------------------------------------------------ #

def test_take_extra_turn_parses():
    eff = _parse_effect("take an extra turn after this one")
    assert eff == TakeExtraTurnAfterThisOne()


def test_take_extra_turn_parses_as_spell_effect_phrase():
    eff = _parse_spell_effect_phrase(
        "Take an extra turn after this one.",
        "Nexus of Fate",
    )
    assert eff == TakeExtraTurnAfterThisOne()


def test_take_extra_turn_does_not_match_partial():
    # A slight variation should NOT parse — guarding against accidental
    # fuzzy matching.
    assert _parse_effect("take an extra turn") is None
    assert _parse_effect("take an extra turn now") is None


# ------------------------------------------------------------------ #
# _SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD recognizer          #
# ------------------------------------------------------------------ #

def test_search_library_green_creature_x_parses():
    eff = _parse_effect(
        "search your library for a green creature card with mana value X "
        "or less, put it onto the battlefield, then shuffle"
    )
    assert eff == SearchLibraryForGreenCreatureWithMVAtMostX()


def test_search_library_green_creature_x_parses_as_spell_phrase():
    eff = _parse_spell_effect_phrase(
        "Search your library for a green creature card with mana value X "
        "or less, put it onto the battlefield, then shuffle.",
        "Green Sun's Zenith",
    )
    assert eff == SearchLibraryForGreenCreatureWithMVAtMostX()


def test_search_library_green_creature_x_does_not_match_hand_tutor():
    # The hand-tutor shape (reveal it, put it into your hand, then shuffle)
    # must NOT be claimed by this recognizer.
    assert _parse_effect(
        "search your library for a green creature card with mana value X "
        "or less, reveal it, put it into your hand, then shuffle"
    ) is None


# ------------------------------------------------------------------ #
# _SURVEIL_X_ARTIFACT_COUNT recognizer                                #
# ------------------------------------------------------------------ #

def test_surveil_x_artifact_count_parses():
    eff = _parse_effect(
        "surveil X, where X is the number of artifacts you control"
    )
    assert eff == Surveil(amount=1, use_artifact_count=True)


def test_surveil_n_still_parses():
    # Regression: the existing bare ``surveil N`` recognizer must still
    # work after the artifact-count variant was added before it.
    from mtgz_ir.cards.base import Surveil as S
    assert _parse_effect("surveil 1") == S(amount=1)
    assert _parse_effect("surveil 2") == S(amount=2)


def test_surveil_x_no_match_on_surveil_n():
    # The artifact-count regex must NOT match bare ``surveil 2`` etc.
    # (regression check for over-broad regex).
    from mtgz_ir.cards.parser.effects import _SURVEIL_X_ARTIFACT_COUNT
    assert _SURVEIL_X_ARTIFACT_COUNT.match("surveil 2") is None
    assert _SURVEIL_X_ARTIFACT_COUNT.match("surveil X") is None


# ------------------------------------------------------------------ #
# Nexus of Fate migration                                             #
# ------------------------------------------------------------------ #

def test_nexus_of_fate_claimed_by_parser():
    assert "Nexus of Fate" in _claimed_names()


def test_nexus_of_fate_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Nexus of Fate"]
    assert p.spell_effect == TakeExtraTurnAfterThisOne()


def test_nexus_of_fate_shuffles_static():
    from mtgz_ir.effects.modifiers import (
        EventPattern, Replace, ReplacementEffect,
    )
    from mtgz_ir.effects.predicates import Event

    parser = OracleParserCardSource()
    p = parser.get_printings()["Nexus of Fate"]
    expected = ReplacementEffect(
        event_pattern=EventPattern(event=Event.WOULD_GO_TO_GRAVEYARD),
        transform=Replace(new_destination="library"),
    )
    assert any(
        s.replacement_effect == expected for s in p.static_abilities
    )


def test_nexus_of_fate_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Take an extra turn after this one.\n"
        "If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead.",
        "Nexus of Fate",
    )
    assert eff == TakeExtraTurnAfterThisOne()


# ------------------------------------------------------------------ #
# Green Sun's Zenith migration                                        #
# ------------------------------------------------------------------ #

def test_green_suns_zenith_claimed_by_parser():
    assert "Green Sun's Zenith" in _claimed_names()


def test_green_suns_zenith_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Green Sun's Zenith"]
    assert p.spell_effect == Sequence(effects=(
        SearchLibraryForGreenCreatureWithMVAtMostX(),
        ShuffleThisSpellIntoOwnersLibrary(),
    ))


def test_green_suns_zenith_has_no_shuffle_replacement_static():
    # "Shuffle Green Sun's Zenith into its owner's library." is done
    # only as the spell resolves (its effect's last step), not "if it
    # would be put into a graveyard from anywhere" (Nexus of Fate).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Green Sun's Zenith"]
    assert p.static_abilities == ()


def test_green_suns_zenith_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Search your library for a green creature card with mana "
        "value X or less, put it onto the battlefield, then shuffle. "
        "Shuffle Green Sun's Zenith into its owner's library.",
        "Green Sun's Zenith",
    )
    assert eff == Sequence(effects=(
        SearchLibraryForGreenCreatureWithMVAtMostX(),
        ShuffleThisSpellIntoOwnersLibrary(),
    ))


# ------------------------------------------------------------------ #
# Cerebral Download migration                                         #
# ------------------------------------------------------------------ #

def test_cerebral_download_claimed_by_parser():
    assert "Cerebral Download" in _claimed_names()


def test_cerebral_download_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Cerebral Download"]
    assert p.spell_effect == Sequence(effects=(
        Surveil(amount=1, use_artifact_count=True),
        DrawCards(amount=3),
    ))


def test_cerebral_download_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Surveil X, where X is the number of artifacts you control. "
        "Then draw three cards.",
        "Cerebral Download",
    )
    assert eff == Sequence(effects=(
        Surveil(amount=1, use_artifact_count=True),
        DrawCards(amount=3),
    ))


# ------------------------------------------------------------------ #
# Jump Scare migration                                                #
# ------------------------------------------------------------------ #

def test_jump_scare_leading_ueot_regex_matches():
    # The leading-UEOT + type-change-rider regex must match Jump Scare's
    # normalized oracle text verbatim.
    m = _PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE.match(
        "until end of turn, target creature gets +2/+2, gains flying, "
        "and becomes a Horror enchantment creature in addition to its "
        "other types"
    )
    assert m is not None
    assert m.group(1) == "+2"
    assert m.group(2) == "+2"
    assert m.group(3) == "flying"


def test_jump_scare_leading_ueot_regex_no_false_positive_bare_pump():
    # The trailing-UEOT bare pump must NOT be claimed by this regex.
    assert _PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE.match(
        "target creature gets +2/+2 until end of turn"
    ) is None


def test_jump_scare_leading_ueot_regex_no_false_positive_trailing_ueot_kw():
    # The trailing-UEOT pump-and-gain-keywords shape must NOT match.
    assert _PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE.match(
        "target creature gets +2/+2 and gains flying until end of turn"
    ) is None


def test_jump_scare_parses_as_effect():
    eff = _parse_effect(
        "until end of turn, target creature gets +2/+2, gains flying, "
        "and becomes a Horror enchantment creature in addition to its "
        "other types"
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=2,
        toughness_bonus=2,
        granted_keywords=frozenset({"flying"}),
    )


def test_jump_scare_parses_as_spell_effect_phrase():
    eff = _parse_spell_effect_phrase(
        "Until end of turn, target creature gets +2/+2, gains flying, "
        "and becomes a Horror enchantment creature in addition to its "
        "other types.",
        "Jump Scare",
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=2,
        toughness_bonus=2,
        granted_keywords=frozenset({"flying"}),
    )


def test_jump_scare_claimed_by_parser():
    assert "Jump Scare" in _claimed_names()


def test_jump_scare_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Jump Scare"]
    assert p.spell_effect == PumpTargetCreatureUntilEOT(
        power_bonus=2,
        toughness_bonus=2,
        granted_keywords=frozenset({"flying"}),
    )


def test_jump_scare_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Until end of turn, target creature gets +2/+2, gains "
        "flying, and becomes a Horror enchantment creature in "
        "addition to its other types.",
        "Jump Scare",
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=2,
        toughness_bonus=2,
        granted_keywords=frozenset({"flying"}),
    )


# ------------------------------------------------------------------ #
# Depressurize migration                                              #
# ------------------------------------------------------------------ #

def test_depressurize_regex_matches():
    m = _DEPRESSURIZE.match(
        "target creature gets -3/-0 until end of turn. "
        "Then if that creature's power is 0 or less, destroy it"
    )
    assert m is not None
    assert m.group(1) == "-3"
    assert m.group(2) == "-0"


def test_depressurize_regex_parameterized():
    # A different power penalty should also match — verifying the
    # recognizer is parameterized, not a fixed-value literal.
    m = _DEPRESSURIZE.match(
        "target creature gets -4/-0 until end of turn. "
        "Then if that creature's power is 0 or less, destroy it"
    )
    assert m is not None
    assert m.group(1) == "-4"


def test_depressurize_regex_no_false_positive_bare_pump():
    # A bare -N/-0 pump (no conditional destroy) must NOT match.
    assert _DEPRESSURIZE.match(
        "target creature gets -3/-0 until end of turn"
    ) is None


def test_depressurize_parses_as_effect():
    eff = _parse_effect(
        "target creature gets -3/-0 until end of turn. "
        "Then if that creature's power is 0 or less, destroy it"
    )
    assert eff == DepressurizeEffect(power_penalty=3)


def test_depressurize_parses_as_spell_effect_phrase():
    eff = _parse_spell_effect_phrase(
        "Target creature gets -3/-0 until end of turn. "
        "Then if that creature's power is 0 or less, destroy it.",
        "Depressurize",
    )
    assert eff == DepressurizeEffect(power_penalty=3)


def test_depressurize_claimed_by_parser():
    assert "Depressurize" in _claimed_names()


def test_depressurize_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Depressurize"]
    assert p.spell_effect == DepressurizeEffect(power_penalty=3)


def test_depressurize_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Target creature gets -3/-0 until end of turn. "
        "Then if that creature's power is 0 or less, destroy it.",
        "Depressurize",
    )
    assert eff == DepressurizeEffect(power_penalty=3)


# ------------------------------------------------------------------ #
# Self-shuffle dropper integration: does not affect non-shuffle spells#
# ------------------------------------------------------------------ #

def test_self_shuffle_dropper_does_not_affect_plasma_bolt():
    # Belt-and-suspenders: a two-clause Void-replacement spell whose
    # second clause is NOT a self-shuffle rider must still parse via
    # the ConditionalIfVoid path (not be accidentally dropped).
    from mtgz_ir.cards.base import ConditionalIfVoid, DealDamageAnyTarget
    eff = _parse_spell_oracle_text(
        "Plasma Bolt deals 3 damage to any target.\n"
        "Void — Plasma Bolt deals 5 damage instead "
        "if a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn.",
        "Plasma Bolt",
    )
    assert eff == ConditionalIfVoid(
        then_effect=DealDamageAnyTarget(amount=5),
        else_effect=DealDamageAnyTarget(amount=3),
    )


def test_self_shuffle_dropper_does_not_affect_single_clause_spell():
    # A single-clause spell must still parse normally (no regression
    # from the new self-shuffle dropping pass).
    from mtgz_ir.cards.base import DestroyTargetCreature
    eff = _parse_spell_oracle_text(
        "Destroy target creature.",
        "Some Destroy Spell",
    )
    assert eff == DestroyTargetCreature()
