"""Engine effects for the PowerLSV cube's blue / black creatures.

Each class models one printed effect shape (not one card); the parser
side lives in :mod:`mtgz_ir.cards.parser.cube_creatures_ub`.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import EventSubject, StackObjectKind, coerce_tuple
from mtgz_ir.effects.base import Effect, Target


def grant_exile_play(state, card, player_id, **kwargs):
    """The base's exile play permission (``cube_creatures_rmc``)."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        grant_exile_play as _grant,
    )

    _grant(state, card, player_id, **kwargs)
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.formulas import Formula
from mtgz_ir.effects.predicates import TriggerPredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# ---------------------------------------------------------------------------
# Event-bound anaphora ("they", "that player")
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class BindEventPlayer(Effect):
    """Resolve ``inner`` against the player named by the triggering
    event's ``slot`` subject -- the "they" of "Whenever an opponent
    draws a card, they lose 2 life." The player is fixed when the
    trigger fires (``dispatch_event``'s ``targets_from_event`` hook), not
    targeted, so the ability never prompts and can't fizzle for lack of
    a target.
    """

    inner: Effect
    slot: EventSubject = EventSubject.PLAYER

    def __post_init__(self) -> None:
        object.__setattr__(self, "slot", EventSubject(self.slot))

    def targets_from_event(self, state: "GameState", subjects) -> tuple:
        player = subjects.get(self.slot)
        if player is None:
            return ()
        return (("player", player.id),)

    def requires_target(self) -> bool:
        return False

    def legal_targets(self, state, controller_id):
        return []

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        self.inner.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return self.inner.describe()


# ---------------------------------------------------------------------------
# Amass (CR 701.44)
# ---------------------------------------------------------------------------


def _army_token_printing(subtype: str):
    from mtgz_ir.cards.base import make_token_creature

    return make_token_creature(
        f"{subtype} Army", 0, 0, subtypes=(subtype, "Army"), colors={"B"},
    )


@dataclass(frozen=True)
class Amass(Effect):
    """``amass <Subtype> N`` (CR 701.44a): if you don't control an Army
    creature, create a 0/0 black <Subtype> Army creature token; choose
    an Army creature you control, put N +1/+1 counters on it, and if it
    isn't a <Subtype> it becomes one in addition to its other types."""

    subtype: str
    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters, is_creature_now

        def armies():
            return [
                o for o in state.battlefield
                if o.controller_id == controller_id
                and is_creature_now(o)
                and "Army" in o.printing.subtypes
            ]

        if not armies():
            from mtgz_ir.effects.generic.tokens import CreateToken

            CreateToken(
                token_printing=_army_token_printing(self.subtype),
            ).resolve(state, controller_id, ())
        pool = armies()
        if not pool:
            return
        army = pool[0]
        p = state.player_by_id(controller_id)
        if len(pool) > 1 and p.agent is not None:
            pick = p.agent.choose_target(
                state, controller_id, [("creature", o.id) for o in pool],
            )
            if pick is not None:
                army = next((o for o in pool if o.id == pick[1]), army)
        add_plus_one_counters(state, army, self.amount)
        if self.subtype not in army.printing.subtypes:
            army.printing = dataclasses.replace(
                army.printing,
                subtypes=army.printing.subtypes + (self.subtype,),
            )
        state.log.append(
            f"P{controller_id} amasses {self.subtype} {self.amount} "
            f"({army.printing.name})"
        )

    def describe(self) -> str:
        return f"amass {self.subtype}s {self.amount}"


# ---------------------------------------------------------------------------
# Self-referential riders
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TapSelf(Effect):
    """"Tap it." where "it" is the ability's own source (Iron-Shield
    Elf's "This creature gains indestructible until end of turn. Tap
    it."). No-op when the source has left the battlefield."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import tap_permanent

        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        tap_permanent(state, src)

    def describe(self) -> str:
        return "tap this creature"


@dataclass(frozen=True)
class SourceCantBeBlockedThisTurn(Effect):
    """"This creature can't be blocked this turn." -- the ability's own
    source gets the layer-6 can't-be-blocked restriction until end of
    turn (same registry entry ``GrantUnblockableUEOT`` writes)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        state.add_continuous_effect(
            source_id=src.id, layer=6, kind="cant_be_blocked",
            target_id=src.id, params=(), expires_at="cleanup",
            description=f"{src.printing.name} can't be blocked this turn",
        )
        state.log.append(f"{src.printing.name} can't be blocked this turn")

    def describe(self) -> str:
        return "this creature can't be blocked this turn"


# ---------------------------------------------------------------------------
# Graveyard cast permissions
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class MayCastTargetGraveyardCardThisTurn(Effect):
    """"Choose target artifact card in your graveyard. You may cast
    that card this turn." (Emry, Lurker of the Loch). The permission
    rides on the card (``castable_from_graveyard_this_turn``) until
    cleanup; ``legal_actions`` offers the cast at its normal timing for
    its mana cost."""

    card_filter: object = None

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_card", c.id) for c in p.graveyard
            if self.card_filter.matches_card(state, c)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import _CowList

        if not targets:
            return
        _, tid = targets[0]
        p = state.player_by_id(controller_id)
        if not any(c.id == tid for c in p.graveyard):
            state.log.append("cast-permission target is gone")
            return
        if isinstance(p.graveyard, _CowList):
            p.graveyard.fork()
        card = next(c for c in p.graveyard if c.id == tid)
        card.castable_from_graveyard_this_turn = True
        state.log.append(
            f"P{controller_id} may cast {card.printing.name} from their "
            f"graveyard this turn"
        )

    def describe(self) -> str:
        return (
            f"choose target {self.card_filter.describe()} in your graveyard; "
            f"you may cast it this turn"
        )


