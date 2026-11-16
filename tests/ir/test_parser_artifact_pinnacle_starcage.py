"""Parser claims Pinnacle Starcage end-to-end through the unified
clause grammar (ETB line) plus the multi-sentence literal-effect
recognizer for the activation body.

Pinnacle Starcage prints:

    When this artifact enters, exile all artifacts and creatures with
    mana value 2 or less until this artifact leaves the battlefield.
    {6}{W}{W}: Put each card exiled with this artifact into its
    owner's graveyard, then create a 2/2 colorless Robot artifact
    creature token for each card put into a graveyard this way.
    Sacrifice this artifact.

The ETB line claims through the unified clause grammar: the
``"this artifact enters"`` predicate-trigger phrase produces
``(ENTERS, IsSelf())`` and the ``_EXILE_ALL_MATCHING_UNTIL_LEAVES``
effect-phrase regex maps the body to the generic
``ExileAllMatchingUntilSourceLeaves`` engine class
(``match_artifact=True, match_creature=True, max_mv=2``).

The activation body decomposes through a multi-sentence literal-
effect recognizer in ``effects.py`` into a Sequence of three generic
primitives:

* ``MoveEachExiledWithSourceToOwnersGraveyard`` — recalls each card
  tagged with ``exiled_by_source == source.id`` to its owner's
  graveyard, recording the count on
  ``state.last_exile_recall_count``.
* ``CreateNTokensFromLastExileRecall(token_name="Robot")`` — reads
  that count and mints one 2/2 colorless Robot artifact creature
  token per card moved.
* ``SacrificeSelf`` — sacrifices the source as the printed body's
  last clause (NOT a cost atom — the activation's printed cost is
  just ``{6}{W}{W}``).

The bespoke ``PinnacleStarcageSacEffect`` engine class was deleted in
favor of these reusable primitives.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CreateNTokensFromLastExileRecall,
    ExileAllMatchingUntilSourceLeaves,
    MoveEachExiledWithSourceToOwnersGraveyard,
    SacrificeSelf,
    TriggeredAbility,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.combinators import Sequence
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


_PINNACLE_STARCAGE_CARD = {
    "name": "Pinnacle Starcage",
    "mana_cost": "{1}{W}{W}",
    "type_line": "Artifact",
    "oracle_text": (
        "When this artifact enters, exile all artifacts and creatures "
        "with mana value 2 or less until this artifact leaves the "
        "battlefield.\n"
        "{6}{W}{W}: Put each card exiled with this artifact into its "
        "owner's graveyard, then create a 2/2 colorless Robot artifact "
        "creature token for each card put into a graveyard this way. "
        "Sacrifice this artifact."
    ),
}


def test_build_artifact_printing_claims_pinnacle_starcage():
    """The Scryfall-shaped Pinnacle Starcage record builds an Artifact
    whose ETB carries the generic ``ExileAllMatchingUntilSourceLeaves``
    engine class and whose ``{6}{W}{W}`` activation carries the
    decomposed Sequence of three generic primitives (recall, create-N,
    sacrifice-self)."""
    printing = _build_artifact_printing(_PINNACLE_STARCAGE_CARD)
    assert printing is not None
    assert printing.name == "Pinnacle Starcage"
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS,
            predicate=IsSelf(),
            effect=ExileAllMatchingUntilSourceLeaves(
                match_artifact=True,
                match_creature=True,
                max_mv=2,
            ),
        ),
    )
    assert printing.activated_abilities == (
        ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{6}{W}{W}"),
                mana_str="{6}{W}{W}",
            ),
            effect=Sequence(effects=(
                MoveEachExiledWithSourceToOwnersGraveyard(),
                CreateNTokensFromLastExileRecall(
                    token_name="Robot",
                    token_printing=TOKEN_PRINTINGS["Robot"],
                ),
                SacrificeSelf(),
            )),
        ),
    )
    assert printing.static_abilities == ()
    assert printing.keywords == frozenset()


