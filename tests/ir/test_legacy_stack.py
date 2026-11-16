"""The Legacy Doomsday deck's lane B cards in the ``legacy`` pool (mtg-zero
ENGINE.md 124.2): Misdirection (``cards/parser/legacy_stack.py``,
``effects/generic/legacy_stack.py``), Flusterstorm and Cavern of Souls (the
roto grammars' parses, the same printings as the roto pool's)."""

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.cards.parser.legacy_stack import parse_legacy_stack_effect
from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
from mtgz_ir.effects.generic.legacy_stack import ChangeTheTargetOfTargetWithASingleTarget
from mtgz_ir.effects.vocab import StackObjectKind

SPELL = (StackObjectKind.SPELL,)
SPELL_OR_ABILITY = (StackObjectKind.SPELL, StackObjectKind.ACTIVATED, StackObjectKind.TRIGGERED)


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


@pytest.fixture(scope="module")
def roto():
    return ir.pool_printings("roto")


def test_misdirection(legacy):
    m = legacy["Misdirection"]
    assert (m.mana_cost_str, m.card_types) == ("{3}{U}{U}", ("Instant",))
    # "You may exile a blue card from your hand rather than pay this
    # spell's mana cost." (Pyrokinesis' shape: exile only, no life).
    assert m.alt_cost_exile_from_hand is not None and m.alt_cost_exile_from_hand.color == "U"
    assert m.alt_cost_exile_from_hand_count == 1
    assert m.alternative_costs == ()
    # "Change the target of target spell with a single target."
    assert m.spell_effect == ChangeTheTargetOfTargetWithASingleTarget(kinds=SPELL, spell_filter=None)


@pytest.mark.parametrize("phrase, kinds, filtered", [
    ("change the target of target spell with a single target", SPELL, False),
    ("change the target of target instant or sorcery spell with a single target", SPELL, True),
])
def test_the_recognizer(phrase, kinds, filtered):
    eff = parse_legacy_stack_effect(phrase)
    assert isinstance(eff, ChangeTheTargetOfTargetWithASingleTarget)
    assert eff.kinds == kinds
    assert (eff.spell_filter is not None) == filtered
    if filtered:
        what = phrase[len("change the target of target "):-len(" with a single target")]
        assert eff.spell_filter == parse_spell_phrase(what)[1]
    # The whole effect grammar reaches it (its last fallback).
    assert _parse_effect(phrase) == eff


@pytest.mark.parametrize("phrase", [
    "change the target of target spell",
    "change the targets of target spell with a single target",
    "change the target of target creature with a single target",
    "change the target of target spell with a single target. draw a card",
    "you may choose new targets for target spell",
    # Left for the fix of Untimely Malfunction's modal (the module docs).
    "change the target of target spell or ability with a single target",
])
def test_phrases_it_does_not_claim(phrase):
    assert parse_legacy_stack_effect(phrase) is None


def test_change_the_target_is_card_ir_data_only():
    # The Python engine's spells carry no targets: choosing or resolving it
    # fails loudly (SEARCH_LIMITATIONS, "Legacy pool" 2).
    eff = ChangeTheTargetOfTargetWithASingleTarget()
    assert eff.requires_target() and eff.target_count_range() == (1, 1)
    assert eff.kinds == SPELL and eff.spell_filter is None
    with pytest.raises(NotImplementedError, match="card-IR data"):
        eff.legal_targets(None, 0)
    with pytest.raises(NotImplementedError, match="card-IR data"):
        eff.resolve(None, 0, ())
    assert eff.describe() == "change the target of target spell with a single target"
    assert ChangeTheTargetOfTargetWithASingleTarget(kinds=SPELL_OR_ABILITY).describe() == (
        "change the target of target spell or activated ability or triggered ability with a single target"
    )


def test_flusterstorm(legacy, roto):
    f = legacy["Flusterstorm"]
    assert (f.mana_cost_str, f.card_types) == ("{U}", ("Instant",))
    # Storm (CR 702.40a).
    assert f.has_storm
    # "Counter target instant or sorcery spell unless its controller pays {1}."
    eff = f.spell_effect
    assert type(eff).__name__ == "CounterSpell" and eff.unless_pays == "{1}"
    assert eff.spell_filter == parse_spell_phrase("instant or sorcery spell")[1]
    # The roto pool's printing (the same Scryfall record).
    assert roto["Flusterstorm"] == f


def test_cavern_of_souls(legacy, roto):
    c = legacy["Cavern of Souls"]
    assert c.card_types == ("Land",) and c.mana_cost_str == ""
    # "As this land enters, choose a creature type." (CR 614.12, 205.3m)
    assert c.enters_choosing_creature_type
    # "{T}: Add {C}."
    assert c.produces == frozenset({"C"}) and c.mana_only_from_produces
    # "{T}: Add one mana of any color. Spend this mana only to cast a
    # creature spell of the chosen type, and that spell can't be countered."
    (ab,) = c.activated_abilities
    assert ab.is_mana_ability and ab.cost.tap_self
    eff = ab.effect
    assert type(eff).__name__ == "AddAnyColorManaForChosenCreatureType"
    assert eff.produces == (("any", 1),) and eff.makes_spell_uncounterable
    assert eff.spend_restriction is None and not eff.grants_haste_to_creature_spell
    assert roto["Cavern of Souls"] == c
