"""Parser claims Cryogen Relic via compound enters-or-leaves trigger.

Cryogen Relic prints:

    When this artifact enters or leaves the battlefield, draw a card.
    {1}{U}, Sacrifice this artifact: Put a stun counter on up to one
    target tapped creature.

Two pieces had to land before the artifact builder claims it:

  * Compound trigger phrase ``"this artifact enters or leaves the
    battlefield"`` decomposing into the two-arm tuple
    ``(ENTERS, IsSelf())`` + ``(LEAVES_BATTLEFIELD, IsSelf())``.
    Mirrors the existing Thawbringer (``enters or dies``) and
    Haliya/Syr Vondam (``enters or attacks``) compound entries.
  * ``put a stun counter on up to one target tapped creature`` —
    new effect-grammar primitive mapping to the existing
    ``PutStunCounterOnTappedCreature`` engine class. ``up to one``
    resolves as a no-op fizzle when no legal target is chosen.

Both atoms compose end-to-end through the unified clause grammar; the
artifact builder produces two separate ``TriggeredAbility`` entries
(one per arm) plus the activated ability — the same shape the static
registry produces.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    DrawCards,
    PutStunCounterOnTappedCreature,
    TriggeredAbility,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# --- Effect-grammar level ------------------------------------------------

def test_put_stun_counter_up_to_one_tapped_creature_parses():
    """``put a stun counter on up to one target tapped creature``
    parses to the existing ``PutStunCounterOnTappedCreature()`` engine
    class — Cryogen Relic's activated payload."""
    eff = _parse_effect("put a stun counter on up to one target tapped creature")
    assert eff == PutStunCounterOnTappedCreature()


# --- Compound-trigger level ----------------------------------------------

def test_compound_artifact_enters_or_leaves_decomposes_into_two_arms():
    """``When this artifact enters or leaves the battlefield, draw a
    card.`` decomposes into one ``TriggeredAbility`` per arm: an
    ENTERS+IsSelf and a LEAVES_BATTLEFIELD+IsSelf, both with the same
    ``DrawCards(amount=1)`` effect."""
    line = (
        "When this artifact enters or leaves the battlefield, "
        "draw a card."
    )
    clause = _parse_clause(line)
    assert clause is not None
    assert clause.triggers == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
        TriggeredAbility(
            event=Event.LEAVES_BATTLEFIELD, predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
    )


# --- Per-card builder level ----------------------------------------------

_CRYOGEN_RELIC_CARD = {
    "name": "Cryogen Relic",
    "mana_cost": "{1}{U}",
    "type_line": "Artifact",
    "oracle_text": (
        "When this artifact enters or leaves the battlefield, "
        "draw a card.\n"
        "{1}{U}, Sacrifice this artifact: Put a stun counter on up to "
        "one target tapped creature. (If a permanent with a stun "
        "counter would become untapped, remove one from it instead.)"
    ),
}


def test_build_artifact_printing_claims_cryogen_relic():
    """The Scryfall-shaped Cryogen Relic record builds an Artifact with
    two ``TriggeredAbility`` entries (one per arm of the compound
    enters-or-leaves trigger) and one activated ability whose effect is
    ``PutStunCounterOnTappedCreature``."""
    printing = _build_artifact_printing(_CRYOGEN_RELIC_CARD)
    assert printing is not None
    assert printing.name == "Cryogen Relic"
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
        TriggeredAbility(
            event=Event.LEAVES_BATTLEFIELD, predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
    )
    assert printing.activated_abilities == (
        ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{1}{U}"),
                mana_str="{1}{U}",
                sacrifice_self=True,
            ),
            effect=PutStunCounterOnTappedCreature(),
        ),
    )


