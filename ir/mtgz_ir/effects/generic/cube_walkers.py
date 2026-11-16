"""Generic effect primitives introduced for the PowerLSV cube's
planeswalkers (Jace, Wielder of Mysteries; The Wandering Emperor;
Elspeth, Storm Slayer; Teferi, Time Raveler; Narset, Parter of Veils;
Minsc & Boo, Timeless Heroes).

Every class here is card-agnostic and parameterized by the pieces of
the printed shape that vary, so a future card with the same surface
claims through the same primitive. They live in one module (rather
than being spread across the per-cluster ``generic/*`` files) to keep
the cube campaign's parallel branches from colliding in the shared
cluster modules; each class notes the cluster it belongs to.

Parser entry points: :mod:`mtgz_ir.cards.parser.effects_ext`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target, _apply_damage
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.generic.targets_helpers import (
    _describe_targets,
    _fizzle_message,
    _slot_target_still_legal,
)
from mtgz_ir.effects.object_predicates import ObjectPredicate

if TYPE_CHECKING:
    from mtgz_ir.cards import CardPrinting
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------------
# Object predicates (object_predicates cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class HasManaValueAtLeast(ObjectPredicate):
    """"with mana value N or greater" — the object's printed mana value
    (``printing.mana_cost.total``; CR 202.3) is at least ``n``. Players
    have no mana value and evaluate False."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return obj.printing.mana_cost.total >= self.n


@dataclass(frozen=True)
class HasManaValueAtMost(ObjectPredicate):
    """"with mana value N or less" — mirror of
    :class:`HasManaValueAtLeast`."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return obj.printing.mana_cost.total <= self.n


@dataclass(frozen=True)
class IsColorless(ObjectPredicate):
    """The object has no color (CR 105.2c) -- read through
    ``effective_colors`` so color-setting effects are honored. Players
    are rejected."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_colors

        return not effective_colors(state, obj)


@dataclass(frozen=True)
class IsPermanent(ObjectPredicate):
    """"target permanent" -- any battlefield object (players, which
    carry no printing, are rejected)."""

    def test(self, obj, state):
        return hasattr(obj, "printing")


@dataclass(frozen=True)
class HasPrintedCardType(ObjectPredicate):
    """The object's printed type line carries ``card_type`` (e.g.
    ``"Creature"``). Used for cards outside the battlefield (library /
    graveyard), where only printed characteristics exist -- "a
    noncreature, nonland card" composes as
    ``~HasPrintedCardType("Creature") & ~HasPrintedCardType("Land")``."""

    card_type: str

    def test(self, obj, state):
        printing = getattr(obj, "printing", None)
        return printing is not None and self.card_type in printing.card_types


# ---------------------------------------------------------------------------
# Conditions + conditional combinator (conditions / combinators cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class YourLibraryHasNoCards(Condition):
    """"if your library has no cards in it" — the viewer's library is
    empty at evaluation time."""

    def matches(self, state, viewer_id, chosen_targets=None):
        return not state.player_by_id(viewer_id).library


@dataclass(frozen=True)
class IfCondition(Effect):
    """"If <condition>, <effect>." — run ``then`` only when
    ``condition.matches(state, controller_id)`` holds at resolve time.

    The positive twin of :class:`mtgz_ir.effects.combinators.IfNot`.
    Targeting delegates to ``then``.
    """

    condition: Condition
    then: Effect

    def requires_target(self) -> bool:
        return self.then.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return self.then.legal_targets(state, controller_id)

    def target_count_range(self) -> tuple[int, int]:
        return self.then.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool | None:
        if not self.condition.matches(state, controller_id):
            state.log.append(
                f"P{controller_id} skips '{self.then.describe()}' "
                f"(if-condition not met)"
            )
            return False
        return self.then.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"if {self.condition!r}, {self.then.describe()}"


