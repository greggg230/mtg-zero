"""Spell lines of the Legacy cards outside the cube (pool ``legacy``):
mtg-zero's Legacy Doomsday deck, lane A (Doomsday, Pact of Negation,
Personal Tutor, Flow State).

Doomsday (``DoomsdayPile``), Flow State
(``LookAtTopPutIntoHandMoreIfInstantAndSorcery``) and Personal Tutor
(``SearchLibrary``, Mystical Tutor's shape) parse through existing
recognizers. Pact of Negation's second line needed two parameterized
recognizers, tried after every other grammar (``effects._parse_effect``):

* ``At the beginning of your next upkeep, <effect>.`` ->
  ``ScheduleDelayedEffect(inner=<effect>, step=DelayedStep.YOUR_UPKEEP)``
  (CR 603.7: a delayed triggered ability the spell creates as it
  resolves; "your" = its controller, 603.7d). The inner effect must not
  target (a delayed trigger choosing targets is another shape).
* ``Pay <mana cost>. If you don't, <effect>.`` ->
  ``PayManaOrElse(cost, otherwise=<effect>)`` (the player may pay; if they
  don't, the effect; CR 118.12a reads "unless" the same way).

And one printing fix for every single-faced card: the Scryfall record's
``color_indicator`` (CR 204, 202.2e: Pact of Negation is blue though its
mana cost is {0}) becomes ``CardPrinting.color_indicator``
(:func:`with_color_indicator`). A back face's indicator already reached
the printing as ``face_colors``.
"""

from __future__ import annotations

import re
from dataclasses import replace

_AT_YOUR_NEXT_UPKEEP_RE = re.compile(r"^at the beginning of your next upkeep, (?P<effect>.+)$")
# (Generic and coloured pips only: a cost with {X}, hybrid or Phyrexian
# pips is another shape, left unclaimed.)
_PAY_OR_ELSE_RE = re.compile(r"^pay (?P<cost>(?:\{(?:\d+|[WUBRGC])\})+)\. If you don't, (?P<otherwise>.+)$")


def _untargeted(phrase: str):
    from mtgz_ir.cards.parser.effects import _parse_effect

    eff = _parse_effect(phrase)
    if eff is None or eff.requires_target():
        return None
    return eff


def _at_your_next_upkeep(phrase: str):
    from mtgz_ir.effects.generic.artifact_mechanics import ScheduleDelayedEffect
    from mtgz_ir.effects.vocab import DelayedStep

    m = _AT_YOUR_NEXT_UPKEEP_RE.match(phrase)
    if m is None:
        return None
    inner = _untargeted(m.group("effect"))
    if inner is None:
        return None
    return ScheduleDelayedEffect(inner=inner, step=DelayedStep.YOUR_UPKEEP)


def _pay_or_else(phrase: str):
    from mtgz_ir.effects.generic.legacy_spells import PayManaOrElse

    m = _PAY_OR_ELSE_RE.match(phrase)
    if m is None:
        return None
    cost = m.group("cost")
    otherwise = _untargeted(m.group("otherwise"))
    if otherwise is None:
        return None
    return PayManaOrElse(cost=cost, otherwise=otherwise)


_RECOGNIZERS = (_at_your_next_upkeep, _pay_or_else)


def parse_legacy_spell_effect(phrase: str):
    """The effect of a *normalized* phrase (``effects._parse_effect``'s
    form: no trailing period, first letter lowercased), or ``None``."""
    for parse in _RECOGNIZERS:
        eff = parse(phrase)
        if eff is not None:
            return eff
    return None


def with_color_indicator(card: dict, printing):
    """``printing`` with the single-faced ``card``'s color indicator (CR
    204.1, 202.2e: the object is each colour it denotes), when the record
    has one and the printing doesn't yet."""
    ci = card.get("color_indicator")
    if not ci or card.get("card_faces") or printing.color_indicator:
        return printing
    return replace(printing, color_indicator=frozenset(ci))
