"""Parser support for "fetch" lands — pure tap-and-sac search effects.

A plain ``Land`` (no supertypes, no subtypes) whose entire oracle
text is the single line:

    {T}, Sacrifice this land: <BODY>.

Distinct from ``_try_build_tap_sac_utility_land`` which gates on a
preceding ``{T}: Add {C}.`` line — fetch lands never mint mana, they
only pay their own sacrifice activation.

Currently claims Terramorphic Expanse.
"""

# mtg-zero (docs/ENGINE.md 169): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_terramorphic_expanse_resolution_fetches_basic

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    SearchLibraryForBasicLandTapped,
    make_utility_land,
)
from mtgz_ir.cards.parser.land_builder import (
    _build_land_printing,
    _try_build_fetch_land,
)


def _terramorphic_expanse_card() -> dict:
    return {
        "name": "Terramorphic Expanse",
        "type_line": "Land",
        "oracle_text": (
            "{T}, Sacrifice this land: Search your library for a "
            "basic land card, put it onto the battlefield tapped, "
            "then shuffle."
        ),
        "set": "dsk",
    }


def test_terramorphic_expanse_claimed():
    parsed = _build_land_printing(_terramorphic_expanse_card())
    assert parsed is not None
    expected = make_utility_land(
        "Terramorphic Expanse",
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True, sacrifice_self=True),
                effect=SearchLibraryForBasicLandTapped(count=1),
            ),
        ),
    )
    assert parsed == expected


def test_terramorphic_expanse_no_produced_mana():
    """Fetch lands have an empty ``produces`` set — they never mint
    mana, only sac for the search."""
    parsed = _build_land_printing(_terramorphic_expanse_card())
    assert parsed is not None
    assert parsed.produces == frozenset()


def test_fetch_skips_when_supertypes_present():
    card = _terramorphic_expanse_card()
    card["type_line"] = "Basic Land"
    assert _try_build_fetch_land(
        card, card["oracle_text"], ("Basic",), (),
    ) is None


def test_fetch_skips_when_subtypes_present():
    card = _terramorphic_expanse_card()
    assert _try_build_fetch_land(
        card, card["oracle_text"], (), ("Cave",),
    ) is None


def test_fetch_skips_unrecognized_body():
    """A sac-self body outside the fetch whitelist must not be claimed
    by the fetch recognizer (it would build the wrong effect). The
    line-composed land grammar then claims it faithfully through the
    unified activated-ability grammar: tap + sacrifice, draw three,
    and no mana production."""
    from mtgz_ir.cards.base import DrawCards

    card = {
        "name": "Hypothetical Fetch",
        "type_line": "Land",
        "oracle_text": (
            "{T}, Sacrifice this land: Draw three cards."
        ),
        "set": "dsk",
    }
    assert _try_build_fetch_land(card, card["oracle_text"], (), ()) is None
    built = _build_land_printing(card)
    assert built is not None
    assert built.produces == frozenset()
    assert built.mana_only_from_produces is True
    (ability,) = built.activated_abilities
    assert ability.cost.tap_self and ability.cost.sacrifice_self
    assert ability.effect == DrawCards(amount=3)


def test_fetch_skips_extra_lines():
    """Multi-line oracle text falls through — Terramorphic Expanse
    is single-line; multi-line tap-and-sac shapes are claimed by the
    older ``_try_build_tap_sac_utility_land`` recognizer."""
    card = {
        "name": "Hypothetical",
        "type_line": "Land",
        "oracle_text": (
            "{T}: Add {C}.\n"
            "{T}, Sacrifice this land: Search your library for a "
            "basic land card, put it onto the battlefield tapped, "
            "then shuffle."
        ),
        "set": "dsk",
    }
    assert _try_build_fetch_land(
        card, card["oracle_text"], (), (),
    ) is None


def test_source_claims_terramorphic_expanse():
    """End-to-end: Terramorphic Expanse is claimed from the cached
    Duskmourn data."""
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    parser = OracleParserCardSource()
    assert "Terramorphic Expanse" in parser.claimed_names


