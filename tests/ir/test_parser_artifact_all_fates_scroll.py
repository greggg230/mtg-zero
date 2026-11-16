"""Parser claims All-Fates Scroll entirely through the unified grammar.

All-Fates Scroll prints:

    {T}: Add one mana of any color.
    {7}, {T}, Sacrifice this artifact: Draw X cards, where X is the
    number of differently named lands you control.

Both activations claim through the unified ``_parse_clause`` /
``_parse_effect`` grammar — no literal-line dispatch:

  * ``add one mana of any color`` →
    ``AddManaToPool(produces=(("any", 1),))``. The activation
    layer auto-flags ``is_mana_ability=True`` because the produced
    effect is a non-targeting ``AddManaToPool`` (CR 605). Distinct
    from the bracketed ``add {<color>}`` shape: the latter mints a
    fixed-color coin known at parse time, this one defers the
    color choice to resolution.
  * ``draw X cards, where X is the number of differently named lands
    you control`` → ``DrawCardsByConditionMultiplier(condition=
    PerDifferentlyNamedLand(), description_fragment=…)``. A generic
    primitive whose resolve site reads ``condition.multiplier(state,
    controller_id)`` at fire time — re-uses the same condition the
    spell-side cost-reduction grammar wires for Fungal Colossus's
    ``This spell costs {X} less to cast, where X is the number of
    differently named lands you control.``
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.conditions import PerDifferentlyNamedLand
from mtgz_ir.effects.generic import DrawCardsByConditionMultiplier


# --- Effect-grammar level ------------------------------------------------

def test_add_one_mana_of_any_color_effect_parses():
    """``add one mana of any color`` parses to
    ``AddManaToPool(produces=(("any", 1),))`` — same production tuple
    ``land_builder``'s Mana Confluence wiring uses for the at-resolution
    choose-color coin."""
    eff = _parse_effect("add one mana of any color")
    assert eff == AddManaToPool(produces=(("any", 1),))


# --- Activated-line level ------------------------------------------------

def test_add_one_mana_of_any_color_activation_is_mana_ability():
    """``{T}: Add one mana of any color.`` lifts to an
    ``ActivatedAbility`` with ``is_mana_ability=True`` — the activation
    layer auto-flags it because the produced ``AddManaToPool`` doesn't
    target (CR 605)."""
    ability = _parse_activated_ability_line(
        "{T}: Add one mana of any color."
    )
    assert ability is not None
    assert ability.cost == ActivationCost(tap_self=True)
    assert ability.effect == AddManaToPool(produces=(("any", 1),))
    assert ability.is_mana_ability is True


# --- Per-card builder level ----------------------------------------------

_ALL_FATES_SCROLL_CARD = {
    "name": "All-Fates Scroll",
    "mana_cost": "{3}",
    "type_line": "Artifact",
    "oracle_text": (
        "{T}: Add one mana of any color.\n"
        "{7}, {T}, Sacrifice this artifact: Draw X cards, where X is "
        "the number of differently named lands you control."
    ),
}


def test_draw_x_per_differently_named_land_effect_parses():
    """``draw X cards, where X is the number of differently named lands
    you control`` parses to the generic
    ``DrawCardsByConditionMultiplier(PerDifferentlyNamedLand(), …)`` —
    the same ``Condition`` the spell-side cost-reduction grammar uses
    for Fungal Colossus's parallel ``costs {X} less to cast`` rider."""
    eff = _parse_effect(
        "draw X cards, where X is the number of differently named lands "
        "you control"
    )
    assert eff == DrawCardsByConditionMultiplier(
        condition=PerDifferentlyNamedLand(),
        description_fragment=(
            "where X is the number of differently named lands you control"
        ),
    )


def test_build_artifact_printing_claims_all_fates_scroll():
    """The Scryfall-shaped All-Fates Scroll record builds an Artifact
    with two activated abilities — both claimed via the unified
    activated grammar (mana ability through the existing ``add one
    mana of any color`` recognizer; sac-draw through the new
    differently-named-land draw recognizer mapping to the generic
    ``DrawCardsByConditionMultiplier`` engine class)."""
    printing = _build_artifact_printing(_ALL_FATES_SCROLL_CARD)
    assert printing is not None
    assert printing.name == "All-Fates Scroll"
    assert printing.activated_abilities == (
        ActivatedAbility(
            cost=ActivationCost(tap_self=True),
            effect=AddManaToPool(produces=(("any", 1),)),
            is_mana_ability=True,
        ),
        ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{7}"),
                mana_str="{7}",
                tap_self=True,
                sacrifice_self=True,
            ),
            effect=DrawCardsByConditionMultiplier(
                condition=PerDifferentlyNamedLand(),
                description_fragment=(
                    "where X is the number of differently named lands "
                    "you control"
                ),
            ),
        ),
    )
    assert printing.triggered_abilities == ()
    assert printing.static_abilities == ()
    assert printing.keywords == frozenset()