# ---------------------------------------------------------------------------
# Ninjutsu
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PutSourceOntoBattlefieldTappedAndAttacking(Effect):
    """Ninjutsu's effect: "Put this card onto the battlefield from your
    hand tapped and attacking" (CR 702.49a/c). The ninja attacks the
    player the returned creature was attacking (the defending player);
    it was never declared as an attacker, so "whenever attacks"
    triggers don't fire."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        card = state.resolving_source
        p = state.player_by_id(controller_id)
        if card is None or card not in p.hand:
            return
        p.hand.remove(card)
        put_into_battlefield(state, card, controller_id)
        if card in state.battlefield:
            card.tapped = True
            card.attacking = True
            state.log.append(
                f"{card.printing.name} enters tapped and attacking"
            )

    def describe(self) -> str:
        return (
            "put this card onto the battlefield from your hand tapped and "
            "attacking"
        )


# ---------------------------------------------------------------------------
# Exile play permissions
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetPlayerExilesTopNYouMayPlayThem(Effect):
    """"That player exiles the top N cards of their library. Until end
    of turn, you may play those cards [without paying their mana
    costs]." (Fallen Shinobi). The player is the (fixed) target -- the
    damaged player on a combat-damage trigger."""

    amount: int = 1
    free: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets or targets[0][0] != "player":
            return
        p = state.player_by_id(targets[0][1])
        for _ in range(self.amount):
            if not p.library:
                break
            card = p.library.pop(0)
            card.zone = Zone.EXILE
            p.exile.append(card)
            grant_exile_play(state, card, controller_id, free=self.free)
            state.log.append(
                f"P{p.id} exiles {card.printing.name}; P{controller_id} "
                f"may play it this turn"
            )

    def describe(self) -> str:
        free = " without paying their mana costs" if self.free else ""
        return (
            f"that player exiles the top {self.amount} cards of their "
            f"library; until end of turn, you may play them{free}"
        )


@dataclass(frozen=True)
class ShuffleThenExileTopMayPlayFree(Effect):
    """"Shuffle your library, then exile the top card. Until end of
    turn, you may play that card without paying its mana cost." (Urza,
    Lord High Artificer)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        shuffle_library(state, controller_id)
        if not p.library:
            return
        card = p.library.pop(0)
        card.zone = Zone.EXILE
        p.exile.append(card)
        grant_exile_play(state, card, controller_id, free=True)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name}; may play it "
            f"free this turn"
        )

    def describe(self) -> str:
        return (
            "shuffle your library, then exile the top card; until end of "
            "turn, you may play it without paying its mana cost"
        )


