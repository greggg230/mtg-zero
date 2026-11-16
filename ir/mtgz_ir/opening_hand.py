"""Opening-hand actions (CR 103.6): cards that let a player begin the
game with them on the battlefield.

After every player has kept their opening hand (and bottomed their
mulligan cards), starting with the starting player and proceeding in
turn order, each player may take the actions their opening-hand cards
allow (CR 103.6, 103.6a). The printed shape is::

    If this card is in your opening hand[ and you're not the starting
    player], you may begin the game with <it | this card's name> on the
    battlefield[ with a <kind> counter on it].[ If you do, exile <N>
    card(s) from your hand.]

(Gemstone Caverns; the Leylines' shorter form is the same shape with no
optional parts.) The parser builds a :class:`BeginGameOnBattlefield` into
``CardPrinting.opening_hand_action``; ``simulate.setup_game`` calls
:func:`apply_opening_hand_actions` once, after the final keep.

More pregame actions share the field. Every action carries ``timing``
(:data:`AFTER_KEEP` or :data:`MULLIGAN_WINDOW`) and ``apply``; besides
roto P1's ``effects.generic.roto_stack.RevealForFirstSpellTax`` (Chancellor
of the Annex), roto package P6 added:

* :class:`RevealFromOpeningHand` -- "You may reveal this card from your
  opening hand. If you do, at the beginning of your first main phase of
  the game, <effect>." (Chancellor of the Tangle). Also a CR 103.6
  opening-hand action: revealing it schedules a delayed triggered ability
  (a ``state.delayed_triggers`` ``"step": "main"`` entry, fired by
  ``turn.run_delayed_main_phase_triggers`` at the player's next main
  phase -- their first of the game, since nothing precedes it).
* :class:`ExchangeHandWhileMulliganing` -- "Any time you could mulligan
  and this card is in your hand, you may exile all the cards from your
  hand, then draw that many cards." (Serum Powder). Not an opening-hand
  action: it is offered in every mulligan window, before the player's
  keep-or-mulligan decision (:func:`apply_mulligan_window_actions`,
  called from ``simulate.setup_game``'s mulligan loop). Using it is not a
  mulligan -- no extra card goes to the bottom on keep.

The "you may" is the player's decision, asked through the generic
``Agent.choose_yes_no`` hook; the card exiled for the "If you do" rider
is picked through ``Agent.choose_discard`` (the hook every "exile a card
from your hand" cost uses). Both are pre-game decisions made by the
seat's setup agent -- search never sees them (``docs/SEARCH_LIMITATIONS.md``).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, ClassVar

if TYPE_CHECKING:
    from mtgz_ir.effects.base import Effect
    from mtgz_ir.game import GameObject, GameState

# When an action is offered: after the final keep (CR 103.6) or in each
# mulligan window (before the keep-or-mulligan decision).
AFTER_KEEP = "after_keep"
MULLIGAN_WINDOW = "mulligan_window"


@dataclass(frozen=True)
class BeginGameOnBattlefield:
    """"You may begin the game with this card on the battlefield."

    ``only_if_not_starting_player`` -- "and you're not the starting
    player" (Gemstone Caverns). ``counters`` -- ``((kind, n), ...)`` the
    permanent begins the game with ("with a luck counter on it").
    ``exile_from_hand`` -- "If you do, exile N card(s) from your hand."
    """

    only_if_not_starting_player: bool = False
    counters: tuple[tuple[str, int], ...] = ()
    exile_from_hand: int = 0

    timing: ClassVar[str] = AFTER_KEEP

    def available_to(self, player_id: int, starting_player_id: int) -> bool:
        return not (
            self.only_if_not_starting_player and player_id == starting_player_id
        )

    def describe(self, name: str) -> str:
        text = f"begin the game with {name} on the battlefield"
        if self.counters:
            text += " with " + ", ".join(
                f"{n} {kind} counter{'s' if n != 1 else ''}"
                for kind, n in self.counters
            ) + " on it"
        return text

    def apply(self, state: "GameState", player, card: "GameObject") -> None:
        _begin_on_battlefield(state, player, card, self)


@dataclass(frozen=True)
class RevealFromOpeningHand:
    """"You may reveal this card from your opening hand. If you do, at the
    beginning of your first main phase of the game, <effect>."

    ``effect`` is the delayed triggered ability's effect (Chancellor of
    the Tangle: ``AddManaToPool(G)``); its source is the revealed card,
    wherever it is when the trigger fires."""

    effect: "Effect"

    timing: ClassVar[str] = AFTER_KEEP

    def available_to(self, player_id: int, starting_player_id: int) -> bool:
        return True

    def describe(self, name: str) -> str:
        return f"reveal {name} from your opening hand"

    def apply(self, state: "GameState", player, card: "GameObject") -> None:
        from mtgz_ir.game import reveal_to_all

        reveal_to_all(state, card)
        state.delayed_triggers.append({
            "step": "main",
            "controller_id": player.id,
            "source_id": card.id,
            "effect": self.effect,
            "fire_at_turn": state.turn_number,
        })
        state.log.append(
            f"P{player.id} reveals {card.printing.name} from their opening "
            f"hand"
        )


@dataclass(frozen=True)
class ExchangeHandWhileMulliganing:
    """"Any time you could mulligan and this card is in your hand, you may
    exile all the cards from your hand, then draw that many cards." """

    timing: ClassVar[str] = MULLIGAN_WINDOW

    def describe(self, name: str) -> str:
        return f"exile your hand with {name}, then draw that many cards"

    def apply(self, state: "GameState", player, card: "GameObject") -> None:
        from mtgz_ir.game import Zone, reveal_to_all
        from mtgz_ir.turn import draw_card

        exiled = list(player.hand)
        player.hand.clear()
        for c in exiled:
            c.zone = Zone.EXILE
            player.exile.append(c)
            reveal_to_all(state, c)
        state.log.append(
            f"P{player.id} exiles their hand ({len(exiled)} cards) with "
            f"{card.printing.name}"
        )
        for _ in exiled:
            draw_card(state, player.id)


