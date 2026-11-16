"""Reanimation and put-into-library effects for the cube spells-B bucket.

* :class:`ReturnCardFromGraveyardToBattlefield` — Reanimate ("target
  creature card from a graveyard ... under your control. You lose life
  equal to that card's mana value."), Shallow Grave / Corpse Dance ("the
  top creature card of your graveyard ... That creature gains haste until
  end of turn. Exile it at the beginning of the next end step."), Goryo's
  Vengeance's reanimation half.
* :class:`EachPlayerPutsCardFromGraveyardOntoBattlefield` — Exhume.
* :class:`PutTargetPermanentIntoOwnersLibrary` — Oust ("second from the
  top ... Its controller gains 3 life") and Unexpectedly Absent ("just
  beneath the top X cards").

Riders that refer back to the moved card ("that creature", "it", "that
card's mana value") are fields of the same effect, because they read the
object the effect itself just moved.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import HasteRider, LibraryPosition, ReanimateSource, coerce
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.generic.card_flow import CardFilter

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


REANIMATE_SOURCES = ("target_any_graveyard", "target_your_graveyard", "top_of_your_graveyard")
HASTE_RIDERS = (None, "until_end_of_turn", "permanent")


def _reset_for_battlefield(card: "GameObject") -> None:
    """A card entering from the graveyard is a new object (CR 400.7):
    drop counters / damage left over from a previous battlefield stint."""
    card.named_counters = ()
    card.exile_instead_of_leaving = False
    card.plus_one_counters = 0
    card.minus_one_counters = 0
    card.charge_counters = 0
    card.stun_counters = 0
    card.damage = 0
    card.temp_power_bonus = 0
    card.temp_toughness_bonus = 0


def _grant_haste(state: "GameState", controller_id: int, obj, until_eot: bool) -> None:
    from mtgz_ir.effects.combinators import ApplyEffect
    from mtgz_ir.effects.durations import EndOfTurn, Permanent
    from mtgz_ir.effects.generic.grant_keywords import GrantKeywords

    ApplyEffect(
        inner=GrantKeywords(keywords=frozenset({"haste"})),
        ends_at=EndOfTurn() if until_eot else Permanent(),
    ).resolve(state, controller_id, (("creature", obj.id),))


def reanimate_card(
    state: "GameState", card: "GameObject", controller_id: int,
) -> None:
    """Move ``card`` from its owner's graveyard onto the battlefield under
    ``controller_id``'s control (ETBs fire via ``put_into_battlefield``)."""
    from mtgz_ir.actions import put_into_battlefield

    owner = state.player_by_id(card.owner_id)
    owner.graveyard.remove(card)
    _reset_for_battlefield(card)
    saved_x = state.resolving_x_value
    state.resolving_x_value = 0
    try:
        put_into_battlefield(state, card, controller_id)
    finally:
        state.resolving_x_value = saved_x
    state.log.append(
        f"{card.printing.name} returns to the battlefield under "
        f"P{controller_id}'s control"
    )


