"""Engine effects for the Vintage Roto P4 package: planeswalkers,
copies, control changes and turn structure.

Every effect is generic (parameterized by the printed shape), not
per-card; the parser side is ``cards/parser/roto_walkers.py`` and the
turn-structure rules live in ``mtgz_ir.roto_turns``. Shortcuts are
documented in the "Vintage Roto P4" section of
``docs/SEARCH_LIMITATIONS.md``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import ZoneName
from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.generic.cube_creatures_rmc import ExilePlayPermission

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# ---------------------------------------------------------------------------
# Delayed "put that card into your hand" (Necropotence)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PutExiledSourceIntoOwnersHand(Effect):
    """The delayed half of "Exile <a card> ... Put that card into your
    hand at the beginning of your next end step.": the scheduled object
    (``state.resolving_source``) moves from its owner's exile to their
    hand -- only if it is still that exiled card (CR 400.7: a card that
    left exile in the meantime is a new object)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        card = state.resolving_source
        if card is None or card.zone != Zone.EXILE:
            return
        owner = state.player_by_id(card.owner_id)
        if card not in owner.exile:
            return
        owner.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        card.zone = Zone.HAND
        owner.hand.append(card)
        state.log.append(
            f"P{owner.id} puts {card.printing.name} from exile into their hand"
        )

    def describe(self) -> str:
        return "put that card into your hand"


@dataclass(frozen=True)
class ExileTopCardsToHandAtYourNextEndStep(Effect):
    """"Exile the top <N> card(s) of your library[ face down]. Put that
    card / them into your hand at the beginning of your next end step."
    (Necropotence). Each exiled card gets its own delayed trigger
    (``roto_turns.schedule_at_your_next_end_step``) that returns it even
    if the source has left the battlefield. A face-down card is known
    to no one -- not even its owner -- until it reaches the hand."""

    count: int = 1
    face_down: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all
        from mtgz_ir.roto_turns import schedule_at_your_next_end_step

        p = state.player_by_id(controller_id)
        for _ in range(self.count):
            if not p.library:
                return
            card = p.library.pop(0)
            card.zone = Zone.EXILE
            state.player_by_id(card.owner_id).exile.append(card)
            card.exiled_face_down = self.face_down
            if self.face_down:
                card.publicly_known_to = frozenset()
                shown = "a card face down"
            else:
                reveal_to_all(state, card)
                shown = card.printing.name
            schedule_at_your_next_end_step(
                state, controller_id, card.id, PutExiledSourceIntoOwnersHand(),
            )
            state.log.append(
                f"P{controller_id} exiles {shown} from the top of their "
                f"library until their next end step"
            )

    def describe(self) -> str:
        n = "the top card" if self.count == 1 else f"the top {self.count} cards"
        down = " face down" if self.face_down else ""
        return (
            f"exile {n} of your library{down}; put it into your hand at the "
            "beginning of your next end step"
        )


# ---------------------------------------------------------------------------
# Turn structure: end the turn, extra turns
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class EndTheTurn(Effect):
    """"End the turn." (Sundial of the Infinite, Day's Undoing; CR 723):
    ``roto_turns.end_the_turn``."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.roto_turns import end_the_turn

        end_the_turn(state)

    def describe(self) -> str:
        return "end the turn"


@dataclass(frozen=True)
class LoseTheGame(Effect):
    """"You lose the game." (CR 104.3d: the effect makes the player lose
    at once; the next state-based-action check ends a two-player game)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        p = state.player_by_id(controller_id)
        if p.has_lost:
            return
        p.has_lost = True
        p.lost_reason = "an effect made them lose the game"
        state.log.append(f"P{controller_id} loses the game")

    def describe(self) -> str:
        return "you lose the game"


@dataclass(frozen=True)
class TakeExtraTurnWithEndStepTrigger(Effect):
    """"Take an extra turn after this one. At the beginning of that
    turn's end step, <effect>." (Final Fortune).

    The extra turn joins ``state.extra_turn_queue`` exactly like
    ``TakeExtraTurnAfterThisOne`` (Nexus of Fate); the queue is consumed
    in order, one turn per entry, so "that turn" is turn number
    ``turn_number + <queue length after the append>``. A delayed
    end-step trigger for that turn number and the controller carries
    ``inner`` -- it never fires if that turn's end step is skipped
    (Sundial of the Infinite ends the turn first)."""

    inner: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.extra_turn_queue.append(controller_id)
        source = state.resolving_spell or state.resolving_source
        state.log.append(
            f"P{controller_id} is scheduled to take an extra turn"
        )
        if source is None:
            return
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number + len(state.extra_turn_queue),
            "controller_id": controller_id,
            "source_id": source.id,
            "effect": self.inner,
        })

    def describe(self) -> str:
        return (
            "take an extra turn after this one; at the beginning of that "
            f"turn's end step, {self.inner.describe()}"
        )


# ---------------------------------------------------------------------------
# Wish: "a card you own from outside the game" (CR 400.11)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PutCardFromOutsideTheGameIntoHand(Effect):
    """"You may reveal a <filter> card you own from outside the game and
    put it into your hand." (Burning Wish).

    The cards a player owns outside the game are their sideboard
    (``Player.sideboard``; the user's decision, 2026-10-02 -- in a roto
    draft, the drafter's unused picks). The controller picks one of the
    matching names (or nothing -- the reveal is optional) through the
    generic ``choose_mode`` hook; the chosen copy becomes a new object
    in their hand (``roto_turns.take_from_outside_the_game``)."""

    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.roto_turns import take_from_outside_the_game

        p = state.player_by_id(controller_id)
        by_name: dict = {}
        for printing in p.sideboard:
            if self.card_filter is None or self.card_filter.matches(printing):
                by_name.setdefault(printing.name, printing)
        if not by_name or p.agent is None:
            state.log.append(
                f"P{controller_id} reveals nothing from outside the game"
            )
            return
        names = sorted(by_name)
        options = [f"reveal {n}" for n in names] + ["reveal nothing"]
        pick = p.agent.choose_mode(state, controller_id, options)
        if not 0 <= pick < len(names):
            state.log.append(
                f"P{controller_id} reveals nothing from outside the game"
            )
            return
        take_from_outside_the_game(state, controller_id, by_name[names[pick]])

    def describe(self) -> str:
        what = (
            self.card_filter.describe() if self.card_filter is not None
            else "card"
        )
        return (
            f"you may reveal a {what} you own from outside the game and put "
            "it into your hand"
        )


