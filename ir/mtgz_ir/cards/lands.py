"""Basic lands, EoE Planet lands, and the shock-land cycle.

Small fixed registries. Larger land sets (Stellar Sights bonus-sheet reprints)
live in ``stellar_sights.py``.
"""

from __future__ import annotations

from mtgz_ir.effects import (
    ActivatedAbility, ActivationCost, AddManaToPool,
    CreateToken, PumpTargetCreatureUntilEOT,
    SacrificeSelfUnlessTapPermanent, TriggeredAbility,
)
from mtgz_ir.cards.base import (
    CardPrinting, make_basic_land, make_colorless_land, make_planet_land,
    make_shock_land, make_utility_land, parse_mana_cost,
)
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.predicates import Event, IsSelf


BASIC_LANDS: dict[str, CardPrinting] = {
    "Plains": make_basic_land("Plains", "W"),
    "Island": make_basic_land("Island", "U"),
    "Swamp": make_basic_land("Swamp", "B"),
    "Mountain": make_basic_land("Mountain", "R"),
    "Forest": make_basic_land("Forest", "G"),
}


# Shock-land cycle from EoE. Each is Land — L1 L2 producing both colors,
# "As this land enters, you may pay 2 life. If you don't, it enters
# tapped."
PLANET_LANDS: dict[str, CardPrinting] = {
    "Adagia, Windswept Bastion": make_planet_land(
        "Adagia, Windswept Bastion", "W",
    ),
    "Uthros, Titanic Godcore": make_planet_land(
        "Uthros, Titanic Godcore", "U",
    ),
    "Susur Secundi, Void Altar": make_planet_land(
        "Susur Secundi, Void Altar", "B",
    ),
    "Kavaron, Memorial World": make_planet_land(
        "Kavaron, Memorial World", "R",
    ),
    "Evendo, Waking Haven": make_planet_land(
        "Evendo, Waking Haven", "G",
    ),
}


UTILITY_LANDS: dict[str, CardPrinting] = {
    # Secluded Starforge: full faithful 3-AA printing (mirrors the
    # parser's _try_build_colorless_utility_land multi-AA path):
    #   AA1  {T}: Add {C}.
    #   AA2  {2}, {T}, Tap X untapped artifacts you control: Target
    #        creature gets +X/+0 until end of turn. Activate only as a
    #        sorcery.  (X = number of artifacts tapped as cost; flows
    #        through resolving_x_value into the power_bonus_from_x pump.)
    #   AA3  {5}, {T}: Create a 2/2 colorless Robot artifact creature
    #        token.
    "Secluded Starforge": make_utility_land(
        "Secluded Starforge",
        produces=frozenset({"C"}),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("C", 1),)),
                is_mana_ability=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{2}"), mana_str="{2}",
                    tap_self=True,
                    tap_x_untapped_artifacts_you_control=True,
                ),
                effect=PumpTargetCreatureUntilEOT(power_bonus_from_x=True),
                sorcery_speed=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{5}"), mana_str="{5}",
                    tap_self=True,
                ),
                effect=CreateToken(
                    token_printing=TOKEN_PRINTINGS["Robot"], count=1,
                ),
            ),
        ),
    ),
    # Command Bridge: enters tapped; "When this land enters, sacrifice
    # it unless you tap an untapped permanent you control"; {T}: add
    # one mana of any color. The any-color tap is via an explicit
    # mana ActivatedAbility (mirrors Gemstone Caverns/All-Fates Scroll
    # pattern). The ETB rider runs through SacrificeSelfUnlessTapPermanent
    # which delegates the tap-or-sac choice to the controller's agent.
    "Command Bridge": make_utility_land(
        "Command Bridge",
        enters_tapped=True,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(), effect=SacrificeSelfUnlessTapPermanent(),
            ),
        ),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("any", 1),)),
                is_mana_ability=True,
            ),
        ),
    ),
}


SHOCK_LANDS: dict[str, CardPrinting] = {
    "Breeding Pool": make_shock_land(
        "Breeding Pool", ("G", "U"), "Forest Island",
    ),
    "Godless Shrine": make_shock_land(
        "Godless Shrine", ("W", "B"), "Plains Swamp",
    ),
    "Sacred Foundry": make_shock_land(
        "Sacred Foundry", ("R", "W"), "Mountain Plains",
    ),
    "Stomping Ground": make_shock_land(
        "Stomping Ground", ("R", "G"), "Mountain Forest",
    ),
    "Watery Grave": make_shock_land(
        "Watery Grave", ("U", "B"), "Island Swamp",
    ),
}
