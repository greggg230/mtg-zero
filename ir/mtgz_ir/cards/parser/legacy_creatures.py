"""Creature lines of the Legacy cards outside the cube (pool ``legacy``).

The mtg-zero Rust engine plays a Legacy Izzet Delver mirror whose 60 cards
are cube cards but for Murktide Regent and Prismari Charm (Prismari Charm
already parses). Murktide Regent's lines:

* ``Delve`` on a creature -> ``CardPrinting.has_delve`` (CR 702.66; the
  spell builder's ``Delve`` line, for a creature spell): since mtg-zero's
  ENGINE.md 220.1 ``legacy_hogaak``'s casting-keyword list recognizer
  ("Convoke, delve" too).
* ``This creature enters with a +1/+1 counter on it for each instant and
  sorcery card exiled with it.`` ->
  ``CardPrinting.enters_with_counter_per_card_exiled_with_it`` = the card
  filter (CR 607.2q: "exiled with it" = the cards exiled to pay the cost of
  the spell that became this permanent; 614.1c).
* ``Whenever an instant or sorcery card leaves your graveyard, put a +1/+1
  counter on this creature.`` -> a ``TriggeredAbility`` on
  ``Event.LEAVES_GRAVEYARD`` (subjects ``player``: the graveyard's owner,
  ``card``: the card as it was there, CR 603.10a).

"<types> card" names printed card types (cards outside the battlefield
have only their printed characteristics): ``HasPrintedCardType`` per
type, joined with ``ObjectOr`` ("instant and sorcery card" = each card
that is an instant or a sorcery, as "instant or sorcery card").

The Python engine doesn't play these two abilities (entering with the
field raises; no dispatch site fires ``LEAVES_GRAVEYARD``): they are data
for the card IR (SEARCH_LIMITATIONS, "Legacy pool" 1).
"""

from __future__ import annotations

import re

_SELF = r"(?:this creature|{name})"
_TYPES = {"instant": "Instant", "sorcery": "Sorcery", "artifact": "Artifact",
          "creature": "Creature", "enchantment": "Enchantment", "land": "Land",
          "planeswalker": "Planeswalker"}


def _card_type_filter(phrase: str):
    """``"instant and sorcery"`` / ``"instant or sorcery"`` / ``"creature"``
    -> the printed-type filter, or ``None``."""
    from mtgz_ir.effects.generic.cube_walkers import HasPrintedCardType
    from mtgz_ir.effects.object_predicates import ObjectOr

    words = re.split(r" (?:and|or) ", phrase.strip().lower())
    if not words or any(w not in _TYPES for w in words) or len(set(words)) != len(words):
        return None
    preds = [HasPrintedCardType(_TYPES[w]) for w in words]
    out = preds[0]
    for p in preds[1:]:
        out = ObjectOr(out, p)
    return out


def _enters_with_counter_per_exiled(line: str, name: str):
    m = re.fullmatch(
        _SELF.format(name=re.escape(name)) + r" enters with a \+1/\+1 counter on it for each "
        r"([a-z ]+?) card exiled with it\.",
        line.strip(),
        flags=re.IGNORECASE,
    )
    if m is None:
        return None
    f = _card_type_filter(m.group(1))
    if f is None:
        return None
    return {"fields": {"enters_with_counter_per_card_exiled_with_it": f}}


def _card_leaves_your_graveyard_counter(line: str, name: str):
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.counters import PutPlusOneCounterOnSelf
    from mtgz_ir.effects.predicates import And, Controlled, Event, Subject

    m = re.fullmatch(
        r"whenever an? ([a-z ]+?) card leaves your graveyard, put a \+1/\+1 counter on "
        + _SELF.format(name=re.escape(name)) + r"\.",
        line.strip(),
        flags=re.IGNORECASE,
    )
    if m is None:
        return None
    f = _card_type_filter(m.group(1))
    if f is None:
        return None
    trig = TriggeredAbility(
        event=Event.LEAVES_GRAVEYARD,
        predicate=And(Subject(slot="card", inner=f), Controlled(subject="player")),
        effect=PutPlusOneCounterOnSelf(),
    )
    return {"triggers": (trig,)}


_PARSERS = (_enters_with_counter_per_exiled, _card_leaves_your_graveyard_counter)


def parse_legacy_creature_line(line: str, name: str):
    """What ``line`` contributes to a creature printing (a ``UBLine``), or
    ``None`` when no recognizer claims it."""
    from mtgz_ir.cards.parser.cube_creatures_ub import UBLine

    for parse in _PARSERS:
        out = parse(line, name)
        if out is not None:
            return UBLine(triggers=out.get("triggers", ()), fields=out.get("fields"))
    return None
