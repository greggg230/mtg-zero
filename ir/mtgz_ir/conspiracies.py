"""Conspiracy cards (CR 313) in the command zone -- roto package P6.

A conspiracy is never a permanent and is never cast: it starts the game
face up in the command zone and stays there (CR 313.2), and its static
abilities function from there. In a Conspiracy Draft game each player
may put conspiracy cards from their card pool into the command zone
before the game begins (CR 905). The engine's decklist is that card pool
as brought to the game, so a conspiracy named in a decklist is one the
player puts into the command zone: :func:`begin_game_with_conspiracies`
(called once by ``simulate.setup_game`` right after the libraries are
built, before the starting player is used, the libraries are shuffled
or any opening hand is drawn) moves it out of the library onto
``Player.conspiracies`` and applies the pregame rules the conspiracies
carry (``CardPrinting.conspiracy_rules``):

* :class:`YouAreTheStartingPlayer` -- Power Play: "You are the starting
  player. If multiple players would be the starting player, one of those
  players is chosen at random." Overrides the caller's starting seat
  (``setup_game(starting_player_idx=...)``, the coin flip of CR 103.1);
  several claimants are resolved with the setup RNG, which is consumed
  only in that case so every other game's shuffles are unchanged.
* :class:`MinimumDeckSizeChange` -- Advantageous Proclamation: "Your
  minimum deck size is reduced by five." Moves the player's minimum
  deck size (CR 100.2a: 40 in limited play). The engine checks deck size
  only when the caller passes ``setup_game(minimum_deck_size=...)`` --
  then a deck below its owner's adjusted minimum is rejected loudly.
  Conspiracy cards aren't part of the deck, so they don't count toward
  it.
* :class:`OpponentRevealsHandAfterMulligans` -- Meddling Wizard (a custom
  conspiracy of the Vintage Roto group, record hand-authored in
  ``data/cube_roto_custom.json``): "At the beginning of each game, after
  mulligans, your opponent reveals their hand to you." Applied by
  :func:`reveal_hands_after_mulligans`, ``simulate.setup_game``'s second
  conspiracy hook, once every player has kept and bottomed.

Conspiracies with other abilities (hidden agenda, draft-matters text)
have no rule class here; the parser leaves them unclaimed.
"""

from __future__ import annotations

import random
from dataclasses import dataclass
from typing import TYPE_CHECKING, Optional

if TYPE_CHECKING:
    from mtgz_ir.game import GameState, Player

# CR 100.2a: "In limited play ... each deck must contain at least forty
# cards." Conspiracy Draft is a limited format.
LIMITED_MINIMUM_DECK_SIZE = 40


@dataclass(frozen=True)
class YouAreTheStartingPlayer:
    """"You are the starting player. If multiple players would be the
    starting player, one of those players is chosen at random." """

    def describe(self) -> str:
        return "you are the starting player"


@dataclass(frozen=True)
class MinimumDeckSizeChange:
    """"Your minimum deck size is reduced/increased by N." ``delta`` is
    signed (-5 for "reduced by five")."""

    delta: int

    def describe(self) -> str:
        verb = "reduced" if self.delta < 0 else "increased"
        return f"your minimum deck size is {verb} by {abs(self.delta)}"


@dataclass(frozen=True)
class OpponentRevealsHandAfterMulligans:
    """"At the beginning of each game, after mulligans, your opponent
    reveals their hand to you." """

    def describe(self) -> str:
        return "after mulligans, your opponent reveals their hand to you"


def _rules(player: "Player", kind: type) -> list:
    return [
        rule
        for card in player.conspiracies
        for rule in card.printing.conspiracy_rules
        if isinstance(rule, kind)
    ]


def put_conspiracies_into_command_zone(state: "GameState") -> None:
    """Move every conspiracy card in each player's library (the freshly
    built decklist) onto ``Player.conspiracies``, face up in the command
    zone (CR 313.2, 905)."""
    from mtgz_ir.game import Zone

    for player in state.players:
        moved = [c for c in player.library if c.printing.is_conspiracy]
        if not moved:
            continue
        for card in moved:
            player.library.remove(card)
            card.zone = Zone.COMMAND
            state.log.append(
                f"P{player.id} starts the game with {card.printing.name} "
                f"face up in the command zone"
            )
        player.conspiracies = player.conspiracies + tuple(moved)


