"""Effect primitives needed by the faces of the cube layouts bucket's
double-faced cards (Jace, Vryn's Prodigy; Tamiyo, Inquisitive Student;
Ajani, Nacatl Pariah; Cecil, Dark Knight; Etali, Primal Conqueror; ...).

None of these is layout-specific; they live in their own module (the
parser entry points are in :mod:`mtgz_ir.cards.parser.layout_grammar`)
so the cube campaign's parallel branches don't collide in the shared
cluster modules. Every class is parameterized by the parts of its
printed shape that vary.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.generic.card_flow import CardFilter
from mtgz_ir.effects.predicates import TriggerPredicate

if TYPE_CHECKING:  # pragma: no cover
    from mtgz_ir.game import GameState


_COLOR_WORDS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}


# ---------------------------------------------------------------------------
# Conditions
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CardsInYourGraveyardAtLeast(Condition):
    """"there are N or more cards in your graveyard" (threshold)."""

    n: int = 7

    def matches(self, state, viewer_id, chosen_targets=None):
        return len(state.player_by_id(viewer_id).graveyard) >= self.n


@dataclass(frozen=True)
class LifeAtMostHalfStartingLife(Condition):
    """"your life total is less than or equal to half your starting life
    total"."""

    def matches(self, state, viewer_id, chosen_targets=None):
        p = state.player_by_id(viewer_id)
        return 2 * p.life <= p.starting_life


@dataclass(frozen=True)
class ControlOtherPermanentOfColor(Condition):
    """"you control a <color> permanent other than <this permanent>" --
    the resolving ability's source (``state.resolving_source``) doesn't
    count."""

    color: str = "R"

    def matches(self, state, viewer_id, chosen_targets=None):
        from mtgz_ir.game import effective_colors

        source = state.resolving_source
        return any(
            o.controller_id == viewer_id
            and (source is None or o.id != source.id)
            and self.color in effective_colors(state, o)
            for o in state.battlefield
        )


# ---------------------------------------------------------------------------
# Trigger predicates
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class NthCardDrawnThisTurn(TriggerPredicate):
    """"you draw your <Nth> card in a turn" -- the DRAWS_CARD event whose
    drawing player has now drawn exactly ``n`` cards this turn (the
    count ``turn.draw_card`` keeps)."""

    n: int = 3

    def matches(self, state, subjects, source):
        player = subjects.get("player")
        return player is not None and player.cards_drawn_this_turn == self.n


# ---------------------------------------------------------------------------
# Effects
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetOpponentMills(Effect):
    """"target opponent mills N cards"."""

    amount: int = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.draw_mill import MillTargetPlayer

        MillTargetPlayer(amount=self.amount).resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"target opponent mills {self.amount} cards"


@dataclass(frozen=True)
class PumpTargetUntilYourNextTurn(Effect):
    """"[Up to one ]target <noun> gets +P/+T until your next turn": a
    layer-7c ``pump_pt`` registry entry that expires at the
    controller's next untap step (``expires_at="controller_untap"``)."""

    power: int = 0
    toughness: int = 0
    targets: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        source = state.resolving_source
        for slot, chosen in zip(self.targets, targets):
            if not _slot_target_still_legal(state, slot, chosen):
                state.log.append(_fizzle_message(state, chosen))
                continue
            obj = next((o for o in state.battlefield if o.id == chosen[1]), None)
            if obj is None:  # pragma: no cover - legality checked above
                continue
            state.add_continuous_effect(
                source_id=source.id if source is not None else obj.id,
                layer=7, sublayer=2, kind="pump_pt", target_id=obj.id,
                params=(self.power, self.toughness),
                expires_at="controller_untap",
                expires_at_player_id=controller_id,
                description=(
                    f"{obj.printing.name} gets {self.power:+d}/"
                    f"{self.toughness:+d} until P{controller_id}'s next turn"
                ),
            )
            state.log.append(
                f"{obj.printing.name} gets {self.power:+d}/{self.toughness:+d} "
                "until your next turn"
            )

    def describe(self) -> str:
        return (
            f"target gets {self.power:+d}/{self.toughness:+d} until your "
            "next turn"
        )