@dataclass(frozen=True)
class ReturnCardFromGraveyardToBattlefield(Effect):
    """Put a ``card_filter`` card from a graveyard onto the battlefield
    under your control, with the printed riders.

    ``source``:
      * ``"target_any_graveyard"`` — "target creature card from a
        graveyard" (Reanimate); target kind ``graveyard_card``.
      * ``"target_your_graveyard"`` — "target ... card from your
        graveyard" (Goryo's Vengeance).
      * ``"top_of_your_graveyard"`` — "the top creature card of your
        graveyard" (Shallow Grave; untargeted — the most recently put
        matching card).

    Riders: ``haste`` (``"until_end_of_turn"`` / ``"permanent"``),
    ``exile_at_next_end_step`` (a delayed trigger on the next end step,
    whoever's turn), ``lose_life_equal_mv`` (Reanimate).
    """

    card_filter: CardFilter = CardFilter(any_types=("Creature",))
    source: ReanimateSource = ReanimateSource.TARGET_ANY_GRAVEYARD
    haste: "HasteRider | None" = None
    exile_at_next_end_step: bool = False
    lose_life_equal_mv: bool = False
    # "... with a <name> counter on it" (From the Catacombs: corpse).
    with_counter: "str | None" = None
    # "If that creature would leave the battlefield, exile it instead of
    # putting it anywhere else."
    exile_if_it_would_leave: bool = False
    # "return up to one target ... card" (Metamorphosis Fanatic): the
    # target is optional.
    up_to_one: bool = False

    def __post_init__(self) -> None:
        if self.source not in REANIMATE_SOURCES:
            raise ValueError(f"unknown reanimate source {self.source!r}")
        if self.haste not in HASTE_RIDERS:
            raise ValueError(f"unknown haste rider {self.haste!r}")
        object.__setattr__(self, "source", ReanimateSource(self.source))
        object.__setattr__(self, "haste", coerce(HasteRider, self.haste))

    def requires_target(self) -> bool:
        return self.source != "top_of_your_graveyard" and not self.up_to_one

    def target_count_range(self) -> tuple[int, int]:
        if self.source == "top_of_your_graveyard":
            return (0, 0)
        return (0 if self.up_to_one else 1, 1)

    def _candidates(self, state: "GameState", controller_id: int) -> list:
        if self.source == "target_any_graveyard":
            pool = [c for p in state.players for c in p.graveyard]
        else:
            pool = list(state.player_by_id(controller_id).graveyard)
        return [c for c in pool if self.card_filter.matches_card(state, c)]

    def legal_targets(self, state, controller_id):
        if self.source == "top_of_your_graveyard":
            return []
        return [("graveyard_card", c.id) for c in self._candidates(state, controller_id)]

    def _pick(self, state, controller_id, targets):
        candidates = self._candidates(state, controller_id)
        if self.source == "top_of_your_graveyard":
            return candidates[-1] if candidates else None
        if not targets:
            return None
        _, tid = targets[0]
        # Still in a (legal) graveyard at resolution, else it fizzles.
        return next((c for c in candidates if c.id == tid), None)

    def resolve(self, state, controller_id, targets=()):
        card = self._pick(state, controller_id, targets)
        if card is None:
            state.log.append("reanimation finds no legal card")
            return
        mv = card.printing.mana_cost.total
        reanimate_card(state, card, controller_id)
        if card not in state.battlefield:  # pragma: no cover - defensive
            return
        if self.with_counter is not None:
            from mtgz_ir.game import named_counter_count, set_named_counters

            set_named_counters(
                card, self.with_counter,
                named_counter_count(card, self.with_counter) + 1,
            )
        if self.exile_if_it_would_leave:
            card.exile_instead_of_leaving = True
        if self.haste is not None:
            _grant_haste(
                state, controller_id, card,
                until_eot=self.haste == "until_end_of_turn",
            )
        if self.exile_at_next_end_step:
            from mtgz_ir.effects.generic.delayed_actions import (
                ExileSourceDelayed,
                schedule_at_next_end_step,
            )

            schedule_at_next_end_step(
                state, controller_id, card.id, ExileSourceDelayed(),
            )
        if self.lose_life_equal_mv and mv:
            p = state.player_by_id(controller_id)
            p.life -= mv
            state.log.append(f"P{controller_id} loses {mv} life (life: {p.life})")

    def describe(self) -> str:
        where = {
            "target_any_graveyard": f"target {self.card_filter.describe()} from a graveyard",
            "target_your_graveyard": f"target {self.card_filter.describe()} from your graveyard",
            "top_of_your_graveyard": f"the top {self.card_filter.describe()} of your graveyard",
        }[self.source]
        text = f"put {where} onto the battlefield under your control"
        if self.haste:
            text += "; it gains haste"
        if self.exile_at_next_end_step:
            text += "; exile it at the beginning of the next end step"
        if self.lose_life_equal_mv:
            text += "; you lose life equal to its mana value"
        return text


