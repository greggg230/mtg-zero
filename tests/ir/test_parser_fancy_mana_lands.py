"""Parser support for fancy-mana single-ability utility lands.

Plain ``Land`` cards (no supertypes, no subtypes) whose entire oracle
text is a single activated mana ability whose surface form matches an
entry in ``_FANCY_MANA_BODIES``. Each entry pairs the exact oracle
line with a factory that returns the canonical ``(cost, effect)``
pair the static registry uses.

Currently claims Mana Confluence, Meteor Crater, Reflecting Pool.
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_fancy_mana_land,
)

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


def test_mana_confluence_matches_static():
    card = {
        "name": "Mana Confluence",
        "type_line": "Land",
        "oracle_text": "{T}, Pay 1 life: Add one mana of any color.",
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Mana Confluence"]


def test_meteor_crater_matches_static():
    card = {
        "name": "Meteor Crater",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Choose a color of a permanent you control. "
            "Add one mana of that color."
        ),
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Meteor Crater"]


def test_reflecting_pool_matches_static():
    card = {
        "name": "Reflecting Pool",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add one mana of any type that a land you control "
            "could produce."
        ),
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Reflecting Pool"]


def test_skips_unknown_oracle_text():
    """Lines outside ``_FANCY_MANA_BODIES`` fall through to the
    silent-skip path. The recognizer is exact-match, not fuzzy."""
    card = {
        "name": "Hypothetical Variant",
        "type_line": "Land",
        "oracle_text": "{T}: Add 2 mana of any color.",
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    assert _try_build_fancy_mana_land(card, card["oracle_text"], (), ()) is None


def test_skips_supertyped_land():
    """A Legendary fancy-mana land would need its own bespoke handler;
    the simple recognizer rejects supertypes."""
    card = {
        "name": "Hypothetical Legendary",
        "type_line": "Legendary Land",
        "oracle_text": "{T}, Pay 1 life: Add one mana of any color.",
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    assert _try_build_fancy_mana_land(
        card, card["oracle_text"], ("Legendary",), (),
    ) is None


def test_skips_subtyped_land():
    """A subtyped fancy-mana land (e.g. a Cave with this oracle text)
    falls outside this shape — needs a per-subtype recognizer."""
    card = {
        "name": "Hypothetical Cave",
        "type_line": "Land — Cave",
        "oracle_text": "{T}, Pay 1 life: Add one mana of any color.",
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    assert _try_build_fancy_mana_land(
        card, card["oracle_text"], (), ("Cave",),
    ) is None
