"""Counterspell phrase recognizer built on the spell noun-phrase grammar
(:func:`mtgz_ir.cards.parser.target_phrases.parse_spell_phrase`).

Shape (normalized phrase, dispatched by
:mod:`mtgz_ir.cards.parser.precise_grammar`)::

    counter target <spell np>[ unless its controller pays {N}][ if it's <color>]
    [. If that spell is countered this way, <destination> instead ...]

Destinations: ``exile it instead of putting it into its owner's
graveyard`` (Force of Negation, No More Lies), ``put it on top of its
owner's library instead of into that player's graveyard`` (Memory
Lapse), ``put it into its owner's hand instead of into that player's
graveyard`` (Remand). All map onto :class:`CounterSpell`.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.parser.removal_phrases import split_sentences
from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.generic.counterspells import CounterSpell
from mtgz_ir.effects.value_predicates import HasColor


_COUNTER_RE = re.compile(
    r"^counter target (?P<np>.+?)"
    r"(?: unless its controller pays (?P<cost>(?:\{[^}]+\})+))?"
    r"(?: if it's (?P<color>white|blue|black|red|green))?$"
)
_COLOR_LETTERS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
_DESTINATION_RIDERS: dict[str, str] = {
    "if that spell is countered this way, exile it instead of putting "
    "it into its owner's graveyard": "exile",
    "if that spell is countered this way, put it on top of its owner's "
    "library instead of into that player's graveyard": "library_top",
    "if that spell is countered this way, put it into its owner's hand "
    "instead of into that player's graveyard": "hand",
}


def parse_counter(phrase: str) -> Effect | None:
    sentences = split_sentences(phrase)
    if not sentences:
        return None
    m = _COUNTER_RE.match(sentences[0])
    if m is None:
        return None
    ok, spell_filter = parse_spell_phrase(m.group("np"))
    if not ok:
        return None
    destination = "graveyard"
    for sentence in sentences[1:]:
        dest = _DESTINATION_RIDERS.get(sentence)
        if dest is None or destination != "graveyard":
            return None
        destination = dest
    color = m.group("color")
    return CounterSpell(
        spell_filter=spell_filter,
        unless_pays=m.group("cost") or "",
        only_if=HasColor(_COLOR_LETTERS[color]) if color else None,
        destination=destination,
        label=m.group("np") + (f" if it's {color}" if color else ""),
    )


__all__ = ["parse_counter"]
