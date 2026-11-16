"""Filter-driven removal: destroy / exile target permanents, mass
destroy, and the "its controller ..." riders printed on removal spells.

The classes here are the engine side of the parser's compositional
target grammar (:mod:`mtgz_ir.cards.parser.target_phrases`): every
"destroy target <np>" / "exile target <np>" printing becomes one
:class:`RemoveTargetPermanents` carrying the composed filter, a target
count range, the regeneration rule, and an optional per-object
:class:`RemovalRider` that runs for the removed object's controller
with last-known information (CR 608.2h) captured *before* the object
left the battlefield:

- Swords to Plowshares: ``ControllerGainsLifeEqualToPower``
- Path to Exile / Erode: ``ControllerSearchesForBasicLand(optional=True)``
- Get Lost: ``ControllerCreatesTokens("Map", 2)``
- Fractured Identity: ``OtherPlayersCreateTokenCopy``

Rider timing follows the printed order and the rulings on these
cards: the rider applies to each target that is still legal when the
spell resolves, even if the removal itself didn't move the permanent
(an indestructible creature hit by Erode still gives its controller
the land search). A target that became illegal is untouched and gets
no rider; if every target is illegal the spell does nothing (CR
608.2b).

Regeneration (CR 701.15): effect-based destruction here honors a
regeneration shield unless the spell says the permanent "can't be
regenerated" (``can_be_regenerated=False``).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import RemovalAction
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.generic.targets_helpers import (
    _fizzle_message,
    _slot_target_still_legal,
)

if TYPE_CHECKING:
    from mtgz_ir.cards import CardPrinting
    from mtgz_ir.effects.object_predicates import ObjectPredicate
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------------
# Destruction helper (regeneration-aware)
# ---------------------------------------------------------------------------


def destroy_permanent(
    state: "GameState", obj: "GameObject", *, can_be_regenerated: bool = True,
) -> bool:
    """Destroy ``obj`` per CR 701.8, honoring indestructible and — when
    ``can_be_regenerated`` — a regeneration shield (CR 701.15: the
    shield is used up, the permanent is tapped, all damage is removed,
    and it's removed from combat instead of being destroyed).

    Returns ``True`` only if the permanent actually went to the
    graveyard.
    """
    from mtgz_ir.game import effective_keywords
    from mtgz_ir.turn import move_off_battlefield

    if "indestructible" in effective_keywords(state, obj):
        state.log.append(
            f"{obj.printing.name} is indestructible — destroy fizzles"
        )
        return False
    if can_be_regenerated and obj.regeneration_shield:
        obj.regeneration_shield = False
        obj.damage = 0
        obj.marked_lethal = False
        obj.tapped = True
        obj.attacking = False
        obj.blocking = None
        obj.blocked_by = []
        state.log.append(
            f"{obj.printing.name} regenerates (tapped, damage cleared)"
        )
        return False
    state.log.append(f"{obj.printing.name} is destroyed")
    move_off_battlefield(state, obj, destination="graveyard")
    return True


# ---------------------------------------------------------------------------
# Riders: "its controller ..." / "each player other than its controller ..."
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RemovedObjectInfo:
    """Last-known information about a removal target, captured while
    it was still on the battlefield (CR 608.2h)."""

    controller_id: int
    power: int
    printing: "CardPrinting"
    # The player whose spell / ability is doing the removing ("you").
    caster_id: int = -1


class RemovalRider:
    """A follow-up printed after a removal clause that refers back to
    the removed object ("its controller ...", "its power")."""

    def apply(self, state: "GameState", info: RemovedObjectInfo) -> None:
        raise NotImplementedError

    def describe(self) -> str:
        raise NotImplementedError


@dataclass(frozen=True)
class ControllerGainsLifeEqualToPower(RemovalRider):
    """"Its controller gains life equal to its power." (Swords to
    Plowshares) — last-known power; negative power gains nothing."""

    def apply(self, state, info):
        from mtgz_ir.game import gain_life

        amount = max(0, info.power)
        gain_life(state, info.controller_id, amount)
        p = state.player_by_id(info.controller_id)
        state.log.append(
            f"P{info.controller_id} gains {amount} life (life: {p.life})"
        )

    def describe(self) -> str:
        return "its controller gains life equal to its power"


@dataclass(frozen=True)
class ControllerSearchesForBasicLand(RemovalRider):
    """"Its controller [may] search[es] their library for a basic land
    card, put[s] it onto the battlefield tapped, then shuffle[s]."
    (Path to Exile / Erode with ``optional=True``; Winds of Abandon's
    mandatory search with ``optional=False``). The optional form asks
    the removed object's controller via ``choose_yes_no``."""

    optional: bool = True

    def apply(self, state, info):
        from mtgz_ir.effects.generic.search import (
            SearchLibraryForBasicLandTapped,
        )

        pid = info.controller_id
        p = state.player_by_id(pid)
        if self.optional and p.agent is not None and not p.agent.choose_yes_no(
            state, pid,
            "search your library for a basic land card, put it onto "
            "the battlefield tapped, then shuffle",
        ):
            state.log.append(f"P{pid} declines to search for a basic land")
            return
        SearchLibraryForBasicLandTapped(count=1).resolve(state, pid)

    def describe(self) -> str:
        may = "may search" if self.optional else "searches"
        return (
            f"its controller {may} their library for a basic land card, "
            f"puts it onto the battlefield tapped, then shuffles"
        )


@dataclass(frozen=True)
class ControllerCreatesTokens(RemovalRider):
    """"Its controller creates N <Name> tokens." (Get Lost: two Map
    tokens). ``token_name`` keys :data:`mtgz_ir.cards.tokens.TOKEN_PRINTINGS`."""

    token_name: str
    count: int = 1

    def apply(self, state, info):
        from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
        from mtgz_ir.effects.generic.tokens import CreateToken

        CreateToken(
            token_printing=TOKEN_PRINTINGS[self.token_name], count=self.count,
        ).resolve(state, info.controller_id)

    def describe(self) -> str:
        return f"its controller creates {self.count} {self.token_name} token(s)"


@dataclass(frozen=True)
class IfYouControlledItDraw(RemovalRider):
    """"If you controlled that permanent, draw N cards." (Boomerang
    Basics) — "you" is the removing spell's controller; the check uses
    the permanent's last-known controller."""

    amount: int = 1

    def apply(self, state, info):
        from mtgz_ir.turn import draw_n_cards

        if info.controller_id != info.caster_id:
            return
        draw_n_cards(state, info.caster_id, self.amount)
        state.log.append(f"P{info.caster_id} draws {self.amount} card(s)")

    def describe(self) -> str:
        return f"if you controlled that permanent, draw {self.amount} card(s)"


@dataclass(frozen=True)
class OtherPlayersCreateTokenCopy(RemovalRider):
    """"Each player other than its controller creates a token that's a
    copy of it." (Fractured Identity) — the copy uses the last-known
    copiable values (the printing) of the removed permanent (CR 707.2,
    608.2h)."""

    def apply(self, state, info):
        import dataclasses as _dc

        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        base = info.printing
        supertypes = tuple(base.supertypes) + (
            ("Token",) if "Token" not in base.supertypes else ()
        )
        token_printing = _dc.replace(base, supertypes=supertypes)
        for p in state.players:
            if p.has_lost or p.id == info.controller_id:
                continue
            obj = state.new_object(
                token_printing, owner_id=p.id, zone=Zone.BATTLEFIELD,
            )
            state.log.append(
                f"P{p.id} creates a token copy of {base.name}"
            )
            put_into_battlefield(state, obj, p.id)

    def describe(self) -> str:
        return (
            "each player other than its controller creates a token "
            "that's a copy of it"
        )


# ---------------------------------------------------------------------------
# Targeted removal
# ---------------------------------------------------------------------------


_ACTIONS = ("destroy", "exile", "bounce")
_ACTION_PAST = {"destroy": "destroyed", "exile": "exiled", "bounce": "returned"}


def _remove_one(
    state: "GameState", obj, action: str, *, can_be_regenerated: bool,
    rider: "RemovalRider | None", caster_id: int,
) -> None:
    """Apply one removal ``action`` to ``obj`` (capturing last-known
    information first) and run ``rider`` for it."""
    from mtgz_ir.effects.generic.targets_helpers import _bounce_to_hand
    from mtgz_ir.game import effective_power, is_creature_now
    from mtgz_ir.turn import move_off_battlefield

    info = RemovedObjectInfo(
        controller_id=obj.controller_id,
        power=effective_power(state, obj) if is_creature_now(obj) else 0,
        printing=obj.printing,
        caster_id=caster_id,
    )
    if action == "destroy":
        destroy_permanent(state, obj, can_be_regenerated=can_be_regenerated)
    elif action == "exile":
        state.log.append(f"{obj.printing.name} is exiled")
        move_off_battlefield(state, obj, destination="exile")
    else:
        _bounce_to_hand(state, obj)
    if rider is not None:
        rider.apply(state, info)


@dataclass(frozen=True)
class RemoveTargetPermanents(Effect):
    """Destroy or exile each chosen target permanent.

    ``target_filter`` is the composed printed noun phrase; the single
    ``TargetSpec`` slot carries ``count_min..count_max`` ("up to two
    target artifacts and/or enchantments" -> 0..2). Each chosen target
    is re-verified at resolution; illegal ones are skipped and get no
    rider. ``only_if`` is a resolution-time condition on each (legal)
    target — Pyroblast's "Destroy target permanent if it's blue."
    targets any permanent but only destroys a blue one.
    """

    action: RemovalAction
    target_filter: "ObjectPredicate"
    count_min: int = 1
    count_max: int = 1
    can_be_regenerated: bool = True
    rider: RemovalRider | None = None
    only_if: "ObjectPredicate | None" = None
    # "up to X target ...": the maximum is the spell's X, which is
    # chosen before targets (``actions._cast_spell``) and substituted by
    # ``cast_time_shape``.
    count_from_x: bool = False
    targets: tuple = ()

    def cast_time_shape(self, *, kicked=False, alt_cost_paid=False, x_value=None):
        if not self.count_from_x or x_value is None:
            return self
        import dataclasses as _dc

        return _dc.replace(self, count_max=x_value, count_from_x=False)

    def __post_init__(self) -> None:
        from mtgz_ir.effects.targets import TargetSpec

        if self.action not in _ACTIONS:
            raise ValueError(f"unknown removal action: {self.action!r}")
        object.__setattr__(self, "action", RemovalAction(self.action))
        object.__setattr__(self, "targets", (TargetSpec(
            self.target_filter,
            count_min=self.count_min,
            count_max=self.count_max,
        ),))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        slot = self.targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal: list = []
            for chosen in targets:
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(
                        _fizzle_message(state, chosen, action=self.action)
                    )
                    continue
                obj = next(
                    (o for o in state.battlefield if o.id == chosen[1]), None,
                )
                if obj is not None:
                    legal.append(obj)
        finally:
            state.resolving_controller_id = prior
        for obj in legal:
            if self.only_if is not None and not self.only_if.test(obj, state):
                state.log.append(
                    f"{obj.printing.name} doesn't meet the condition — "
                    f"not {_ACTION_PAST[self.action]}"
                )
                continue
            _remove_one(
                state, obj, self.action,
                can_be_regenerated=self.can_be_regenerated,
                rider=self.rider, caster_id=controller_id,
            )

    def describe(self) -> str:
        from mtgz_ir.effects.generic.targets_helpers import _describe_filter

        count = (
            f"up to {self.count_max} "
            if self.count_min == 0 else
            (f"{self.count_max} " if self.count_max > 1 else "")
        )
        text = f"{self.action} {count}{_describe_filter(self.target_filter)}"
        if self.only_if is not None:
            text += " if it meets the condition"
        if not self.can_be_regenerated:
            text += "; it can't be regenerated"
        if self.rider is not None:
            text += f"; {self.rider.describe()}"
        return text


@dataclass(frozen=True)
class RemoveAllMatching(Effect):
    """"Destroy all <np>." / "Exile each <np>." — every battlefield
    permanent matching ``permanent_filter`` is removed (victims are
    chosen before any leaves), each followed by ``rider``.

    ``can_be_regenerated=False`` models "They can't be regenerated."
    (Wrath of God, overloaded Damn). ``max_mana_value_from_x=True``
    additionally restricts to permanents with mana value X or less, X
    being the resolving spell's X (Vicious Rivalry's "with mana value X
    or less", X paid as life). Overloaded Winds of Abandon is
    ``action="exile"`` with a per-creature mandatory basic-land search.
    """

    permanent_filter: "ObjectPredicate"
    action: RemovalAction = RemovalAction.DESTROY
    can_be_regenerated: bool = True
    max_mana_value_from_x: bool = False
    rider: RemovalRider | None = None
    label: str = "all matching permanents"

    def __post_init__(self) -> None:
        if self.action not in _ACTIONS:
            raise ValueError(f"unknown removal action: {self.action!r}")
        object.__setattr__(self, "action", RemovalAction(self.action))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.value_predicates import mana_value_of

        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            victims = [
                o for o in state.battlefield
                if self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        if self.max_mana_value_from_x:
            x = state.resolving_x_value
            victims = [o for o in victims if mana_value_of(o) <= x]
        state.log.append(
            f"{self.action} {self.label}: {len(victims)} permanent(s)"
        )
        for obj in victims:
            if obj in state.battlefield:
                _remove_one(
                    state, obj, self.action,
                    can_be_regenerated=self.can_be_regenerated,
                    rider=self.rider, caster_id=controller_id,
                )

    def describe(self) -> str:
        text = f"{self.action} {self.label}"
        if self.max_mana_value_from_x:
            text += " with mana value X or less"
        if not self.can_be_regenerated:
            text += "; they can't be regenerated"
        if self.rider is not None:
            text += f"; for each, {self.rider.describe()}"
        return text


@dataclass(frozen=True)
class EachOpponentSacrificesMatching(Effect):
    """"Each opponent sacrifices a <np> of their choice." — each
    opponent picks one permanent they control matching
    ``permanent_filter`` (via ``choose_sacrifice``) and sacrifices it;
    an opponent with none is unaffected. (Sheoldred's Edict modes 2-3:
    "a creature token", "a planeswalker".)"""

    permanent_filter: "ObjectPredicate"
    label: str = "permanent"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            prior = state.resolving_controller_id
            try:
                state.resolving_controller_id = p.id
                legal = [
                    o for o in state.battlefield
                    if o.controller_id == p.id
                    and self.permanent_filter.test(o, state)
                ]
            finally:
                state.resolving_controller_id = prior
            if not legal:
                continue
            chosen = legal[0]
            if p.agent is not None:
                pick = p.agent.choose_sacrifice(
                    state, p.id, [o.id for o in legal],
                )
                chosen = next((o for o in legal if o.id == pick), legal[0])
            state.log.append(f"P{p.id} sacrifices {chosen.printing.name}")
            sacrifice(state, chosen)

    def describe(self) -> str:
        return f"each opponent sacrifices a {self.label} of their choice"


__all__ = [
    "EachOpponentSacrificesMatching",
    "ControllerCreatesTokens",
    "ControllerGainsLifeEqualToPower",
    "ControllerSearchesForBasicLand",
    "IfYouControlledItDraw",
    "RemoveAllMatching",
    "OtherPlayersCreateTokenCopy",
    "RemovalRider",
    "RemoveTargetPermanents",
    "RemovedObjectInfo",
    "destroy_permanent",
]
