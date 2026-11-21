"""The legacy pool's Legacy Hogaak Tempo cards of lane C (mtg-zero's deck
MTGGoldfish 8027954: Stitcher's Supplier, Dark Matter Manipulator,
Moonshadow, Dryad Arbor; docs/ENGINE.md 220.3): their printings and the
recognizers ``cards/parser/legacy_hogaak_c.py`` adds or widens: a land
creature's land type and intrinsic mana ability (docs/ir_defects.md 440),
"for every N cards in your graveyard", "enters with N -1/-1 counters",
"remove a -1/-1 counter from this creature", "one or more <type> cards are
put into your graveyard from anywhere" and the "while this creature has a
<kind> counter on it" gate."""

from __future__ import annotations

import json

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_hogaak_c import (
    as_land_creature,
    parse_hogaak_c_trigger_phrase,
    parse_source_has_counter_gate,
)
from mtgz_ir.effects.predicates import Event

LANE_C = ("Stitcher's Supplier", "Dark Matter Manipulator", "Moonshadow", "Dryad Arbor")


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_four_cards_are_claimed(legacy):
    assert set(LANE_C) <= set(legacy)


def test_stitchers_supplier(legacy):
    from mtgz_ir.effects.generic import SelfMill  # noqa: F401  (the class the grammar builds)

    s = legacy["Stitcher's Supplier"]
    assert (s.mana_cost_str, s.card_types, s.subtypes, s.power, s.toughness) == ("{B}", ("Creature",), ("Zombie",), 1, 1)
    # "When this creature enters or dies": one printed ability, the parser's
    # compound-condition shape writes one arm per event with the shared
    # effect (the events are disjoint; ENGINE.md 220.3).
    arms = s.triggered_abilities
    assert [a.event for a in arms] == [Event.ENTERS, Event.DIES]
    assert all(type(a.effect).__name__ == "SelfMill" and a.effect.amount == 3 for a in arms)
    assert all(type(a.predicate).__name__ == "IsSelf" for a in arms)


def test_dark_matter_manipulator(legacy):
    from mtgz_ir.effects.generic.roto_board import CardsInYourGraveyard

    d = legacy["Dark Matter Manipulator"]
    assert (d.mana_cost_str, d.card_types, d.subtypes, d.power, d.toughness) == ("{B}", ("Creature",), ("Human", "Warlock"), 1, 2)
    (enters,) = d.triggered_abilities
    assert enters.event == Event.ENTERS and type(enters.effect).__name__ == "SelfMill" and enters.effect.amount == 3
    (st,) = d.static_abilities
    assert (st.power_bonus, st.toughness_bonus, st.bonus_per_condition_count) == (2, 0, True)
    assert st.scope.kind.value == "self"
    assert isinstance(st.condition, CardsInYourGraveyard)
    assert st.condition.per == 7 and st.condition.at_least == 1
    # "cards": any card (no type named).
    f = st.condition.card_filter
    assert (f.any_types, f.excluded_types, f.max_mv, f.color, f.supertype, f.subtype) == ((), (), None, None, None, None)


def test_every_n_cards_counts_full_groups():
    from mtgz_ir.cards.parser.roto_board import _graveyard_pt_static
    from mtgz_ir.effects.generic.roto_board import CardsInYourGraveyard

    line = _graveyard_pt_static("This creature gets +1/+1 for every three land cards in your graveyard.")
    (st,) = line.statics
    assert st.condition.per == 3 and st.condition.card_filter.any_types == ("Land",)
    # "for every one card" is "for each": not this shape.
    assert _graveyard_pt_static("This creature gets +1/+1 for every one cards in your graveyard.") is None

    class _P:
        def __init__(self, n):
            self.graveyard = list(range(n))
            self.id = 0

    class _S:
        def __init__(self, n):
            self._p = _P(n)

        def player_by_id(self, _):
            return self._p

    class _Any:
        def matches_card(self, state, c):
            return True

    cond = CardsInYourGraveyard(card_filter=_Any(), per=7)
    assert [cond.multiplier(_S(n), 0) for n in (0, 6, 7, 13, 14, 20, 21)] == [0, 0, 1, 1, 2, 2, 3]


def test_moonshadow(legacy):
    from mtgz_ir.effects.generic.cube_creatures_rmc import OneOrMoreBatch
    from mtgz_ir.effects.generic.roto_mana import RemoveCountersFromSource
    from mtgz_ir.effects.object_predicates import HasCounter
    from mtgz_ir.effects.predicates import YOU, Controlled, Source, Subject
    from mtgz_ir.effects.value_predicates import IsPermanentCard

    m = legacy["Moonshadow"]
    assert (m.mana_cost_str, m.card_types, m.subtypes, m.power, m.toughness) == ("{B}", ("Creature",), ("Elemental",), 7, 7)
    assert m.keywords == frozenset({"menace"})
    # "This creature enters with six -1/-1 counters on it" (CR 614.1c).
    assert m.enters_with_named_counters == (("-1/-1", 6),)
    (t,) = m.triggered_abilities
    assert t.event == Event.CARD_PUT_INTO_GRAVEYARD
    assert t.effect == RemoveCountersFromSource(kind="-1/-1", amount=1)
    assert t.predicate == (
        (Controlled(YOU, subject="player") & (Subject("card", IsPermanentCard()) & OneOrMoreBatch()))
        & Source(HasCounter("-1/-1"))
    )
    assert not t.once_per_turn and not t.requires_void


