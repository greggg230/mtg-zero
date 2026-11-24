"""Board rules of the Legacy cards outside the cube (pool ``legacy``) that no
earlier class expressed: mtg-zero's Legacy Lands deck (MTGGoldfish 8027955,
lane C; docs/ENGINE.md 270.3). Pithing Needle, Cursed Totem and The
Tabernacle at Pendrell Vale reuse existing classes (Disruptor Flute's
``roto_statics``, Clarion Conqueror's ``locks_activated_abilities_of``, the
roto pool's ``GrantsTriggerToCreatures``); Choke reuses ``DoesntUntap`` with
a basic land type. Two cards needed new board rules (``board_rules``, the
printing-level static abilities of a permanent):

* :class:`EnteringDoesntCauseTriggers` -- "<Permanents> entering don't cause
  abilities to trigger." (Torpor Orb: creatures.)
* :class:`MultiManaLandsProduceColorless` -- "If a land is tapped for two or
  more mana, it produces {C} instead of any other type and amount." (Damping
  Sphere's first ability.)
* :class:`SpellTaxPerOtherSpellThisTurn` -- "Each spell a player casts costs
  {n} more to cast for each other spell that player has cast this turn."
  (Damping Sphere's second ability.)

The rules are the engine's (mtg-zero, ``engine/src/lands_c.rs``); these are
descriptions only (the Python engine of mtg-bot is retired)."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class EnteringDoesntCauseTriggers:
    """"<Permanents> entering don't cause abilities to trigger." (Torpor
    Orb: ``permanent_filter`` = creatures.) A permanent matching the filter
    as it exists on the battlefield (CR 603.6b, its rulings), entering, causes
    no triggered ability to trigger (an enters-the-battlefield ability, CR
    603.6a: its own and any other's); replacement effects and "as this
    enters" are not triggered abilities and still apply (614.1c, 614.12)."""

    permanent_filter: object
    label: str = "creatures"

    def describe(self) -> str:
        return f"{self.label.capitalize()} entering don't cause abilities to trigger."


@dataclass(frozen=True)
class MultiManaLandsProduceColorless:
    """"If a land is tapped for two or more mana, it produces {C} instead of
    any other type and amount." (Damping Sphere.) A replacement effect on the
    mana production of a land's mana ability with {T} in its cost (CR 106.12,
    106.12b, 614.1a)."""

    def describe(self) -> str:
        return "If a land is tapped for two or more mana, it produces {C} instead of any other type and amount."


@dataclass(frozen=True)
class SpellTaxPerOtherSpellThisTurn:
    """"Each spell a player casts costs {``amount``} more to cast for each
    other spell that player has cast this turn." (Damping Sphere.) A cost
    increase (CR 601.2f) by the spells its caster cast earlier this turn."""

    amount: int = 1

    def describe(self) -> str:
        return (
            f"Each spell a player casts costs {{{self.amount}}} more to cast for each "
            f"other spell that player has cast this turn."
        )


__all__ = [
    "EnteringDoesntCauseTriggers",
    "MultiManaLandsProduceColorless",
    "SpellTaxPerOtherSpellThisTurn",
]
