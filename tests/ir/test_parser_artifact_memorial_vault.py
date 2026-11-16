"""Parser claims Memorial Vault via the unified activated-grammar
plus the formula-driven generic ``ExileTopXOfLibraryMayPlayThisTurn``.

Memorial Vault prints:

    {T}, Sacrifice another artifact: Exile the top X cards of your
    library, where X is one plus the mana value of the sacrificed
    artifact. You may play those cards this turn.

Cost atom ``{T}, Sacrifice another artifact`` parses through the
unified activated grammar. The body composes the new generic
``ExileTopXOfLibraryMayPlayThisTurn(amount=MVOfCostPaidObjectPlus
(constant=1))``: the ``MVOfCostPaidObjectPlus`` formula reads
``state.resolving_cost_paid_objects[0]`` (the artifact threaded onto
the StackItem by the cost-payment pipeline) and adds 1, giving the
"X is one plus the mana value of the sacrificed artifact" semantics.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.effects.formulas import MVOfCostPaidObjectPlus
from mtgz_ir.effects.generic import ExileTopXOfLibraryMayPlayThisTurn


_MEMORIAL_VAULT_CARD = {
    "name": "Memorial Vault",
    "mana_cost": "{3}{R}",
    "type_line": "Artifact",
    "oracle_text": (
        "{T}, Sacrifice another artifact: Exile the top X cards of "
        "your library, where X is one plus the mana value of the "
        "sacrificed artifact. You may play those cards this turn."
    ),
}


def test_build_artifact_printing_claims_memorial_vault():
    """The Scryfall-shaped Memorial Vault record now builds via the
    unified clause grammar — cost atom through the activated cost
    parser, body through ``_parse_effect``'s literal-multi-sentence
    recognizer mapping to the generic primitive."""
    printing = _build_artifact_printing(_MEMORIAL_VAULT_CARD)
    assert printing is not None
    assert printing.name == "Memorial Vault"
    assert printing.activated_abilities == (
        ActivatedAbility(
            cost=ActivationCost(
                tap_self=True,
                sacrifice_another_artifact=True,
            ),
            effect=ExileTopXOfLibraryMayPlayThisTurn(
                amount=MVOfCostPaidObjectPlus(constant=1),
            ),
        ),
    )
    # Memorial Vault has no triggered/static abilities or keywords.
    assert printing.triggered_abilities == ()
    assert printing.static_abilities == ()
    assert printing.keywords == frozenset()
