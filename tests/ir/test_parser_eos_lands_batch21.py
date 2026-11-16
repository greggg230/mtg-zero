"""Parity tests: parser vs static registry for 7 EoS (Stellar Sights) lands.

Each test verifies that ``OracleParserCardSource`` produces a
``CardPrinting`` identical to the one in the static registry
(``_PRINTINGS``).

The seven lands covered here:

- Cathedral of War   — enters-tapped + exalted keyword (generic grammar)
- Mutavault          — manland (all creature types, colorless)
- Blinkmoth Nexus    — manland + explicit tap-for-C + Blinkmoth pump
- Inkmoth Nexus      — manland with flying + infect
- Grove of the Burnwillows — dual mana ability (C or R/G with life rider)
- Swarmyard          — {T}: Regenerate target typed creatures
                       (``_try_build_tap_regenerate_land``)
- Contested War Zone — combat-damage trigger + attacking-pump activation
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import _build_land_printing
from mtgz_ir.cards.parser_source import OracleParserCardSource

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


# ---------------------------------------------------------------------------
# Shared oracle data matching Scryfall's stellar_sights.json exactly.
# ---------------------------------------------------------------------------

def _cathedral_of_war_card() -> dict:
    return {
        "name": "Cathedral of War",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "Exalted (Whenever a creature you control attacks alone, "
            "that creature gets +1/+1 until end of turn.)\n"
            "{T}: Add {C}."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _mutavault_card() -> dict:
    return {
        "name": "Mutavault",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{1}: This land becomes a 2/2 creature with all creature "
            "types until end of turn. It's still a land."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _blinkmoth_nexus_card() -> dict:
    return {
        "name": "Blinkmoth Nexus",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{1}: This land becomes a 1/1 Blinkmoth artifact creature "
            "with flying until end of turn. It's still a land.\n"
            "{1}, {T}: Target Blinkmoth creature gets +1/+1 until end "
            "of turn."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _inkmoth_nexus_card() -> dict:
    return {
        "name": "Inkmoth Nexus",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{1}: This land becomes a 1/1 Phyrexian Blinkmoth artifact "
            "creature with flying and infect until end of turn. It's "
            "still a land. (It deals damage to creatures in the form of "
            "-1/-1 counters and to players in the form of poison counters.)"
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _grove_of_the_burnwillows_card() -> dict:
    return {
        "name": "Grove of the Burnwillows",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {R} or {G}. Each opponent gains 1 life."
        ),
        "produced_mana": ["C", "G", "R"],
        "set": "eos",
    }


def _swarmyard_card() -> dict:
    return {
        "name": "Swarmyard",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Regenerate target Insect, Rat, Spider, or Squirrel. "
            "(The next time it would be destroyed this turn, instead tap "
            "it, remove it from combat, and heal all damage on it.)"
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _contested_war_zone_card() -> dict:
    return {
        "name": "Contested War Zone",
        "type_line": "Land",
        "oracle_text": (
            "Whenever a creature deals combat damage to you, that "
            "creature's controller gains control of this land.\n"
            "{T}: Add {C}.\n"
            "{1}, {T}: Attacking creatures get +1/+0 until end of turn."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


# ---------------------------------------------------------------------------
# Per-land parity tests.
# ---------------------------------------------------------------------------

def test_cathedral_of_war_parity():
    """enters_tapped=True + exalted keyword — no activated abilities."""
    parsed = _build_land_printing(_cathedral_of_war_card())
    assert parsed is not None, "Cathedral of War was not claimed"
    assert parsed == _PRINTINGS["Cathedral of War"]


def test_mutavault_parity():
    """{1}: becomes 2/2 with all creature types UEOT."""
    parsed = _build_land_printing(_mutavault_card())
    assert parsed is not None, "Mutavault was not claimed"
    assert parsed == _PRINTINGS["Mutavault"]


def test_blinkmoth_nexus_parity():
    """Three abilities: animation, explicit tap-for-C, Blinkmoth pump."""
    parsed = _build_land_printing(_blinkmoth_nexus_card())
    assert parsed is not None, "Blinkmoth Nexus was not claimed"
    assert parsed == _PRINTINGS["Blinkmoth Nexus"]


def test_inkmoth_nexus_parity():
    """Animation with flying + infect keywords."""
    parsed = _build_land_printing(_inkmoth_nexus_card())
    assert parsed is not None, "Inkmoth Nexus was not claimed"
    assert parsed == _PRINTINGS["Inkmoth Nexus"]


def test_grove_of_the_burnwillows_parity():
    """Dual mana: colorless tap + R/G tap with opponent life-gain rider."""
    parsed = _build_land_printing(_grove_of_the_burnwillows_card())
    assert parsed is not None, "Grove of the Burnwillows was not claimed"
    assert parsed == _PRINTINGS["Grove of the Burnwillows"]


def test_swarmyard_parity():
    """{T}: Regenerate typed creature (Insect/Rat/Spider/Squirrel)."""
    parsed = _build_land_printing(_swarmyard_card())
    assert parsed is not None, "Swarmyard was not claimed"
    assert parsed == _PRINTINGS["Swarmyard"]


def test_contested_war_zone_parity():
    """Combat-damage trigger + attacking-pump activation."""
    parsed = _build_land_printing(_contested_war_zone_card())
    assert parsed is not None, "Contested War Zone was not claimed"
    assert parsed == _PRINTINGS["Contested War Zone"]


# ---------------------------------------------------------------------------
# OracleParserCardSource integration: all 7 cards appear in get_printings()
# and match the static registry.
# ---------------------------------------------------------------------------

def test_all_seven_claimed_by_parser_source():
    """All 7 bespoke eos lands appear in OracleParserCardSource.get_printings()."""
    names = {
        "Cathedral of War",
        "Mutavault",
        "Blinkmoth Nexus",
        "Inkmoth Nexus",
        "Grove of the Burnwillows",
        "Swarmyard",
        "Contested War Zone",
    }
    printings = OracleParserCardSource().get_printings()
    missing = names - set(printings)
    assert not missing, f"Parser did not claim: {missing}"


def test_all_seven_match_static_via_parser_source():
    """End-to-end: parser source printings equal static registry for all 7."""
    names = [
        "Cathedral of War",
        "Mutavault",
        "Blinkmoth Nexus",
        "Inkmoth Nexus",
        "Grove of the Burnwillows",
        "Swarmyard",
        "Contested War Zone",
    ]
    printings = OracleParserCardSource().get_printings()
    for name in names:
        assert printings[name] == _PRINTINGS[name], (
            f"Parity failure for {name!r}"
        )
