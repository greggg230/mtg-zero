"""Parity tests for spacecraft-batch22 grammar-claimed entries.

Each test verifies that ``OracleParserCardSource`` returns a
``CardPrinting`` equal to the one in the static registry for a
spacecraft migrated from the ``_BESPOKE_SPACECRAFT`` dispatch table to
the unified oracle-text grammar in the current batch.
"""

from __future__ import annotations

from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.cards.source import get_active_source

_PARSER_PRINTINGS = OracleParserCardSource().get_printings()
_STATIC_PRINTINGS = get_active_source().get_printings()


def test_entropic_battlecruiser_parser_parity():
    """Parser-built Entropic Battlecruiser equals static registry.

    Slice 6 grammar extensions used:
    - ``"an opponent discards a card"`` trigger condition phrase →
      ``(DISCARDS_CARD, Controlled(OPPONENT))``
    - ``"they lose 3 life"`` effect → ``TargetPlayerLosesLife(amount=3)``
    - Multi-sentence literal for the attack-trigger body →
      ``EachOpponentDiscardsOrLosesLife(life_if_cant=3)``
    """
    assert _PARSER_PRINTINGS["Entropic Battlecruiser"] == (
        _STATIC_PRINTINGS["Entropic Battlecruiser"]
    )


def test_synthesizer_labship_parser_parity():
    """Parser-built Synthesizer Labship equals static registry.

    Slice 6 grammar extension used:
    - Parameterized ``_TARGET_ARTIFACT_BECOMES_CREATURE_UEOT`` regex →
      ``TargetArtifactBecomesCreatureUEOT(power=2, toughness=2,
      granted_keywords=frozenset({'flying'}))``
    - ``BEGINNING_OF_COMBAT + Controlled(YOU)`` accepted by the
      tier-gated trigger arm.
    """
    assert _PARSER_PRINTINGS["Synthesizer Labship"] == (
        _STATIC_PRINTINGS["Synthesizer Labship"]
    )


def test_dawnsire_sunstar_dreadnought_parser_parity():
    """Parser-built Dawnsire, Sunstar Dreadnought equals static registry.

    Slice 6 grammar extensions used:
    - ``_DEAL_DAMAGE_UP_TO_ONE_TARGET_CREATURE`` regex extended with
      ``(?:this creature )?`` prefix and ``(?: or planeswalker)?`` suffix.
    - ``YOU_ATTACK + Controlled(YOU)`` accepted by the tier-gated trigger
      arm (oracle-text faithful: "Whenever you attack" → YOU_ATTACK).
    """
    assert _PARSER_PRINTINGS["Dawnsire, Sunstar Dreadnought"] == (
        _STATIC_PRINTINGS["Dawnsire, Sunstar Dreadnought"]
    )
