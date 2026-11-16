"""Spell lines of the Legacy cards outside the cube (pool ``legacy``): the
Legacy Izzet Delver 75's sideboard (mtg-zero's Delver mirror, MTGGoldfish
8016541).

Consign to Memory (``CounterTargetStackObjectFiltered`` with
``replicate_cost``), Hydroblast (Pyroblast's ``ModalChooseOne`` with red),
Meltdown (``RemoveAllMatching`` with ``max_mana_value_from_x``) and Price
of Progress (``DamageEachPlayerPerControlled``) parse through the existing
roto recognizers. Rough // Tumble's halves needed one parameterized
recognizer, tried after every other grammar (``effects._parse_effect``):

* ``deals <N> damage to each creature with <qualifier>`` ->
  ``DealNDamageToEachCreatureMatching(amount=N, creature_filter=<q>)``;
  ``... without <keyword>`` -> the keyword predicate negated. The
  qualifier is the target-phrase grammar's ``with`` tail
  (``effects_ext._parse_with_qualifier``: a keyword or a keyword
  disjunction, mana value or power N or greater / less); "without" takes
  keywords only.
"""

from __future__ import annotations

import re

_EACH_CREATURE_WITH_RE = re.compile(
    r"^deals (?P<n>\d+) damage to each creature (?P<w>with|without) (?P<q>.+)$"
)


def _keyword_qualifier(text: str):
    """A keyword or a keyword disjunction (``flying``, ``flying or
    reach``) as an object predicate, or ``None``."""
    from mtgz_ir.cards.parser.effects_ext import _keyword_names
    from mtgz_ir.effects.object_predicates import HasKeyword

    kws = [k.strip() for k in text.split(" or ")]
    known = _keyword_names()
    if not kws or not all(k in known for k in kws):
        return None
    pred = HasKeyword(kws[0])
    for k in kws[1:]:
        pred = pred | HasKeyword(k)
    return pred


def _each_creature_with(phrase: str):
    from mtgz_ir.cards.parser.effects_ext import _parse_with_qualifier
    from mtgz_ir.effects.generic.legacy_delver_sb import (
        DealNDamageToEachCreatureMatching,
    )

    m = _EACH_CREATURE_WITH_RE.match(phrase)
    if m is None:
        return None
    if m.group("w") == "with":
        pred = _parse_with_qualifier(m.group("q"))
    else:
        kw = _keyword_qualifier(m.group("q"))
        pred = None if kw is None else ~kw
    if pred is None:
        return None
    return DealNDamageToEachCreatureMatching(
        amount=int(m.group("n")), creature_filter=pred,
    )


_RECOGNIZERS = (_each_creature_with,)


def parse_legacy_delver_sb_effect(phrase: str):
    """The effect of a *normalized* phrase (``effects._parse_effect``'s
    form: no trailing period, first letter lowercased, the card's name
    stripped as the subject), or ``None``."""
    for parse in _RECOGNIZERS:
        eff = parse(phrase)
        if eff is not None:
            return eff
    return None