# ---------------------------------------------------------------------------
# Game outcome (life / misc cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class WinTheGame(Effect):
    """"You win the game." (CR 104.2a) — every opponent of the
    controller loses. See :func:`mtgz_ir.turn.win_the_game` for the
    empty-library-draw interaction."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import win_the_game

        source = state.resolving_source
        reason = source.printing.name if source is not None else "effect"
        win_the_game(state, controller_id, reason=reason)

    def describe(self) -> str:
        return "you win the game"


# ---------------------------------------------------------------------------
# Casting-permission grant (stack_control cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class MayCastSorceriesAsThoughFlashUntilYourNextTurn(Effect):
    """"Until your next turn, you may cast sorcery spells as though they
    had flash." (Teferi, Time Raveler's +1).

    Writes a ``cast_sorceries_as_flash`` continuous-effect registry
    entry (``params=(controller_id,)``) that expires at the
    controller's next untap step (``expires_at="controller_untap"``).
    The entry is a rules-modifying effect rather than a CR 613
    characteristic change, so it carries ``layer=0``; the entry
    survives its source leaving the battlefield (its lifetime is the
    printed duration). ``legal.legal_actions`` reads the registry and
    offers the covered player's sorceries at instant speed.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.legal import CAST_SORCERIES_AS_FLASH_KIND

        source = state.resolving_source
        source_id = source.id if source is not None else -1
        state.add_continuous_effect(
            source_id=source_id,
            layer=0,
            kind=CAST_SORCERIES_AS_FLASH_KIND,
            params=(controller_id,),
            expires_at="controller_untap",
            expires_at_player_id=controller_id,
            description=(
                f"P{controller_id} may cast sorceries as though they had "
                f"flash until their next turn"
            ),
        )
        state.log.append(
            f"P{controller_id} may cast sorcery spells as though they had "
            f"flash until their next turn"
        )

    def describe(self) -> str:
        return (
            "until your next turn, you may cast sorcery spells as though "
            "they had flash"
        )


# ---------------------------------------------------------------------------
# Counters (counters cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PutPlusOneCountersOnTargetThenGrantKeywordsUEOT(Effect):
    """"Put N +1/+1 counter(s) on [up to one] target <filter>. It gains
    <keywords> until end of turn." — new-shape targeted counters with an
    anaphoric keyword grant on the same object (The Wandering Emperor's
    +1). ``granted_keywords`` may be empty (plain counters on a filtered
    / optional target, Minsc & Boo's +1).

    Target legality is re-verified at resolution (CR 608.2b); an
    illegal target gets neither counters nor keywords.
    """

    amount: int = 1
    targets: tuple = ()
    granted_keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantKeywords
        from mtgz_ir.game import add_plus_one_counters

        slot = self.targets[0]
        chosen = targets[0]
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, slot, chosen)
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(
                _fizzle_message(state, chosen, action="counter placement")
            )
            return
        obj = next(
            (o for o in state.battlefield if o.id == chosen[1]), None,
        )
        if obj is None:  # pragma: no cover - legality check covers it
            return
        add_plus_one_counters(state, obj, self.amount)
        state.log.append(
            f"{obj.printing.name} gets {self.amount} +1/+1 counter(s) "
            f"(now {obj.plus_one_counters})"
        )
        if self.granted_keywords:
            ApplyEffect(
                inner=GrantKeywords(keywords=frozenset(self.granted_keywords)),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, (chosen,))

    def describe(self) -> str:
        noun = "a +1/+1 counter" if self.amount == 1 else (
            f"{self.amount} +1/+1 counters"
        )
        out = f"put {noun} on {_describe_targets(self.targets)}"
        if self.granted_keywords:
            out += (
                f"; it gains {', '.join(sorted(self.granted_keywords))} "
                f"until end of turn"
            )
        return out


@dataclass(frozen=True)
class PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords(Effect):
    """"Put a +1/+1 counter on each creature you control. Those
    creatures gain <keywords> until <end of turn | your next turn>."
    (Elspeth, Storm Slayer's 0).

    "Those creatures" is the set that received counters — snapshotted
    at resolution, so a creature entering later is not covered.
    ``until_your_next_turn`` picks the grant's lifetime
    (``EndOfYourNextTurn`` vs ``EndOfTurn``).
    """

    amount: int = 1
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    until_your_next_turn: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn, EndOfYourNextTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantKeywords
        from mtgz_ir.game import add_plus_one_counters, is_creature_now

        those = [
            o for o in state.battlefield
            if o.controller_id == controller_id and is_creature_now(o)
        ]
        duration = EndOfYourNextTurn() if self.until_your_next_turn else (
            EndOfTurn()
        )
        grant = ApplyEffect(
            inner=GrantKeywords(keywords=frozenset(self.granted_keywords)),
            ends_at=duration,
        )
        for obj in those:
            add_plus_one_counters(state, obj, self.amount)
            if self.granted_keywords:
                grant.resolve(state, controller_id, (("creature", obj.id),))
        state.log.append(
            f"P{controller_id} puts +1/+1 counter(s) on {len(those)} "
            f"creature(s)"
            + (
                f"; they gain {', '.join(sorted(self.granted_keywords))} "
                f"{duration.describe()}"
                if self.granted_keywords else ""
            )
        )

    def describe(self) -> str:
        dur = "your next turn" if self.until_your_next_turn else "end of turn"
        return (
            f"put a +1/+1 counter on each creature you control; those "
            f"creatures gain {', '.join(sorted(self.granted_keywords))} "
            f"until {dur}"
        )


