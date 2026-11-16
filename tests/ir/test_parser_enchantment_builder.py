"""Parity tests for the enchantment_builder whole-card dispatch.

Each test asserts that ``OracleParserCardSource`` builds the same
``CardPrinting`` as the static registry for one of the 15 EoE
enchantments claimed by ``enchantment_builder``.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser_source import OracleParserCardSource

_PARSER = OracleParserCardSource()
_PARSER_PRINTINGS = _PARSER.get_printings()


# ---------------------------------------------------------------------------
# Helper to assert parity
# ---------------------------------------------------------------------------



# ---------------------------------------------------------------------------
# Non-Aura enchantments
# ---------------------------------------------------------------------------



















# ---------------------------------------------------------------------------
# Aura enchantments
# ---------------------------------------------------------------------------













# ---------------------------------------------------------------------------
# Gate check: type-line guard does not claim non-enchantments
# ---------------------------------------------------------------------------

def test_enchantment_builder_skips_non_enchantment():
    from mtgz_ir.cards.parser.enchantment_builder import (
        _build_enchantment_printing,
    )
    card = {
        "name": "Banishing Light",
        "mana_cost": "{2}{W}",
        "type_line": "Creature — Human",
        "oracle_text": "When this enchantment enters, ...",
    }
    assert _build_enchantment_printing(card) is None


def test_enchantment_builder_skips_enchantment_creature():
    from mtgz_ir.cards.parser.enchantment_builder import (
        _build_enchantment_printing,
    )
    card = {
        "name": "Banishing Light",
        "mana_cost": "{2}{W}",
        "type_line": "Enchantment Creature — Human",
        "oracle_text": "",
    }
    assert _build_enchantment_printing(card) is None


def test_enchantment_builder_skips_enchantment_artifact():
    from mtgz_ir.cards.parser.enchantment_builder import (
        _build_enchantment_printing,
    )
    card = {
        "name": "Banishing Light",
        "mana_cost": "{2}{W}",
        "type_line": "Artifact Enchantment",
        "oracle_text": "",
    }
    assert _build_enchantment_printing(card) is None


def test_enchantment_builder_skips_unknown_name():
    from mtgz_ir.cards.parser.enchantment_builder import (
        _build_enchantment_printing,
    )
    card = {
        "name": "Some Unknown Enchantment",
        "mana_cost": "{2}{W}",
        "type_line": "Enchantment",
        "oracle_text": "",
    }
    assert _build_enchantment_printing(card) is None


# ---------------------------------------------------------------------------
# End-to-end: parser source claims all 15 via data walk
# ---------------------------------------------------------------------------

_EXPECTED_ENCHANTMENT_NAMES = [
    "Banishing Light",
    "Seam Rip",
    "Loading Zone",
    "Weftwalking",
    "Sothera, the Supervoid",
    "Terrasymbiosis",
    "Weapons Manufacturing",
    "Bioengineered Future",
    "Eusocial Engineering",
    "Moonlit Meditation",
    "Tractor Beam",
    "Chorale of the Void",
    "Cryoshatter",
    "Meltstrider's Resolve",
    "Hardlight Containment",
]


@pytest.mark.parametrize("name", _EXPECTED_ENCHANTMENT_NAMES)
def test_source_claims_enchantment(name: str):
    assert name in _PARSER.claimed_names, (
        f"OracleParserCardSource did not claim {name!r}"
    )
