"""End-to-end claim tests for EoE Special Guests bonus-sheet cards.

The legacy ``special_guests_builder`` module hosted a name-keyed
exact-oracle-text dispatch that returned canonical printings for a
small set of Special Guests reprints. Every entry has been migrated
to the unified per-builder grammar, the dispatch table is gone, and
the module itself was retired. These tests keep the end-to-end claim
assertions so future refactors can't silently regress one of the
migrated reprints.
"""

from __future__ import annotations

from mtgz_ir.cards.source import get_active_source
from mtgz_ir.effects.predicates import Event

_PRINTINGS = get_active_source().get_printings()


def test_magus_of_the_moon_claimed_via_creature_builder():
    """``Nonbasic lands are Mountains.`` static is recognized in the
    creature builder via ``_parse_nonbasic_lands_are_mountains_line``,
    which sets the printing-level flag.
    """
    assert _PRINTINGS["Magus of the Moon"].nonbasic_lands_are_mountains is True


def test_deafening_silence_claimed_via_enchantment_builder():
    """The noncreature-spell static is recognized by the enchantment
    builder's printing-flag-only fallback path which sets the
    ``restricts_noncreature_spells_to_one_per_turn`` flag."""
    ds = _PRINTINGS["Deafening Silence"]
    assert ds.restricts_noncreature_spells_to_one_per_turn is True


def test_burgeoning_claimed_via_enchantment_builder():
    """The opponent-plays-land trigger is recognized by the
    enchantment builder's unified oracle-text fallback, which produces
    the same ``OptionalPutLandFromHandOntoBattlefield`` effect via the
    generalized ``you may put a (creature|land) card from your hand
    onto the battlefield`` regex (shared with Sledge-Class Seedship's
    creature variant)."""
    burgeoning = _PRINTINGS["Burgeoning"]
    assert len(burgeoning.triggered_abilities) == 1
    assert burgeoning.triggered_abilities[0].event == Event.PLAYS_LAND


def test_sliver_overlord_claimed_via_creature_builder():
    """Both activated abilities parse via the unified grammar:
      * "{3}: Search your library for a Sliver card, reveal that
        card, put it into your hand, then shuffle." claims through
        _SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE with the new "Sliver"
        filter entry (and "reveal that card" wording variant).
      * "{3}: Gain control of target Sliver." claims through the new
        _GAIN_CONTROL_TARGET_SUBTYPE regex dispatching to
        GainControlOfTargetSliverPermanently()."""
    sliver = _PRINTINGS["Sliver Overlord"]
    assert sliver.activated_abilities[0].effect.__class__.__name__ == (
        "SearchLibraryForCard"
    )
    assert sliver.activated_abilities[1].effect.__class__.__name__ == (
        "GainControlOfTargetSliverPermanently"
    )


def test_source_claims_special_guests_via_data_walk():
    """End-to-end: the parser source's data walk picks up the
    Special Guests reprints. All three flagship migrated cards are
    in the claimed set after the dispatch table retirement."""
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    parser = OracleParserCardSource()
    parsed = parser.get_printings()
    assert "Magus of the Moon" in parsed
    assert "Deafening Silence" in parsed
    assert "Sliver Overlord" in parsed
    assert parsed["Magus of the Moon"] == _PRINTINGS["Magus of the Moon"]
    assert parsed["Deafening Silence"] == _PRINTINGS["Deafening Silence"]
    assert parsed["Sliver Overlord"] == _PRINTINGS["Sliver Overlord"]
