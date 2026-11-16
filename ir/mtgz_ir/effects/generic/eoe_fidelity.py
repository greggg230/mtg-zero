"""Effects for Edge of Eternities printings the parser used to flatten.

mtg-zero (the Rust rules engine that reads the exported card IR) found
these printings contradicting their Oracle text (its
``docs/ir_defects.md``); the shapes here say what the cards print.
Every class is parameterized by the printed shape, not by a card.
Python-engine shortcuts are listed in ``docs/SEARCH_LIMITATIONS.md``
("EoE fidelity shapes (feature/eoe-ir-fixes, second batch)" and "...,
third batch").
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.predicates import (
    OPPONENT,
    YOU,
    TriggerPredicate,
    _Controller,
    _resolve_subject,
)
from mtgz_ir.effects.vocab import GraveyardReturnDestination, ZoneName

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# ---------------------------------------------------------------------------
# "Return [one or two | up to N] target <...> cards from your graveyard to
# your hand [, then ... to the battlefield tapped]"
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GraveyardReturnGroup:
    """One target group of :class:`ReturnTargetCardsFromYourGraveyard`:
    ``min_n``..``max_n`` target cards in the controller's graveyard
    matching ``card_filter`` (a ``CardFilter``), moved to ``destination``
    (``tapped``: onto the battlefield tapped)."""

    card_filter: object = None
    min_n: int = 1
    max_n: int = 1
    destination: GraveyardReturnDestination = GraveyardReturnDestination.HAND
    tapped: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "destination", GraveyardReturnDestination(self.destination),
        )


@dataclass(frozen=True)
class ReturnTargetCardsFromYourGraveyard(Effect):
    """Return target cards from your graveyard, one target group after
    another ("Return one or two target creature and/or planeswalker cards
    from your graveyard to your hand." -- Archenemy's Charm; "Return up
    to two target nonland permanent cards from your graveyard to your
    hand, then return up to two target land cards from your graveyard to
    the battlefield tapped." -- Pull Through the Weft). Each group is its
    own target requirement (CR 115.1, 601.2c).

    Python targets are ``("graveyard_group<i>", id)``; the cast pipeline
    picks from the flat pool (``target_count_range`` sums the groups), so
    a pick over a group's maximum is ignored on resolution (the
    SEARCH_LIMITATIONS entry)."""

    groups: tuple[GraveyardReturnGroup, ...] = ()

    def requires_target(self) -> bool:
        return any(g.min_n > 0 for g in self.groups)

    def target_count_range(self) -> tuple[int, int]:
        return (
            sum(g.min_n for g in self.groups),
            sum(g.max_n for g in self.groups),
        )

    def _matches(self, state, group: GraveyardReturnGroup, card) -> bool:
        return group.card_filter is None or group.card_filter.matches_card(state, card)

    def legal_targets(self, state: "GameState", controller_id: int) -> list[Target]:
        p = state.player_by_id(controller_id)
        out: list[Target] = []
        for i, g in enumerate(self.groups):
            out.extend(
                (f"graveyard_group{i}", c.id) for c in p.graveyard
                if self._matches(state, g, c)
            )
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        for i, g in enumerate(self.groups):
            kind = f"graveyard_group{i}"
            ids: list[int] = []
            for k, tid in targets:
                if k == kind and tid not in ids:
                    ids.append(tid)
            for tid in ids[: g.max_n]:
                card = next((c for c in p.graveyard if c.id == tid), None)
                if card is None or not self._matches(state, g, card):
                    continue
                p.graveyard.remove(card)
                if g.destination == GraveyardReturnDestination.BATTLEFIELD:
                    state.log.append(
                        f"{card.printing.name} returns from P{controller_id}'s "
                        f"graveyard to the battlefield"
                        + (" tapped" if g.tapped else "")
                    )
                    put_into_battlefield(state, card, controller_id)
                    # (``put_into_battlefield`` sets ``tapped`` from the
                    # printing's own "enters tapped"; the effect's "tapped"
                    # is applied after it.)
                    if g.tapped:
                        card.tapped = True
                else:
                    card.zone = Zone.HAND
                    p.hand.append(card)
                    reveal_to_all(state, card)
                    state.log.append(
                        f"{card.printing.name} returns from P{controller_id}'s "
                        f"graveyard to hand"
                    )

    def describe(self) -> str:
        parts = []
        for g in self.groups:
            what = g.card_filter.describe() if g.card_filter is not None else "card"
            where = "the battlefield" if g.destination == "battlefield" else "your hand"
            parts.append(
                f"return {g.min_n}-{g.max_n} target {what} from your graveyard "
                f"to {where}" + (" tapped" if g.tapped else "")
            )
        return ", then ".join(parts)


# ---------------------------------------------------------------------------
# "Return target creature card from your graveyard to the battlefield. It's
# a <subtype> in addition to its other types and has no abilities."
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ReturnTargetCardFromYourGraveyardToBattlefieldAs(Effect):
    """Return target card (``card_filter``) from your graveyard to the
    battlefield; the permanent it becomes has ``added_subtypes`` in
    addition to its other types (layer 4) and, with
    ``loses_all_abilities``, no abilities (layer 6), for as long as it
    stays there (CR 611.2c: an effect of a resolved ability that changes
    an object's characteristics lasts indefinitely; Xu-Ifit,
    Osteoharmonist)."""

    card_filter: object = None
    added_subtypes: tuple[str, ...] = ()
    loses_all_abilities: bool = False

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state: "GameState", controller_id: int) -> list[Target]:
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_card", c.id) for c in p.graveyard
            if self.card_filter is None or self.card_filter.matches_card(state, c)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        if not targets or targets[0][0] != "graveyard_card":
            return
        p = state.player_by_id(controller_id)
        card = next((c for c in p.graveyard if c.id == targets[0][1]), None)
        if card is None:
            return
        p.graveyard.remove(card)
        state.log.append(
            f"{card.printing.name} returns from P{controller_id}'s graveyard "
            f"to the battlefield"
        )
        put_into_battlefield(state, card, controller_id)
        for sub in self.added_subtypes:
            state.add_continuous_effect(
                source_id=card.id, layer=4, kind="add_subtype",
                target_id=card.id, params=(sub,),
                description=f"{card.printing.name} is a {sub}",
            )
        if self.loses_all_abilities:
            state.add_continuous_effect(
                source_id=card.id, layer=6, kind="lose_all_abilities",
                target_id=card.id, params=(),
                description=f"{card.printing.name} has no abilities",
            )

    def describe(self) -> str:
        text = "return target card from your graveyard to the battlefield"
        if self.added_subtypes:
            text += f"; it's a {' '.join(self.added_subtypes)} in addition"
        if self.loses_all_abilities:
            text += " and has no abilities"
        return text


# ---------------------------------------------------------------------------
# "Return each creature card exiled with this <permanent> to the battlefield
# under its owner's control."
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ReturnCardsExiledWithSourceToBattlefield(Effect):
    """Return each card exiled with the source (the CR 607.2a link) that
    matches ``card_filter`` (a ``CardFilter``; ``None`` = every card) to
    the battlefield under its owner's control (Endless Sands: "each
    creature card exiled with this land"). The link survives the source
    leaving (its sacrifice is the ability's cost)."""

    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.artifact_mechanics import imprinted_cards

        source = state.resolving_source
        if source is None:
            return
        for card in imprinted_cards(state, source):
            if self.card_filter is not None and not self.card_filter.matches_card(state, card):
                continue
            owner = state.player_by_id(card.owner_id)
            owner.exile.remove(card)
            card.exiled_face_down = False
            card.imprinted_on = None
            state.log.append(
                f"{card.printing.name} returns to the battlefield under "
                f"P{owner.id}'s control"
            )
            put_into_battlefield(state, card, owner.id)

    def describe(self) -> str:
        what = self.card_filter.describe() if self.card_filter is not None else "card"
        return (
            f"return each {what} exiled with this to the battlefield under "
            f"its owner's control"
        )


# ---------------------------------------------------------------------------
# "dies or is put into exile" (from the battlefield)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class LeftBattlefieldTo(TriggerPredicate):
    """On a ``LEAVES_BATTLEFIELD`` event: the object went to one of
    ``zones`` ("dies or is put into exile": graveyard or exile, CR 700.4;
    Syr Vondam, Sunstar Exemplar). A return to a hand or a library doesn't
    match."""

    zones: tuple[ZoneName, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "zones", tuple(ZoneName(z) for z in self.zones))

    def matches(self, state, subjects, source):
        obj = _resolve_subject(subjects, None)
        if obj is None:
            return False
        zone = getattr(obj, "zone", None)
        name = getattr(zone, "value", zone)
        return any(z.value == name for z in self.zones)


# ---------------------------------------------------------------------------
# "Move a counter from target <A> onto a second target <B>."
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class MoveCounterBetweenTargets(Effect):
    """Move a counter from the first target onto the second ("Move a
    counter from target permanent you control onto a second target
    permanent." -- Nesting Grounds).

    ``targets`` holds two ``TargetSpec`` slots: the object the counter
    comes from, and the object it goes onto (``distinct_from=(0,)``:
    "a second target", CR 115.3). As the ability resolves, its
    controller chooses one counter of any kind on the first target
    (the kind is chosen then, not when the targets are chosen: Nesting
    Grounds' rulings, 2020-04-17); that counter is removed from the
    first target and put on the second (CR 122.5, 122.8). Putting it
    is a "put": replacements such as Loading Zone's apply, and "whenever
    you put counters" triggers see it. If either target is illegal as
    the ability resolves (CR 608.2b), nothing moves; nothing moves
    either when the first target has no counter."""

    targets: tuple = ()

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def legal_targets_for_slot(
        self, state: "GameState", controller_id: int, slot_idx: int,
    ) -> list[Target]:
        from mtgz_ir.keyword_rules import filter_targetable

        pool = self._enumerate_for_filter(
            state, controller_id, self.targets[slot_idx].filter,
        )
        return filter_targetable(
            state, pool, state.resolving_source, controller_id,
        )

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        """First-slot candidates, or ``[]`` when no two different legal
        targets exist (then the ability can't be activated, CR 602.2b /
        601.2c)."""
        first = self.legal_targets_for_slot(state, controller_id, 0)
        second = self.legal_targets_for_slot(state, controller_id, 1)
        for a in first:
            if any(b != a for b in second):
                return first
        return []

    def _still_legal(self, state, controller_id, targets) -> bool:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.keyword_rules import can_be_targeted_by

        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if not _slot_target_still_legal(state, slot, chosen):
                    return False
                obj = next(
                    (o for o in state.battlefield if o.id == chosen[1]), None,
                )
                if obj is None or not can_be_targeted_by(
                    state, obj, state.resolving_source, controller_id,
                ):
                    return False
            return True
        finally:
            state.resolving_controller_id = prior

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_wg import _counter_kinds_on
        from mtgz_ir.game import add_counters, remove_counters

        if len(targets) < 2 or targets[0] == targets[1]:
            return
        if not self._still_legal(state, controller_id, targets[:2]):
            state.log.append(
                "a target is no longer legal; the counter move does nothing"
            )
            return
        first = next(o for o in state.battlefield if o.id == targets[0][1])
        second = next(o for o in state.battlefield if o.id == targets[1][1])
        kinds = _counter_kinds_on(first)
        if not kinds:
            state.log.append(
                f"{first.printing.name} has no counter; nothing moves"
            )
            return
        idx = 0
        p = state.player_by_id(controller_id)
        if len(kinds) > 1 and p.agent is not None:
            pick = p.agent.choose_mode(state, controller_id, list(kinds))
            if isinstance(pick, int) and 0 <= pick < len(kinds):
                idx = pick
        kind = kinds[idx]
        remove_counters(first, kind, 1)
        added = add_counters(state, second, kind, 1)
        state.log.append(
            f"P{controller_id} moves a {kind} counter from "
            f"{first.printing.name} onto {second.printing.name}"
            + (f" ({added} put)" if added != 1 else "")
        )

    def describe(self) -> str:
        names = [_noun(getattr(s, "filter", None)) for s in self.targets[:2]]
        while len(names) < 2:
            names.append("permanent")
        return (
            f"move a counter from target {names[0]} onto a second target "
            f"{names[1]}"
        )


def _noun(pred) -> str:
    """A coarse noun phrase for a ``MoveCounterBetweenTargets`` slot
    filter (log / describe only)."""
    from mtgz_ir.effects.object_predicates import (
        ControlledByOpponent, ControlledByYou, IsCreature, ObjectAnd,
    )
    from mtgz_ir.effects.value_predicates import IsPermanent

    if isinstance(pred, ObjectAnd):
        base = _noun(pred.left)
        if isinstance(pred.right, ControlledByYou):
            return f"{base} you control"
        if isinstance(pred.right, ControlledByOpponent):
            return f"{base} an opponent controls"
        return base
    if isinstance(pred, IsCreature):
        return "creature"
    if isinstance(pred, IsPermanent):
        return "permanent"
    return "permanent"


# ---------------------------------------------------------------------------
# "Whenever you put one or more <kind> counters on ..."
# ---------------------------------------------------------------------------


def counters_putter_id(state: "GameState", subjects) -> int | None:
    """The player who put the counters of the ``COUNTERS_ADDED`` event
    being dispatched, as the Python engine knows it: the controller of
    the spell or ability resolving (``state.resolving_item_controller_id``,
    CR 122.6), else the recipient's controller (counters put while no
    spell or ability resolves: a cost, or an object entering with them,
    CR 122.6a). See ``docs/SEARCH_LIMITATIONS.md`` (third batch)."""
    putter = getattr(state, "resolving_item_controller_id", None)
    if putter is not None:
        return putter
    obj = _resolve_subject(subjects, None)
    return getattr(obj, "controller_id", None)


@dataclass(frozen=True)
class CountersPutBy(TriggerPredicate):
    """On a ``COUNTERS_ADDED`` event: the player who put the counters is
    ``by`` (``YOU``: the source's controller; ``OPPONENT``: another
    player), as ``Controlled.by`` reads ("Whenever **you** put one or
    more +1/+1 counters on a creature you control" -- Terrasymbiosis).

    CR 122.6: counters are put on an object by the controller of the
    spell or ability whose effect puts them, or by the player who pays
    a cost that puts them; CR 122.6a: counters an object enters the
    battlefield with are put by the player the effect names, else by
    the object's controller. An opponent's effect putting a counter on
    your creature is not "you put"."""

    by: _Controller = YOU

    def matches(self, state, subjects, source):
        putter = counters_putter_id(state, subjects)
        if putter is None:
            return False
        if self.by == YOU:
            return putter == source.controller_id
        if self.by == OPPONENT:
            return putter != source.controller_id
        raise NotImplementedError(f"unsupported controller ref: {self.by!r}")


__all__ = [
    "CountersPutBy",
    "GraveyardReturnGroup",
    "LeftBattlefieldTo",
    "MoveCounterBetweenTargets",
    "ReturnCardsExiledWithSourceToBattlefield",
    "ReturnTargetCardFromYourGraveyardToBattlefieldAs",
    "ReturnTargetCardsFromYourGraveyard",
    "counters_putter_id",
]
