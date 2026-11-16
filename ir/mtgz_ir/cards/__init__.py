"""Card printings, mana cost parsing, basic lands, and the Stage-1 synthetic pool.

Public API: everything below remains importable from ``mtgz_ir.cards`` for
back-compat. Internal layout (submodules):

- ``base``           — CardPrinting + ManaCost + all ``make_*`` helpers +
                       pre-built granted-ability constants.
- ``lands``          — Basic lands, Planet lands, Utility lands, Shock lands.
- ``synthetic``      — Stage-1 invented vanilla creatures.
- ``tokens``         — Token printings created by effects (Lander, Robot,
                       Drone, Sliver, Munitions).
- ``parser``         — Oracle-text parser sub-package (the production
                       card source). Pulls printings from
                       ``data/edge_of_eternities.json`` and friends.
- ``source``         — ``CardSource`` protocol + ``get_active_source`` /
                       ``set_active_source`` seam.
"""

from __future__ import annotations

# Re-export the full effects surface that used to be imported at the
# top of cards.py — tests and other modules rely on names like
# ActivatedAbility / TriggeredAbility being reachable via
# ``from mtgz_ir.cards import X``.
from mtgz_ir.cards.base import *  # noqa: F401,F403
from mtgz_ir.cards.base import (
    COLORS,
    CardPrinting,
    CostReduction,
    DOMINION_BRACELET_GRANTED_ABILITY,
    ExternalCostReduction,
    MMNEMON_BLUE_MANA_GRANTED_ABILITY,
    ManaCost,
    RAGOST_FOOD_GRANTED_ABILITY,
    RoomHalf,
    cost,
    make_artifact,
    make_aura,
    make_basic_land,
    make_colorless_land,
    make_creature,
    make_enchantment,
    make_equipment,
    make_instant,
    make_planet_land,
    make_planeswalker,
    make_room,
    make_shock_land,
    make_sorcery,
    make_spacecraft,
    make_token_creature,
    make_utility_land,
    parse_mana_cost,
)
from mtgz_ir.cards.lands import (
    BASIC_LANDS, PLANET_LANDS, SHOCK_LANDS, UTILITY_LANDS,
)
from mtgz_ir.cards.synthetic import SYNTHETIC_CREATURES
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
