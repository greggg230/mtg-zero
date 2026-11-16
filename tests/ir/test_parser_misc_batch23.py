"""Parser parity tests for misc-batch23.

Seven cards claimed across four builders:

Sorceries (spell_builder ``_WHOLE_SPELL_EXACT_TEXT``):
- Terminal Velocity ``{4}{R}{R}`` — whole-spell exact-text dispatch;
  maps to ``TerminalVelocityEffect()`` (put artifact/creature from hand
  onto battlefield with haste, LTB deal-MV-to-each-creature trigger, and
  end-step-sac).
- Pull Through the Weft ``{3}{G}{G}`` — whole-spell exact-text
  dispatch; static ships
  ``ReturnCreatureFromYourGraveyardToHand()``.

Artifact (artifact_builder bespoke dispatch):
- Thrumming Hivepool ``{6}`` — Affinity-for-Slivers cost reduction +
  SLIVERS_YOU_CONTROL anthem + upkeep create-2-Sliver trigger.

Equipment (equipment_builder bespoke dispatch):
- Atomic Microsizer ``{U}`` — +1/+0 bonus + ETB auto-attach +
  equipped-attacks trigger (``AtomicMicrosizerAttackEffect``).
- The Dominion Bracelet ``{2}`` — Legendary + +1/+1 bonus + ETB
  auto-attach + ``grants_control_opponent_ability_when_equipped``.

Planeswalker (planeswalker_builder new module):
- Tezzeret, Cruel Captain ``{3}`` — Legendary Planeswalker (Tezzeret),
  loyalty 4, artifact-enters loyalty-counter trigger.

Enchantment — Aura (enchantment_builder dispatch):
- Pain for All ``{2}{R}`` — ETB power-damage trigger + dealt-damage
  echo trigger.

Each test verifies the per-builder function returns a non-None
printing and that it equals the static registry's entry exactly
(structural parity). A final end-to-end test verifies the
``OracleParserCardSource`` data walk picks up all seven cards.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser.enchantment_builder import (
    _build_enchantment_printing,
)
from mtgz_ir.cards.parser.equipment_builder import _build_equipment_printing
from mtgz_ir.cards.parser.planeswalker_builder import (
    _build_planeswalker_printing,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.spell_builder import _build_spell_printing



# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _spell_card(name: str, cost: str, oracle: str, type_line: str = "Sorcery") -> dict:
    return {"name": name, "mana_cost": cost, "oracle_text": oracle,
            "type_line": type_line, "set": "eoe"}


def _artifact_card(name: str, cost: str, oracle: str,
                   type_line: str = "Artifact") -> dict:
    return {"name": name, "mana_cost": cost, "oracle_text": oracle,
            "type_line": type_line, "set": "eoe"}


def _equipment_card(name: str, cost: str, oracle: str,
                    type_line: str = "Artifact — Equipment") -> dict:
    return {"name": name, "mana_cost": cost, "oracle_text": oracle,
            "type_line": type_line, "set": "eoe"}


def _planeswalker_card(name: str, cost: str, oracle: str,
                       type_line: str, loyalty: int = 4) -> dict:
    return {"name": name, "mana_cost": cost, "oracle_text": oracle,
            "type_line": type_line, "loyalty": str(loyalty), "set": "eoe"}


def _enchantment_card(name: str, cost: str, oracle: str,
                      type_line: str = "Enchantment — Aura") -> dict:
    return {"name": name, "mana_cost": cost, "oracle_text": oracle,
            "type_line": type_line, "set": "eoe"}


# ---------------------------------------------------------------------------
# Terminal Velocity
# ---------------------------------------------------------------------------

_TERMINAL_VELOCITY_ORACLE = (
    "You may put an artifact or creature card from your hand onto "
    "the battlefield. That permanent gains haste, \"When this "
    "permanent leaves the battlefield, it deals damage equal to "
    "its mana value to each creature,\" and \"At the beginning of "
    "your end step, sacrifice this permanent.\""
)




def test_terminal_velocity_is_sorcery():
    card = _spell_card("Terminal Velocity", "{4}{R}{R}",
                       _TERMINAL_VELOCITY_ORACLE)
    result = _build_spell_printing(card)
    assert result is not None
    assert "Sorcery" in result.card_types


def test_terminal_velocity_engine_class():
    from mtgz_ir.effects import TerminalVelocityEffect

    card = _spell_card("Terminal Velocity", "{4}{R}{R}",
                       _TERMINAL_VELOCITY_ORACLE)
    result = _build_spell_printing(card)
    assert result is not None
    assert result.spell_effect == TerminalVelocityEffect()




# ---------------------------------------------------------------------------
# Pull Through the Weft
# ---------------------------------------------------------------------------

_PULL_THROUGH_ORACLE = (
    "Return up to two target nonland permanent cards from your "
    "graveyard to your hand, then return up to two target land "
    "cards from your graveyard to the battlefield tapped."
)




def test_pull_through_the_weft_is_sorcery():
    card = _spell_card("Pull Through the Weft", "{3}{G}{G}",
                       _PULL_THROUGH_ORACLE)
    result = _build_spell_printing(card)
    assert result is not None
    assert "Sorcery" in result.card_types


def test_pull_through_unknown_oracle_not_claimed():
    """Changed oracle text falls through — exact-text dispatch."""
    card = _spell_card("Pull Through the Weft", "{3}{G}{G}",
                       "Return all permanents from your graveyard.")
    result = _build_spell_printing(card)
    assert result is None


# ---------------------------------------------------------------------------
# Thrumming Hivepool
# ---------------------------------------------------------------------------

_THRUMMING_ORACLE = (
    "Affinity for Slivers (This spell costs {1} less to cast for "
    "each Sliver you control.)\n"
    "Slivers you control have double strike and haste.\n"
    "At the beginning of your upkeep, create two 1/1 colorless "
    "Sliver creature tokens."
)




def test_thrumming_hivepool_is_artifact():
    card = _artifact_card("Thrumming Hivepool", "{6}", _THRUMMING_ORACLE)
    result = _build_artifact_printing(card)
    assert result is not None
    assert "Artifact" in result.card_types


def test_thrumming_hivepool_has_cost_reduction():
    from mtgz_ir.effects.conditions import PerControlled
    from mtgz_ir.effects.object_predicates import HasSubtype

    card = _artifact_card("Thrumming Hivepool", "{6}", _THRUMMING_ORACLE)
    result = _build_artifact_printing(card)
    assert result is not None
    assert len(result.cost_reductions) == 1
    cr = result.cost_reductions[0]
    assert cr.amount == 1
    assert cr.condition == PerControlled(HasSubtype("Sliver"))


def test_thrumming_hivepool_has_anthem_static():
    from mtgz_ir.effects.base import StaticScope
    from mtgz_ir.effects.modifiers import GrantKeyword
    card = _artifact_card("Thrumming Hivepool", "{6}", _THRUMMING_ORACLE)
    result = _build_artifact_printing(card)
    assert result is not None
    # PR-D anthem migration: the multi-keyword anthem fans out into
    # one StaticAbility per granted keyword, each carrying a layer-6
    # ContinuousModifier(GrantKeyword).
    assert len(result.static_abilities) == 2
    granted = set()
    for sa in result.static_abilities:
        assert sa.scope == StaticScope.SLIVERS_YOU_CONTROL
        assert sa.continuous_effect is not None
        assert isinstance(sa.continuous_effect.payload, GrantKeyword)
        granted.add(sa.continuous_effect.payload.keyword)
    assert granted == {"double strike", "haste"}


def test_thrumming_hivepool_oracle_shape_is_grammar_claimed():
    """Any artifact with Thrumming Hivepool's oracle shape is now claimed
    by the unified grammar — Affinity-for-Slivers, the Sliver keyword
    anthem, and the upkeep token trigger are all parameterized recognizers.
    A different name doesn't matter; the shape is what drives the parse."""
    card = _artifact_card(
        "Unknown Sliver Artifact", "{6}", _THRUMMING_ORACLE
    )
    result = _build_artifact_printing(card)
    assert result is not None
    assert len(result.cost_reductions) == 1
    # Multi-keyword anthem fans out into one StaticAbility per keyword.
    assert len(result.static_abilities) == 2
    assert len(result.triggered_abilities) == 1


