"""Closed vocabularies of the string discriminants on card primitives.

Many parsed primitives carry a short string that their own code (or the
engine) dispatches on: ``RemoveTargetPermanents.action`` is ``"destroy"``,
``"exile"`` or ``"bounce"``; ``SearchLibrary.destination`` is one of four
zones; predicates name an event subject (``"attacker"``, ``"spell"``).
Each such field is typed with one of the ``StrEnum``s below and coerced
in its class's ``__post_init__``, so an unknown value is a construction
error rather than a silently ignored branch. A ``StrEnum`` member
compares and hashes equal to its value, so every existing ``== "exile"``
comparison and string-keyed lookup keeps working.

The members of each enum are the values the code gives meaning to
(collected from the dispatching code paths, not only from the values the
current card pools happen to use). Fields whose values come from an open
vocabulary stay ``str``: counter names (``"charge"``, ``"corpse"``),
subtypes, display labels, and target-kind tags (``"graveyard_creature"``,
``"my_creature"``), which form one engine-wide namespace shared by every
targeting effect.

Runtime game state keeps plain strings: an effect that copies one of
these values into a game record (a mana coin, a delayed trigger, a
continuous-effect kind) stores ``.value``.
"""

from __future__ import annotations

from enum import StrEnum


def coerce(enum_cls, value):
    """``enum_cls(value)`` (``None`` stays ``None``)."""
    return None if value is None else enum_cls(value)


def coerce_tuple(enum_cls, values) -> tuple:
    return tuple(enum_cls(v) for v in values)


def plain(value):
    """The plain ``str`` of a vocabulary member (anything else unchanged):
    what an effect stores into runtime game state, which stays made of
    builtin values (``search.canonical_state`` marshals it)."""
    return value.value if isinstance(value, StrEnum) else value


class ZoneName(StrEnum):
    """Zone names (the values of ``game.Zone``)."""

    LIBRARY = "library"
    HAND = "hand"
    BATTLEFIELD = "battlefield"
    GRAVEYARD = "graveyard"
    EXILE = "exile"
    STACK = "stack"
    COMMAND = "command"


class CardTypeWord(StrEnum):
    """Lowercase card-type words (CR 205.2a) used as filters
    (``printing.is_<word>``, layer-4 type grants)."""

    ARTIFACT = "artifact"
    BATTLE = "battle"
    CREATURE = "creature"
    ENCHANTMENT = "enchantment"
    INSTANT = "instant"
    LAND = "land"
    PLANESWALKER = "planeswalker"
    SORCERY = "sorcery"


class EventSubject(StrEnum):
    """Names of the subjects an event's dispatcher passes to trigger
    predicates (``predicates.EVENT_SUBJECTS`` plus the replacement-event
    payload names)."""

    OBJ = "obj"
    ATTACKER = "attacker"
    BLOCKER = "blocker"
    SOURCE = "source"
    TARGET = "target"
    SPELL = "spell"
    CONTROLLER = "controller"
    LAND = "land"
    PLAYER = "player"
    DEFENDER = "defender"
    CARD = "card"
    ROOM = "room"
    TARGET_SPELL = "target_spell"
    TARGETING = "targeting"
    KIND = "kind"
    TRIGGERING_OBJ = "triggering_obj"
    ABILITY_SOURCE = "ability_source"
    # ``TRIGGERED_ABILITY_TRIGGERS``: the permanent whose entering the
    # battlefield caused the ability to trigger; absent when the trigger
    # event is not a permanent entering (Starfield Vocalist).
    ENTERING_OBJ = "entering_obj"
    LEVEL = "level"
    LORE_BEFORE = "lore_before"
    LORE_AFTER = "lore_after"


# ---- printing-level discriminants -----------------------------------------


class EnchantKind(StrEnum):
    """``CardPrinting.aura_enchant_type``: what an Aura enchants (also the
    target kind its cast picks)."""

    CREATURE = "creature"
    ARTIFACT = "artifact"
    LAND = "land"
    CREATURE_CARD_IN_GRAVEYARD = "creature_card_in_graveyard"
    # "Enchant creature or Spacecraft" (Tractor Beam): a Spacecraft below
    # its station threshold is a noncreature artifact and still legal
    # (CR 303.4c, 722.2b).
    CREATURE_OR_SPACECRAFT = "creature_or_spacecraft"
    # "Enchant artifact or creature [you control]" (Moonlit Meditation).
    ARTIFACT_OR_CREATURE = "artifact_or_creature"


class XCounterMode(StrEnum):
    """``CardPrinting.enters_with_counters_from_x``: where the number of
    +1/+1 (or charge) counters an object enters with comes from."""

    X = "x"
    X_CHARGE = "x_charge"
    MANA_SPENT = "mana_spent"


# ---- zone-moving effects --------------------------------------------------


class SearchDestination(StrEnum):
    HAND = "hand"
    GRAVEYARD = "graveyard"
    TOP = "top"
    BATTLEFIELD = "battlefield"


class RestZone(StrEnum):
    """Where the rest of looked-at cards go."""

    BOTTOM = "bottom"
    GRAVEYARD = "graveyard"


class DistributePile(StrEnum):
    HAND = "hand"
    BOTTOM = "bottom"
    EXILE_PLAYABLE = "exile_playable"


class RevealedCardFate(StrEnum):
    """What happens to the card chosen from a revealed hand."""

    DISCARD = "discard"
    EXILE = "exile"


