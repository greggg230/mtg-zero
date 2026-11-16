"""Split card builder (CR 709) -- cube layouts bucket.

A split card (Life // Death) is one card with two halves, each an
instant or sorcery with its own name, mana cost and effect. Each half is
built on its own through the regular spell builder (a half is a card
record with ``layout == "normal"``). The card's printing carries the
combined characteristics it has in every zone but the stack (CR 709.4:
both names, the combined mana cost -- so its mana value is the total --
and both halves' types) with no effect of its own, and the halves in
``split_faces``; it is always cast as one half
(``CastSpell(cast_face=<half>)``, ``mtgz_ir.layouts``).

Duskmourn Rooms share Scryfall's ``split`` layout but are permanents
with doors; they stay with ``room_builder`` (a split card here must have
two instant / sorcery halves).
"""

from __future__ import annotations

import dataclasses

from mtgz_ir.cards.base import CardPrinting, ManaCost
from mtgz_ir.cards.parser.dfc_builder import build_face
from mtgz_ir.cards.parser.type_line import _parse_type_line


def _combined_cost(a: ManaCost, b: ManaCost) -> ManaCost:
    colored: dict[str, int] = dict(a.colored)
    for c, n in b.colored:
        colored[c] = colored.get(c, 0) + n
    return ManaCost(
        generic=a.generic + b.generic,
        colored=tuple(sorted(colored.items())),
        x_pips=a.x_pips + b.x_pips,
        hybrid=a.hybrid + b.hybrid,
        phyrexian=a.phyrexian + b.phyrexian,
    )


def _build_split_printing(card: dict) -> CardPrinting | None:
    if card.get("layout") != "split":
        return None
    faces = card.get("card_faces") or []
    if len(faces) != 2:
        return None
    halves: list[CardPrinting] = []
    for face in faces:
        _, types, _ = _parse_type_line(face.get("type_line", ""))
        if "Instant" not in types and "Sorcery" not in types:
            return None
        half = build_face(face)
        if half is None:
            return None
        halves.append(half)
    first, second = halves
    card_types = tuple(dict.fromkeys(first.card_types + second.card_types))
    return dataclasses.replace(
        first,
        name=card["name"],
        mana_cost_str=card.get("mana_cost", ""),
        mana_cost=_combined_cost(first.mana_cost, second.mana_cost),
        type_line=card.get("type_line", ""),
        card_types=card_types,
        spell_effect=None,
        split_faces=(first, second),
    )
