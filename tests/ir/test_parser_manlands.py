"""Parser support for Stellar Sights "simple" manlands.

The 6 EoE Stellar Sights manlands without bespoke per-animation
extensions share a fixed three-line oracle shape that maps cleanly
onto ``BecomeLandCreatureUEOT`` with a single keyword set. This
module verifies the parser claims those cards into the same shape
the static registry produces, and rejects near-miss variants
(extra abilities, X-cost activations, attacks-trigger riders, the
"It can't be blocked this turn." Tar Pit shape).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    BecomeLandCreatureUEOT,
    make_utility_land,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _parse_manland_keywords,
    _try_build_manland,
)

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


_SIMPLE_MANLAND_NAMES = (
    "Celestial Colonnade",
    "Hissing Quagmire",
    "Lumbering Falls",
    "Needle Spires",
    "Shambling Vent",
    "Stirring Wildwood",
)


def _shambling_vent_card() -> dict:
    return {
        "name": "Shambling Vent",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W} or {B}.\n"
            "{1}{W}{B}: Until end of turn, this land becomes a 2/3 "
            "white and black Elemental creature with lifelink. It's "
            "still a land."
        ),
        "produced_mana": ["B", "W"],
        "set": "eos",
    }


def test_shambling_vent_matches_static():
    parsed = _build_land_printing(_shambling_vent_card())
    assert parsed is not None
    assert parsed == _PRINTINGS["Shambling Vent"]


def test_celestial_colonnade_matches_static():
    card = {
        "name": "Celestial Colonnade",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W} or {U}.\n"
            "{3}{W}{U}: Until end of turn, this land becomes a 4/4 "
            "white and blue Elemental creature with flying and "
            "vigilance. It's still a land."
        ),
        "produced_mana": ["U", "W"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Celestial Colonnade"]


def test_all_six_simple_manlands_claimed_with_parity():
    """End-to-end: every named simple manland is claimed by the
    parser and matches its static counterpart."""
    from mtgz_ir.cards.parser.source import OracleParserCardSource
    parser = OracleParserCardSource()
    parsed = parser.get_printings()
    for name in _SIMPLE_MANLAND_NAMES:
        assert name in parsed, f"{name} not claimed"
        assert parsed[name] == _PRINTINGS[name]


def test_creeping_tar_pit_matches_static():
    """Creeping Tar Pit's third line replaces the keyword tail with
    the ``It can't be blocked this turn.`` rider. The unblockable
    variant of ``_try_build_manland`` claims it with
    ``unblockable=True`` and no granted keywords."""
    card = {
        "name": "Creeping Tar Pit",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {U} or {B}.\n"
            "{1}{U}{B}: Until end of turn, this land becomes a 3/2 "
            "blue and black Elemental creature. It's still a land. "
            "It can't be blocked this turn."
        ),
        "produced_mana": ["B", "U"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Creeping Tar Pit"]


def test_skips_when_color_pair_mismatches_animate_line():
    """Defensive: the colors in the dual mana ability must match the
    color words on the animate line."""
    card = {
        "name": "Mismatched Manland",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W} or {U}.\n"
            "{3}{W}{U}: Until end of turn, this land becomes a 4/4 "
            "red and green Elemental creature with flying. It's "
            "still a land."
        ),
        "produced_mana": ["U", "W"],
        "set": "eos",
    }
    assert _try_build_manland(card, card["oracle_text"], (), ()) is None


def test_skips_when_supertype_present():
    """A Legendary manland (e.g. a hypothetical Legendary version)
    must skip the simple-manland path — supertypes belong on the
    dedicated bespoke handler."""
    card = {
        "name": "Hypothetical Legendary Manland",
        "type_line": "Legendary Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W} or {U}.\n"
            "{3}{W}{U}: Until end of turn, this land becomes a 4/4 "
            "white and blue Elemental creature with flying. It's "
            "still a land."
        ),
        "produced_mana": ["U", "W"],
        "set": "eos",
    }
    assert _try_build_manland(
        card, card["oracle_text"], ("Legendary",), (),
    ) is None


def test_skips_when_subtype_present():
    """A subtyped manland (e.g. a hypothetical Cave or Desert manland)
    skips the simple-manland path."""
    card = {
        "name": "Hypothetical Cave Manland",
        "type_line": "Land — Cave",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W} or {U}.\n"
            "{3}{W}{U}: Until end of turn, this land becomes a 4/4 "
            "white and blue Elemental creature with flying. It's "
            "still a land."
        ),
        "produced_mana": ["U", "W"],
        "set": "eos",
    }
    assert _try_build_manland(
        card, card["oracle_text"], (), ("Cave",),
    ) is None


def test_skips_when_first_line_is_not_enters_tapped():
    """All EoE simple manlands enter tapped; an untapped variant
    falls outside this shape."""
    card = {
        "name": "Untapped Manland",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {W} or {U}.\n"
            "{3}{W}{U}: Until end of turn, this land becomes a 4/4 "
            "white and blue Elemental creature with flying. It's "
            "still a land."
        ),
        "produced_mana": ["U", "W"],
        "set": "eos",
    }
    assert _try_build_manland(card, card["oracle_text"], (), ()) is None


def test_parse_manland_keywords_handles_single_keyword():
    assert _parse_manland_keywords("flying") == frozenset({"flying"})


def test_parse_manland_keywords_handles_two_keywords_with_and():
    assert _parse_manland_keywords("flying and vigilance") == frozenset(
        {"flying", "vigilance"}
    )


def test_parse_manland_keywords_handles_compound_keyword_phrase():
    """``double strike`` is a single keyword, not two."""
    assert _parse_manland_keywords("double strike") == frozenset(
        {"double strike"}
    )


def test_parse_manland_keywords_rejects_unknown_keyword():
    """If any token isn't a known surface keyword, return None so
    the parser falls through rather than claiming a card whose grant
    semantics aren't yet supported."""
    assert _parse_manland_keywords("flying and unicorn-mode") is None