# ---------------------------------------------------------------------------
# Planeswalker costs and keywords
# ---------------------------------------------------------------------------


def choose_and_pay_loyalty_x(
    state: "GameState", player_id: int, source,
) -> int:
    """A "-X" loyalty cost: the activator chooses X (0 up to the
    walker's loyalty -- they can't remove counters it doesn't have)
    through ``choose_x`` and X loyalty counters are removed. Returns X,
    which rides the stack item as ``state.resolving_x_value``."""
    upper = max(0, source.loyalty_counters)
    p = state.player_by_id(player_id)
    x = 0
    if p.agent is not None:
        x = p.agent.choose_x(state, player_id, source.printing.name, upper)
    x = min(max(0, x), upper)
    source.loyalty_counters -= x
    state.log.append(
        f"P{player_id} pays X={x} loyalty ({source.printing.name} now at "
        f"{source.loyalty_counters})"
    )
    return x


def apply_compleated(state: "GameState", obj) -> None:
    """Compleated (CR 702.150a): a permanent with compleated whose
    Phyrexian mana symbols were paid with life enters with two fewer
    loyalty counters for each such symbol. The count is the cast
    pipeline's ``GameObject.phyrexian_life_pips``; it is consumed here."""
    pips, obj.phyrexian_life_pips = obj.phyrexian_life_pips, 0
    if "compleated" not in obj.printing.keywords or pips <= 0:
        return
    obj.loyalty_counters = max(0, obj.loyalty_counters - 2 * pips)
    state.log.append(
        f"{obj.printing.name} enters with {2 * pips} fewer loyalty counters "
        "(compleated)"
    )


# ---------------------------------------------------------------------------
# Loyalty-ability effects
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PutUpToNCardsFromHandOntoBattlefield(Effect):
    """"Put up to <N> <filter> cards from your hand onto the battlefield."
    (Ugin, the Spirit Dragon's -10). The controller picks the cards one
    at a time (``choose_card_from_library``; ``None`` stops), then they
    enter one after another."""

    count: int = 1
    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        p = state.player_by_id(controller_id)
        chosen: list = []
        while len(chosen) < self.count and p.agent is not None:
            options = [
                c for c in p.hand
                if c not in chosen and self.card_filter.matches_card(state, c)
            ]
            if not options:
                break
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            card = next((c for c in options if c.id == pick), None)
            if card is None:
                break
            chosen.append(card)
        for card in chosen:
            p.hand.remove(card)
            state.log.append(
                f"P{controller_id} puts {card.printing.name} onto the "
                "battlefield"
            )
            put_into_battlefield(state, card, controller_id)

    def describe(self) -> str:
        return (
            f"put up to {self.count} {self.card_filter.describe()}s from your "
            "hand onto the battlefield"
        )


@dataclass(frozen=True)
class SeparatePermanentsIntoPilesSacrificeChosen(Effect):
    """"Separate all permanents target player controls into two piles.
    That player sacrifices all permanents in the pile of their choice."
    (Liliana of the Veil's -6). The controller sorts the permanents
    (one ``choose_yes_no`` per permanent: yes = pile 1); the target
    player then picks the pile to sacrifice through ``choose_mode``."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice

        pid = next((t[1] for t in targets if t[0] == "player"), None)
        if pid is None:
            return
        victim = state.player_by_id(pid)
        if victim.has_lost:
            return
        me = state.player_by_id(controller_id)
        perms = sorted(
            (o for o in state.battlefield if o.controller_id == pid),
            key=lambda o: o.id,
        )
        piles: tuple[list, list] = ([], [])
        for o in perms:
            first = True
            if me.agent is not None:
                first = me.agent.choose_yes_no(
                    state, controller_id,
                    f"Put {o.printing.name} into pile 1 (else pile 2)?",
                )
            piles[0 if first else 1].append(o)
        options = [
            f"sacrifice pile {i + 1}: "
            + (", ".join(o.printing.name for o in pile) or "nothing")
            for i, pile in enumerate(piles)
        ]
        pick = 0
        if victim.agent is not None:
            pick = victim.agent.choose_mode(state, pid, options)
        chosen = 1 if pick == 1 else 0
        doomed = piles[chosen]
        state.log.append(
            f"P{pid} sacrifices pile {chosen + 1} ({len(doomed)} permanent(s))"
        )
        for o in doomed:
            if o in state.battlefield:
                sacrifice(state, o)

    def describe(self) -> str:
        return (
            "separate all permanents target player controls into two piles; "
            "that player sacrifices the pile of their choice"
        )


@dataclass(frozen=True)
class MillTargetPlayerTimesX(Effect):
    """"Target player mills <K> times X cards." (Jace, the Perfected
    Mind's -X): ``MillTargetPlayer`` with ``K * X`` cards, X being the
    resolving ability's X."""

    per_x: int = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.draw_mill import MillTargetPlayer

        n = self.per_x * max(0, state.resolving_x_value)
        MillTargetPlayer(amount=n).resolve(state, controller_id, targets)

    def describe(self) -> str:
        k = "" if self.per_x == 1 else f"{self.per_x} times "
        return f"target player mills {k}X cards"


@dataclass(frozen=True)
class AnyGraveyardHasAtLeast:
    """"if a graveyard has <N> or more cards in it" -- any player's
    graveyard (Jace, the Perfected Mind's -2)."""

    n: int = 1

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return any(len(p.graveyard) >= self.n for p in state.players)


# ---------------------------------------------------------------------------
# Control changes
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ExchangeControlOfSourceAndTarget(Effect):
    """"Exchange control of this <permanent> and [up to one ]target <np>.
    If you don't or can't make an exchange, sacrifice this <permanent>.
    [This ability still resolves if its target becomes illegal.]"
    (Gilded Drake).

    The exchange happens only if the source is still on the battlefield
    and the target is still legal with a different controller (CR
    701.10); it sets ``controller_id`` directly and leaves both
    permanents summoning-sick for their new controllers (the Oko
    precedent, ``cube_glue.ExchangeControlOfTargets``). Otherwise the
    source is sacrificed. ``still_resolves_if_targets_illegal`` lifts the
    CR 608.2b fizzle (``actions._resolve_top_of_stack_item``) so an
    illegal target still leads to the sacrifice."""

    targets: tuple = ()
    still_resolves_if_targets_illegal: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import sacrifice

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        other = None
        if targets and self.targets:
            prior = state.resolving_controller_id
            try:
                state.resolving_controller_id = controller_id
                legal = _slot_target_still_legal(
                    state, self.targets[0], targets[0],
                )
            finally:
                state.resolving_controller_id = prior
            if legal:
                from mtgz_ir.keyword_rules import can_be_targeted_by

                other = next(
                    (o for o in state.battlefield if o.id == targets[0][1]),
                    None,
                )
                # Hexproof / shroud / protection gained in response make
                # the target illegal too (CR 608.2b).
                if other is not None and not can_be_targeted_by(
                    state, other, source, controller_id,
                ):
                    other = None
        if other is not None and other.controller_id != source.controller_id:
            source.controller_id, other.controller_id = (
                other.controller_id, source.controller_id,
            )
            source.summoning_sick = True
            other.summoning_sick = True
            state.log.append(
                f"{source.printing.name} and {other.printing.name} exchange "
                "controllers"
            )
            return
        state.log.append(
            f"no exchange: P{controller_id} sacrifices {source.printing.name}"
        )
        sacrifice(state, source)

    def describe(self) -> str:
        return (
            "exchange control of this permanent and up to one target; if you "
            "don't, sacrifice this permanent"
        )


@dataclass(frozen=True)
class GainControlUntapHasteUEOT(Effect):
    """"Gain control of target <np> until end of turn. Untap that
    <permanent>. It gains haste until end of turn." -- the Threaten shape
    over any permanent noun (Zealous Conscripts: "target permanent").
    Control rides the layer-2 cleanup-expiring registry
    (``state.grant_control_change_ueot``, the ``ThreatenTargetCreatureUEOT``
    path); haste is a cleanup-expiring layer-6 grant."""

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        if not targets or not self.targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, targets[0]))
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is None:  # pragma: no cover - legality checked above
            return
        src = state.resolving_source
        state.grant_control_change_ueot(
            source_id=src.id if src is not None else obj.id,
            target_id=obj.id,
            new_controller_id=controller_id,
            description=(
                f"P{controller_id} controls {obj.printing.name} until end of "
                "turn"
            ),
        )
        obj.tapped = False
        state.add_continuous_effect(
            source_id=src.id if src is not None else obj.id,
            layer=6, kind="grant_keyword", target_id=obj.id,
            params=("haste",), expires_at="cleanup",
            description=f"{obj.printing.name} gains haste until end of turn",
        )
        state.log.append(
            f"P{controller_id} gains control of {obj.printing.name} until end "
            "of turn; it untaps and gains haste"
        )

    def describe(self) -> str:
        return (
            "gain control of target permanent until end of turn; untap it; "
            "it gains haste until end of turn"
        )