def _exile_from_hand(state: "GameState", player, n: int) -> None:
    from mtgz_ir.game import Zone, reveal_to_all

    for _ in range(n):
        if not player.hand:
            return
        ids = [c.id for c in player.hand]
        pick = player.agent.choose_discard(state, player.id, ids)
        card = next((c for c in player.hand if c.id == pick), None)
        if card is None:
            raise ValueError(
                f"P{player.id} agent.choose_discard returned {pick!r}, "
                f"not a card in hand {ids}"
            )
        player.hand.remove(card)
        card.zone = Zone.EXILE
        player.exile.append(card)
        reveal_to_all(state, card)
        state.log.append(
            f"P{player.id} exiles {card.printing.name} from their hand"
        )


def _begin_on_battlefield(
    state: "GameState", player, card: "GameObject",
    action: BeginGameOnBattlefield,
) -> None:
    from mtgz_ir.actions import put_into_battlefield
    from mtgz_ir.game import add_counters

    player.hand.remove(card)
    put_into_battlefield(state, card, player.id)
    for kind, n in action.counters:
        add_counters(state, card, kind, n)
    state.log.append(
        f"P{player.id} begins the game with "
        f"{action.describe(card.printing.name)}"
    )
    _exile_from_hand(state, player, action.exile_from_hand)


def apply_opening_hand_actions(
    state: "GameState", starting_player_idx: int,
) -> None:
    """Run every opening-hand action (CR 103.6): players in turn order
    from the starting player, each card in hand order. A card is only
    offered while it is still in its owner's hand (an earlier "exile a
    card from your hand" can take it)."""
    n = len(state.players)
    starting_player_id = state.players[starting_player_idx].id
    for offset in range(n):
        player = state.players[(starting_player_idx + offset) % n]
        for card in list(player.hand):
            action = card.printing.opening_hand_action
            if action is None or card not in player.hand:
                continue
            if action.timing != AFTER_KEEP:
                continue
            if not action.available_to(player.id, starting_player_id):
                continue
            prompt = action.describe(card.printing.name)
            if player.agent.choose_yes_no(state, player.id, prompt):
                action.apply(state, player, card)


def apply_mulligan_window_actions(state: "GameState", player) -> None:
    """Offer the player's mulligan-window actions ("Any time you could
    mulligan and this card is in your hand ...", Serum Powder) before
    their keep-or-mulligan decision. After one is used the player could
    still mulligan, so the new hand is offered again; the loop ends when
    the player declines or no such card is in hand (each use exiles the
    card, so it terminates)."""
    while True:
        card = next(
            (
                c for c in player.hand
                if c.printing.opening_hand_action is not None
                and c.printing.opening_hand_action.timing == MULLIGAN_WINDOW
            ),
            None,
        )
        if card is None:
            return
        action = card.printing.opening_hand_action
        prompt = action.describe(card.printing.name)
        if not player.agent.choose_yes_no(state, player.id, prompt):
            return
        action.apply(state, player, card)


__all__ = [
    "AFTER_KEEP",
    "BeginGameOnBattlefield",
    "ExchangeHandWhileMulliganing",
    "MULLIGAN_WINDOW",
    "RevealFromOpeningHand",
    "apply_mulligan_window_actions",
    "apply_opening_hand_actions",
]