@dataclass(frozen=True)
class EventSubjectGetsUEOT(Effect):
    """"it gets +P/+T until end of turn" where "it" is the triggering
    event's object (the attacking creature of "whenever a creature
    attacks ..."): the stack item remembers it (``event_subject_id``)."""

    power: int = 0
    toughness: int = 0

    def event_subject_id(self, subjects) -> int:
        for value in subjects.values():
            if hasattr(value, "printing"):
                return value.id
        return -1

    def resolve(self, state, controller_id, targets=()):
        oid = state.resolving_event_subject_id
        obj = next((o for o in state.battlefield if o.id == oid), None)
        if obj is None:
            return
        # The engine's until-end-of-turn P/T bonus (cleared at cleanup).
        obj.temp_power_bonus += self.power
        obj.temp_toughness_bonus += self.toughness
        state.log.append(
            f"{obj.printing.name} gets {self.power:+d}/{self.toughness:+d} "
            "until end of turn"
        )

    def describe(self) -> str:
        return f"it gets {self.power:+d}/{self.toughness:+d} until end of turn"


@dataclass(frozen=True)
class CreateTriggersUntilYourNextTurn(Effect):
    """"Until your next turn, <triggered ability>." -- a delayed
    triggered ability that exists until the controller's next turn
    (CR 611.2b / 603.7): a command-zone holder carrying the abilities
    (the emblem trigger source ``dispatch_event`` scans), removed at the
    controller's next untap step (``GameObject.expires_at_untap_of``)."""

    triggered_abilities: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.cards.base import make_emblem
        from mtgz_ir.game import Zone

        source = state.resolving_source
        label = source.printing.name if source is not None else "effect"
        holder = state.new_object(
            make_emblem(
                name=f"{label} (until your next turn)",
                triggered_abilities=self.triggered_abilities,
            ),
            owner_id=controller_id, zone=Zone.COMMAND,
        )
        holder.expires_at_untap_of = controller_id
        holder.etb_timestamp = state.next_timestamp
        state.next_timestamp += 1
        state.emblems.append(holder)
        state.log.append(
            f"P{controller_id} gets a delayed triggered ability until "
            f"their next turn ({label})"
        )

    def describe(self) -> str:
        return "until your next turn: " + "; ".join(
            ta.describe() for ta in self.triggered_abilities
        )


@dataclass(frozen=True)
class CreateEmblemWithStatics(Effect):
    """'You get an emblem with "<static ability>."' (Tamiyo, Seasoned
    Scholar's "You have no maximum hand size."): an emblem carrying the
    static abilities (``turn.run_cleanup_step`` reads emblems' statics
    for the hand-size rule)."""

    name: str = "emblem"
    static_abilities: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.cards.base import make_emblem
        from mtgz_ir.game import Zone

        emblem = state.new_object(
            make_emblem(name=self.name, static_abilities=self.static_abilities),
            owner_id=controller_id, zone=Zone.COMMAND,
        )
        emblem.etb_timestamp = state.next_timestamp
        state.next_timestamp += 1
        state.emblems.append(emblem)
        state.log.append(f"P{controller_id} gets an emblem ({self.name})")

    def describe(self) -> str:
        return f"you get an emblem ({self.name})"


@dataclass(frozen=True)
class LoseLifeEqualToEventAmount(Effect):
    """"you lose that much life" -- the damage amount of the triggering
    event (``state.resolving_combat_damage_amount``)."""

    def resolve(self, state, controller_id, targets=()):
        n = max(0, state.resolving_combat_damage_amount)
        if not n:
            return
        p = state.player_by_id(controller_id)
        p.life -= n
        state.log.append(f"P{controller_id} loses {n} life (life: {p.life})")

    def describe(self) -> str:
        return "you lose that much life"


