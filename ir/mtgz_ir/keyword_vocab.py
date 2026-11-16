"""The closed vocabulary of keyword abilities (CR 702) as structured data.

The engine spells keywords as lowercase strings in every keyword set
(``CardPrinting.keywords``, ``effective_keywords``, granted-keyword
fields): ``"flying"``, ``"first strike"``, ``"annihilator 6"``,
``"protection from the chosen player"``. This module is the definition
of that vocabulary: a :class:`KeywordKind` enum plus parameters
(``n`` for annihilator / toxic, a :class:`ProtectionQuality` for
protection / hexproof from), with :func:`parse_keyword` /
:attr:`Keyword.spelling` converting between the spelling and the
structure. An unknown spelling is an error.

Fields whose values are keyword spellings are annotated
``KeywordSpelling`` (``Annotated[str, KEYWORD_SPELLING]``); the card IR
(``mtgz_ir.cards.ir``) emits them as :class:`Keyword` objects, and
``CardPrinting`` validates its keyword sets on construction.

The vocabulary is collected from the code that gives keywords meaning:
the parser's keyword grammar (``cards.parser.keywords``,
``creature_rmc_grammar`` annihilator / cascade, ``roto_keywords`` split
second / start your engines! / toxic, ``roto_walkers`` compleated) and
the engine's consumers (``keyword_rules``, combat, ``roto_mana``
changeling, ``roto_board`` hexproof from).
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Annotated


class KeywordKind(StrEnum):
    """Keyword names. A parameterized kind's spelling is ``"<kind> <n>"``
    (annihilator, toxic) or ``"<kind> <quality>"`` (protection from,
    hexproof from)."""

    FLYING = "flying"
    TRAMPLE = "trample"
    VIGILANCE = "vigilance"
    HASTE = "haste"
    REACH = "reach"
    DEATHTOUCH = "deathtouch"
    LIFELINK = "lifelink"
    DEFENDER = "defender"
    FLASH = "flash"
    MENACE = "menace"
    DOUBLE_STRIKE = "double strike"
    FIRST_STRIKE = "first strike"
    PROWESS = "prowess"
    HEXPROOF = "hexproof"
    INDESTRUCTIBLE = "indestructible"
    SHROUD = "shroud"
    EXALTED = "exalted"
    INFECT = "infect"
    SHADOW = "shadow"
    BATTLE_CRY = "battle cry"
    DEVOID = "devoid"
    PERSIST = "persist"
    ASCEND = "ascend"
    PLAINSWALK = "plainswalk"
    ISLANDWALK = "islandwalk"
    SWAMPWALK = "swampwalk"
    MOUNTAINWALK = "mountainwalk"
    FORESTWALK = "forestwalk"
    CASCADE = "cascade"
    SPLIT_SECOND = "split second"
    START_YOUR_ENGINES = "start your engines!"
    COMPLEATED = "compleated"
    CHANGELING = "changeling"
    # Parameterized by ``n``.
    ANNIHILATOR = "annihilator"
    TOXIC = "toxic"
    # Modular N (CR 702.43; Power Depot). The Python engine doesn't model
    # it (docs/SEARCH_LIMITATIONS.md); the card IR carries it.
    MODULAR = "modular"
    # Parameterized by ``quality``.
    PROTECTION_FROM = "protection from"
    HEXPROOF_FROM = "hexproof from"


class ProtectionQuality(StrEnum):
    """Qualities a ``protection from`` / ``hexproof from`` keyword names
    (CR 702.16, 702.11d); resolved by ``keyword_rules.source_has_quality``
    and ``keyword_rules.is_protected_from``."""

    WHITE = "white"
    BLUE = "blue"
    BLACK = "black"
    RED = "red"
    GREEN = "green"
    COLORLESS = "colorless"
    MULTICOLORED = "multicolored"
    MONOCOLORED = "monocolored"
    EVERYTHING = "everything"
    CREATURES = "creatures"
    ARTIFACTS = "artifacts"
    ENCHANTMENTS = "enchantments"
    INSTANTS = "instants"
    SORCERIES = "sorceries"
    PLANESWALKERS = "planeswalkers"
    # Player qualities (CR 702.16j).
    THE_CHOSEN_PLAYER = "the chosen player"
    EACH_OF_YOUR_OPPONENTS = "each of your opponents"
    # Emrakul, the Aeons Torn.
    SPELLS_THAT_ARE_ONE_OR_MORE_COLORS = "spells that are one or more colors"


_WITH_N = frozenset({KeywordKind.ANNIHILATOR, KeywordKind.TOXIC, KeywordKind.MODULAR})
_WITH_QUALITY = frozenset({KeywordKind.PROTECTION_FROM, KeywordKind.HEXPROOF_FROM})


@dataclass(frozen=True)
class Keyword:
    """One keyword ability: ``kind`` plus its parameter (``n`` for
    annihilator / toxic, ``quality`` for protection / hexproof from)."""

    kind: KeywordKind
    n: int = 0
    quality: ProtectionQuality | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "kind", KeywordKind(self.kind))
        if self.quality is not None:
            object.__setattr__(self, "quality", ProtectionQuality(self.quality))
        if (self.kind in _WITH_N) != (self.n > 0):
            raise ValueError(f"{self.kind.value!r} takes a positive n: {self!r}")
        if (self.kind in _WITH_QUALITY) != (self.quality is not None):
            raise ValueError(f"{self.kind.value!r} quality mismatch: {self!r}")

    @property
    def spelling(self) -> str:
        """The engine's keyword string."""
        if self.kind in _WITH_N:
            return f"{self.kind.value} {self.n}"
        if self.kind in _WITH_QUALITY:
            return f"{self.kind.value} {self.quality.value}"
        return self.kind.value