# ---------------------------------------------------------------------------
# Flicker
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ExileTargetThenReturnUnderOwnersControl(Effect):
    """"[You may ]exile [another ]target <np>, then return it / that card
    to the battlefield under its owner's control." (Ephemerate, Felidar
    Guardian). The returned card is a new object (CR 400.7: fresh ETB,
    counters and attachments gone); a token exiled this way ceases to
    exist. ``optional`` ("you may") asks ``choose_yes_no`` first."""

    targets: tuple = ()
    optional: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import move_off_battlefield

        if not targets or not self.targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, targets[0]))
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is None:  # pragma: no cover - legality checked above
            return
        agent = state.player_by_id(controller_id).agent
        if self.optional and agent is not None and not agent.choose_yes_no(
            state, controller_id, f"Exile and return {obj.printing.name}?",
        ):
            return
        move_off_battlefield(state, obj, destination="exile")
        if obj.printing.is_token:
            return
        owner = state.player_by_id(obj.owner_id)
        if obj not in owner.exile:
            return  # a replacement sent it elsewhere
        owner.exile.remove(obj)
        obj.exiled_face_down = False  # left exile (CR 400.7)
        # CR 400.7: it returns as a new object -- no counters.
        from mtgz_ir.turn import _reset_left_battlefield_object

        _reset_left_battlefield_object(obj)
        state.log.append(f"{obj.printing.name} returns to the battlefield")
        put_into_battlefield(state, obj, obj.owner_id)

    def describe(self) -> str:
        may = "you may " if self.optional else ""
        return (
            f"{may}exile target permanent, then return it to the battlefield "
            "under its owner's control"
        )


# ---------------------------------------------------------------------------
# Rebound (CR 702.88)
# ---------------------------------------------------------------------------


def exile_with_rebound(state: "GameState", card, controller_id: int) -> None:
    """Rebound (CR 702.88a): the spell, cast from its owner's hand, is
    exiled as it resolves, with a delayed "at the beginning of your next
    upkeep, you may cast this card from exile without paying its mana
    cost" trigger (``turn.run_delayed_upkeep_triggers``, ``own_upkeep``)."""
    from mtgz_ir.game import Zone

    card.cast_via_hand = False
    card.zone = Zone.EXILE
    state.player_by_id(card.owner_id).exile.append(card)
    state.delayed_triggers.append({
        "step": "upkeep",
        "own_upkeep": True,
        "fire_at_turn": state.turn_number + 1,
        "controller_id": controller_id,
        "source_id": card.id,
        "effect": MayCastExiledSourceFree(),
    })
    state.log.append(f"{card.printing.name} is exiled (rebound)")


