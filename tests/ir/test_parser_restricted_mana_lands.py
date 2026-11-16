"""Parser support for restricted-mana utility lands.

Plain ``Land`` cards (no supertypes, no subtypes) whose oracle text is
the strict two-line shape:

    {T}: Add {C}.
    {T}: Add <MANA>. <RESTRICTION-PHRASE>.

The first line is modeled as an explicit ``AddManaToPool`` mana
ability (the unrestricted base coin); the second line's restriction
phrase maps via ``_RESTRICTED_MANA_PHRASES`` to a
``spend_restriction`` tag the engine consults during cost-pay.

Currently claims Eldrazi Temple.
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_restricted_mana_land,
)

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


def test_eldrazi_temple_matches_static():
    card = {
        "name": "Eldrazi Temple",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {C}{C}. Spend this mana only to cast colorless "
            "Eldrazi spells or activate abilities of colorless Eldrazi."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Eldrazi Temple"]


def test_skips_unknown_restriction_phrase():
    """A restriction phrase outside the whitelist must skip — without
    a known tag the parser would build a non-restricted coin and the
    static-parity check would fail."""
    card = {
        "name": "Hypothetical Strange Restriction",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {C}{C}. Spend this mana only on Tuesdays."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_restricted_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_skips_when_first_line_is_not_tap_for_c():
    """The base unrestricted coin must be exactly ``{T}: Add {C}.``;
    other base lines (e.g. dual-color, any-color) need a different
    recognizer."""
    card = {
        "name": "Hypothetical Dual Base",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {U} or {B}.\n"
            "{T}: Add {C}{C}. Spend this mana only to cast colorless "
            "Eldrazi spells or activate abilities of colorless Eldrazi."
        ),
        "produced_mana": ["U", "B", "C"],
        "set": "eos",
    }
    assert _try_build_restricted_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_skips_supertyped_land():
    """Legendary or other-supertyped restricted-mana lands fall outside
    this shape — they need the bespoke per-card path (e.g. Plaza of
    Heroes is Legendary)."""
    card = {
        "name": "Hypothetical Legendary",
        "type_line": "Legendary Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {C}{C}. Spend this mana only to cast colorless "
            "Eldrazi spells or activate abilities of colorless Eldrazi."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_restricted_mana_land(
        card, card["oracle_text"], ("Legendary",), (),
    ) is None


def test_skips_subtyped_land():
    """A subtyped restricted-mana land (e.g. a Cave) needs the bespoke
    per-subtype recognizer."""
    card = {
        "name": "Hypothetical Cave",
        "type_line": "Land — Cave",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {C}{C}. Spend this mana only to cast colorless "
            "Eldrazi spells or activate abilities of colorless Eldrazi."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_restricted_mana_land(
        card, card["oracle_text"], (), ("Cave",),
    ) is None


def test_skips_three_line_shape():
    """The recognizer is for the strict two-line shape only. A third
    oracle line falls through (richer cards need a bespoke path)."""
    card = {
        "name": "Hypothetical Extra Line",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {C}{C}. Spend this mana only to cast colorless "
            "Eldrazi spells or activate abilities of colorless Eldrazi.\n"
            "Indestructible"
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_restricted_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_skips_colored_restricted_mana():
    """The current shape captures all-colorless ``{C}+`` production
    only. A colored or hybrid restricted-mana variant would need a
    separate per-color tally and skips for now."""
    card = {
        "name": "Hypothetical Colored Restriction",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}: Add {R}{R}. Spend this mana only to cast colorless "
            "Eldrazi spells or activate abilities of colorless Eldrazi."
        ),
        "produced_mana": ["C", "R"],
        "set": "eos",
    }
    assert _try_build_restricted_mana_land(
        card, card["oracle_text"], (), (),
    ) is None
