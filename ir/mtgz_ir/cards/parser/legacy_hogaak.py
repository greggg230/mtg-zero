"""Hogaak, Arisen Necropolis's lines (pool ``legacy``; mtg-zero's Legacy Hogaak
Tempo deck, MTGGoldfish 8027954, ENGINE.md 220.1).

Hogaak, Arisen Necropolis {5}{B/G}{B/G}, Legendary Creature - Avatar 8/8:
"You can't spend mana to cast this spell. / Convoke, delve / You may cast this
card from your graveyard. / Trample". Trample is an ordinary keyword line; the
other three:

* A keyword line listing the casting keywords convoke and delve (CR 702.51,
  702.66), alone or comma-joined in any order ("Convoke, delve"; Murktide
  Regent's "Delve") -> ``CardPrinting.has_convoke`` / ``has_delve``: each a
  static ability of the spell while it is on the stack. Parameterized: one
  recognizer for every list of the two (Murktide Regent's lone "Delve" claims
  through it, as it did through ``legacy_creatures``' recognizer, which this
  one replaced).
* ``You can't spend mana to cast this spell.`` ->
  ``CardPrinting.cant_spend_mana_to_cast`` (CR 113.6e, 601.2f-h). A literal:
  the only card of the pools with this surface; broader grammar ("... to
  cast <object>") deferred until a second example exists.
* ``You may cast this card from your graveyard.`` ->
  ``CardPrinting.castable_from_graveyard`` (CR 601.3, 113.6f). A literal: the
  only unconditional one of the pools (Timeline Culler's "... using its warp
  ability" is ``printing_keywords``' own; Undead Sprinter's conditional one is
  unclaimed); broader grammar (a condition, a cost rider) deferred until a
  second example exists.

The Python engine doesn't play these fields: they are data for the card IR.
"""

from __future__ import annotations

import re

# The casting keywords a creature line may list (CR 702.51, 702.66), each
# with the printing flag it sets.
_CASTING_KEYWORDS = {"convoke": "has_convoke", "delve": "has_delve"}

_CASTING_KEYWORD_LIST_RE = re.compile(
    r"^(?P<first>[A-Za-z]+)(?P<rest>(?:, [a-z]+)*)$"
)

# The only card of the pools with this surface (see the module docs).
_CANT_SPEND_MANA_LINE = "You can't spend mana to cast this spell."

# The only unconditional one of the pools (see the module docs).
_CAST_FROM_GRAVEYARD_LINE = "You may cast this card from your graveyard."


def _casting_keyword_list(line: str):
    """``Convoke, delve`` / ``Delve`` / ``Convoke``: the flags, or ``None``
    (a word that isn't one of the two, a repeated one, or a lowercase first
    word: a keyword line is capitalized only at its start)."""
    m = _CASTING_KEYWORD_LIST_RE.fullmatch(line.strip())
    if m is None or not m.group("first")[0].isupper():
        return None
    words = [m.group("first").lower()] + [w.strip() for w in m.group("rest").split(",") if w.strip()]
    if len(set(words)) != len(words) or any(w not in _CASTING_KEYWORDS for w in words):
        return None
    return {"fields": {_CASTING_KEYWORDS[w]: True for w in words}}


def _cant_spend_mana(line: str):
    if line.strip() == _CANT_SPEND_MANA_LINE:
        return {"fields": {"cant_spend_mana_to_cast": True}}
    return None


def _cast_from_graveyard(line: str):
    if line.strip() == _CAST_FROM_GRAVEYARD_LINE:
        return {"fields": {"castable_from_graveyard": True}}
    return None


_PARSERS = (_casting_keyword_list, _cant_spend_mana, _cast_from_graveyard)


def parse_legacy_hogaak_line(line: str, name: str):
    """What ``line`` (reminder text stripped) contributes to a creature
    printing (a ``UBLine`` of printing fields), or ``None`` when no
    recognizer claims it."""
    from mtgz_ir.cards.parser.cube_creatures_ub import UBLine

    del name  # (the lines name no object by its card name)
    for parse in _PARSERS:
        out = parse(line)
        if out is not None:
            return UBLine(fields=out["fields"])
    return None