@dataclass(frozen=True)
class MayCastExiledSourceFree(Effect):
    """Rebound's delayed trigger: "you may cast this card from exile
    without paying its mana cost" -- if it is still that exiled card,
    castable now (targets available), and the controller agrees, it is
    cast through ``creature_casting_ub.cast_now`` (above the trigger)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.game import Zone

        card = state.resolving_source
        if card is None or card.zone != Zone.EXILE:
            return
        if card not in state.player_by_id(card.owner_id).exile:
            return
        if not can_cast_now(state, controller_id, card, free=True):
            state.log.append(f"{card.printing.name} can't be cast (rebound)")
            return
        agent = state.player_by_id(controller_id).agent
        if agent is not None and not agent.choose_yes_no(
            state, controller_id, f"Cast {card.printing.name} (rebound)?",
        ):
            return
        cast_now(state, controller_id, card, free=True)

    def describe(self) -> str:
        return "you may cast this card from exile without paying its mana cost"


# ---------------------------------------------------------------------------
# Nicol Bolas, God-Pharaoh
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetOpponentExilesUntilNonlandCastFree(Effect):
    """"Target opponent exiles cards from the top of their library until
    they exile a nonland card. Until end of turn, you may cast that card
    without paying its mana cost." (Nicol Bolas, God-Pharaoh's +2). The
    cards are exiled face up; the nonland card carries a this-turn,
    free, cast-only ``ExilePlayPermission`` for the controller."""

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
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            exile_card,
            grant_exile_play,
        )

        pid = next((t[1] for t in targets if t[0] == "player"), None)
        if pid is None:
            return
        opp = state.player_by_id(pid)
        while opp.library:
            card = opp.library.pop(0)
            exile_card(state, card)
            state.log.append(f"P{pid} exiles {card.printing.name}")
            if not card.printing.is_land:
                grant_exile_play(
                    state, card, controller_id, free=True, cast_only=True,
                )
                return

    def describe(self) -> str:
        return (
            "target opponent exiles cards from the top of their library until "
            "a nonland card; you may cast it free this turn"
        )


@dataclass(frozen=True)
class EachOpponentExilesCardsFromHand(Effect):
    """"Each opponent exiles <N> cards from their hand." (Nicol Bolas,
    God-Pharaoh's +1). Each opponent picks their own cards (the
    ``choose_discard`` hand-card picker); the cards are exiled face up."""

    count: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import exile_card

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            for _ in range(self.count):
                if not p.hand:
                    break
                ids = [c.id for c in p.hand]
                pick = ids[0]
                if p.agent is not None:
                    pick = p.agent.choose_discard(state, p.id, ids)
                card = next((c for c in p.hand if c.id == pick), p.hand[0])
                p.hand.remove(card)
                exile_card(state, card)
                state.log.append(
                    f"P{p.id} exiles {card.printing.name} from their hand"
                )

    def describe(self) -> str:
        n = "a card" if self.count == 1 else f"{self.count} cards"
        return f"each opponent exiles {n} from their hand"


# ---------------------------------------------------------------------------
# Counts ("equal to the number of ...", "where X is ...")
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CardsInYourZone:
    """Formula: the number of cards in the controller's ``zone``
    ("library" / "graveyard" / "hand") matching ``card_filter`` (``None``
    = any card) -- "the number of cards in your library" (Enter the
    Infinite), "the number of creature cards in your graveyard" (Grist)."""

    zone: ZoneName = ZoneName.LIBRARY
    card_filter: object = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "zone", ZoneName(self.zone))

    def value(self, state: "GameState", controller_id: int) -> int:
        cards = getattr(state.player_by_id(controller_id), self.zone)
        if self.card_filter is None:
            return len(cards)
        return sum(1 for c in cards if self.card_filter.matches_card(state, c))


@dataclass(frozen=True)
class SourceLoyalty:
    """Formula: the resolving source's loyalty ("where X is Nissa's
    loyalty") -- its current loyalty counters, or the last value it had
    if it has left the battlefield."""

    def value(self, state: "GameState", controller_id: int) -> int:
        src = state.resolving_source
        return max(0, src.loyalty_counters) if src is not None else 0


@dataclass(frozen=True)
class DrawCardsEqualTo(Effect):
    """"Draw cards equal to <count>." (Enter the Infinite)."""

    count: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        n = max(0, self.count.value(state, controller_id))
        drawn = draw_n_cards(state, controller_id, n)
        state.log.append(f"P{controller_id} draws {drawn} card(s)")

    def describe(self) -> str:
        return "draw cards equal to a count"


@dataclass(frozen=True)
class EachOpponentLosesLifeEqualTo(Effect):
    """"Each opponent loses life equal to <count>." (Grist's -5)."""

    count: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        n = max(0, self.count.value(state, controller_id))
        for p in state.players:
            if p.id != controller_id and not p.has_lost and n:
                p.life -= n
                state.log.append(f"P{p.id} loses {n} life")

    def describe(self) -> str:
        return "each opponent loses life equal to a count"


@dataclass(frozen=True)
class CreateTokenWithPTEqualTo(Effect):
    """"Create an X/X <token>, where X is <count>." (Nissa, Ascended
    Animist's +1): the parsed token printing with power and toughness
    set to the count at resolution."""

    token_printing: object = None
    count: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        import dataclasses as _dc

        from mtgz_ir.effects.generic.tokens import CreateToken

        x = max(0, self.count.value(state, controller_id))
        printing = _dc.replace(self.token_printing, power=x, toughness=x)
        CreateToken(token_printing=printing).resolve(state, controller_id, ())

    def describe(self) -> str:
        return f"create an X/X {self.token_printing.name} token"


@dataclass(frozen=True)
class NoMaximumHandSizeUntilYourNextTurn(Effect):
    """"You have no maximum hand size until your next turn." (Enter the
    Infinite): an emblem carrying the "no maximum hand size" static
    (read by ``turn.run_cleanup_step``) that expires as its controller's
    next turn begins (``GameObject.expires_at_untap_of``)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.cards.base import make_emblem
        from mtgz_ir.effects.base import StaticAbility, StaticScope
        from mtgz_ir.game import Zone

        printing = make_emblem(
            name="no maximum hand size",
            static_abilities=(StaticAbility(
                scope=StaticScope.SELF, grants_no_max_hand_size=True,
            ),),
        )
        holder = state.new_object(
            printing, owner_id=controller_id, zone=Zone.COMMAND,
        )
        holder.expires_at_untap_of = controller_id
        state.emblems.append(holder)
        state.log.append(
            f"P{controller_id} has no maximum hand size until their next turn"
        )

    def describe(self) -> str:
        return "you have no maximum hand size until your next turn"


@dataclass(frozen=True)
class PumpCreaturesYouControlPerCount(Effect):
    """"Until end of turn, creatures you control get +P/+T for each
    <count> and gain <keywords>." (Nissa, Ascended Animist's -7): the
    creatures you control as it resolves (CR 611.2c) get until-end-of-
    turn bonuses (``temp_power_bonus`` / ``temp_toughness_bonus``) and
    cleanup-expiring layer-6 keyword grants."""

    power: int = 0
    toughness: int = 0
    count: object = None
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_creature_now

        n = max(0, self.count.value(state, controller_id))
        src = state.resolving_source
        for obj in list(state.battlefield):
            if obj.controller_id != controller_id or not is_creature_now(obj):
                continue
            sid = src.id if src is not None else obj.id
            # UEOT P/T bonuses ride the cleanup-cleared temp fields (the
            # engine's until-end-of-turn pump path).
            obj.temp_power_bonus += self.power * n
            obj.temp_toughness_bonus += self.toughness * n
            for kw in sorted(self.keywords):
                state.add_continuous_effect(
                    source_id=sid, layer=6, kind="grant_keyword",
                    target_id=obj.id, params=(kw,), expires_at="cleanup",
                    description=f"{obj.printing.name} gains {kw}",
                )
        state.log.append(
            f"creatures P{controller_id} controls get +{self.power * n}/"
            f"+{self.toughness * n} until end of turn"
        )

    def describe(self) -> str:
        return "creatures you control get a per-count bonus until end of turn"


@dataclass(frozen=True)
class GrantKeywordsUntilYourNextTurn(Effect):
    """"[Up to one ]target <np> gains <keywords> until your next turn."
    (Dihada, Binder of Wills' +2): layer-6 grants that expire as the
    controller's next turn begins (``expires_at="controller_untap"``)."""

    targets: tuple = ()
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        src = state.resolving_source
        for slot, chosen in zip(self.targets, targets):
            prior = state.resolving_controller_id
            try:
                state.resolving_controller_id = controller_id
                legal = _slot_target_still_legal(state, slot, chosen)
            finally:
                state.resolving_controller_id = prior
            if not legal:
                state.log.append(_fizzle_message(state, chosen))
                continue
            obj = next((o for o in state.battlefield if o.id == chosen[1]), None)
            if obj is None:  # pragma: no cover - legality checked above
                continue
            for kw in sorted(self.keywords):
                state.add_continuous_effect(
                    source_id=src.id if src is not None else obj.id,
                    layer=6, kind="grant_keyword", target_id=obj.id,
                    params=(kw,), expires_at="controller_untap",
                    expires_at_player_id=controller_id,
                    description=f"{obj.printing.name} gains {kw}",
                )
            state.log.append(
                f"{obj.printing.name} gains {', '.join(sorted(self.keywords))} "
                "until your next turn"
            )

    def describe(self) -> str:
        return (
            f"target gains {', '.join(sorted(self.keywords))} until your "
            "next turn"
        )


@dataclass(frozen=True)
class GainControlOfAllUntapHasteUEOT(Effect):
    """"Gain control of all <np> until end of turn. Untap them. They gain
    haste until end of turn." (Dihada, Binder of Wills' -11): the
    Threaten shape over every matching permanent."""

    permanent_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            victims = [
                o for o in state.battlefield
                if self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        src = state.resolving_source
        for obj in victims:
            sid = src.id if src is not None else obj.id
            if obj.controller_id != controller_id:
                state.grant_control_change_ueot(
                    source_id=sid, target_id=obj.id,
                    new_controller_id=controller_id,
                    description=(
                        f"P{controller_id} controls {obj.printing.name} until "
                        "end of turn"
                    ),
                )
            obj.tapped = False
            state.add_continuous_effect(
                source_id=sid, layer=6, kind="grant_keyword",
                target_id=obj.id, params=("haste",), expires_at="cleanup",
                description=f"{obj.printing.name} gains haste",
            )
        state.log.append(
            f"P{controller_id} gains control of {len(victims)} permanent(s) "
            "until end of turn"
        )

    def describe(self) -> str:
        return "gain control of all matching permanents until end of turn"


@dataclass(frozen=True)
class RevealTopPutMatchingInHandRestInGraveyard(Effect):
    """"Reveal the top <N> cards of your library. Put any number of
    <filter> cards from among them into your hand and the rest into your
    graveyard. [Create a Treasure token for each card put into your
    graveyard this way.]" (Dihada, Binder of Wills' -3). The controller
    picks the hand cards one at a time (``choose_card_from_library``;
    ``None`` stops)."""

    count: int = 1
    card_filter: object = None
    treasure_per_graveyard_card: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        revealed = list(p.library[: self.count])
        for c in revealed:
            reveal_to_all(state, c)
        keep: list = []
        while p.agent is not None:
            options = [
                c for c in revealed
                if c not in keep and self.card_filter.matches_card(state, c)
            ]
            if not options:
                break
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            card = next((c for c in options if c.id == pick), None)
            if card is None:
                break
            keep.append(card)
        binned = 0
        for c in revealed:
            p.library.remove(c)
            if c in keep:
                c.zone = Zone.HAND
                p.hand.append(c)
            else:
                c.zone = Zone.GRAVEYARD
                p.graveyard.append(c)
                binned += 1
        state.log.append(
            f"P{controller_id} reveals {len(revealed)}: {len(keep)} to hand, "
            f"{binned} to the graveyard"
        )
        if self.treasure_per_graveyard_card and binned:
            from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
            from mtgz_ir.effects.generic.tokens import CreateToken

            CreateToken(
                token_printing=TOKEN_PRINTINGS["Treasure"], count=binned,
            ).resolve(state, controller_id, ())

    def describe(self) -> str:
        rider = "; a Treasure for each card put into your graveyard" if (
            self.treasure_per_graveyard_card
        ) else ""
        return (
            f"reveal the top {self.count} cards; put any number of "
            f"{self.card_filter.describe()}s into your hand and the rest into "
            f"your graveyard{rider}"
        )


@dataclass(frozen=True)
class EachOpponentMayScry(Effect):
    """"Each opponent may scry <N>." (Kaya, Intangible Slayer's 0): each
    opponent agrees through ``choose_yes_no`` and scries with the real
    keep / bottom decision (``Scry(bottom_choice=True)``)."""

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.library_lookat import Scry

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            if p.agent is not None and not p.agent.choose_yes_no(
                state, p.id, f"Scry {self.amount}?",
            ):
                continue
            Scry(amount=self.amount, bottom_choice=True).resolve(state, p.id)

    def describe(self) -> str:
        return f"each opponent may scry {self.amount}"


# ---------------------------------------------------------------------------
# Token copies with "except" riders (Saheeli Rai, Kaya, Intangible Slayer)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CopyExceptions:
    """The "except ..." riders of a token copy (CR 707.9b: they become
    part of the copiable values): added card types / subtypes, a set
    base P/T, a set color, added keywords, added supertypes ("except
    it's legendary", Adagia, Windswept Bastion)."""

    add_types: tuple = ()
    add_subtypes: tuple = ()
    add_supertypes: tuple = ()
    base_pt: "tuple | None" = None
    colors: "frozenset | None" = None
    add_keywords: frozenset[KeywordSpelling] = frozenset()

    def apply(self, base):
        import dataclasses as _dc

        changes: dict = {
            "supertypes": tuple(base.supertypes) + tuple(
                s for s in self.add_supertypes if s not in base.supertypes
            ) + (
                () if "Token" in base.supertypes else ("Token",)
            ),
            "transform_back_face": None,
            "mdfc_back_face": None,
        }
        extra_types = tuple(t for t in self.add_types if t not in base.card_types)
        if extra_types:
            changes["card_types"] = tuple(base.card_types) + extra_types
        extra_subs = tuple(
            s for s in self.add_subtypes if s not in base.subtypes
        )
        if extra_subs:
            changes["subtypes"] = tuple(base.subtypes) + extra_subs
        if self.base_pt is not None:
            changes["power"], changes["toughness"] = self.base_pt
        if self.colors is not None:
            changes["face_colors"] = self.colors
            changes["color_indicator"] = frozenset()
        if self.add_keywords:
            changes["keywords"] = frozenset(base.keywords) | self.add_keywords
        return _dc.replace(base, **changes)


def _create_token_copy(state, controller_id, base, exceptions):
    """Create one token copy of ``base`` (a printing) with
    ``exceptions``; returns the new token object (or ``None``)."""
    from mtgz_ir.effects.generic.tokens import CreateToken

    printing = exceptions.apply(base)
    before = {o.id for o in state.battlefield}
    CreateToken(token_printing=printing).resolve(state, controller_id, ())
    return next(
        (o for o in state.battlefield if o.id not in before), None,
    )


@dataclass(frozen=True)
class CreateTokenCopyOfTargetWithExceptions(Effect):
    """"Create a token that's a copy of target <np>, except <riders>.
    [That token gains haste.] [Exile it at the beginning of the next end
    step.]" (Saheeli Rai's -2). Haste is a permanent self-sourced grant
    (it lasts as long as the token); the exile is a delayed "next end
    step" trigger."""

    targets: tuple = ()
    exceptions: CopyExceptions = CopyExceptions()
    gains_haste: bool = False
    exile_at_next_end_step: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.delayed_actions import (
            ExileSourceDelayed,
            schedule_at_next_end_step,
        )
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        if not targets or not self.targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, targets[0]))
            return
        target = next(
            (o for o in state.battlefield if o.id == targets[0][1]), None,
        )
        if target is None:  # pragma: no cover - legality checked above
            return
        token = _create_token_copy(
            state, controller_id, target.printing, self.exceptions,
        )
        if token is None:
            return
        if self.gains_haste:
            state.add_continuous_effect(
                source_id=token.id, layer=6, kind="grant_keyword",
                target_id=token.id, params=("haste",),
                description=f"{token.printing.name} gains haste",
            )
        if self.exile_at_next_end_step:
            schedule_at_next_end_step(
                state, controller_id, token.id, ExileSourceDelayed(),
            )

    def describe(self) -> str:
        return "create a token that's a copy of target permanent, except ..."


@dataclass(frozen=True)
class ExileTargetThenTokenCopyUnlessAura(Effect):
    """"Exile target <np>. If it wasn't an Aura, create a token that's a
    copy of it, except <riders>." (Kaya, Intangible Slayer's -3). The
    token copies the exiled card's copiable values (its printing as it
    sits in exile); "it wasn't an Aura" reads the permanent as it last
    existed on the battlefield."""

    targets: tuple = ()
    exceptions: CopyExceptions = CopyExceptions()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )
        from mtgz_ir.game import effective_subtypes
        from mtgz_ir.turn import move_off_battlefield

        if not targets or not self.targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, targets[0]))
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is None:  # pragma: no cover - legality checked above
            return
        was_aura = "Aura" in effective_subtypes(state, obj)
        move_off_battlefield(state, obj, destination="exile")
        if was_aura:
            return
        _create_token_copy(state, controller_id, obj.printing, self.exceptions)

    def describe(self) -> str:
        return (
            "exile target permanent; if it wasn't an Aura, create a token "
            "copy of it, except ..."
        )


