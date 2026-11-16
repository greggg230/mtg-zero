"""Roto package P6: damage prevention and redirection (CR 615
prevention effects; redirection, CR 614.1 / 615).

Three rules-modifying effects ride the continuous-effects registry as
``layer=0`` entries (the ``cast_sorceries_as_flash`` precedent) that end
at cleanup (``expires_at="cleanup"``), so they survive their source
leaving the battlefield and are cloned / hashed with the rest of the
registry:

* ``PREVENT_COMBAT_TO_AND_BY`` (``target_id`` = a creature) -- "Prevent
  all combat damage that would be dealt to and dealt by <that creature>
  this turn." (Maze of Ith, Cephalid Illusionist).
* ``DAMAGE_CANT_BE_PREVENTED`` (global) -- "Damage can't be prevented
  this turn." (Impractical Joke); see :func:`damage_cant_be_prevented`.
* ``REDIRECT_NEXT_DAMAGE`` (``target_id`` = the protected creature,
  ``params=(n, redirect_to_id)``) -- "The next N damage that would be
  dealt to this creature this turn is dealt to target creature you
  control instead." (Nomads en-Kor). A shield, used up as damage is
  redirected; redirection isn't prevention, so it applies even to
  damage that can't be prevented.

And one printed static, held in ``CardPrinting.replacement_statics``:
:class:`PreventAllDamageToSelf` -- "Prevent all damage that would be
dealt to <this creature>." (Black Panther, Hope Enduring).

Damage is routed through these by two hooks: :func:`route_damage` at
the top of ``effects.base._apply_damage`` (noncombat damage) and
:func:`route_combat_damage_events` in ``combat._resolve_damage_step``.
Prevention applies first, then redirection; redirected damage is a
modified event that the new recipient's own prevention / shields apply
to in turn (CR 616.1, each shield at most once -- it's used up).
"""

from __future__ import annotations

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState

PREVENT_COMBAT_TO_AND_BY = "prevent_combat_damage_to_and_by"
REDIRECT_NEXT_DAMAGE = "redirect_next_damage"
DAMAGE_CANT_BE_PREVENTED = "damage_cant_be_prevented"


@dataclass(frozen=True)
class PreventAllDamageToSelf:
    """Printed static: "Prevent all damage that would be dealt to <this
    creature>." (``CardPrinting.replacement_statics``)."""

    def describe(self) -> str:
        return "prevent all damage that would be dealt to this creature"


# ---------------------------------------------------------------- queries


def damage_cant_be_prevented(state: "GameState") -> bool:
    """"Damage can't be prevented this turn" (Impractical Joke) is in
    force: every damage-prevention check in the engine consults this
    (``game.player_damage_prevented``,
    ``creature_statics_wg.combat_damage_unpreventable``, the protection
    check in ``effects.base._apply_damage`` and this module's
    routers)."""
    return any(
        ce.kind == DAMAGE_CANT_BE_PREVENTED for ce in state.continuous_effects
    )


def _combat_damage_prevented_for(state: "GameState", obj_id: int) -> bool:
    return any(
        ce.kind == PREVENT_COMBAT_TO_AND_BY and ce.target_id == obj_id
        for ce in state.continuous_effects
    )


def prevents_all_damage_to(obj: "GameObject") -> bool:
    return any(
        isinstance(rule, PreventAllDamageToSelf)
        for rule in obj.printing.replacement_statics
    )


def _find(state: "GameState", obj_id: int):
    return next((o for o in state.battlefield if o.id == obj_id), None)


def _take_shield(state: "GameState", obj_id: int, amount: int):
    """Use up to ``amount`` of the oldest redirection shield on
    ``obj_id``: returns ``(redirected, redirect_to_id)`` or ``None``."""
    for i, ce in enumerate(state.continuous_effects):
        if ce.kind != REDIRECT_NEXT_DAMAGE or ce.target_id != obj_id:
            continue
        left, redirect_to = ce.params
        used = min(left, amount)
        if left - used > 0:
            state.continuous_effects[i] = replace(ce, params=(left - used, redirect_to))
        else:
            del state.continuous_effects[i]
        state._invalidate_layered_cache()
        return used, redirect_to
    return None


# ------------------------------------------------------------- routing


def route_damage(state: "GameState", target: Target, amount: int) -> int:
    """Noncombat damage about to be dealt to ``target``: apply the P6
    prevention and redirection effects. Redirected damage is dealt here
    (through ``_apply_damage``, so the recipient's own effects apply);
    returns the amount still dealt to ``target``."""
    kind, target_id = target
    if kind != "creature" or amount <= 0:
        return amount
    obj = _find(state, target_id)
    if obj is None:
        return amount
    if prevents_all_damage_to(obj) and not damage_cant_be_prevented(state):
        state.log.append(
            f"{amount} damage to {obj.printing.name} is prevented"
        )
        return 0
    while amount > 0:
        taken = _take_shield(state, obj.id, amount)
        if taken is None:
            break
        used, redirect_to = taken
        recipient = _find(state, redirect_to)
        if recipient is None or recipient is obj:
            # Gone (or the creature itself): the damage stays put.
            break
        amount -= used
        state.log.append(
            f"{used} damage to {obj.printing.name} is dealt to "
            f"{recipient.printing.name} instead"
        )
        from mtgz_ir.effects.base import _apply_damage

        _apply_damage(state, ("creature", recipient.id), used)
    return amount