@dataclass(frozen=True)
class MayMillDamageThenMayTakeCreature(Effect):
    """"You may mill that many cards. If you do, you may put a creature
    card from among them into your hand." (Barrowgoyf) -- "that many"
    is the combat damage dealt (``state.resolving_combat_damage_amount``).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        n = state.resolving_combat_damage_amount
        p = state.player_by_id(controller_id)
        if n <= 0 or not p.library:
            return
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, f"Mill {n} cards?",
        ):
            return
        milled = []
        for _ in range(n):
            if not p.library:
                break
            card = p.library.pop(0)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            milled.append(card)
        state.log.append(f"P{controller_id} mills {len(milled)} cards")
        creatures = [c for c in milled if c.printing.is_creature]
        if not creatures or p.agent is None:
            return
        pick = p.agent.choose_card_from_library(
            state, controller_id, [c.id for c in creatures],
        )
        chosen = next((c for c in creatures if c.id == pick), None)
        if chosen is None:
            return
        p.graveyard.remove(chosen)
        chosen.zone = Zone.HAND
        p.hand.append(chosen)
        state.log.append(
            f"P{controller_id} puts {chosen.printing.name} into their hand"
        )

    def describe(self) -> str:
        return (
            "you may mill that many cards; if you do, you may put a "
            "creature card from among them into your hand"
        )


@dataclass(frozen=True)
class ChooseOpponentsExiledCardWithCounterMayPlayFree(Effect):
    """"Choose an exiled card an opponent owns with a <kind> counter on
    it. You may play it this turn without paying its mana cost."
    (Dauthi Voidwalker's sacrifice ability)."""

    counter: str = "void"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import _CowList, named_counter_count

        options = [
            c for pl in state.players if pl.id != controller_id
            for c in pl.exile if named_counter_count(c, self.counter) > 0
        ]
        if not options:
            return
        p = state.player_by_id(controller_id)
        chosen = options[0]
        if p.agent is not None:
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            chosen = next((c for c in options if c.id == pick), None)
        if chosen is None:
            return
        owner = state.player_by_id(chosen.owner_id)
        if isinstance(owner.exile, _CowList):
            owner.exile.fork()
            chosen = next(c for c in owner.exile if c.id == chosen.id)
        grant_exile_play(state, chosen, controller_id, free=True)
        state.log.append(
            f"P{controller_id} chooses {chosen.printing.name}; may play it "
            f"free this turn"
        )

    def describe(self) -> str:
        return (
            f"choose an exiled card an opponent owns with a {self.counter} "
            "counter on it; you may play it this turn without paying its "
            "mana cost"
        )


@dataclass(frozen=True)
class ExileUntilNonlandThenCounterOrMayCastIt(Effect):
    """Black Widow, Super Spy: "that player exiles cards from the top of
    their library until they exile a nonland card. You may put a +1/+1
    counter on this creature. If you don't, you may cast the exiled
    nonland card until end of turn and mana of any type can be spent to
    cast that spell." The player is the fixed target (the damaged
    player)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, add_plus_one_counters

        if not targets or targets[0][0] != "player":
            return
        victim = state.player_by_id(targets[0][1])
        found = None
        while victim.library:
            card = victim.library.pop(0)
            card.zone = Zone.EXILE
            victim.exile.append(card)
            state.log.append(f"P{victim.id} exiles {card.printing.name}")
            if not card.printing.is_land:
                found = card
                break
        me = state.player_by_id(controller_id)
        src = state.resolving_source
        take_counter = src is not None and src in state.battlefield and (
            me.agent is None or me.agent.choose_yes_no(
                state, controller_id,
                f"Put a +1/+1 counter on {src.printing.name}?",
            )
        )
        if take_counter:
            add_plus_one_counters(state, src, 1)
            return
        if found is not None:
            grant_exile_play(
                state, found, controller_id, cast_only=True, any_color_mana=True,
            )
            state.log.append(
                f"P{controller_id} may cast {found.printing.name} this turn"
            )

    def describe(self) -> str:
        return (
            "that player exiles cards from the top until a nonland card; "
            "put a +1/+1 counter on this creature or you may cast that card "
            "this turn (mana of any type)"
        )


@dataclass(frozen=True)
class LootThenMayCastDiscardedFreeIfCounters(Effect):
    """Malcolm, Alluring Scoundrel: "Draw a card, then discard a card.
    If there are N or more <kind> counters on this creature, you may
    cast the discarded card without paying its mana cost." """

    kind: str = "chorus"
    threshold: int = 4

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.game import named_counter_count
        from mtgz_ir.turn import draw_n_cards

        p = state.player_by_id(controller_id)
        draw_n_cards(state, controller_id, 1)
        if not p.hand:
            return
        card = p.hand[0]
        if p.agent is not None:
            pick = p.agent.choose_discard(state, controller_id, [c.id for c in p.hand])
            card = next((c for c in p.hand if c.id == pick), card)
        discard_card(state, p, card)
        src = state.resolving_source
        if src is None or named_counter_count(src, self.kind) < self.threshold:
            return
        if card not in p.graveyard or not can_cast_now(
            state, controller_id, card, free=True,
        ):
            return
        if p.agent is None or p.agent.choose_yes_no(
            state, controller_id, f"Cast {card.printing.name} for free?",
        ):
            cast_now(state, controller_id, card, free=True)

    def describe(self) -> str:
        return (
            f"draw a card, then discard a card; with {self.threshold}+ "
            f"{self.kind} counters, you may cast the discarded card free"
        )


@dataclass(frozen=True)
class MayCastSpellFromGraveyardNow(Effect):
    """Bilbo, Thief in the Night: "you may cast an artifact, instant, or
    sorcery spell from your graveyard. If an instant or sorcery spell
    cast this way would be put into your graveyard, exile it instead."
    The spell is cast immediately (paying its costs)."""

    card_filter: object = None
    exile_instants_and_sorceries: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now

        p = state.player_by_id(controller_id)
        options = [
            c for c in p.graveyard
            if self.card_filter.matches_card(state, c)
            and can_cast_now(state, controller_id, c, free=False)
        ]
        if not options:
            return
        chosen = options[0]
        if p.agent is not None:
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            chosen = next((c for c in options if c.id == pick), None)
        if chosen is None:
            return
        if self.exile_instants_and_sorceries and (
            chosen.printing.is_instant or chosen.printing.is_sorcery
        ):
            chosen.exile_if_put_into_graveyard_from_stack = True
        cast_now(state, controller_id, chosen, free=False)

    def describe(self) -> str:
        return (
            f"you may cast a {self.card_filter.describe()} spell from your "
            "graveyard (instants and sorceries are exiled afterwards)"
        )


@dataclass(frozen=True)
class TargetOpponentExilesThreeMayCastOneFree(Effect):
    """Crabomination: "target opponent exiles the top card of their
    library, a card at random from their graveyard, and a card at random
    from their hand. You may cast a spell from among cards exiled this
    way without paying its mana cost." """

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.game import Zone

        if not targets or targets[0] not in self.legal_targets(
            state, controller_id,
        ):
            return
        opp = state.player_by_id(targets[0][1])
        exiled = []
        if opp.library:
            exiled.append(opp.library.pop(0))
        if opp.graveyard:
            card = state.rng.choice(list(opp.graveyard))
            opp.graveyard.remove(card)
            exiled.append(card)
        if opp.hand:
            card = state.rng.choice(list(opp.hand))
            opp.hand.remove(card)
            exiled.append(card)
        for card in exiled:
            card.zone = Zone.EXILE
            opp.exile.append(card)
            state.log.append(f"P{opp.id} exiles {card.printing.name}")
        options = [
            c for c in exiled if can_cast_now(state, controller_id, c, free=True)
        ]
        if not options:
            return
        me = state.player_by_id(controller_id)
        chosen = options[0]
        if me.agent is not None:
            pick = me.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            chosen = next((c for c in options if c.id == pick), None)
        if chosen is not None:
            cast_now(state, controller_id, chosen, free=True)

    def describe(self) -> str:
        return (
            "target opponent exiles the top card of their library and a "
            "random card from their graveyard and hand; you may cast one "
            "of them without paying its mana cost"
        )


@dataclass(frozen=True)
class MayCastSourceFromHandForMiracleCost(Effect):
    """The miracle trigger (CR 702.94a): "When you reveal it this way,
    you may cast it by paying [miracle cost] rather than its mana
    cost." The source is the revealed card in hand."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import cast_from_hand_for

        card = state.resolving_source
        if card is None or card.printing.miracle_cost is None:
            return
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            f"Cast {card.printing.name} for {card.printing.miracle_cost_str}?",
        ):
            return
        cast_from_hand_for(state, controller_id, card, card.printing.miracle_cost)

    def describe(self) -> str:
        return "you may cast this card for its miracle cost"


@dataclass(frozen=True)
class ExileGraveyardCardById(Effect):
    """Exile the (fixed) graveyard card named by ``targets[0]`` if it's
    still in a graveyard ("When you do, exile that card.")."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        tid = targets[0][1]
        for p in state.players:
            card = next((c for c in p.graveyard if c.id == tid), None)
            if card is not None:
                p.graveyard.remove(card)
                card.zone = Zone.EXILE
                p.exile.append(card)
                state.log.append(f"{card.printing.name} is exiled")
                return

    def describe(self) -> str:
        return "exile that card"


@dataclass(frozen=True)
class BecomeSaddled(Effect):
    """"This Mount becomes saddled until end of turn." (CR 702.171b)"""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_source
        if src is not None and src in state.battlefield:
            src.saddled = True
            state.log.append(f"{src.printing.name} becomes saddled")

    def describe(self) -> str:
        return "this Mount becomes saddled until end of turn"


# ---------------------------------------------------------------------------
# Parity choice / once-per-mode modal (Gollum, Riddle Master)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SpellMVHasSourceParity(TriggerPredicate):
    """Trigger predicate: the cast spell's mana value is odd / even per
    the source's ``chosen_parity`` ("a spell with mana value of the
    chosen quality"; 0 is even). No choice -> never."""

    def matches(self, state, subjects, source) -> bool:
        spell = subjects.get("spell")
        parity = getattr(source, "chosen_parity", None)
        if spell is None or parity is None:
            return False
        mv = spell.printing.mana_cost.total
        return (mv % 2 == 1) == (parity == "odd")


@dataclass(frozen=True)
class ModalChooseOneNotYetChosen(Effect):
    """"choose one that hasn't been chosen —" (CR 700.2): each mode can
    be chosen once for this object; once all have been chosen the
    ability does nothing. Used modes are recorded on the source
    (``GameObject.modes_chosen``)."""

    modes: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_source
        used = tuple(src.modes_chosen) if src is not None else ()
        open_modes = [i for i in range(len(self.modes)) if i not in used]
        if not open_modes:
            state.log.append("every mode has already been chosen")
            return
        p = state.player_by_id(controller_id)
        pick = open_modes[0]
        if p.agent is not None and len(open_modes) > 1:
            idx = p.agent.choose_mode(
                state, controller_id,
                [self.modes[i].describe() for i in open_modes],
            )
            pick = open_modes[max(0, min(idx, len(open_modes) - 1))]
        if src is not None:
            src.modes_chosen = used + (pick,)
        state.log.append(f"mode chosen: {self.modes[pick].describe()}")
        self.modes[pick].resolve(state, controller_id, ())

    def describe(self) -> str:
        return "choose one that hasn't been chosen: " + " / ".join(
            m.describe() for m in self.modes
        )


# ---------------------------------------------------------------------------
# Emperor of Bones
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ExileUpToOneTargetGraveyardCardWithSource(Effect):
    """"Exile up to one target card from a graveyard." -- linked to the
    source as "exiled with this creature" (``GameObject.exiled_with``);
    the card stays in exile when the source leaves."""

    def requires_target(self) -> bool:
        return False

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1)

    def legal_targets(self, state, controller_id):
        return [("card_in_graveyard", c.id) for p in state.players for c in p.graveyard]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        tid = targets[0][1]
        for p in state.players:
            card = next((c for c in p.graveyard if c.id == tid), None)
            if card is None:
                continue
            p.graveyard.remove(card)
            card.zone = Zone.EXILE
            src = state.resolving_source
            card.exiled_with = src.id if src is not None else None
            p.exile.append(card)
            state.log.append(f"{card.printing.name} exiled from P{p.id}'s graveyard")
            return

    def describe(self) -> str:
        return "exile up to one target card from a graveyard"