# ---------------------------------------------------------------------------
# Library dig (library_lookat cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class LookTopNMayRevealMatchingToHandRestBottomRandom(Effect):
    """"Look at the top N cards of your library. You may reveal a
    <filter> card from among them and put it into your hand. Put the
    rest on the bottom of your library in a random order." (Narset,
    Parter of Veils' -2).

    ``card_filter`` is an :class:`ObjectPredicate` over the examined
    cards (their printed characteristics). The optional pick is two
    agent decisions: ``choose_yes_no`` (take one?) then
    ``choose_card_from_library`` among the matching cards. The chosen
    card is revealed to all players; the remainder go to the bottom in
    an order drawn from ``state.rng`` (CR 401.4 "random order").
    """

    n: int = 4
    card_filter: ObjectPredicate | None = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        examined = list(p.library[:self.n])
        if not examined:
            return
        for c in examined:
            p.library.remove(c)
        matching = [
            c for c in examined
            if self.card_filter is None or self.card_filter.test(c, state)
        ]
        taken = None
        if matching and (
            p.agent is None
            or p.agent.choose_yes_no(
                state, controller_id,
                "Reveal a card from among them and put it into your hand?",
            )
        ):
            pick_id = None
            if p.agent is not None:
                pick_id = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in matching],
                )
            taken = next(
                (c for c in matching if c.id == pick_id), matching[0],
            )
            taken.zone = Zone.HAND
            p.hand.append(taken)
            reveal_to_all(state, taken)
            state.log.append(
                f"P{controller_id} reveals {taken.printing.name} and puts "
                f"it into their hand (look at top {self.n})"
            )
        rest = [c for c in examined if c is not taken]
        state.rng.shuffle(rest)
        for c in rest:
            c.zone = Zone.LIBRARY
            p.library.append(c)
        if rest:
            state.log.append(
                f"P{controller_id} puts {len(rest)} card(s) on the bottom "
                f"of their library in a random order"
            )

    def describe(self) -> str:
        return (
            f"look at the top {self.n} cards of your library; you may "
            f"reveal a matching card and put it into your hand; put the "
            f"rest on the bottom in a random order"
        )


# ---------------------------------------------------------------------------
# Sacrifice + reflexive trigger (opponent_punishers / damage cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SacrificeCreatureThenDamageEqualToItsPower(Effect):
    """"Sacrifice a creature. When you do, <source> deals X damage to
    any target, where X is that creature's power. If the sacrificed
    creature was a <subtype>, draw X cards." (Minsc & Boo, Timeless
    Heroes' -2).

    The sacrifice is mandatory; with no creature to sacrifice there is
    no reflexive trigger. X is the sacrificed creature's last-known
    power (CR 608.2h) and its last-known subtypes decide the draw rider
    (``draw_if_subtype``; empty string = no rider).

    Engine simplification (same precedent as Boilerbilges Ripper /
    Selfcraft Mechan, documented in docs/SEARCH_LIMITATIONS.md): the
    reflexive trigger's "any target" is chosen when the ability is
    activated and the damage + draw resolve inline instead of as a
    separate reflexive trigger on the stack.
    """

    draw_if_subtype: str = ""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        from mtgz_ir.effects.targets import any_target_filter

        return self._enumerate_for_filter(
            state, controller_id, any_target_filter(),
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool | None:
        from mtgz_ir.game import (
            effective_power, effective_subtypes, is_creature_now,
        )
        from mtgz_ir.turn import draw_n_cards, sacrifice as _sacrifice

        p = state.player_by_id(controller_id)
        candidates = [
            o for o in state.battlefield
            if o.controller_id == controller_id and is_creature_now(o)
        ]
        if not candidates:
            state.log.append(
                f"P{controller_id} has no creature to sacrifice"
            )
            return False
        chosen = candidates[0]
        if p.agent is not None:
            pick_id = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in candidates],
            )
            chosen = next(
                (o for o in candidates if o.id == pick_id), candidates[0],
            )
        x = max(0, effective_power(state, chosen))
        was_subtype = bool(self.draw_if_subtype) and (
            self.draw_if_subtype in effective_subtypes(state, chosen)
        )
        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name} "
            f"(power {x})"
        )
        _sacrifice(state, chosen)
        if targets:
            target = targets[0]
            still_legal = target in self.legal_targets(state, controller_id)
            if still_legal and x > 0:
                _apply_damage(state, target, x)
            elif not still_legal:
                state.log.append(
                    _fizzle_message(state, target, action="damage")
                )
        if was_subtype and x > 0:
            draw_n_cards(state, controller_id, x)
        return True

    def describe(self) -> str:
        rider = (
            f"; if it was a {self.draw_if_subtype}, draw X cards"
            if self.draw_if_subtype else ""
        )
        return (
            "sacrifice a creature; when you do, deal X damage to any "
            f"target where X is its power{rider}"
        )


