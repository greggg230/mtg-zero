"""The Legacy Lands cards of lane C (mtg-zero's deck MTGGoldfish 8027955;
docs/ENGINE.md 270.3): Pithing Needle, Cursed Totem, Torpor Orb, Damping
Sphere, Choke, The Tabernacle at Pendrell Vale.

Three parse through earlier grammars unchanged: Pithing Needle (Disruptor
Flute's ``ChooseCardNameAsEnters`` + ``ActivationProhibition``), Cursed Totem
(Clarion Conqueror's ``locks_activated_abilities_of``) and The Tabernacle at
Pendrell Vale (the roto pool's ``GrantsTriggerToCreatures`` with
``DestroySourceUnlessPay``). This module adds what the other three need:

* the plural basic land types as permanent phrases ("Islands": permanents
  with the land subtype Island, CR 205.3i), for the existing
  "<permanents> don't untap during their controllers' untap steps"
  (``DoesntUntap``) line; Choke joins the cube enchantment builder's
  opt-in list (:data:`LEGACY_LANDS_C_ENCHANTMENTS`);
* "<Permanents> entering don't cause abilities to trigger." (Torpor Orb):
  :class:`~mtgz_ir.effects.generic.legacy_lands_c.EnteringDoesntCauseTriggers`;
* "If a land is tapped for two or more mana, it produces {C} instead of any
  other type and amount." and "Each spell a player casts costs {n} more to
  cast for each other spell that player has cast this turn." (Damping
  Sphere): ``MultiManaLandsProduceColorless``,
  ``SpellTaxPerOtherSpellThisTurn``.

The three line recognizers are board lines (``roto_board._LINE_RECOGNIZERS``),
tried after every earlier one, so no claimed card of another pool changes."""

from __future__ import annotations

import re

# Enchantments claimed by the cube enchantment builder's line grammar
# (``cube_enchantments.build_cube_enchantment``'s opt-in) for this lane.
LEGACY_LANDS_C_ENCHANTMENTS: frozenset[str] = frozenset({"Choke"})

# The basic land types (CR 205.3i), plural as printed ("Islands").
_BASIC_LAND_TYPES = ("Plains", "Island", "Swamp", "Mountain", "Forest")
_PLURAL_LAND_TYPE = {
    ("plains" if t == "Plains" else t.lower() + "s"): t for t in _BASIC_LAND_TYPES
}


def basic_land_type_phrase(np: str, *, plural: bool):
    """``HasSubtype(<type>)`` for a plural basic land type ("islands": every
    permanent with that land subtype, CR 205.3i; a land creature such as
    Dryad Arbor is one too), else ``None``. Singular forms are not read."""
    from mtgz_ir.effects.object_predicates import HasSubtype

    if not plural:
        return None
    t = _PLURAL_LAND_TYPE.get(np.strip().lower())
    return None if t is None else HasSubtype(t)


_ENTERING_NO_TRIGGERS = re.compile(
    r"^(?P<np>Creatures) entering don't cause abilities to trigger\.$"
)
_DAMPING = re.compile(
    r"^If a land is tapped for two or more mana, it produces \{C\} instead "
    r"of any other type and amount\.$"
)
_SPELL_TAX_PER_OTHER = re.compile(
    r"^Each spell a player casts costs \{(?P<n>[1-9])\} more to cast for each "
    r"other spell that player has cast this turn\.$"
)


def lands_c_board_line(line: str):
    """A board line of lane C's cards, or ``None``."""
    from mtgz_ir.cards.parser.roto_board import BoardLine, permanent_phrase
    from mtgz_ir.effects.generic import legacy_lands_c as lc

    m = _ENTERING_NO_TRIGGERS.match(line)
    if m is not None:
        np = m.group("np").lower()
        f = permanent_phrase(np, plural=True)
        if f is None:
            return None
        return BoardLine(fields={"board_rules": (lc.EnteringDoesntCauseTriggers(permanent_filter=f, label=np),)})
    if _DAMPING.match(line):
        return BoardLine(fields={"board_rules": (lc.MultiManaLandsProduceColorless(),)})
    m = _SPELL_TAX_PER_OTHER.match(line)
    if m is not None:
        return BoardLine(fields={"board_rules": (lc.SpellTaxPerOtherSpellThisTurn(amount=int(m.group("n"))),)})
    return None


__all__ = [
    "LEGACY_LANDS_C_ENCHANTMENTS",
    "basic_land_type_phrase",
    "lands_c_board_line",
]
