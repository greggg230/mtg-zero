"""The legacy pool's Legacy Lands cards of lane A (mtg-zero's deck MTGGoldfish
8027955: Dark Depths, Thespian's Stage, Blast Zone, Ghost Quarter, Maze of Ith,
Bojuka Bog; docs/ENGINE.md 270.1, docs/ir_defects.md section AB): their
printings, parsed by the existing grammar (no recognizer added). Thespian's
Stage and Blast Zone are the EoE pool's printings too."""

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir

LANE_A = ("Dark Depths", "Thespian's Stage", "Blast Zone", "Ghost Quarter", "Maze of Ith", "Bojuka Bog")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_six_cards_are_claimed(legacy):
    assert set(LANE_A) <= set(legacy)


def test_dark_depths(legacy):
    d = legacy["Dark Depths"]
    assert (d.supertypes, d.card_types, d.subtypes) == (("Legendary", "Snow"), ("Land",), ())
    # No mana ability (the ruling): nothing produced.
    assert d.produces == frozenset() and d.mana_only_from_produces
    assert d.enters_with_named_counters == (("ice", 10),)
    (a,) = d.activated_abilities
    assert a.cost.mana_str == "{3}" and not a.cost.tap_self and not a.is_mana_ability
    assert type(a.effect).__name__ == "RemoveCountersFromSource" and (a.effect.kind, a.effect.amount) == ("ice", 1)
    # CR 603.8: the last line is a state trigger.
    (st,) = d.board_rules
    assert type(st.condition).__name__ == "HasNoCountersOfKind" and st.condition.kind == "ice"
    t = st.ability
    assert t.event is None and t.predicate is None
    assert type(t.effect).__name__ == "SacrificeSourceThen"
    tok = t.effect.then_effect
    assert type(tok).__name__ == "CreateToken" and tok.count == 1 and not tok.tapped
    lage = tok.token_printing
    assert (lage.name, lage.power, lage.toughness, lage.subtypes) == ("Marit Lage", 20, 20, ("Avatar",))
    assert lage.card_types == ("Creature",) and "Legendary" in lage.supertypes
    assert lage.color_indicator == frozenset({"B"})
    assert lage.keywords == frozenset({"flying", "indestructible"})


def test_thespians_stage_and_blast_zone_are_the_eoe_printings(legacy):
    eoe = ir.pool_printings("eoe")
    for n in ("Thespian's Stage", "Blast Zone"):
        assert ir.strict_equal(legacy[n], eoe[n]), n
    s = legacy["Thespian's Stage"]
    assert s.produces == frozenset({"C"})
    (a,) = s.activated_abilities
    assert a.cost.mana_str == "{2}" and a.cost.tap_self and type(a.effect).__name__ == "BecomesCopyOfTargetLand"
    b = legacy["Blast Zone"]
    assert b.enters_with_named_counters == (("charge", 1),)
    x, boom = b.activated_abilities
    assert x.cost.mana_str == "{X}{X}" and x.cost.mana.x_pips == 2 and x.effect.amount_from_x
    assert boom.cost.sacrifice_self and boom.cost.tap_self and boom.cost.mana_str == "{3}"
    assert type(boom.effect).__name__ == "DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters"


def test_ghost_quarter(legacy):
    g = legacy["Ghost Quarter"]
    mana, destroy = g.activated_abilities
    assert mana.is_mana_ability and mana.effect.produces == (("C", 1),)
    assert destroy.cost.tap_self and destroy.cost.sacrifice_self and destroy.cost.mana is None
    e = destroy.effect
    assert type(e).__name__ == "RemoveTargetPermanents" and e.action.value == "destroy"
    assert type(e.target_filter).__name__ == "IsLand" and (e.count_min, e.count_max) == (1, 1)
    r = e.rider
    assert type(r).__name__ == "ControllerMaySearchOntoBattlefield" and not r.tapped
    assert r.card_filter.any_types == ("Land",) and r.card_filter.supertype == "Basic"


def test_maze_of_ith(legacy):
    m = legacy["Maze of Ith"]
    assert m.produces == frozenset()
    (a,) = m.activated_abilities
    assert a.cost.tap_self and a.cost.mana is None and not a.is_mana_ability
    e = a.effect
    assert type(e).__name__ == "PreventCombatDamageToAndByTarget" and e.untap
    (t,) = e.targets
    assert type(t.filter).__name__ == "ObjectAnd"
    assert {type(t.filter.left).__name__, type(t.filter.right).__name__} == {"IsCreature", "IsAttacking"}


def test_bojuka_bog(legacy):
    b = legacy["Bojuka Bog"]
    assert b.enters_tapped and b.produces == frozenset({"B"})
    (t,) = b.triggered_abilities
    assert t.event.value == "enters" and type(t.predicate).__name__ == "IsSelf"
    assert type(t.effect).__name__ == "ExileTargetPlayersGraveyard"