@dataclass(frozen=True)
class Adapt(Effect):
    """Adapt N (CR 701.46): if this permanent has no +1/+1 counters on
    it, put N +1/+1 counters on it."""

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters

        src = state.resolving_source
        if src is None or src not in state.battlefield or src.plus_one_counters:
            return
        add_plus_one_counters(state, src, self.amount)
        state.log.append(f"{src.printing.name} adapts {self.amount}")

    def describe(self) -> str:
        return f"adapt {self.amount}"


@dataclass(frozen=True)
class ReanimateExiledWithSourceFinalityHasteSacrifice(Effect):
    """Emperor of Bones: "put a creature card exiled with this creature
    onto the battlefield under your control with a finality counter on
    it. It gains haste. Sacrifice it at the beginning of the next end
    step." """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.delayed_actions import (
            SacrificeSourceDelayed, schedule_at_next_end_step,
        )
        from mtgz_ir.game import _CowList, set_named_counters

        src = state.resolving_source
        if src is None:
            return
        options = [
            (c, p) for p in state.players for c in p.exile
            if c.exiled_with == src.id and c.printing.is_creature
        ]
        if not options:
            return
        me = state.player_by_id(controller_id)
        card, owner = options[0]
        if me.agent is not None and len(options) > 1:
            pick = me.agent.choose_target(
                state, controller_id, [("card", c.id) for c, _ in options],
            )
            if pick is not None:
                card, owner = next(
                    ((c, p) for c, p in options if c.id == pick[1]), options[0],
                )
        if isinstance(owner.exile, _CowList):
            owner.exile.fork()
            card = next(c for c in owner.exile if c.id == card.id)
        owner.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        card.exiled_with = None
        card.named_counters = ()
        put_into_battlefield(state, card, controller_id)
        if card not in state.battlefield:  # pragma: no cover - defensive
            return
        set_named_counters(card, "finality", 1)
        state.add_continuous_effect(
            source_id=card.id, layer=6, kind="grant_keyword",
            target_id=card.id, params=("haste",),
            description=f"{card.printing.name} gains haste",
        )
        schedule_at_next_end_step(
            state, controller_id, card.id, SacrificeSourceDelayed(),
        )
        state.log.append(
            f"{card.printing.name} returns from exile with a finality counter"
        )

    def describe(self) -> str:
        return (
            "put a creature card exiled with this creature onto the "
            "battlefield under your control with a finality counter; it "
            "gains haste; sacrifice it at the next end step"
        )