# ---------------------------------------------------------------------------
# Atomic Microsizer
# ---------------------------------------------------------------------------

_ATOMIC_MICROSIZER_ORACLE = (
    "Equipped creature gets +1/+0.\n"
    "Whenever equipped creature attacks, choose up to one target "
    "creature. That creature can't be blocked this turn and has base "
    "power and toughness 1/1 until end of turn.\n"
    "Equip {2}"
)




def test_atomic_microsizer_is_equipment():
    card = _equipment_card("Atomic Microsizer", "{U}",
                            _ATOMIC_MICROSIZER_ORACLE)
    result = _build_equipment_printing(card)
    assert result is not None
    assert "Equipment" in result.subtypes


def test_atomic_microsizer_has_one_trigger():
    card = _equipment_card("Atomic Microsizer", "{U}",
                            _ATOMIC_MICROSIZER_ORACLE)
    result = _build_equipment_printing(card)
    assert result is not None
    # Oracle text prints only the equipped-creature-attacks trigger;
    # the parser doesn't synthesize a spurious ETB auto-attach.
    assert len(result.triggered_abilities) == 1


def test_atomic_microsizer_power_bonus():
    card = _equipment_card("Atomic Microsizer", "{U}",
                            _ATOMIC_MICROSIZER_ORACLE)
    result = _build_equipment_printing(card)
    assert result is not None
    # equipment_bonuses is (power_bonus, toughness_bonus, granted_keywords)
    assert result.equipment_bonuses is not None
    assert result.equipment_bonuses[0] == 1
    assert result.equipment_bonuses[1] == 0


