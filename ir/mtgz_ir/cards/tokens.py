"""Token printings created by effects (Lander, Robot, Drone, Sliver, etc.).

Tokens are never cast or drawn. They're created by resolving effects and
cease to exist when they leave the battlefield. This module keeps the
token registry separate from the main card pool.
"""

from __future__ import annotations

from mtgz_ir.cards.base import *  # noqa: F401,F403
from mtgz_ir.cards.base import _parse_type_line
from mtgz_ir.effects.predicates import Event, IsSelf


def _make_lander_token() -> CardPrinting:
    """EoE Lander: a colorless artifact token with an activated ability
    (CR 111.10: the predefined Lander token).

    "{2}, {T}, Sacrifice this token: Search your library for a basic land
    card, put it onto the battlefield tapped, then shuffle."
    """
    type_line = "Token Artifact — Lander"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name="Lander",
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{2}"),
                    mana_str="{2}",
                    tap_self=True,
                    sacrifice_self=True,
                ),
                effect=SearchLibraryForBasicLandTapped(),
            ),
        ),
    )


def _make_artifact_token(
    name: str, ability: ActivatedAbility,
) -> CardPrinting:
    """A colorless ``Token Artifact — <name>`` with one activated
    ability — the shape shared by the predefined tokens (CR 111.10):
    Clue, Food, Map, Blood."""
    type_line = f"Token Artifact — {name}"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        activated_abilities=(ability,),
    )


def _predefined_artifact_tokens() -> dict[str, CardPrinting]:
    from mtgz_ir.effects.generic.keyword_actions import Explore

    return {
        # CR 111.10f Clue: "{2}, Sacrifice this token: Draw a card."
        "Clue": _make_artifact_token("Clue", ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{2}"), mana_str="{2}",
                sacrifice_self=True,
            ),
            effect=DrawCards(amount=1),
        )),
        # CR 111.10b Food: "{2}, {T}, Sacrifice this token: You gain 3 life."
        "Food": _make_artifact_token("Food", ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{2}"), mana_str="{2}",
                tap_self=True, sacrifice_self=True,
            ),
            effect=GainLife(amount=3),
        )),
        # CR 111.10k Map: "{1}, {T}, Sacrifice this token: Target
        # creature you control explores. Activate only as a sorcery."
        "Map": _make_artifact_token("Map", ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{1}"), mana_str="{1}",
                tap_self=True, sacrifice_self=True,
            ),
            effect=Explore(target=True),
            sorcery_speed=True,
        )),
        # CR 111.10g Blood: "{1}, {T}, Discard a card, Sacrifice this
        # token: Draw a card."
        "Blood": _make_artifact_token("Blood", ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost("{1}"), mana_str="{1}",
                tap_self=True, discard_a_card=True, sacrifice_self=True,
            ),
            effect=DrawCards(amount=1),
        )),
    }


# Back face of the Incubator token: a 0/0 colorless Phyrexian artifact
# creature (the Incubator's +1/+1 counters carry over on transform).
_PHYREXIAN_TOKEN = make_token_creature(
    "Phyrexian", 0, 0, subtypes=("Phyrexian",), artifact=True,
)


def _make_incubator_token() -> CardPrinting:
    """Incubator: artifact token with "{2}: Transform this artifact."
    (transforms into ``_PHYREXIAN_TOKEN``)."""
    from mtgz_ir.effects.generic.incubate import TransformSelfIntoPrinting

    type_line = "Token Artifact — Incubator"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name="Incubator",
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{2}"), mana_str="{2}",
                ),
                effect=TransformSelfIntoPrinting(back_printing=_PHYREXIAN_TOKEN),
            ),
        ),
    )