# ---------------------------------------------------------------------------
# Library manipulation of a target player (library_lookat / draw_mill)
# ---------------------------------------------------------------------------


def _living_player_targets(state: "GameState") -> list[Target]:
    return [("player", p.id) for p in state.players if not p.has_lost]


def _target_player(state: "GameState", targets):
    """The still-legal target player from ``targets`` (``None`` when
    absent / not a player / has left the game)."""
    if not targets:
        return None
    kind, pid = targets[0]
    if kind != "player":
        return None
    p = state.player_by_id(pid)
    return None if p.has_lost else p


@dataclass(frozen=True)
class LookAtTopCardOfTargetPlayersLibraryMayBottom(Effect):
    """"Look at the top card of target player's library. You may put
    that card on the bottom of that player's library." (Jace, the Mind
    Sculptor's +2).

    The controller looks (the card is not revealed) and decides via
    ``choose_yes_no``."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return _living_player_targets(state)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        target = _target_player(state, targets)
        if target is None or not target.library:
            return
        me = state.player_by_id(controller_id)
        bottom = me.agent is None or me.agent.choose_yes_no(
            state, controller_id,
            f"Put the top card of P{target.id}'s library on the bottom?",
        )
        if bottom:
            target.library.append(target.library.pop(0))
            state.log.append(
                f"P{controller_id} puts the top card of P{target.id}'s "
                f"library on the bottom"
            )
        else:
            state.log.append(
                f"P{controller_id} leaves the top card of P{target.id}'s "
                f"library on top"
            )

    def describe(self) -> str:
        return (
            "look at the top card of target player's library; you may "
            "put it on the bottom"
        )


@dataclass(frozen=True)
class PutCardsFromHandOnTopOfLibrary(Effect):
    """"Put N cards from your hand on top of your library in any order."
    (Jace, the Mind Sculptor's 0, after "Draw three cards, then").

    The controller picks ``amount`` cards one at a time via
    ``choose_card_from_hand`` (mandatory: a decline / unknown id takes
    the first card); each pick goes on top of the previous one, so the
    last card chosen ends up on top -- the pick order IS the chosen
    order. With fewer cards in hand, all of them go back."""

    amount: int = 2

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, hide_hand_moved_into_library

        p = state.player_by_id(controller_id)
        n = min(self.amount, len(p.hand))
        moved = []
        for _ in range(n):
            ids = [c.id for c in p.hand]
            pick = None
            if p.agent is not None:
                pick = p.agent.choose_card_from_hand(state, controller_id, ids)
            card = next((c for c in p.hand if c.id == pick), p.hand[0])
            p.hand.remove(card)
            card.zone = Zone.LIBRARY
            p.library.insert(0, card)
            moved.append(card)
        # Face down: the other players can't tell which cards went back.
        hide_hand_moved_into_library(state, controller_id, moved)
        state.log.append(
            f"P{controller_id} puts {n} card(s) from hand on top of "
            f"their library"
        )
        if n:
            from mtgz_ir.turn import note_cards_put_into_library

            note_cards_put_into_library(state, controller_id)

    def describe(self) -> str:
        return (
            f"put {self.amount} cards from your hand on top of your "
            f"library in any order"
        )


@dataclass(frozen=True)
class ExileTargetPlayersLibraryThenShuffleHandIn(Effect):
    """"Exile all cards from target player's library, then that player
    shuffles their hand into their library." (Jace, the Mind Sculptor's
    -12)."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return _living_player_targets(state)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, shuffle_library

        target = _target_player(state, targets)
        if target is None:
            return
        exiled = list(target.library)
        target.library.clear()
        for c in exiled:
            c.zone = Zone.EXILE
            target.exile.append(c)
        hand = list(target.hand)
        target.hand.clear()
        for c in hand:
            c.zone = Zone.LIBRARY
            target.library.append(c)
        # The shared shuffle helper also forgets what anyone knew about
        # the shuffled cards (the same RNG draw as a bare shuffle).
        shuffle_library(state, target.id)
        state.log.append(
            f"P{target.id} exiles {len(exiled)} library card(s) and "
            f"shuffles {len(hand)} hand card(s) into their library"
        )
        if hand:
            from mtgz_ir.turn import note_cards_put_into_library

            note_cards_put_into_library(state, target.id)

    def describe(self) -> str:
        return (
            "exile all cards from target player's library, then that "
            "player shuffles their hand into their library"
        )