@dataclass(frozen=True)
class SearchUpToNDifferentNamesOntoBattlefield(Effect):
    """"Search your library for up to <N> <filter> cards with different
    names, put them onto the battlefield, then shuffle." (Saheeli Rai's
    -7). Cards are picked one at a time (``choose_card_from_library``;
    ``None`` stops), each with a name not picked yet."""

    count: int = 1
    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        chosen: list = []
        while len(chosen) < self.count and p.agent is not None:
            names = {c.printing.name for c in chosen}
            options = [
                c for c in p.library
                if c not in chosen and c.printing.name not in names
                and self.card_filter.matches_card(state, c)
            ]
            if not options:
                break
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            card = next((c for c in options if c.id == pick), None)
            if card is None:
                break
            chosen.append(card)
        for card in chosen:
            p.library.remove(card)
            state.log.append(
                f"P{controller_id} puts {card.printing.name} onto the "
                "battlefield"
            )
            put_into_battlefield(state, card, controller_id)
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            f"search for up to {self.count} {self.card_filter.describe()}s "
            "with different names and put them onto the battlefield"
        )


# ---------------------------------------------------------------------------
# Emrakul, the Promised End
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PerCardTypeInYourGraveyard(Condition):
    """Variable-amount condition: the number of card types among cards
    in the viewer's graveyard (the shared delirium count,
    ``effects.card_types``). ``matches`` iff at least one; ``multiplier``
    is the count -- "This spell costs {1} less to cast for each card type
    among cards in your graveyard." (Emrakul, the Promised End)."""

    def _count(self, state, viewer_id) -> int:
        from mtgz_ir.effects.card_types import count_card_types

        return count_card_types(state.player_by_id(viewer_id).graveyard)

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return self._count(state, viewer_id) > 0

    def multiplier(self, state, viewer_id, chosen_targets=None) -> int:
        return self._count(state, viewer_id)


