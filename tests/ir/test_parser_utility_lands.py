"""Parser support for colorless utility lands.

The colorless-utility-land branch matches plain ``Land`` cards (no
supertypes, no subtypes) whose first oracle line is exactly
``"{T}: Add {C}."``.

  * **Single-line** (only ``{T}: Add {C}.``) → the historical
    ``make_colorless_land`` simplification (no abilities, generic-only).
  * **Multi-line** (Secluded Starforge — ``{T}: Add {C}.`` plus a
    ``Tap X untapped artifacts → +X/+0`` pump and a ``{5},{T}: Create a
    Robot token`` ability) → the FULL faithful printing: every line is
    routed through the activated-ability grammar and the land is built
    via ``make_utility_land`` with all parsed AAs. NO trailing-AA drop
    (was the prior simplification, now superseded by the tap-X-artifacts
    X-channel substrate — see ``tests/test_secluded_starforge_parity.py``).

Other utility lands (Command Bridge — ETB sac-self trigger + any-color
mana ability) are out of scope this batch and remain skipped.
"""

from __future__ import annotations

from mtgz_ir.cards.lands import UTILITY_LANDS
from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_colorless_utility_land,
)
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.cards.parser_source import OracleParserCardSource


def _colorless_branch(card: dict):
    """Run ONLY the colorless-utility-land recognizer on ``card``."""
    supertypes, _card_types, subtypes = _parse_type_line(card["type_line"])
    return _try_build_colorless_utility_land(
        card, card["oracle_text"], supertypes, subtypes,
    )


_STARFORGE_CARD = {
    "name": "Secluded Starforge",
    "mana_cost": "",
    "type_line": "Land",
    "oracle_text": (
        "{T}: Add {C}.\n"
        "{2}, {T}, Tap X untapped artifacts you control: Target "
        "creature gets +X/+0 until end of turn. Activate only as a "
        "sorcery.\n"
        "{5}, {T}: Create a 2/2 colorless Robot artifact creature "
        "token."
    ),
    "produced_mana": ["C"],
}


def test_build_land_printing_claims_secluded_starforge():
    """The Scryfall-shaped Starforge record builds the FULL faithful
    3-AA utility land — produces {C}, all three activated abilities
    routed through the AA grammar (NO trailing-AA drop)."""
    from mtgz_ir.effects import (
        AddManaToPool, CreateToken, PumpTargetCreatureUntilEOT,
    )

    printing = _build_land_printing(_STARFORGE_CARD)
    assert printing is not None
    assert printing.name == "Secluded Starforge"
    assert printing.is_land
    assert printing.is_basic_land is False
    assert printing.supertypes == ()
    assert printing.card_types == ("Land",)
    assert printing.subtypes == ()
    # Faithful full printing: taps for {C}; all three AAs present.
    assert printing.produces == frozenset({"C"})
    assert printing.triggered_abilities == ()
    assert printing.enters_tapped is False
    aas = printing.activated_abilities
    assert len(aas) == 3
    # AA1: {T}: Add {C} (mana ability).
    assert isinstance(aas[0].effect, AddManaToPool)
    assert aas[0].is_mana_ability is True
    assert aas[0].cost.tap_self is True
    # AA2: {2},{T}, Tap X untapped artifacts: +X/+0 UEOT (sorcery-speed).
    assert isinstance(aas[1].effect, PumpTargetCreatureUntilEOT)
    assert aas[1].effect.power_bonus_from_x is True
    assert aas[1].cost.tap_x_untapped_artifacts_you_control is True
    assert aas[1].cost.mana.generic == 2
    assert aas[1].cost.tap_self is True
    assert aas[1].sorcery_speed is True
    # AA3: {5},{T}: Create a 2/2 Robot artifact creature token.
    assert isinstance(aas[2].effect, CreateToken)
    assert aas[2].effect.count == 1
    assert aas[2].cost.mana.generic == 5
    assert aas[2].cost.tap_self is True


def test_build_land_printing_matches_static_registry():
    """The parser-built printing must equal what the static registry
    ships for Secluded Starforge — required by the parity gate
    (parser source and static source must agree), asserted directly
    here for fast feedback."""
    parsed = _build_land_printing(_STARFORGE_CARD)
    expected = UTILITY_LANDS["Secluded Starforge"]
    assert parsed == expected


def test_build_land_printing_skips_basic_with_extra_lines():
    """A non-basic ``Land`` whose first line is ``{T}: Add {C}.`` but
    has no further detail is also fine — same colorless-utility shape,
    just without the dropped abilities."""
    card = {
        "name": "Hypothetical Lone Tap",
        "mana_cost": "",
        "type_line": "Land",
        "oracle_text": "{T}: Add {C}.",
        "produced_mana": ["C"],
    }
    printing = _build_land_printing(card)
    assert printing is not None
    assert printing.name == "Hypothetical Lone Tap"
    assert printing.produces == frozenset()


