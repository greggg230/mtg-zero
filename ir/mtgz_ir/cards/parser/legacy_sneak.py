"""Spell lines of the Legacy cards outside the cube (pool ``legacy``):
mtg-zero's Legacy Sneak and Show deck (MTGGoldfish 8014220).

Show and Tell (``EachPlayerMayPutFromHand``), Omniscience
(``grants_free_casts_from_hand``), Mistrise Village
(``NextSpellThisTurnCantBeCountered``), Disruptor Flute
(``ChooseCardNameAsEnters``, ``SpellTax(named)``,
``ActivationProhibition``) and Magus of the Moon
(``nonbasic_lands_are_mountains``) parse through the existing roto and EoE
recognizers. Brotherhood's End's first mode needed one parameterized
recognizer, tried after every other grammar (``effects._parse_effect``):

* ``deals <N> damage to each creature and each planeswalker`` ->
  ``DealNDamageToEachCreatureAndPlaneswalker(amount=N)`` (its second mode,
  "destroy all artifacts with mana value 3 or less", is the existing
  ``RemoveAllMatching``).
"""

from __future__ import annotations

import re

_EACH_CREATURE_AND_PLANESWALKER_RE = re.compile(
    r"^deals (?P<n>\d+) damage to each creature and each planeswalker$"
)


def _each_creature_and_planeswalker(phrase: str):
    from mtgz_ir.effects.generic.legacy_sneak import (
        DealNDamageToEachCreatureAndPlaneswalker,
    )

    m = _EACH_CREATURE_AND_PLANESWALKER_RE.match(phrase)
    if m is None:
        return None
    return DealNDamageToEachCreatureAndPlaneswalker(amount=int(m.group("n")))


_RECOGNIZERS = (_each_creature_and_planeswalker,)


def parse_legacy_sneak_effect(phrase: str):
    """The effect of a *normalized* phrase (``effects._parse_effect``'s
    form: no trailing period, first letter lowercased, the card's name
    stripped as the subject), or ``None``."""
    for parse in _RECOGNIZERS:
        eff = parse(phrase)
        if eff is not None:
            return eff
    return None
