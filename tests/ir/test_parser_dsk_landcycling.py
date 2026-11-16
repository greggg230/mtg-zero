"""Tests for the DSK basic-landcycling printing-level keyword.

The DSK landcycling cycle (Plains/Island/Swamp/Mountain/Forest, all
``{2}`` cost) prints on five large creatures — Shepherding Spirits W,
Daggermaw Megalodon U, Spectral Snatcher B, Bedhead Beastie R,
Slavering Branchsnapper G. Each creature is otherwise a vanilla
keyword body (Flying / Vigilance / Ward—Discard / Menace / Trample)
plus the matching basic-landcycling line. This batch lands the
parser/printing plumbing:

- ``CardPrinting.landcycling: tuple[str, str] | None`` — declarative
  ``(land_subtype, mana_cost_str)`` field. Today this is *purely*
  declarative metadata: the engine does not yet enumerate hand-
  activated abilities, so the cycling cost cannot actually be paid in
  simulation. The field is populated honestly so the printing reflects
  oracle text, and a future activation pipeline (``legal_actions``
  enumeration of from-hand activations + an ``ActivationCost.discard_
  self`` cost atom) consumes the same field without having to re-walk
  the oracle text.
- ``_parse_landcycling_line(line)`` — printing-keywords recognizer
  that matches ``<Land>cycling {cost}`` for each of the five basic-
  land subtypes. Returns ``(subtype, cost_str)`` or ``None``.
- Creature-builder wiring: a recognized landcycling line populates
  the ``landcycling`` kwarg on ``make_creature`` and the line is
  consumed (continued past) so the rest of the clause grammar
  doesn't see it.

The five DSK creatures listed above each carry exactly one
non-landcycling line that already parses through the existing
keyword grammar (Flying / Vigilance / Trample / Menace) or the
em-dash ward-discard recognizer (Ward—Discard a card.) — adding the
landcycling recognizer is the only piece needed for end-to-end
parser claims on this batch.
"""

from __future__ import annotations

from mtgz_ir.cards.parser.printing_keywords import _parse_landcycling_line
from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- ``_parse_landcycling_line`` recognizer ----------


def test_parse_landcycling_line_plains():
    # Canonical DSK Plainscycling printing — Shepherding Spirits.
    # The mana-cost capture preserves the printed ``{2}`` verbatim;
    # the subtype capture's casing matches Scryfall's ``Plains``.
    assert _parse_landcycling_line("Plainscycling {2}") == ("Plains", "{2}")


def test_parse_landcycling_line_island():
    # Daggermaw Megalodon's ``Islandcycling {2}`` line.
    assert _parse_landcycling_line("Islandcycling {2}") == ("Island", "{2}")


def test_parse_landcycling_line_swamp():
    # Spectral Snatcher's ``Swampcycling {2}`` line.
    assert _parse_landcycling_line("Swampcycling {2}") == ("Swamp", "{2}")


def test_parse_landcycling_line_mountain():
    # Bedhead Beastie's ``Mountaincycling {2}`` line.
    assert _parse_landcycling_line("Mountaincycling {2}") == (
        "Mountain", "{2}",
    )


def test_parse_landcycling_line_forest():
    # Slavering Branchsnapper's ``Forestcycling {2}`` line.
    assert _parse_landcycling_line("Forestcycling {2}") == ("Forest", "{2}")


def test_parse_landcycling_line_hybrid_cost():
    # Forward-looking grammar coverage. The mana-cost capture is the
    # generic brace-wrapped-symbols family used by Warp/Ward; a
    # hypothetical hybrid-cost printing (no DSK card today) parses
    # without further regex work. The subtype capture stays bound to
    # the five basic-land subtypes by name — typecycling on a non-
    # basic subtype isn't accepted (separate test below).
    assert _parse_landcycling_line("Plainscycling {1}{R}") == (
        "Plains", "{1}{R}",
    )


