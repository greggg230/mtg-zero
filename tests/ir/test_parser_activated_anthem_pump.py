"""Activated-ability anthem / target-pump effects.

This batch wires two new pump shapes through the unified effect
grammar so they compose with the activated-ability cost classifier:

  * ``Creatures you control get +N/+M until end of turn`` —
    anthem-style mass pump on every creature the controller controls.
    Maps to ``PumpCreaturesYouControlUntilEOT``. Dual-Sun Adepts'
    ``{5}`` activated ability is the EoE printing.
  * ``Target creature you control gets +N/+M until end of turn`` —
    you-control-only single-target pump. Maps to
    ``PumpTargetCreatureUntilEOT(you_control_only=True)``. Zookeeper
    Mechan's ``{6}{R}`` sorcery-speed activated ability is the EoE
    printing.

Both shapes ride the standard ``{cost}: <effect>.`` activated-ability
line, so the tests exercise ``_parse_activated_ability_line`` directly
and via the per-card parser-vs-static parity check at the source level.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    PumpCreaturesYouControlUntilEOT,
    PumpTargetCreatureUntilEOT,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect


# --- Effect-grammar level tests ---------------------------------------------

def test_pump_creatures_you_control_ueot_parses():
    """Bare anthem phrase parses to ``PumpCreaturesYouControlUntilEOT``
    with no source-exclusion / attacking-only / untap-rider flags set —
    the surface form on Dual-Sun Adepts' ``{5}`` ability."""
    eff = _parse_effect("creatures you control get +1/+1 until end of turn")
    assert eff == PumpCreaturesYouControlUntilEOT(
        power_bonus=1, toughness_bonus=1,
    )


def test_pump_creatures_you_control_ueot_accepts_negative_bonus():
    """Both signs accepted for symmetry with the targeted/opponent-mass
    pump regexes; no EoE printing uses a negative self-mass pump today
    but the uniform shape parses one without further grammar work."""
    eff = _parse_effect("creatures you control get -1/-2 until end of turn")
    assert eff == PumpCreaturesYouControlUntilEOT(
        power_bonus=-1, toughness_bonus=-2,
    )


def test_pump_target_creature_you_control_ueot_parses():
    """``target creature you control gets ±N/±M until end of turn``
    surface (Zookeeper Mechan's ``{6}{R}`` ability) parses to
    ``PumpTargetCreatureUntilEOT(you_control_only=True)`` so
    ``legal_targets`` filters the creature pool to the controller's
    own creatures."""
    eff = _parse_effect(
        "target creature you control gets +4/+0 until end of turn"
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=4, toughness_bonus=0, you_control_only=True,
    )


def test_pump_target_creature_bare_form_unaffected():
    """Smoke check: the new ``you control`` regex is more-specific so
    the bare ``target creature gets …`` form still maps to the
    unrestricted ``PumpTargetCreatureUntilEOT``."""
    eff = _parse_effect("target creature gets +2/+2 until end of turn")
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=2, toughness_bonus=2,
    )


# --- Activated-ability line level tests -------------------------------------

def test_dual_sun_adepts_activated_ability_line():
    """End-to-end shape on Dual-Sun Adepts: ``{5}: Creatures you
    control get +1/+1 until end of turn.`` — pure mana cost, anthem
    payload, no sorcery-speed rider."""
    line = "{5}: Creatures you control get +1/+1 until end of turn."
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost("{5}"), mana_str="{5}",
        ),
        effect=PumpCreaturesYouControlUntilEOT(
            power_bonus=1, toughness_bonus=1,
        ),
    )


def test_zookeeper_mechan_pump_activated_ability_line():
    """End-to-end shape on Zookeeper Mechan's second ability:
    ``{6}{R}: Target creature you control gets +4/+0 until end of
    turn. Activate only as a sorcery.`` The inline rider is
    consumed and the resulting ability is marked
    ``sorcery_speed=True``."""
    line = (
        "{6}{R}: Target creature you control gets +4/+0 until end of turn."
        " Activate only as a sorcery."
    )
    ability = _parse_activated_ability_line(line)
    assert ability == ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost("{6}{R}"), mana_str="{6}{R}",
        ),
        effect=PumpTargetCreatureUntilEOT(
            power_bonus=4, toughness_bonus=0, you_control_only=True,
        ),
        sorcery_speed=True,
    )


# --- Per-card parser-vs-static parity ---------------------------------------





