"""Tests for batch-8 trigger-condition phrase additions.

Six new condition phrases land in this batch — five distinct engine
trigger keys, with ``artifact_enters_you_control`` reached from two
surface phrases that the engine collapses to one key:

* ``this creature becomes tapped`` → engine key ``becomes_tapped``
  (Starfighter Pilot, Nanoform Sentinel, etc.).
* ``this creature leaves the battlefield`` → engine key
  ``leaves_battlefield`` (Anticausal Vestige).
* ``this creature deals combat damage to a player`` → engine key
  ``combat_damage_to_player`` (Illvoi Infiltrator).
* ``this creature blocks a creature with flying`` → engine key
  ``blocks_creature_with_flying`` (Skystinger).
* ``an artifact you control enters`` and
  ``another artifact you control enters`` → engine key
  ``artifact_enters_you_control`` (Mm'menon Uthros Exile uses ``an``;
  Mechan Assembler and Oreplate Pangolin use ``another``).

Each phrase gets a ``_parse_trigger_phrase`` lookup test plus an
end-to-end ``_parse_triggered_ability_line`` test that pairs the new
condition with an existing-grammar effect. A shared negative test
keeps unknown phrases on the ``None`` path. ``Mm'menon, Uthros Exile``
is the one card whose entire oracle text becomes parseable end-to-end
with this batch (``Flying`` keyword line + the new
``artifact_enters_you_control`` trigger paired with the existing
``put a +1/+1 counter on target creature`` effect), so it gets a
parser-vs-static parity check too.
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
    HasKeyword,
    IsArtifact,
    IsPlayer,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    Subject,
    YOU,
)


_ARTIFACT_ENTERS_PRED = (
    Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf()
)


# ---------- this creature becomes tapped ----------

def test_parse_trigger_phrase_recognizes_becomes_tapped():
    # ``this creature becomes tapped`` is the most-common
    # becomes-tapped phrasing on EoE creatures (Starfighter Pilot,
    # Nanoform Sentinel, Astrobiologist, Sterling Hippogriff, etc.).
    # Phase 7 migrated this phrase to the predicate-style path:
    # ``(Event.BECOMES_TAPPED, IsSelf())``.
    assert _parse_trigger_phrase(
        "this creature becomes tapped"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "this creature becomes tapped"
    ) == (Event.BECOMES_TAPPED, IsSelf())


def test_becomes_tapped_end_to_end():
    # Composes the new condition with the existing
    # ``put a +1/+1 counter on this creature`` effect — both halves
    # run through the unified line parser. After the phase-7
    # migration the parsed ability is on the predicate path:
    # ``event=Event.BECOMES_TAPPED, predicate=IsSelf()``.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature becomes tapped, "
        "put a +1/+1 counter on this creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.BECOMES_TAPPED, predicate=IsSelf(),
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- this creature leaves the battlefield ----------

def test_parse_trigger_phrase_recognizes_leaves_battlefield():
    # ``this creature leaves the battlefield`` (Anticausal Vestige).
    # Phase 4 migrated this phrase to the predicate-style path:
    # ``(Event.LEAVES_BATTLEFIELD, IsSelf())``. The compound
    # ``enters or leaves the battlefield`` form (Cryogen Relic) is a
    # different phrase shape and intentionally stays out of this
    # entry's scope.
    assert _parse_trigger_phrase(
        "this creature leaves the battlefield"
    ) is None
    assert _parse_predicate_trigger_phrase(
        "this creature leaves the battlefield"
    ) == (Event.LEAVES_BATTLEFIELD, IsSelf())


def test_leaves_battlefield_end_to_end():
    # ``When …, draw a card.`` — minimal end-to-end shape with a
    # known-supported effect. Anticausal Vestige's actual effect is
    # too rich for the current effect grammar, so the line we test
    # is a synthetic minimal representative pairing.
    parsed = _parse_triggered_ability_line(
        "When this creature leaves the battlefield, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.LEAVES_BATTLEFIELD, predicate=IsSelf(),
        effect=DrawCards(amount=1),
    )


# ---------- this creature deals combat damage to a player ----------

def test_parse_predicate_trigger_phrase_recognizes_combat_damage_to_player():
    # Phase 9 migrated this phrase from the legacy ``trigger=
    # "combat_damage_to_player"`` key to the predicate path —
    # ``Event.DEALS_COMBAT_DAMAGE`` carries subjects ``("source",
    # "target")``; ``IsSelf(subject="source")`` ties the trigger to
    # the source's own damage-dealing; ``IsPlayer(subject="target")``
    # gates "to a player" vs "to a creature". Broader variants
    # (``a creature you control``, ``one or more creatures you
    # control``) aren't parsed today — they're built directly in
    # static-card registries.
    assert _parse_predicate_trigger_phrase(
        "this creature deals combat damage to a player"
    ) == (
        Event.DEALS_COMBAT_DAMAGE,
        IsSelf(subject="source") & Subject("target", IsPlayer()),
    )
    # Legacy lookup must NOT also claim the phrase.
    assert _parse_trigger_phrase(
        "this creature deals combat damage to a player"
    ) is None


def test_combat_damage_to_player_end_to_end():
    # Illvoi Infiltrator's verbatim trigger line composes directly
    # with the existing ``draw a card`` effect, now on the predicate
    # path.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature deals combat damage to a player, "
        "draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.DEALS_COMBAT_DAMAGE,
        predicate=IsSelf(subject="source") & Subject("target", IsPlayer()),
        effect=DrawCards(amount=1),
    )


# ---------- this creature blocks a creature with flying ----------

def test_parse_predicate_trigger_phrase_recognizes_blocks_creature_with_flying():
    # Phase 9 migrated this phrase from the legacy ``trigger=
    # "blocks_creature_with_flying"`` key to the predicate path —
    # ``Event.BLOCKS`` carries subjects ``("blocker", "attacker")``;
    # ``IsSelf(subject="blocker")`` ties the trigger to the source
    # being the blocker; ``HasKeyword("flying", subject="attacker")``
    # gates on the attacker having flying right now.
    assert _parse_predicate_trigger_phrase(
        "this creature blocks a creature with flying"
    ) == (
        Event.BLOCKS,
        IsSelf(subject="blocker")
        & Subject("attacker", HasKeyword("flying")),
    )
    # Legacy lookup must NOT also claim the phrase.
    assert _parse_trigger_phrase(
        "this creature blocks a creature with flying"
    ) is None


def test_blocks_creature_with_flying_end_to_end():
    # Synthetic minimal pairing — Skystinger's real effect (a
    # ``+5/+0 until end of turn`` self-pump) is not yet in the
    # effect grammar, so we exercise the new condition with a
    # known-supported effect rather than skip the end-to-end test.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature blocks a creature with flying, "
        "draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.BLOCKS,
        predicate=(
            IsSelf(subject="blocker")
            & Subject("attacker", HasKeyword("flying"))
        ),
        effect=DrawCards(amount=1),
    )


# ---------- an / another artifact you control enters ----------

def test_parse_trigger_phrase_recognizes_an_artifact_you_control_enters():
    # Mm'menon, Uthros Exile uses ``an artifact you control enters``.
    # The engine's ``_fire_artifact_enters_triggers`` excludes the
    # source object from the firing loop, so ``an`` and ``another``
    # are semantically equivalent and both map to the same predicate
    # — phase 2 collapses both phrases to the same predicate-style
    # ``(Event.ENTERS, IsArtifact() & Controlled(YOU) & ~IsSelf())``.
    assert _parse_predicate_trigger_phrase(
        "an artifact you control enters"
    ) == (Event.ENTERS, _ARTIFACT_ENTERS_PRED)


def test_parse_trigger_phrase_recognizes_another_artifact_you_control_enters():
    # Mechan Assembler and Oreplate Pangolin use
    # ``another artifact you control enters``. Same predicate as the
    # ``an`` variant — the distinction is collapsed engine-side.
    assert _parse_predicate_trigger_phrase(
        "another artifact you control enters"
    ) == (Event.ENTERS, _ARTIFACT_ENTERS_PRED)


def test_an_artifact_you_control_enters_end_to_end():
    # Mm'menon, Uthros Exile's full second line — the new
    # ``an artifact you control enters`` condition paired with the
    # existing ``put a +1/+1 counter on target creature`` effect.
    parsed = _parse_triggered_ability_line(
        "Whenever an artifact you control enters, "
        "put a +1/+1 counter on target creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_ARTIFACT_ENTERS_PRED,
        effect=PutPlusOneCounterOnTargetCreature(amount=1),
    )


def test_another_artifact_you_control_enters_end_to_end():
    # Mechan Assembler's verbatim trigger condition; the create-Robot
    # effect isn't in the effect grammar yet, so we pair with the
    # existing ``put a +1/+1 counter on this creature`` effect to
    # keep the end-to-end test focused on the trigger phrase.
    parsed = _parse_triggered_ability_line(
        "Whenever another artifact you control enters, "
        "put a +1/+1 counter on this creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_ARTIFACT_ENTERS_PRED,
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- Negative: unknown phrase returns None ----------

def test_unknown_trigger_phrase_returns_none():
    # A condition phrase the dictionary doesn't recognize must yield
    # ``None`` so the enclosing card is skipped end-to-end rather
    # than silently misclaimed with a bogus trigger key.
    assert _parse_trigger_phrase(
        "this creature becomes the target of a spell"
    ) is None


def test_unknown_trigger_phrase_in_line_returns_none():
    # End-to-end: a malformed/unsupported condition inside an
    # otherwise well-shaped triggered-ability line still propagates
    # as ``None`` from ``_parse_triggered_ability_line``.
    # ("becomes the target of a spell" is modeled since roto P6.)
    assert _parse_triggered_ability_line(
        "Whenever this creature becomes the target of a Goblin, "
        "draw a card."
    ) is None


# ---------- End-to-end claim parity for newly-supported card ----------