@dataclass(frozen=True)
class TargetPlayerDrawsThenDiscards(Effect):
    """"Target player draws N cards, then discards M cards." (Dack
    Fayden's +1). The target player chooses the discards (their agent's
    ``choose_discard``); discard observers fire as for any discard."""

    draw: int = 2
    discard: int = 2

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return _living_player_targets(state)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.discard import DiscardSelf
        from mtgz_ir.turn import draw_n_cards

        target = _target_player(state, targets)
        if target is None:
            return
        draw_n_cards(state, target.id, self.draw)
        DiscardSelf(amount=self.discard).resolve(state, target.id)

    def describe(self) -> str:
        return (
            f"target player draws {self.draw} cards, then discards "
            f"{self.discard} cards"
        )


# ---------------------------------------------------------------------------
# Control change (control cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GainControlOfTargetPermanently(Effect):
    """"Gain control of target <permanent filter>." with no duration --
    the control change lasts indefinitely (CR 611.2a). Generalizes
    ``GainControlOfTargetSliverPermanently`` to any composed target
    filter (Dack Fayden's -2: "target artifact").

    Sets ``controller_id`` directly (the same permanent-steal precedent
    as the Sliver variant) and marks the permanent summoning-sick for
    its new controller (CR 302.6)."""

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        chosen = targets[0]
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], chosen)
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(
                _fizzle_message(state, chosen, action="control change")
            )
            return
        obj = next(o for o in state.battlefield if o.id == chosen[1])
        if obj.controller_id == controller_id:
            return
        obj.controller_id = controller_id
        obj.summoning_sick = True
        state._invalidate_layered_cache()
        state.log.append(
            f"P{controller_id} gains control of {obj.printing.name}"
        )

    def describe(self) -> str:
        return f"gain control of {_describe_targets(self.targets)}"


# ---------------------------------------------------------------------------
# Delayed triggers / untap (delayed_actions / tap cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class AtBeginningOfNextEndStep(Effect):
    """"At the beginning of the next end step, <effect>." -- schedules
    a delayed triggered ability (CR 603.7) that fires ``inner`` at the
    beginning of the current turn's end step (Teferi, Hero of
    Dominaria's +1 rider).

    Rides the engine's ``state.delayed_triggers`` queue (see
    ``turn.run_delayed_end_step_triggers``), which fires entries on the
    scheduling controller's own end step. Every producer in the pool
    schedules at sorcery timing (a loyalty ability), so "the next end
    step" is always this turn's; an instant-speed producer scheduling
    during another player's turn would need the queue to key entries on
    the upcoming end step rather than the controller's turn (documented
    in docs/SEARCH_LIMITATIONS.md)."""

    inner: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        source = state.resolving_source
        if source is None:
            return False
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number,
            "controller_id": controller_id,
            "source_id": source.id,
            "effect": self.inner,
        })
        state.log.append(
            f"P{controller_id} schedules '{self.inner.describe()}' for "
            f"the beginning of the next end step"
        )
        return True

    def describe(self) -> str:
        return (
            f"at the beginning of the next end step, "
            f"{self.inner.describe()}"
        )


@dataclass(frozen=True)
class UntapUpToNPermanents(Effect):
    """"Untap up to N <permanents>." (untargeted; Teferi, Hero of
    Dominaria's "untap up to two lands"). The controller picks, one at
    a time, among the matching TAPPED permanents via
    ``choose_permanent_to_tap`` (``None`` stops early); untapping an
    already-untapped permanent would be a no-op, so only tapped ones
    are offered."""

    n: int = 2
    permanent_filter: ObjectPredicate | None = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        p = state.player_by_id(controller_id)
        untapped = 0
        for _ in range(self.n):
            options = [
                o for o in state.battlefield
                if o.tapped and (
                    self.permanent_filter is None
                    or self.permanent_filter.test(o, state)
                )
            ]
            if not options:
                break
            pick = options[0].id
            if p.agent is not None:
                pick = p.agent.choose_permanent_to_tap(
                    state, controller_id, [o.id for o in options],
                )
            obj = next((o for o in options if o.id == pick), None)
            if obj is None:
                break
            obj.tapped = False
            untapped += 1
            state.log.append(f"P{controller_id} untaps {obj.printing.name}")
        if not untapped:
            state.log.append(f"P{controller_id} untaps nothing")

    def describe(self) -> str:
        return f"untap up to {self.n} matching permanents"


