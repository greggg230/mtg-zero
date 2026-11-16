"""Activated reanimate with the Xu-Ifit, Osteoharmonist Skeleton-rider.

Xu-Ifit's printed activated ability is

  ``{T}: Return target creature card from your graveyard to the
  battlefield. It's a Skeleton in addition to its other types and
  has no abilities. Activate only as a sorcery.``

The static registry intentionally drops the ``It's a Skeleton …``
rider — the engine doesn't yet model "has no abilities" suppression
nor in-place subtype grants on an arriving creature. This batch
mirrors that simplification in the parser by adding an explicit
multi-sentence regex that maps the full Xu-Ifit surface to the same
bare ``ReturnCreatureFromYourGraveyardToBattlefield`` primitive the
short-form regex produces.

Keeping the rider as its own regex (rather than a generic
preprocessing strip) makes the simplification local and explicit:
any future "raise dead" printing with a DIFFERENT rider tail will
miss the bare regex's ``$`` anchor and surface a parser miss instead
of being silently dropped.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    ReturnCreatureFromYourGraveyardToBattlefield,
)
from mtgz_ir.effects.generic.card_flow import CardFilter
from mtgz_ir.effects.generic.eoe_fidelity import (
    ReturnTargetCardFromYourGraveyardToBattlefieldAs,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect


# Faithful printing (feature/eoe-ir-fixes): the creature card is returned
# as a Skeleton in addition to its other types and loses all abilities.
_XU_IFIT_REANIMATE = ReturnTargetCardFromYourGraveyardToBattlefieldAs(
    card_filter=CardFilter(any_types=("Creature",)),
    added_subtypes=("Skeleton",),
    loses_all_abilities=True,
)


def test_reanimate_with_skeleton_rider_parses():
    """Multi-sentence Xu-Ifit surface (without the trailing
    ``Activate only as a sorcery.`` rider, which the
    ``_parse_activated_ability_line`` strips before handing the
    effect phrase to the unified grammar) parses to the same bare
    reanimate primitive the static registry uses."""
    eff = _parse_effect(
        "return target creature card from your graveyard to the "
        "battlefield. It's a Skeleton in addition to its other types "
        "and has no abilities"
    )
    assert eff == _XU_IFIT_REANIMATE


def test_reanimate_bare_form_unaffected():
    """Smoke check: the bare reanimate regex still claims the
    short-form phrase. The Xu-Ifit-specific multi-sentence regex is
    more-specific and is tried first; the bare regex below it is
    untouched."""
    eff = _parse_effect(
        "return target creature card from your graveyard to the battlefield"
    )
    assert eff == ReturnCreatureFromYourGraveyardToBattlefield()


def test_xu_ifit_activated_ability_line():
    """End-to-end shape: ``{T}: Return target creature card from your
    graveyard to the battlefield. It's a Skeleton in addition to its
    other types and has no abilities. Activate only as a sorcery.``"""
    line = (
        "{T}: Return target creature card from your graveyard to the "
        "battlefield. It's a Skeleton in addition to its other types "
        "and has no abilities. Activate only as a sorcery."
    )
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(tap_self=True),
        effect=_XU_IFIT_REANIMATE,
        sorcery_speed=True,
    )