def test_parse_manland_keywords_rejects_empty():
    assert _parse_manland_keywords("") is None
    assert _parse_manland_keywords("   ") is None


def test_lavaclaw_reaches_matches_static():
    """Lavaclaw Reaches grants an X-pump activated ability while
    animated. The granted-ability variant of ``_try_build_manland``
    should claim it via ``_GRANTED_ABILITY_BODIES`` dispatch and the
    parsed printing must match the static registry."""
    card = {
        "name": "Lavaclaw Reaches",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {B} or {R}.\n"
            "{1}{B}{R}: Until end of turn, this land becomes a 2/2 "
            "black and red Elemental creature with \"{X}: This "
            "creature gets +X/+0 until end of turn.\" It's still a "
            "land."
        ),
        "produced_mana": ["B", "R"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Lavaclaw Reaches"]


def test_wandering_fumarole_matches_static():
    """Wandering Fumarole grants a {0} P/T-swap activated ability."""
    card = {
        "name": "Wandering Fumarole",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {U} or {R}.\n"
            "{2}{U}{R}: Until end of turn, this land becomes a 1/4 "
            "blue and red Elemental creature with \"{0}: Switch this "
            "creature's power and toughness until end of turn.\" "
            "It's still a land."
        ),
        "produced_mana": ["U", "R"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Wandering Fumarole"]


def test_raging_ravine_matches_static():
    """Raging Ravine grants an attacks-triggered +1/+1 counter
    ability — exercises the granted_triggered_abilities path."""
    card = {
        "name": "Raging Ravine",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {R} or {G}.\n"
            "{2}{R}{G}: Until end of turn, this land becomes a 3/3 "
            "red and green Elemental creature with \"Whenever this "
            "creature attacks, put a +1/+1 counter on it.\" It's "
            "still a land."
        ),
        "produced_mana": ["R", "G"],
        "set": "eos",
    }
    parsed = _build_land_printing(card)
    assert parsed is not None
    assert parsed == _PRINTINGS["Raging Ravine"]


def test_skips_granted_body_outside_dispatch_table():
    """A manland with a quoted granted ability whose body isn't on
    the ``_GRANTED_ABILITY_BODIES`` whitelist must NOT be claimed —
    the parser would otherwise produce a printing missing the
    granted ability and fail parity."""
    card = {
        "name": "Hypothetical Hexproof Manland",
        "type_line": "Land",
        "oracle_text": (
            "This land enters tapped.\n"
            "{T}: Add {W} or {U}.\n"
            "{2}{W}{U}: Until end of turn, this land becomes a 3/3 "
            "white and blue Elemental creature with \"{1}: This "
            "creature gains hexproof until end of turn.\" It's "
            "still a land."
        ),
        "produced_mana": ["W", "U"],
        "set": "eos",
    }
    assert _try_build_manland(card, card["oracle_text"], (), ()) is None


def test_simple_manland_builder_returns_make_utility_land_shape():
    """White-box: the builder produces a CardPrinting equal to the
    one ``make_utility_land`` would produce with the same parameters.
    Asserts no extra fields creep in over time."""
    card = _shambling_vent_card()
    parsed = _build_land_printing(card)
    expected = make_utility_land(
        "Shambling Vent",
        produces=frozenset({"W", "B"}),
        enters_tapped=True,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{1}{W}{B}"),
                    mana_str="{1}{W}{B}",
                ),
                effect=BecomeLandCreatureUEOT(
                    power=2,
                    toughness=3,
                    granted_keywords=frozenset({"lifelink"}),
                    # "a 2/3 white and black Elemental creature"
                    granted_subtype="Elemental",
                    set_colors=frozenset({"W", "B"}),
                ),
            ),
        ),
    )
    assert parsed == expected
