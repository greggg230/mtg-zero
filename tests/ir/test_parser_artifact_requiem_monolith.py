"""Parser claims Requiem Monolith via decomposed generic primitives.

Requiem Monolith prints:

    {T}: Until end of turn, target creature gains "Whenever this
    creature is dealt damage, you draw that many cards and lose that
    much life." That creature's controller may have this artifact deal
    1 damage to it. Activate only as a sorcery.

The body decomposes into a ``Sequence`` of two generic primitives:

  1. ``GrantTriggeredAbilityUEOT(ability=TriggeredAbility(trigger=
     Event.DEALT_DAMAGE + IsSelf("obj"), effect=DrawCardsAndLoseLifeEqualToResolving
     Damage()))`` — the quoted "Whenever this creature is dealt
     damage, you draw that many cards and lose that much life."
     trigger granted UEOT to the target creature.
  2. ``MayHaveSourceDealDamageToTarget(amount=1)`` — the optional
     1-damage ping the target's controller may take. Anaphoric to the
     target chosen by the upstream UEOT grant; ``Sequence.resolve``
     threads the chosen target tuple to every sub-effect.

The trailing ``Activate only as a sorcery.`` rider lifts to
``sorcery_speed=True`` on the ability via the existing inline-rider
strip in ``activated.py``.
"""

from __future__ import annotations

from mtgz_ir.effects.predicates import Event, IsSelf

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.effects import (
    DrawCardsAndLoseLifeEqualToResolvingDamage,
    GrantTriggeredAbilityUEOT,
    MayHaveSourceDealDamageToTarget,
    Sequence,
)


_REQUIEM_MONOLITH_CARD = {
    "name": "Requiem Monolith",
    "mana_cost": "{2}{B}",
    "type_line": "Artifact",
    "oracle_text": (
        "{T}: Until end of turn, target creature gains \"Whenever "
        "this creature is dealt damage, you draw that many cards and "
        "lose that much life.\" That creature's controller may have "
        "this artifact deal 1 damage to it. Activate only as a sorcery."
    ),
}


def test_build_artifact_printing_claims_requiem_monolith():
    """The Scryfall-shaped Requiem Monolith record builds an Artifact
    with the canonical ``{T}`` sorcery-speed activation whose effect is
    the decomposed ``Sequence(GrantTriggeredAbilityUEOT(...),
    MayHaveSourceDealDamageToTarget(amount=1))``."""
    printing = _build_artifact_printing(_REQUIEM_MONOLITH_CARD)
    assert printing is not None
    assert printing.name == "Requiem Monolith"
    assert printing.activated_abilities == (
        ActivatedAbility(
            cost=ActivationCost(tap_self=True),
            effect=Sequence(effects=(
                GrantTriggeredAbilityUEOT(
                    ability=TriggeredAbility(
                        event=Event.DEALT_DAMAGE,
                        predicate=IsSelf("obj"),
                        effect=DrawCardsAndLoseLifeEqualToResolvingDamage(),
                    ),
                ),
                MayHaveSourceDealDamageToTarget(amount=1),
            )),
            sorcery_speed=True,
        ),
    )
    assert printing.triggered_abilities == ()
    assert printing.static_abilities == ()
    assert printing.keywords == frozenset()
