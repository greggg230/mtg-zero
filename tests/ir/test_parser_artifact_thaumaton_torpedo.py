"""Parser claims Thaumaton Torpedo via two new grammar atoms.

Thaumaton Torpedo prints:

    {6}, {T}, Sacrifice this artifact: Destroy target nonland permanent.
    This ability costs {3} less to activate if you attacked with a
    Spacecraft this turn.

Two pieces had to land before the artifact builder claims it:

  * ``destroy target nonland permanent`` — new effect-grammar primitive
    mapping to the existing ``DestroyTargetNonlandPermanent`` engine
    class. Distinct from the existing
    ``destroy target noncreature permanent`` (creatures excluded) — the
    nonland form excludes only lands.
  * ``you attacked with a Spacecraft this turn`` — new condition phrase
    on the activated-ability cost-reduction rider whitelist, mapping to
    the engine key ``"attacked_with_spacecraft_this_turn"`` already
    wired in ``apply_cost_reductions`` and latched via
    ``Player.attacked_with_spacecraft_this_turn``.

Once both atoms exist the unified ``_parse_clause`` orchestrator
already glues the rest together — the activated-ability line carries
the inline rider, the rider's condition resolves to a ``CostReduction``,
and the resulting ``ActivatedAbility`` matches the static registry's
``make_artifact(...)`` exactly.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CostReduction,
    DestroyTargetNonlandPermanent,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.conditions import AttackedWithThisTurn
from mtgz_ir.effects.object_predicates import HasSubtype


# --- Effect-grammar level ------------------------------------------------

def test_destroy_target_nonland_permanent_effect_parses():
    """``destroy target nonland permanent`` parses to
    ``DestroyTargetNonlandPermanent()`` — the engine class whose
    ``legal_targets`` filters the battlefield to non-land permanents."""
    assert _parse_effect("destroy target nonland permanent") == (
        DestroyTargetNonlandPermanent()
    )


def test_destroy_target_nonland_permanent_distinct_from_noncreature():
    """``destroy target noncreature permanent`` and ``destroy target
    nonland permanent`` are different primitives — the former excludes
    creatures, the latter excludes lands."""
    nonland = _parse_effect("destroy target nonland permanent")
    noncreature = _parse_effect("destroy target noncreature permanent")
    assert nonland is not None
    assert noncreature is not None
    assert type(nonland) is not type(noncreature)


# --- Activated-ability line level ----------------------------------------

def test_thaumaton_torpedo_activated_line_parses_with_cost_reduction():
    """End-to-end shape: ``{6}, {T}, Sacrifice this artifact: Destroy
    target nonland permanent. This ability costs {3} less to activate
    if you attacked with a Spacecraft this turn.``

    The rider strips out before the standard ``{cost}: <effect>.`` shape
    matches, and the captured reduction + condition produce a
    ``CostReduction`` on the ability's ``cost_reductions`` tuple — the
    same shape the static registry produces."""
    line = (
        "{6}, {T}, Sacrifice this artifact: "
        "Destroy target nonland permanent. "
        "This ability costs {3} less to activate "
        "if you attacked with a Spacecraft this turn."
    )
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost("{6}"), mana_str="{6}",
            tap_self=True,
            sacrifice_self=True,
        ),
        effect=DestroyTargetNonlandPermanent(),
        cost_reductions=(
            CostReduction(
                amount=3,
                condition=AttackedWithThisTurn(HasSubtype("Spacecraft")),
            ),
        ),
    )


# --- Per-card parser-vs-static parity ------------------------------------

_THAUMATON_TORPEDO_CARD = {
    "name": "Thaumaton Torpedo",
    "mana_cost": "{1}",
    "type_line": "Artifact",
    "oracle_text": (
        "{6}, {T}, Sacrifice this artifact: Destroy target nonland "
        "permanent. This ability costs {3} less to activate if you "
        "attacked with a Spacecraft this turn."
    ),
}


def test_build_artifact_printing_claims_thaumaton_torpedo():
    """The Scryfall-shaped Thaumaton Torpedo record builds an Artifact
    with one activated ability whose cost is ``{6}, {T}, Sacrifice this
    artifact``, whose effect is ``DestroyTargetNonlandPermanent``, and
    whose ``cost_reductions`` carry the spacecraft-attacked condition."""
    printing = _build_artifact_printing(_THAUMATON_TORPEDO_CARD)
    assert printing is not None
    assert printing.name == "Thaumaton Torpedo"
    assert printing.triggered_abilities == ()
    assert printing.keywords == frozenset()
    assert len(printing.activated_abilities) == 1
    ability = printing.activated_abilities[0]
    assert ability.effect == DestroyTargetNonlandPermanent()
    assert ability.cost.sacrifice_self
    assert ability.cost.tap_self
    assert ability.cost.mana_str == "{6}"
    assert ability.cost_reductions == (
        CostReduction(
            amount=3,
            condition=AttackedWithThisTurn(HasSubtype("Spacecraft")),
        ),
    )


