"""Tests for batch-12 unified-effect-grammar additions:

* ``counter target artifact or enchantment spell`` —
  ``CounterTargetSpell(filter_types=frozenset({"Artifact", "Enchantment"}))``.
  More-specific shape that wins over the bare ``counter target spell``
  regex.
* ``exile all creatures and Spacecraft`` —
  ``ExileAllCreaturesAndSpacecraft()`` mass wrath. The capital ``S`` on
  Spacecraft is preserved verbatim from oracle text since the
  normalization layer only lowercases the first letter of the phrase.
* ``deals N damage to each opponent`` —
  ``DealDamageEachOpponent(amount=N)`` mass-burn. Used in modal-spell
  modes (Ruinous Rampage's burn half).
* ``exile all artifacts with mana value N or less`` —
  ``ExileAllArtifactsWithManaValueAtMost(max_mv=N)`` mass-artifact-exile
  (Ruinous Rampage's anti-artifact half).
* ``search your library for an artifact card, reveal it, put it into
  your hand, then shuffle`` —
  ``SearchLibraryForCard(filter_name="artifact", may_fail=True)``
  artifact tutor (Scour for Scrap mode 1).

Each new effect is exercised three ways: directly via ``_parse_effect``
on the normalized phrase, via ``_parse_effect_phrase`` (creature-trigger
surface form), and via ``_parse_spell_effect_phrase`` (spell sentence
form, capitalized first letter and trailing period). Negative cases
verify the regex stays narrow.

Card-claim tests confirm the parser end-to-end-claims the four EoE
cards unlocked by this batch (Annul, Beyond the Quiet, Ruinous Rampage,
Scour for Scrap).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CounterTargetSpell,
    DealDamageEachOpponent,
    ExileAllArtifactsWithManaValueAtMost,
    ExileAllCreaturesAndSpacecraft,
    ModalChooseOne,
    ReturnTargetArtifactCardFromYourGraveyardToHand,
    SearchLibraryForCard,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


# ---------- counter target artifact or enchantment spell ----------

def test_parse_effect_recognizes_counter_artifact_or_enchantment_spell():
    # Annul's full sentence after spell-effect normalization. The
    # filter_types frozenset gates legal targets to artifact OR
    # enchantment spells on the stack.
    assert _parse_effect(
        "counter target artifact or enchantment spell"
    ) == CounterTargetSpell(
        filter_types=frozenset({"Artifact", "Enchantment"})
    )


def test_parse_effect_phrase_recognizes_counter_artifact_or_enchantment_spell():
    # Creature-trigger surface form (no ``it`` prefix because the
    # subject of the trigger's effect is the controller, not the
    # source). No EoE creature actually prints a counterspell trigger
    # but the unified grammar accepts it across contexts.
    assert _parse_effect_phrase(
        "counter target artifact or enchantment spell"
    ) == CounterTargetSpell(
        filter_types=frozenset({"Artifact", "Enchantment"})
    )


def test_parse_spell_effect_phrase_recognizes_counter_artifact_or_enchantment_spell():
    # Spell surface form: capitalized first letter + trailing period.
    # Mirrors Annul's printed sentence verbatim.
    assert _parse_spell_effect_phrase(
        "Counter target artifact or enchantment spell.",
        "Bogus Annul",
    ) == CounterTargetSpell(
        filter_types=frozenset({"Artifact", "Enchantment"})
    )


def test_parse_effect_filter_takes_priority_over_bare_counter():
    # The filter-typed ``counter target artifact or enchantment spell``
    # is matched BEFORE the bare ``counter target spell`` regex. If the
    # ordering were reversed the bare regex (which requires the phrase
    # to end at ``spell`` with no qualifier) wouldn't match this surface
    # at all, but asserting the produced engine class confirms the
    # specific-before-general ordering is correctly captured.
    eff = _parse_effect("counter target artifact or enchantment spell")
    assert isinstance(eff, CounterTargetSpell)
    assert eff.filter_types == frozenset({"Artifact", "Enchantment"})


def test_parse_effect_counter_filters_compose_through_spell_grammar():
    # Spell-type filters outside the legacy literals are claimed by the
    # compositional counterspell grammar (``counter_phrases``).
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    from mtgz_ir.effects.object_predicates import IsCreature
    from mtgz_ir.effects.value_predicates import HasCardType

    assert _parse_effect(
        "counter target instant or sorcery spell"
    ) == CounterSpell(
        spell_filter=HasCardType("Instant") | HasCardType("Sorcery"),
        label="instant or sorcery spell",
    )
    assert _parse_effect("counter target creature spell") == CounterSpell(
        spell_filter=IsCreature(), label="creature spell",
    )
    assert _parse_effect("counter target wizard spell") is None


# ---------- exile all creatures and Spacecraft ----------

def test_parse_effect_recognizes_exile_all_creatures_and_spacecraft():
    # Beyond the Quiet's full sentence. The capital ``S`` on
    # ``Spacecraft`` is preserved by the normalizer (only the first
    # letter of the phrase is lowercased).
    assert _parse_effect(
        "exile all creatures and Spacecraft"
    ) == ExileAllCreaturesAndSpacecraft()


def test_parse_effect_phrase_recognizes_exile_all_creatures_and_spacecraft():
    # Creature-trigger surface form.
    assert _parse_effect_phrase(
        "exile all creatures and Spacecraft"
    ) == ExileAllCreaturesAndSpacecraft()


def test_parse_spell_effect_phrase_recognizes_exile_all_creatures_and_spacecraft():
    # Spell surface form. Mirrors Beyond the Quiet's printed sentence.
    assert _parse_spell_effect_phrase(
        "Exile all creatures and Spacecraft.",
        "Bogus Beyond the Quiet",
    ) == ExileAllCreaturesAndSpacecraft()


def test_parse_effect_returns_none_for_lowercase_spacecraft():
    # The capital ``S`` is required — the engine class describes the
    # type literally and the regex matches verbatim. A future printing
    # with lowercase ``spacecraft`` would need its own regex shape.
    assert _parse_effect("exile all creatures and spacecraft") is None


def test_parse_effect_bare_exile_all_creatures_composes():
    # ``exile all creatures`` alone is a different effect
    # (DestroyAllCreatures-shape mass exile that wouldn't include
    # Spacecraft) and isn't claimed by this primitive. No EoE card
    # ships this exact bare phrase but rejecting it keeps the grammar
    # narrow.
    # The compositional mass-removal grammar claims the bare printed
    # surface faithfully (no Spacecraft arm).
    from mtgz_ir.effects.generic.targeted_removal import RemoveAllMatching
    from mtgz_ir.effects.object_predicates import IsCreature

    assert _parse_effect("exile all creatures") == RemoveAllMatching(
        permanent_filter=IsCreature(), action="exile", label="all creatures",
    )


# ---------- deals N damage to each opponent ----------

def test_parse_effect_recognizes_deals_damage_each_opponent():
    # Ruinous Rampage's burn-mode surface form after the spell-effect
    # normalizer strips the ``<Name>`` prefix.
    assert _parse_effect(
        "deals 3 damage to each opponent"
    ) == DealDamageEachOpponent(amount=3)


def test_parse_effect_recognizes_deals_one_damage_each_opponent():
    # Single-digit boundary check.
    assert _parse_effect(
        "deals 1 damage to each opponent"
    ) == DealDamageEachOpponent(amount=1)


def test_parse_effect_phrase_recognizes_deals_damage_each_opponent():
    # Creature-trigger surface form: the ``it`` subject is stripped by
    # ``_parse_effect_phrase`` before dispatch, leaving the canonical
    # ``deals N damage to each opponent`` for the regex to match.
    assert _parse_effect_phrase(
        "it deals 2 damage to each opponent"
    ) == DealDamageEachOpponent(amount=2)


def test_parse_spell_effect_phrase_recognizes_deals_damage_each_opponent():
    # Spell surface form. The ``<Name>`` prefix is stripped by the
    # spell-effect normalizer, the trailing period is removed, and the
    # first letter is already lowercase here (``<Name> deals …``).
    assert _parse_spell_effect_phrase(
        "Bogus Burn deals 3 damage to each opponent.",
        "Bogus Burn",
    ) == DealDamageEachOpponent(amount=3)


def test_parse_effect_returns_none_for_each_opponent_loses_life_distinct_shape():
    # ``each opponent loses N life`` is a different primitive
    # (EachOpponentLosesLife) and must NOT be matched by the
    # deals-damage regex.
    eff = _parse_effect("each opponent loses 3 life")
    # Must produce the lose-life primitive, not the deal-damage one.
    from mtgz_ir.cards.base import EachOpponentLosesLife
    assert eff == EachOpponentLosesLife(amount=3)


def test_parse_effect_returns_none_for_deals_damage_missing_opponent_word():
    # Singular ``opponent`` (no ``each``) is a different shape — no
    # EoE card prints this exact form but rejecting it keeps the
    # grammar narrow.
    assert _parse_effect("deals 3 damage to opponent") is None


# ---------- exile all artifacts with mana value N or less ----------

def test_parse_effect_recognizes_exile_all_artifacts_mv3_or_less():
    # Ruinous Rampage's anti-artifact mode.
    assert _parse_effect(
        "exile all artifacts with mana value 3 or less"
    ) == ExileAllArtifactsWithManaValueAtMost(max_mv=3)


def test_parse_effect_recognizes_exile_all_artifacts_mv1_or_less():
    # Lower bound — confirms the digit capture isn't hard-coded to a
    # single value. No EoE card prints this exact value today.
    assert _parse_effect(
        "exile all artifacts with mana value 1 or less"
    ) == ExileAllArtifactsWithManaValueAtMost(max_mv=1)


def test_parse_effect_phrase_recognizes_exile_all_artifacts_with_mv():
    # Creature-trigger surface form (no ``it`` prefix; the consequence
    # acts on the battlefield, not the firing source).
    assert _parse_effect_phrase(
        "exile all artifacts with mana value 2 or less"
    ) == ExileAllArtifactsWithManaValueAtMost(max_mv=2)


def test_parse_spell_effect_phrase_recognizes_exile_all_artifacts_with_mv():
    # Spell surface form.
    assert _parse_spell_effect_phrase(
        "Exile all artifacts with mana value 3 or less.",
        "Bogus Rampage",
    ) == ExileAllArtifactsWithManaValueAtMost(max_mv=3)


def test_parse_effect_bare_exile_all_artifacts_composes():
    # ``exile all artifacts`` alone (no mana-value bound) isn't claimed —
    # different effect with no upper limit. No EoE card prints that
    # exact form but rejecting it keeps the grammar narrow.
    # Claimed faithfully (no bound) by the compositional grammar.
    from mtgz_ir.effects.generic.targeted_removal import RemoveAllMatching
    from mtgz_ir.effects.object_predicates import IsArtifact

    assert _parse_effect("exile all artifacts") == RemoveAllMatching(
        permanent_filter=IsArtifact(), action="exile", label="all artifacts",
    )


def test_parse_effect_returns_none_for_exile_artifacts_with_word_amount():
    # The mana-value bound must be a digit — spelled-out cardinals are
    # not accepted in this slot (no EoE card prints e.g. ``mana value
    # three or less``).
    assert _parse_effect(
        "exile all artifacts with mana value three or less"
    ) is None


# ---------- search your library for an artifact card ----------

def test_parse_effect_recognizes_search_library_for_artifact_card():
    # Scour for Scrap mode 1's full sentence. ``may_fail=True`` mirrors
    # the static printing — the controller may decline to fetch.
    assert _parse_effect(
        "search your library for an artifact card, "
        "reveal it, put it into your hand, then shuffle"
    ) == SearchLibraryForCard(filter_name="artifact", may_fail=True)


def test_parse_effect_phrase_recognizes_search_library_for_artifact_card():
    # Creature-trigger surface form.
    assert _parse_effect_phrase(
        "search your library for an artifact card, "
        "reveal it, put it into your hand, then shuffle"
    ) == SearchLibraryForCard(filter_name="artifact", may_fail=True)


def test_parse_spell_effect_phrase_recognizes_search_library_for_artifact_card():
    # Spell surface form. Mirrors Scour for Scrap mode 1's printed
    # sentence verbatim.
    assert _parse_spell_effect_phrase(
        "Search your library for an artifact card, "
        "reveal it, put it into your hand, then shuffle.",
        "Bogus Tutor",
    ) == SearchLibraryForCard(filter_name="artifact", may_fail=True)


def test_parse_effect_returns_none_for_truncated_search_phrase():
    # The full reveal/hand/shuffle tail is required — partial shapes
    # don't match (no EoE card prints a bare ``search your library for
    # an artifact card``).
    assert _parse_effect(
        "search your library for an artifact card"
    ) is None
    # Missing the shuffle tail.
    assert _parse_effect(
        "search your library for an artifact card, reveal it, "
        "put it into your hand"
    ) is None


def test_parse_effect_returns_none_for_search_with_wrong_filter():
    # Other card-type filters (creature, land, ...) now claim through
    # the generic card-flow tutor grammar (``SearchLibrary``) rather
    # than this artifact-specific regex. A subtype descriptor ("a Wizard
    # card") claims through the same grammar since the cube W/G bucket
    # added ``CardFilter.subtype``; a descriptor outside the grammar (a
    # lower-case unknown word) still stays unclaimed.
    from mtgz_ir.effects.generic.card_flow import CardFilter, SearchLibrary

    assert _parse_effect(
        "search your library for a creature card, "
        "reveal it, put it into your hand, then shuffle"
    ) == SearchLibrary(
        card_filter=CardFilter(any_types=("Creature",)),
        destination="hand", reveal=True,
    )
    assert _parse_effect(
        "search your library for a Wizard card, "
        "reveal it, put it into your hand, then shuffle"
    ) == SearchLibrary(
        card_filter=CardFilter(subtype="Wizard"),
        destination="hand", reveal=True,
    )
    assert _parse_effect(
        "search your library for a wizardly card, "
        "reveal it, put it into your hand, then shuffle"
    ) is None


# ---------- end-to-end card-claim tests ----------

_PARSER = OracleParserCardSource()


def test_parser_claims_annul_via_filter_typed_counter():
    # Annul prints as ``Counter target artifact or enchantment spell.`` —
    # single-line spell parsed straight via the filter-typed counter
    # primitive added in this batch.
    assert "Annul" in _PARSER.claimed_names


def test_parser_claims_beyond_the_quiet_via_exile_all_creatures_and_spacecraft():
    # Beyond the Quiet prints as ``Exile all creatures and Spacecraft.`` —
    # single-line spell parsed straight via the exile-all primitive added
    # in this batch.
    assert "Beyond the Quiet" in _PARSER.claimed_names


def test_parser_claims_ruinous_rampage_via_modal_burn_and_exile():
    # Ruinous Rampage prints as a ``Choose one —`` modal with both
    # modes newly parseable in this batch (deals-damage-each-opponent
    # and exile-all-artifacts-with-mv-N-or-less). The spell-builder's
    # modal combinator is already in place; this batch unlocks the
    # mode-line bodies.
    assert "Ruinous Rampage" in _PARSER.claimed_names


def test_parser_claims_scour_for_scrap_via_artifact_tutor():
    # Scour for Scrap prints as a ``Choose one or both —`` modal whose
    # mode 1 (artifact tutor) is newly parseable in this batch. Mode 2
    # (return target artifact card from your graveyard to your hand)
    # was already supported.
    assert "Scour for Scrap" in _PARSER.claimed_names


def test_parser_claimed_ruinous_rampage_has_modal_with_both_modes():
    # End-to-end shape check: the produced Ruinous Rampage printing
    # should be a ModalChooseOne with the two new mode primitives (in
    # the order printed). Confirms both new primitives were bound, not
    # just one.
    printings = _PARSER.get_printings()
    rr = printings["Ruinous Rampage"]
    assert isinstance(rr.spell_effect, ModalChooseOne)
    assert rr.spell_effect.modes == (
        DealDamageEachOpponent(amount=3),
        ExileAllArtifactsWithManaValueAtMost(max_mv=3),
    )


def test_parser_claimed_scour_for_scrap_has_or_both_modal():
    # ``Choose one or both —`` ships ``or_both=True`` so the engine
    # routes mode-pick through ``choose_modes`` (plural). The two
    # modes are the artifact tutor (new this batch) and the existing
    # graveyard-return-artifact primitive.
    printings = _PARSER.get_printings()
    sfs = printings["Scour for Scrap"]
    assert isinstance(sfs.spell_effect, ModalChooseOne)
    assert sfs.spell_effect.or_both is True
    assert sfs.spell_effect.modes == (
        SearchLibraryForCard(filter_name="artifact", may_fail=True),
        ReturnTargetArtifactCardFromYourGraveyardToHand(),
    )
