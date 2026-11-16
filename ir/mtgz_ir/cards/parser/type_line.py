"""Scryfall type-line parser.

Splits a string like ``"Legendary Creature — Human Soldier"`` into
``(supertypes, card_types, subtypes)``. The em-dash separator is
U+2014 (``—``). Empty tuple for any section not present.
"""

from __future__ import annotations


_SUPERTYPES = frozenset({"Basic", "Legendary", "Snow", "World", "Ongoing"})

_TYPE_LINE_SEPARATOR = "—"  # em-dash, used by Scryfall


def _parse_type_line(
    text: str,
) -> tuple[tuple[str, ...], tuple[str, ...], tuple[str, ...]]:
    """Split a Scryfall type line into ``(supertypes, card_types,
    subtypes)``. Empty tuple for any section not present.

    Examples: ``"Creature — Dragon"`` →
    ``((), ("Creature",), ("Dragon",))``;
    ``"Legendary Creature — Human Soldier"`` →
    ``(("Legendary",), ("Creature",), ("Human", "Soldier"))``;
    ``"Sorcery"`` → ``((), ("Sorcery",), ())``.
    """
    if _TYPE_LINE_SEPARATOR in text:
        left, right = text.split(_TYPE_LINE_SEPARATOR, 1)
    else:
        left, right = text, ""
    left_words = left.split()
    supertypes = tuple(w for w in left_words if w in _SUPERTYPES)
    card_types = tuple(w for w in left_words if w not in _SUPERTYPES)
    subtypes = tuple(right.split())
    return supertypes, card_types, subtypes
