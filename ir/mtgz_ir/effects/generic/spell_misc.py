"""Filter-driven effects used by modal cube spells' modes.

- :class:`CreateTokenCopyOfTargetPermanent` — "Create a token that's a
  copy of target <np>." (Three Steps Ahead: "artifact or creature you
  control").
- :class:`CreateThisTurnTriggeredAbility` — "Whenever <event> this
  turn, <effect>." (Forth Eorlingas!).
- :class:`CreateTokensScaledByX` — "Create [twice] X <token>s" (Pest
  Infestation, Forth Eorlingas!).
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.generic.targets_helpers import _slot_target_still_legal

if TYPE_CHECKING:
    from mtgz_ir.effects.object_predicates import ObjectPredicate
    from mtgz_ir.game import GameState


def _still_legal(state, effect, chosen, controller_id) -> bool:
    prior = state.resolving_controller_id
    try:
        state.resolving_controller_id = controller_id
        return _slot_target_still_legal(state, effect.targets[0], chosen)
    finally:
        state.resolving_controller_id = prior


@dataclass(frozen=True)
class CreateTokenCopyOfTargetPermanent(Effect):
    """"Create a token that's a copy of target <np>." — the token copies
    the target's copiable values (its printing, CR 707.2) and enters
    under the controller's control."""

    target_filter: "ObjectPredicate"
    label: str = "permanent"
    targets: tuple = ()

    def __post_init__(self) -> None:
        from mtgz_ir.effects.targets import TargetSpec

        object.__setattr__(self, "targets", (TargetSpec(self.target_filter),))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.tokens import CreateToken

        if not targets or not _still_legal(state, self, targets[0], controller_id):
            return
        target = next(
            (o for o in state.battlefield if o.id == targets[0][1]), None,
        )
        base = target.printing
        supertypes = tuple(base.supertypes) + (
            ("Token",) if "Token" not in base.supertypes else ()
        )
        CreateToken(
            token_printing=dataclasses.replace(base, supertypes=supertypes),
        ).resolve(state, controller_id, ())

    def describe(self) -> str:
        return f"create a token that's a copy of target {self.label}"


@dataclass(frozen=True)
class CreateTokensScaledByX(Effect):
    """"Create [twice] X <token>s" — ``multiplier`` times the resolving
    spell's X tokens of ``token_printing`` (built from the printed
    characteristics, CR 111.4)."""

    token_printing: "object"
    multiplier: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.tokens import CreateToken

        count = self.multiplier * max(0, state.resolving_x_value)
        if count <= 0:
            return
        CreateToken(
            token_printing=self.token_printing, count=count,
        ).resolve(state, controller_id, ())

    def describe(self) -> str:
        times = "twice X" if self.multiplier == 2 else f"{self.multiplier}*X"
        return f"create {times} {self.token_printing.name} tokens"


@dataclass(frozen=True)
class CreateThisTurnTriggeredAbility(Effect):
    """"Whenever <event> this turn, <effect>." — a delayed triggered
    ability that exists for the rest of the turn (CR 603.7c).

    Modeled as a synthetic command-zone object carrying
    ``triggered_abilities`` (the emblem trigger source
    ``dispatch_event`` already scans, ``state.emblems``) flagged
    ``expires_at_cleanup``: ``turn.run_cleanup_step`` removes it, so the
    ability never triggers on a later turn."""

    triggered_abilities: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.cards.base import make_emblem
        from mtgz_ir.game import Zone

        spell = state.resolving_spell
        label = spell.printing.name if spell is not None else "effect"
        printing = make_emblem(
            name=f"{label} (this turn)",
            triggered_abilities=self.triggered_abilities,
        )
        holder = state.new_object(
            printing, owner_id=controller_id, zone=Zone.COMMAND,
        )
        holder.expires_at_cleanup = True
        holder.etb_timestamp = state.next_timestamp
        state.next_timestamp += 1
        state.emblems.append(holder)
        state.log.append(
            f"P{controller_id} gets a delayed triggered ability "
            f"until end of turn ({label})"
        )

    def describe(self) -> str:
        return "this turn: " + "; ".join(
            ta.describe() if hasattr(ta, "describe") else repr(ta)
            for ta in self.triggered_abilities
        )


__all__ = [
    "CreateThisTurnTriggeredAbility",
    "CreateTokensScaledByX",
    "CreateTokenCopyOfTargetPermanent",
]