def test_parse_landcycling_line_rejects_nonbasic_subtype():
    # ``Spiritcycling`` and other non-basic typecycling phrases would
    # need their own engine field (the ``land_subtype`` half of the
    # tuple is meaningful only when paired with a basic-land
    # designation in this engine). The recognizer rejects rather than
    # silently mis-modeling the printing.
    assert _parse_landcycling_line("Spiritcycling {2}") is None


def test_parse_landcycling_line_rejects_bare_cycling():
    # ``Cycling {N}`` (the generic, non-typecycling shape) is a
    # different mechanic — discard for a generic draw rather than a
    # basic-land tutor — and its engine semantics don't fit the
    # ``(subtype, cost)`` tuple. The recognizer rejects it for the
    # same don't-silently-mis-model discipline as the non-basic-
    # subtype rejection above.
    assert _parse_landcycling_line("Cycling {2}") is None


def test_parse_landcycling_line_rejects_unrelated_lines():
    # Belt-and-suspenders that the recognizer doesn't accidentally
    # claim a line whose surface form happens to mention a basic-
    # land subtype.
    assert _parse_landcycling_line("Flying") is None
    assert _parse_landcycling_line("This creature gets +1/+1.") is None
    assert _parse_landcycling_line("") is None


def test_parse_landcycling_line_strips_surrounding_whitespace():
    # The recognizer applies ``.strip()`` to its input, mirroring
    # every other printing-keyword recognizer (warp / ward / devour
    # / enters-tapped / must-be-blocked) so callers don't have to
    # pre-strip.
    assert _parse_landcycling_line("  Forestcycling {2}  ") == (
        "Forest", "{2}",
    )


# ---------- ``CardPrinting.landcycling`` field semantics ----------


def test_card_printing_landcycling_default_none():
    # Every printing built without the kwarg has ``landcycling = None``
    # — the field has no engine consumer today, and ``None`` is the
    # "no landcycling" sentinel.
    from mtgz_ir.cards.base import make_creature
    p = make_creature("Test Creature", "{1}{G}", 2, 2)
    assert p.landcycling is None


def test_card_printing_landcycling_field_round_trips_kwarg():
    # The make_creature kwarg flows through to the printing's
    # ``landcycling`` attribute as a tuple. Mirrors the
    # ``warp_castable_from_graveyard`` / ``unblockable_if_two_spells_
    # cast_this_turn`` precedent — declarative metadata, no engine
    # consumer in this batch.
    from mtgz_ir.cards.base import make_creature
    p = make_creature(
        "Test Creature", "{4}{G}{G}", 7, 6,
        keywords={"trample"},
        landcycling=("Forest", "{2}"),
    )
    assert p.landcycling == ("Forest", "{2}")


# ---------- end-to-end parser claims ----------


def test_shepherding_spirits_claimed_by_parser():
    # Shepherding Spirits {4}{W}{W} 4/5 Spirit. Two oracle lines:
    # ``Flying`` and ``Plainscycling {2}``. Claims via the existing
    # keyword-line grammar plus the new landcycling recognizer.
    assert "Shepherding Spirits" in _claimed_names()


def test_shepherding_spirits_carries_flying_and_plainscycling():
    # Belt-and-suspenders that the claimed printing matches the
    # printed oracle text: keyword set ``{flying}`` and
    # ``landcycling=("Plains", "{2}")``. Mana cost / P/T are
    # round-tripped from Scryfall data so the field-level checks
    # below pin both the parsed-cost path and the declarative
    # metadata path.
    src = OracleParserCardSource()
    p = src.get_printings()["Shepherding Spirits"]
    assert p.power == 4
    assert p.toughness == 5
    assert p.subtypes == ("Spirit",)
    assert p.keywords == frozenset({"flying"})
    assert p.landcycling == ("Plains", "{2}")
    assert p.mana_cost_str == "{4}{W}{W}"


def test_daggermaw_megalodon_claimed_by_parser():
    # Daggermaw Megalodon {4}{U}{U} 5/7 Shark. Two oracle lines:
    # ``Vigilance`` and ``Islandcycling {2}``.
    assert "Daggermaw Megalodon" in _claimed_names()