@dataclass(frozen=True)
class ControlTargetOpponentDuringNextTurn(Effect):
    """"You gain control of target opponent during that player's next
    turn. [After that turn, that player takes an extra turn.]" (Emrakul,
    the Promised End's cast trigger). The controller's agent makes the
    target's decisions during their next turn (the Dominion Bracelet
    mechanism: ``Player.scheduled_agent_override``, installed by
    ``turn.run_untap_step`` and reverted at that turn's cleanup);
    ``extra_turn_after`` makes that cleanup queue an extra turn for the
    controlled player (``Player.extra_turn_after_controlled_turn``)."""

    extra_turn_after: bool = False

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
        pid = next((t[1] for t in targets if t[0] == "player"), None)
        if pid is None:
            return
        target = state.player_by_id(pid)
        if target.has_lost:
            return
        target.scheduled_agent_override = state.player_by_id(controller_id).agent
        target.scheduled_agent_override_by = controller_id
        if self.extra_turn_after:
            target.extra_turn_after_controlled_turn = True
        state.log.append(
            f"P{controller_id} will control P{pid} during P{pid}'s next turn"
        )

    def describe(self) -> str:
        rider = "; after that turn, they take an extra turn" if (
            self.extra_turn_after
        ) else ""
        return f"you control target opponent during their next turn{rider}"