@dataclass(frozen=True)
class GainControlOfTargetThenAttachIfEquipment(Effect):
    """"gain control of target artifact with mana value X or less. If
    that artifact is an Equipment, attach it to this creature."
    (Thieving Skydiver)."""

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            IsArtifact, ManaValueAtMostSourceX,
        )
        from mtgz_ir.effects.targets import TargetSpec

        object.__setattr__(self, "targets", (
            TargetSpec(IsArtifact() & ManaValueAtMostSourceX()),
        ))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_walkers import (
            GainControlOfTargetPermanently,
        )

        if not targets:
            return
        GainControlOfTargetPermanently(targets=self.targets).resolve(
            state, controller_id, targets,
        )
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        src = state.resolving_source
        if (
            obj is None or obj.controller_id != controller_id
            or "Equipment" not in obj.printing.subtypes
            or src is None or src not in state.battlefield
        ):
            return
        obj.attached_to = src.id
        state.log.append(f"{obj.printing.name} is attached to {src.printing.name}")

    def describe(self) -> str:
        return (
            "gain control of target artifact with mana value X or less; if "
            "it's an Equipment, attach it to this creature"
        )


@dataclass(frozen=True)
class OwnerPutsTargetSecondFromTopOrBottom(Effect):
    """"target nonland permanent's owner puts it into their library
    second from the top or on the bottom." (Wan Shi Tong) -- the owner
    chooses (``choose_yes_no``: yes = second from the top)."""

    def __post_init__(self) -> None:
        from mtgz_ir.effects.generic.cube_walkers import IsPermanent
        from mtgz_ir.effects.object_predicates import IsLand
        from mtgz_ir.effects.targets import TargetSpec

        object.__setattr__(self, "targets", (TargetSpec(IsPermanent() & ~IsLand()),))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is None or obj.printing.is_land:
            return
        owner = state.player_by_id(obj.owner_id)
        second = owner.agent is None or owner.agent.choose_yes_no(
            state, owner.id,
            f"Put {obj.printing.name} second from the top (no = bottom)?",
        )
        index = 1 if second else len(owner.library) + 1
        move_off_battlefield(state, obj, "library", library_index=index)

    def describe(self) -> str:
        return (
            "target nonland permanent's owner puts it into their library "
            "second from the top or on the bottom"
        )