def test_daggermaw_megalodon_carries_vigilance_and_islandcycling():
    src = OracleParserCardSource()
    p = src.get_printings()["Daggermaw Megalodon"]
    assert p.power == 5
    assert p.toughness == 7
    assert p.subtypes == ("Shark",)
    assert p.keywords == frozenset({"vigilance"})
    assert p.landcycling == ("Island", "{2}")


def test_spectral_snatcher_claimed_by_parser():
    # Spectral Snatcher {4}{B}{B} 6/5 Spirit. Two oracle lines:
    # ``Ward—Discard a card.`` (em-dash discard ward) and
    # ``Swampcycling {2}``. Ward—Discard claims via the existing
    # ``_parse_ward_discard_line`` recognizer; Swampcycling claims
    # via the new ``_parse_landcycling_line`` recognizer.
    assert "Spectral Snatcher" in _claimed_names()


def test_spectral_snatcher_carries_ward_discard_and_swampcycling():
    # The claimed printing must carry BOTH the ``ward_discard_chosen``
    # flag (from the em-dash ward line) AND the
    # ``landcycling=("Swamp", "{2}")`` field. Spectral Snatcher is the
    # only DSK landcycling creature with a non-trivial second line,
    # so this test pins the parser correctly composing two printing-
    # level keyword recognizers on the same card.
    src = OracleParserCardSource()
    p = src.get_printings()["Spectral Snatcher"]
    assert p.ward_discard_chosen is True
    assert p.ward_cost is None  # the bare ``Ward {cost}`` form
    assert p.landcycling == ("Swamp", "{2}")
    assert p.power == 6
    assert p.toughness == 5
    assert p.subtypes == ("Spirit",)


def test_bedhead_beastie_claimed_by_parser():
    # Bedhead Beastie {4}{R}{R} 5/6 Beast. Two oracle lines:
    # ``Menace`` (with parenthetical reminder text) and
    # ``Mountaincycling {2}``.
    assert "Bedhead Beastie" in _claimed_names()


def test_bedhead_beastie_carries_menace_and_mountaincycling():
    src = OracleParserCardSource()
    p = src.get_printings()["Bedhead Beastie"]
    assert p.power == 5
    assert p.toughness == 6
    assert p.subtypes == ("Beast",)
    assert p.keywords == frozenset({"menace"})
    assert p.landcycling == ("Mountain", "{2}")


def test_slavering_branchsnapper_claimed_by_parser():
    # Slavering Branchsnapper {4}{G}{G} 7/6 Lizard. Two oracle lines:
    # ``Trample`` and ``Forestcycling {2}``.
    assert "Slavering Branchsnapper" in _claimed_names()


def test_slavering_branchsnapper_carries_trample_and_forestcycling():
    src = OracleParserCardSource()
    p = src.get_printings()["Slavering Branchsnapper"]
    assert p.power == 7
    assert p.toughness == 6
    assert p.subtypes == ("Lizard",)
    assert p.keywords == frozenset({"trample"})
    assert p.landcycling == ("Forest", "{2}")


# ---------- non-cycling printings keep ``landcycling = None`` ----------


def test_existing_printings_keep_landcycling_none():
    # Belt-and-suspenders that adding the ``landcycling`` field
    # didn't accidentally populate it on any pre-existing parser
    # claim. We sample a few known-claimed creatures from earlier
    # batches and verify ``landcycling is None`` on each.
    src = OracleParserCardSource()
    printings = src.get_printings()
    # Wildfire Wickerfolk (DSK batch 4) — Delirium static gate.
    if "Wildfire Wickerfolk" in printings:
        assert printings["Wildfire Wickerfolk"].landcycling is None
    # Enduring Curiosity (DSK Enduring batch) — Enduring rider.
    if "Enduring Curiosity" in printings:
        assert printings["Enduring Curiosity"].landcycling is None
