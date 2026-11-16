"""Tests for the bespoke-spells3 migration batch.

This batch migrates seven entries out of ``_WHOLE_SPELL_EXACT_TEXT`` in
``parser.spell_builder`` by introducing nine grammar extensions:

1. **``_SENTENCE_BOUNDARY_RE`` lookahead extension** — the negative
   lookahead in ``spell_builder._split_sentences`` is widened from
   ``(?!If you do,)`` to ``(?!(?:If|When) you do,)`` so the splitter
   does NOT break the ``You may sacrifice an artifact. When you do, …``
   compound primitive at the embedded ". W" boundary (same reason
   ``If you do,`` was already guarded — Lithobraking prints
   "Then you may sacrifice an artifact. When you do, …").

2. **``Then`` prefix stripping in ``_normalize_spell_effect_phrase``**
   (``effects.py``) — a leading "Then " / "then " connective is
   stripped from each phrase before it reaches ``_parse_effect``.
   This handles split sentences produced by ``_split_sentences`` that
   begin with the sequencing connector (e.g. Scrounge for Eternity's
   "Then create a Lander token." after the sentence splitter carves
   the preceding clause off).  Also strips ``<Name> `` from the
   consequent clause ``". When/If you do, <Name> <effect>."`` so
   multi-sentence compound primitives whose inner consequence names
   the card as the grammatical subject (Lithobraking's "When you do,
   Lithobraking deals 2 damage to each creature") reach ``_parse_effect``
   in normalized form.

3. **``_YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO`` recognizer** (``effects.py``)
   — regex ``^(?:then )?you may sacrifice an artifact\\. When you do,
   (.+)$`` mapping to
   ``OptionalSacArtifactForEffect(inner_effect=parsed_inner)``.
   The inner tail is parsed recursively by ``_parse_effect``; if it
   fails the whole recognizer rejects (no silent drop).
   Migrates: **Lithobraking** (second clause of the two-clause Sequence;
   the first clause "Create a Lander token." parses via the existing
   ``_CREATE_BARE_NAME_TOKEN``).

4. **``_RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV``
   recognizer** (``effects.py``) — regex matching "return target creature
   or Spacecraft card with mana value N or less from your graveyard to
   the battlefield" → ``ReturnCreatureFromYourGraveyardToBattlefield(
   or_spacecraft=True, max_mv=N)``.  Combines the existing
   ``or_spacecraft`` and ``max_mv`` flags that previously only appeared
   independently.
   Migrates: **Scrounge for Eternity** (first clause of the two-clause
   Sequence; the second clause "Then create a Lander token." is claimed
   by extension #2 + ``_CREATE_BARE_NAME_TOKEN``). Note: the new
   per-clause parse is MORE faithful than the previous whole-spell
   simplification (which dropped the ``or Spacecraft`` widening, the
   ``max_mv=5`` cap, and the Lander rider entirely, returning only the
   bare ``ReturnCreatureFromYourGraveyardToBattlefield()``).

5. **``_RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED``
   recognizer** (``effects.py``) + new
   ``ReturnUpToNNonlandPermanentCardsFromGraveyardToHand`` engine class —
   matches "return up to N target nonland permanent cards from your
   graveyard to your hand, then return up to M target land cards from
   your graveyard to the battlefield tapped" → ``ReturnUpToNNonlandPermanent
   CardsFromGraveyardToHand(max_n=N, min_n=0)``, dropping the second
   compound piece (land cards to battlefield tapped) as an unimplemented
   rider.  The new per-clause parse is MORE faithful than the previous
   simplification which returned bare ``ReturnCreatureFromYourGraveyardToHand()``
   (dropping both the "up to two" count AND the nonland-permanent widening).
   Migrates: **Pull Through the Weft**.

6. **``_COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL`` recognizer**
   (``effects.py``) — regex matching "counter target creature or enchantment
   spell" → ``CounterTargetSpell(filter_types=frozenset({"Creature",
   "Enchantment"}))``. Sibling to the existing artifact-or-enchantment
   literal; enables Get Out's mode 1 to parse via the unified grammar.

7. **``_parse_modal_choose_one`` exactly-one-parseable fallback**
   (``spell_builder.py``) — when exactly ONE mode of a "Choose one —"
   modal spell parses (and all others are unrecognized), the parser returns
   that one mode's effect directly rather than ``ModalChooseOne``. Same
   "parser-is-right" simplification the whole-spell table previously
   hard-coded for Get Out and Untimely Malfunction. Only applies to
   "Choose one —", NOT "Choose one or both —".
   Migrates: **Get Out** (mode 1), **Untimely Malfunction** (mode 1).

Total migration: 7 entries removed from ``_WHOLE_SPELL_EXACT_TEXT``.
Table shrinks from 22 to 15.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    CounterTargetSpell,
    CreateToken,
    DealNDamageToEachCreature,
    DestroyTargetArtifact,
    GainLife,
    LookTopNPutOneInHandRestToGraveyard,
    ManifestDread,
    OptionalSacArtifactForEffect,
    ReturnCreatureFromYourGraveyardToBattlefield,
    ReturnTargetCreatureToOwnersHand,
    ReturnUpToNNonlandPermanentCardsFromGraveyardToHand,
    Sequence,
)
from mtgz_ir.cards.parser.effects import (
    _YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO,
    _COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL,
    _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE,
    _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV,
    _RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED,
    _RETURN_UP_TO_ONE_TARGET_NONLAND_PERMANENT_TO_OWNERS_HAND,
    _normalize_spell_effect_phrase,
    _parse_effect,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.spell_builder import _SENTENCE_BOUNDARY_RE, _split_sentences, _parse_spell_oracle_text
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# Table-size regression covered comprehensively by
# tests/test_parser_bespoke_spells1.py::test_bespoke_table_count.

# ------------------------------------------------------------------ #
# _SENTENCE_BOUNDARY_RE — "When you do," guard                       #
# ------------------------------------------------------------------ #

def test_sentence_boundary_does_not_split_before_when_you_do():
    # "Then you may sacrifice an artifact. When you do, …" must NOT be
    # split at the ". W" boundary — the "When you do," guard in the
    # negative lookahead prevents it.
    line = (
        "Then you may sacrifice an artifact. "
        "When you do, Lithobraking deals 2 damage to each creature."
    )
    pieces = _SENTENCE_BOUNDARY_RE.split(line)
    assert len(pieces) == 1


def test_sentence_boundary_does_not_split_before_if_you_do():
    # Legacy guard — "If you do," must still not be split (regression).
    line = "You may discard a card. If you do, draw a card."
    pieces = _SENTENCE_BOUNDARY_RE.split(line)
    assert len(pieces) == 1


def test_sentence_boundary_splits_normal_boundaries():
    # A normal two-sentence line (no If/When-you-do) still splits.
    line = "Create a Lander token. Then you gain 1 life."
    # "Then you gain …" starts with "Then" — not guarded → splits.
    pieces = _SENTENCE_BOUNDARY_RE.split(line)
    assert len(pieces) == 2


def test_split_sentences_keeps_when_you_do_compound_whole():
    # End-to-end: the multi-sentence Lithobraking body splits at
    # "Create a Lander token. / Then you may …" but NOT at
    # "Then you may … / When you do, …".
    oracle = (
        "Create a Lander token. "
        "Then you may sacrifice an artifact. "
        "When you do, Lithobraking deals 2 damage to each creature."
    )
    sentences = _split_sentences(oracle)
    assert sentences == [
        "Create a Lander token.",
        "Then you may sacrifice an artifact. "
        "When you do, Lithobraking deals 2 damage to each creature.",
    ]


# ------------------------------------------------------------------ #
# _normalize_spell_effect_phrase — "Then" prefix stripping           #
# ------------------------------------------------------------------ #

def test_normalize_strips_then_prefix_capital():
    # "Then create a Lander token." → "create a Lander token"
    normalized = _normalize_spell_effect_phrase(
        "Then create a Lander token.", "Scrounge for Eternity"
    )
    assert normalized == "create a Lander token"


def test_normalize_strips_then_prefix_lower():
    # "then create a Lander token." → "create a Lander token"
    normalized = _normalize_spell_effect_phrase(
        "then create a Lander token.", "SomeCard"
    )
    assert normalized == "create a Lander token"


def test_normalize_strips_name_from_when_you_do_consequent():
    # "Then you may sacrifice an artifact. When you do, Lithobraking
    # deals 2 damage to each creature." → inner name stripped from
    # the consequent clause.
    normalized = _normalize_spell_effect_phrase(
        "Then you may sacrifice an artifact. "
        "When you do, Lithobraking deals 2 damage to each creature.",
        "Lithobraking",
    )
    assert normalized == (
        "you may sacrifice an artifact. "
        "When you do, deals 2 damage to each creature"
    )


def test_normalize_does_not_strip_then_from_middle():
    # "Then" in the middle of a phrase (not at the start) must NOT
    # be stripped — only a leading "Then " connective is removed.
    normalized = _normalize_spell_effect_phrase(
        "Draw a card, then discard a card.",
        "SomeCard",
    )
    # Strip trailing period only; inner "then" is untouched.
    assert normalized == "draw a card, then discard a card"


# ------------------------------------------------------------------ #
# _YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO regex                            #
# ------------------------------------------------------------------ #

def test_you_may_sac_artifact_when_you_do_matches_lithobraking():
    m = _YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO.match(
        "you may sacrifice an artifact. When you do, "
        "deals 2 damage to each creature"
    )
    assert m is not None
    assert m.group(1) == "deals 2 damage to each creature"


def test_you_may_sac_artifact_when_you_do_matches_with_then_prefix():
    # Optional leading "then " accepted.
    m = _YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO.match(
        "then you may sacrifice an artifact. When you do, "
        "deals 2 damage to each creature"
    )
    assert m is not None


def test_you_may_sac_artifact_when_you_do_no_match_no_when():
    # Without "When you do," the pattern must not match.
    m = _YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO.match(
        "you may sacrifice an artifact"
    )
    assert m is None


def test_you_may_sac_artifact_when_you_do_parses_as_effect():
    # End-to-end via _parse_effect.
    eff = _parse_effect(
        "you may sacrifice an artifact. When you do, "
        "deals 2 damage to each creature"
    )
    assert eff == OptionalSacArtifactForEffect(
        inner_effect=DealNDamageToEachCreature(amount=2)
    )


def test_you_may_sac_artifact_when_you_do_rejects_unknown_inner():
    # If the inner effect doesn't parse, the whole pattern rejects
    # rather than returning a partial or None-inner result.
    eff = _parse_effect(
        "you may sacrifice an artifact. When you do, "
        "cast a spell for free"  # Not a recognized effect.
    )
    assert eff is None


# ------------------------------------------------------------------ #
# _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV
# ------------------------------------------------------------------ #

def test_return_creature_or_spacecraft_with_mv_regex_matches():
    m = _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV.match(
        "return target creature or Spacecraft card with mana value 5 or less "
        "from your graveyard to the battlefield"
    )
    assert m is not None
    assert m.group(1) == "5"


def test_return_creature_or_spacecraft_with_mv_regex_parameterized():
    # A different MV cap should also match — verifying the recognizer
    # is parameterized, not a fixed-value literal.
    m = _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV.match(
        "return target creature or Spacecraft card with mana value 3 or less "
        "from your graveyard to the battlefield"
    )
    assert m is not None
    assert m.group(1) == "3"


def test_return_creature_or_spacecraft_with_mv_parses_as_effect():
    eff = _parse_effect(
        "return target creature or Spacecraft card with mana value 5 or less "
        "from your graveyard to the battlefield"
    )
    assert eff == ReturnCreatureFromYourGraveyardToBattlefield(
        or_spacecraft=True, max_mv=5
    )


def test_return_creature_or_spacecraft_with_mv_does_not_match_no_spacecraft():
    # Without "or Spacecraft" the bare MV regex should match, not this one.
    m = _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV.match(
        "return target creature card with mana value 5 or less "
        "from your graveyard to the battlefield"
    )
    assert m is None


def test_return_creature_or_spacecraft_with_mv_does_not_match_no_mv():
    # Without the MV cap the bare ``or Spacecraft`` regex should match.
    m = _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV.match(
        "return target creature or Spacecraft card from your graveyard "
        "to the battlefield"
    )
    assert m is None


# ------------------------------------------------------------------ #
# Lithobraking migration                                              #
# ------------------------------------------------------------------ #

def test_lithobraking_claimed_by_parser():
    assert "Lithobraking" in _claimed_names()


def test_lithobraking_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Lithobraking"]
    assert p.spell_effect == Sequence(effects=(
        CreateToken(token_printing=TOKEN_PRINTINGS["Lander"], count=1),
        OptionalSacArtifactForEffect(
            inner_effect=DealNDamageToEachCreature(amount=2)
        ),
    ))


def test_lithobraking_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Create a Lander token. Then you may sacrifice an artifact. "
        "When you do, Lithobraking deals 2 damage to each creature.",
        "Lithobraking",
    )
    assert eff == Sequence(effects=(
        CreateToken(token_printing=TOKEN_PRINTINGS["Lander"], count=1),
        OptionalSacArtifactForEffect(
            inner_effect=DealNDamageToEachCreature(amount=2)
        ),
    ))


# ------------------------------------------------------------------ #
# Scrounge for Eternity migration                                     #
# ------------------------------------------------------------------ #

def test_scrounge_for_eternity_claimed_by_parser():
    assert "Scrounge for Eternity" in _claimed_names()


def test_scrounge_for_eternity_engine_class_more_faithful():
    # The per-clause parse is MORE faithful than the old simplification:
    # it preserves ``or_spacecraft=True`` and ``max_mv=5`` (which the
    # bespoke table dropped) AND includes the Lander token creation.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Scrounge for Eternity"]
    assert isinstance(p.spell_effect, Sequence)
    assert len(p.spell_effect.effects) == 2
    assert p.spell_effect.effects[0] == ReturnCreatureFromYourGraveyardToBattlefield(
        or_spacecraft=True, max_mv=5
    )
    assert p.spell_effect.effects[1] == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"], count=1,
    )


def test_scrounge_for_eternity_additional_cost_flag():
    # The additional-cost flag is wired separately via
    # ``_detect_additional_cost_flags`` and must still be set.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Scrounge for Eternity"]
    assert p.additional_cost_sacrifice_creature_or_artifact is True


def test_scrounge_for_eternity_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "As an additional cost to cast this spell, sacrifice an "
        "artifact or creature.\n"
        "Return target creature or Spacecraft card with mana value 5 "
        "or less from your graveyard to the battlefield. "
        "Then create a Lander token.",
        "Scrounge for Eternity",
    )
    assert isinstance(eff, Sequence)
    assert eff.effects[0] == ReturnCreatureFromYourGraveyardToBattlefield(
        or_spacecraft=True, max_mv=5
    )
    assert eff.effects[1] == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"], count=1,
    )


# ------------------------------------------------------------------ #
# Non-regression: previously passing spells not broken               #
# ------------------------------------------------------------------ #

def test_optional_discard_then_draw_still_parses():
    # "You may discard a card. If you do, draw a card." — must still
    # be recognized by ``_OPTIONAL_DISCARD_THEN_DRAW`` after our changes
    # (regression for the ``If you do,`` boundary guard). The shape
    # composes the generic ``IfYouDo(MayDo(DiscardSelf(1)),
    # DrawCards(N))`` directly.
    from mtgz_ir.cards.base import (
        DiscardSelf, DrawCards, IfYouDo, MayDo,
    )
    eff = _parse_effect("you may discard a card. If you do, draw a card")
    assert eff == IfYouDo(
        do_action=MayDo(DiscardSelf(amount=1)),
        then_action=DrawCards(amount=1),
    )


def test_you_may_pay_then_effect_still_parses():
    # "You may pay {1}. If you do, draw a card." — must still parse
    # via ``_YOU_MAY_PAY_THEN_EFFECT`` (regression).
    from mtgz_ir.cards.base import DrawCards, OptionalManaCostThenEffect
    eff = _parse_effect("you may pay {1}. If you do, draw a card")
    assert eff == OptionalManaCostThenEffect(
        cost_str="{1}", inner_effect=DrawCards(amount=1)
    )


def test_create_lander_token_still_parses():
    # "Create a Lander token." alone must still parse as before — the
    # ``Then`` prefix strip in ``_normalize_spell_effect_phrase`` must
    # not strip the word from the middle of effect phrases.
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    eff = _parse_spell_effect_phrase(
        "Create a Lander token.", "SomeCard"
    )
    assert eff == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"], count=1
    )


# ------------------------------------------------------------------ #
# _RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED  #
# ------------------------------------------------------------------ #

def test_return_nonland_permanent_gy_to_hand_regex_matches():
    m = _RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED.match(
        "return up to two target nonland permanent cards from your graveyard "
        "to your hand, then return up to two target land cards from your "
        "graveyard to the battlefield tapped"
    )
    assert m is not None
    assert m.group(1) == "two"  # first count
    assert m.group(2) == "two"  # second count (captured but unused)


def test_return_nonland_permanent_gy_to_hand_regex_parameterized():
    # Different counts should also match — verifying the recognizer is
    # parameterized.
    m = _RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED.match(
        "return up to three target nonland permanent cards from your graveyard "
        "to your hand, then return up to one target land cards from your "
        "graveyard to the battlefield tapped"
    )
    assert m is not None
    assert m.group(1) == "three"


def _pull_through_the_weft_effect():
    """Faithful two-group shape (feature/eoe-ir-fixes): up to two
    nonland permanent cards to hand, then up to two land cards to the
    battlefield tapped."""
    from mtgz_ir.effects.generic.card_flow import CardFilter
    from mtgz_ir.effects.generic.eoe_fidelity import (
        GraveyardReturnDestination,
        GraveyardReturnGroup,
        ReturnTargetCardsFromYourGraveyard,
    )
    return ReturnTargetCardsFromYourGraveyard(groups=(
        GraveyardReturnGroup(
            card_filter=CardFilter(
                any_types=("Permanent",), excluded_types=("Land",),
            ),
            min_n=0, max_n=2,
            destination=GraveyardReturnDestination.HAND,
        ),
        GraveyardReturnGroup(
            card_filter=CardFilter(any_types=("Land",)),
            min_n=0, max_n=2,
            destination=GraveyardReturnDestination.BATTLEFIELD,
            tapped=True,
        ),
    ))


def test_return_nonland_permanent_gy_to_hand_parses_as_effect():
    eff = _parse_effect(
        "return up to two target nonland permanent cards from your graveyard "
        "to your hand, then return up to two target land cards from your "
        "graveyard to the battlefield tapped"
    )
    assert eff == _pull_through_the_weft_effect()


def test_return_nonland_permanent_gy_to_hand_does_not_match_partial():
    # Without the "then return" second clause must not match.
    m = _RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED.match(
        "return up to two target nonland permanent cards from your graveyard "
        "to your hand"
    )
    assert m is None


# ------------------------------------------------------------------ #
# Pull Through the Weft migration                                     #
# ------------------------------------------------------------------ #

def test_pull_through_the_weft_claimed_by_parser():
    assert "Pull Through the Weft" in _claimed_names()


def test_pull_through_the_weft_engine_class_more_faithful():
    # The per-clause parse preserves the "up to two" count and the
    # "nonland permanent" scope — more faithful than the old
    # ReturnCreatureFromYourGraveyardToHand() simplification.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Pull Through the Weft"]
    assert p.spell_effect == _pull_through_the_weft_effect()


def test_pull_through_the_weft_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Return up to two target nonland permanent cards from your "
        "graveyard to your hand, then return up to two target land "
        "cards from your graveyard to the battlefield tapped.",
        "Pull Through the Weft",
    )
    assert eff == _pull_through_the_weft_effect()


# ------------------------------------------------------------------ #
# _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE          #
# ------------------------------------------------------------------ #

def test_look_top_n_graveyard_gain_life_regex_matches():
    m = _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE.match(
        "look at the top four cards of your library. "
        "Put one of them into your hand and the rest into your graveyard. "
        "You gain 3 life"
    )
    assert m is not None
    assert m.group(1) == "four"  # N
    assert m.group(2) == "3"     # M


def test_look_top_n_graveyard_gain_life_regex_parameterized():
    # A different N and M should also match.
    m = _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE.match(
        "look at the top 5 cards of your library. "
        "Put one of them into your hand and the rest into your graveyard. "
        "You gain 2 life"
    )
    assert m is not None
    assert m.group(1) == "5"
    assert m.group(2) == "2"


def test_look_top_n_graveyard_gain_life_parses_as_effect():
    eff = _parse_effect(
        "look at the top four cards of your library. "
        "Put one of them into your hand and the rest into your graveyard. "
        "You gain 3 life"
    )
    assert eff == Sequence(effects=(
        LookTopNPutOneInHandRestToGraveyard(n=4),
        GainLife(amount=3),
    ))


def test_look_top_two_split_hand_graveyard_not_claimed_by_new_regex():
    # The bare two-sentence form (Codecracker Hound) must NOT match the
    # new three-sentence recognizer — the two forms are disjoint.
    m = _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE.match(
        "look at the top two cards of your library. "
        "Put one into your hand and the other into your graveyard"
    )
    assert m is None


# ------------------------------------------------------------------ #
# Commune with Evil migration                                         #
# ------------------------------------------------------------------ #

def test_commune_with_evil_claimed_by_parser():
    assert "Commune with Evil" in _claimed_names()


def test_commune_with_evil_engine_class_more_faithful():
    # The per-clause parse produces Sequence(LookTopN..., GainLife(3))
    # — more faithful than the previous atomic CommuneWithEvilEffect().
    parser = OracleParserCardSource()
    p = parser.get_printings()["Commune with Evil"]
    assert p.spell_effect == Sequence(effects=(
        LookTopNPutOneInHandRestToGraveyard(n=4),
        GainLife(amount=3),
    ))


def test_commune_with_evil_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Look at the top four cards of your library. Put one of them "
        "into your hand and the rest into your graveyard. You gain 3 "
        "life.",
        "Commune with Evil",
    )
    assert eff == Sequence(effects=(
        LookTopNPutOneInHandRestToGraveyard(n=4),
        GainLife(amount=3),
    ))


# ------------------------------------------------------------------ #
# Sequence.target_count_range() fix + _RETURN_UP_TO_ONE_NONLAND fix  #
# ------------------------------------------------------------------ #

def test_sequence_target_count_range_optional_subeffect():
    # Regression: Sequence with an optional sub-effect (count_min=0,
    # count_max=1) must return (0, 1) from target_count_range() so the
    # cast pipeline offers an optional target.
    eff = Sequence(effects=(
        ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True, optional=True),
        ManifestDread(count=1),
    ))
    assert eff.target_count_range() == (0, 1)


def test_sequence_target_count_range_mandatory_subeffect():
    # Regression: Sequence with a mandatory sub-effect (count_min=1,
    # count_max=1) must still return (1, 1) — no change in behavior.
    from mtgz_ir.cards.base import DestroyTargetCreature
    eff = Sequence(effects=(
        DestroyTargetCreature(),
        ManifestDread(count=1),
    ))
    assert eff.target_count_range() == (1, 1)


def test_return_up_to_one_nonland_permanent_regex_matches():
    m = _RETURN_UP_TO_ONE_TARGET_NONLAND_PERMANENT_TO_OWNERS_HAND.match(
        "return up to one target nonland permanent to its owner's hand"
    )
    assert m is not None


def test_return_up_to_one_nonland_permanent_parses_as_effect():
    eff = _parse_effect(
        "return up to one target nonland permanent to its owner's hand"
    )
    assert eff == ReturnTargetCreatureToOwnersHand(
        allow_nonland_permanent=True, optional=True
    )


def test_return_up_to_one_nonland_permanent_does_not_match_creature():
    # The bare creature form must NOT match this regex — disjoint.
    m = _RETURN_UP_TO_ONE_TARGET_NONLAND_PERMANENT_TO_OWNERS_HAND.match(
        "return up to one target creature to its owner's hand"
    )
    assert m is None


# ------------------------------------------------------------------ #
# Unnerving Grasp migration                                           #
# ------------------------------------------------------------------ #

def test_unnerving_grasp_claimed_by_parser():
    assert "Unnerving Grasp" in _claimed_names()


def test_unnerving_grasp_engine_class_more_faithful():
    # The per-clause parse preserves the optional bounce — more faithful
    # than the previous ManifestDread(count=1) simplification that
    # dropped the bounce entirely.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Unnerving Grasp"]
    assert p.spell_effect == Sequence(effects=(
        ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True, optional=True),
        ManifestDread(count=1),
    ))


def test_unnerving_grasp_parses_via_per_clause_grammar():
    eff = _parse_spell_oracle_text(
        "Return up to one target nonland permanent to its owner's "
        "hand. Manifest dread.",
        "Unnerving Grasp",
    )
    assert eff == Sequence(effects=(
        ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True, optional=True),
        ManifestDread(count=1),
    ))


# ------------------------------------------------------------------ #
# _COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL regex                #
# ------------------------------------------------------------------ #

def test_counter_creature_or_enchantment_regex_matches():
    m = _COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL.match(
        "counter target creature or enchantment spell"
    )
    assert m is not None


def test_counter_creature_or_enchantment_parses_as_effect():
    eff = _parse_effect("counter target creature or enchantment spell")
    assert eff == CounterTargetSpell(
        filter_types=frozenset({"Creature", "Enchantment"})
    )


def test_counter_creature_or_enchantment_does_not_match_bare_counter():
    from mtgz_ir.cards.parser.effects import _COUNTER_TARGET_SPELL
    # The bare regex must not match the filter form (different anchoring).
    # (This test just verifies the specific regex doesn't over-match.)
    m = _COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL.match("counter target spell")
    assert m is None


# ------------------------------------------------------------------ #
# Modal exactly-one-parseable fallback                               #
# ------------------------------------------------------------------ #

def test_modal_exactly_one_parseable_returns_that_mode():
    # The exactly-one fallback returns the parseable mode's effect.
    eff = _parse_spell_oracle_text(
        "Choose one —\n"
        "• Counter target creature or enchantment spell.\n"
        "• Return one or two target creatures and/or enchantments you own to your hand.",
        "Get Out",
    )
    assert eff == CounterTargetSpell(
        filter_types=frozenset({"Creature", "Enchantment"})
    )


def test_modal_or_both_with_unrecognized_mode_returns_none():
    # "Choose one or both —" with an unrecognized mode must NOT fall
    # back — the or_both semantics would be lost.
    eff = _parse_spell_oracle_text(
        "Choose one or both —\n"
        "• Counter target creature or enchantment spell.\n"
        "• Return one or two target creatures and/or enchantments you own to your hand.",
        "Bogus Or Both",
    )
    assert eff is None


# ------------------------------------------------------------------ #
# Get Out migration                                                   #
# ------------------------------------------------------------------ #

def test_get_out_claimed_by_parser():
    assert "Get Out" in _claimed_names()


def test_get_out_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Get Out"]
    assert p.spell_effect == CounterTargetSpell(
        filter_types=frozenset({"Creature", "Enchantment"})
    )


# ------------------------------------------------------------------ #
# Untimely Malfunction migration                                      #
# ------------------------------------------------------------------ #

def test_untimely_malfunction_claimed_by_parser():
    assert "Untimely Malfunction" in _claimed_names()


def test_untimely_malfunction_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Untimely Malfunction"]
    assert p.spell_effect == DestroyTargetArtifact()
