"""Double-faced card builder (CR 712) -- cube layouts bucket.

Scryfall stores a double-faced card as one record whose ``card_faces``
array carries each face's name, mana cost, type line, oracle text,
P/T / loyalty and colors. Each face is built on its own through the
regular per-type builders (a face is just a card record with
``layout == "normal"``), then the two are joined:

* ``layout == "transform"`` -- the front face's printing (renamed to the
  full card name, the pool / deck key) carries the back face in
  ``transform_back_face``. The back face borrows the front face's mana
  cost for its mana value (CR 712.8e) and takes its colors from its
  color indicator (``face_colors``).
* ``layout == "modal_dfc"`` with a permanent front face -- the back face
  is either a land the card may be played as (``mdfc_back_face``, the
  base's spell // land substrate) or a castable face of its own
  (``mdfc_back_face`` again; ``legal_actions`` offers casting it). A
  front face with a "Transform ~" ability also gets the back face as
  ``transform_back_face``. Modal DFCs whose front face is an instant or
  sorcery stay with ``spell_builder._build_mdfc_spell_printing``.

The whole card is claimed only if both faces build.
"""

from __future__ import annotations

import dataclasses

from mtgz_ir.cards.base import CardPrinting, parse_mana_cost
from mtgz_ir.cards.parser.type_line import _parse_type_line

_FACE_KEYS = (
    "name", "mana_cost", "type_line", "oracle_text", "power", "toughness",
    "loyalty", "colors",
)


def face_record(face: dict) -> dict:
    """A single-face Scryfall-shaped record for ``face``."""
    rec = {k: face[k] for k in _FACE_KEYS if face.get(k) is not None}
    rec.setdefault("mana_cost", "")
    rec.setdefault("oracle_text", "")
    rec["layout"] = "normal"
    return rec


_DAY_NIGHT_LINES = {"Daybound": "daybound", "Nightbound": "nightbound"}


def build_face(face: dict) -> CardPrinting | None:
    """Build one face through the regular builder dispatch. The
    Daybound / Nightbound keyword lines (CR 702.145) become the
    printing's ``daybound`` / ``nightbound`` flags -- the day/night cycle
    in ``mtgz_ir.layouts`` implements them."""
    from mtgz_ir.cards.parser.source import _build_printing, _split_hand_abilities
    from mtgz_ir.cards.parser.source import _with_hand_abilities
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    rec = face_record(face)
    flags: dict = {}
    kept: list[str] = []
    for raw in rec["oracle_text"].split("\n"):
        flag = _DAY_NIGHT_LINES.get(_strip_reminder_text(raw))
        if flag is not None:
            flags[flag] = True
        else:
            kept.append(raw)
    rec["oracle_text"] = "\n".join(kept)
    from mtgz_ir.cards.parser.roto_keywords import build_with_p6_lines

    body, hand_abilities = _split_hand_abilities(rec)
    built = build_with_p6_lines(body, _build_printing)
    if built is None:
        return None
    if flags:
        built = dataclasses.replace(built, **flags)
    return _with_hand_abilities(built, hand_abilities)


def _has_transform_ability(printing: CardPrinting) -> bool:
    from mtgz_ir.effects.generic.layout_effects import TransformSource

    return any(
        isinstance(a.effect, TransformSource)
        for a in printing.activated_abilities
    )


def transform_back(front_face: dict, back: CardPrinting, back_face: dict) -> CardPrinting:
    """``back`` as a transforming back face: the front face's mana cost
    (mana value, CR 712.8e) and the back face's own colors."""
    front_cost = front_face.get("mana_cost", "")
    return dataclasses.replace(
        back,
        mana_cost=parse_mana_cost(front_cost),
        mana_cost_str="",
        face_colors=frozenset(back_face.get("colors") or ()),
    )


def _build_dfc_printing(card: dict) -> CardPrinting | None:
    layout = card.get("layout")
    if layout not in ("transform", "modal_dfc"):
        return None
    faces = card.get("card_faces") or []
    if len(faces) != 2:
        return None
    front_face, back_face = faces
    _, front_types, _ = _parse_type_line(front_face.get("type_line", ""))
    if layout == "modal_dfc" and (
        "Instant" in front_types or "Sorcery" in front_types
    ):
        # Spell // land modal DFCs: ``spell_builder`` owns them.
        return None
    front = build_face(front_face)
    if front is None:
        return None
    _, back_types, _ = _parse_type_line(back_face.get("type_line", ""))
    if layout == "modal_dfc":
        if "Land" in back_types:
            from mtgz_ir.cards.parser.spell_builder import _build_mdfc_land_face

            back = _build_mdfc_land_face(back_face)
            if back is None:
                return None
            return dataclasses.replace(
                front, name=card["name"], mdfc_back_face=back,
            )
        back = build_face(back_face)
        if back is None:
            return None
        return dataclasses.replace(
            front,
            name=card["name"],
            mdfc_back_face=back,
            transform_back_face=back if _has_transform_ability(front) else None,
        )
    back = build_face(back_face)
    if back is None:
        return None
    return dataclasses.replace(
        front,
        name=card["name"],
        transform_back_face=transform_back(front_face, back, back_face),
    )