TOKEN_PRINTINGS: dict[str, CardPrinting] = {
    # "1/1 white Human Soldier creature token" (Knight Luminary,
    # Cosmogrand Zenith, Honored Knight-Captain, Rayblade Trooper): white
    # through its color indicator (CR 111.4: a token has the colors its
    # creating effect names).
    "Human Soldier": make_token_creature(
        "Human Soldier", 1, 1, subtypes=("Human", "Soldier"),
        colors=frozenset({"W"}),
    ),
    # 2/2 colorless Robot artifact creature — EoE's generic Robot token,
    # created by many Spacecraft ETBs and dies-triggered creatures.
    "Robot": make_token_creature(
        "Robot", 2, 2, subtypes=("Robot",), artifact=True
    ),
    # 1/1 colorless Drone artifact creature with flying and "this
    # token can block only creatures with flying." Created by Pinnacle
    # Emissary, Station Monitor, and other EoE artifact triggers. The
    # block-restriction is enforced via
    # CardPrinting.can_only_block_creatures_with_flying, which
    # combat.can_block consults when the blocker is a Drone.
    "Drone": make_token_creature(
        "Drone", 1, 1, subtypes=("Drone",), artifact=True,
        keywords=frozenset({"flying"}),
        can_only_block_creatures_with_flying=True,
    ),
    # Lander — artifact token (not a creature) with a sacrifice-to-ramp
    # activated ability. Created by many EoE spells and triggers.
    "Lander": _make_lander_token(),
    # 1/1 colorless Sliver creature token — Thrumming Hivepool's upkeep
    # trigger.
    "Sliver": make_token_creature(
        "Sliver", 1, 1, subtypes=("Sliver",),
    ),
    # 1/1 colorless Eldrazi Scion creature token with "Sacrifice this
    # token: Add {C}." Created by Warping Wail's third mode. The sac-
    # for-{C} ability is a mana ability (bypasses the stack) just like
    # Lander's tap/sac ramp — modeled via is_mana_ability=True so the
    # spell-cast mana pipeline can consume the produced {C} before
    # passing priority.
    "Eldrazi Scion": make_token_creature(
        "Eldrazi Scion", 1, 1, subtypes=("Eldrazi", "Scion"),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(sacrifice_self=True),
                effect=AddManaToPool(produces=(("C", 1),)),
                is_mana_ability=True,
            ),
        ),
    ),
    # 0/1 colorless Eldrazi Spawn creature token with "Sacrifice this
    # token: Add {C}." — Malevolent Rumble (cube). Same mana ability
    # shape as the Eldrazi Scion token above.
    "Eldrazi Spawn": make_token_creature(
        "Eldrazi Spawn", 0, 1, subtypes=("Eldrazi", "Spawn"),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(sacrifice_self=True),
                effect=AddManaToPool(produces=(("C", 1),)),
                is_mana_ability=True,
            ),
        ),
    ),
    # 2/1 white Insect creature token with flying — the Mistmoors
    # Overlord (Duskmourn) creates two of these whenever it enters
    # or attacks. Color isn't tracked through tokens (the engine
    # derives color from mana_cost.colors() and tokens have an
    # empty mana cost), so this surfaces as a colorless 2/1 Insect
    # with flying — a simplification that matches how every other
    # color-bearing token is shipped today (no token printing
    # carries an explicit color tag yet).
    "Insect": make_token_creature(
        "Insect", 2, 1, subtypes=("Insect",),
        keywords=frozenset({"flying"}),
    ),
    # Munitions: colorless artifact token created by Weapons Manufacturing.
    # "When this token leaves the battlefield, it deals 2 damage to any
    # target." Modeled via a leaves-battlefield trigger on the token.
    "Munitions": CardPrinting(
        name="Munitions",
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line="Token Artifact — Munitions",
        supertypes=("Token",),
        card_types=("Artifact",),
        subtypes=("Munitions",),
        triggered_abilities=(
            TriggeredAbility(
                event=Event.LEAVES_BATTLEFIELD, predicate=IsSelf(),
                effect=DealDamageAnyTarget(amount=2),
            ),
        ),
    ),
    # Treasure: the standard evergreen colorless artifact token. Single
    # activated ability: "{T}, Sacrifice this token: Add one mana of
    # any color." The mana ability is flagged ``is_mana_ability=True``
    # so the engine bypasses the stack at activation per CR 605 — the
    # same pattern Lander and Eldrazi Scion use for their
    # sac-for-mana abilities. Modeled identically to the in-print
    # printing on Scryfall: an artifact (no creature type), no P/T,
    # no flavor text, exactly one ability.
    "Treasure": CardPrinting(
        name="Treasure",
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line="Token Artifact — Treasure",
        supertypes=("Token",),
        card_types=("Artifact",),
        subtypes=("Treasure",),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    tap_self=True,
                    sacrifice_self=True,
                ),
                effect=AddManaToPool(produces=(("any", 1),)),
                is_mana_ability=True,
            ),
        ),
    ),
    # Glimmer: 1/1 white Glimmer enchantment creature token. Created
    # by Tunnel Surveyor's ETB (and other Duskmourn cards that
    # interact with the Glimmer subtype). The first
    # enchantment-creature token in the EoE/Duskmourn pool — both
    # ``Enchantment`` and ``Creature`` appear in ``card_types``,
    # mirroring the printed type line. The token has no abilities and
    # no flavor text; it's a vanilla 1/1 with the dual card type so
    # enchantment-targeting effects can interact with it the same way
    # they would with a non-token enchantment creature.
    "Glimmer": CardPrinting(
        name="Glimmer",
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line="Token Enchantment Creature — Glimmer",
        supertypes=("Token",),
        card_types=("Enchantment", "Creature"),
        subtypes=("Glimmer",),
        power=1, toughness=1,
    ),
    # Gremlin: 1/1 red Gremlin creature token. Created by Razorkin
    # Hordecaller's "Whenever you attack" trigger and by Gremlin
    # Tamer's Eerie trigger. Vanilla 1/1 with no abilities — color
    # isn't tracked on tokens (the engine derives color from
    # ``mana_cost.colors()`` and tokens have an empty mana cost), so
    # the printing surfaces as a colorless 1/1 Gremlin, mirroring the
    # other color-bearing token printings (Insect, Spirit-to-be, etc.)
    # that ship without an explicit color tag today.
    "Gremlin": make_token_creature(
        "Gremlin", 1, 1, subtypes=("Gremlin",),
    ),
    # Demon: 6/6 black Demon creature token with flying. Created by
    # Ritual Chamber (Unholy Annex // Ritual Chamber)'s door trigger.
    # Color isn't tracked on tokens (every other color-tagged token
    # in this pool ships without an explicit color tag) so the
    # printing surfaces as a colorless 6/6 Demon with flying. The
    # token's ``Demon`` subtype is what Unholy Annex's end-step gate
    # reads: "If you control a Demon, each opponent loses 2 life and
    # you gain 2 life. Otherwise, you lose 2 life." A Demon token on
    # the battlefield satisfies that gate via the ``HasSubtype``-style
    # check the new ``ConditionalIfYouControlSubtype`` combinator does.
    "Demon": make_token_creature(
        "Demon", 6, 6, subtypes=("Demon",),
        keywords=frozenset({"flying"}),
    ),
}

TOKEN_PRINTINGS.update(_predefined_artifact_tokens())
# Incubator (CR 701.53): the artifact token Incubate creates — "{2}:
# Transform this artifact." — whose back face is the "Phyrexian" token
# (a 0/0 artifact creature; the Incubator's +1/+1 counters size it).
# Sunfall (cube).
TOKEN_PRINTINGS["Incubator"] = _make_incubator_token()
TOKEN_PRINTINGS["Phyrexian"] = _PHYREXIAN_TOKEN