@dataclass(frozen=True)
class PoisonCountersEqualToDamage(Effect):
    """"they get that many poison counters" -- the player the
    combat-damage event hit (its fixed target) gets poison counters equal
    to the damage dealt."""

    def resolve(self, state, controller_id, targets=()):
        n = max(0, state.resolving_combat_damage_amount)
        if not targets or targets[0][0] != "player" or not n:
            return
        p = state.player_by_id(targets[0][1])
        p.poison_counters += n
        state.log.append(
            f"P{p.id} gets {n} poison counter(s) (now {p.poison_counters})"
        )

    def describe(self) -> str:
        return "they get that many poison counters"


@dataclass(frozen=True)
class PutPlusOneCounterOnEachMatching(Effect):
    """"Put a +1/+1 counter on each <noun> you control." (Cats, tapped
    creatures): ``predicate`` is the composed noun filter."""

    predicate: object = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import add_plus_one_counters

        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            matching = [
                o for o in state.battlefield
                if o.controller_id == controller_id
                and self.predicate.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        for obj in matching:
            add_plus_one_counters(state, obj, 1)
        state.log.append(
            f"P{controller_id} puts a +1/+1 counter on {len(matching)} "
            "permanent(s)"
        )

    def describe(self) -> str:
        return "put a +1/+1 counter on each matching permanent you control"


@dataclass(frozen=True)
class ReturnTargetCardFromYourGraveyardToHand(Effect):
    """"Return target <filter> card from your graveyard to your hand[.
    If it's a <color> card, <bonus>]." (Tamiyo, Seasoned Scholar's -3):
    ``bonus`` resolves when the returned card has ``bonus_color``."""

    card_filter: CardFilter = CardFilter()
    bonus_color: "str | None" = None
    bonus: "Effect | None" = None

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("graveyard_card", c.id)
            for c in state.player_by_id(controller_id).graveyard
            if self.card_filter.matches_card(state, c)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import effective_colors

        from mtgz_ir.game import Zone, reveal_to_all

        if not targets:
            return
        p = state.player_by_id(controller_id)
        card = next(
            (c for c in p.graveyard
             if c.id == targets[0][1] and self.card_filter.matches_card(state, c)),
            None,
        )
        if card is None:
            state.log.append("return target is gone")
            return
        p.graveyard.remove(card)
        card.zone = Zone.HAND
        p.hand.append(card)
        reveal_to_all(state, card)
        state.log.append(f"{card.printing.name} returns to P{controller_id}'s hand")
        if (
            self.bonus is not None
            and self.bonus_color in effective_colors(state, card)
        ):
            self.bonus.resolve(state, controller_id, ())

    def describe(self) -> str:
        text = (
            f"return target {self.card_filter.describe()} from your graveyard "
            "to your hand"
        )
        if self.bonus is not None:
            text += f"; if it's {self.bonus_color}, {self.bonus.describe()}"
        return text


@dataclass(frozen=True)
class DrawHalfLibraryRoundedUp(Effect):
    """"Draw cards equal to half the number of cards in your library,
    rounded up." """

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        n = (len(state.player_by_id(controller_id).library) + 1) // 2
        draw_n_cards(state, controller_id, n)

    def describe(self) -> str:
        return "draw cards equal to half your library, rounded up"


@dataclass(frozen=True)
class EachChoosesOneOfEachTypeSacrificesRest(Effect):
    """"Each <opponent|player> chooses an artifact, a creature, an
    enchantment, and a planeswalker from among the nonland permanents
    they control, then sacrifices the rest." (Ajani, Nacatl Avenger's
    -4.) Each affected player (APNAP order) picks, per listed card
    type, one permanent of that type they control (the same permanent
    may be picked for several types it has) through ``choose_legend_keep``
    (the "which one stays" decision), then sacrifices every other
    permanent in the pool."""

    card_types: tuple = ("Artifact", "Creature", "Enchantment", "Planeswalker")
    opponents_only: bool = True
    nonland_only: bool = True

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import has_card_type
        from mtgz_ir.effects.generic.mass_effects import _apnap
        from mtgz_ir.turn import sacrifice

        def has_type(o, t):
            return has_card_type(state, o, t)

        for p in _apnap(state, controller_id):
            if self.opponents_only and p.id == controller_id:
                continue
            pool = [
                o for o in state.battlefield
                if o.controller_id == p.id
                and not (self.nonland_only and o.printing.is_land)
            ]
            kept: set[int] = set()
            for t in self.card_types:
                options = [o for o in pool if has_type(o, t)]
                if not options:
                    continue
                pick = options[0]
                if p.agent is not None and len(options) > 1:
                    pid = p.agent.choose_legend_keep(
                        state, p.id, [o.id for o in options],
                    )
                    pick = next((o for o in options if o.id == pid), options[0])
                kept.add(pick.id)
            doomed = [o for o in pool if o.id not in kept]
            for o in doomed:
                if o in state.battlefield:
                    sacrifice(state, o)
            state.log.append(
                f"P{p.id} keeps {len(kept)} and sacrifices {len(doomed)}"
            )

    def describe(self) -> str:
        who = "each opponent" if self.opponents_only else "each player"
        types = ", ".join(t.lower() for t in self.card_types)
        return f"{who} keeps one {types}; sacrifices the rest"


@dataclass(frozen=True)
class MayCastTargetGraveyardCardThisTurnExileInstead(Effect):
    """"You may cast target <filter> card from your graveyard this turn.
    If that spell would be put into your graveyard, exile it instead."
    (Jace, Telepath Unbound's -3): the card may be cast this turn for
    its mana cost at its normal timing (``castable_from_graveyard_this_
    turn``) and is exiled if it would go to the graveyard from the stack
    (``exile_if_put_into_graveyard_from_stack``); cleanup clears both
    while the card is still in the graveyard."""

    card_filter: CardFilter = CardFilter()

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("graveyard_card", c.id)
            for c in state.player_by_id(controller_id).graveyard
            if self.card_filter.matches_card(state, c)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import _CowList

        if not targets:
            return
        p = state.player_by_id(controller_id)
        if not any(
            c.id == targets[0][1] and self.card_filter.matches_card(state, c)
            for c in p.graveyard
        ):
            state.log.append("cast-permission target is gone")
            return
        if isinstance(p.graveyard, _CowList):
            p.graveyard.fork()
        card = next(c for c in p.graveyard if c.id == targets[0][1])
        card.castable_from_graveyard_this_turn = True
        card.exile_if_put_into_graveyard_from_stack = True
        state.log.append(
            f"P{controller_id} may cast {card.printing.name} from their "
            "graveyard this turn (exiled afterwards)"
        )

    def describe(self) -> str:
        return (
            f"you may cast target {self.card_filter.describe()} from your "
            "graveyard this turn; exile it instead of putting it into your "
            "graveyard"
        )


@dataclass(frozen=True)
class EachPlayerExilesUntilNonlandMayCastFree(Effect):
    """"Each player exiles cards from the top of their library until they
    exile a nonland card. You may cast any number of spells from among
    the nonland cards exiled this way without paying their mana costs."
    (Etali, Primal Conqueror.) Players exile in APNAP order; then, for
    each nonland card exiled this way that can be cast right now, the
    controller chooses (``choose_yes_no``) whether to cast it for free
    during the resolution (``creature_casting_ub.cast_now``, CR 608.2g)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.effects.generic.mass_effects import _apnap
        from mtgz_ir.game import Zone

        found: list = []
        for p in _apnap(state, controller_id):
            exiled = 0
            while p.library:
                card = p.library.pop(0)
                card.zone = Zone.EXILE
                p.exile.append(card)
                exiled += 1
                if not card.printing.is_land:
                    found.append(card)
                    break
            state.log.append(f"P{p.id} exiles {exiled} card(s)")
        caster = state.player_by_id(controller_id)
        for card in found:
            owner = state.player_by_id(card.owner_id)
            if card not in owner.exile or not can_cast_now(
                state, controller_id, card, free=True,
            ):
                continue
            if caster.agent is not None and not caster.agent.choose_yes_no(
                state, controller_id,
                f"Cast {card.printing.name} without paying its mana cost?",
            ):
                continue
            cast_now(state, controller_id, card, free=True)

    def describe(self) -> str:
        return (
            "each player exiles cards from the top of their library until "
            "a nonland card; you may cast any of those for free"
        )


@dataclass(frozen=True)
class Airbend(Effect):
    """"Airbend up to one other target creature or spell" (Avatar: The
    Last Airbender keyword action): "Exile it. While it's exiled, its
    owner may cast it for {2} rather than its mana cost." A creature is
    exiled from the battlefield (a token ceases to exist), a spell from
    the stack; the owner then holds a never-expiring cast permission with
    the alternative cost ``alt_cost`` (``ExilePlayPermission``)."""

    alt_cost: str = "{2}"

    def requires_target(self) -> bool:
        return True

    def target_count_range(self):
        return (0, 1)

    def legal_targets(self, state, controller_id):
        from mtgz_ir.game import is_creature_now

        source = state.resolving_source
        out = [
            ("creature", o.id) for o in state.battlefield
            if is_creature_now(o) and (source is None or o.id != source.id)
        ]
        out.extend(
            ("stack", i) for i, item in enumerate(state.stack)
            if item.ability is None and not item.countered
        )
        return out

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import move_spell_off_stack
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            WHILE_EXILED,
            grant_exile_play,
        )
        from mtgz_ir.game import is_creature_now
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        kind, tid = targets[0]
        if kind == "stack":
            if not (0 <= tid < len(state.stack)):
                return
            item = state.stack[tid]
            if item.ability is not None or item.countered:
                return
            del state.stack[tid]
            card = item.source
            move_spell_off_stack(state, card, destination="exile")
        else:
            card = next((o for o in state.battlefield if o.id == tid), None)
            if card is None or not is_creature_now(card):
                return
            move_off_battlefield(state, card, destination="exile")
        owner = state.player_by_id(card.owner_id)
        if card not in owner.exile:
            return  # a token / spell copy ceased to exist
        grant_exile_play(
            state, card, card.owner_id, duration=WHILE_EXILED,
            cast_only=True, alt_cost=self.alt_cost,
        )
        state.log.append(
            f"{card.printing.name} is airbent (P{card.owner_id} may cast it "
            f"for {self.alt_cost})"
        )

    def describe(self) -> str:
        return "airbend up to one other target creature or spell"


@dataclass(frozen=True)
class SourceConnives(Effect):
    """"<this creature> connives" (CR 701.50): its controller draws a
    card, then discards a card; if a nonland card was discarded, put a
    +1/+1 counter on it (if it's still on the battlefield)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.cube_creatures_rmc import _discard_chosen
        from mtgz_ir.game import add_plus_one_counters
        from mtgz_ir.turn import draw_n_cards

        source = state.resolving_source
        who = source.controller_id if source is not None else controller_id
        draw_n_cards(state, who, 1)
        discarded = _discard_chosen(state, who, 1)
        nonland = sum(1 for c in discarded if not c.printing.is_land)
        if nonland and source is not None and source in state.battlefield:
            add_plus_one_counters(state, source, nonland)
        state.log.append(
            f"{source.printing.name if source else 'it'} connives "
            f"({nonland} counter(s))"
        )

    def describe(self) -> str:
        return "this creature connives"


@dataclass(frozen=True)
class SourceBecomesPrepared(Effect):
    """"this creature becomes prepared" (``layouts.become_prepared``)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.layouts import become_prepared

        source = state.resolving_source
        if source is not None:
            become_prepared(state, source)

    def describe(self) -> str:
        return "this creature becomes prepared"


from mtgz_ir.effects.state_predicates import StatePredicate  # noqa: E402


@dataclass(frozen=True)
class YouControlAtLeast(StatePredicate):
    """"you control N or more <card type>s" (an intervening-if gate)."""

    n: int = 1
    card_type: str = "Land"

    def test(self, state, viewer_id):
        from mtgz_ir.game import has_card_type

        def has(o):
            return has_card_type(state, o, self.card_type)

        return sum(
            1 for o in state.battlefield
            if o.controller_id == viewer_id and has(o)
        ) >= self.n
