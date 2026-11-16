"""Activated-ability cost-reduction rider parsing.

Adds the inline ``This ability costs {N} less to activate if
<condition>.`` rider to ``_parse_activated_ability_line``.

The first EoE printing is Starport Security's
``{3}{W}, {T}: Tap another target creature. This ability costs {2}
less to activate if you control a creature with a +1/+1 counter on
it.`` Two pieces had to land before the line claims:

  * ``tap another target creature`` effect — new primitive in the
    unified grammar that maps to ``TapAnotherTargetCreature``
    (creature-only legal pool, source-self exclusion via
    ``state.resolving_source``).
  * Inline cost-reduction rider — the trailing rider sentence is
    stripped from the activation line before the standard
    ``{cost}: <effect>.`` shape is matched, and the captured
    reduction amount + whitelisted condition phrase produce a
    ``CostReduction`` attached to the ability's ``cost_reductions``
    tuple.

Unknown condition phrase rejects the whole line so the wrapping card
stays unclaimed rather than silently dropping the gate.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CostReduction,
    TapAnotherTargetCreature,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.conditions import ControlPermanent
from mtgz_ir.effects.object_predicates import HasCounter, IsCreature


# --- Effect-grammar level tests ---------------------------------------------

def test_tap_another_target_creature_effect_parses():
    """``tap another target creature`` parses to
    ``TapAnotherTargetCreature()`` — Starport Security's activated
    payload."""
    eff = _parse_effect("tap another target creature")
    assert eff == TapAnotherTargetCreature()


# --- Activated-ability line level tests -------------------------------------

def test_starport_security_activated_ability_line():
    """End-to-end shape: ``{3}{W}, {T}: Tap another target creature.
    This ability costs {2} less to activate if you control a creature
    with a +1/+1 counter on it.``

    The rider strips out before the standard ``{cost}: <effect>.``
    shape matches, and the engine ``CostReduction`` rides the
    ability's ``cost_reductions`` tuple — same shape the static
    registry produces."""
    line = (
        "{3}{W}, {T}: Tap another target creature. "
        "This ability costs {2} less to activate "
        "if you control a creature with a +1/+1 counter on it."
    )
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost("{3}{W}"),
            mana_str="{3}{W}",
            tap_self=True,
        ),
        effect=TapAnotherTargetCreature(),
        cost_reductions=(
            CostReduction(
                amount=2,
                condition=ControlPermanent(
                    IsCreature() & HasCounter("+1/+1"),
                ),
            ),
        ),
    )


def test_unknown_cost_reduction_condition_rejects_whole_line():
    """Defensive: an activated-ability rider with a condition phrase
    that isn't on the closed whitelist returns ``None`` so the
    wrapping card stays unclaimed. Silent drop would leave the engine
    free to activate the ability for the full cost when the static
    side gates it on a condition."""
    line = (
        "{3}{W}, {T}: Tap another target creature. "
        "This ability costs {2} less to activate "
        "if the moon is full."
    )
    assert _parse_activated_ability_line(line) is None


def test_no_cost_reduction_rider_unaffected():
    """Smoke check: a vanilla activated-ability line with no rider
    still parses cleanly. The rider regex requires the literal
    ``This ability costs ...`` sentence so plain effects fall through
    to the standard ``{cost}: <effect>.`` shape."""
    line = "{T}: Tap another target creature."
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(tap_self=True),
        effect=TapAnotherTargetCreature(),
    )


# --- Per-card parser-vs-static parity ---------------------------------------

