"""Tests for batch-17 whole-spell exact-text dispatch additions plus
the self-shuffle StaticAbility detector.

Batch 17 originally extended the ``_WHOLE_SPELL_EXACT_TEXT`` dispatcher
with three entries (Darkness, Nexus of Fate, Green Sun's Zenith) and
added a name-aware self-shuffle rider detector that produces a SELF-
scope ``StaticAbility(replacement_effect=ReplacementEffect(
WOULD_GO_TO_GRAVEYARD, Replace(new_destination="library")))`` per
matching spell — historically a printing-flag detector
``_detect_shuffle_self_flags``, now ``_detect_shuffle_self_statics``.

In bespoke-spells2, **Nexus of Fate** and **Green Sun's Zenith** were
further migrated OUT of ``_WHOLE_SPELL_EXACT_TEXT`` to the per-clause
grammar:
  - Nexus of Fate: the new ``_TAKE_EXTRA_TURN_AFTER_THIS_ONE`` recognizer
    in ``effects.py`` claims "Take an extra turn after this one." as
    ``TakeExtraTurnAfterThisOne()``; the Nexus self-shuffle rider
    (second oracle line) is dropped by ``_is_self_shuffle_clause``.
  - Green Sun's Zenith: the new ``_SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD``
    recognizer claims the tutor sentence as
    ``SearchLibraryForGreenCreatureWithMVAtMostX()``; the self-shuffle
    sentence (second sentence on the same line, split off by
    ``_split_sentences``) is the next step of the spell's ``Sequence``,
    ``ShuffleThisSpellIntoOwnersLibrary()`` (2026-10-24; it was dropped
    and wired as Nexus of Fate's replacement static before).

Darkness remains in the whole-spell dispatcher (its
``PreventAllCombatDamageThisTurn`` primitive lives in the per-clause
grammar, not the whole-spell tier — Darkness is claimed there).

The dispatch tests for Nexus and GSZ are updated to assert that the
whole-spell tier falls through (returns ``None``) while the per-clause
grammar claims them correctly.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CloseEncounterEffect,
    ConsultTheStarChartsEffect,
    CounterTargetSpell,
    CreateToken,
    DealNDamageToEachCreature,
    ExileTargetCreatureWithPowerOrToughnessAtMost,
    ModalChooseOne,
    OptionalSacArtifactForEffect,
    PreventAllCombatDamageThisTurn,
    SearchLibraryForGreenCreatureWithMVAtMostX,
    Sequence,
    TakeExtraTurnAfterThisOne,
    TargetOpponentDiscardsCasterChoice,
)
from mtgz_ir.cards.parser.spell_builder import _detect_kicker_cost, _detect_shuffle_self_statics, _is_kicker_preamble, _kicker_cost
from mtgz_ir.effects.base import StaticAbility, StaticScope
from mtgz_ir.effects.modifiers import (
    EventPattern, Replace, ReplacementEffect,
)
from mtgz_ir.effects.predicates import Event


_SHUFFLE_SELF_STATIC = StaticAbility(
    scope=StaticScope.SELF,
    replacement_effect=ReplacementEffect(
        event_pattern=EventPattern(event=Event.WOULD_GO_TO_GRAVEYARD),
        transform=Replace(new_destination="library"),
    ),
)
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.cards.parser_source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- claim tests (end-to-end via the parser source) ----------

def test_darkness_claimed_by_parser():
    # Darkness ``{B}`` Instant — prevent all combat damage that would
    # be dealt this turn. Single-sentence spell whose effect engine
    # class (``PreventAllCombatDamageThisTurn``) sets a state flag the
    # combat damage step honors. The whole-spell exact-text dispatch
    # claims it because no per-clause primitive for "prevent all
    # combat damage" exists elsewhere in the EoE pool.
    assert "Darkness" in _claimed_names()


def test_nexus_of_fate_claimed_by_parser():
    # Nexus of Fate ``{5}{U}{U}`` Instant — take-extra-turn plus a
    # shuffle-instead-of-graveyard rider. Migrated to per-clause grammar
    # in bespoke-spells2; the new ``_TAKE_EXTRA_TURN_AFTER_THIS_ONE``
    # recognizer claims the main clause and ``_is_self_shuffle_clause``
    # drops the Nexus rider.
    assert "Nexus of Fate" in _claimed_names()


def test_green_suns_zenith_claimed_by_parser():
    # Green Sun's Zenith ``{X}{G}`` Sorcery — variable-X tutor for a
    # green creature plus a self-shuffle rider. Migrated to per-clause
    # grammar in bespoke-spells2; the new
    # ``_SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD`` recognizer
    # claims the tutor clause and the self-shuffle sentence is
    # ``ShuffleThisSpellIntoOwnersLibrary``.
    assert "Green Sun's Zenith" in _claimed_names()


# ---------- dispatch tests (direct lookup) ----------

def test_dispatch_darkness_returns_engine_class():
    # Darkness migrated from the whole-spell exact-text tier to the
    # per-clause grammar (effects.py's
    # `_PREVENT_ALL_COMBAT_DAMAGE_THIS_TURN`); whole-spell dispatch
    # now falls through. End-to-end claim verified via the active
    # source's printing.
    from mtgz_ir.cards.source import get_active_source
    darkness = get_active_source().get_printings().get("Darkness")
    assert darkness is not None
    assert darkness.spell_effect == PreventAllCombatDamageThisTurn()


def test_nexus_of_fate_parses_via_per_clause_grammar():
    # The per-clause grammar produces TakeExtraTurnAfterThisOne() for
    # Nexus of Fate's printed oracle text — the same effect the static
    # side ships.  The Nexus self-shuffle rider (second oracle line) is
    # dropped by ``_is_self_shuffle_clause``; the first clause is claimed
    # by ``_TAKE_EXTRA_TURN_AFTER_THIS_ONE``.
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Take an extra turn after this one.\n"
        "If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead.",
        "Nexus of Fate",
    )
    assert eff == TakeExtraTurnAfterThisOne()


def test_green_suns_zenith_parses_via_per_clause_grammar():
    # The per-clause grammar produces SearchLibraryForGreenCreatureWithMVAtMostX()
    # for Green Sun's Zenith's tutor sentence (claimed by
    # ``_SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD``), then
    # ShuffleThisSpellIntoOwnersLibrary() for its self-shuffle sentence
    # (split off by ``_split_sentences``; the name normalizes to "this
    # spell").
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    from mtgz_ir.effects.generic.card_flow import (
        ShuffleThisSpellIntoOwnersLibrary,
    )
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


# ---------- self-shuffle rider detector ----------

def test_detect_shuffle_self_statics_nexus_shape():
    # Nexus-of-Fate-style rider: ``If <Name> would be put into a
    # graveyard from anywhere, reveal <Name> and shuffle it into its
    # owner's library instead.``
    text = (
        "Take an extra turn after this one.\n"
        "If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead."
    )
    statics = _detect_shuffle_self_statics(text, "Nexus of Fate")
    assert statics == (_SHUFFLE_SELF_STATIC,)


def test_detect_shuffle_self_statics_gsz_shape_is_not_a_replacement():
    # GSZ's ``Shuffle <Name> into its owner's library.`` is the last
    # instruction of the spell's effect, done only as it resolves —
    # not a replacement static.
    text = (
        "Search your library for a green creature card with mana "
        "value X or less, put it onto the battlefield, then shuffle. "
        "Shuffle Green Sun's Zenith into its owner's library."
    )
    assert _detect_shuffle_self_statics(text, "Green Sun's Zenith") == ()


def test_detect_shuffle_self_statics_no_rider():
    # Spell text without a self-shuffle rider returns an empty tuple so
    # the caller's ``static_abilities=`` argument is a no-op.
    text = "Prevent all combat damage that would be dealt this turn."
    assert _detect_shuffle_self_statics(text, "Darkness") == ()


def test_detect_shuffle_self_statics_name_with_apostrophe():
    # A name with an apostrophe; the helper ``re.escape``-s the name so
    # the rider matches verbatim. Sanity-check that this works
    # (regression for a future card with similar punctuation).
    text = (
        "Do something.\n"
        "If Urza's Zenith would be put into a graveyard from anywhere, "
        "reveal Urza's Zenith and shuffle it into its owner's library "
        "instead."
    )
    statics = _detect_shuffle_self_statics(text, "Urza's Zenith")
    assert statics == (_SHUFFLE_SELF_STATIC,)


def test_detect_shuffle_self_statics_wrong_name():
    # The detector keys on the card's name, so a spell that mentions
    # a DIFFERENT card's shuffle rider doesn't trip the static. (No
    # real card in the EoE pool does this — defensive regression.)
    text = (
        "Take an extra turn after this one.\n"
        "If Nexus of Fate would be put into a graveyard from anywhere, "
        "reveal Nexus of Fate and shuffle it into its owner's library "
        "instead."
    )
    assert _detect_shuffle_self_statics(text, "Some Other Card") == ()


# ---------- Lithobraking and Temporal Intervention ----------

def test_lithobraking_claimed_by_parser():
    # Lithobraking ``{2}{R}`` Instant — create-Lander + reflexive-
    # trigger optional sac. Migrated to per-clause grammar in
    # bespoke-spells3: the new ``_YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO``
    # recognizer claims "Then you may sacrifice an artifact. When you
    # do, …" as ``OptionalSacArtifactForEffect``; the Sequence
    # combinator assembles it with ``CreateToken(Lander)``.
    assert "Lithobraking" in _claimed_names()


def test_lithobraking_parses_via_per_clause_grammar():
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Create a Lander token. Then you may sacrifice an artifact. "
        "When you do, Lithobraking deals 2 damage to each creature.",
        "Lithobraking",
    )
    assert eff == Sequence(effects=(
        CreateToken(
            token_printing=TOKEN_PRINTINGS["Lander"], count=1,
        ),
        OptionalSacArtifactForEffect(
            inner_effect=DealNDamageToEachCreature(amount=2),
        ),
    ))


def test_temporal_intervention_claimed_by_parser():
    # Temporal Intervention ``{2}{B}`` Sorcery — 3-sentence directed
    # discard plus a Void-prefixed cost-reduction rider. The
    # cost-reduction parser's Void-prefix variant emits
    # ``CostReduction(2, "void_active")``; the body effect is now
    # claimed via the per-clause grammar
    # (``TargetOpponentDiscardsCasterChoice``) after the Void
    # cost-reduction line is dropped by ``_parse_spell_oracle_text``.
    assert "Temporal Intervention" in _claimed_names()


def test_directed_discard_body_parses_via_per_clause_grammar():
    # The 3-sentence directed-discard body now parses directly via the
    # per-clause effect grammar (``_parse_spell_effect_phrase``).  The
    # normalizer strips the card-name prefix; since Temporal
    # Intervention's body uses no name prefix, the body is passed
    # verbatim as a single oracle line.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Target opponent reveals their hand. "
        "You choose a nonland card from it. "
        "That player discards that card."
    )
    eff = _parse_spell_effect_phrase(body, "Temporal Intervention")
    assert eff == TargetOpponentDiscardsCasterChoice()


# ---------- Close Encounter and Consult the Star Charts ----------

def test_close_encounter_claimed_by_parser():
    # Close Encounter ``{1}{G}`` Instant — power-as-damage burn whose
    # source is chosen as an additional cost (a creature you control
    # or a warped creature card you own in exile). Migrated from the
    # whole-spell exact-text tier to the per-clause grammar in
    # bespoke-final-spells: the additional-cost preamble is stripped by
    # ``_is_additional_cost_preamble``; the body normalizes to "deals
    # damage equal to the power of the chosen creature or card to target
    # creature" which the new literal in ``effects.py`` claims as
    # ``CloseEncounterEffect()``.
    assert "Close Encounter" in _claimed_names()


def test_dispatch_close_encounter_body_parses_via_per_clause_grammar():
    # After the additional-cost preamble is stripped, the body clause
    # normalizes to "deals damage equal to the power of the chosen
    # creature or card to target creature" and is claimed by the new
    # literal recognizer in ``_parse_effect``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    body = (
        "Close Encounter deals damage equal to the power of the "
        "chosen creature or card to target creature."
    )
    eff = _parse_spell_effect_phrase(body, "Close Encounter")
    assert eff == CloseEncounterEffect()


def test_consult_the_star_charts_claimed_by_parser():
    # Consult the Star Charts ``{1}{U}`` Instant with Kicker ``{1}{U}``.
    # The Kicker preamble is detected and threaded onto the printing
    # via the ``kicker`` kwarg; the 4-sentence body is claimed by the
    # new multi-sentence literal in ``effects.py`` (migrated from the
    # whole-spell exact-text tier in bespoke-spells6).
    assert "Consult the Star Charts" in _claimed_names()


# ---------- Kicker preamble detector ----------

def test_is_kicker_preamble_recognizes_canonical_shape():
    # Canonical Kicker preamble: keyword + cost + parenthesized
    # reminder describing the additional cost.
    assert _is_kicker_preamble(
        "Kicker {1}{U} (You may pay an additional {1}{U} as you cast "
        "this spell.)"
    )


def test_is_kicker_preamble_rejects_non_kicker_lines():
    # Plain effect lines, Convoke (handled by the keyword whitelist),
    # and additional-cost preambles all fall through.
    assert not _is_kicker_preamble("Counter target spell.")
    assert not _is_kicker_preamble(
        "Convoke (Your creatures can help cast this spell.)"
    )
    assert not _is_kicker_preamble(
        "As an additional cost to cast this spell, sacrifice an "
        "artifact or creature."
    )


def test_kicker_cost_extracts_mana_string():
    # The cost capture group returns the verbatim mana-cost string
    # (with braces preserved). The engine accepts this surface form
    # on ``CardPrinting.kicker_cost_str``.
    cost = _kicker_cost(
        "Kicker {1}{U} (You may pay an additional {1}{U} as you cast "
        "this spell.)"
    )
    assert cost == "{1}{U}"


def test_kicker_cost_returns_none_for_non_kicker():
    assert _kicker_cost("Counter target spell.") is None


def test_detect_kicker_cost_walks_past_other_preambles():
    # The detector skips past Convoke and additional-cost preambles
    # when looking for the Kicker line — keeps the dispatcher order-
    # insensitive should a future card stack preambles. Today no card
    # does, but the walk is cheap and defensive.
    text = (
        "Convoke (Your creatures can help cast this spell.)\n"
        "Kicker {2}{R} (You may pay an additional {2}{R} as you cast "
        "this spell.)\n"
        "Do something."
    )
    assert _detect_kicker_cost(text) == "{2}{R}"


def test_detect_kicker_cost_returns_none_when_absent():
    assert _detect_kicker_cost(
        "Counter target spell. You draw a card."
    ) is None


# ---------- Warping Wail ----------

def test_warping_wail_claimed_by_parser():
    # Warping Wail ``{1}{C}`` Instant — 3-mode modal now claimed via
    # the per-clause modal grammar: mode 1 (exile with p/t threshold)
    # via ``_EXILE_TARGET_CREATURE_WITH_POWER_OR_TOUGHNESS_AT_MOST``,
    # mode 2 (sorcery counter) via ``_COUNTER_TARGET_SORCERY_SPELL``,
    # mode 3 (Eldrazi Scion token with inner "It has" suffix) via the
    # extended ``_CREATE_CREATURE_TOKEN`` regex that accepts the
    # ``. It has ...`` trailing clause.
    assert "Warping Wail" in _claimed_names()


def test_warping_wail_per_clause_grammar_produces_correct_modal():
    # Belt-and-suspenders: the per-clause modal grammar produces the
    # expected ModalChooseOne with three parsed modes.
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    from mtgz_ir.cards.base import (
        CounterTargetSpell, CreateToken,
        ExileTargetCreatureWithPowerOrToughnessAtMost, ModalChooseOne,
    )
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    eff = _parse_spell_oracle_text(
        "Choose one —\n"
        "• Exile target creature with power or toughness 1 or less.\n"
        "• Counter target sorcery spell.\n"
        "• Create a 1/1 colorless Eldrazi Scion creature token. "
        "It has \"Sacrifice this token: Add {C}.\"",
        "Warping Wail",
    )
    assert eff == ModalChooseOne(modes=(
        ExileTargetCreatureWithPowerOrToughnessAtMost(max_value=1),
        CounterTargetSpell(filter_types=frozenset({"Sorcery"})),
        CreateToken(token_printing=TOKEN_PRINTINGS["Eldrazi Scion"]),
    ))
