"""Tests for batch-7 trigger-condition phrase additions.

Three new condition phrases land in this batch:

* ``you cast your second spell each turn`` → engine key
  ``second_spell_each_turn`` (Illvoi Operative, Sunstar Lightsmith,
  Station Monitor, etc.).
* ``you sacrifice another creature or artifact`` → engine key
  ``sacrifice_creature_or_artifact`` (Lightless Evangel).
* ``a creature you control with a +1/+1 counter on it dies`` →
  engine key ``creature_you_control_with_counter_dies``
  (Meltstrider Eulogist).

Each gets a ``_parse_trigger_phrase`` lookup test, an end-to-end
``_parse_triggered_ability_line`` test that pairs the new condition
with an existing-grammar effect, and a shared negative test for an
unknown phrase.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DrawCards,
    PutPlusOneCounterOnSelf,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.triggers import (
    _parse_predicate_trigger_phrase,
    _parse_trigger_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.effects.object_predicates import (
    HasCounter,
    IsArtifact,
    IsCreature,
)
from mtgz_ir.effects.state_predicates import (
    SpellsCastThisTurn,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    StateBound,
    Subject,
    YOU,
)


_COUNTER_DIES_PRED = (
    Subject(None, IsCreature() & HasCounter("+1/+1")) & Controlled(YOU)
)
_SECOND_SPELL_PRED = (
    Controlled(YOU, subject="controller") & StateBound(SpellsCastThisTurn(n=2))
)
_SAC_CREATURE_OR_ARTIFACT_PRED = (
    Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU) & ~IsSelf()
)


# ---------- you cast your second spell each turn ----------

def test_parse_trigger_phrase_recognizes_second_spell_each_turn():
    # Phase 8 migrated the ``you cast your second spell each turn``
    # phrase to the predicate-style path: ``(Event.IS_CAST,
    # Controlled(YOU, subject="controller") & SpellsCastThisTurn(n=
    # 2))``. The legacy lookup must therefore return ``None``.
    assert _parse_trigger_phrase(
        "you cast your second spell each turn"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "you cast your second spell each turn"
    ) == (Event.IS_CAST, _SECOND_SPELL_PRED)


def test_second_spell_each_turn_end_to_end():
    # End-to-end Illvoi Operative oracle text — entirely one line of
    # the form ``Whenever <condition>, <effect>.``. The condition
    # resolves to predicate-style ``Event.IS_CAST`` +
    # ``Controlled(YOU, "controller") & StateBound(SpellsCastThisTurn(n=2))``;
    # the effect resolves via the existing unified effect grammar's
    # ``put a +1/+1 counter on this creature`` rule.
    parsed = _parse_triggered_ability_line(
        "Whenever you cast your second spell each turn, "
        "put a +1/+1 counter on this creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.IS_CAST, predicate=_SECOND_SPELL_PRED,
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- you sacrifice another creature or artifact ----------

def test_parse_trigger_phrase_recognizes_sacrifice_creature_or_artifact():
    # Phase 8 migrated the ``you sacrifice another creature or
    # artifact`` phrase to the predicate-style path: ``(Event.
    # IS_SACRIFICED, (IsCreature() | IsArtifact()) & Controlled(YOU)
    # & ~IsSelf())``. The legacy lookup must therefore return
    # ``None``. The ``~IsSelf()`` clause carries the ``another`` vs
    # ``a`` distinction explicitly in the predicate (rather than
    # implicitly via the source-still-on-battlefield gate the legacy
    # walk used).
    assert _parse_trigger_phrase(
        "you sacrifice another creature or artifact"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "you sacrifice another creature or artifact"
    ) == (Event.IS_SACRIFICED, _SAC_CREATURE_OR_ARTIFACT_PRED)


def test_sacrifice_creature_or_artifact_end_to_end():
    # Lightless Evangel's full oracle text — one line.
    parsed = _parse_triggered_ability_line(
        "Whenever you sacrifice another creature or artifact, "
        "put a +1/+1 counter on this creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.IS_SACRIFICED,
        predicate=_SAC_CREATURE_OR_ARTIFACT_PRED,
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- a creature you control with a +1/+1 counter on it dies ----------

def test_parse_trigger_phrase_recognizes_counter_creature_dies():
    # Meltstrider Eulogist's oracle uses ``a creature you control with
    # a +1/+1 counter on it dies``. Phase 4 migrated this phrase to the
    # predicate-style path: ``(Event.DIES, IsCreature() & Controlled(
    # YOU) & HasCounter("+1/+1"))``. The legacy lookup must therefore
    # return ``None``.
    assert _parse_trigger_phrase(
        "a creature you control with a +1/+1 counter on it dies"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "a creature you control with a +1/+1 counter on it dies"
    ) == (Event.DIES, _COUNTER_DIES_PRED)


def test_counter_creature_dies_end_to_end():
    # Meltstrider Eulogist's full oracle text — one line that composes
    # the new predicate-style condition with the existing
    # ``draw a card`` effect.
    parsed = _parse_triggered_ability_line(
        "Whenever a creature you control with a +1/+1 counter on it dies, "
        "draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.DIES, predicate=_COUNTER_DIES_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- Negative: unknown phrase returns None ----------

def test_unknown_trigger_phrase_returns_none():
    # A condition phrase the dictionary doesn't recognize must yield
    # ``None`` so the enclosing card is skipped end-to-end rather
    # than silently misclaimed with a bogus trigger key.
    assert _parse_trigger_phrase("you blink an enchantment") is None


def test_unknown_trigger_phrase_in_line_returns_none():
    # End-to-end version: a malformed/unsupported condition inside an
    # otherwise well-shaped triggered-ability line still propagates as
    # ``None`` from ``_parse_triggered_ability_line``.
    assert _parse_triggered_ability_line(
        "Whenever you blink an enchantment, draw a card."
    ) is None


# ---------- End-to-end claim parity for newly-supported cards ----------





