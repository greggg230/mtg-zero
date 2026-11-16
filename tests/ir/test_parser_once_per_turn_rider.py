"""Tests for the trailing once-per-turn rider on triggered-ability lines.

Scryfall prints ``This ability triggers only once each turn.`` as a
second sentence glued to the trigger body with a single space
separator. The parser strips the suffix up front (in
``_parse_triggered_ability_line``) and sets ``once_per_turn=True`` on
the resulting ``TriggeredAbility``. The strip is rider-aware on both
the plain trigger path and the intervening-if path.

Mechan Assembler is the EoE printing that ships this rider on a
parser-claimed card today (its trigger phrase is already in
``_PREDICATE_TRIGGER_PHRASES`` and its effect parses via the
unified token-creation grammar). Nanoform Sentinel also carries the
rider but its effect (``untap another target permanent``) isn't yet
in the effect grammar, so it stays unclaimed.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CreateToken,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.object_predicates import (
    IsArtifact,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    Subject,
    YOU,
)

from mtgz_ir.cards.source import get_active_source

_PRINTINGS = get_active_source().get_printings()


def test_once_per_turn_rider_sets_flag_on_predicate_path():
    """``Whenever another artifact you control enters, create a 2/2
    colorless Robot artifact creature token. This ability triggers
    only once each turn.`` (Mechan Assembler) parses to a single
    ``TriggeredAbility`` whose ``once_per_turn=True``. The trigger
    body is identical to the no-rider equivalent except for the flag.
    """
    line = (
        "Whenever another artifact you control enters, create a 2/2 "
        "colorless Robot artifact creature token. This ability triggers "
        "only once each turn."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed is not None
    assert parsed.once_per_turn is True
    assert parsed.event == Event.ENTERS
    # Predicate is the existing ``another artifact you control enters``
    # phrase mapping — IsArtifact & Controlled(YOU) & ~IsSelf().
    assert parsed.predicate == (
        Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf()
    )
    assert parsed.effect == CreateToken(
        token_printing=TOKEN_PRINTINGS["Robot"], count=1,
    )


def test_no_once_per_turn_rider_leaves_flag_false():
    """A trigger line WITHOUT the rider parses with ``once_per_turn=
    False`` — the flag is opt-in via the trailing suffix, not a
    default."""
    line = (
        "Whenever another artifact you control enters, create a 2/2 "
        "colorless Robot artifact creature token."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed is not None
    assert parsed.once_per_turn is False


def test_once_per_turn_rider_on_intervening_if_path():
    """The rider strip happens BEFORE intervening-if shape detection,
    so a hypothetical gated-trigger line carrying the rider still
    parses cleanly with ``once_per_turn=True``. No EoE printing today
    pairs intervening-if with the once-per-turn rider, but the parser
    must support the composition because the strip is generic and
    covers both shapes uniformly.
    """
    line = (
        "When this creature enters, if an opponent controls more lands "
        "than you, create a Lander token. This ability triggers only "
        "once each turn."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed is not None
    assert parsed.once_per_turn is True
    assert parsed.event == Event.ENTERS
    assert parsed.effect == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"], count=1,
    )


def test_once_per_turn_rider_strict_match():
    """Variant phrasings of the rider (different spacing, missing
    period, alternate text) do NOT trigger the strip — the suffix
    match is exact-literal. A line with mismatched rider text falls
    through to the standard trigger-line regex, which sees the rider
    as part of the effect phrase and (in this synthetic case) fails
    to parse — exactly the behavior we want, since silently dropping
    a rider would lie about engine semantics.
    """
    # No leading space before "This ability" — not the printed shape.
    line = (
        "Whenever another artifact you control enters, create a 2/2 "
        "colorless Robot artifact creature token.This ability triggers "
        "only once each turn."
    )
    parsed = _parse_triggered_ability_line(line)
    # Falls through, effect contains both sentences glued together,
    # which doesn't parse; the line is rejected outright.
    assert parsed is None


def test_once_per_turn_rider_round_trips_via_creature_builder():
    """End-to-end: Mechan Assembler's full oracle text builds a
    ``CardPrinting`` whose triggered ability carries
    ``once_per_turn=True`` AND matches the static registry's
    ``CardPrinting`` exactly. This is the parity assertion that the
    auto-parametrized ``test_parser_card_matches_static`` test would
    also catch, but spelled out here so the once-per-turn rider has
    its own targeted regression test.
    """
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    parser = OracleParserCardSource()
    parsed = parser.get_printings()["Mechan Assembler"]
    static = _PRINTINGS["Mechan Assembler"]
    assert parsed == static
    assert len(parsed.triggered_abilities) == 1
    assert parsed.triggered_abilities[0].once_per_turn is True
