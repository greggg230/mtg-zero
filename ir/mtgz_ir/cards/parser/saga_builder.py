"""Saga builder (CR 714) -- cube layouts bucket.

A Saga's oracle text is its lore-counter reminder (stripped as reminder
text) followed by chapter lines ``<numerals> — <effect>``. Each chapter
numeral becomes a chapter ability: a ``TriggeredAbility`` with
``event == Event.SAGA_CHAPTER`` and ``predicate == ChapterReached(N)``
whose effect is the body parsed through the unified spell-effect
grammar. ``chapter_count`` is the final chapter
number. The lore counters, chapter triggers and the final-chapter
sacrifice are engine mechanics (``mtgz_ir.layouts``, ``turn``).

Every line must be a chapter line whose body parses; "Read ahead"
(CR 702.155) and other Saga keywords aren't modeled, so a Saga printing
them isn't claimed.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import CardPrinting, TriggeredAbility, parse_mana_cost
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line

_ROMAN = {"I": 1, "II": 2, "III": 3, "IV": 4, "V": 5, "VI": 6}
_CHAPTER_LINE = re.compile(
    r"^(?P<nums>[IVX]+(?:, [IVX]+)*) — (?P<body>.+)$"
)


def chapter_ability(n: int, effect) -> TriggeredAbility:
    """Chapter ``n``'s triggered ability (CR 714.2b)."""
    from mtgz_ir.effects.predicates import ChapterReached, Event

    return TriggeredAbility(
        event=Event.SAGA_CHAPTER, predicate=ChapterReached(n), effect=effect,
    )


def _build_saga_printing(card: dict) -> CardPrinting | None:
    supertypes, card_types, subtypes = _parse_type_line(card.get("type_line", ""))
    if "Saga" not in subtypes:
        return None
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    name = card["name"]
    chapters: list[TriggeredAbility] = []
    for raw in (card.get("oracle_text") or "").split("\n"):
        line = _strip_reminder_text(raw)
        if not line:
            continue
        m = _CHAPTER_LINE.match(line)
        if m is None:
            return None
        effect = _parse_spell_effect_phrase(m.group("body"), name)
        if effect is None:
            return None
        for numeral in m.group("nums").split(", "):
            n = _ROMAN.get(numeral)
            if n is None:
                return None
            chapters.append(chapter_ability(n, effect))
    if not chapters:
        return None
    chapter_count = max(t.predicate.n for t in chapters)
    extra: dict = {}
    if "Land" in card_types:
        # A Saga land (Urza's Saga) has no mana ability of its own; it
        # pays for spells only through abilities it gains
        # (``game.effective_land_produces`` reads the granted ones).
        extra["mana_only_from_produces"] = True
    return CardPrinting(
        name=name,
        mana_cost_str=card.get("mana_cost", ""),
        mana_cost=parse_mana_cost(card.get("mana_cost", "")),
        type_line=card.get("type_line", ""),
        supertypes=tuple(supertypes),
        card_types=tuple(card_types),
        subtypes=tuple(subtypes),
        triggered_abilities=tuple(chapters),
        chapter_count=chapter_count,
        **extra,
    )