@dataclass(frozen=True)
class EachPlayerPutsCardFromGraveyardOntoBattlefield(Effect):
    """"Each player puts a creature card from their graveyard onto the
    battlefield." (Exhume.) Each player picks their own card via
    ``choose_card_from_library`` (the "pick a card from a hidden-ish
    list" hook) in APNAP order; the cards enter simultaneously after all
    choices are made."""

    card_filter: CardFilter = CardFilter(any_types=("Creature",))

    def resolve(self, state, controller_id, targets=()):
        chosen: list = []
        for i in range(len(state.players)):
            p = state.players[(state.active_player_idx + i) % len(state.players)]
            if p.has_lost:
                continue
            options = [c for c in p.graveyard if self.card_filter.matches_card(state, c)]
            if not options:
                continue
            pick = options[0]
            if p.agent is not None:
                picked = p.agent.choose_card_from_library(
                    state, p.id, [c.id for c in options],
                )
                pick = next((c for c in options if c.id == picked), options[0])
            chosen.append((p.id, pick))
        for pid, card in chosen:
            reanimate_card(state, card, pid)

    def describe(self) -> str:
        return (
            f"each player puts a {self.card_filter.describe()} from their "
            f"graveyard onto the battlefield"
        )


LIBRARY_POSITIONS = ("second_from_top", "beneath_top_x")


@dataclass(frozen=True)
class PutTargetPermanentIntoOwnersLibrary(Effect):
    """Put target ``card_filter`` permanent into its owner's library at a
    fixed depth.

    ``position``:
      * ``"second_from_top"`` — Oust.
      * ``"beneath_top_x"`` — "just beneath the top X cards of that
        library" (Unexpectedly Absent; X from the spell's ``{X}``).

    ``controller_gains_life`` — "Its controller gains N life." (Oust),
    paid to the permanent's controller as it last existed on the
    battlefield.
    """

    card_filter: CardFilter = CardFilter(any_types=("Creature",))
    position: LibraryPosition = LibraryPosition.SECOND_FROM_TOP
    controller_gains_life: int = 0

    def __post_init__(self) -> None:
        if self.position not in LIBRARY_POSITIONS:
            raise ValueError(f"unknown library position {self.position!r}")
        object.__setattr__(self, "position", LibraryPosition(self.position))

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        from mtgz_ir.alt_casting import permanent_matches_filter
        from mtgz_ir.game import is_creature_now

        out: list[Target] = []
        for o in state.battlefield:
            if o.phased_out or not permanent_matches_filter(state, o, self.card_filter):
                continue
            out.append(("creature" if is_creature_now(o) else "permanent", o.id))
        return out

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.alt_casting import permanent_matches_filter
        from mtgz_ir.game import gain_life
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        _, tid = targets[0]
        obj = next((o for o in state.battlefield if o.id == tid), None)
        if obj is None or not permanent_matches_filter(state, obj, self.card_filter):
            state.log.append("put-into-library target is gone")
            return
        last_controller = obj.controller_id
        index = 1 if self.position == "second_from_top" else max(0, state.resolving_x_value)
        move_off_battlefield(state, obj, destination="library", library_index=index)
        state.log.append(
            f"{obj.printing.name} is put into P{obj.owner_id}'s library "
            f"({index} card(s) above it)"
        )
        if self.controller_gains_life:
            gain_life(state, last_controller, self.controller_gains_life)

    def describe(self) -> str:
        where = (
            "second from the top" if self.position == "second_from_top"
            else "just beneath the top X cards"
        )
        text = f"put target {self.card_filter.describe()} into its owner's library {where}"
        if self.controller_gains_life:
            text += f"; its controller gains {self.controller_gains_life} life"
        return text
