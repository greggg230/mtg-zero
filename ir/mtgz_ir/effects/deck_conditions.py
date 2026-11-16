"""Companion (CR 702.139) and the deck conditions it names.

Companion is a keyword ability that functions outside the game, written
"Companion — [Condition]." (CR 702.139a). Before the game, a player may
reveal one card they own from outside the game (their sideboard, CR
400.11a) with a companion ability whose condition their starting deck
fulfills (CR 103.2b); "starting deck" is the deck after the sideboard is
set aside (CR 103.2a, 702.139b). Once during the game, any time they have
priority and the stack is empty during a main phase of their turn, they
may pay {3} and put that card into their hand: a special action (CR
116.2g). The {3} is a rule, not part of the printed condition, so the IR
carries only the condition.

A :class:`DeckCondition` is a structured condition over the cards of a
starting deck. The two shapes here cover the cube's companions and leave
room for others (Gyruda, Keruga, Obosh, Kaheera: :class:`EachCardInStartingDeck`
with another pair of predicates; Umori, Yorion, Jegantha would add a
class each):

* :class:`EachCardInStartingDeck` - "Each <cards> in your starting deck
  <must>": Lurrus of the Dream-Den (each permanent card has mana value 2
  or less), Zirda, the Dawnwaker (each permanent card has an activated
  ability).
* :class:`StartingDeckDifferentNames` - "Each <cards> in your starting
  deck has a different name": Lutri, the Spellchaser (nonland cards).

The Python engine does not play companions: ``CardPrinting.companion``
is data for the card IR (the Rust engine in mtg-zero implements the
rules); no game code reads it. :meth:`DeckCondition.fulfilled_by` is the
reference check over a deck's ``GameObject`` s (cards in a library, whose
characteristics are their printed ones).
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass

from mtgz_ir.effects.object_predicates import ObjectPredicate


class DeckCondition(ABC):
    """A condition on a player's starting deck (CR 702.139a-b)."""

    @abstractmethod
    def fulfilled_by(self, cards, state) -> bool:
        """Does the starting deck ``cards`` (``GameObject`` s, read where
        they are: a library) fulfill the condition?"""


@dataclass(frozen=True)
class EachCardInStartingDeck(DeckCondition):
    """"Each <cards> in your starting deck <must>": every card matching
    ``cards`` also matches ``must`` (vacuously true with none)."""

    cards: ObjectPredicate
    must: ObjectPredicate

    def fulfilled_by(self, cards, state) -> bool:
        return all(self.must.test(c, state) for c in cards if self.cards.test(c, state))


@dataclass(frozen=True)
class StartingDeckDifferentNames(DeckCondition):
    """"Each <cards> in your starting deck has a different name": no two
    cards matching ``cards`` share a name (CR 201.2)."""

    cards: ObjectPredicate

    def fulfilled_by(self, cards, state) -> bool:
        names = [c.printing.name for c in cards if self.cards.test(c, state)]
        return len(names) == len(set(names))


@dataclass(frozen=True)
class Companion:
    """A companion ability (CR 702.139a): "Companion — <condition>"."""

    condition: DeckCondition


__all__ = [
    "Companion",
    "DeckCondition",
    "EachCardInStartingDeck",
    "StartingDeckDifferentNames",
]
