"""Parser support for the Duskmourn horror-dual cycle.

Plain ``Land`` cards (no supertypes, no subtypes) whose oracle text is
the strict two-line shape:

    This land enters tapped unless a player has <N> or less life.
    {T}: Add {<C1>} or {<C2>}.

The condition is a deterministic state check at ETB (no agent
choice). The threshold flows through
``CardPrinting.enters_tapped_unless_any_player_life_at_most``;
``_play_land`` reads it after applying the default ``enters_tapped``.
"""

# mtg-zero (docs/ENGINE.md 169): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_horror_dual_etb_enters_untapped_when_player_low_life

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_low_life_dual_land,
)


_HORROR_DUAL_NAMES = (
    "Abandoned Campground",
    "Bleeding Woods",
    "Etched Cornfield",
    "Lakeside Shack",
    "Murky Sewer",
    "Neglected Manor",
    "Peculiar Lighthouse",
    "Raucous Carnival",
    "Razortrap Gorge",
    "Strangled Cemetery",
)


def _horror_card(name: str, c1: str, c2: str, threshold: int = 13) -> dict:
    return {
        "name": name,
        "type_line": "Land",
        "oracle_text": (
            f"This land enters tapped unless a player has {threshold} "
            f"or less life.\n"
            f"{{T}}: Add {{{c1}}} or {{{c2}}}."
        ),
        "produced_mana": [c1, c2],
        "set": "dsk",
    }


@pytest.mark.parametrize(
    "name,c1,c2",
    [
        ("Abandoned Campground", "W", "U"),
        ("Bleeding Woods", "R", "G"),
        ("Etched Cornfield", "G", "W"),
        ("Lakeside Shack", "G", "U"),
        ("Murky Sewer", "U", "B"),
        ("Neglected Manor", "W", "B"),
        ("Peculiar Lighthouse", "U", "R"),
        ("Raucous Carnival", "R", "W"),
        ("Razortrap Gorge", "B", "R"),
        ("Strangled Cemetery", "B", "G"),
    ],
)
def test_each_horror_dual_claimed(name, c1, c2):
    parsed = _build_land_printing(_horror_card(name, c1, c2))
    assert parsed is not None
    assert parsed.name == name
    assert parsed.produces == frozenset({c1, c2})
    assert parsed.enters_tapped is False
    assert parsed.enters_tapped_unless_any_player_life_at_most == 13
    # No bespoke activated abilities — relies on the auto-tap-for-cost
    # generic-mana fallback (same as basic / shock duals).
    assert parsed.activated_abilities == ()


def test_horror_dual_skips_when_subtypes_present():
    """Land with a subtype on its type line falls through (the cycle
    has none)."""
    card = _horror_card("Hypothetical", "W", "U")
    assert _try_build_low_life_dual_land(
        card, card["oracle_text"], (), ("Cave",),
    ) is None


def test_horror_dual_skips_when_supertypes_present():
    """Basic supertype falls through (Basic Lands take a different
    arm)."""
    card = _horror_card("Hypothetical", "W", "U")
    assert _try_build_low_life_dual_land(
        card, card["oracle_text"], ("Basic",), (),
    ) is None


def test_horror_dual_skips_color_collision():
    """A printing whose two ``Add`` slots name the same color (e.g.
    ``Add {W} or {W}``) bails — the horror-dual cycle is two colors."""
    card = {
        "name": "Hypothetical Mono Horror",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped unless a player has 13 or less "
            "life.\n{T}: Add {W} or {W}."
        ),
        "produced_mana": ["W"],
        "set": "dsk",
    }
    assert _build_land_printing(card) is None


def test_horror_dual_skips_extra_third_line():
    card = {
        "name": "Hypothetical",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped unless a player has 13 or less "
            "life.\n{T}: Add {W} or {U}.\nWhen this land enters, draw "
            "a card."
        ),
        "produced_mana": ["W", "U"],
        "set": "dsk",
    }
    assert _build_land_printing(card) is None


def test_horror_dual_skips_unrecognized_condition():
    """A condition phrase outside the recognized shape (e.g. ``unless
    you control three lands``) falls through."""
    card = {
        "name": "Hypothetical",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped unless you control three or more "
            "lands.\n{T}: Add {W} or {U}."
        ),
        "produced_mana": ["W", "U"],
        "set": "dsk",
    }
    assert _build_land_printing(card) is None


def test_horror_dual_skips_unrecognized_mana_line():
    """Second line that isn't ``{T}: Add {<C>} or {<C>}.`` falls through."""
    card = {
        "name": "Hypothetical",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped unless a player has 13 or less "
            "life.\n{T}: Add {C}."
        ),
        "produced_mana": ["C"],
        "set": "dsk",
    }
    assert _build_land_printing(card) is None




def test_source_claims_each_horror_dual_land():
    """End-to-end: every horror dual in data/duskmourn.json is claimed."""
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    parser = OracleParserCardSource()
    for name in _HORROR_DUAL_NAMES:
        assert name in parser.claimed_names, f"{name} not claimed"
