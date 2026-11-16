"""The legacy pool's Doomsday cards, lane A (mtg-zero's Legacy Doomsday deck:
Doomsday, Pact of Negation, Personal Tutor, Flow State): their printings,
the two recognizers of ``cards/parser/legacy_spells.py`` ("at the beginning
of your next upkeep, <effect>", "pay <cost>. If you don't, <effect>"), the
single-faced colour indicator, and the Python semantics of
``effects/generic/legacy_spells.PayManaOrElse`` and of
``ScheduleDelayedEffect``'s ``your_upkeep`` step."""

# mtg-zero (docs/ENGINE.md 169): 3 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_pay_mana_or_else_declined_or_unpayable_loses, test_pay_mana_or_else_pays, test_your_upkeep_waits_for_the_controllers_next_upkeep

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_spells import (
    parse_legacy_spell_effect,
    with_color_indicator,
)
from mtgz_ir.cards.parser.source import build_card_record
from mtgz_ir.effects.generic.artifact_mechanics import ScheduleDelayedEffect
from mtgz_ir.effects.generic.legacy_spells import PayManaOrElse
from mtgz_ir.effects.generic.roto_walkers import LoseTheGame
from mtgz_ir.effects.vocab import DelayedStep

LANE_A = ("Doomsday", "Pact of Negation", "Personal Tutor", "Flow State")
PACT_TRIGGER = ScheduleDelayedEffect(
    inner=PayManaOrElse(cost="{3}{U}{U}", otherwise=LoseTheGame()),
    step=DelayedStep.YOUR_UPKEEP,
)


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_the_four_cards_are_claimed(legacy):
    assert set(LANE_A) <= set(legacy)


def test_doomsday(legacy):
    from mtgz_ir.effects.generic.roto_zones import DoomsdayPile

    d = legacy["Doomsday"]
    assert (d.mana_cost_str, d.card_types) == ("{B}{B}{B}", ("Sorcery",))
    # "Search your library and graveyard for five cards and exile the rest.
    # Put the chosen cards on top of your library in any order. You lose
    # half your life, rounded up." -- the class is the whole text.
    assert d.spell_effect == DoomsdayPile(count=5)


def test_pact_of_negation_printing(legacy):
    from mtgz_ir.effects.combinators import Sequence

    p = legacy["Pact of Negation"]
    assert (p.mana_cost_str, p.card_types) == ("{0}", ("Instant",))
    # CR 204, 202.2e: the colour indicator makes it blue.
    assert p.color_indicator == frozenset({"U"})
    eff = p.spell_effect
    assert isinstance(eff, Sequence)
    counter, delayed = eff.effects
    assert type(counter).__name__ == "CounterTargetSpell" and not counter.filter_types
    assert delayed == PACT_TRIGGER


def test_personal_tutor(legacy):
    from mtgz_ir.effects.vocab import SearchDestination

    t = legacy["Personal Tutor"]
    assert t.mana_cost_str == "{U}"
    s = t.spell_effect
    assert type(s).__name__ == "SearchLibrary"
    assert s.card_filter.any_types == ("Sorcery",) and s.destination == SearchDestination.TOP and s.reveal


def test_flow_state(legacy):
    from mtgz_ir.effects.generic.roto_zones import (
        LookAtTopPutIntoHandMoreIfInstantAndSorcery,
    )

    f = legacy["Flow State"]
    assert f.mana_cost_str == "{1}{U}"
    assert f.spell_effect == LookAtTopPutIntoHandMoreIfInstantAndSorcery(count=3, take=1, take_if=2)


def test_the_upkeep_recognizer():
    assert parse_legacy_spell_effect(
        "at the beginning of your next upkeep, pay {3}{U}{U}. If you don't, you lose the game"
    ) == PACT_TRIGGER
    # Every Pact's cost (Slaughter Pact's {2}{B}, Pact of the Titan's {4}{R}).
    assert parse_legacy_spell_effect(
        "at the beginning of your next upkeep, pay {2}{B}. If you don't, you lose the game"
    ) == ScheduleDelayedEffect(
        inner=PayManaOrElse(cost="{2}{B}", otherwise=LoseTheGame()), step=DelayedStep.YOUR_UPKEEP,
    )
    assert parse_legacy_spell_effect("pay {4}{R}. If you don't, you lose the game") == PayManaOrElse(
        cost="{4}{R}", otherwise=LoseTheGame(),
    )


@pytest.mark.parametrize("phrase", [
    # A targeting inner effect is another shape.
    "at the beginning of your next upkeep, destroy target creature",
    "at the beginning of your next upkeep, frobnicate the widget",
    # {X}, hybrid and Phyrexian pips: another shape.
    "pay {X}. If you don't, you lose the game",
    "pay {U/P}. If you don't, you lose the game",
    "pay 2 life. If you don't, you lose the game",
    "pay {2}. If you don't, frobnicate the widget",
    "you lose the game",
])
def test_phrases_it_does_not_claim(phrase):
    assert parse_legacy_spell_effect(phrase) is None


def _record(**kw):
    rec = {"object": "card", "name": "X", "type_line": "Instant", "mana_cost": "{0}", "oracle_text": "Draw a card."}
    rec.update(kw)
    return rec


def test_with_color_indicator():
    base = build_card_record(_record())
    assert base.color_indicator == frozenset()
    assert with_color_indicator(_record(color_indicator=["U"]), base).color_indicator == frozenset({"U"})
    # No indicator, or a multi-faced card (its back face's is ``face_colors``):
    # unchanged.
    assert with_color_indicator(_record(), base) is base
    assert with_color_indicator(_record(color_indicator=["U"], card_faces=[{}, {}]), base) is base
    # An indicator already set stays.
    green = with_color_indicator(_record(color_indicator=["G"]), base)
    assert with_color_indicator(_record(color_indicator=["U"]), green).color_indicator == frozenset({"G"})


def test_dryad_arbor_is_green():
    # The fix is general (``build_card_record``): the roto pool's Dryad Arbor
    # gains its green indicator too.
    import json

    from mtgz_ir.cards.pools import DATA_DIR

    rec = next(c for c in json.loads((DATA_DIR / "cube_roto.json").read_text(encoding="utf-8")) if c["name"] == "Dryad Arbor")
    assert build_card_record(rec).color_indicator == frozenset({"G"})


# ---- Python semantics ---------------------------------------------------------


def _state(yes=True, lands=0):
    from _rmc_helpers import ScriptedAgent, add_lands, make_state

    state = make_state(agent0=ScriptedAgent(yes_no=[yes]))
    add_lands(state, 0, "Island", lands)
    return state






def test_describe():
    assert PACT_TRIGGER.inner.describe() == "pay {3}{U}{U}. If you don't, you lose the game"
    assert PACT_TRIGGER.describe() == (
        "pay {3}{U}{U}. If you don't, you lose the game at the beginning of your next upkeep"
    )


