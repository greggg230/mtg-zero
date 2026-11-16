"""Parity tests: parser vs static registry for 6 EoS + 6 EoE lands (batch 23).

Each test verifies that ``OracleParserCardSource`` (or ``_build_land_printing``
directly) produces a ``CardPrinting`` identical to the one in the static
registry.  Failure means the bespoke dispatch table has drifted from the
canonical factory call in ``stellar_sights.py`` or ``lands.py``.

EoS (Stellar Sights) complex lands:
- Lotus Field        — hexproof, enters tapped, ETB sac 2 lands, {T}: 3 any-one-color
- Mirrorpool         — legendary, tapped, {2}{C},{T},sac: copy instant/sorcery;
                       {4}{C},{T},sac: token copy of creature
- Plaza of Heroes    — legendary, 4 abilities (colorless, legendary-spell,
                       legendary-permanent-ability, {3},{T},exile: grant hexproof+indestr)
- Thespian's Stage   — {T}: {C}, {2},{T}: becomes copy of target land
- Echoing Deeps      — Cave, optional-copy-from-graveyard ETB
- Gemstone Caverns   — legendary, taps for any (simplification: always has luck counter)

EoE bespoke lands:
- Adagia, Windswept Bastion   — Land — Planet, W
- Evendo, Waking Haven        — Land — Planet, G
- Kavaron, Memorial World     — Land — Planet, R
- Susur Secundi, Void Altar   — Land — Planet, B
- Uthros, Titanic Godcore     — Land — Planet, U
- Command Bridge              — Land, enters tapped, ETB sac-unless-tap, {T}: any
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import _build_land_printing
from _planet_helpers import assert_planet_matches_static
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.cards.lands import PLANET_LANDS, UTILITY_LANDS

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


# ---------------------------------------------------------------------------
# Oracle data fixtures — match Scryfall's stellar_sights.json / edge_of_eternities.json
# ---------------------------------------------------------------------------

def _lotus_field_card() -> dict:
    return {
        "name": "Lotus Field",
        "type_line": "Land",
        "oracle_text": (
            "Hexproof\n"
            "This land enters tapped.\n"
            "When this land enters, sacrifice two lands.\n"
            "{T}: Add three mana of any one color."
        ),
        "produced_mana": ["B", "G", "R", "U", "W"],
        "set": "eos",
    }


def _mirrorpool_card() -> dict:
    return {
        "name": "Mirrorpool",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {C}.\n"
            "{2}{C}, {T}, Sacrifice this land: Copy target instant or sorcery "
            "spell you control. You may choose new targets for the copy.\n"
            "{4}{C}, {T}, Sacrifice this land: Create a token that's a copy "
            "of target creature you control."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _plaza_of_heroes_card() -> dict:
    return {
        "name": "Plaza of Heroes",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add one mana of any color. Spend this mana only to cast a "
            "legendary spell.\n"
            "{T}: Add one mana of any color among legendary permanents you "
            "control.\n"
            "{3}, {T}, Exile this land: Target legendary creature gains "
            "hexproof and indestructible until end of turn."
        ),
        "produced_mana": ["B", "C", "G", "R", "U", "W"],
        "set": "eos",
    }


def _thespians_stage_card() -> dict:
    return {
        "name": "Thespian's Stage",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{2}, {T}: This land becomes a copy of target land, except it "
            "has this ability."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _echoing_deeps_card() -> dict:
    return {
        "name": "Echoing Deeps",
        "type_line": "Land — Cave",
        "oracle_text": (
            "You may have this land enter tapped as a copy of any land card "
            "in a graveyard, except it's a Cave in addition to its other "
            "types.\n"
            "{T}: Add {C}."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }


def _gemstone_caverns_card() -> dict:
    return {
        "name": "Gemstone Caverns",
        "type_line": "Legendary Land",
        "oracle_text": (
            "If this card is in your opening hand and you're not the starting "
            "player, you may begin the game with Gemstone Caverns on the "
            "battlefield with a luck counter on it. If you do, exile a card "
            "from your hand.\n"
            "{T}: Add {C}. If Gemstone Caverns has a luck counter on it, "
            "instead add one mana of any color."
        ),
        "produced_mana": ["B", "C", "G", "R", "U", "W"],
        "set": "eos",
    }


def _adagia_card() -> dict:
    return {
        "name": "Adagia, Windswept Bastion",
        "type_line": "Land — Planet",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W}.\n"
            "Station (Tap another creature you control: Put charge counters "
            "equal to its power on this Planet. Station only as a sorcery.)\n"
            "12+ | {3}{W}, {T}: Create a token that's a copy of target "
            "artifact or enchantment you control, except it's legendary. "
            "Activate only as a sorcery."
        ),
        "produced_mana": ["W"],
        "set": "eoe",
    }


def _evendo_card() -> dict:
    return {
        "name": "Evendo, Waking Haven",
        "type_line": "Land — Planet",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {G}.\n"
            "Station (Tap another creature you control: Put charge counters "
            "equal to its power on this Planet. Station only as a sorcery.)\n"
            "12+ | {G}, {T}: Add {G} for each creature you control."
        ),
        "produced_mana": ["G"],
        "set": "eoe",
    }


def _kavaron_card() -> dict:
    return {
        "name": "Kavaron, Memorial World",
        "type_line": "Land — Planet",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {R}.\n"
            "Station (Tap another creature you control: Put charge counters "
            "equal to its power on this Planet. Station only as a sorcery.)\n"
            "12+ | {1}{R}, {T}, Sacrifice a land: Create a 2/2 colorless "
            "Robot artifact creature token, then creatures you control get "
            "+1/+0 and gain haste until end of turn."
        ),
        "produced_mana": ["R"],
        "set": "eoe",
    }


def _susur_card() -> dict:
    return {
        "name": "Susur Secundi, Void Altar",
        "type_line": "Land — Planet",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {B}.\n"
            "Station (Tap another creature you control: Put charge counters "
            "equal to its power on this Planet. Station only as a sorcery.)\n"
            "12+ | {1}{B}, {T}, Pay 2 life, Sacrifice a creature: Draw cards "
            "equal to the sacrificed creature's power. Activate only as a "
            "sorcery."
        ),
        "produced_mana": ["B"],
        "set": "eoe",
    }


def _uthros_card() -> dict:
    return {
        "name": "Uthros, Titanic Godcore",
        "type_line": "Land — Planet",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {U}.\n"
            "Station (Tap another creature you control: Put charge counters "
            "equal to its power on this Planet. Station only as a sorcery.)\n"
            "12+ | {U}, {T}: Add {U} for each artifact you control."
        ),
        "produced_mana": ["U"],
        "set": "eoe",
    }


def _command_bridge_card() -> dict:
    return {
        "name": "Command Bridge",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "When this land enters, sacrifice it unless you tap an untapped "
            "permanent you control.\n"
            "{T}: Add one mana of any color."
        ),
        "produced_mana": ["B", "G", "R", "U", "W"],
        "set": "eoe",
    }


# ---------------------------------------------------------------------------
# Per-land parity tests — EoS
# ---------------------------------------------------------------------------

def test_lotus_field_parity():
    """Hexproof keyword + enters tapped + ETB sac-2-lands trigger + 3-any-color tap."""
    parsed = _build_land_printing(_lotus_field_card())
    assert parsed is not None, "Lotus Field was not claimed"
    assert parsed == _PRINTINGS["Lotus Field"]


def test_mirrorpool_parity():
    """Legendary, tapped, two sac-self activations (copy spell + token copy)."""
    parsed = _build_land_printing(_mirrorpool_card())
    assert parsed is not None, "Mirrorpool was not claimed"
    assert parsed == _PRINTINGS["Mirrorpool"]


def test_plaza_of_heroes_parity():
    """Legendary, 4 abilities: {C}, legend-spell any, legend-perm any, pump+grant."""
    parsed = _build_land_printing(_plaza_of_heroes_card())
    assert parsed is not None, "Plaza of Heroes was not claimed"
    assert parsed == _PRINTINGS["Plaza of Heroes"]


def test_thespians_stage_parity():
    """{T}: {C} + {2},{T}: copy target land (retains own ability)."""
    parsed = _build_land_printing(_thespians_stage_card())
    assert parsed is not None, "Thespian's Stage was not claimed"
    assert parsed == _PRINTINGS["Thespian's Stage"]


def test_echoing_deeps_parity():
    """Cave subtype + optional_copy_land_from_graveyard ETB."""
    parsed = _build_land_printing(_echoing_deeps_card())
    assert parsed is not None, "Echoing Deeps was not claimed"
    assert parsed == _PRINTINGS["Echoing Deeps"]


def test_gemstone_caverns_parity():
    """Legendary, taps for any (always-luck-counter simplification)."""
    parsed = _build_land_printing(_gemstone_caverns_card())
    assert parsed is not None, "Gemstone Caverns was not claimed"
    assert parsed == _PRINTINGS["Gemstone Caverns"]


# ---------------------------------------------------------------------------
# Per-land parity tests — EoE Planet lands
# ---------------------------------------------------------------------------

def test_adagia_parity():
    """Planet land — Land — Planet, W, enters tapped; Station 12 (CR 702.184)."""
    parsed = _build_land_printing(_adagia_card())
    assert parsed is not None, "Adagia, Windswept Bastion was not claimed"
    assert_planet_matches_static(parsed, PLANET_LANDS["Adagia, Windswept Bastion"])


def test_evendo_parity():
    """Planet land — Land — Planet, G, enters tapped; Station 12 (CR 702.184)."""
    parsed = _build_land_printing(_evendo_card())
    assert parsed is not None, "Evendo, Waking Haven was not claimed"
    assert_planet_matches_static(parsed, PLANET_LANDS["Evendo, Waking Haven"])


def test_kavaron_parity():
    """Planet land — Land — Planet, R, enters tapped; Station 12 (CR 702.184)."""
    parsed = _build_land_printing(_kavaron_card())
    assert parsed is not None, "Kavaron, Memorial World was not claimed"
    assert_planet_matches_static(parsed, PLANET_LANDS["Kavaron, Memorial World"])


def test_susur_parity():
    """Planet land — Land — Planet, B, enters tapped; Station 12 (CR 702.184)."""
    parsed = _build_land_printing(_susur_card())
    assert parsed is not None, "Susur Secundi, Void Altar was not claimed"
    assert_planet_matches_static(parsed, PLANET_LANDS["Susur Secundi, Void Altar"])


def test_uthros_parity():
    """Planet land — Land — Planet, U, enters tapped; Station 12 (CR 702.184)."""
    parsed = _build_land_printing(_uthros_card())
    assert parsed is not None, "Uthros, Titanic Godcore was not claimed"
    assert_planet_matches_static(parsed, PLANET_LANDS["Uthros, Titanic Godcore"])


# ---------------------------------------------------------------------------
# Per-land parity test — EoE Command Bridge
# ---------------------------------------------------------------------------

def test_command_bridge_parity():
    """Enters tapped, ETB sac-unless-tap-permanent, {T}: any mana."""
    parsed = _build_land_printing(_command_bridge_card())
    assert parsed is not None, "Command Bridge was not claimed"
    assert parsed == UTILITY_LANDS["Command Bridge"]


# ---------------------------------------------------------------------------
# OracleParserCardSource integration: all 12 new cards appear
# ---------------------------------------------------------------------------

_ALL_NAMES = {
    "Lotus Field",
    "Mirrorpool",
    "Plaza of Heroes",
    "Thespian's Stage",
    "Echoing Deeps",
    "Gemstone Caverns",
    "Adagia, Windswept Bastion",
    "Evendo, Waking Haven",
    "Kavaron, Memorial World",
    "Susur Secundi, Void Altar",
    "Uthros, Titanic Godcore",
    "Command Bridge",
}


def test_all_twelve_claimed_by_parser_source():
    """All 12 batch-23 lands appear in OracleParserCardSource.get_printings()."""
    printings = OracleParserCardSource().get_printings()
    missing = _ALL_NAMES - set(printings)
    assert not missing, f"Parser did not claim: {missing}"


def test_eos_six_match_static_via_parser_source():
    """End-to-end: parser source printings equal _PRINTINGS for 6 EoS."""
    eos_names = [
        "Lotus Field",
        "Mirrorpool",
        "Plaza of Heroes",
        "Thespian's Stage",
        "Echoing Deeps",
        "Gemstone Caverns",
    ]
    printings = OracleParserCardSource().get_printings()
    for name in eos_names:
        assert printings[name] == _PRINTINGS[name], (
            f"Parity failure for {name!r}"
        )


def test_eoe_planets_match_static_via_parser_source():
    """End-to-end: parser source printings equal PLANET_LANDS for 5 EoE planets."""
    planet_names = [
        "Adagia, Windswept Bastion",
        "Evendo, Waking Haven",
        "Kavaron, Memorial World",
        "Susur Secundi, Void Altar",
        "Uthros, Titanic Godcore",
    ]
    printings = OracleParserCardSource().get_printings()
    for name in planet_names:
        assert_planet_matches_static(printings[name], PLANET_LANDS[name])


def test_command_bridge_matches_static_via_parser_source():
    """End-to-end: parser source printing equals UTILITY_LANDS for Command Bridge."""
    printings = OracleParserCardSource().get_printings()
    assert printings["Command Bridge"] == UTILITY_LANDS["Command Bridge"], (
        "Parity failure for 'Command Bridge'"
    )
