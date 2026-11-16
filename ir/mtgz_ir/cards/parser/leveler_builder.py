"""Leveler builder (CR 711, 702.87) -- cube layouts bucket.

A leveler's oracle text is ``Level up {cost}`` plus any other abilities,
then level-symbol sections: a ``LEVEL a-b`` / ``LEVEL a+`` header, the
section's power/toughness, and the abilities the creature has while its
level-counter count is in that range.

* ``Level up {cost}`` -> an activated ability, sorcery speed, putting a
  level counter on the creature (``PutNamedCountersOnSelf("level")``).
* Each section's P/T -> ``LevelerPT`` (a structured
  ``dynamic_pt_formula``: base P/T by level range, else the printed P/T;
  CR 711.2b).
* Each section's keyword lines -> SELF static abilities granting the
  keyword while ``LevelCountersInRange`` holds (CR 711.2a). A section
  ability that isn't a keyword isn't modeled, so such a card isn't
  claimed.
"""

from __future__ import annotations

import dataclasses
import re

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CardPrinting,
    StaticAbility,
    StaticScope,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.text import _strip_reminder_text

_LEVEL_UP = re.compile(r"^Level up (?P<cost>(?:\{[^}]+\})+)$")
_LEVEL_HEADER = re.compile(r"^LEVEL (?P<lo>\d+)(?:-(?P<hi>\d+)|(?P<plus>\+))$")
_PT_LINE = re.compile(r"^(?P<p>\d+)/(?P<t>\d+)$")


def _build_leveler_printing(card: dict) -> CardPrinting | None:
    if card.get("layout") != "leveler":
        return None
    from mtgz_ir.cards.parser.dfc_builder import build_face
    from mtgz_ir.cards.parser.keywords import _parse_keyword_line
    from mtgz_ir.effects.generic.artifact_mechanics import PutNamedCountersOnSelf
    from mtgz_ir.effects.generic.layout_effects import (
        LevelCountersInRange,
        LevelerPT,
    )
    from mtgz_ir.effects.modifiers import ContinuousModifier, GrantKeyword

    lines = [
        _strip_reminder_text(raw)
        for raw in (card.get("oracle_text") or "").split("\n")
    ]
    lines = [ln for ln in lines if ln]
    base_lines: list[str] = []
    level_up_cost: str | None = None
    i = 0
    while i < len(lines) and _LEVEL_HEADER.match(lines[i]) is None:
        m = _LEVEL_UP.match(lines[i])
        if m is not None:
            level_up_cost = m.group("cost")
        else:
            base_lines.append(lines[i])
        i += 1
    if level_up_cost is None:
        return None
    ranges: list[tuple[int, int | None, int, int]] = []
    statics: list[StaticAbility] = []
    while i < len(lines):
        header = _LEVEL_HEADER.match(lines[i])
        if header is None or i + 1 >= len(lines):
            return None
        pt = _PT_LINE.match(lines[i + 1])
        if pt is None:
            return None
        lo = int(header.group("lo"))
        hi = int(header.group("hi")) if header.group("hi") else None
        ranges.append((lo, hi, int(pt.group("p")), int(pt.group("t"))))
        i += 2
        while i < len(lines) and _LEVEL_HEADER.match(lines[i]) is None:
            kws = _parse_keyword_line(lines[i])
            if kws is None:
                return None
            for kw in kws:
                statics.append(StaticAbility(
                    scope=StaticScope.SELF,
                    condition=LevelCountersInRange(lo=lo, hi=hi),
                    continuous_effect=ContinuousModifier(
                        layer=6, payload=GrantKeyword(kw),
                    ),
                ))
            i += 1
    if not ranges:
        return None
    base = build_face(dict(
        card, oracle_text="\n".join(base_lines), layout="normal",
    ))
    if base is None or not base.is_creature:
        return None
    level_up = ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost(level_up_cost), mana_str=level_up_cost,
        ),
        effect=PutNamedCountersOnSelf(kind="level", amount=1),
        sorcery_speed=True,
    )
    return dataclasses.replace(
        base,
        activated_abilities=tuple(base.activated_abilities) + (level_up,),
        static_abilities=tuple(base.static_abilities) + tuple(statics),
        dynamic_pt_formula=LevelerPT(ranges=tuple(ranges)),
    )
