"""Object predicates over an object's characteristic *values* — mana
value, color, and combined power/toughness — plus the bare
"permanent" atom.

Sibling of :mod:`mtgz_ir.effects.object_predicates` (type / state /
controller atoms). Every class here is an ``ObjectPredicate`` and
therefore also a ``TargetFilter``, so the parser's target-phrase
grammar (:mod:`mtgz_ir.cards.parser.target_phrases`) composes them
with the type atoms via ``&`` / ``|`` / ``~``:

- "nonland permanent with mana value 3 or less"
  -> ``IsPermanent() & ~IsLand() & HasManaValueAtMost(3)``
- "nonblack creature" -> ``IsCreature() & ~HasColor("B")``
- "creature with total power and toughness 5 or less"
  -> ``IsCreature() & HasTotalPowerAndToughnessAtMost(5)``

The predicates read characteristics from the object wherever it is
(battlefield permanent, spell on the stack, card in a graveyard), so
the same atoms serve counterspell filters ("target creature spell
with power or toughness 2 or less") and removal filters alike.
Players have no ``printing`` and evaluate ``False`` everywhere.
"""

from __future__ import annotations

from dataclasses import dataclass

from mtgz_ir.effects.object_predicates import ObjectPredicate


def mana_value_of(obj) -> int:
    """CR 202.3: an object's mana value is the total mana in its mana
    cost; ``{X}`` counts as 0 everywhere except on the stack, where it
    is the chosen X (CR 202.3e). The engine keeps X on the
    ``StackItem`` rather than the object, so callers that need the
    on-stack value add it themselves (see
    :func:`mtgz_ir.effects.generic.counterspells.spell_mana_value`)."""
    return obj.printing.mana_cost.total


@dataclass(frozen=True)
class IsPermanent(ObjectPredicate):
    """The object is a permanent (anything with a printing that is on
    the battlefield when used as a target filter). Players evaluate
    ``False``. Models the bare printed noun "permanent" ("destroy
    target permanent")."""

    def test(self, obj, state):
        return hasattr(obj, "printing")


@dataclass(frozen=True)
class HasManaValueAtMost(ObjectPredicate):
    """"with mana value ``n`` or less" (CR 202.3)."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return mana_value_of(obj) <= self.n


@dataclass(frozen=True)
class HasColor(ObjectPredicate):
    """The object is ``color`` (one of ``W U B R G``), per its
    *effective* colors (layer 5 — :func:`mtgz_ir.game.effective_colors`).
    Negate with ``~`` for the printed "non<color>" qualifier
    ("nonblack creature")."""

    color: str

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_colors

        return self.color in effective_colors(state, obj)


@dataclass(frozen=True)
class HasTotalPowerAndToughnessAtMost(ObjectPredicate):
    """"with total power and toughness ``n`` or less" (Cut Down):
    effective power plus effective toughness is at most ``n``.
    Non-creature objects (no measurable P/T) evaluate ``False``."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import (
            effective_power, effective_toughness, is_creature_now,
        )

        if not is_creature_now(obj):
            return False
        total = effective_power(state, obj) + effective_toughness(state, obj)
        return total <= self.n


@dataclass(frozen=True)
class HasCardType(ObjectPredicate):
    """The object's card types include ``card_type`` (e.g.
    ``"Instant"``, ``"Sorcery"``) — the spell-type heads of counterspell
    noun phrases ("instant or sorcery spell"). Reads
    :func:`mtgz_ir.game.has_card_type` (layer 4 for permanents)."""

    card_type: str

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, self.card_type)


@dataclass(frozen=True)
class IsPermanentCard(ObjectPredicate):
    """"permanent card" (CR 110.4a): its printing has a permanent card type
    (artifact, battle, creature, enchantment, land, planeswalker). Unlike
    :class:`IsPermanent` (a target filter on the battlefield, where
    anything with a printing is a permanent), it rejects instants and
    sorceries wherever the card is: a companion's deck condition reads it
    over cards in a library (``effects.deck_conditions``)."""

    def test(self, obj, state):
        p = getattr(obj, "printing", None)
        if p is None:
            return False
        return any(t in _PERMANENT_CARD_TYPES for t in p.card_types)


_PERMANENT_CARD_TYPES = frozenset(
    {"Artifact", "Battle", "Creature", "Enchantment", "Land", "Planeswalker"}
)


@dataclass(frozen=True)
class HasActivatedAbility(ObjectPredicate):
    """"has an activated ability" (CR 602; Zirda, the Dawnwaker's companion
    condition): its printing has one - mana abilities (a land's ``produces``,
    CR 605.1a, including basic lands' intrinsic ones, 305.6), loyalty
    abilities (606.2) and keyword abilities that are activated (equip,
    cycling and landcycling, reconfigure, Class levels) included. Read from
    the printing, so abilities an effect grants or removes on the
    battlefield are not seen: its one use is a deck condition, over cards
    outside the battlefield (``effects.deck_conditions``)."""

    def test(self, obj, state):
        p = getattr(obj, "printing", None)
        if p is None:
            return False
        return bool(
            p.activated_abilities
            or p.hand_activated_abilities
            or p.graveyard_activated_abilities
            or p.produces
            or p.landcycling
            or p.reconfigure_cost is not None
            or p.class_levels
        )


@dataclass(frozen=True)
class HasManaValueAtMostColorsSpent(ObjectPredicate):
    """Converge: "if its mana value is less than or equal to the number
    of colors of mana spent to cast this spell" — compares against the
    resolving spell's ``sunburst_colors_spent`` (Prismatic Ending)."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        spell = getattr(state, "resolving_spell", None)
        spent = spell.sunburst_colors_spent if spell is not None else 0
        return mana_value_of(obj) <= spent


__all__ = [
    "HasActivatedAbility",
    "HasCardType",
    "HasManaValueAtMostColorsSpent",
    "HasColor",
    "HasManaValueAtMost",
    "HasTotalPowerAndToughnessAtMost",
    "IsPermanent",
    "IsPermanentCard",
    "mana_value_of",
]
