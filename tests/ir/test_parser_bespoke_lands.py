"""Parser support for bespoke single-line / payment-shape utility lands.

Two recognizers, each tightly scoped:

- ``_try_build_pain_two_mana_land`` — Ancient Tomb shape: a single
  oracle line whose ``{T}`` mana ability fuses an Add-N-colorless
  production with a damage-to-controller rider, modeled by the
  ``DealDamageToControllerAndAddMana`` engine class.
- ``_try_build_paid_fancy_mana_land`` — Crystal Quarry / Cascading
  Cataracts shape: optional evergreen-keyword line + the dropped
  ``{T}: Add {C}.`` line + a single paid-mana activation whose body
  is on a small whitelist.
"""

from __future__ import annotations

from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_paid_fancy_mana_land,
    _try_build_pain_two_mana_land,
)

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


def test_ancient_tomb_matches_static():
    card = {
        "name": "Ancient Tomb",
        "type_line": "Land",
        "oracle_text": "{T}: Add {C}{C}. This land deals 2 damage to you.",
        "produced_mana": ["C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Ancient Tomb"]


def test_pain_land_skips_when_mana_includes_color():
    """The current effect class assumes all-colorless production.
    A hypothetical colored-pain-land variant must skip until a
    colored shape lands in the engine."""
    card = {
        "name": "Hypothetical Colored Pain",
        "type_line": "Land",
        "oracle_text": "{T}: Add {R}{R}. This land deals 2 damage to you.",
        "produced_mana": ["R"],
        "set": "eos",
    }
    assert _try_build_pain_two_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_pain_land_skips_supertyped():
    card = {
        "name": "Hypothetical Legendary Pain",
        "type_line": "Legendary Land",
        "oracle_text": "{T}: Add {C}{C}. This land deals 2 damage to you.",
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_pain_two_mana_land(
        card, card["oracle_text"], ("Legendary",), (),
    ) is None


def test_pain_land_skips_subtyped():
    card = {
        "name": "Hypothetical Cave Pain",
        "type_line": "Land — Cave",
        "oracle_text": "{T}: Add {C}{C}. This land deals 2 damage to you.",
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_pain_two_mana_land(
        card, card["oracle_text"], (), ("Cave",),
    ) is None


def test_crystal_quarry_matches_static():
    card = {
        "name": "Crystal Quarry",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{5}, {T}: Add {W}{U}{B}{R}{G}."
        ),
        "produced_mana": ["W", "U", "B", "R", "G", "C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Crystal Quarry"]


def test_cascading_cataracts_matches_static():
    """Three-line variant: leading evergreen keyword line +
    dropped tap-for-C + paid fancy mana."""
    card = {
        "name": "Cascading Cataracts",
        "type_line": "Land",
        "oracle_text": (
            "Indestructible\n"
            "{T}: Add {C}.\n"
            "{5}, {T}: Add five mana in any combination of colors."
        ),
        "produced_mana": ["W", "U", "B", "R", "G", "C"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Cascading Cataracts"]


def test_paid_fancy_skips_unknown_keyword_line():
    """Leading-line keyword outside the land-keyword whitelist must
    skip — the parser would otherwise drop the keyword silently."""
    card = {
        "name": "Hypothetical Lifelink Land",
        "type_line": "Land",
        "oracle_text": (
            "Lifelink\n"
            "{T}: Add {C}.\n"
            "{5}, {T}: Add {W}{U}{B}{R}{G}."
        ),
        "produced_mana": ["W", "U", "B", "R", "G", "C"],
        "set": "eos",
    }
    assert _try_build_paid_fancy_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_paid_fancy_skips_unknown_body():
    """Paid-mana body outside the whitelist must skip; an unrecognized
    fancy-mana production needs its own engine class."""
    card = {
        "name": "Hypothetical Strange Quarry",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{5}, {T}: Untap each artifact you control."
        ),
        "produced_mana": ["C"],
        "set": "eos",
    }
    assert _try_build_paid_fancy_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_paid_fancy_skips_when_dropped_tap_line_missing():
    """Without the canonical ``{T}: Add {C}.`` middle line the shape
    is something else entirely — fall through."""
    card = {
        "name": "Hypothetical No-Tap-C",
        "type_line": "Land",
        "oracle_text": "{5}, {T}: Add {W}{U}{B}{R}{G}.",
        "produced_mana": ["W", "U", "B", "R", "G"],
        "set": "eos",
    }
    assert _try_build_paid_fancy_mana_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_paid_fancy_skips_supertyped_or_subtyped():
    base_card = {
        "name": "Hypothetical Legendary Quarry",
        "type_line": "Legendary Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{5}, {T}: Add {W}{U}{B}{R}{G}."
        ),
        "produced_mana": ["W", "U", "B", "R", "G", "C"],
        "set": "eos",
    }
    assert _try_build_paid_fancy_mana_land(
        base_card, base_card["oracle_text"], ("Legendary",), (),
    ) is None
    sub_card = dict(base_card)
    sub_card["type_line"] = "Land — Desert"
    assert _try_build_paid_fancy_mana_land(
        sub_card, sub_card["oracle_text"], (), ("Desert",),
    ) is None