def test_build_land_printing_skips_utility_land_with_subtypes():
    """A non-basic ``Land — <subtype>`` that taps for {C} is NOT this
    shape (subtypes mean it's something more specific, e.g. a Locus
    or named land type with its own grammar). Parser must skip rather
    than wrongly drop the subtype to coerce a colorless utility land.
    """
    card = {
        "name": "Hand-Crafted Locus",
        "mana_cost": "",
        "type_line": "Land — Locus",
        "oracle_text": "{T}: Add {C}.",
        "produced_mana": ["C"],
    }
    assert _colorless_branch(card) is None
    # The line-composed land grammar claims it with the subtype kept.
    built = _build_land_printing(card)
    assert built is not None
    assert built.subtypes == ("Locus",)
    assert built.produces == frozenset({"C"})


def test_build_land_printing_skips_when_first_line_is_not_colorless_tap():
    """Command Bridge's first line is ``"This land enters tapped."``,
    not ``{T}: Add {C}.`` — must not match the colorless-utility
    branch even though it's a plain ``Land``.
    """
    card = {
        "name": "Command Bridge",
        "mana_cost": "",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "When this land enters, sacrifice it unless you tap an "
            "untapped permanent you control.\n"
            "{T}: Add one mana of any color."
        ),
    }
    assert _build_land_printing(card) is None


def test_build_land_printing_skips_legendary_supertype_with_colorless_tap():
    """A non-basic ``Land`` with another supertype (e.g. Legendary or
    Snow) is out of scope for the colorless-utility branch. The
    static-side ``make_colorless_land`` does support ``legendary=True``
    but we don't claim those yet — silent skip rather than miss the
    Legendary supertype on the type line.
    """
    card = {
        "name": "Hand-Crafted Legendary Tap",
        "mana_cost": "",
        "type_line": "Legendary Land",
        "oracle_text": "{T}: Add {C}.",
        "produced_mana": ["C"],
    }
    assert _colorless_branch(card) is None
    # The line-composed land grammar claims it with Legendary kept.
    built = _build_land_printing(card)
    assert built is not None
    assert built.is_legendary
    assert built.produces == frozenset({"C"})


def test_build_land_printing_skips_basic_supertype_with_colorless_tap():
    """A ``Basic Land`` (with the Basic supertype) is handled by the
    basic branch only — the colorless-utility branch must not steal a
    Basic whose oracle text happens to read ``{T}: Add {C}.`` (without
    parentheses, so the basic branch also rejects it).
    """
    card = {
        "name": "Hand-Crafted Wastes",
        "mana_cost": "",
        "type_line": "Basic Land — Wastes",
        "oracle_text": "{T}: Add {C}.",
        "produced_mana": ["C"],
    }
    # Basic supertype routes to the basic branch, which requires
    # parentheses around the reminder. No fallthrough to the
    # colorless-utility branch — the card is silently skipped.
    assert _build_land_printing(card) is None


def test_build_land_printing_skips_colorless_tap_with_colored_produced_mana():
    """Defensive: a plain ``Land`` whose first line is
    ``{T}: Add {C}.`` but whose ``produced_mana`` lists a color must
    not match — the card actually produces colored mana via some
    other ability, so dropping that ability would silently neuter it.
    """
    card = {
        "name": "Hand-Crafted Mixed",
        "mana_cost": "",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{1}, {T}: Add {U}."
        ),
        "produced_mana": ["C", "U"],
    }
    assert _colorless_branch(card) is None
    # The line-composed land grammar keeps the paid {U} ability as an
    # explicit mana ability (it isn't free, so it stays out of
    # ``produces``).
    built = _build_land_printing(card)
    assert built is not None
    assert built.produces == frozenset({"C"})
    (paid,) = built.activated_abilities
    assert paid.is_mana_ability
    assert paid.cost.mana_str == "{1}" and paid.cost.tap_self


def test_build_land_printing_skips_empty_oracle_on_plain_land():
    """A plain ``Land`` with no oracle text at all has nothing to key
    off — must skip rather than build a stub colorless utility land."""
    card = {
        "name": "Hand-Crafted Empty",
        "mana_cost": "",
        "type_line": "Land",
        "oracle_text": "",
    }
    assert _build_land_printing(card) is None


def test_source_claims_secluded_starforge():
    """End-to-end: ``OracleParserCardSource`` walks the cached EoE
    Scryfall data and ends up claiming Secluded Starforge."""
    source = OracleParserCardSource()
    assert "Secluded Starforge" in source.claimed_names
    assert "Secluded Starforge" in source.get_printings()
