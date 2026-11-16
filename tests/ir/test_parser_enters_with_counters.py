"""Tests for the ETB-replacement-style ``This creature enters with …``
line shape.

Scryfall prints these as static replacement effects (CR 614), but the
engine doesn't model replacements today; the static-card convention
is to express them as a ``TriggeredAbility(event=ENTERS,
predicate=IsSelf(), effect=...)`` pair. The parser mirrors that
convention here — the line shape is recognized as a triggered-ability
source even though its surface text doesn't carry a
``When``/``Whenever`` keyword, and the produced ``TriggeredAbility``
matches the static entry shape verbatim.

Two body variants ship in this batch:

* ``a +1/+1 counter on it`` — Dockworker Drone's first oracle line.
  Maps to ``PutPlusOneCounterOnSelf(amount=1)``. The plural-N
  ``two/three/N +1/+1 counters on it`` shape is forward-looking
  grammar (no EoE creature prints N>1 today) but the regex covers it
  to keep the body-parser uniform.
* ``a +1/+1 counter on it for each other creature and/or artifact you
  control`` — Luxknight Breacher's printed body. Maps to
  ``PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact()``, the same
  engine class the static registry already uses.

End-to-end card claims: Dockworker Drone (whose dies-trigger second
line was already parseable, blocked only on this batch's ETB-with-
counters first line) and Luxknight Breacher (single-line body).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    PutPlusOneCounterOnSelf,
    PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.triggers import (
    _parse_enters_with_body,
    _parse_triggered_ability_line,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# ---------- body-parser lookup ----------


def test_parse_enters_with_body_singular_counter():
    # The bare ``a +1/+1 counter`` body collapses to amount=1 — there's
    # no captured count word; the alternation in the regex chooses the
    # singular branch and the body-parser falls back to amount=1.
    assert _parse_enters_with_body(
        "a +1/+1 counter on it"
    ) == PutPlusOneCounterOnSelf(amount=1)


def test_parse_enters_with_body_plural_digit_counters():
    # Forward-looking — no EoE creature prints a digit-N enters-with
    # body, but the grammar is uniform. ``2`` is the captured count
    # word; ``_parse_amount`` resolves it to int 2.
    assert _parse_enters_with_body(
        "2 +1/+1 counters on it"
    ) == PutPlusOneCounterOnSelf(amount=2)


def test_parse_enters_with_body_plural_spelled_out_counters():
    # Spelled-out cardinal — ``two`` etc. ``_parse_amount`` resolves
    # via the ``_SMALL_CARDINAL_WORDS`` table the body shares with the
    # rest of the effect grammar.
    assert _parse_enters_with_body(
        "three +1/+1 counters on it"
    ) == PutPlusOneCounterOnSelf(amount=3)


def test_parse_enters_with_body_per_other_creature_or_artifact():
    # Luxknight Breacher's printed body. Maps to the same engine class
    # the static registry uses — no parameters, the per-other-creature-
    # or-artifact scaling is the engine class's whole semantics.
    assert _parse_enters_with_body(
        "a +1/+1 counter on it for each other creature "
        "and/or artifact you control"
    ) == PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact()


def test_parse_enters_with_body_unknown_shape_returns_none():
    # An unrecognized body returns ``None`` so the caller propagates
    # the rejection up to ``_parse_triggered_ability_line``. Forward-
    # looking variants like ``a charge counter on it`` would land here
    # until the body-parser grows a charge-counter primitive.
    assert _parse_enters_with_body("a charge counter on it") is None
    assert _parse_enters_with_body("flying") is None


# ---------- end-to-end line parsing ----------


def test_parse_triggered_ability_line_enters_with_singular_counter():
    # Dockworker Drone's first oracle line. Produces an ENTERS trigger
    # with ``IsSelf()`` predicate and a single-counter
    # ``PutPlusOneCounterOnSelf`` effect — exactly mirroring the static
    # registry's Dockworker Drone entry.
    parsed = _parse_triggered_ability_line(
        "This creature enters with a +1/+1 counter on it."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=IsSelf(),
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


def test_parse_triggered_ability_line_enters_with_plural_counters():
    # Forward-looking N>1 plural form — no EoE printing today, but the
    # grammar is uniform.
    parsed = _parse_triggered_ability_line(
        "This creature enters with two +1/+1 counters on it."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=IsSelf(),
        effect=PutPlusOneCounterOnSelf(amount=2),
    )


def test_parse_triggered_ability_line_enters_with_per_creature_or_artifact():
    # Luxknight Breacher's full single-line oracle text.
    parsed = _parse_triggered_ability_line(
        "This creature enters with a +1/+1 counter on it for each "
        "other creature and/or artifact you control."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=IsSelf(),
        effect=PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact(),
    )


def test_parse_triggered_ability_line_enters_with_unknown_body_rejects():
    # Recognized line shape (``This creature enters with …``) but an
    # unparseable body. The whole line rejects (returns ``None``) — we
    # don't fall through to the When/Whenever paths because an
    # unparseable ``enters with`` body has no other valid interpretation
    # as a trigger line. Claiming a partial trigger would silently drop
    # the actual replacement-effect semantics.
    assert _parse_triggered_ability_line(
        "This creature enters with a charge counter on it."
    ) is None


# ---------- end-to-end card claims ----------


def test_parser_claims_dockworker_drone():
    # Dockworker Drone's full oracle text now parses end-to-end:
    # line 1 (``This creature enters with a +1/+1 counter on it.``)
    # via the new ETB-replacement grammar, line 2 (``When this
    # creature dies, put its counters on target creature you
    # control.``) via the existing dies-trigger / move-self-counters
    # grammar (pre-existing, shipped with Thawbringer's batch).
    # Per-card parity against the static registry is enforced by
    # ``test_parser_parity.py``; this test confirms the card is in
    # the parser's claimed set, which is the gating observable for
    # the end-to-end claim.
    parser = OracleParserCardSource()
    assert "Dockworker Drone" in parser.claimed_names


def test_parser_claims_luxknight_breacher():
    # Luxknight Breacher's full single-line oracle text parses through
    # the new ETB-replacement grammar's per-other-creature-or-artifact
    # body shape.
    parser = OracleParserCardSource()
    assert "Luxknight Breacher" in parser.claimed_names
