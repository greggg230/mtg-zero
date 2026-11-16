"""Tests for additional ``As long as ...`` static-ability conditions.

The static-ability grammar maps a small phrasebook of "As long as
<condition>, this creature gets ..." surface forms to ``StaticAbility``
gate kwargs. This file covers the conditions added beyond the original
``you control an artifact`` entry:

* ``you've cast two or more spells this turn`` →
  ``requires_spells_cast_this_turn=2``

The corresponding engine gate is exercised by the static registry on
Mm'menon, Uthros Exile, but Mm'menon's printed oracle text uses a
different surface form (``~ gets +N/+N as long as ...``, with the
rider trailing) that the static-ability regex does not match. Adding
the leading-``As long as`` phrase to the phrasebook is forward-looking
grammar coverage — no EoE card unlocks today, but oracle text in this
shape will parse correctly.

The existing ``you control an artifact`` claims (Cloudsculpt
Technician, Gravblade Heavy) are pinned via the parser-source
end-to-end check to guard against regressions when the phrasebook is
extended.

Other candidate conditions:

* ``it's your turn`` → ``active_turn_only=True``. Added in batch 10.
  See ``test_parser_statics_batch10.py`` for the dedicated coverage.
* ``you have void`` — has no matching ``StaticAbility`` gate field
  (only ``TriggeredAbility.requires_void`` exists), so the parser
  must fall through.
* ``you control a/an <creature-type>`` — no matching engine field.
"""

from __future__ import annotations

from mtgz_ir.cards.base import StaticAbility, StaticScope
from mtgz_ir.cards.parser.statics import (
    _STATIC_CONDITION_KWARGS,
    _parse_static_ability_line,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.conditions import AtLeastNSpellsCastThisTurn
from mtgz_ir.effects.modifiers import (
    ContinuousModifier,
    GrantKeyword,
    PowerToughnessBonus,
)


# ---------- "you've cast two or more spells this turn" → requires_spells_cast_this_turn=2 ----------


def test_spells_cast_two_or_more_parses_to_static():
    # PR-D continuous-effects shape: P/T contribution rides on a
    # layer-7c ContinuousModifier(PowerToughnessBonus); the legacy
    # requires_spells_cast_this_turn gate stays as the surface today.
    parsed = _parse_static_ability_line(
        "As long as you've cast two or more spells this turn, this "
        "creature gets +2/+0."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=2, toughness_bonus=0),
        ),
        condition=AtLeastNSpellsCastThisTurn(2),
    ),)


def test_spells_cast_two_or_more_with_keyword_rider():
    # The ``and has <keyword>`` tail must compose with the new
    # condition the same way it composes with the artifact condition.
    # Compound P/T + keyword statics fan out into one StaticAbility
    # for the layer-7c P/T contribution + one per keyword for the
    # layer-6 GrantKeyword.
    parsed = _parse_static_ability_line(
        "As long as you've cast two or more spells this turn, this "
        "creature gets +2/+0 and has flying."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(
                    power_bonus=2, toughness_bonus=0,
                ),
            ),
            condition=AtLeastNSpellsCastThisTurn(2),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("flying"),
            ),
            condition=AtLeastNSpellsCastThisTurn(2),
        ),
    )


def test_spells_cast_two_or_more_does_not_set_other_gates():
    # A condition must set its own gate field and leave every other
    # gate at its default. Otherwise two conditions would conflict
    # if they ever appeared on the same printing.
    parsed = _parse_static_ability_line(
        "As long as you've cast two or more spells this turn, this "
        "creature gets +1/+1."
    )
    assert len(parsed) == 1
    static = parsed[0]
    assert static.condition == AtLeastNSpellsCastThisTurn(2)
    assert static.tier_threshold == 0


def test_spells_cast_two_or_more_rejects_unknown_keyword_rider():
    # Same conservative behavior as the artifact condition: a
    # non-evergreen keyword in the ``and has`` tail fails the parse
    # rather than silently dropping the rider.
    assert (
        _parse_static_ability_line(
            "As long as you've cast two or more spells this turn, this "
            "creature gets +2/+0 and has scry."
        )
        == ()
    )


# ---------- Negative: differently-worded forms must not match ----------


def test_one_spell_form_returns_none():
    # ``one or more`` is a different threshold than ``two or more``
    # and isn't in the phrasebook — the parser must not silently
    # match the wrong surface phrase.
    assert (
        _parse_static_ability_line(
            "As long as you've cast one or more spells this turn, this "
            "creature gets +2/+0."
        )
        == ()
    )


def test_unrecognized_void_condition_returns_none():
    # ``you have void`` is a real EoE mechanic, but ``StaticAbility``
    # has no gate field for it (only ``TriggeredAbility`` does), so
    # the static parser must fall through.
    assert (
        _parse_static_ability_line(
            "As long as you have void, this creature gets +1/+0."
        )
        == ()
    )


def test_unrecognized_creature_type_condition_returns_none():
    # ``you control a creature`` is plausible English but has no
    # corresponding ``StaticAbility`` gate field, so it must not
    # produce a ``StaticAbility`` — better to skip the card than
    # silently mis-model it as always-on.
    assert (
        _parse_static_ability_line(
            "As long as you control a creature, this creature gets +1/+1."
        )
        == ()
    )


# ---------- Phrasebook integrity ----------


def test_each_condition_phrase_parses_into_a_static_ability():
    # Every entry in the phrasebook must yield a ``StaticAbility`` when
    # plugged into a minimal surface line — guards against typos in
    # the dict keys (e.g. trailing whitespace, capitalization drift)
    # that would silently fall through.
    for phrase in _STATIC_CONDITION_KWARGS:
        line = f"As long as {phrase}, this creature gets +1/+0."
        result = _parse_static_ability_line(line)
        assert result is not None, (
            f"phrasebook entry {phrase!r} did not round-trip through "
            "the parser"
        )


def test_phrasebook_includes_the_new_condition():
    # Pin the new entry so an accidental revert is loud.
    assert "you've cast two or more spells this turn" in (
        _STATIC_CONDITION_KWARGS
    )
    assert _STATIC_CONDITION_KWARGS[
        "you've cast two or more spells this turn"
    ] == {"condition": AtLeastNSpellsCastThisTurn(2)}


# ---------- End-to-end: existing claims still resolve ----------


def test_existing_artifact_condition_cards_still_claim_end_to_end():
    # The artifacts-matter rider was the original condition. Adding
    # new entries to the phrasebook must not break the existing
    # claims — Cloudsculpt Technician (Flying + +1/+0) and Gravblade
    # Heavy (+1/+0 and deathtouch) are the two EoE cards that ride
    # on this gate and are exercised end-to-end through the parser
    # source.
    claimed = OracleParserCardSource().claimed_names
    assert "Cloudsculpt Technician" in claimed
    assert "Gravblade Heavy" in claimed