def route_combat_damage_events(state: "GameState", events: list) -> list:
    """Combat damage events ``(kind, target_id, amount, source_id,
    source_keywords, source_controller)``: drop the prevented ones
    (combat damage to or by a creature under a Maze-style effect; any
    damage to a creature that prevents all damage dealt to it) unless the
    damage can't be prevented, and split off the parts redirection
    shields send elsewhere."""
    if not state.continuous_effects and not any(
        o.printing.replacement_statics for o in state.battlefield
    ):
        return events
    from mtgz_ir.creature_statics_wg import combat_damage_unpreventable

    out: list = []
    for ev in events:
        kind, target_id, amount, source_id, kws, controller = ev
        source = _find(state, source_id)
        unpreventable = source is not None and combat_damage_unpreventable(state, source)
        if not unpreventable and _combat_damage_prevented_for(state, source_id):
            state.log.append(f"{amount} combat damage is prevented")
            continue
        pending = [(kind, target_id, amount)]
        while pending:
            k, tid, amt = pending.pop()
            obj = _find(state, tid) if k == "creature" else None
            if obj is not None and not unpreventable and (
                prevents_all_damage_to(obj)
                or _combat_damage_prevented_for(state, tid)
            ):
                state.log.append(f"{amt} damage to {obj.printing.name} is prevented")
                continue
            if obj is not None:
                taken = _take_shield(state, tid, amt)
                if taken is not None:
                    used, redirect_to = taken
                    recipient = _find(state, redirect_to)
                    if recipient is not None and recipient is not obj:
                        state.log.append(
                            f"{used} damage to {obj.printing.name} is dealt to "
                            f"{recipient.printing.name} instead"
                        )
                        pending.append(("creature", recipient.id, used))
                        if amt - used > 0:
                            pending.append((k, tid, amt - used))
                        continue
            out.append((k, tid, amt, source_id, kws, controller))
    return out


# -------------------------------------------------------------- effects


def _source_id(state: "GameState") -> int:
    source = state.resolving_spell or state.resolving_source
    return source.id if source is not None else -1


def _still_legal_target(state, effect: Effect, controller_id, targets):
    from mtgz_ir.effects.generic.targets_helpers import _slot_target_still_legal

    if not targets or not effect.targets:
        return None
    chosen = targets[0]
    prior = state.resolving_controller_id
    try:
        state.resolving_controller_id = controller_id
        if not _slot_target_still_legal(state, effect.targets[0], chosen):
            return None
    finally:
        state.resolving_controller_id = prior
    return _find(state, chosen[1])


@dataclass(frozen=True)
class DamageCantBePreventedThisTurn(Effect):
    """"Damage can't be prevented this turn." (Impractical Joke.)"""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.add_continuous_effect(
            source_id=_source_id(state),
            layer=0,
            kind=DAMAGE_CANT_BE_PREVENTED,
            expires_at="cleanup",
            description="damage can't be prevented this turn",
        )
        state.log.append("Damage can't be prevented this turn")

    def describe(self) -> str:
        return "damage can't be prevented this turn"


@dataclass(frozen=True)
class PreventCombatDamageToAndByTarget(Effect):
    """"[Untap target <creature>.] Prevent all combat damage that would be
    dealt to and dealt by <that | target> creature this turn."

    ``untap`` -- Maze of Ith's leading "Untap target attacking
    creature." on the same target."""

    targets: tuple = ()
    untap: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        obj = _still_legal_target(state, self, controller_id, targets)
        if obj is None:
            return
        if self.untap and obj.tapped:
            obj.tapped = False
            state.log.append(f"{obj.printing.name} is untapped")
        state.add_continuous_effect(
            source_id=_source_id(state),
            layer=0,
            kind=PREVENT_COMBAT_TO_AND_BY,
            target_id=obj.id,
            expires_at="cleanup",
            description=(
                f"prevent all combat damage dealt to and by "
                f"{obj.printing.name} this turn"
            ),
        )
        state.log.append(
            f"All combat damage to and by {obj.printing.name} will be "
            f"prevented this turn"
        )

    def describe(self) -> str:
        from mtgz_ir.effects.generic.targets_helpers import _describe_targets

        what = _describe_targets(self.targets)
        text = (
            f"prevent all combat damage that would be dealt to and dealt by "
            f"{what} this turn"
        )
        return f"untap {what}, then {text}" if self.untap else text


@dataclass(frozen=True)
class RedirectNextDamageToSourceToTarget(Effect):
    """"The next N damage that would be dealt to this creature this turn
    is dealt to target <creature> instead." (Nomads en-Kor.)"""

    amount: int = 1
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        recipient = _still_legal_target(state, self, controller_id, targets)
        if source is None or recipient is None or source not in state.battlefield:
            return
        state.add_continuous_effect(
            source_id=source.id,
            layer=0,
            kind=REDIRECT_NEXT_DAMAGE,
            target_id=source.id,
            params=(self.amount, recipient.id),
            expires_at="cleanup",
            description=(
                f"the next {self.amount} damage to {source.printing.name} "
                f"is dealt to {recipient.printing.name} instead"
            ),
        )
        state.log.append(
            f"The next {self.amount} damage to {source.printing.name} this "
            f"turn will be dealt to {recipient.printing.name} instead"
        )

    def describe(self) -> str:
        from mtgz_ir.effects.generic.targets_helpers import _describe_targets

        return (
            f"the next {self.amount} damage that would be dealt to this "
            f"creature this turn is dealt to {_describe_targets(self.targets)} "
            f"instead"
        )


__all__ = [
    "DAMAGE_CANT_BE_PREVENTED",
    "DamageCantBePreventedThisTurn",
    "damage_cant_be_prevented",
    "PREVENT_COMBAT_TO_AND_BY",
    "PreventAllDamageToSelf",
    "PreventCombatDamageToAndByTarget",
    "REDIRECT_NEXT_DAMAGE",
    "RedirectNextDamageToSourceToTarget",
    "prevents_all_damage_to",
    "route_combat_damage_events",
    "route_damage",
]
