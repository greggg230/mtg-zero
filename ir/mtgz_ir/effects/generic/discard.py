"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import (
    Effect, Target, TriggeredAbility,
    _apply_damage, _multicolored_permanents_count,
    _find_moonlit_replacement_source,
)
from mtgz_ir.effects.generic.targets_helpers import (
    _eff_subtypes,
    _slot_target_still_legal,
    _fizzle_message,
    _describe_targets,
    _describe_filter,
    _bounce_to_hand,
)

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class DiscardSelf(Effect):
    """Controller discards ``amount`` card(s) from their hand. The discarding
    player's agent picks which cards via ``choose_discard``.

    Returns ``True`` if at least one card was actually discarded,
    ``False`` if the hand was empty before any discard could happen.
    The bool signal lets ``IfYouDo``/``Optional`` combinators gate
    follow-up effects on whether the discard accomplished anything
    (e.g. "If you do, draw a card." fizzles on empty hand).
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import fire_opponent_discards_triggers

        p = state.player_by_id(controller_id)
        discarded_any = False
        for _ in range(self.amount):
            if not p.hand:
                break
            hand_ids = [c.id for c in p.hand]
            chosen_id = p.agent.choose_discard(state, p.id, hand_ids)
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(f"P{p.id} discards {card.printing.name}")
            fire_opponent_discards_triggers(state, p.id, card=card)
            discarded_any = True
        return discarded_any

    def describe(self) -> str:
        if self.amount == 1:
            return "you discard a card"
        return f"you discard {self.amount} cards"



@dataclass(frozen=True)
class DiscardHandThenDrawN(Effect):
    """"You may discard your hand. If you do, draw ``amount`` cards."

    Matches Vaultguard Trooper's conditional end-step ability. Acts
    on the CONTROLLER's hand. The "may" is exposed via
    ``agent.choose_yes_no`` — if the controller declines, the hand is
    untouched and nothing is drawn (the dependent "If you do" rider
    does not fire).
    """

    amount: int = 2

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        # Optional "may discard your hand" — controller decides.
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            f"Discard your hand to draw {self.amount}?",
        ):
            state.log.append(
                f"P{controller_id} declines to discard hand "
                f"(DiscardHandThenDrawN)"
            )
            return
        # Discard every card.
        discarded_count = 0
        while p.hand:
            card = p.hand.pop(0)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            discarded_count += 1
        state.log.append(
            f"P{controller_id} discards hand ({discarded_count} cards)"
        )
        # Draw N cards.
        drew = 0
        for _ in range(self.amount):
            if not p.library:
                break
            card = p.library.pop(0)
            card.zone = Zone.HAND
            p.hand.append(card)
            drew += 1
        state.log.append(
            f"P{controller_id} draws {drew} card(s)"
        )

    def describe(self) -> str:
        return f"discard your hand, then draw {self.amount} cards"



@dataclass(frozen=True)
class EachOpponentDiscards(Effect):
    """Each opponent discards ``count`` cards (untargeted; no choice of target).

    Each discarding player chooses their own discard via their agent's
    ``choose_discard`` — the ability's controller doesn't pick for them.
    """

    count: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import fire_opponent_discards_triggers

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            from mtgz_ir.player_protections import (
                opponent_effect_blocked_for,
            )

            for _ in range(self.count):
                if not p.hand or opponent_effect_blocked_for(state, p.id):
                    break
                hand_ids = [c.id for c in p.hand]
                chosen_id = p.agent.choose_discard(state, p.id, hand_ids)
                card = next(c for c in p.hand if c.id == chosen_id)
                p.hand.remove(card)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
                state.log.append(
                    f"P{p.id} discards {card.printing.name}"
                )
                fire_opponent_discards_triggers(state, p.id, card=card)

    def describe(self) -> str:
        if self.count == 1:
            return "each opponent discards a card"
        return f"each opponent discards {self.count} cards"



@dataclass(frozen=True)
class EachOpponentExilesCardFromHandWithCastPermission(Effect):
    """Lightstall Inquisitor ETB: each opponent exiles a card from their
    hand and may play that card for as long as it remains exiled.  Each
    spell cast this way costs {1} more; each land played this way enters
    tapped.

    The exiling player chooses which card to exile via their agent's
    ``choose_discard`` hook (the choice is the same: pick one of your
    hand cards).  The chosen card moves to exile with
    ``cast_from_exile_with_lightstall_surcharge=True``; ``legal.py`` and
    ``actions.py`` honour that flag to grant and enforce the permission +
    surcharge + tapped-land-entry.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            if not p.hand:
                continue
            hand_ids = [c.id for c in p.hand]
            chosen_id = p.agent.choose_discard(state, p.id, hand_ids)
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.EXILE
            card.cast_from_exile_with_lightstall_surcharge = True
            p.exile.append(card)
            state.log.append(
                f"P{p.id} exiles {card.printing.name} "
                f"(Lightstall Inquisitor; may play from exile, costs {{1}} more)"
            )

    def describe(self) -> str:
        return (
            "each opponent exiles a card from their hand and may play "
            "that card for as long as it remains exiled"
        )