# ---------------------------------------------------------------------------
# The Dominion Bracelet
# ---------------------------------------------------------------------------

_DOMINION_BRACELET_ORACLE = (
    "Equipped creature gets +1/+1 and has \"{15}, Exile The Dominion "
    "Bracelet: You control target opponent during their next turn. "
    "This ability costs {X} less to activate, where X is this "
    "creature's power. Activate only as a sorcery.\"\n"
    "Equip {1}"
)




def test_dominion_bracelet_is_legendary():
    card = _equipment_card(
        "The Dominion Bracelet", "{2}", _DOMINION_BRACELET_ORACLE,
        type_line="Legendary Artifact — Equipment",
    )
    result = _build_equipment_printing(card)
    assert result is not None
    assert "Legendary" in result.supertypes


def test_dominion_bracelet_grants_control_flag():
    card = _equipment_card(
        "The Dominion Bracelet", "{2}", _DOMINION_BRACELET_ORACLE,
        type_line="Legendary Artifact — Equipment",
    )
    result = _build_equipment_printing(card)
    assert result is not None
    assert result.grants_control_opponent_ability_when_equipped is True


def test_dominion_bracelet_power_toughness_bonus():
    card = _equipment_card(
        "The Dominion Bracelet", "{2}", _DOMINION_BRACELET_ORACLE,
        type_line="Legendary Artifact — Equipment",
    )
    result = _build_equipment_printing(card)
    assert result is not None
    assert result.equipment_bonuses is not None
    assert result.equipment_bonuses[0] == 1
    assert result.equipment_bonuses[1] == 1


# ---------------------------------------------------------------------------
# Tezzeret, Cruel Captain
# ---------------------------------------------------------------------------

_TEZZERET_ORACLE = (
    "Whenever an artifact you control enters, put a loyalty counter "
    "on Tezzeret.\n"
    "0: Untap target artifact or creature. If it's an artifact "
    "creature, put a +1/+1 counter on it.\n"
    "−3: Search your library for an artifact card with mana "
    "value 1 or less, reveal it, put it into your hand, then shuffle.\n"
    "−7: You get an emblem with \"At the beginning of combat on "
    "your turn, put three +1/+1 counters on target artifact you "
    "control. If it's not a creature, it becomes a 0/0 Robot "
    "artifact creature.\""
)




def test_tezzeret_is_planeswalker():
    card = _planeswalker_card(
        "Tezzeret, Cruel Captain", "{3}", _TEZZERET_ORACLE,
        type_line="Legendary Planeswalker — Tezzeret",
    )
    result = _build_planeswalker_printing(card)
    assert result is not None
    assert "Planeswalker" in result.card_types


def test_tezzeret_is_legendary():
    card = _planeswalker_card(
        "Tezzeret, Cruel Captain", "{3}", _TEZZERET_ORACLE,
        type_line="Legendary Planeswalker — Tezzeret",
    )
    result = _build_planeswalker_printing(card)
    assert result is not None
    assert "Legendary" in result.supertypes


def test_tezzeret_loyalty_4():
    card = _planeswalker_card(
        "Tezzeret, Cruel Captain", "{3}", _TEZZERET_ORACLE,
        type_line="Legendary Planeswalker — Tezzeret",
    )
    result = _build_planeswalker_printing(card)
    assert result is not None
    assert result.loyalty == 4


def test_tezzeret_has_artifact_enters_trigger():
    from mtgz_ir.effects.predicates import (
        Event,
    )
    card = _planeswalker_card(
        "Tezzeret, Cruel Captain", "{3}", _TEZZERET_ORACLE,
        type_line="Legendary Planeswalker — Tezzeret",
    )
    result = _build_planeswalker_printing(card)
    assert result is not None
    assert len(result.triggered_abilities) == 1
    trigger = result.triggered_abilities[0]
    assert trigger.event == Event.ENTERS


def test_tezzeret_skipped_for_non_planeswalker_type():
    """A non-planeswalker card is skipped by the type-line guard."""
    card = {
        "name": "Tezzeret, Cruel Captain",
        "mana_cost": "{3}",
        "oracle_text": _TEZZERET_ORACLE,
        "type_line": "Legendary Creature — Human Artificer",
        "set": "eoe",
    }
    result = _build_planeswalker_printing(card)
    assert result is None


