"""Parser claims The Endstone end-to-end via the unified clause grammar.

The Endstone is a Legendary Artifact whose oracle text is two trigger
lines:

    Whenever you play a land or cast a spell, draw a card.
    At the beginning of your end step, your life total becomes half
    your starting life total, rounded up.

Both lines now parse through the unified ``_parse_clause`` grammar:

  * The first is a composite trigger — two events (``play a land`` /
    ``cast a spell``) joined by ``or`` — claimed by the
    ``"you play a land or cast a spell"`` entry in
    ``_COMPOUND_PREDICATE_TRIGGER_PHRASES`` (one ``TriggeredAbility``
    per arm, mirroring the per-arm fan-out shape Cryogen Relic's
    compound enters-or-leaves trigger uses). The arms appear in
    printed-text order — PLAYS_LAND first, then IS_CAST. Predicate
    vocabulary stays primitive (one event per ability).
  * The second uses the existing ``"your end step"`` entry in
    ``_AT_BEGINNING_OF_PHRASES`` for the trigger header plus a new
    literal-phrase recognizer in ``effects.py`` for the body
    ``"your life total becomes half your starting life total, rounded
    up"`` mapping to the generic ``SetLifeToHalfStartingRoundedUp``
    engine class.

The Legendary supertype rides through ``make_artifact(legendary=True)``
the builder already wires off the parsed type-line.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DrawCards,
    SetLifeToHalfStartingRoundedUp,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    YOU,
)


_ENDSTONE_CARD = {
    "name": "The Endstone",
    "mana_cost": "{7}",
    "type_line": "Legendary Artifact",
    "oracle_text": (
        "Whenever you play a land or cast a spell, draw a card.\n"
        "At the beginning of your end step, your life total becomes "
        "half your starting life total, rounded up."
    ),
}


def test_build_artifact_printing_claims_the_endstone():
    """The Scryfall-shaped Endstone record builds a Legendary Artifact
    with three ``TriggeredAbility`` entries — PLAYS_LAND + IS_CAST for
    the composite first line (per-arm fan-out, in printed-text order),
    and BEGINNING_OF_END_STEP for the ``SetLifeToHalfStartingRoundedUp``
    body — every arm produced by the unified ``_parse_clause`` grammar."""
    printing = _build_artifact_printing(_ENDSTONE_CARD)
    assert printing is not None
    assert printing.name == "The Endstone"
    assert printing.is_legendary is True
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.PLAYS_LAND,
            predicate=Controlled(YOU, subject="controller"),
            effect=DrawCards(amount=1),
        ),
        TriggeredAbility(
            event=Event.IS_CAST,
            predicate=Controlled(YOU, subject="controller"),
            effect=DrawCards(amount=1),
        ),
        TriggeredAbility(
            event=Event.BEGINNING_OF_END_STEP,
            predicate=Controlled(YOU),
            effect=SetLifeToHalfStartingRoundedUp(),
        ),
    )
    assert printing.activated_abilities == ()
    assert printing.static_abilities == ()
    assert printing.keywords == frozenset()