def extra_turn_after_controlled_turn(state: "GameState", player) -> None:
    """At the cleanup of a turn another player controlled: "After that
    turn, that player takes an extra turn." -- the most recently created
    extra turn is taken first (CR 500.7), so it goes to the front of
    ``state.extra_turn_queue``."""
    if not player.extra_turn_after_controlled_turn:
        return
    player.extra_turn_after_controlled_turn = False
    state.extra_turn_queue.insert(0, player.id)
    state.log.append(f"P{player.id} will take an extra turn")


# ---------------------------------------------------------------------------
# Chain of Vapor
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class BounceThenChainCopyBySacrificingLand(Effect):
    """"Return target <np> to its owner's hand. Then that permanent's
    controller may sacrifice a land of their choice. If the player does,
    they may copy this spell and may choose a new target for that copy."
    (Chain of Vapor). The bounced permanent's controller (as it last
    existed) decides through ``choose_yes_no`` / ``choose_sacrifice``;
    the copy goes on the stack under their control
    (``actions.push_spell_copy``, the Chain Lightning precedent)."""

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import push_spell_copy
        from mtgz_ir.effects.generic.spell_copies import _choose_copy_targets
        from mtgz_ir.effects.generic.targets_helpers import (
            _bounce_to_hand,
            _fizzle_message,
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import sacrifice

        if not targets or not self.targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append(_fizzle_message(state, targets[0]))
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is None:  # pragma: no cover - legality checked above
            return
        chooser = state.player_by_id(obj.controller_id)
        _bounce_to_hand(state, obj)
        state.log.append(f"{obj.printing.name} returns to its owner's hand")
        spell = state.resolving_spell
        if spell is None or chooser.has_lost or chooser.agent is None:
            return
        lands = [
            o for o in state.battlefield
            if o.controller_id == chooser.id and o.printing.is_land
        ]
        if not lands or not chooser.agent.choose_yes_no(
            state, chooser.id, f"Sacrifice a land to copy {spell.printing.name}?",
        ):
            return
        pick = chooser.agent.choose_sacrifice(
            state, chooser.id, [o.id for o in lands],
        )
        land = next((o for o in lands if o.id == pick), lands[0])
        sacrifice(state, land)
        if not chooser.agent.choose_yes_no(
            state, chooser.id, f"Copy {spell.printing.name}?",
        ):
            return
        copy_targets = _choose_copy_targets(
            state, chooser.id, spell.printing, targets,
        )
        push_spell_copy(state, spell.printing, chooser.id, copy_targets)

    def describe(self) -> str:
        return (
            "return target permanent to its owner's hand; its controller may "
            "sacrifice a land to copy this spell"
        )


# ---------------------------------------------------------------------------
# Sin, Spira's Punishment
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ExileRandomGraveyardCardTokenCopyRepeatIfLand(Effect):
    """"Exile a <filter> card from your graveyard at random, then create a
    tapped token that's a copy of that card. If the exiled card is a land
    card, repeat this process." (Sin, Spira's Punishment). The random
    pick uses ``state.rng``; the token copies the card's copiable values
    (its printing) and enters tapped."""

    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import exile_card
        from mtgz_ir.effects.generic.tokens import CreateToken

        p = state.player_by_id(controller_id)
        while True:
            options = [
                c for c in p.graveyard if self.card_filter.matches_card(state, c)
            ]
            if not options:
                return
            card = state.rng.choice(options)
            p.graveyard.remove(card)
            exile_card(state, card)
            state.log.append(
                f"P{controller_id} exiles {card.printing.name} at random"
            )
            CreateToken(
                token_printing=CopyExceptions().apply(card.printing),
                tapped=True,
            ).resolve(state, controller_id, ())
            if not card.printing.is_land:
                return

    def describe(self) -> str:
        return (
            f"exile a {self.card_filter.describe()} from your graveyard at "
            "random and create a tapped token copy; repeat if it's a land"
        )


# ---------------------------------------------------------------------------
# Chandra, Hope's Beacon
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CopyTriggeringSpell(Effect):
    """"Whenever you cast <a spell>, copy it. You may choose new targets
    for the copy." (Chandra, Hope's Beacon). The triggering spell is the
    event's ``spell`` subject (``event_subject_id``, bound when the
    trigger is put on the stack); if it is still on the stack it is
    copied under the trigger's controller (``actions.push_spell_copy``),
    new targets through ``spell_copies._choose_copy_targets``."""

    def event_subject_id(self, subjects) -> int:
        spell = subjects.get("spell")
        return spell.id if spell is not None else -1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import push_spell_copy
        from mtgz_ir.effects.generic.spell_copies import _choose_copy_targets

        sid = state.resolving_event_subject_id
        item = next(
            (
                i for i in state.stack
                if i.ability is None and i.source.id == sid
            ),
            None,
        )
        if item is None:
            state.log.append("the spell to copy is gone")
            return
        printing = item.source.printing
        copy_targets = _choose_copy_targets(
            state, controller_id, printing, item.targets,
        )
        push_spell_copy(
            state, printing, controller_id, copy_targets, x_value=item.x_value,
        )
        state.log.append(f"P{controller_id} copies {printing.name}")

    def describe(self) -> str:
        return "copy that spell; you may choose new targets for the copy"


@dataclass(frozen=True)
class GroupedExilePlayPermission(ExilePlayPermission):
    """"You may cast <a spell> from among those exiled cards": one
    permission per eligible card, all void once any card of the group
    has left exile (the one that was cast)."""

    group_ids: tuple = ()

    def usable_now(self, state, player_id) -> bool:
        if not ExilePlayPermission.usable_now(self, state, player_id):
            return False
        in_exile = {c.id for pl in state.players for c in pl.exile}
        return all(gid in in_exile for gid in self.group_ids)


@dataclass(frozen=True)
class ExileTopMayCastOneFromAmongUntilEndOfNextTurn(Effect):
    """"Exile the top <N> cards of your library. Until the end of your
    next turn, you may cast <a filter> spell from among those exiled
    cards." (Chandra, Hope's Beacon's +1). The cards are exiled face
    up; each matching card gets a grouped, cast-only permission lasting
    until the end of the controller's next turn, and casting one voids
    the rest ("a spell", one)."""

    count: int = 1
    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            UNTIL_END_OF_YOUR_NEXT_TURN,
            exile_card,
        )

        p = state.player_by_id(controller_id)
        exiled = []
        for _ in range(self.count):
            if not p.library:
                break
            card = p.library.pop(0)
            exile_card(state, card)
            exiled.append(card)
        eligible = [c for c in exiled if self.card_filter.matches_card(state, c)]
        group = tuple(c.id for c in eligible)
        for card in eligible:
            card.exile_play_permission = GroupedExilePlayPermission(
                player_id=controller_id,
                duration=UNTIL_END_OF_YOUR_NEXT_TURN,
                granted_turn=state.turn_number,
                granted_player_turns=p.turns_started,
                cast_only=True,
                group_ids=group,
            )
        state.log.append(
            f"P{controller_id} exiles {len(exiled)} card(s); may cast one of "
            f"{len(eligible)}"
        )

    def describe(self) -> str:
        return (
            f"exile the top {self.count} cards; until the end of your next "
            f"turn, you may cast a {self.card_filter.describe()} from among them"
        )


