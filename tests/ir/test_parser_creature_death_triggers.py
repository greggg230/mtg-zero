"""Tests for the creature-death-trigger cluster.

This batch ships two related additions:

* **Compound trigger condition** ``this creature enters or dies`` →
  a 2-tuple of ``TriggeredAbility`` (one per arm), one bound to
  ``Event.ENTERS`` and one to ``Event.DIES``. Both arms share the
  same predicate (``IsSelf()``) and the same parsed effect; the
  parser fans the single oracle line into per-arm abilities so the
  engine's per-event dispatch sees them as independent observers,
  matching the static-registry's two-entry layout for Thawbringer.
* **Effect primitive** ``put its counters on target creature you
  control`` → ``MoveSelfCountersToTargetCreatureYouControl()``.
  Surfaces on Dockworker Drone's dies trigger; the engine class
  reads ``state.resolving_source.plus_one_counters`` at resolve time
  and transfers them to the chosen target creature.

End-to-end card claim: Thawbringer's full oracle line (``When this
creature enters or dies, surveil 1.``) parses cleanly via the
compound-trigger arm, so the parser now claims the printing. The
static registry already modeled this card via two ``TriggeredAbility``
entries, so the per-card parity test in ``test_parser_parity.py``
also covers Thawbringer once the static is updated to carry its
printed subtypes.

Dockworker Drone stays unclaimed by this batch — its first oracle
line (``This creature enters with a +1/+1 counter on it.``) is a
static replacement effect whose grammar isn't in scope here. The
``put its counters on target creature you control`` effect ships
without an end-to-end card claim today; it unblocks Dockworker
Drone the day the enters-with-counter line gains a parser.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    MoveSelfCountersToTargetCreatureYouControl,
    Surveil,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser.triggers import (
    _COMPOUND_PREDICATE_TRIGGER_PHRASES,
    _parse_compound_predicate_trigger_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# ---------- compound trigger phrasebook lookup ----------


def test_compound_predicate_trigger_phrasebook_includes_enters_or_dies():
    # The compound table must list the Thawbringer phrase verbatim. A
    # future migration that drifts the table key (e.g. accidentally
    # collapsing the ``this creature `` prefix) gets caught here.
    assert "this creature enters or dies" in (
        _COMPOUND_PREDICATE_TRIGGER_PHRASES
    )


def test_parse_compound_predicate_trigger_phrase_returns_two_arms():
    # Thawbringer's compound phrase fans out into two ``(Event,
    # Predicate)`` pairs — one for entry, one for death — both bound
    # to ``IsSelf()``. The arm order matches the surface-text order
    # (enters first, then dies) so the produced ``TriggeredAbility``
    # tuple's iteration order mirrors the static registry's two-entry
    # layout.
    arms = _parse_compound_predicate_trigger_phrase(
        "this creature enters or dies"
    )
    assert arms == (
        (Event.ENTERS, IsSelf()),
        (Event.DIES, IsSelf()),
    )


def test_parse_compound_predicate_trigger_phrase_strips_whitespace():
    # The lookup helper strips surrounding whitespace before the table
    # lookup — a leading/trailing space (caused by a future regex
    # surface-form change) shouldn't make the phrase unrecognizable.
    assert _parse_compound_predicate_trigger_phrase(
        "  this creature enters or dies  "
    ) is not None


def test_parse_compound_predicate_trigger_phrase_rejects_unknown():
    # An unrelated condition phrase doesn't accidentally get claimed
    # by the compound table. The single-condition predicate path is
    # the right home for ``this creature enters`` alone — the
    # compound table only carries the joined-with-``or`` shape.
    assert _parse_compound_predicate_trigger_phrase(
        "this creature enters"
    ) is None


# ---------- end-to-end compound trigger line ----------


def test_parse_triggered_ability_line_thawbringer_enters_or_dies():
    # Thawbringer's full oracle line. The parser produces a 2-tuple of
    # ``TriggeredAbility``, one per condition arm. Both arms share the
    # same effect (Surveil 1) and the same ``IsSelf()`` predicate; only
    # the event differs. Predicate / effect parity against the static
    # registry's Thawbringer printing is enforced by
    # ``test_parser_parity.py`` once the static carries the printed
    # subtypes.
    parsed = _parse_triggered_ability_line(
        "When this creature enters or dies, surveil 1."
    )
    assert parsed == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=Surveil(amount=1),
        ),
        TriggeredAbility(
            event=Event.DIES, predicate=IsSelf(),
            effect=Surveil(amount=1),
        ),
    )


def test_parse_triggered_ability_line_compound_condition_unknown_effect():
    # The compound condition is recognized but the effect phrase
    # (``do something weird``) isn't in the effect grammar. The whole
    # line rejects (returns ``None``) rather than emitting a
    # half-built tuple — claiming a partial trigger would silently
    # drop the actual effect.
    assert _parse_triggered_ability_line(
        "When this creature enters or dies, do something weird."
    ) is None


def test_parse_triggered_ability_line_unknown_compound_phrase():
    # An unrecognized compound phrase falls through to the single-
    # condition lookups. ``enters or leaves the battlefield`` isn't in
    # the compound table, and the bare ``"this creature enters or leaves
    # the battlefield"`` literal isn't in the single-condition tables
    # either, so the whole line rejects (no silent drop of one arm).
    # Earlier this test used ``"enters or attacks"`` as the canary
    # unknown-compound — the parser-creature-attack-triggers batch
    # added that exact phrase to the compound table, so the canary
    # moved here to a still-unclaimed shape.
    assert _parse_triggered_ability_line(
        "When this creature enters or leaves the battlefield, "
        "draw a card."
    ) is None


# ---------- put its counters on target creature you control ----------


def test_parse_effect_recognizes_move_self_counters_to_target():
    # Dockworker Drone's dies-trigger body. ``its`` refers to the
    # source (dying creature); the engine class reads
    # ``state.resolving_source.plus_one_counters`` at resolve time
    # and transfers them to the chosen target creature. Surface form
    # is post-normalization (the ``it `` prefix-strip in the creature-
    # effect normalizer doesn't fire because the phrase starts with
    # the verb ``put``, not a pronoun subject — ``its`` survives
    # intact).
    assert _parse_effect(
        "put its counters on target creature you control"
    ) == MoveSelfCountersToTargetCreatureYouControl()


def test_parse_effect_phrase_creature_trigger_surface_form():
    # Creature triggered-ability surface form is the normalized
    # phrase verbatim — no ``it`` prefix because the verb-led
    # ``put its counters …`` doesn't start with a pronoun subject.
    # The triggered-ability normalizer's ``it `` prefix-strip is a
    # no-op here, so the phrase reaches ``_parse_effect`` as-is.
    assert _parse_effect_phrase(
        "put its counters on target creature you control"
    ) == MoveSelfCountersToTargetCreatureYouControl()


def test_parse_spell_effect_phrase_sentence_form():
    # Spell-form surface text is a full sentence (capitalized first
    # letter, trailing period). The spell-effect normalizer
    # lowercases the leading ``P`` to ``p`` and strips the period
    # before dispatch, leaving the same canonical phrase the
    # creature-trigger path sees. Forward-looking — no EoE spell
    # prints this exact phrase today, but the unified grammar makes
    # it available for free.
    assert _parse_spell_effect_phrase(
        "Put its counters on target creature you control.",
        name="<Name>",
    ) == MoveSelfCountersToTargetCreatureYouControl()


def test_parse_effect_rejects_non_self_pronoun():
    # ``put its counters …`` is the source-self transfer; ``put a
    # +1/+1 counter on target creature you control`` is a different
    # primitive (counter-CREATION on a target creature, not transfer
    # from the source). The two regexes must stay disjoint — a future
    # widening of the move-self-counters regex that swallows the
    # creation phrase would silently mis-wire counter spells. We
    # don't have to assert the alternative here (other tests cover
    # the ``put a +1/+1 counter`` phrase) — just that our regex
    # doesn't claim a phrase it shouldn't.
    assert _parse_effect(
        "put a +1/+1 counter on target creature"
    ) != MoveSelfCountersToTargetCreatureYouControl()


# ---------- end-to-end card claim ----------


def test_parser_claims_thawbringer_end_to_end():
    # Thawbringer's full oracle text parses through the new compound-
    # trigger arm. Per-card parity against the static registry is
    # enforced by ``test_parser_parity.py``; this test confirms the
    # card is in the parser's claimed set, which is the gating
    # observable for the end-to-end claim.
    parser = OracleParserCardSource()
    assert "Thawbringer" in parser.claimed_names