def parse_keyword(spelling: str) -> Keyword:
    """The :class:`Keyword` an engine keyword string spells; raises
    ``ValueError`` for a spelling outside the vocabulary."""
    for kind in _WITH_QUALITY:
        prefix = kind.value + " "
        if spelling.startswith(prefix):
            try:
                return Keyword(kind, quality=ProtectionQuality(spelling[len(prefix):]))
            except ValueError:
                raise ValueError(f"unknown keyword {spelling!r}") from None
    for kind in _WITH_N:
        prefix = kind.value + " "
        if spelling.startswith(prefix) and spelling[len(prefix):].isdigit():
            return Keyword(kind, n=int(spelling[len(prefix):]))
    try:
        kind = KeywordKind(spelling)
    except ValueError:
        raise ValueError(f"unknown keyword {spelling!r}") from None
    if kind in _WITH_N or kind in _WITH_QUALITY:
        raise ValueError(f"keyword {spelling!r} is missing its parameter")
    return Keyword(kind)


def is_keyword_spelling(spelling: str) -> bool:
    try:
        parse_keyword(spelling)
    except ValueError:
        return False
    return True


class _KeywordSpellingMarker:
    """``Annotated`` metadata marking a ``str`` slot as a keyword spelling."""

    def __repr__(self) -> str:
        return "KEYWORD_SPELLING"


KEYWORD_SPELLING = _KeywordSpellingMarker()
KeywordSpelling = Annotated[str, KEYWORD_SPELLING]


def validate_keywords(owner: str, spellings) -> None:
    """Raise ``ValueError`` if any spelling in ``spellings`` is outside
    the vocabulary (``owner`` names the card for the message)."""
    for s in spellings:
        try:
            parse_keyword(s)
        except ValueError as ex:
            raise ValueError(f"{owner}: {ex}") from None


__all__ = [
    "KEYWORD_SPELLING",
    "Keyword",
    "KeywordKind",
    "KeywordSpelling",
    "ProtectionQuality",
    "is_keyword_spelling",
    "parse_keyword",
    "validate_keywords",
]