@dataclass(frozen=True)
class PutTargetIntoOwnersLibraryNthFromTop(Effect):
    """"Put target <filter> into its owner's library Nth from the top."
    (Teferi, Hero of Dominaria's -3: "third from the top").

    Leaves the battlefield like a bounce (combat / damage / counters /
    temp bonuses cleared, CR 400.7 new object); a token ceases to exist
    (CR 704.5d / 111.8). With fewer than N-1 cards in the library the
    card goes to the bottom (CR 401.4 -- as deep as it can)."""

    position: int = 3
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        chosen = targets[0]
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], chosen)
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, chosen, action="tuck"))
            return
        obj = next(o for o in state.battlefield if o.id == chosen[1])
        state.battlefield.remove(obj)
        state.remove_continuous_effects_from_source(obj.id, only_static=True)
        for attr, value in (
            ("damage", 0), ("marked_lethal", False),
            ("damaged_this_turn", False), ("tapped", False),
            ("attacking", False), ("blocking", None), ("blocked_by", []),
            ("plus_one_counters", 0), ("charge_counters", 0),
            ("stun_counters", 0), ("loyalty_counters", 0),
            ("temp_power_bonus", 0), ("temp_toughness_bonus", 0),
            ("attached_to", None),
        ):
            setattr(obj, attr, value)
        obj.controller_id = obj.owner_id
        if not obj.printing.is_land:
            state.permanent_left_battlefield_this_turn = True
        if obj.printing.is_token:
            state.log.append(
                f"{obj.printing.name} token ceases to exist (tucked)"
            )
            return
        owner = state.player_by_id(obj.owner_id)
        obj.zone = Zone.LIBRARY
        idx = min(self.position - 1, len(owner.library))
        owner.library.insert(idx, obj)
        state.log.append(
            f"{obj.printing.name} is put into P{owner.id}'s library "
            f"(position {idx + 1} from the top)"
        )
        from mtgz_ir.turn import note_cards_put_into_library

        note_cards_put_into_library(state, owner.id)

    def describe(self) -> str:
        return (
            f"put {_describe_targets(self.targets)} into its owner's "
            f"library {self.position} from the top"
        )


# ---------------------------------------------------------------------------
# Emblems (emblem cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CreateEmblemWithAbilities(Effect):
    """"You get an emblem with '<ability>'." for an emblem whose quoted
    ability parses through the unified grammar (CR 114).

    Every emblem the engine creates goes through this effect (Tezzeret,
    Cruel Captain's included -- ``emblem.tezzeret_cruel_captain_emblem``):
    the emblem's triggered abilities are carried on the effect itself
    and stamped onto a ``make_emblem`` printing at resolution, so any
    parseable quoted trigger claims without a per-card emblem kind. The
    emblem joins ``state.emblems`` (command zone), whose triggers
    ``dispatch_event`` already scans.

    ``static_abilities`` (anthems -- "Ninjas you control get +1/+1",
    "Lands you control have indestructible") are read by the layered
    static walks, which scan emblems as static sources alongside the
    battlefield (cube glue bucket). ``grants_retrace_to`` is the
    emblem's printing-level retrace grant (a ``CardFilter`` over its
    controller's graveyard cards: Wrenn and Six's "Instant and sorcery
    cards in your graveyard have retrace."), stamped onto the emblem's
    ``CardPrinting.grants_retrace_to``."""

    name: str = "emblem"
    triggered_abilities: tuple = ()
    static_abilities: tuple = ()
    grants_retrace_to: "object | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from dataclasses import replace as _replace

        from mtgz_ir.cards.base import make_emblem
        from mtgz_ir.game import Zone

        printing = make_emblem(
            name=self.name, triggered_abilities=self.triggered_abilities,
            static_abilities=self.static_abilities,
        )
        if self.grants_retrace_to is not None:
            printing = _replace(printing, grants_retrace_to=self.grants_retrace_to)
        emblem = state.new_object(
            printing, owner_id=controller_id, zone=Zone.COMMAND,
        )
        emblem.etb_timestamp = state.next_timestamp
        state.next_timestamp += 1
        state.emblems.append(emblem)
        state.log.append(f"P{controller_id} gets an emblem ({self.name})")

    def describe(self) -> str:
        return f"you get an emblem ({self.name})"


# ---------------------------------------------------------------------------
# "Whenever you cast a spell that targets ..." (Dack Fayden's emblem)
# ---------------------------------------------------------------------------


def _spell_stack_item(state: "GameState", spell):
    """The StackItem for ``spell`` (a cast spell, not an ability)."""
    for item in state.stack:
        if item.source is spell and item.ability is None:
            return item
    return None


def _spell_targets_matching(state, spell, noun_filter) -> tuple:
    item = _spell_stack_item(state, spell)
    if item is None:
        return ()
    out = []
    for kind, target_id in item.targets:
        if kind == "player":
            continue
        obj = next((o for o in state.battlefield if o.id == target_id), None)
        if obj is not None and noun_filter.test(obj, state):
            out.append((kind, target_id))
    return tuple(out)


