"""Parity tests: parser vs static registry for 6 EoS (Stellar Sights) lands.

Each test verifies that ``OracleParserCardSource`` produces a
``CardPrinting`` identical to the one in the static registry
(``_PRINTINGS``).

The six lands covered here:

- Blast Zone        — ETB charge counter + X counter placement + sac destroy
- Nesting Grounds   — counter-move sorcery-speed activation
- Endless Sands     — Desert: {2},{T} exile / {4},{T},sac return
                      (``_try_build_desert_exile_return_land``)
- Inventors' Fair   — legendary, upkeep gain-life trigger + {4},{T},sac search
                      gated on 3+ artifacts (``_try_build_legendary_artifact_search_land``)
- Power Depot       — Artifact Land, tapped, restricted-any mana ability
- Sunken Citadel    — Cave, tapped, any-color + land-activation restricted mana
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import _build_land_printing
from mtgz_ir.cards.parser_source import OracleParserCardSource

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


# ---------------------------------------------------------------------------
# Shared oracle data matching Scryfall's stellar_sights.json exactly.
# ---------------------------------------------------------------------------

def _blast_zone_card() -> dict:
    return {
        "name": "Blast Zone",
        "type_line": "Land",
        "oracle_text": (
            "This land enters with a charge counter on it.\n"
            "{T}: Add {C}.\n"
            "{X}{X}, {T}: Put X charge counters on this land.\n"
            "{3}, {T}, Sacrifice this land: Destroy each nonland permanent "
            "with mana value equal to the number of charge counters on "
            "this land."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _nesting_grounds_card() -> dict:
    return {
        "name": "Nesting Grounds",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{1}, {T}: Move a counter from target permanent you control "
            "onto a second target permanent. Activate only as a sorcery."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _endless_sands_card() -> dict:
    return {
        "name": "Endless Sands",
        "type_line": "Land — Desert",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{2}, {T}: Exile target creature you control.\n"
            "{4}, {T}, Sacrifice this land: Return each creature card "
            "exiled with this land to the battlefield under its owner's "
            "control."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _inventors_fair_card() -> dict:
    return {
        "name": "Inventors' Fair",
        "type_line": "Legendary Land",
        "oracle_text": (
            "At the beginning of your upkeep, if you control three or "
            "more artifacts, you gain 1 life.\n"
            "{T}: Add {C}.\n"
            "{4}, {T}, Sacrifice Inventors' Fair: Search your library "
            "for an artifact card, reveal it, put it into your hand, "
            "then shuffle. Activate only if you control three or more "
            "artifacts."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _power_depot_card() -> dict:
    return {
        "name": "Power Depot",
        "type_line": "Artifact Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {C}.\n"
            "{T}: Add one mana of any color. Spend this mana only to cast "
            "artifact spells or activate abilities of artifacts.\n"
            "Modular 1"
        ),
        "produced_mana": ["B", "C", "G", "R", "U", "W"],
        "set": "eos",
    }


def _sunken_citadel_card() -> dict:
    return {
        "name": "Sunken Citadel",
        "type_line": "Land — Cave",
        "oracle_text": (
            "This land enters tapped. As it enters, choose a color.\n"
            "{T}: Add one mana of the chosen color.\n"
            "{T}: Add two mana of the chosen color. Spend this mana only "
            "to activate abilities of land sources."
        ),
        "produced_mana": ["B", "G", "R", "U", "W"],
        "set": "eos",
    }


# ---------------------------------------------------------------------------
# Per-land parity tests.
# ---------------------------------------------------------------------------

def test_blast_zone_parity():
    """ETB charge counter + {X}{X},{T} counter placement + {3},{T},sac destroy."""
    parsed = _build_land_printing(_blast_zone_card())
    assert parsed is not None, "Blast Zone was not claimed"
    assert parsed == _PRINTINGS["Blast Zone"]


def test_nesting_grounds_parity():
    """{1},{T}: Move a +1/+1 counter between permanents (sorcery speed)."""
    parsed = _build_land_printing(_nesting_grounds_card())
    assert parsed is not None, "Nesting Grounds was not claimed"
    assert parsed == _PRINTINGS["Nesting Grounds"]


def test_endless_sands_parity():
    """Desert: exile-creature activation + sac-self-return activation."""
    parsed = _build_land_printing(_endless_sands_card())
    assert parsed is not None, "Endless Sands was not claimed"
    assert parsed == _PRINTINGS["Endless Sands"]


def test_inventors_fair_parity():
    """Legendary, upkeep life-gain trigger + {4},{T},sac search activation
    (gated on 3+ artifacts; enters_tapped=False per oracle text)."""
    parsed = _build_land_printing(_inventors_fair_card())
    assert parsed is not None, "Inventors' Fair was not claimed"
    assert parsed == _PRINTINGS["Inventors' Fair"]


def test_power_depot_parity():
    """Artifact Land, tapped, restricted-any mana ability (artifact_spell)."""
    parsed = _build_land_printing(_power_depot_card())
    assert parsed is not None, "Power Depot was not claimed"
    assert parsed == _PRINTINGS["Power Depot"]


def test_sunken_citadel_parity():
    """Cave, tapped, any-color mana + land-activation restricted any mana."""
    parsed = _build_land_printing(_sunken_citadel_card())
    assert parsed is not None, "Sunken Citadel was not claimed"
    assert parsed == _PRINTINGS["Sunken Citadel"]


# ---------------------------------------------------------------------------
# OracleParserCardSource integration: all 6 cards appear in get_printings()
# and match the static registry.
# ---------------------------------------------------------------------------

def test_all_six_claimed_by_parser_source():
    """All 6 bespoke eos lands appear in OracleParserCardSource.get_printings()."""
    names = {
        "Blast Zone",
        "Nesting Grounds",
        "Endless Sands",
        "Inventors' Fair",
        "Power Depot",
        "Sunken Citadel",
    }
    printings = OracleParserCardSource().get_printings()
    missing = names - set(printings)
    assert not missing, f"Parser did not claim: {missing}"


def test_all_six_match_static_via_parser_source():
    """End-to-end: parser source printings equal static registry for all 6."""
    names = [
        "Blast Zone",
        "Nesting Grounds",
        "Endless Sands",
        "Inventors' Fair",
        "Power Depot",
        "Sunken Citadel",
    ]
    printings = OracleParserCardSource().get_printings()
    for name in names:
        assert printings[name] == _PRINTINGS[name], (
            f"Parity failure for {name!r}"
        )