def minimum_deck_size(player: "Player", base: int) -> int:
    """``base`` (the format's minimum, CR 100.2a) moved by every
    ``MinimumDeckSizeChange`` among the player's conspiracies."""
    total = base + sum(rule.delta for rule in _rules(player, MinimumDeckSizeChange))
    return max(total, 0)


def check_minimum_deck_sizes(state: "GameState", base: int) -> None:
    """Raise ``ValueError`` for a player whose deck (library, conspiracies
    excluded) is smaller than their minimum deck size."""
    for player in state.players:
        need = minimum_deck_size(player, base)
        have = len(player.library)
        if have < need:
            raise ValueError(
                f"deck for player {player.id} has {have} cards; its "
                f"minimum deck size is {need}"
            )


def choose_starting_player(
    state: "GameState", default_idx: int, rng: random.Random,
) -> int:
    """The starting player's index: ``default_idx`` unless some players'
    conspiracies say "You are the starting player" -- then that player,
    or one of them chosen at random."""
    claimants = [
        idx for idx, player in enumerate(state.players)
        if _rules(player, YouAreTheStartingPlayer)
    ]
    if not claimants:
        return default_idx
    idx = claimants[0] if len(claimants) == 1 else rng.choice(claimants)
    state.log.append(
        f"P{state.players[idx].id} is the starting player (conspiracy)"
    )
    return idx


def begin_game_with_conspiracies(
    state: "GameState",
    default_starting_idx: int,
    rng: random.Random,
    minimum_deck_size_base: Optional[int] = None,
) -> int:
    """``simulate.setup_game``'s single conspiracy hook. Puts the
    conspiracies into the command zone, enforces the minimum deck size
    when one is in force, and returns the starting player's index."""
    put_conspiracies_into_command_zone(state)
    if minimum_deck_size_base is not None:
        check_minimum_deck_sizes(state, minimum_deck_size_base)
    return choose_starting_player(state, default_starting_idx, rng)


def reveal_hands_after_mulligans(
    state: "GameState", starting_player_idx: int,
) -> None:
    """``simulate.setup_game``'s after-mulligans conspiracy hook, called
    once: after every player has kept their opening hand and put their
    mulligan cards on the bottom (the end of the mulligan process, CR
    103.5) and before the opening-hand actions (CR 103.6). Players go in
    turn order from the starting player; for each
    :class:`OpponentRevealsHandAfterMulligans` among a player's
    conspiracies, each opponent still in the game reveals their hand to
    that player.

    The reveal is recorded as per-card knowledge (``game.reveal_to``):
    every card in the revealed hand is known to the conspiracy's
    controller. That is what the search's hidden-information handling
    reads -- the determinizer never re-samples a card its viewer knows,
    and the ISMCTS information-set key folds it in by identity
    (``search.determinize.viewer_knows``). The knowledge then follows
    the cards like every other private reveal: kept while a card stays
    in that hand, moot in a public zone, dropped when the card is
    shuffled away (``game.shuffle_library``) or put face down into the
    library from the hand (``game.hide_hand_moved_into_library``); a
    card drawn later was never revealed."""
    from mtgz_ir.game import reveal_to

    n = len(state.players)
    for offset in range(n):
        player = state.players[(starting_player_idx + offset) % n]
        if player.has_lost:
            continue
        for card in player.conspiracies:
            if not any(
                isinstance(rule, OpponentRevealsHandAfterMulligans)
                for rule in card.printing.conspiracy_rules
            ):
                continue
            for opponent in state.players:
                if opponent is player or opponent.has_lost:
                    continue
                for revealed in opponent.hand:
                    reveal_to(revealed, player.id)
                names = ", ".join(
                    c.printing.name for c in opponent.hand
                ) or "no cards"
                state.log.append(
                    f"P{opponent.id} reveals their hand to P{player.id} "
                    f"({card.printing.name}): {names}"
                )


__all__ = [
    "LIMITED_MINIMUM_DECK_SIZE",
    "MinimumDeckSizeChange",
    "OpponentRevealsHandAfterMulligans",
    "YouAreTheStartingPlayer",
    "begin_game_with_conspiracies",
    "check_minimum_deck_sizes",
    "choose_starting_player",
    "minimum_deck_size",
    "put_conspiracies_into_command_zone",
    "reveal_hands_after_mulligans",
]