@dataclass(frozen=True)
class TargetOpponentDiscardsCasterChoice(Effect):
    """Target opponent reveals their hand. You choose a nonland card
    from it. That player discards the chosen card.

    Matches Temporal Intervention's main effect. Unlike
    TargetOpponentDiscards (opponent picks their own discard), here the
    CASTER picks — a directed discard.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        target = state.player_by_id(target_id)
        if target.has_lost:
            return
        # The reveal step is "Target opponent reveals their hand" — the
        # caster sees every card in target's hand, and remembers it
        # afterward. Mark every card currently in target's hand as
        # publicly known to the caster; whatever stays in hand after
        # the directed discard preserves that mark.
        from mtgz_ir.game import reveal_to
        for c in target.hand:
            reveal_to(c, controller_id)
        # Nonland cards the target could be forced to discard.
        options = [
            c for c in target.hand if not c.printing.is_land
        ]
        if not options:
            state.log.append(
                f"P{target_id} has no nonland cards — directed discard "
                f"fizzles"
            )
            return
        caster = state.player_by_id(controller_id)
        if caster.agent is None:
            chosen = options[0]
        else:
            chosen_id = caster.agent.choose_card_from_opponent_hand(
                state, controller_id, [c.id for c in options]
            )
            chosen = next(
                (c for c in options if c.id == chosen_id), options[0]
            )
        from mtgz_ir.player_protections import opponent_effect_blocked_for

        if opponent_effect_blocked_for(state, target.id):
            return
        target.hand.remove(chosen)
        chosen.zone = Zone.GRAVEYARD
        target.graveyard.append(chosen)
        state.log.append(
            f"P{controller_id} forces P{target_id} to discard "
            f"{chosen.printing.name}"
        )

    def describe(self) -> str:
        return (
            "target opponent reveals their hand; you choose a nonland "
            "card from it and they discard it"
        )



@dataclass(frozen=True)
class TargetOpponentLookHandMayDiscard(Effect):
    """"Look at target opponent's hand. You may choose a nonland card
    from it. That player discards that card."

    Variant of ``TargetOpponentDiscardsCasterChoice`` that honors the
    printed ``You may`` gate via ``Agent.choose_yes_no`` — the caster
    is OFFERED the directed discard rather than forced into it. Used
    by Cracked Skull's ETB. The reveal scope is ``look at`` (caster-
    only) rather than the public ``reveals their hand``, but the
    state-tracking primitive is the same: every card in the hand
    becomes ``publicly_known_to`` the caster (one-way reveal).

    Simplification vs. the printed ``target player`` scope on Cracked
    Skull: targets are restricted to opponents only (the same
    convention the original ``TargetOpponentDiscardsCasterChoice``
    rides). Self-targeting is never strategically correct for the
    controller, so the legality narrowing doesn't affect rational
    play; documented as the standard EoE / DSK target-player
    simplification.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        target = state.player_by_id(target_id)
        if target.has_lost:
            return
        # ``Look at`` step: caster sees every card in target's hand and
        # remembers it. Same one-way reveal primitive
        # ``TargetOpponentDiscardsCasterChoice`` uses for its public
        # ``reveals their hand`` step — engine-equivalent for the
        # discard-direction phase that follows.
        for c in target.hand:
            reveal_to(c, controller_id)
        # Nonland options for the may-discard.
        options = [c for c in target.hand if not c.printing.is_land]
        if not options:
            state.log.append(
                f"P{target_id} has no nonland cards — directed discard "
                f"declines silently"
            )
            return
        caster = state.player_by_id(controller_id)
        # The "may" gate. Default-decline if the caster has no agent;
        # otherwise the agent's ``choose_yes_no`` answers the prompt.
        if caster.agent is None:
            return
        if not caster.agent.choose_yes_no(
            state, controller_id,
            f"Force P{target_id} to discard a nonland card?",
        ):
            state.log.append(
                f"P{controller_id} declines the may-discard against "
                f"P{target_id}"
            )
            return
        chosen_id = caster.agent.choose_card_from_opponent_hand(
            state, controller_id, [c.id for c in options]
        )
        chosen = next(
            (c for c in options if c.id == chosen_id), options[0]
        )
        from mtgz_ir.player_protections import opponent_effect_blocked_for

        if opponent_effect_blocked_for(state, target.id):
            return
        target.hand.remove(chosen)
        chosen.zone = Zone.GRAVEYARD
        target.graveyard.append(chosen)
        state.log.append(
            f"P{controller_id} chooses P{target_id}'s "
            f"{chosen.printing.name} for the may-discard"
        )

    def describe(self) -> str:
        return (
            "look at target opponent's hand; you may choose a nonland "
            "card from it and they discard it"
        )



@dataclass(frozen=True)
class TargetOpponentDiscards(Effect):
    """Target opponent discards ``count`` cards.

    The discarding player (the target) chooses which cards to discard — the
    target player's agent makes the pick via ``choose_discard``.
    """

    count: int = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id)
            for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        p = state.player_by_id(target_id)
        if p.has_lost:
            return
        from mtgz_ir.player_protections import opponent_effect_blocked_for

        for _ in range(self.count):
            if not p.hand or opponent_effect_blocked_for(state, p.id):
                break
            hand_ids = [c.id for c in p.hand]
            chosen_id = p.agent.choose_discard(state, p.id, hand_ids)
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(
                f"P{target_id} discards {card.printing.name}"
            )

    def describe(self) -> str:
        if self.count == 1:
            return "target opponent discards a card"
        return f"target opponent discards {self.count} cards"
