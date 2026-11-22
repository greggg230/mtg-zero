"""The ``legacy`` pool (the Legacy Izzet Delver 75's cards outside the cube:
Murktide Regent, Prismari Charm) in the parser and the card IR, and the
shared-schema export (``mtgz_ir cards export-ir --pool cube legacy``) that
mtg-zero's ``card_ir`` crate reads."""

# mtg-zero (docs/ENGINE.md 170): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_the_python_engine_refuses_to_play_murktide

from __future__ import annotations

import json

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_creatures import parse_legacy_creature_line
from mtgz_ir.cards.pools import load_pool_cards, pool_card_names
from mtgz_ir.export import export_ir
from mtgz_ir.effects.base import TriggeredAbility
from mtgz_ir.effects.generic.counters import PutPlusOneCounterOnSelf
from mtgz_ir.effects.generic.cube_walkers import HasPrintedCardType
from mtgz_ir.effects.object_predicates import ObjectOr
from mtgz_ir.effects.predicates import EVENT_SUBJECTS, And, Controlled, Event, Subject

INSTANT_OR_SORCERY = ObjectOr(HasPrintedCardType("Instant"), HasPrintedCardType("Sorcery"))


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


# The Delver 75's two maindeck cards outside the cube, then mtg-zero's Legacy
# Doomsday deck's cards (ENGINE.md 124): lane A's four (test_legacy_spells.py),
# lane B's three (test_legacy_stack.py), lane C's three
# (test_doomsday_c_cards.py); then its Legacy Sneak and Show deck's six
# (ENGINE.md 123; test_legacy_sneak.py); then the Delver 75's five sideboard
# cards (ENGINE.md 122; test_legacy_delver_sb.py); then the Legacy Hogaak Tempo
# deck's seven by lane (ENGINE.md 220.1-220.3): lane A's one (test_legacy_hogaak.py),
# lane B's two (test_legacy_hogaak_b.py), lane C's four (test_legacy_hogaak_c.py).
LEGACY_CARDS = {
    "Murktide Regent", "Prismari Charm",
    "Doomsday", "Pact of Negation", "Personal Tutor", "Flow State",
    "Misdirection", "Flusterstorm", "Cavern of Souls",
    "Edge of Autumn", "Street Wraith", "Leyline of the Void",
    "Show and Tell", "Omniscience", "Mistrise Village",
    "Brotherhood's End", "Disruptor Flute", "Magus of the Moon",
    "Consign to Memory", "Hydroblast", "Meltdown", "Price of Progress", "Rough // Tumble",
    "Hogaak, Arisen Necropolis",
    "Mockingbird", "Massacre",
    "Stitcher's Supplier", "Dark Matter Manipulator", "Moonshadow", "Dryad Arbor",
}


def test_the_pool_is_the_two_maindeck_cards():
    assert pool_card_names("legacy") == LEGACY_CARDS
    # Scryfall records, no basic lands.
    assert all(c["object"] == "card" for c in load_pool_cards("legacy"))


def test_both_cards_are_claimed(legacy):
    assert set(legacy) == LEGACY_CARDS


def test_murktide_regent(legacy):
    m = legacy["Murktide Regent"]
    assert (m.mana_cost_str, m.power, m.toughness, m.subtypes) == ("{5}{U}{U}", 3, 3, ("Dragon",))
    # Delve (CR 702.66) and flying.
    assert m.has_delve and m.keywords == frozenset({"flying"})
    # CR 607.2q, 614.1c: a +1/+1 counter for each instant and sorcery card
    # exiled to pay its cost.
    assert m.enters_with_counter_per_card_exiled_with_it == INSTANT_OR_SORCERY
    # "Whenever an instant or sorcery card leaves your graveyard, put a +1/+1
    # counter on this creature." (one trigger per card, CR 603.10a).
    assert m.triggered_abilities == (
        TriggeredAbility(
            event=Event.LEAVES_GRAVEYARD,
            predicate=And(Subject(slot="card", inner=INSTANT_OR_SORCERY), Controlled(subject="player")),
            effect=PutPlusOneCounterOnSelf(),
        ),
    )
    assert EVENT_SUBJECTS[Event.LEAVES_GRAVEYARD] == ("player", "card")


def test_prismari_charm(legacy):
    from mtgz_ir.effects import ModalChooseOne

    p = legacy["Prismari Charm"]
    assert p.mana_cost_str == "{U}{R}" and p.card_types == ("Instant",)
    eff = p.spell_effect
    assert isinstance(eff, ModalChooseOne) and not eff.or_both
    surveil_draw, damage, bounce = eff.modes
    assert surveil_draw.describe().lower().startswith("surveil 2")
    # "deals 1 damage to each of one or two targets": any target, 1..2.
    assert type(damage).__name__ == "DealDamageToEachTarget" and damage.amount == 1
    (spec,) = damage.targets
    assert (spec.count_min, spec.count_max) == (1, 2)
    # "Return target nonland permanent to its owner's hand."
    assert type(bounce).__name__ == "Bounce"


# (Murktide Regent's "Delve" line is ``legacy_hogaak``'s casting-keyword list
# since mtg-zero ENGINE.md 220.1: test_legacy_hogaak.py.)
@pytest.mark.parametrize("line", [
    "This creature enters with a +1/+1 counter on it for each instant and sorcery card exiled with it.",
    "Murktide Regent enters with a +1/+1 counter on it for each instant and sorcery card exiled with it.",
    "Whenever an instant or sorcery card leaves your graveyard, put a +1/+1 counter on this creature.",
])
def test_the_recognizers(line):
    assert parse_legacy_creature_line(line, "Murktide Regent") is not None


@pytest.mark.parametrize("line", [
    "Delve, flying",
    "This creature enters with a +1/+1 counter on it for each instant and instant card exiled with it.",
    "This creature enters with a +1/+1 counter on it for each spell card exiled with it.",
    "Whenever a card leaves your graveyard during your turn, draw a card.",
    "Whenever an instant or sorcery card leaves your graveyard, draw a card.",
])
def test_lines_it_does_not_claim(line):
    assert parse_legacy_creature_line(line, "Murktide Regent") is None


def test_shared_schema_export(tmp_path):
    summary = export_ir(["cube", "legacy"], tmp_path)
    cube = json.loads((tmp_path / "powerlsv.ir.json").read_text(encoding="utf-8"))
    leg = json.loads((tmp_path / "legacy.ir.json").read_text(encoding="utf-8"))
    assert cube["schema_sha256"] == leg["schema_sha256"] == summary["schema_sha256"]
    assert not (tmp_path / "legacy.schema.json").exists()
    both = [pr[n] for pr in (ir.pool_printings("cube"), ir.pool_printings("legacy")) for n in sorted(pr)]
    schema = ir.build_schema(both)
    assert schema.sha256() == cube["schema_sha256"]
    written = json.loads((tmp_path / "powerlsv.schema.json").read_text(encoding="utf-8"))
    assert written == json.loads(json.dumps(schema.to_json()))
    # Each document decodes with the one schema; the cube's cards are those
    # of the cube-only export (only the schema hash differs).
    assert set(ir.decode_document(leg, schema)) == LEGACY_CARDS
    alone, _ = ir.build_document("cube")
    assert cube["cards"] == alone["cards"]
    assert [d["pool"] for d in summary["documents"]] == ["cube", "legacy"]
    with pytest.raises(ValueError):
        export_ir(["cube", "cube"], tmp_path)
    with pytest.raises(ValueError):
        export_ir(["cube", "legacy"], tmp_path, name="x")


