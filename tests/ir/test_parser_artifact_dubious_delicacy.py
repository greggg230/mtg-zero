"""Parser claims Dubious Delicacy via the up-to-one optional pump.

Dubious Delicacy prints:

    Flash
    When this artifact enters, up to one target creature gets -3/-3
    until end of turn.
    {2}, {T}, Sacrifice this artifact: You gain 3 life.
    {2}, {T}, Sacrifice this artifact: Target opponent loses 3 life.

The new grammar atom is the ``up to one target creature gets ±N/±M
until end of turn`` regex. ``up to one`` flips the engine class's
``target_count_range`` to ``(0, 1)`` and ``requires_target`` to False
on ``PumpTargetCreatureUntilEOT(optional=True)``, so a no-target
resolution is a clean no-op rather than a fizzle (matters when the
battlefield has no creatures at trigger-time). The flash keyword,
typed-self ETB trigger phrase, and the two activated lines all
already parse via the unified clause grammar — only the optional
pump variant was missing.

The static registry's ``Dubious Delicacy`` entry currently drops the
printed Food subtype (``make_artifact`` doesn't expose subtypes), so
the parser-built printing matches by also producing a bare Artifact
type line — parity holds. This is a known static-side simplification
documented in eoe_creatures.py and is out of scope for this batch.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    GainLife,
    PumpTargetCreatureUntilEOT,
    TargetOpponentLosesLife,
    TriggeredAbility,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# --- Effect-grammar level ------------------------------------------------

def test_up_to_one_target_creature_pump_parses_optional():
    """``up to one target creature gets -3/-3 until end of turn`` parses
    to ``PumpTargetCreatureUntilEOT(power_bonus=-3, toughness_bonus=-3,
    optional=True)`` — the ``optional=True`` flag is what distinguishes
    this from the bare ``target creature gets`` form (which would
    fizzle when no creature is on the battlefield)."""
    eff = _parse_effect(
        "up to one target creature gets -3/-3 until end of turn"
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=-3, toughness_bonus=-3, optional=True,
    )


def test_up_to_one_pump_accepts_positive_bonus():
    """The regex accepts both signs (no EoE printing today uses
    ``+N/+M`` with ``up to one``, but symmetry with the bare-pump
    grammar means a future printing parses without further work)."""
    eff = _parse_effect(
        "up to one target creature gets +1/+2 until end of turn"
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=1, toughness_bonus=2, optional=True,
    )


def test_up_to_one_pump_distinct_from_bare_pump():
    """The ``up to one`` form returns ``optional=True``; the bare form
    does not. Both produce ``PumpTargetCreatureUntilEOT`` instances —
    the difference is the ``optional`` flag controlling target-count
    semantics at resolution."""
    optional = _parse_effect(
        "up to one target creature gets -3/-3 until end of turn"
    )
    bare = _parse_effect(
        "target creature gets -3/-3 until end of turn"
    )
    assert optional == PumpTargetCreatureUntilEOT(
        power_bonus=-3, toughness_bonus=-3, optional=True,
    )
    assert bare == PumpTargetCreatureUntilEOT(
        power_bonus=-3, toughness_bonus=-3, optional=False,
    )
    assert optional != bare


# --- Per-card builder level ----------------------------------------------

_DUBIOUS_DELICACY_CARD = {
    "name": "Dubious Delicacy",
    "mana_cost": "{2}{B}",
    "type_line": "Artifact — Food",
    "oracle_text": (
        "Flash\n"
        "When this artifact enters, up to one target creature gets "
        "-3/-3 until end of turn.\n"
        "{2}, {T}, Sacrifice this artifact: You gain 3 life.\n"
        "{2}, {T}, Sacrifice this artifact: Target opponent loses 3 "
        "life."
    ),
}


def test_build_artifact_printing_claims_dubious_delicacy():
    """The Scryfall-shaped Dubious Delicacy record builds an Artifact
    with the flash keyword, the typed-self ETB optional debuff, and
    the two ``{2}, {T}, Sacrifice this artifact`` activated abilities
    — same shape as the static registry's ``make_artifact(...)`` for
    this card."""
    printing = _build_artifact_printing(_DUBIOUS_DELICACY_CARD)
    assert printing is not None
    assert printing.name == "Dubious Delicacy"
    assert "flash" in printing.keywords
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=PumpTargetCreatureUntilEOT(
                power_bonus=-3, toughness_bonus=-3, optional=True,
            ),
        ),
    )
    # Two activated abilities: identical cost shape, distinct effects.
    assert len(printing.activated_abilities) == 2
    cost = ActivationCost(
        mana=parse_mana_cost("{2}"), mana_str="{2}",
        tap_self=True, sacrifice_self=True,
    )
    assert printing.activated_abilities[0] == ActivatedAbility(
        cost=cost, effect=GainLife(amount=3),
    )
    assert printing.activated_abilities[1] == ActivatedAbility(
        cost=cost, effect=TargetOpponentLosesLife(amount=3),
    )


