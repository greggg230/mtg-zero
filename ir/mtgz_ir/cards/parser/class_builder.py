"""Class enchantment builder (CR 716) -- cube layouts bucket.

A Class's oracle text is its level-1 abilities, then for each further
level a ``{cost}: Level N`` line followed by that level's abilities.
Each level becomes its own printing (``CardPrinting.class_levels``,
``class_level``) carrying every ability of the levels up to it; the
level-N printing also carries the "Level N+1" activated ability, so it
can be activated only while the Class is exactly level N (CR 716.2a).
"When this Class becomes level N, ..." is a triggered ability with
``event == Event.CLASS_LEVEL_GAINED`` and ``predicate ==
ClassLevelReached(N)``, fired by ``layouts.gain_class_level``.

Ability lines parse through the unified clause grammar with "this
Class" read as "this enchantment"; any unparsed line rejects the card.
"""

from __future__ import annotations

import dataclasses
import re

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CardPrinting,
    TriggeredAbility,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line

_LEVEL_LINE = re.compile(r"^(?P<cost>(?:\{[^}]+\})+): Level (?P<n>\d+)$")
_BECOMES_LEVEL_LINE = re.compile(
    r"^When this Class becomes level (?P<n>\d+), (?P<body>.+)$"
)


def class_level_ability(n: int, effect) -> TriggeredAbility:
    """"When this Class becomes level ``n``, <effect>." (CR 716.2a)."""
    from mtgz_ir.effects.predicates import ClassLevelReached, Event

    return TriggeredAbility(
        event=Event.CLASS_LEVEL_GAINED, predicate=ClassLevelReached(n),
        effect=effect,
    )


def _parse_section(lines: list[str], name: str):
    """(triggers, activated, statics, keywords) for one level's lines,
    or ``None`` if any line doesn't parse."""
    from mtgz_ir.cards.parser.clause import _parse_clause
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    triggers: list = []
    activated: list = []
    statics: list = []
    keywords: set[str] = set()
    for line in lines:
        m = _BECOMES_LEVEL_LINE.match(line)
        if m is not None:
            effect = _parse_spell_effect_phrase(m.group("body"), name)
            if effect is None:
                return None
            triggers.append(class_level_ability(int(m.group("n")), effect))
            continue
        parsed = _parse_clause(
            line.replace("this Class", "this enchantment"), name=name,
        )
        if parsed is None:
            return None
        keywords.update(parsed.keywords)
        triggers.extend(parsed.triggers)
        if parsed.activated is not None:
            activated.append(parsed.activated)
        statics.extend(parsed.statics)
    return triggers, activated, statics, keywords


def _build_class_printing(card: dict) -> CardPrinting | None:
    supertypes, card_types, subtypes = _parse_type_line(card.get("type_line", ""))
    if "Class" not in subtypes:
        return None
    from mtgz_ir.effects.generic.layout_effects import GainClassLevel

    name = card["name"]
    sections: list[list[str]] = [[]]
    level_costs: list[str] = []
    for raw in (card.get("oracle_text") or "").split("\n"):
        line = _strip_reminder_text(raw)
        if not line:
            continue
        m = _LEVEL_LINE.match(line)
        if m is not None:
            if int(m.group("n")) != len(sections) + 1:
                return None
            level_costs.append(m.group("cost"))
            sections.append([])
            continue
        sections[-1].append(line)
    if len(sections) < 2:
        return None
    parsed = [_parse_section(lines, name) for lines in sections]
    if any(p is None for p in parsed):
        return None
    base = CardPrinting(
        name=name,
        mana_cost_str=card.get("mana_cost", ""),
        mana_cost=parse_mana_cost(card.get("mana_cost", "")),
        type_line=card.get("type_line", ""),
        supertypes=tuple(supertypes),
        card_types=tuple(card_types),
        subtypes=tuple(subtypes),
    )
    levels: list[CardPrinting] = []
    triggers: list = []
    activated: list = []
    statics: list = []
    keywords: set[str] = set()
    for level, (t, a, s, k) in enumerate(parsed, start=1):
        triggers.extend(t)
        activated.extend(a)
        statics.extend(s)
        keywords.update(k)
        level_up: tuple = ()
        if level <= len(level_costs):
            cost = level_costs[level - 1]
            level_up = (ActivatedAbility(
                cost=ActivationCost(mana=parse_mana_cost(cost), mana_str=cost),
                effect=GainClassLevel(level=level + 1),
                sorcery_speed=True,
            ),)
        levels.append(dataclasses.replace(
            base,
            triggered_abilities=tuple(triggers),
            activated_abilities=tuple(activated) + level_up,
            static_abilities=tuple(statics),
            keywords=frozenset(keywords),
            class_level=level,
        ))
    return dataclasses.replace(levels[0], class_levels=tuple(levels[1:]))
