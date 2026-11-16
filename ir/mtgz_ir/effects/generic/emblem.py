"""Emblem substrate (CR 114) — the Tezzeret, Cruel Captain emblem's
begin-combat trigger body and the emblem that carries it.

An emblem is a command-zone object (``GameState.emblems``) created by a
"You get an emblem with '<abilities>'" effect (CR 113.1g / 114). It has
no characteristics other than its abilities (CR 114.3) and never leaves
the command zone (CR 114.5). We model an emblem as a synthetic
``GameObject`` whose ``printing`` is a ``make_emblem`` ``CardPrinting``
carrying the emblem's triggered abilities; the existing trigger-dispatch
machinery (extended to scan ``state.emblems`` as a source) and the
existing ``resolve_top_of_stack`` path then fire + resolve the emblem's
abilities exactly like a permanent's.

Emblems are created by ``cube_walkers.CreateEmblemWithAbilities``, which
carries the emblem's abilities inline. This module ships the body of the
one emblem the unified grammar can't parse -- Tezzeret, Cruel Captain's
−7 emblem: "At the beginning of combat on your turn, put three +1/+1
counters on target artifact you control. If it's not a creature, it
becomes a 0/0 Robot artifact creature." (:func:`tezzeret_cruel_captain_emblem`).

**Honest-stop (documented in docs/SEARCH_LIMITATIONS.md).** The
"becomes a 0/0 Robot artifact creature" half is a PERMANENT layer-4
type-set + layer-7b P/T-set continuous-effect animate and is NOT
modeled here — the emblem trigger body lands the three +1/+1 counters
on the target artifact (byte-equal cross-engine) and stops. See the
``TezzeretEmblemBeginCombat`` docstring for the precise remainder map.
"""

from __future__ import annotations

from dataclasses import dataclass

from mtgz_ir.effects.base import Effect, Target, TriggeredAbility


TEZZERET_CRUEL_CAPTAIN_EMBLEM_NAME = "Tezzeret, Cruel Captain emblem"


def tezzeret_cruel_captain_emblem():
    """The ``CreateEmblemWithAbilities`` of Tezzeret, Cruel Captain's −7:
    an emblem with "At the beginning of combat on your turn, put three
    +1/+1 counters on target artifact you control. If it's not a
    creature, it becomes a 0/0 Robot artifact creature." The emblem's
    ability is inline data (no per-card emblem key); its body is
    :class:`TezzeretEmblemBeginCombat`."""
    from mtgz_ir.effects.generic.cube_walkers import CreateEmblemWithAbilities
    from mtgz_ir.effects.predicates import YOU, Controlled, Event

    return CreateEmblemWithAbilities(
        name=TEZZERET_CRUEL_CAPTAIN_EMBLEM_NAME,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.BEGINNING_OF_COMBAT,
                predicate=Controlled(by=YOU),
                effect=TezzeretEmblemBeginCombat(),
            ),
        ),
    )


@dataclass(frozen=True)
class TezzeretEmblemBeginCombat(Effect):
    """Tezzeret, Cruel Captain emblem's begin-combat trigger.

    Printed: "At the beginning of combat on your turn, put three +1/+1
    counters on target artifact you control. If it's not a creature, it
    becomes a 0/0 Robot artifact creature."

    **What this resolves (byte-equal cross-engine, LANDED).** Picks a
    target artifact the emblem's controller controls (CR 608.2b
    re-verified at resolution) and adds THREE +1/+1 counters via
    ``add_plus_one_counters`` (honoring the counter-doubling replacement
    + the counters-added observer trigger inside the helper).

    **What is HONEST-STOPPED (the becomes-Robot half).** "If it's not a
    creature, it becomes a 0/0 Robot artifact creature" is a PERMANENT
    (not UEOT) continuous effect: a layer-4 type-adding set (the
    artifact gains creature + the Robot creature type) plus a layer-7b
    P/T-set (0/0). Neither engine has a PERMANENT type+P/T-set animate on
    an artifact today — the existing animate substrate
    (``BecomeLandCreatureUEOT`` / Spacecraft "becomes a creature") is
    UEOT and land-shaped. Building it is its own layer-system slice; it
    is NOT taken here per the campaign honest-stop policy. See
    docs/SEARCH_LIMITATIONS.md for the precise layer map. The
    three-counter half alone is the byte-equal landing.
    """

    amount: int = 3

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # "target artifact you control" — any artifact the controller
        # controls is legal (CR 115.1 target restriction).
        from mtgz_ir.game import is_artifact_now

        return [
            ("permanent", o.id)
            for o in state.battlefield
            if is_artifact_now(state, o) and o.controller_id == controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters, is_artifact_now

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        # CR 608.2b — re-verify the target still matches "artifact you
        # control" at resolution.
        if not (
            is_artifact_now(state, obj) and obj.controller_id == controller_id
        ):
            return
        added = add_plus_one_counters(state, obj, self.amount)
        if added:
            state.log.append(
                f"{obj.printing.name} gets {self.amount} +1/+1 counter(s) "
                f"(now {obj.plus_one_counters})"
            )
        # HONEST-STOP: the "if it's not a creature, it becomes a 0/0
        # Robot artifact creature" permanent animate is NOT applied here
        # (no permanent layer-4 type-set + layer-7b P/T-set animate
        # substrate on either engine). See module docstring +
        # docs/SEARCH_LIMITATIONS.md.

    def describe(self) -> str:
        return (
            "put three +1/+1 counters on target artifact you control "
            "(becomes-0/0-Robot animate deferred)"
        )