@dataclass(frozen=True)
class OwnerPutsTargetSpellOnTopOrBottom(Effect):
    """"choose up to one target creature spell or planeswalker spell. Its
    owner puts it on their choice of the top or bottom of their library."
    (Subtlety.) ``card_types`` are the spell types that may be targeted;
    ``up_to_one`` makes the target optional. Not a counter: the spell
    leaves the stack at once even if it can't be countered. The owner
    chooses (``choose_yes_no``: yes = top). A copy of a spell ceases to
    exist instead (CR 707.10a). Later stack items' ``("stack", i)``
    targets above the removed spell are re-indexed."""

    card_types: tuple = ("Creature",)
    up_to_one: bool = False

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0 if self.up_to_one else 1, 1)

    def legal_targets(self, state, controller_id):
        return [
            ("stack", i) for i, item in enumerate(state.stack)
            if item.ability is None and not item.countered
            and any(t in item.source.printing.card_types for t in self.card_types)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all
        from mtgz_ir.turn import note_cards_put_into_library

        if not targets or targets[0] not in self.legal_targets(
            state, controller_id,
        ):
            return
        idx = targets[0][1]
        item = state.stack.pop(idx)
        for other in state.stack:
            if any(k == "stack" and j > idx for k, j in other.targets):
                other.targets = tuple(
                    (k, j - 1) if k == "stack" and j > idx else (k, j)
                    for k, j in other.targets
                )
        card = item.source
        if card.is_spell_copy:
            state.log.append(f"copy of {card.printing.name} ceases to exist")
            return
        owner = state.player_by_id(card.owner_id)
        top = owner.agent is None or owner.agent.choose_yes_no(
            state, owner.id,
            f"Put {card.printing.name} on top of your library (no = bottom)?",
        )
        card.zone = Zone.LIBRARY
        if top:
            owner.library.insert(0, card)
        else:
            owner.library.append(card)
        reveal_to_all(state, card)
        note_cards_put_into_library(state, owner.id)
        state.log.append(
            f"{card.printing.name} is put on the {'top' if top else 'bottom'} "
            f"of P{owner.id}'s library"
        )

    def describe(self) -> str:
        kinds = " or ".join(f"{t.lower()} spell" for t in self.card_types)
        return (
            f"choose {'up to one ' if self.up_to_one else ''}target {kinds}; "
            f"its owner puts it on the top or bottom of their library"
        )


@dataclass(frozen=True)
class LookAtHandMayExileNonlandUntilSourceLeaves(Effect):
    """"look at target opponent's hand. You may exile a nonland card from
    it until this creature leaves the battlefield." (Deep-Cavern Bat.)
    The exiled card carries the base ``exiled_by_source`` link plus
    ``exiled_until_leaves_from_hand`` so the on-leave release puts it
    back into its owner's hand. If the source has already left the
    battlefield when this resolves, nothing is exiled (CR 610.3c)."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to

        opp = next(
            (state.player_by_id(t[1]) for t in targets if t[0] == "player"),
            None,
        )
        if opp is None:
            return
        for c in opp.hand:
            reveal_to(c, controller_id)
        src = state.resolving_source
        if src is None or not any(o.id == src.id for o in state.battlefield):
            return
        options = [c for c in opp.hand if not c.printing.is_land]
        if not options:
            return
        me = state.player_by_id(controller_id)
        if me.agent is not None and not me.agent.choose_yes_no(
            state, controller_id,
            "exile a nonland card from their hand until this leaves?",
        ):
            return
        pick = options[0].id
        if me.agent is not None:
            pick = me.agent.choose_card_from_opponent_hand(
                state, controller_id, [c.id for c in options],
            )
        card = next((c for c in options if c.id == pick), options[0])
        opp.hand.remove(card)
        card.zone = Zone.EXILE
        card.exiled_by_source = src.id
        card.exiled_until_leaves_from_hand = True
        opp.exile.append(card)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} from P{opp.id}'s "
            f"hand until {src.printing.name} leaves the battlefield"
        )

    def describe(self) -> str:
        return (
            "look at target opponent's hand; you may exile a nonland card "
            "from it until this creature leaves the battlefield"
        )


# ---------------------------------------------------------------------------
# Library-top reveals
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RevealTopCardToHandLoseLifeEqualMV(Effect):
    """"Reveal the top card of your library and put that card into your
    hand. You lose life equal to its mana value." (Dark Confidant). An
    empty library reveals nothing and costs no life.

    ``unless_saddled``: Caustic Bronco's "... if this creature isn't
    saddled. Otherwise, each opponent loses that much life." """

    unless_saddled: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        if not p.library:
            return
        card = p.library.pop(0)
        card.zone = Zone.HAND
        p.hand.append(card)
        card.publicly_known_to = frozenset(pl.id for pl in state.players)
        mv = card.printing.mana_cost.total
        src = state.resolving_source
        if self.unless_saddled and src is not None and src.saddled:
            for opp in state.players:
                if opp.id != controller_id and not opp.has_lost:
                    opp.life -= mv
            state.log.append(
                f"P{controller_id} reveals {card.printing.name}; each "
                f"opponent loses {mv} life"
            )
            return
        p.life -= mv
        state.log.append(
            f"P{controller_id} reveals {card.printing.name} and puts it "
            f"into their hand, losing {mv} life (now {p.life})"
        )

    def describe(self) -> str:
        return (
            "reveal the top card of your library and put it into your "
            "hand; you lose life equal to its mana value"
        )


# ---------------------------------------------------------------------------
# "Target opponent <does A>, <does B>, and <does C>"
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetPlayerPerformsActions(Effect):
    """One targeted player performs a list of actions in printed order
    (Archon of Cruelty: "target opponent sacrifices a creature or
    planeswalker of their choice, discards a card, and loses 3 life").

    ``actions`` holds ``(verb, arg)`` pairs:

    * ``("sacrifice", CardFilter)`` -- a permanent of their choice
      matching the filter (none -> nothing happens)
    * ``("discard", n)`` -- n cards of their choice
    * ``("lose_life", n)``
    """

    actions: tuple = ()
    opponent_only: bool = True

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state: "GameState", controller_id: int):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost
            and not (self.opponent_only and p.id == controller_id)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.turn import sacrifice

        if not targets or targets[0][0] != "player":
            return
        p = state.player_by_id(targets[0][1])
        if p.has_lost:
            return
        for verb, arg in self.actions:
            if verb == "sacrifice":
                legal = [
                    o for o in state.battlefield
                    if o.controller_id == p.id and arg.matches_card(state, o)
                ]
                if not legal:
                    continue
                pick = legal[0]
                if p.agent is not None:
                    pid = p.agent.choose_sacrifice(
                        state, p.id, [o.id for o in legal],
                    )
                    pick = next((o for o in legal if o.id == pid), pick)
                state.log.append(f"P{p.id} sacrifices {pick.printing.name}")
                sacrifice(state, pick)
            elif verb == "discard":
                for _ in range(arg):
                    if not p.hand:
                        break
                    card = p.hand[0]
                    if p.agent is not None:
                        cid = p.agent.choose_discard(
                            state, p.id, [c.id for c in p.hand],
                        )
                        card = next((c for c in p.hand if c.id == cid), card)
                    discard_card(state, p, card)
            else:  # lose_life
                p.life -= arg
                state.log.append(f"P{p.id} loses {arg} life (now {p.life})")

    def describe(self) -> str:
        parts = []
        for verb, arg in self.actions:
            if verb == "sacrifice":
                parts.append(f"sacrifices a {arg.describe()[:-5]} of their choice")
            elif verb == "discard":
                parts.append(f"discards {arg} card(s)")
            else:
                parts.append(f"loses {arg} life")
        who = "target opponent" if self.opponent_only else "target player"
        return f"{who} " + ", ".join(parts)