@dataclass(frozen=True)
class SpellTargetsMatching(ObjectPredicate):
    """A spell (on the stack) with at least one target that is a
    battlefield object matching ``noun_filter`` -- "a spell that targets
    one or more permanents". Reads the spell's ``StackItem.targets``."""

    noun_filter: ObjectPredicate

    def test(self, obj, state):
        return bool(_spell_targets_matching(state, obj, self.noun_filter))


@dataclass(frozen=True)
class GainControlOfTriggeringSpellTargets(Effect):
    """"...gain control of those permanents." -- the anaphoric body of
    "Whenever you cast a spell that targets one or more permanents"
    (Dack Fayden's emblem). "Those permanents" are the triggering
    spell's matching targets, bound when the trigger fires through the
    ``targets_from_event`` hook ``dispatch_event`` consults (they are
    not targets of the trigger itself). The control change lasts
    indefinitely (CR 611.2a)."""

    noun_filter: ObjectPredicate

    def targets_from_event(self, state: "GameState", subjects) -> tuple:
        spell = subjects.get("spell")
        if spell is None:
            return ()
        return _spell_targets_matching(state, spell, self.noun_filter)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        for _kind, target_id in targets:
            obj = next(
                (o for o in state.battlefield if o.id == target_id), None,
            )
            if obj is None or obj.controller_id == controller_id:
                continue
            obj.controller_id = controller_id
            obj.summoning_sick = True
            state._invalidate_layered_cache()
            state.log.append(
                f"P{controller_id} gains control of {obj.printing.name}"
            )

    def describe(self) -> str:
        return "gain control of the triggering spell's targeted permanents"


# ---------------------------------------------------------------------------
# Karn, Scion of Urza (library_lookat / graveyard-exile cluster)
# ---------------------------------------------------------------------------

_COUNTER_FIELDS = {"silver": "silver_counters"}


@dataclass(frozen=True)
class RevealTopNOpponentChoosesOneRestExiledWithCounter(Effect):
    """"Reveal the top N cards of your library. An opponent chooses one
    of them. Put that card into your hand and exile the other with a
    <kind> counter on it." (Karn, Scion of Urza's +1, N=2).

    The revealed cards are public (``reveal_to_all``); an opponent's
    agent picks via ``choose_card_from_library``. The other card(s) go
    to exile carrying one ``counter`` counter each. With one card left
    it goes to hand; with none the ability does nothing."""

    n: int = 2
    counter: str = "silver"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        revealed = list(p.library[:self.n])
        if not revealed:
            return
        for c in revealed:
            p.library.remove(c)
            reveal_to_all(state, c)
        chooser = next(
            (o for o in state.players
             if o.id != controller_id and not o.has_lost),
            None,
        )
        pick_id = None
        if chooser is not None and chooser.agent is not None:
            pick_id = chooser.agent.choose_card_from_library(
                state, chooser.id, [c.id for c in revealed],
            )
        chosen = next((c for c in revealed if c.id == pick_id), revealed[0])
        chosen.zone = Zone.HAND
        p.hand.append(chosen)
        state.log.append(
            f"P{controller_id} puts {chosen.printing.name} into their hand "
            f"(chosen by an opponent)"
        )
        attr = _COUNTER_FIELDS[self.counter]
        for c in revealed:
            if c is chosen:
                continue
            c.zone = Zone.EXILE
            setattr(c, attr, getattr(c, attr) + 1)
            p.exile.append(c)
            state.log.append(
                f"P{controller_id} exiles {c.printing.name} with a "
                f"{self.counter} counter on it"
            )

    def describe(self) -> str:
        return (
            f"reveal the top {self.n} cards of your library; an opponent "
            f"chooses one for your hand; exile the rest with a "
            f"{self.counter} counter"
        )


@dataclass(frozen=True)
class PutCardYouOwnWithCounterFromExileIntoHand(Effect):
    """"Put a card you own with a <kind> counter on it from exile into
    your hand." (Karn, Scion of Urza's -1). Untargeted; the controller
    picks among their eligible exiled cards via
    ``choose_card_from_library``. The card leaves exile as a new object
    (CR 400.7), so its counters are removed."""

    counter: str = "silver"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        attr = _COUNTER_FIELDS[self.counter]
        p = state.player_by_id(controller_id)
        options = [
            c for c in p.exile
            if c.owner_id == controller_id and getattr(c, attr) > 0
        ]
        if not options:
            state.log.append(
                f"P{controller_id} has no exiled card with a "
                f"{self.counter} counter"
            )
            return
        pick_id = None
        if p.agent is not None:
            pick_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
        card = next((c for c in options if c.id == pick_id), options[0])
        p.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        setattr(card, attr, 0)
        card.zone = Zone.HAND
        p.hand.append(card)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} from exile into "
            f"their hand"
        )

    def describe(self) -> str:
        return (
            f"put a card you own with a {self.counter} counter on it from "
            f"exile into your hand"
        )