class SpellDestination(StrEnum):
    """Where a countered spell goes."""

    GRAVEYARD = "graveyard"
    EXILE = "exile"
    LIBRARY_TOP = "library_top"
    HAND = "hand"


class GraveyardReturnDestination(StrEnum):
    HAND = "hand"
    BATTLEFIELD = "battlefield"


class LibraryPosition(StrEnum):
    SECOND_FROM_TOP = "second_from_top"
    BENEATH_TOP_X = "beneath_top_x"


class ReanimateSource(StrEnum):
    TARGET_ANY_GRAVEYARD = "target_any_graveyard"
    TARGET_YOUR_GRAVEYARD = "target_your_graveyard"
    TOP_OF_YOUR_GRAVEYARD = "top_of_your_graveyard"


class HasteRider(StrEnum):
    UNTIL_END_OF_TURN = "until_end_of_turn"
    PERMANENT = "permanent"


class RemovalAction(StrEnum):
    DESTROY = "destroy"
    EXILE = "exile"
    BOUNCE = "bounce"


class StackObjectKind(StrEnum):
    SPELL = "spell"
    ACTIVATED = "activated"
    TRIGGERED = "triggered"


class ExilePlayDuration(StrEnum):
    """How long an "exile ... you may play it" permission lasts."""

    THIS_TURN = "this_turn"
    UNTIL_YOUR_NEXT_END_STEP = "until_your_next_end_step"
    UNTIL_END_OF_YOUR_NEXT_TURN = "until_end_of_your_next_turn"
    WHILE_EXILED = "while_exiled"


class ImpulseLibraryOwner(StrEnum):
    """Whose library an impulse-draw effect exiles from."""

    YOU = "you"
    THAT_PLAYER = "that_player"
    DEFENDING_PLAYER = "defending_player"


# ---- players, timing, costs, mana -----------------------------------------


class AffectedPlayers(StrEnum):
    """Which players a replacement effect applies to."""

    YOU = "you"
    OPPONENTS = "opponents"
    ANY = "any"


class DelayedStep(StrEnum):
    """When a scheduled delayed effect resolves."""

    UPKEEP = "upkeep"
    END = "end"
    # "At the beginning of your next upkeep" (Pact of Negation): the
    # scheduling controller's next upkeep, not the next turn's whoever
    # its player (mtg-zero's Legacy Doomsday deck, pool ``legacy``).
    YOUR_UPKEEP = "your_upkeep"


class SpendRestriction(StrEnum):
    """What a restricted mana coin may pay for (``actions.
    _coin_satisfies_context`` / ``roto_mana.roto_coin_satisfies``)."""

    ARTIFACT_SPELL = "artifact_spell"
    ARTIFACT_ACTIVATION = "artifact_activation"
    COLORLESS_ELDRAZI = "colorless_eldrazi"
    LEGENDARY_SPELL = "legendary_spell"
    LEGENDARY_PERMANENT_ABILITY = "legendary_permanent_ability"
    LAND_ACTIVATION = "land_activation"
    CAST_FROM_OUTSIDE_HAND = "cast_from_outside_hand"
    NOT_NONARTIFACT_SPELL = "not_nonartifact_spell"
    COLORED_ONLY = "colored_only"
    # "Spend this mana only to cast artifact spells or activate abilities
    # of artifacts" (Power Depot; mtg-zero docs/ir_defects.md 47).
    ARTIFACT_SPELL_OR_ACTIVATION = "artifact_spell_or_activation"


class LibraryCardFilter(StrEnum):
    """Named card filters of library searches / reveals."""

    LEGENDARY_CREATURE = "legendary_creature"
    BASIC_LAND = "basic_land"
    BASIC_PLAINS = "basic_plains"
    BASIC_ISLAND = "basic_island"
    BASIC_SWAMP = "basic_swamp"
    BASIC_MOUNTAIN = "basic_mountain"
    BASIC_FOREST = "basic_forest"
    CREATURE = "creature"
    CREATURE_SMALL = "creature_small"
    ARTIFACT = "artifact"
    ARTIFACT_SMALL = "artifact_small"
    SPACECRAFT = "spacecraft"
    SLIVER = "sliver"
    DEMON = "demon"
    SUBTYPE = "subtype"


class AdditionalCostKind(StrEnum):
    DISCARD = "discard"
    PAY_LIFE = "pay_life"
    SACRIFICE = "sacrifice"


class AlternativeCostTurn(StrEnum):
    """Whose turn an alternative cost may be paid in."""

    YOURS = "yours"
    NOT_YOURS = "not_yours"


# ---- Vintage Roto primitives ----------------------------------------------


class CantCastScope(StrEnum):
    OPPONENTS = "opponents"
    TARGET_PLAYER = "target_player"


class CastProhibitionScope(StrEnum):
    EACH = "each"
    OPPONENTS = "opponents"
    PLAYER = "player"


class GraveyardOwner(StrEnum):
    YOURS = "yours"
    OPPONENTS = "opponents"


class PlayerRuleKind(StrEnum):
    CANT_LOSE_THE_GAME = "cant_lose_the_game"
    OPPONENTS_CANT_WIN = "opponents_cant_win"
    DAMAGE_LIFE_FLOOR_ONE = "damage_life_floor_one"


class TargetingSource(StrEnum):
    SPELL = "spell"
    ABILITY = "ability"


class PerCounterAction(StrEnum):
    SACRIFICE = "sacrifice"
    TAP = "tap"