def test_dryad_arbor(legacy):
    a = legacy["Dryad Arbor"]
    # docs/ir_defects.md 440: the land type was dropped, and its Forest's
    # intrinsic "{T}: Add {G}" (CR 305.6) with it.
    assert a.card_types == ("Land", "Creature")
    assert a.type_line == "Land Creature — Forest Dryad"
    assert a.subtypes == ("Forest", "Dryad") and a.supertypes == ()
    assert (a.power, a.toughness, a.mana_cost_str) == (1, 1, "")
    assert a.produces == frozenset({"G"}) and a.mana_only_from_produces
    assert a.color_indicator == frozenset({"G"})
    assert a.activated_abilities == () and a.triggered_abilities == () and a.static_abilities == ()


def test_the_roto_pools_dryad_arbor_is_a_land_too():
    from mtgz_ir.cards.parser.source import build_card_record
    from mtgz_ir.cards.pools import DATA_DIR

    rec = next(c for c in json.loads((DATA_DIR / "cube_roto.json").read_text(encoding="utf-8")) if c["name"] == "Dryad Arbor")
    p = build_card_record(rec)
    assert p.card_types == ("Land", "Creature") and p.produces == frozenset({"G"})


def test_as_land_creature_needs_the_printed_types():
    from mtgz_ir.cards.base import make_creature

    c = make_creature(name="X", cost="", power=1, toughness=1, subtypes=("Island", "Swamp", "Wizard"))
    rec = {"name": "X", "type_line": "Land Creature — Island Swamp Wizard"}
    p = as_land_creature(c, rec)
    assert p.card_types == ("Land", "Creature") and p.produces == frozenset({"U", "B"})
    # A non-basic land type: no intrinsic mana ability.
    c2 = make_creature(name="Y", cost="", power=1, toughness=1, subtypes=("Desert", "Wurm"))
    p2 = as_land_creature(c2, {"name": "Y", "type_line": "Land Creature — Desert Wurm"})
    assert p2.produces == frozenset() and p2.mana_only_from_produces
    # Not a land, or types that don't add up: not claimed.
    assert as_land_creature(c, {"name": "X", "type_line": "Creature — Island Swamp Wizard"}) is None
    assert as_land_creature(c, {"name": "X", "type_line": "Artifact Land Creature — Island Swamp Wizard"}) is None
    assert as_land_creature(c, {"name": "X", "type_line": "Land Creature — Island Wizard"}) is None


@pytest.mark.parametrize(
    "phrase, noun, whose",
    [
        ("one or more permanent cards are put into your graveyard from anywhere", "IsPermanentCard", "you"),
        ("one or more creature cards are put into an opponent's graveyard from anywhere", "IsCreature", "opponent"),
        ("one or more cards are put into a graveyard from anywhere", None, None),
        ("one or more land cards are put into your graveyard from anywhere", "IsLand", "you"),
    ],
)
def test_the_cards_put_into_a_graveyard_condition(phrase, noun, whose):
    event, pred = parse_hogaak_c_trigger_phrase(phrase)
    assert event == Event.CARD_PUT_INTO_GRAVEYARD
    text = repr(pred)
    assert "OneOrMoreBatch" in text
    assert (noun is None) == ("Subject" not in text)
    if noun is not None:
        assert noun in text
    if whose is None:
        assert "Controlled" not in text
    else:
        assert f"_Controller.{whose.upper() if whose == 'you' else 'OPPONENT'}" in text


def test_conditions_that_are_not_this_shape():
    for phrase in (
        "a permanent card is put into your graveyard from anywhere",
        "one or more permanent cards are put into your graveyard from your library",
        "one or more permanent cards are put into your graveyard",
    ):
        assert parse_hogaak_c_trigger_phrase(phrase) is None


def test_the_source_has_counter_gate():
    from mtgz_ir.effects.object_predicates import HasCounter
    from mtgz_ir.effects.predicates import Source

    assert parse_source_has_counter_gate("this creature has a -1/-1 counter on it") == Source(HasCounter("-1/-1"))
    assert parse_source_has_counter_gate("this artifact has a charge counter on it") == Source(HasCounter("charge"))
    assert parse_source_has_counter_gate("this creature has two -1/-1 counters on it") is None


def test_enters_with_counters_and_remove_counter_grammars():
    from mtgz_ir.cards.parser.creature_builder import _parse_enters_with_pt_counters_line
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic.roto_mana import RemoveCountersFromSource

    assert _parse_enters_with_pt_counters_line("This creature enters with six -1/-1 counters on it.") == ("-1/-1", 6)
    assert _parse_enters_with_pt_counters_line("This creature enters with a +1/+1 counter on it.") == ("+1/+1", 1)
    assert _parse_enters_with_pt_counters_line("This creature enters with 12 -1/-1 counters on it.") == ("-1/-1", 12)
    assert _parse_enters_with_pt_counters_line("This creature enters with two -1/-1 counters on it if you cast it from your hand.") is None
    assert _parse_effect("remove a -1/-1 counter from this creature") == RemoveCountersFromSource(kind="-1/-1", amount=1)
    assert _parse_effect("remove two +1/+1 counters from this creature") == RemoveCountersFromSource(kind="+1/+1", amount=2)
