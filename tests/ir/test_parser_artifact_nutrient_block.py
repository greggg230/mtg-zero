"""Parser claims Nutrient Block via graveyard-from-battlefield trigger.

Nutrient Block prints:

    Indestructible (Effects that say "destroy" don't destroy this
    artifact.)
    {2}, {T}, Sacrifice this artifact: You gain 3 life.
    When this artifact is put into a graveyard from the battlefield,
    draw a card.

The new grammar atom is the trigger phrase ``"this artifact is put
into a graveyard from the battlefield"``: the ``Event.DIES`` zone
change (graveyard from the battlefield, any permanent type), not
LEAVES_BATTLEFIELD (which also fires on exile / hand / library
moves). It was LEAVES_BATTLEFIELD until 2026-10-24, mirroring a
static-registry simplification ("slightly looser — also fires if
exiled").

Other lines (Indestructible keyword, ``{2}, {T}, Sacrifice this
artifact: You gain 3 life.`` activated ability) all already parse via
the unified clause grammar — only the graveyard-zone-change trigger
phrase was missing.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    DrawCards,
    GainLife,
    TriggeredAbility,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# --- Trigger-phrase level ------------------------------------------------

def test_artifact_grave_from_battlefield_trigger_parses():
    """``When this artifact is put into a graveyard from the
    battlefield, draw a card.`` parses to a single
    ``TriggeredAbility(event=DIES, predicate=IsSelf(),
    effect=DrawCards(amount=1))``."""
    line = (
        "When this artifact is put into a graveyard from the "
        "battlefield, draw a card."
    )
    clause = _parse_clause(line)
    assert clause is not None
    assert clause.triggers == (
        TriggeredAbility(
            event=Event.DIES,
            predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
    )


# --- Per-card builder level ----------------------------------------------

_NUTRIENT_BLOCK_CARD = {
    "name": "Nutrient Block",
    "mana_cost": "{1}",
    "type_line": "Artifact — Food",
    "oracle_text": (
        "Indestructible (Effects that say \"destroy\" don't destroy "
        "this artifact.)\n"
        "{2}, {T}, Sacrifice this artifact: You gain 3 life.\n"
        "When this artifact is put into a graveyard from the "
        "battlefield, draw a card."
    ),
}


def test_build_artifact_printing_claims_nutrient_block():
    """The Scryfall-shaped Nutrient Block record builds an Artifact
    with the indestructible keyword, the DIES (put into a graveyard
    from the battlefield) trigger drawing a card, and the ``{2}, {T},
    Sacrifice this artifact: You gain 3 life.`` activated ability."""
    printing = _build_artifact_printing(_NUTRIENT_BLOCK_CARD)
    assert printing is not None
    assert printing.name == "Nutrient Block"
    assert printing.keywords == frozenset({"indestructible"})
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.DIES,
            predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
    )
    assert printing.activated_abilities == (
        ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{2}"), mana_str="{2}",
                tap_self=True, sacrifice_self=True,
            ),
            effect=GainLife(amount=3),
        ),
    )


