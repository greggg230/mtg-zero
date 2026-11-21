"""The legacy pool's Hogaak, Arisen Necropolis (mtg-zero's Legacy Hogaak Tempo
deck, MTGGoldfish 8027954, lane A: docs/ENGINE.md 220.1): its printing and the
recognizers of ``cards/parser/legacy_hogaak.py`` (the casting-keyword list,
"You can't spend mana to cast this spell", "You may cast this card from your
graveyard")."""

from __future__ import annotations

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.parser.legacy_hogaak import parse_legacy_hogaak_line

HOGAAK = "Hogaak, Arisen Necropolis"


@pytest.fixture(scope="module")
def legacy():
    return ir.pool_printings("legacy")


def test_hogaak_is_claimed_with_every_line(legacy):
    h = legacy[HOGAAK]
    assert (h.mana_cost_str, h.card_types, h.subtypes, h.supertypes) == (
        "{5}{B/G}{B/G}", ("Creature",), ("Avatar",), ("Legendary",),
    )
    assert (h.power, h.toughness) == (8, 8)
    assert h.mana_cost.generic == 5 and h.mana_cost.hybrid == (("B", "G"), ("B", "G"))
    # "You can't spend mana to cast this spell."
    assert h.cant_spend_mana_to_cast
    # "Convoke, delve"
    assert h.has_convoke and h.has_delve
    # "You may cast this card from your graveyard."
    assert h.castable_from_graveyard and not h.warp_castable_from_graveyard
    # "Trample"
    assert h.keywords == frozenset({"trample"})
    # Nothing else: no ability, no other casting rule.
    assert not (h.triggered_abilities or h.activated_abilities or h.static_abilities)
    assert h.warp_cost is None and h.flashback_cost is None and h.escape_cost is None


def test_murktide_regent_still_claims_its_delve_through_the_list(legacy):
    m = legacy["Murktide Regent"]
    assert m.has_delve and not m.has_convoke and not m.cant_spend_mana_to_cast


@pytest.mark.parametrize("line,fields", [
    ("Convoke, delve", {"has_convoke": True, "has_delve": True}),
    ("Delve, convoke", {"has_convoke": True, "has_delve": True}),
    ("Delve", {"has_delve": True}),
    ("Convoke", {"has_convoke": True}),
    ("You can't spend mana to cast this spell.", {"cant_spend_mana_to_cast": True}),
    ("You may cast this card from your graveyard.", {"castable_from_graveyard": True}),
])
def test_the_recognizers(line, fields):
    out = parse_legacy_hogaak_line(line, HOGAAK)
    assert out is not None and out.fields == fields
    assert not (out.triggers or out.activated or out.statics or out.keywords)


@pytest.mark.parametrize("line", [
    "Convoke, convoke",          # a repeated keyword
    "convoke, delve",            # a keyword line starts capitalized
    "Convoke, flying",           # not a casting keyword
    "Convoke, Delve",            # only the first word is capitalized
    "Delve, convoke, trample",
    "You may cast this card from your graveyard using its warp ability.",
    "You may cast this card from your graveyard if a non-Zombie creature died this turn.",
    "You can't spend mana to cast spells.",
])
def test_other_lines_are_not_claimed(line):
    assert parse_legacy_hogaak_line(line, HOGAAK) is None


def test_the_ir_roundtrips(legacy):
    schema = ir.build_schema([legacy[n] for n in sorted(legacy)])
    enc = ir.Encoder(schema)
    p = legacy[HOGAAK]
    data = enc.encode_printing(p)
    assert data["cant_spend_mana_to_cast"] is True and data["castable_from_graveyard"] is True
    assert data["has_convoke"] is True and data["has_delve"] is True
