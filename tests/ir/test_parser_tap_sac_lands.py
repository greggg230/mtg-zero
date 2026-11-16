"""Parser support for tap-and-sac utility lands.

Recognizes plain colorless utility lands whose oracle text is
``{T}: Add {C}.`` plus a single tap-and-sac activated ability whose
cost shape and body match a small per-shape whitelist:

- ``{T}, Sacrifice this land: <BODY>`` — Strip Mine, Petrified Field
- ``<MANA>, {T}, Sacrifice a land: <BODY>`` — Dust Bowl
- ``{T}, Sacrifice a creature: <BODY>`` — High Market
- ``<MANA>, {T}, Sacrifice a Desert: <BODY>`` — Scavenger Grounds
  (requires ``Land — Desert`` type line)

Bodies outside the whitelist fall through to the silent-skip path.
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_tap_sac_utility_land,
)

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


def _strip_mine_card() -> dict:
    return {
        "name": "Strip Mine",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Destroy target land."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def test_strip_mine_matches_static():
    parsed = _build_land_printing(_strip_mine_card())
    assert parsed is not None
    assert parsed == _PRINTINGS["Strip Mine"]


def test_petrified_field_matches_static():
    card = {
        "name": "Petrified Field",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Return target land card from "
            "your graveyard to your hand."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Petrified Field"]


def test_dust_bowl_matches_static():
    card = {
        "name": "Dust Bowl",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{3}, {T}, Sacrifice a land: Destroy target nonbasic land."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Dust Bowl"]


def test_high_market_matches_static():
    card = {
        "name": "High Market",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice a creature: You gain 1 life."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["High Market"]


def test_scavenger_grounds_matches_static():
    card = {
        "name": "Scavenger Grounds",
        "type_line": "Land — Desert",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{2}, {T}, Sacrifice a Desert: Exile all graveyards."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Scavenger Grounds"]


def test_skips_unknown_sac_self_body():
    """A sac-self body outside the whitelist must NOT be claimed —
    parity would fail because the parser would build the wrong effect."""
    card = {
        "name": "Hypothetical Strange Mine",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Untap target permanent."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_tap_sac_utility_land(card, card["oracle_text"], (), ()) is None


def test_skips_when_first_line_is_not_tap_for_c():
    """A two-line shape whose first line isn't ``{T}: Add {C}.`` falls
    through. Lands like Mana Confluence (no tap-for-C base ability)
    aren't this shape."""
    card = {
        "name": "Hypothetical Variant",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add one mana of any color.\n"
            "{T}, Sacrifice this land: Destroy target land."
        ),
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    assert _try_build_tap_sac_utility_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_skips_supertyped_land():
    """Legendary or other-supertyped utility lands fall outside this
    shape — they need the bespoke per-card path."""
    card = {
        "name": "Hypothetical Legendary",
        "type_line": "Legendary Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Destroy target land."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_tap_sac_utility_land(
        card, card["oracle_text"], ("Legendary",), (),
    ) is None


def test_skips_subtyped_land_for_sac_self_path():
    """A subtyped land can't claim the no-cost sac-self path; only
    the Desert sac-Desert path accepts subtypes (and only ``Desert``).
    """
    card = {
        "name": "Hypothetical Cave Strip",
        "type_line": "Land — Cave",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Destroy target land."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_tap_sac_utility_land(
        card, card["oracle_text"], (), ("Cave",),
    ) is None


def test_skips_three_line_shape():
    """The recognizer is for the strict two-line shape. A third
    oracle line falls through (those need the bespoke per-card path)."""
    card = {
        "name": "Hypothetical Extra Line",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Destroy target land.\n"
            "Indestructible"
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_tap_sac_utility_land(
        card, card["oracle_text"], (), (),
    ) is None