# ---------------------------------------------------------------------------
# Temporary copy effect with two differently-filtered targets (misc cluster)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetBecomesCopyOfAnotherTargetUEOT(Effect):
    """"Target <A> becomes a copy of another target <B> until end of
    turn[, except it's a <type> in addition to its other types]."
    (Saheeli, Sublime Artificer's -2: A = artifact you control, B =
    artifact or creature you control, extra type Artifact).

    Two ``TargetSpec`` slots with their own filters; the activation
    pipeline enumerates each slot through ``legal_targets_for_slot``
    and keeps them distinct ("another"). Resolution follows the
    Cytoshape layer-1 precedent: snapshot ``original_printing``, swap
    in the copied printing, and register an ``is_copy`` entry expiring
    at cleanup (``run_cleanup_step`` / ``move_off_battlefield`` revert
    it). The copy keeps the copier's token status (CR 707.2 -- being a
    token isn't a copiable value) and gains ``add_card_types``."""

    targets: tuple = ()
    add_card_types: tuple = ()

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def legal_targets_for_slot(
        self, state: "GameState", controller_id: int, slot_idx: int,
    ) -> list[Target]:
        return self._enumerate_for_filter(
            state, controller_id, self.targets[slot_idx].filter,
        )

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        """Slot-0 candidates, or ``[]`` when no DISTINCT pair of legal
        targets exists (so the ability isn't offered)."""
        first = self.legal_targets_for_slot(state, controller_id, 0)
        second = self.legal_targets_for_slot(state, controller_id, 1)
        for a in first:
            if any(b != a for b in second):
                return first
        return []

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        import dataclasses as _dc

        if len(targets) < 2 or targets[0] == targets[1]:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            legal = all(
                _slot_target_still_legal(state, slot, chosen)
                for slot, chosen in zip(self.targets, targets)
            )
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, targets[0], action="copy"))
            return
        first = next(o for o in state.battlefield if o.id == targets[0][1])
        second = next(o for o in state.battlefield if o.id == targets[1][1])
        if first.original_printing is None:
            first.original_printing = first.printing
        copied = second.printing
        extra_types = tuple(
            t for t in self.add_card_types if t not in copied.card_types
        )
        supertypes = copied.supertypes
        if (
            "Token" in first.original_printing.supertypes
            and "Token" not in supertypes
        ):
            supertypes = supertypes + ("Token",)
        elif "Token" not in first.original_printing.supertypes:
            supertypes = tuple(s for s in supertypes if s != "Token")
        copied = _dc.replace(
            copied,
            card_types=extra_types + copied.card_types,
            supertypes=supertypes,
        )
        first.printing = copied
        src = state.resolving_source
        state.add_continuous_effect(
            source_id=src.id if src is not None else first.id,
            layer=1,
            kind="is_copy",
            target_id=first.id,
            params=(copied,),
            expires_at="cleanup",
            description=(
                f"{first.original_printing.name} becomes a copy of "
                f"{second.printing.name} UEOT"
            ),
        )
        state.log.append(
            f"{first.original_printing.name} becomes a copy of "
            f"{second.printing.name} until end of turn"
        )

    def describe(self) -> str:
        return (
            f"{_describe_targets(self.targets[:1])} becomes a copy of "
            f"another target until end of turn"
        )


__all__ = [
    "TargetBecomesCopyOfAnotherTargetUEOT",
    "PutCardYouOwnWithCounterFromExileIntoHand",
    "RevealTopNOpponentChoosesOneRestExiledWithCounter",
    "GainControlOfTriggeringSpellTargets",
    "SpellTargetsMatching",
    "AtBeginningOfNextEndStep",
    "CreateEmblemWithAbilities",
    "IsColorless",
    "PutTargetIntoOwnersLibraryNthFromTop",
    "UntapUpToNPermanents",
    "ExileTargetPlayersLibraryThenShuffleHandIn",
    "GainControlOfTargetPermanently",
    "LookAtTopCardOfTargetPlayersLibraryMayBottom",
    "PutCardsFromHandOnTopOfLibrary",
    "TargetPlayerDrawsThenDiscards",
    "HasManaValueAtLeast",
    "HasPrintedCardType",
    "IsPermanent",
    "HasManaValueAtMost",
    "IfCondition",
    "LookTopNMayRevealMatchingToHandRestBottomRandom",
    "MayCastSorceriesAsThoughFlashUntilYourNextTurn",
    "PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords",
    "PutPlusOneCountersOnTargetThenGrantKeywordsUEOT",
    "SacrificeCreatureThenDamageEqualToItsPower",
    "WinTheGame",
    "YourLibraryHasNoCards",
]