# ---------------------------------------------------------------------------
# Squad / Offspring token copies
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CreateTokenCopiesOfSelf(Effect):
    """Create token copies of the resolving ability's source: one per
    squad payment (``per_squad_payment``, CR 702.157a), else one; with
    ``base_pt`` set the copies are that size (Offspring's "except it's
    1/1", CR 702.175a). The copy uses the source's copiable values (its
    printing), or its last known printing if it has left the
    battlefield."""

    per_squad_payment: bool = False
    base_pt: "tuple[int, int] | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.tokens import CreateToken

        src = state.resolving_source
        if src is None:
            return
        count = src.squad_paid_count if self.per_squad_payment else 1
        if count <= 0:
            return
        base = src.printing
        changes: dict = {}
        if "Token" not in base.supertypes:
            changes["supertypes"] = base.supertypes + ("Token",)
        if self.base_pt is not None:
            changes["power"], changes["toughness"] = self.base_pt
        CreateToken(
            token_printing=dataclasses.replace(base, **changes), count=count,
        ).resolve(state, controller_id, ())

    def describe(self) -> str:
        what = "a token copy of it"
        if self.per_squad_payment:
            what = "a token copy of it for each squad payment"
        if self.base_pt is not None:
            what += f", except it's {self.base_pt[0]}/{self.base_pt[1]}"
        return f"create {what}"


# ---------------------------------------------------------------------------
# Devotion / library-top selection (Thassa's Oracle)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DevotionTo(Formula):
    """Formula: your devotion to ``color`` (CR 700.5) -- the number of
    that color's mana symbols (hybrid and Phyrexian included) among the
    mana costs of permanents you control."""

    color: str

    def value(self, state: "GameState", controller_id: int) -> int:
        total = 0
        for o in state.battlefield:
            if o.controller_id != controller_id:
                continue
            mc = o.printing.mana_cost
            total += sum(n for c, n in mc.colored if c == self.color)
            total += sum(1 for pair in mc.hybrid if self.color in pair)
            total += sum(1 for c in mc.phyrexian if c == self.color)
        return total


@dataclass(frozen=True)
class LookTopNKeepUpToOneOnTopRestBottom(Effect):
    """"Look at the top N cards of your library. Put up to one of them
    on top of your library and the rest on the bottom of your library
    in a random order." The kept card is the agent's pick
    (``choose_card_from_library``; ``None`` keeps none)."""

    count: object = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        n = self.count if isinstance(self.count, int) else self.count.value(
            state, controller_id,
        )
        p = state.player_by_id(controller_id)
        n = max(0, min(n, len(p.library)))
        if n == 0:
            return
        looked = [p.library.pop(0) for _ in range(n)]
        keep = None
        if p.agent is not None:
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in looked],
            )
            keep = next((c for c in looked if c.id == pick), None)
        rest = [c for c in looked if c is not keep]
        state.rng.shuffle(rest)
        p.library.extend(rest)
        if keep is not None:
            p.library.insert(0, keep)
        kept = "one" if keep is not None else "none"
        state.log.append(
            f"P{controller_id} looks at the top {n} cards; keeps {kept} on top"
        )

    def describe(self) -> str:
        return (
            "look at the top N cards of your library; put up to one on top "
            "and the rest on the bottom in a random order"
        )


@dataclass(frozen=True)
class FormulaCoversLibrary(Condition):
    """Condition: ``formula`` >= the number of cards in your library
    ("If X is greater than or equal to the number of cards in your
    library")."""

    formula: object

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return self.formula.value(state, viewer_id) >= len(
            state.player_by_id(viewer_id).library
        )


# ---------------------------------------------------------------------------
# "... loses all abilities for as long as <source> remains on the battlefield"
# ---------------------------------------------------------------------------


def _lose_abilities_while_source_remains(state, source, obj) -> None:
    """Layer-6 lose-all-abilities on ``obj`` that lasts while ``source``
    is on the battlefield (a registry entry with no expiry is dropped
    when its source leaves play)."""
    if source is None or source not in state.battlefield:
        return
    if obj not in state.battlefield:
        return
    state.add_continuous_effect(
        source_id=source.id, layer=6, kind="lose_all_abilities",
        target_id=obj.id, params=(),
        description=(
            f"{obj.printing.name} loses all abilities while "
            f"{source.printing.name} remains"
        ),
    )
    state.log.append(
        f"{obj.printing.name} loses all abilities for as long as "
        f"{source.printing.name} remains on the battlefield"
    )


@dataclass(frozen=True)
class ThenTargetLosesAbilitiesWhileSourceRemains(Effect):
    """"<targeted effect>. It loses all abilities for as long as this
    creature remains on the battlefield." (The Wondrous Wasp). The
    inner effect's single target is the "it"."""

    inner: Effect

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(self, state, controller_id):
        return self.inner.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.inner.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        self.inner.resolve(state, controller_id, targets)
        if not targets:
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is not None:
            _lose_abilities_while_source_remains(
                state, state.resolving_source, obj,
            )

    def describe(self) -> str:
        return (
            f"{self.inner.describe()}; it loses all abilities for as long "
            "as this creature remains on the battlefield"
        )