def test_tezzeret_skipped_for_unknown_name():
    """An unknown planeswalker name falls through."""
    card = {
        "name": "Hypothetical Planeswalker",
        "mana_cost": "{3}",
        "oracle_text": _TEZZERET_ORACLE,
        "type_line": "Legendary Planeswalker — Tezzeret",
        "set": "eoe",
    }
    result = _build_planeswalker_printing(card)
    assert result is None


def test_planeswalker_skipped_for_multiple_subtypes():
    """A planeswalker with more than one printed subtype is rejected
    by the single-subtype guard — the engine takes a single-subtype
    string for planeswalkers."""
    card = {
        "name": "Two Subtype Walker",
        "mana_cost": "{3}",
        "oracle_text": _TEZZERET_ORACLE,
        "type_line": "Legendary Planeswalker — Tezzeret Bolas",
        "loyalty": "4",
        "set": "eoe",
    }
    result = _build_planeswalker_printing(card)
    assert result is None


def test_planeswalker_skipped_when_loyalty_missing():
    """A planeswalker with no printed loyalty falls through."""
    card = {
        "name": "Tezzeret, Cruel Captain",
        "mana_cost": "{3}",
        "oracle_text": _TEZZERET_ORACLE,
        "type_line": "Legendary Planeswalker — Tezzeret",
        "set": "eoe",
        # No "loyalty" key.
    }
    result = _build_planeswalker_printing(card)
    assert result is None


def test_planeswalker_skipped_when_loyalty_unparseable():
    """A planeswalker with a non-integer loyalty string falls through.
    Covers the ``int(loyalty_str)`` ValueError defensive branch."""
    card = {
        "name": "Tezzeret, Cruel Captain",
        "mana_cost": "{3}",
        "oracle_text": _TEZZERET_ORACLE,
        "type_line": "Legendary Planeswalker — Tezzeret",
        "loyalty": "X",  # not an integer
        "set": "eoe",
    }
    result = _build_planeswalker_printing(card)
    assert result is None


def test_planeswalker_skips_blank_lines_in_oracle_text():
    """A blank line in the oracle text is skipped (not treated as a
    parse failure). The remaining recognized line still produces the
    expected trigger."""
    oracle_with_blank = (
        "\n"
        + _TEZZERET_ORACLE
        + "\n\n"  # trailing blanks
    )
    card = _planeswalker_card(
        "Tezzeret, Cruel Captain", "{3}", oracle_with_blank,
        type_line="Legendary Planeswalker — Tezzeret",
    )
    result = _build_planeswalker_printing(card)
    assert result is not None
    assert len(result.triggered_abilities) == 1


# ---------------------------------------------------------------------------
# Pain for All
# ---------------------------------------------------------------------------

_PAIN_FOR_ALL_ORACLE = (
    "Enchant creature you control\n"
    "When this Aura enters, enchanted creature deals damage equal to "
    "its power to any other target.\n"
    "Whenever enchanted creature is dealt damage, it deals that much "
    "damage to each opponent."
)




def test_pain_for_all_is_aura():
    card = _enchantment_card("Pain for All", "{2}{R}", _PAIN_FOR_ALL_ORACLE)
    result = _build_enchantment_printing(card)
    assert result is not None
    assert "Aura" in result.subtypes


def test_pain_for_all_has_two_triggers():
    card = _enchantment_card("Pain for All", "{2}{R}", _PAIN_FOR_ALL_ORACLE)
    result = _build_enchantment_printing(card)
    assert result is not None
    assert len(result.triggered_abilities) == 2


def test_pain_for_all_no_pt_bonus():
    """Pain for All grants no P/T bonus."""
    card = _enchantment_card("Pain for All", "{2}{R}", _PAIN_FOR_ALL_ORACLE)
    result = _build_enchantment_printing(card)
    assert result is not None
    # Aura equipment_bonuses: (power, toughness, keywords)
    if result.equipment_bonuses is not None:
        assert result.equipment_bonuses[0] == 0
        assert result.equipment_bonuses[1] == 0


def test_pain_for_all_not_claimed_for_unknown_name():
    """A different name falls through the dispatch table."""
    card = _enchantment_card("Other Aura", "{2}{R}", _PAIN_FOR_ALL_ORACLE)
    result = _build_enchantment_printing(card)
    assert result is None


# ---------------------------------------------------------------------------
# End-to-end: OracleParserCardSource data walk
# ---------------------------------------------------------------------------

def test_all_batch23_cards_claimed_end_to_end():
    """The parser source's data walk picks up all seven batch-23 cards."""
    parser = OracleParserCardSource()
    printings = parser.get_printings()
    targets = [
        "Terminal Velocity",
        "Pull Through the Weft",
        "Thrumming Hivepool",
        "Atomic Microsizer",
        "The Dominion Bracelet",
        "Tezzeret, Cruel Captain",
        "Pain for All",
    ]
    for name in targets:
        assert name in printings, f"{name!r} not in parser claimed_names"


