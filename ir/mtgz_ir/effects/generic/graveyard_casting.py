"""Graveyard-casting grants: the Flashback card's granted flashback and
Yawgmoth's Will's play-from-graveyard permission + graveyard-to-exile
replacement. The casting side lives in ``mtgz_ir.alt_casting`` /
``legal.py`` / ``actions._cast_spell``; the replacement sweep in
``turn.apply_graveyard_exile_replacement``."""

from __future__ import annotations

from dataclasses import dataclass

from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.generic.card_flow import CardFilter


@dataclass(frozen=True)
class GrantFlashbackToTargetCardUEOT(Effect):
    """"Target <filter> card in your graveyard gains flashback until end
    of turn. The flashback cost is equal to its mana cost." (Flashback.)

    Sets ``flashback_granted_until_eot`` on the target card, which
    ``alt_casting.flashback_cost`` reads (cost = the card's mana cost).
    """

    card_filter: CardFilter = CardFilter(any_types=("Instant", "Sorcery"))

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
        card = next(
            (c for c in p.graveyard
             if c.id == tid and self.card_filter.matches_card(state, c)),
            None,
        )
        if card is None:
            state.log.append("flashback grant target is gone")
            return
        if isinstance(p.graveyard, _CowList):
            p.graveyard.fork()
            card = next(c for c in p.graveyard if c.id == tid)
        card.flashback_granted_until_eot = True
        state.log.append(f"{card.printing.name} gains flashback until end of turn")

    def describe(self) -> str:
        return (
            f"target {self.card_filter.describe()} in your graveyard gains "
            f"flashback until end of turn (cost = its mana cost)"
        )


@dataclass(frozen=True)
class MayPlayFromGraveyardThisTurn(Effect):
    """"Until end of turn, you may play lands and cast spells from your
    graveyard." (Yawgmoth's Will.)"""

    def resolve(self, state, controller_id, targets=()):
        state.player_by_id(controller_id).may_play_from_graveyard_this_turn = True
        state.log.append(
            f"P{controller_id} may play lands and cast spells from their "
            f"graveyard this turn"
        )

    def describe(self) -> str:
        return (
            "until end of turn, you may play lands and cast spells from "
            "your graveyard"
        )


@dataclass(frozen=True)
class ExileInsteadOfGraveyardThisTurn(Effect):
    """"If a card would be put into your graveyard from anywhere this
    turn, exile that card instead." (Yawgmoth's Will.)

    Cards already in the graveyard stay (they're recorded in
    ``Player.graveyard_exile_snapshot``); the spell resolving this
    effect is itself exiled as it leaves the stack.
    """

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        if not p.graveyard_exile_replacement_this_turn:
            p.graveyard_exile_snapshot = frozenset(c.id for c in p.graveyard)
        p.graveyard_exile_replacement_this_turn = True
        state.log.append(
            f"cards put into P{controller_id}'s graveyard this turn are "
            f"exiled instead"
        )

    def describe(self) -> str:
        return (
            "if a card would be put into your graveyard from anywhere this "
            "turn, exile that card instead"
        )