# ---------------------------------------------------------------------------
# Countering spells and abilities
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CounterTargetStackObject(Effect):
    """"Counter target spell, activated ability, or triggered ability"
    and its narrower forms (Ertai Resurrected, Tishana's Tidebinder).
    ``kinds`` names what may be targeted: ``"spell"``, ``"activated"``,
    ``"triggered"``. ``up_to_one`` makes the target optional.
    ``lose_abilities_while_source`` adds Tidebinder's rider: if the
    countered ability's source is an artifact, creature, or
    planeswalker, it loses all abilities while this creature remains.
    Mana abilities never use the stack, so they're never targets."""

    kinds: tuple[StackObjectKind, ...] = (
        StackObjectKind.SPELL, StackObjectKind.ACTIVATED, StackObjectKind.TRIGGERED,
    )
    up_to_one: bool = False
    lose_abilities_while_source: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "kinds", coerce_tuple(StackObjectKind, self.kinds))

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0 if self.up_to_one else 1, 1)

    @staticmethod
    def _kind(item) -> str:
        from mtgz_ir.effects import ActivatedAbility

        if item.ability is None:
            return "spell"
        if isinstance(item.ability, ActivatedAbility):
            return "activated"
        return "triggered"

    def legal_targets(self, state, controller_id):
        from mtgz_ir.effects.generic.stack_control import CounterTargetSpell

        out = []
        for i, item in enumerate(state.stack):
            if item.countered or self._kind(item) not in self.kinds:
                continue
            if item.ability is None and CounterTargetSpell._is_uncounterable(
                state, item,
            ):
                continue
            out.append(("stack", i))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets or targets[0] not in self.legal_targets(
            state, controller_id,
        ):
            return
        item = state.stack[targets[0][1]]
        item.countered = True
        state.log.append(
            f"{item.source.printing.name}'s {self._kind(item)} is countered"
        )
        if self.lose_abilities_while_source and item.ability is not None:
            src = item.source
            pr = src.printing
            from mtgz_ir.game import is_artifact_now

            if (
                is_artifact_now(state, src) or pr.is_creature
                or "Planeswalker" in pr.card_types
            ):
                _lose_abilities_while_source_remains(
                    state, state.resolving_source, src,
                )

    def describe(self) -> str:
        words = {
            "spell": "spell", "activated": "activated ability",
            "triggered": "triggered ability",
        }
        noun = " or ".join(words[k] for k in self.kinds)
        up = "up to one " if self.up_to_one else ""
        return f"counter {up}target {noun}"


@dataclass(frozen=True)
class ThenItsControllerDraws(Effect):
    """"<targeted effect>. Its controller draws a card." -- the
    controller of the (still legal) target, fixed before ``inner``
    resolves, draws ``amount`` (Ertai Resurrected's modes). A target
    that has become illegal means the ability does nothing (CR
    608.2b)."""

    inner: Effect
    amount: int = 1

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(self, state, controller_id):
        return self.inner.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.inner.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        if not targets:
            return
        prior = state.resolving_controller_id
        state.resolving_controller_id = controller_id
        try:
            legal = targets[0] in self.inner.legal_targets(state, controller_id)
        finally:
            state.resolving_controller_id = prior
        if not legal:
            return
        kind, tid = targets[0]
        if kind == "stack":
            who = state.stack[tid].controller_id
        else:
            obj = next((o for o in state.battlefield if o.id == tid), None)
            who = obj.controller_id if obj is not None else None
        self.inner.resolve(state, controller_id, targets)
        if who is not None:
            draw_n_cards(state, who, self.amount)

    def describe(self) -> str:
        return f"{self.inner.describe()}; its controller draws a card"


@dataclass(frozen=True)
class ChooseNoMode(Effect):
    """The "none" option of a "choose up to one" modal ability."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.log.append(f"P{controller_id} chooses no mode")

    def describe(self) -> str:
        return "choose no mode"


__all__ = [
    "OwnerPutsTargetSecondFromTopOrBottom",
    "GainControlOfTargetThenAttachIfEquipment",
    "Adapt",
    "ExileUpToOneTargetGraveyardCardWithSource",
    "ReanimateExiledWithSourceFinalityHasteSacrifice",
    "ModalChooseOneNotYetChosen",
    "SpellMVHasSourceParity",
    "BecomeSaddled",
    "ExileGraveyardCardById",
    "MayCastSourceFromHandForMiracleCost",
    "TargetOpponentExilesThreeMayCastOneFree",
    "ExileUntilNonlandThenCounterOrMayCastIt",
    "LootThenMayCastDiscardedFreeIfCounters",
    "MayCastSpellFromGraveyardNow",
    "ChooseOpponentsExiledCardWithCounterMayPlayFree",
    "MayMillDamageThenMayTakeCreature",
    "ShuffleThenExileTopMayPlayFree",
    "PutSourceOntoBattlefieldTappedAndAttacking",
    "TargetPlayerExilesTopNYouMayPlayThem",
    "ChooseNoMode",
    "CounterTargetStackObject",
    "DevotionTo",
    "FormulaCoversLibrary",
    "LookTopNKeepUpToOneOnTopRestBottom",
    "ThenItsControllerDraws",
    "ThenTargetLosesAbilitiesWhileSourceRemains",
    "MayCastTargetGraveyardCardThisTurn",
    "SourceCantBeBlockedThisTurn",
    "CreateTokenCopiesOfSelf",
    "TargetPlayerPerformsActions",
    "RevealTopCardToHandLoseLifeEqualMV",
    "TapSelf",
    "Amass",
    "BindEventPlayer",
]
