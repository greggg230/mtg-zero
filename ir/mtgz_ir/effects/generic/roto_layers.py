"""Effects and rules for the roto cards that change card types and colors
(CR 613 layers 4 and 5): Mycosynth Lattice, Liquimetal Coating, Unable to
Scream and Karn, the Great Creator.

The layered reads themselves live in :mod:`mtgz_ir.game`
(``effective_card_types`` / ``has_card_type`` / ``is_artifact_now`` /
``effective_colors``); Lattice's statics are printing fields synthesized
there, and its mana rule is ``actions.mana_spendable_as_any_color``.
This module holds the one-shot effects that write layer-4 / layer-7
registry entries and the two hand / exile fetches. Design:
``docs/EFFECTIVE_CHARACTERISTICS_DESIGN.md``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.durations import Duration, EndOfTurn

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


def _mana_value(obj: "GameObject") -> int:
    """A permanent's mana value (CR 202.3: X is 0 off the stack)."""
    return obj.printing.mana_cost.total


@dataclass(frozen=True)
class TargetBecomesTypes(Effect):
    """"<Up to one >target <np> becomes a[n] <types>[ with power and
    toughness each equal to its mana value][ in addition to its other
    types] <duration>." -- Liquimetal Coating ("Target permanent becomes
    an artifact in addition to its other types until end of turn."),
    Karn, the Great Creator's +1 ("Until your next turn, up to one target
    noncreature artifact becomes an artifact creature with power and
    toughness each equal to its mana value.").

    Writes layer-4 ``add_type`` entries (read by
    ``game.effective_card_types`` / ``is_creature_now``'s cache) and, with
    ``pt_equal_to_mana_value``, a layer-7b ``set_base_pt`` entry -- all
    through ``ApplyEffect(BecomeType(...), duration)``, so "until end of
    turn" ends at cleanup and "until your next turn" at the controller's
    next untap step. "Becomes an artifact creature" keeps the target's
    other card types and subtypes (CR 205.1b). The P/T is the mana value
    as the ability resolves (CR 608.2h). A noncreature permanent that
    entered this turn becomes a creature that hasn't been under its
    controller's control since the turn began (CR 302.6), so it is
    summoning-sick. The target is re-checked on resolution (CR 608.2b).
    """

    targets: tuple = ()
    types_to_add: tuple[str, ...] = ()
    pt_equal_to_mana_value: bool = False
    duration: Duration = EndOfTurn()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.generic.become_type import BecomeType
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        if not targets:
            state.log.append("no target chosen -- nothing becomes anything")
            return
        chosen = targets[0]
        if not _slot_target_still_legal(state, self.targets[0], chosen):
            state.log.append(_fizzle_message(state, chosen, "type change"))
            return
        from mtgz_ir.game import is_creature_now

        obj = next(o for o in state.battlefield if o.id == chosen[1])
        was_creature = is_creature_now(obj)
        mv = _mana_value(obj)
        ApplyEffect(
            inner=BecomeType(
                types_to_add=tuple(t.lower() for t in self.types_to_add),
                base_pt=(mv, mv) if self.pt_equal_to_mana_value else None,
            ),
            ends_at=self.duration,
        ).resolve(state, controller_id, (("permanent", obj.id),))
        if (
            "creature" in (t.lower() for t in self.types_to_add)
            and not was_creature
            and obj.entered_battlefield_turn == state.turn_number
        ):
            obj.summoning_sick = True

    def describe(self) -> str:
        types = " ".join(t.lower() for t in self.types_to_add)
        pt = (
            " with power and toughness each equal to its mana value"
            if self.pt_equal_to_mana_value else ""
        )
        return (
            f"target becomes {types}{pt} in addition to its other types "
            f"{self.duration.describe()}"
        )