# ---------------------------------------------------------------------------
# "Whenever you gain life, target opponent loses that much life."
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetOpponentLosesThatMuchLife(Effect):
    """"Target opponent loses that much life." -- "that much" is the
    amount of the triggering event (``state.resolving_counter_amount``;
    ``Event.GAINS_LIFE`` carries the life gained -- Professor Dellian
    Fel's emblem, the only printing of the phrase in any pool, always
    after "Whenever you gain life")."""

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
        n = state.resolving_counter_amount
        pid = next((t[1] for t in targets if t[0] == "player"), None)
        if pid is None:
            return
        if n <= 0:
            state.log.append("no amount: the opponent loses no life")
            return
        opp = state.player_by_id(pid)
        opp.life -= n
        state.log.append(f"P{pid} loses {n} life")

    def describe(self) -> str:
        return "target opponent loses that much life"


# ---------------------------------------------------------------------------
# "Exchange control of two target <np> that share a card type." (Bilbo)
# ---------------------------------------------------------------------------


def _current_card_types(state, obj) -> frozenset:
    """The card types ``obj`` has right now (``game.effective_card_types``:
    layer 4, creature-ness included)."""
    from mtgz_ir.game import effective_card_types

    return frozenset(effective_card_types(state, obj))


@dataclass(frozen=True)
class ExchangeControlOfTwoTargetsSharingCardType(Effect):
    """"Exchange control of two target <np> that share a card type."
    (Bilbo, Luckwearer's Burglar's Plot). The second target must share a
    card type with the first (``legal_targets_for_slot`` with
    ``slot_targets_depend_on_chosen``, honoured by both the spell and
    the ability target pickers). On resolution both must still be legal
    and share a card type; the exchange sets ``controller_id`` directly
    (the Oko precedent) and does nothing when they have one controller
    (CR 701.10)."""

    targets: tuple = ()
    slot_targets_depend_on_chosen = True

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def _pool(self, state, controller_id):
        return self._enumerate_for_filter(
            state, controller_id, self.targets[0].filter,
        )

    def legal_targets(self, state, controller_id):
        pool = self._pool(state, controller_id)
        by_id = {o.id: o for o in state.battlefield}
        return [
            t for t in pool
            if any(
                u != t and _current_card_types(state, by_id[t[1]])
                & _current_card_types(state, by_id[u[1]])
                for u in pool
            )
        ]

    def legal_targets_for_slot(self, state, controller_id, slot_idx, chosen=()):
        if slot_idx == 0 or not chosen:
            return self.legal_targets(state, controller_id)
        by_id = {o.id: o for o in state.battlefield}
        first = by_id.get(chosen[0][1])
        if first is None:
            return []
        return [
            t for t in self._pool(state, controller_id)
            if t != chosen[0]
            and _current_card_types(state, by_id[t[1]]) & _current_card_types(state, first)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        if len(targets) < 2 or targets[0] == targets[1]:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = all(
                _slot_target_still_legal(state, self.targets[0], t)
                for t in targets[:2]
            )
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append("exchange of control fizzles")
            return
        a = next(o for o in state.battlefield if o.id == targets[0][1])
        b = next(o for o in state.battlefield if o.id == targets[1][1])
        if not (_current_card_types(state, a) & _current_card_types(state, b)):
            state.log.append("no exchange: they no longer share a card type")
            return
        if a.controller_id == b.controller_id:
            return
        a.controller_id, b.controller_id = b.controller_id, a.controller_id
        a.summoning_sick = True
        b.summoning_sick = True
        state.log.append(
            f"{a.printing.name} and {b.printing.name} exchange controllers"
        )

    def describe(self) -> str:
        return (
            "exchange control of two target permanents that share a card type"
        )


# ---------------------------------------------------------------------------
# "Gain control of target <filter> spell." (Commandeer)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GainControlOfTargetSpell(Effect):
    """"Gain control of target <filter> spell. You may choose new targets
    for it." (Commandeer). The spell's stack item (and its card) change
    controller -- a permanent spell enters under the new controller --
    and the new controller re-picks its targets through
    ``spell_copies._choose_copy_targets`` (the Invert Polarity path)."""

    card_filter: object = None

    def requires_target(self) -> bool:
        return True

    def _matches(self, item) -> bool:
        return (
            item.ability is None and not item.countered
            and (
                self.card_filter is None
                or self.card_filter.matches(item.source.printing)
            )
        )

    def legal_targets(self, state, controller_id):
        return [
            ("stack", i) for i, item in enumerate(state.stack)
            if self._matches(item)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.spell_copies import _choose_copy_targets

        if not targets or targets[0][0] != "stack":
            return
        idx = targets[0][1]
        if not 0 <= idx < len(state.stack) or not self._matches(state.stack[idx]):
            state.log.append("gain control of spell fizzles")
            return
        item = state.stack[idx]
        item.controller_id = controller_id
        item.source.controller_id = controller_id
        if item.targets:
            item.targets = _choose_copy_targets(
                state, controller_id, item.source.printing, item.targets,
            )
        state.log.append(
            f"P{controller_id} gains control of {item.source.printing.name}"
        )

    def describe(self) -> str:
        return "gain control of target spell; you may choose new targets for it"
