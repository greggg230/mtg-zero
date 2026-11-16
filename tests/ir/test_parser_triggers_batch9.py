"""Tests for batch-9 trigger-condition phrase additions.

Five new condition phrases land in this batch — all on the predicate-
trigger path (the legacy string-key path is empty after phases 4-9):

* ``a creature you control dies`` →
  ``(Event.DIES, IsCreature() & Controlled(YOU))`` — Sothera, the
  Supervoid. Broader than the existing
  ``a creature you control with a +1/+1 counter on it dies``
  (Meltstrider Eulogist) phrase: no counter qualifier.
* ``you sacrifice an artifact`` →
  ``(Event.IS_SACRIFICED, IsArtifact() & Controlled(YOU) & ~IsSelf())``
  — Biotech Specialist. Narrower type filter than the existing
  ``you sacrifice another creature or artifact`` (Lightless Evangel)
  phrase, plus the ``an``-vs-``another`` surface distinction collapsed
  onto the same ``~IsSelf()``-bearing predicate per the
  ``an artifact you control enters`` precedent.
* ``a creature you control deals combat damage to a player`` →
  ``(Event.DEALS_COMBAT_DAMAGE, IsCreature(subject="source") &
  Controlled(YOU, subject="source") & IsPlayer(subject="target"))`` —
  Starwinder. Broader than the existing ``this creature deals combat
  damage to a player`` (Illvoi Infiltrator) phrase: any creature you
  control, not just the source.
* ``a nontoken creature you control with a +1/+1 counter on it dies``
  → ``(Event.DIES, IsCreature() & Controlled(YOU) & IsNontoken() &
  HasCounter("+1/+1"))`` — Rayblade Trooper. Stricter than the
  existing ``a creature you control with a +1/+1 counter on it dies``
  variant: adds ``IsNontoken()`` so token deaths don't count.
* ``another creature or artifact you control enters`` →
  ``(Event.ENTERS, (IsCreature() | IsArtifact()) & Controlled(YOU)
  & ~IsSelf())`` — Weftstalker Ardent. Compound type qualifier;
  mirrors the static registry's Weftstalker predicate exactly.

Each phrase gets a ``_parse_trigger_phrase`` /
``_parse_predicate_trigger_phrase`` lookup test plus an end-to-end
``_parse_triggered_ability_line`` test that pairs the new condition
with an existing-grammar effect. No newly-supported card claims its
oracle text end-to-end this batch — every candidate has at least one
other unparseable line (effect grammar limitations like "this creature
deals 1 damage to each opponent", or non-creature card types like
Sothera's enchantment frame), so the batch ships predicate plumbing
without a parity-vs-static card claim test.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DrawCards,
    PutPlusOneCounterOnSelf,
    PutPlusOneCounterOnTargetCreature,
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
    IsNontoken,
    IsPlayer,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    Subject,
    YOU,
)


# Reused predicate aliases — one per new phrase, mirroring the
# ``_PREDICATE_TRIGGER_PHRASES`` table entries verbatim so a parser
# refactor that drifts the predicate shape gets caught here.
_CREATURE_YOU_CONTROL_DIES_PRED = Subject(None, IsCreature()) & Controlled(YOU)
_SAC_ARTIFACT_PRED = Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf()
_YOUR_CREATURE_COMBAT_DAMAGE_PRED = (
    Subject("source", IsCreature())
    & Controlled(YOU, subject="source")
    & Subject("target", IsPlayer())
)
_NONTOKEN_COUNTER_DIES_PRED = (
    Subject(None, IsCreature() & IsNontoken() & HasCounter("+1/+1"))
    & Controlled(YOU)
)
_CREATURE_OR_ARTIFACT_ENTERS_PRED = (
    Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU) & ~IsSelf()
)


# ---------- a creature you control dies ----------

def test_parse_predicate_trigger_phrase_recognizes_creature_you_control_dies():
    # Sothera, the Supervoid's broad death trigger. Distinct from the
    # existing Meltstrider Eulogist phrase (which adds the +1/+1
    # counter qualifier). The legacy string-key lookup must NOT also
    # claim the phrase — phases 4-9 emptied the legacy table for every
    # parsed phrase.
    assert _parse_trigger_phrase("a creature you control dies") is None
    assert _parse_predicate_trigger_phrase(
        "a creature you control dies"
    ) == (Event.DIES, _CREATURE_YOU_CONTROL_DIES_PRED)


def test_creature_you_control_dies_end_to_end():
    # Sothera's actual effect (each opponent chooses a creature to
    # exile) isn't in the effect grammar, so we pair the new condition
    # with the existing ``draw a card`` effect to keep the end-to-end
    # test focused on the trigger phrase. The parsed ability shape
    # mirrors the static registry's Sothera death trigger predicate
    # exactly — predicate parity is checked here even when the effect
    # half is a synthetic stand-in.
    parsed = _parse_triggered_ability_line(
        "Whenever a creature you control dies, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.DIES, predicate=_CREATURE_YOU_CONTROL_DIES_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- you sacrifice an artifact ----------

def test_parse_predicate_trigger_phrase_recognizes_sacrifice_an_artifact():
    # Biotech Specialist's verbatim trigger condition. Different
    # from Lightless Evangel's ``you sacrifice another creature or
    # artifact``: narrower type filter (artifact only) and the
    # ``an``-vs-``another`` surface distinction. Both collapse to a
    # ``~IsSelf()``-bearing predicate per the
    # ``an artifact you control enters`` precedent — even though
    # Biotech Specialist itself is a non-artifact creature so the
    # source can never be the sacrificed artifact, mirroring the
    # static registry keeps parser/static parity exact.
    assert _parse_trigger_phrase("you sacrifice an artifact") is None
    assert _parse_predicate_trigger_phrase(
        "you sacrifice an artifact"
    ) == (Event.IS_SACRIFICED, _SAC_ARTIFACT_PRED)


def test_sacrifice_an_artifact_end_to_end():
    # Biotech Specialist's actual effect (deal 2 damage to target
    # opponent) isn't in the effect grammar today, so we pair with
    # ``draw a card`` to keep the end-to-end test focused on the
    # trigger phrase.
    parsed = _parse_triggered_ability_line(
        "Whenever you sacrifice an artifact, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.IS_SACRIFICED, predicate=_SAC_ARTIFACT_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- a creature you control deals combat damage to a player ----------

def test_parse_predicate_trigger_phrase_recognizes_your_creature_combat_damage():
    # Starwinder's verbatim trigger condition. Broader than Illvoi
    # Infiltrator's ``this creature deals combat damage to a player``:
    # any creature you control, not just the source — the source-side
    # ``IsSelf()`` of Illvoi's predicate is replaced by ``IsCreature()
    # & Controlled(YOU)``. Mirrors the static registry's Starwinder
    # predicate exactly. The aggregated "one or more creatures you
    # control" variant (Elegy Acolyte) is intentionally NOT mapped:
    # its once-per-combat fire semantics don't fit the per-event
    # predicate model.
    assert _parse_trigger_phrase(
        "a creature you control deals combat damage to a player"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "a creature you control deals combat damage to a player"
    ) == (Event.DEALS_COMBAT_DAMAGE, _YOUR_CREATURE_COMBAT_DAMAGE_PRED)


def test_your_creature_combat_damage_to_player_end_to_end():
    # Starwinder's actual effect is "you may draw that many cards"
    # (the resolving-combat-damage-amount channel) which isn't in the
    # effect grammar; pair with the existing ``draw a card`` effect
    # for the end-to-end shape check.
    parsed = _parse_triggered_ability_line(
        "Whenever a creature you control deals combat damage to a "
        "player, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.DEALS_COMBAT_DAMAGE,
        predicate=_YOUR_CREATURE_COMBAT_DAMAGE_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- a nontoken creature you control with a +1/+1 counter on it dies ----------

def test_parse_predicate_trigger_phrase_recognizes_nontoken_counter_dies():
    # Rayblade Trooper's verbatim trigger condition. Stricter than
    # Meltstrider Eulogist's ``a creature you control with a +1/+1
    # counter on it dies``: adds ``IsNontoken()`` so a token creature
    # with a +1/+1 counter dying doesn't satisfy the trigger.
    assert _parse_trigger_phrase(
        "a nontoken creature you control with a +1/+1 counter on it dies"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "a nontoken creature you control with a +1/+1 counter on it dies"
    ) == (Event.DIES, _NONTOKEN_COUNTER_DIES_PRED)


def test_nontoken_counter_dies_end_to_end():
    # Rayblade Trooper's actual effect (create a 1/1 white Human
    # Soldier creature token) needs a generic create-token effect
    # parser that doesn't exist today; pair with the existing
    # ``put a +1/+1 counter on this creature`` effect to keep the
    # end-to-end test focused on the trigger phrase.
    parsed = _parse_triggered_ability_line(
        "Whenever a nontoken creature you control with a +1/+1 "
        "counter on it dies, put a +1/+1 counter on this creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.DIES, predicate=_NONTOKEN_COUNTER_DIES_PRED,
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- another creature or artifact you control enters ----------

def test_parse_predicate_trigger_phrase_recognizes_creature_or_artifact_enters():
    # Weftstalker Ardent's verbatim trigger condition — compound type
    # qualifier (creature OR artifact) combined with ``Controlled(YOU)``
    # and ``~IsSelf()`` for the ``another`` qualifier. Mirrors the
    # static registry's Weftstalker predicate exactly.
    assert _parse_trigger_phrase(
        "another creature or artifact you control enters"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "another creature or artifact you control enters"
    ) == (Event.ENTERS, _CREATURE_OR_ARTIFACT_ENTERS_PRED)


def test_creature_or_artifact_enters_end_to_end():
    # Weftstalker Ardent's actual effect ("this creature deals 1
    # damage to each opponent") isn't in the effect grammar today —
    # the ``it``-stripping normalization in
    # ``_normalize_creature_effect_phrase`` doesn't strip
    # ``this creature``. Pair with the existing ``put a +1/+1
    # counter on target creature`` effect for the end-to-end shape
    # check.
    parsed = _parse_triggered_ability_line(
        "Whenever another creature or artifact you control enters, "
        "put a +1/+1 counter on target creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_CREATURE_OR_ARTIFACT_ENTERS_PRED,
        effect=PutPlusOneCounterOnTargetCreature(amount=1),
    )