@dataclass(frozen=True)
class PutCardFromOutsideTheGameOrFaceUpExileIntoHand(Effect):
    """"You may reveal a <filter> card you own from outside the game or
    choose a face-up <filter> card you own in exile. Put that card into
    your hand." (Karn, the Great Creator's -2.)

    Outside the game is the player's sideboard (``Player.sideboard``; the
    user's decision of 2026-10-02 -- in a roto draft, the drafter's unused
    picks; P4's ``take_from_outside_the_game``). In exile only face-up
    cards the controller owns qualify (``GameObject.exiled_face_down`` --
    Necropotence's and Memory Jar's cards don't); their card type is read
    through ``CardFilter.matches_card`` (printed, in exile). One
    ``choose_mode`` decision among "reveal <name>" (one entry per distinct
    sideboard name), "take <name> from exile" (one per exiled card) and
    "nothing" ("You may"). The exiled card leaves exile as a new object
    (CR 400.7: its counters go; it is revealed -- it was face up)."""

    card_filter: object = None

    def _sideboard_options(self, player) -> dict:
        by_name: dict = {}
        for printing in player.sideboard:
            if self.card_filter is None or self.card_filter.matches(printing):
                by_name.setdefault(printing.name, printing)
        return by_name

    def _exile_options(self, state: "GameState", controller_id: int) -> list:
        out = []
        for pl in state.players:
            for card in pl.exile:
                if card.owner_id != controller_id or card.exiled_face_down:
                    continue
                if card.printing.is_token or getattr(card, "is_spell_copy", False):
                    continue
                if (
                    self.card_filter is None
                    or self.card_filter.matches_card(state, card)
                ):
                    out.append(card)
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all
        from mtgz_ir.roto_turns import take_from_outside_the_game

        p = state.player_by_id(controller_id)
        outside = self._sideboard_options(p)
        exiled = self._exile_options(state, controller_id)
        names = sorted(outside)
        options = (
            [f"reveal {n} from outside the game" for n in names]
            + [f"take {c.printing.name} from exile" for c in exiled]
            + ["nothing"]
        )
        if len(options) == 1 or p.agent is None:
            state.log.append(
                f"P{controller_id} puts nothing into their hand "
                "(no artifact card outside the game or face up in exile)"
                if len(options) == 1 else
                f"P{controller_id} puts nothing into their hand"
            )
            return
        pick = p.agent.choose_mode(state, controller_id, options)
        if not isinstance(pick, int) or not 0 <= pick < len(options) - 1:
            state.log.append(f"P{controller_id} puts nothing into their hand")
            return
        if pick < len(names):
            take_from_outside_the_game(state, controller_id, outside[names[pick]])
            return
        card = exiled[pick - len(names)]
        owner = state.player_by_id(card.owner_id)
        owner.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        from mtgz_ir.turn import _reset_left_battlefield_object

        _reset_left_battlefield_object(card)
        card.silver_counters = 0
        card.exile_play_permission = None
        card.exiled_by_source = None
        card.zone = Zone.HAND
        owner.hand.append(card)
        reveal_to_all(state, card)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} from exile into "
            "their hand"
        )

    def describe(self) -> str:
        what = (
            self.card_filter.describe() if self.card_filter is not None
            else "card"
        )
        return (
            f"you may reveal a {what} you own from outside the game or "
            f"choose a face-up {what} you own in exile; put that card into "
            "your hand"
        )


# ---------------------------------------------------------------------------
# "As long as enchanted creature is face down, it can't be turned face up."
# ---------------------------------------------------------------------------


def turning_face_up_prevented(state: "GameState", obj: "GameObject") -> bool:
    """Whether an Aura attached to face-down ``obj`` says it can't be
    turned face up (Unable to Scream). An Aura that has lost its
    abilities (``roto_board_statics.abilities_suppressed``) or is phased
    out doesn't."""
    if not obj.face_down:
        return False
    from mtgz_ir.roto_board_statics import abilities_suppressed

    for aura in state.battlefield:
        if aura.attached_to != obj.id or aura.phased_out:
            continue
        if not aura.printing.attached_prevents_turning_face_up:
            continue
        if not abilities_suppressed(state, aura):
            return True
    return False


__all__ = [
    "PutCardFromOutsideTheGameOrFaceUpExileIntoHand",
    "TargetBecomesTypes",
    "turning_face_up_prevented",
]
