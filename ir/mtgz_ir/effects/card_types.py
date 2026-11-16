"""Card types of cards outside the battlefield and the stack -- the one
shared definition behind every "card types among cards in <zone>" count.

Consumers: Delirium (``state_predicates.Delirium`` for trigger gates,
``conditions.DeliriumActive`` for statics / spell riders / activation
restrictions -- both delegate to :func:`delirium_active`), the
Lhurgoyfs' dynamic P/T (Pyrogoyf, Barrowgoyf), Ursine Monstrosity's
pump, Keen-Eyed Curator's exile count, Occult Epiphany's token count
and Atraxa, Grand Unifier's one-per-type pick.

Rules encoded here (the per-card helpers these replace each got some of
them wrong or left them out):

* CR 205.2a -- the card types are artifact, battle, creature,
  enchantment, instant, kindred, land, planeswalker and sorcery.
  Supertypes (legendary, basic, snow) and subtypes never count. The
  pre-2024 name of kindred, "Tribal", is folded into ``"Kindred"`` so
  an old-wording type line counts as the same type.
* CR 709.4 -- a split card outside the stack has the combined
  characteristics of both halves, so Commit // Memory contributes
  instant AND sorcery.
* CR 712.8a -- a double-faced card (transforming or modal) outside the
  battlefield and stack has only its front face's characteristics:
  Shatterskull Smashing in a graveyard is a sorcery card, not a land.
  The engine turns such cards front-face up as they leave the
  battlefield / stack (``game.restore_mdfc_front``); reading the
  snapshot here as well keeps the count right even on a zone move that
  forgot to restore it.
* CR 715.4 -- an adventurer card outside the stack has only its normal
  characteristics (Brazen Borrower in a graveyard is a creature card).
* Tokens and copies of spells are not cards (CR 108.2, 707.10a); the
  engine never leaves them in a graveyard / exile, and they are skipped
  here regardless.

The count is always taken from the zone's current contents at the
moment the caller asks, so statics see it continuously and spells /
abilities see it as they resolve.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Iterable

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


# CR 205.2a, alphabetical (the order the rules list them in).
CARD_TYPES: tuple[str, ...] = (
    "Artifact", "Battle", "Creature", "Enchantment", "Instant",
    "Kindred", "Land", "Planeswalker", "Sorcery",
)
_CARD_TYPE_SET = frozenset(CARD_TYPES)
# Renamed card types: old type-line word -> current name.
_RENAMED_TYPES: dict[str, str] = {"Tribal": "Kindred"}

# "Delirium -- ... if there are four or more card types among cards in
# your graveyard" (CR 207.2c ability word; the number is printed on every
# delirium card, so it is carried as a field on the condition objects
# rather than hidden in their code).
DELIRIUM_THRESHOLD = 4


def _normalize(card_type: str) -> str | None:
    t = _RENAMED_TYPES.get(card_type, card_type)
    return t if t in _CARD_TYPE_SET else None


def card_types_of(card: "GameObject") -> frozenset[str]:
    """The card types ``card`` has in a zone other than the battlefield
    and the stack (see the module docstring for the multi-face rules).
    Empty for a token or a copy of a spell (not cards)."""
    printing = getattr(card, "mdfc_front_printing", None) or card.printing
    if getattr(printing, "is_token", False) or getattr(card, "is_spell_copy", False):
        return frozenset()
    types: set[str] = set()
    for t in printing.card_types:
        norm = _normalize(t)
        if norm is not None:
            types.add(norm)
    for half in getattr(printing, "split_faces", ()):
        for t in half.card_types:
            norm = _normalize(t)
            if norm is not None:
                types.add(norm)
    return frozenset(types)


def distinct_card_types(cards: Iterable["GameObject"]) -> frozenset[str]:
    """Every card type among ``cards`` (CR 205.2a; each type counted
    once however many cards carry it)."""
    seen: set[str] = set()
    for card in cards:
        seen |= card_types_of(card)
    return frozenset(seen)


def count_card_types(cards: Iterable["GameObject"]) -> int:
    """How many different card types are among ``cards``."""
    return len(distinct_card_types(cards))


def delirium_active(
    state: "GameState", player_id: int,
    threshold: int = DELIRIUM_THRESHOLD,
) -> bool:
    """``True`` when there are ``threshold`` or more card types among
    cards in ``player_id``'s graveyard ("your graveyard" is the
    graveyard of the player the condition is evaluated for -- the
    source's controller for statics / triggers, the caster for spells).
    Short-circuits once the threshold is reached."""
    try:
        player = state.player_by_id(player_id)
    except (KeyError, AttributeError):
        return False
    seen: set[str] = set()
    for card in player.graveyard:
        seen |= card_types_of(card)
        if len(seen) >= threshold:
            return True
    return len(seen) >= threshold


__all__ = [
    "CARD_TYPES",
    "DELIRIUM_THRESHOLD",
    "card_types_of",
    "count_card_types",
    "delirium_active",
    "distinct_card_types",
]
