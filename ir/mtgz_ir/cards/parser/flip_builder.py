"""Flip cards (CR 710; ``layout == "flip"``) -- roto package P6.

A flip card's top half is the card as it's cast and enters; its bottom
half is what the permanent becomes when an effect flips it. Both halves
build through the regular per-face dispatch (``dfc_builder.build_face``).
The flipped half keeps the card's mana cost (CR 710.1c / 710.2a: only
the name, text, type line and P/T change). The top half's "flip <this>"
effect needs the flipped printing, so the top half is built inside
:func:`flipping_into`, which lets the P6 effect grammar turn "flip this
creature" into ``FlipSource(flipped=<bottom half>)``; outside a flip
card the phrase doesn't parse.
"""

from __future__ import annotations

import dataclasses
from contextlib import contextmanager

from mtgz_ir.cards.base import CardPrinting, parse_mana_cost

_FLIPPED_FACE: list = [None]


@contextmanager
def flipping_into(flipped: CardPrinting):
    """While building a flip card's top half: "flip this creature"
    flips into ``flipped``."""
    prior = _FLIPPED_FACE[0]
    _FLIPPED_FACE[0] = flipped
    try:
        yield
    finally:
        _FLIPPED_FACE[0] = prior


def current_flipped_face() -> CardPrinting | None:
    return _FLIPPED_FACE[0]


_PERMANENT_TYPES = frozenset({
    "Artifact", "Creature", "Enchantment", "Land", "Planeswalker",
})


def _build_flipped_noncreature_half(face: dict) -> CardPrinting | None:
    """A non-creature flipped half (Erayo's Essence, a Legendary
    Enchantment): every line through the generic clause grammar
    (keywords, triggered, activated, static abilities). The flipped half
    is never a card of its own, so it doesn't go through the per-name
    opt-in gates of the enchantment / artifact builders."""
    from mtgz_ir.cards.parser.clause import _parse_clause
    from mtgz_ir.cards.parser.text import _strip_reminder_text
    from mtgz_ir.cards.parser.type_line import _parse_type_line

    supertypes, card_types, subtypes = _parse_type_line(face.get("type_line", ""))
    keywords: list[str] = []
    triggers: list = []
    activated: list = []
    statics: list = []
    for raw in (face.get("oracle_text") or "").split("\n"):
        line = _strip_reminder_text(raw)
        if not line:
            continue
        clause = _parse_clause(line, name=face["name"])
        if clause is None:
            return None
        keywords.extend(clause.keywords)
        triggers.extend(clause.triggers)
        if clause.activated is not None:
            activated.append(clause.activated)
        statics.extend(clause.statics)
    return CardPrinting(
        name=face["name"],
        mana_cost_str="",
        mana_cost=parse_mana_cost(""),
        type_line=face.get("type_line", ""),
        supertypes=tuple(supertypes),
        card_types=tuple(card_types),
        subtypes=tuple(subtypes),
        keywords=frozenset(keywords),
        triggered_abilities=tuple(triggers),
        activated_abilities=tuple(activated),
        static_abilities=tuple(statics),
    )


def _is_permanent_face(face: dict) -> bool:
    from mtgz_ir.cards.parser.type_line import _parse_type_line

    _, card_types, _ = _parse_type_line(face.get("type_line", ""))
    return bool(card_types) and set(card_types) <= _PERMANENT_TYPES


def _build_flip_printing(card: dict) -> CardPrinting | None:
    if card.get("layout") != "flip":
        return None
    faces = card.get("card_faces") or []
    if len(faces) != 2 or not all(_is_permanent_face(f) for f in faces):
        # CR 710.1: both halves of a flip card are permanents.
        return None
    from mtgz_ir.cards.parser.dfc_builder import build_face

    top_face, bottom_face = faces
    if "Creature" in bottom_face.get("type_line", ""):
        bottom = build_face(bottom_face)
    else:
        bottom = _build_flipped_noncreature_half(bottom_face)
    if bottom is None:
        return None
    cost = top_face.get("mana_cost", "") or card.get("mana_cost", "")
    bottom = dataclasses.replace(
        bottom, mana_cost=parse_mana_cost(cost), mana_cost_str=cost,
    )
    with flipping_into(bottom):
        top = build_face(top_face)
    if top is None:
        return None
    return dataclasses.replace(top, name=card["name"])


__all__ = ["_build_flip_printing", "current_flipped_face", "flipping_into"]
