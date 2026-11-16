"""Tests for the audit-litdispatch-auras decomposition that retired
the literal-trigger-line dispatch on Aura builders.

The parent audit flagged an 8-entry literal-trigger-line table on
``mtgz_ir.cards.parser.enchantment_builder`` as "hidden bespoke
dispatch": each entry mapped a verbatim oracle trigger line to a
``TriggeredAbility`` rather than letting the unified
``_parse_triggered_ability_line`` grammar claim the line. Seven of
the eight entries were migrated to grammar-driven dispatch in the
initial pass (audit-litdispatch-auras). The eighth and final entry
— Chorale of the Void's attacks-trigger reanimate — was migrated in
audit-monoist-chorale: the body now claims through a literal multi-
sentence recognizer in ``effects._parse_effect`` mapping to the
generic ``ReanimateTargetCreatureCardFromDefendersGraveyard``
primitive, and the trigger header parses through the unified
grammar. The empty-shell dict was retired in
retire-empty-bespoke-shells.

This file asserts:

* every migrated trigger line still parses through the unified
  ``_parse_triggered_ability_line`` path to the same
  ``TriggeredAbility`` shape the literal table previously
  produced;
* Chorale of the Void's "Void —" end-step rider, which uses the
  "sacrifice unless" surface (not the standard intervening-if
  Void-gate surface), parses with ``requires_void=False`` because
  the void check rides the resolving effect class
  (``SacrificeSelfUnlessVoidActive``) rather than the trigger
  flag.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import TriggeredAbility
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.effects import (
    DestroyEnchantedPermanent,
    EnchantedCreatureFightsTargetEffect,
    PainForAllDamageEchoEffect,
    PainForAllETBEffect,
    ReanimateTargetCreatureCardFromDefendersGraveyard,
    SacrificeSelfUnlessVoidActive,
    TargetOpponentLookHandMayDiscard,
    TargetOpponentSacrificesCreatureOtherThanEnchanted,
)
from mtgz_ir.effects.predicates import (
    YOU,
    Controlled,
    Event,
    IsHost,
    IsSelf,
)


# ---------- migrated entries: parse via unified path ----------


@pytest.mark.parametrize(
    "line,expected",
    [
        # Cryoshatter / Cracked Skull / Sporogenic Infection
        # ``destroy it`` Aura DEALT_DAMAGE body.
        (
            "When enchanted creature is dealt damage, destroy it.",
            TriggeredAbility(
                event=Event.DEALT_DAMAGE,
                predicate=IsHost(),
                effect=DestroyEnchantedPermanent(),
            ),
        ),
        # Meltstrider's Resolve fight ETB.
        (
            "When this Aura enters, enchanted creature fights up to one "
            "target creature an opponent controls.",
            TriggeredAbility(
                event=Event.ENTERS,
                predicate=IsSelf(),
                effect=EnchantedCreatureFightsTargetEffect(),
            ),
        ),
        # Pain for All ETB body.
        (
            "When this Aura enters, enchanted creature deals damage equal "
            "to its power to any other target.",
            TriggeredAbility(
                event=Event.ENTERS,
                predicate=IsSelf(),
                effect=PainForAllETBEffect(),
            ),
        ),
        # Pain for All damage-echo body.
        (
            "Whenever enchanted creature is dealt damage, it deals that "
            "much damage to each opponent.",
            TriggeredAbility(
                event=Event.DEALT_DAMAGE,
                predicate=IsHost(),
                effect=PainForAllDamageEchoEffect(),
            ),
        ),
        # Cracked Skull look-at-hand multi-sentence body.
        (
            "When this Aura enters, look at target player's hand. "
            "You may choose a nonland card from it. "
            "That player discards that card.",
            TriggeredAbility(
                event=Event.ENTERS,
                predicate=IsSelf(),
                effect=TargetOpponentLookHandMayDiscard(),
            ),
        ),
        # Sporogenic Infection target-opp-sacs body.
        (
            "When this Aura enters, target player sacrifices a creature "
            "of their choice other than enchanted creature.",
            TriggeredAbility(
                event=Event.ENTERS,
                predicate=IsSelf(),
                effect=TargetOpponentSacrificesCreatureOtherThanEnchanted(),
            ),
        ),
        # Chorale's "Void — sacrifice unless" end-step rider.
        # ``requires_void=False`` here is correct: the void check
        # rides the resolving effect class itself
        # (``SacrificeSelfUnlessVoidActive``) which inverts the
        # trigger-flag semantics — the trigger fires every turn, the
        # SACRIFICE only happens when Void is inactive.
        (
            "Void — At the beginning of your end step, sacrifice this "
            "Aura unless a nonland permanent left the battlefield this "
            "turn or a spell was warped this turn.",
            TriggeredAbility(
                event=Event.BEGINNING_OF_END_STEP,
                predicate=Controlled(YOU),
                effect=SacrificeSelfUnlessVoidActive(),
                requires_void=False,
            ),
        ),
    ],
)
def test_migrated_aura_trigger_lines_parse_via_unified_path(
    line: str, expected: TriggeredAbility,
) -> None:
    """Each migrated line claims through ``_parse_triggered_ability_line``
    and produces the same ``TriggeredAbility`` shape the (now-retired)
    literal table previously held."""
    parsed = _parse_triggered_ability_line(line)
    assert parsed == expected


# ---------- final literal entry migrated: Chorale attacks trigger ----------


def test_chorale_attacks_trigger_parses_via_unified_path() -> None:
    """Chorale's attacks-trigger body claims through the unified
    grammar: trigger header (``Whenever enchanted creature attacks``)
    parses via the predicate-trigger phrasebook; effect body parses
    via ``_parse_effect``'s literal multi-sentence recognizer mapping
    to the generic ``ReanimateTargetCreatureCardFromDefendersGraveyard``
    primitive.
    """
    line = (
        "Whenever enchanted creature attacks, put target creature card "
        "from defending player's graveyard onto the battlefield under "
        "your control tapped and attacking."
    )
    parsed = _parse_triggered_ability_line(line)
    expected = TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsHost(subject="attacker"),
        effect=ReanimateTargetCreatureCardFromDefendersGraveyard(),
    )
    assert parsed == expected
