"""The closed keyword vocabulary (``mtgz_ir.keyword_vocab``)."""

from __future__ import annotations

import dataclasses

import pytest

from mtgz_ir.cards.base import make_creature
from mtgz_ir.cards.parser.keywords import _KEYWORD_LINES
from mtgz_ir.keyword_rules import PROTECTION_QUALITIES, protection_keyword
from mtgz_ir.keyword_vocab import (
    Keyword,
    KeywordKind,
    ProtectionQuality,
    is_keyword_spelling,
    parse_keyword,
    validate_keywords,
)


@pytest.mark.parametrize("spelling, expected", [
    ("flying", Keyword(KeywordKind.FLYING)),
    ("first strike", Keyword(KeywordKind.FIRST_STRIKE)),
    ("start your engines!", Keyword(KeywordKind.START_YOUR_ENGINES)),
    ("annihilator 6", Keyword(KeywordKind.ANNIHILATOR, n=6)),
    ("toxic 1", Keyword(KeywordKind.TOXIC, n=1)),
    ("protection from the chosen player",
     Keyword(KeywordKind.PROTECTION_FROM, quality=ProtectionQuality.THE_CHOSEN_PLAYER)),
    ("hexproof from blue",
     Keyword(KeywordKind.HEXPROOF_FROM, quality=ProtectionQuality.BLUE)),
])
def test_parse_and_spell_round_trip(spelling, expected):
    kw = parse_keyword(spelling)
    assert kw == expected
    assert kw.spelling == spelling
    assert is_keyword_spelling(spelling)


@pytest.mark.parametrize("bad", [
    "flyin", "annihilator", "annihilator x", "toxic", "protection from",
    "protection from purple", "hexproof from", "Flying", "",
])
def test_unknown_spellings_raise(bad):
    with pytest.raises(ValueError):
        parse_keyword(bad)
    assert not is_keyword_spelling(bad)


def test_keyword_validates_its_parameters():
    with pytest.raises(ValueError):
        Keyword(KeywordKind.ANNIHILATOR)  # needs n
    with pytest.raises(ValueError):
        Keyword(KeywordKind.FLYING, n=2)
    with pytest.raises(ValueError):
        Keyword(KeywordKind.PROTECTION_FROM)  # needs a quality
    with pytest.raises(ValueError):
        Keyword(KeywordKind.HASTE, quality=ProtectionQuality.RED)
    # Strings coerce to the enums.
    assert Keyword("protection from", quality="red").quality is ProtectionQuality.RED


def test_vocabulary_covers_the_parser_grammar():
    for spelling in _KEYWORD_LINES.values():
        assert is_keyword_spelling(spelling), spelling
    assert PROTECTION_QUALITIES == {q.value for q in ProtectionQuality}
    for quality in PROTECTION_QUALITIES:
        assert parse_keyword(protection_keyword(quality)).quality == quality


def test_printings_reject_unknown_keywords():
    bear = make_creature("Bear", "{1}{G}", 2, 2, keywords=("flying",))
    with pytest.raises(ValueError, match="unknown keyword 'flyng'"):
        dataclasses.replace(bear, keywords=frozenset({"flyng"}))
    with pytest.raises(ValueError):
        dataclasses.replace(bear, stationed_keywords=frozenset({"annihilator"}))
    with pytest.raises(ValueError):
        dataclasses.replace(
            bear, creature_during_your_turn=(3, 4, ("Ninja",), frozenset({"bogus"})),
        )
    validate_keywords("ok", ("haste", "toxic 2"))


def test_every_pool_keyword_is_in_the_vocabulary():
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    for name, p in OracleParserCardSource(pool="all").get_printings().items():
        for kw in p.keywords | p.stationed_keywords:
            assert is_keyword_spelling(kw), (name, kw)
