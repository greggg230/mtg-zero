"""Compound-effect parsing — multiple primitive effects joined by `` and ``.

Covers ``_parse_compound_effect`` (the new helper) and its integration
into ``_parse_effect``, plus end-to-end coverage through the per-context
wrappers (``_parse_effect_phrase`` for creature triggers and
``_parse_spell_effect_phrase`` for spells). Single-effect parsing
should remain unchanged for phrases with no `` and ``.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DestroyTargetCreature,
    DrawCards,
    ExileTargetCreature,
    GainLife,
    PutPlusOneCounterOnSelf,
    PutPlusOneCounterOnTargetCreature,
    Sequence,
)
from mtgz_ir.cards.parser.effects import (
    _parse_compound_effect,
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)


def test_two_effect_compound_parses_to_sequence():
    result = _parse_effect("draw a card and you gain 1 life")
    assert result == Sequence(effects=(DrawCards(amount=1), GainLife(amount=1)))


def test_three_effect_compound_parses_to_sequence_of_three():
    result = _parse_effect(
        "draw a card and you gain 1 life and draw 2 cards"
    )
    assert result == Sequence(
        effects=(
            DrawCards(amount=1),
            GainLife(amount=1),
            DrawCards(amount=2),
        )
    )


def test_compound_with_one_unparseable_piece_returns_none():
    # First piece is parseable, second is gibberish — the whole compound
    # rejects, and (because the unsplit phrase isn't a valid single
    # effect either) ``_parse_effect`` ultimately returns None.
    result = _parse_effect(
        "draw a card and frobnicate the widget thoroughly"
    )
    assert result is None


def test_compound_helper_returns_none_when_no_and():
    # The helper itself short-circuits when there's no `` and `` —
    # callers know to fall through to the single-effect grammar.
    assert _parse_compound_effect("draw a card") is None


def test_compound_helper_returns_none_when_any_piece_unparseable():
    # The helper, called directly, also returns None on partial-parse
    # failure (rather than raising or returning a partial Sequence).
    assert _parse_compound_effect(
        "draw a card and total nonsense here"
    ) is None


def test_single_effect_no_and_still_parses_normally():
    # Sanity check: phrases without `` and `` flow through to the
    # existing single-effect dispatch unchanged.
    assert _parse_effect("draw a card") == DrawCards(amount=1)
    assert _parse_effect("you gain 3 life") == GainLife(amount=3)
    assert _parse_effect("destroy target creature") == DestroyTargetCreature()


def test_compound_via_creature_trigger_phrase_wrapper():
    # End-to-end via the creature-triggered-ability wrapper. Surface form
    # has no leading "it " here because compound effects rarely subject
    # the source explicitly; the phrase normalizer is a no-op then.
    result = _parse_effect_phrase(
        "put a +1/+1 counter on this creature and draw a card"
    )
    assert result == Sequence(effects=(
        PutPlusOneCounterOnSelf(amount=1),
        DrawCards(amount=1),
    ))


def test_compound_via_spell_effect_phrase_wrapper():
    # End-to-end via the spell wrapper. ``<Name>`` prefix stripping,
    # trailing-period stripping, and first-letter lowercasing all
    # happen before compound parsing — so a printed sentence like
    # "Solace puts a +1/+1 counter on target creature and draws a card."
    # ... wait — the spell wrapper strips "Solace " but the verb "draws"
    # isn't in our grammar. We construct a printed-shape phrase that
    # uses the same "you-subject" verbs the compound grammar already
    # supports: "Draw a card and you gain 2 life." with name "Blessing".
    result = _parse_spell_effect_phrase(
        "Draw a card and you gain 2 life.", name="Blessing"
    )
    assert result == Sequence(effects=(
        DrawCards(amount=1),
        GainLife(amount=2),
    ))


def test_compound_with_targeted_and_untargeted_subeffect():
    # Sequence supports at most one targeting sub-effect; this case
    # has exactly one (PutPlusOneCounterOnTargetCreature) plus an
    # untargeted DrawCards, so it constructs cleanly.
    result = _parse_effect(
        "put a +1/+1 counter on target creature and draw a card"
    )
    assert result == Sequence(effects=(
        PutPlusOneCounterOnTargetCreature(amount=1),
        DrawCards(amount=1),
    ))


def test_compound_two_targeting_subeffects_returns_none():
    # ``Sequence`` only supports a single targeting sub-effect (its
    # cast-time target-selection pipeline is single-choice). Two
    # targeting pieces in one compound — e.g. ``destroy target
    # creature and exile target creature`` — would crash Sequence's
    # __post_init__, so the parser declines them up-front and returns
    # None instead. Multi-target spells need a different combinator.
    assert _parse_effect(
        "destroy target creature and exile target creature"
    ) is None


def test_compound_single_target_destroy_and_draw():
    # Combined targeting (destroy) + non-targeting (draw) is fine.
    result = _parse_effect("destroy target creature and draw a card")
    assert result == Sequence(effects=(
        DestroyTargetCreature(),
        DrawCards(amount=1),
    ))


def test_compound_exile_target_and_gain_life():
    result = _parse_effect("exile target creature and you gain 2 life")
    assert result == Sequence(effects=(
        ExileTargetCreature(),
        GainLife(amount=2),
    ))
