"""Put-permanents-onto-the-battlefield effects with a payment rider:
Flash ("sacrifice it unless you pay its mana cost reduced by {2}") and
Transmute Artifact (sacrifice, tutor, pay the mana-value difference)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.generic.roto_search import (
    library_search,
    searched_card_taken,
)
from mtgz_ir.effects.base import Effect

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.game import GameState


def pay_mana_during_resolution(
    state: "GameState", player_id: int, cost: "ManaCost", prompt: str,
) -> bool:
    """Offer to pay ``cost`` mid-resolution ("unless you pay ..." / "you
    may pay ..."). Returns True iff paid. Asks ``choose_yes_no`` only
    when the cost is affordable; pays with pool / sac-for-mana / lands
    like a spell cost (Phyrexian pips settled first)."""
    from mtgz_ir.actions import (
        _activate_sac_for_mana_for_cost,
        _settle_phyrexian_pips,
        auto_tap_for_cost,
        can_pay_cost,
    )

    if not can_pay_cost(state, player_id, cost, sac_for_mana=True):
        return False
    p = state.player_by_id(player_id)
    if p.agent is not None and not p.agent.choose_yes_no(state, player_id, prompt):
        return False
    kinds = frozenset({"spell"})
    if cost.phyrexian:
        cost = _settle_phyrexian_pips(state, player_id, prompt, cost, kinds)
    cost = _activate_sac_for_mana_for_cost(state, player_id, cost, kinds)
    auto_tap_for_cost(state, player_id, cost, spell_kind=kinds)
    return True


@dataclass(frozen=True)
class PutCreatureFromHandSacrificeUnlessPay(Effect):
    """Flash: "You may put a creature card from your hand onto the
    battlefield. If you do, sacrifice it unless you pay its mana cost
    reduced by {N}."

    The creature is picked with ``choose_card_from_library`` (the
    zone-agnostic pick hook the other put-from-hand effects use;
    ``None`` declines). Its ETB triggers go on the stack as usual; the
    pay-or-sacrifice happens while this spell is still resolving, so a
    creature that isn't paid for is gone before those triggers resolve.
    """

    reduction: int = 2

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import ManaCost
        from mtgz_ir.turn import sacrifice

        p = state.player_by_id(controller_id)
        options = [c for c in p.hand if c.printing.is_creature]
        if not options or p.agent is None:
            return
        picked = p.agent.choose_card_from_library(
            state, controller_id, [c.id for c in options],
        )
        card = next((c for c in options if c.id == picked), None)
        if card is None:
            state.log.append(f"P{controller_id} puts nothing onto the battlefield")
            return
        p.hand.remove(card)
        put_into_battlefield(state, card, controller_id)
        mc = card.printing.mana_cost
        cost = ManaCost(
            generic=mc.generic, colored=mc.colored, hybrid=mc.hybrid,
            phyrexian=mc.phyrexian,
        ).reduced(self.reduction)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the battlefield"
        )
        if card not in state.battlefield:  # pragma: no cover - defensive
            return
        if pay_mana_during_resolution(
            state, controller_id, cost,
            f"Pay for {card.printing.name} to keep it?",
        ):
            state.log.append(f"P{controller_id} pays to keep {card.printing.name}")
            return
        sacrifice(state, card)
        state.log.append(f"P{controller_id} sacrifices {card.printing.name}")

    def describe(self) -> str:
        return (
            f"you may put a creature card from your hand onto the "
            f"battlefield; sacrifice it unless you pay its mana cost "
            f"reduced by {{{self.reduction}}}"
        )


@dataclass(frozen=True)
class SacrificeArtifactTutorArtifactPayDifference(Effect):
    """Transmute Artifact: sacrifice an artifact; if you do, search for
    an artifact card; it enters if its mana value is at most the
    sacrificed one's, or if you pay {X} (X = the difference); otherwise
    it goes to the graveyard. Then shuffle.

    The sacrifice is chosen with ``choose_sacrifice``, the tutor with
    ``choose_card_from_library`` (a filtered search may fail), the
    optional payment with ``choose_yes_no``. Only card in the pool with
    this shape.
    """

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import is_artifact_now
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import ManaCost
        from mtgz_ir.game import Zone, reveal_to_all, shuffle_library
        from mtgz_ir.turn import sacrifice

        p = state.player_by_id(controller_id)
        artifacts = [
            o for o in state.battlefield
            if o.controller_id == controller_id and is_artifact_now(state, o)
        ]
        if not artifacts:
            state.log.append(f"P{controller_id} has no artifact to sacrifice")
            return
        victim = artifacts[0]
        if p.agent is not None:
            vid = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in artifacts],
            )
            victim = next((o for o in artifacts if o.id == vid), artifacts[0])
        sacrificed_mv = victim.printing.mana_cost.total
        sacrifice(state, victim)
        options = [c for c in p.library if is_artifact_now(state, c)]
        found = None
        if options and p.agent is not None:
            picked = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            found = next((c for c in options if c.id == picked), None)
        if found is not None:
            p.library.remove(found)
            if searched_card_taken(state, controller_id, found):
                found = None
        if found is not None:
            reveal_to_all(state, found)
            diff = found.printing.mana_cost.total - sacrificed_mv
            enters = diff <= 0 or pay_mana_during_resolution(
                state, controller_id, ManaCost(generic=diff),
                f"Pay {{{diff}}} to put {found.printing.name} onto the battlefield?",
            )
            if enters:
                put_into_battlefield(state, found, controller_id)
                state.log.append(
                    f"{found.printing.name} enters the battlefield (transmute)"
                )
            else:
                found.zone = Zone.GRAVEYARD
                state.player_by_id(found.owner_id).graveyard.append(found)
                state.log.append(f"{found.printing.name} goes to the graveyard")
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            "sacrifice an artifact; search for an artifact card; it enters "
            "if its mana value is no greater or you pay the difference"
        )


@dataclass(frozen=True)
class PutCreatureFromHandHasteSacrificeAtNextEndStep(Effect):
    """Through the Breach: "You may put a creature card from your hand
    onto the battlefield. That creature gains haste. Sacrifice that
    creature at the beginning of the next end step." (Pick with
    ``choose_card_from_library``; ``None`` declines.)"""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.delayed_actions import (
            SacrificeSourceDelayed,
            schedule_at_next_end_step,
        )
        from mtgz_ir.effects.generic.reanimation import _grant_haste

        p = state.player_by_id(controller_id)
        options = [c for c in p.hand if c.printing.is_creature]
        if not options or p.agent is None:
            return
        picked = p.agent.choose_card_from_library(
            state, controller_id, [c.id for c in options],
        )
        card = next((c for c in options if c.id == picked), None)
        if card is None:
            return
        p.hand.remove(card)
        put_into_battlefield(state, card, controller_id)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the battlefield"
        )
        if card not in state.battlefield:  # pragma: no cover - defensive
            return
        _grant_haste(state, controller_id, card, until_eot=False)
        schedule_at_next_end_step(
            state, controller_id, card.id, SacrificeSourceDelayed(),
        )

    def describe(self) -> str:
        return (
            "you may put a creature card from your hand onto the "
            "battlefield; it gains haste; sacrifice it at the beginning of "
            "the next end step"
        )
