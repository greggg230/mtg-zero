"""Synthetic Stage-1 vanilla creatures.

Invented creatures used while real EoE cards lacked the required keywords.
Now dwarfed by EOE_CREATURES but still referenced by simulate.py's deck
builder (and by some tests that want vanilla stats).
"""

from __future__ import annotations

from mtgz_ir.cards.base import *  # noqa: F401,F403
from mtgz_ir.effects.predicates import Event, IsSelf


SYNTHETIC_CREATURES: dict[str, CardPrinting] = {
    c.name: c
    for c in [
        # White
        make_creature("Battlefield Recruit", "{W}", 1, 2),
        make_creature("Trained Soldier", "{1}{W}", 2, 2),
        make_creature("Stalwart Knight", "{2}{W}", 3, 3),
        make_creature("Sky Sentinel", "{1}{W}", 1, 3, keywords={"flying"}),
        make_creature("Vigilant Guard", "{2}{W}", 2, 3, keywords={"vigilance"}),
        make_creature("Wall of Stone", "{1}{W}", 0, 5, keywords={"defender"}),
        make_creature("Cleric of Dawn", "{2}{W}", 2, 2, keywords={"lifelink"}),
        make_creature(
            "Healing Wisp", "{W}", 1, 1,
            triggered_abilities=(
                TriggeredAbility(event=Event.ENTERS, predicate=IsSelf(), effect=GainLife(amount=2)),
            ),
        ),
        # Blue
        make_creature("Coastal Scout", "{U}", 1, 2),
        make_creature("Tidal Warrior", "{1}{U}", 2, 2),
        make_creature("Deep Leviathan", "{3}{U}", 4, 4),
        make_creature("Scale Drake", "{2}{U}", 2, 3, keywords={"flying"}),
        make_creature(
            "Inquisitive Scholar", "{2}{U}", 2, 2,
            triggered_abilities=(
                TriggeredAbility(event=Event.ENTERS, predicate=IsSelf(), effect=DrawCards(amount=1)),
            ),
        ),
        make_creature(
            "Court Archivist", "{3}{U}", 2, 3,
            triggered_abilities=(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(), effect=EachPlayerDraws(amount=1)
                ),
            ),
        ),
        # Black
        make_creature("Cemetery Rat", "{B}", 2, 1),
        make_creature("Grave Brute", "{1}{B}", 2, 2),
        make_creature("Infernal Horror", "{2}{B}{B}", 4, 4),
        make_creature("Ebon Bat", "{1}{B}", 2, 1, keywords={"flying"}),
        make_creature("Venomous Serpent", "{1}{B}", 1, 1, keywords={"deathtouch"}),
        # Red
        make_creature("Raging Goblin", "{R}", 2, 1),
        make_creature("Blood Warrior", "{1}{R}", 2, 2),
        make_creature("Stone Ogre", "{2}{R}", 3, 3),
        make_creature("Dragon Whelp", "{2}{R}", 2, 2, keywords={"flying"}),
        make_creature("Goblin Runner", "{R}", 2, 1, keywords={"haste"}),
        make_creature("Charging Warrior", "{2}{R}", 3, 2, keywords={"haste"}),
        make_creature("Pack Brute", "{2}{R}", 3, 2, keywords={"menace"}),
        # Green
        make_creature("Sapling", "{G}", 1, 2),
        make_creature("Wild Bear", "{1}{G}", 2, 2),
        make_creature("Forest Giant", "{2}{G}", 3, 3),
        make_creature("Great Wurm", "{3}{G}{G}", 6, 6),
        make_creature("Spider Sentinel", "{2}{G}", 1, 4, keywords={"reach"}),
        make_creature("Cliff Elk", "{3}{G}", 3, 4, keywords={"vigilance"}),
        make_creature("Charging Rhino", "{2}{G}", 3, 3, keywords={"trample"}),
    ]
}


# Real Edge of Eternities cards. These match actual Scryfall oracle text (set
# code: eoe) using the Stage-3 primitives we've built. More EoE cards unlock
