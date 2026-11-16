"""Activated-ability mana abilities with a ``Spend this mana only
to <restriction>`` rider (Steelswarm Operator).

The unified effect grammar learned the multi-sentence
``Add {<mana>}. Spend this mana only to <restriction>`` shape this
batch. Restrictions wired today (one per Steelswarm Operator
ability):

  * ``cast an artifact spell`` → ``ActivationCost.spend_restriction =
    "artifact_spell"`` (gates cost-payment to artifact spells via
    ``_coin_satisfies_context`` in ``actions.py``).
  * ``activate abilities of artifact sources`` →
    ``"artifact_activation"`` (gates cost-payment to activated
    abilities of artifact-typed sources).

The two-coin same-color ``Add {U}{U}`` mana segment is also wired so
the second activation produces a tuple of two single-coin entries —
matching the static registry's
``produces=(("U", 1), ("U", 1))`` shape.

The activated-ability ``is_mana_ability`` flag fires for the
restricted variant too: ``_is_mana_ability_effect`` only checks the
effect class and target requirement, both of which still hold for an
``AddManaToPool`` carrying a ``spend_restriction`` (no targets, mints
mana into the controller's pool).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect


# --- Effect-grammar level tests ---------------------------------------------

def test_add_one_color_with_artifact_spell_restriction_parses():
    """``add {U}. Spend this mana only to cast an artifact spell``
    parses to ``AddManaToPool(produces=(('U', 1),),
    spend_restriction='artifact_spell')`` — Steelswarm Operator's
    first ability."""
    eff = _parse_effect(
        "add {U}. Spend this mana only to cast an artifact spell"
    )
    assert eff == AddManaToPool(
        produces=(("U", 1),),
        spend_restriction="artifact_spell",
    )


def test_add_two_same_color_with_artifact_activation_restriction_parses():
    """``add {U}{U}. Spend this mana only to activate abilities of
    artifact sources`` parses to two single-coin entries with the
    artifact-activation restriction — Steelswarm Operator's second
    ability."""
    eff = _parse_effect(
        "add {U}{U}. Spend this mana only to activate abilities "
        "of artifact sources"
    )
    assert eff == AddManaToPool(
        produces=(("U", 1), ("U", 1)),
        spend_restriction="artifact_activation",
    )


def test_add_two_same_color_bare_parses():
    """``add {R}{R}`` (no spend rider) parses to two same-color coins
    with no restriction. No EoE printing today uses an unrestricted
    two-coin add — the pattern is wired uniformly with the restricted
    shape so a future printing parses without further grammar work."""
    eff = _parse_effect("add {R}{R}")
    assert eff == AddManaToPool(produces=(("R", 1), ("R", 1)))


def test_add_two_mismatched_color_rejected():
    """Defensive: the two-coin same-color regex captures both letters
    independently and the dispatcher rejects mismatched pairs so the
    engine class always sees a uniform color tuple. No EoE printing
    uses a dual-color two-coin add anyway: with the bucket grammars off
    (as for EoE-only printings) the phrase stays unparsed. The Vintage
    Roto P3 karoo recognizer (Simic Growth Chamber's "{T}: Add {G}{U}.")
    mints one coin of each color instead."""
    from mtgz_ir.cards.parser.creature_rmc_grammar import rmc_grammar_disabled

    with rmc_grammar_disabled():
        assert _parse_effect("add {U}{R}") is None
    assert _parse_effect("add {U}{R}") == AddManaToPool(
        produces=(("U", 1), ("R", 1)),
    )


# --- Activated-ability line level tests -------------------------------------

def test_steelswarm_operator_first_activated_ability_line():
    """End-to-end first ability: ``{T}: Add {U}. Spend this mana only
    to cast an artifact spell.`` — pure tap cost, restricted mana
    payload, ``is_mana_ability=True`` (the engine bypasses the stack
    on activation per CR 605)."""
    line = (
        "{T}: Add {U}. Spend this mana only to cast an artifact spell."
    )
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(tap_self=True),
        effect=AddManaToPool(
            produces=(("U", 1),),
            spend_restriction="artifact_spell",
        ),
        is_mana_ability=True,
    )


def test_steelswarm_operator_second_activated_ability_line():
    """End-to-end second ability: ``{T}: Add {U}{U}. Spend this mana
    only to activate abilities of artifact sources.``"""
    line = (
        "{T}: Add {U}{U}. Spend this mana only to "
        "activate abilities of artifact sources."
    )
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(tap_self=True),
        effect=AddManaToPool(
            produces=(("U", 1), ("U", 1)),
            spend_restriction="artifact_activation",
        ),
        is_mana_ability=True,
    )


# --- Per-card parser-vs-static parity ---------------------------------------

