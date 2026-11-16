"""Adventurer cards (CR 715) and preparation cards (Secrets of
Strixhaven "prepare") -- two different two-face layouts whose card is a
creature card in every zone but the stack.

* ``layout == "adventure"`` (Brazen Borrower // Petty Theft): the creature
  face's printing (keyed by the full card name) carries the Adventure half
  in ``adventure_face``. The card may be cast as either; a spell cast as
  an Adventure is exiled as it resolves and its controller may later cast
  the creature from exile (``layouts.exile_on_adventure``).
* ``layout == "prepare"`` (Emeritus of Abundance // Regrowth): the second
  face is the creature's *prepare spell* (``prepare_spell``). The card can
  only be cast as the creature (its base characteristics); when the
  creature becomes prepared its controller gets a castable copy of the
  prepare spell in exile (``layouts.become_prepared``).

Both faces must build through the regular builders (a face is a card
record with ``layout == "normal"``) or the card isn't claimed.
"""

from __future__ import annotations

import dataclasses

from mtgz_ir.cards.base import CardPrinting
from mtgz_ir.cards.parser.dfc_builder import build_face
from mtgz_ir.cards.parser.type_line import _parse_type_line


def _faces(card: dict, layout: str):
    if card.get("layout") != layout:
        return None
    faces = card.get("card_faces") or []
    if len(faces) != 2:
        return None
    creature_face, spell_face = faces
    _, ctypes, _ = _parse_type_line(creature_face.get("type_line", ""))
    _, stypes, _ = _parse_type_line(spell_face.get("type_line", ""))
    if "Creature" not in ctypes or not ({"Instant", "Sorcery"} & set(stypes)):
        return None
    creature = build_face(creature_face)
    spell = build_face(spell_face)
    if creature is None or spell is None:
        return None
    return creature, spell


def _build_adventure_printing(card: dict) -> CardPrinting | None:
    built = _faces(card, "adventure")
    if built is None:
        return None
    creature, spell = built
    if "Adventure" not in spell.subtypes:
        return None
    return dataclasses.replace(creature, name=card["name"], adventure_face=spell)


def _build_prepare_printing(card: dict) -> CardPrinting | None:
    built = _faces(card, "prepare")
    if built is None:
        return None
    creature, spell = built
    return dataclasses.replace(creature, name=card["name"], prepare_spell=spell)
