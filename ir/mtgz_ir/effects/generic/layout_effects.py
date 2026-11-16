"""Effect primitives of the non-normal card layouts (cube layouts bucket):
transforming double-faced cards, Sagas, Classes, levelers.

The engine mechanics these drive live in :mod:`mtgz_ir.layouts`; parser
entry points are in :mod:`mtgz_ir.cards.parser.layout_grammar`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.pt_formulas import PTFormula

if TYPE_CHECKING:  # pragma: no cover
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class TransformSource(Effect):
    """"Transform <this permanent>." (CR 701.28a) -- the resolving
    ability's source transforms. No-op when the source has left the
    battlefield."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        from mtgz_ir.layouts import transform_permanent

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return False
        return transform_permanent(state, source)

    def describe(self) -> str:
        return "transform this permanent"


@dataclass(frozen=True)
class ExileSourceThenReturnTransformed(Effect):
    """"Exile <this permanent>, then return it to the battlefield
    transformed under <its owner's | your> control." The source is
    exiled; if it is a transforming double-faced card it returns with
    its back face up as a new object (a planeswalker back face enters
    with its printed loyalty). A token / non-DFC stays in exile
    (CR 701.28c). ``under_owners_control`` picks the owner over the
    ability's controller."""

    under_owners_control: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        from mtgz_ir.layouts import put_onto_battlefield_transformed
        from mtgz_ir.turn import _reset_left_battlefield_object, move_off_battlefield

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return False
        move_off_battlefield(state, source, destination="exile")
        owner = state.player_by_id(source.owner_id)
        if source not in owner.exile:
            # A token ceased to exist / a replacement moved it elsewhere.
            return False
        new_controller = (
            source.owner_id if self.under_owners_control else controller_id
        )
        owner.exile.remove(source)
        source.exiled_face_down = False  # left exile (CR 400.7)
        # CR 400.7: it returns as a new object -- no counters, damage or
        # temporary bonuses carry over (the flicker effects' reset).
        _reset_left_battlefield_object(source)
        if not put_onto_battlefield_transformed(state, source, new_controller):
            owner.exile.append(source)
            return False
        return True

    def describe(self) -> str:
        who = "its owner's" if self.under_owners_control else "your"
        return (
            "exile this permanent, then return it to the battlefield "
            f"transformed under {who} control"
        )


@dataclass(frozen=True)
class YourPermanentsBecomeCreaturesUEOT(Effect):
    """"All <card type>s you control become P/T creatures until end of
    turn. They're still <card type>s." (Life): each such permanent gains
    the creature type with base P/T ``power``/``toughness`` until end of
    turn (layer-4 ``add_type`` + layer-7b ``set_base_pt`` registry
    entries through ``ApplyEffect(BecomeType(...), EndOfTurn())``), in
    addition to its other types."""

    card_type: str = "Land"
    power: int = 1
    toughness: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import has_card_type
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.become_type import BecomeType

        affected = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and has_card_type(state, o, self.card_type)
        ]
        for obj in affected:
            ApplyEffect(
                inner=BecomeType(
                    types_to_add=("creature",),
                    base_pt=(self.power, self.toughness),
                ),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, (("permanent", obj.id),))
        state.log.append(
            f"{len(affected)} {self.card_type.lower()}(s) P{controller_id} "
            f"controls become {self.power}/{self.toughness} creatures "
            "until end of turn"
        )

    def describe(self) -> str:
        return (
            f"all {self.card_type.lower()}s you control become "
            f"{self.power}/{self.toughness} creatures until end of turn"
        )


@dataclass(frozen=True)
class CreateTokenCopyOfTargetWithKeywordsSacAtEndStep(Effect):
    """"Create a token that's a copy of <target>, except it has
    <keywords>. Sacrifice it at the beginning of the next end step."
    (Reflection of Kiki-Jiki).

    The token copies the target's copiable values -- its current face's
    printing (CR 707.2 / 707.8; a copy of a double-faced permanent has
    only that face, so the token can't transform) -- with the Token
    supertype and the ``keywords`` exception added. A delayed trigger
    sacrifices it at the beginning of the next end step (whoever's turn).
    ``targets`` carries the composed target filter (``TargetSpec``)."""

    targets: tuple = ()
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        import dataclasses as _dc

        from mtgz_ir.effects.generic.delayed_actions import (
            SacrificeSourceDelayed,
            schedule_at_next_end_step,
        )
        from mtgz_ir.effects.generic.tokens import CreateToken

        if not targets:
            return
        _, tid = targets[0]
        target = next((o for o in state.battlefield if o.id == tid), None)
        if target is None:
            return
        base = target.printing
        printing = _dc.replace(
            base,
            supertypes=base.supertypes + (
                () if "Token" in base.supertypes else ("Token",)
            ),
            keywords=base.keywords | self.keywords,
            transform_back_face=None,
            mdfc_back_face=None,
        )
        before = {o.id for o in state.battlefield}
        CreateToken(token_printing=printing).resolve(state, controller_id, ())
        for obj in state.battlefield:
            if obj.id not in before and obj.printing is printing:
                schedule_at_next_end_step(
                    state, controller_id, obj.id, SacrificeSourceDelayed(),
                )

    def describe(self) -> str:
        kws = ", ".join(sorted(self.keywords))
        return (
            "create a token that's a copy of target creature, except it "
            f"has {kws}; sacrifice it at the beginning of the next end step"
        )


@dataclass(frozen=True)
class GainClassLevel(Effect):
    """"[Cost]: Level N" (CR 716.2a): the resolving ability's source --
    a Class at level N-1 -- becomes level N (``layouts.gain_class_level``).
    No-op if the source left the battlefield or is no longer level N-1."""

    level: int = 2

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        from mtgz_ir.layouts import gain_class_level

        source = state.resolving_source
        if (
            source is None or source not in state.battlefield
            or source.printing.class_level != self.level - 1
        ):
            return False
        return gain_class_level(state, source, self.level)

    def describe(self) -> str:
        return f"this Class becomes level {self.level}"


@dataclass(frozen=True)
class ThenTargetBecomesCreatureIfNot(Effect):
    """"<inner effect on target permanent>. If it isn't a creature, it
    becomes a P/T <Subtype> creature in addition to its other types."
    (Does Machines' level 3): resolve ``inner`` on the chosen target,
    then -- if that permanent still isn't a creature -- it becomes one
    with base P/T ``power``/``toughness`` and the subtype, with no end
    (layer-4 ``add_type`` / ``add_subtype`` and layer-7b ``set_base_pt``
    registry entries sourced by the permanent itself, so they last as
    long as it stays on the battlefield)."""

    inner: "Effect | None" = None
    power: int = 0
    toughness: int = 0
    subtype: str = ""

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(self, state, controller_id):
        return self.inner.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.inner.target_count_range()

    def target_slot_kinds(self):
        return self.inner.target_slot_kinds()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.become_type import BecomeType
        from mtgz_ir.game import is_creature_now

        self.inner.resolve(state, controller_id, targets)
        if not targets:
            return
        _, tid = targets[0]
        obj = next((o for o in state.battlefield if o.id == tid), None)
        if obj is None or is_creature_now(obj):
            return
        saved = state.resolving_source
        state.resolving_source = obj
        try:
            BecomeType(
                types_to_add=("creature",),
                subtypes_to_add=(self.subtype,) if self.subtype else (),
                base_pt=(self.power, self.toughness),
            ).resolve(state, controller_id, (("permanent", obj.id),))
        finally:
            state.resolving_source = saved
        state.log.append(
            f"{obj.printing.name} becomes a {self.power}/{self.toughness} "
            f"{self.subtype} creature"
        )

    def describe(self) -> str:
        return (
            f"{self.inner.describe()}; if it isn't a creature, it becomes a "
            f"{self.power}/{self.toughness} {self.subtype} creature"
        )


# ---------------------------------------------------------------------------
# Levelers (CR 711)
# ---------------------------------------------------------------------------


from mtgz_ir.effects.conditions import Condition  # noqa: E402


@dataclass(frozen=True)
class LevelCountersInRange(Condition):
    """A leveler's level-symbol gate (CR 711.2a): the static ability's
    source has at least ``lo`` and (when ``hi`` is set) at most ``hi``
    level counters. Source-aware (``StaticAbility.applies_to`` calls
    :meth:`matches_for_source`)."""

    lo: int = 0
    hi: "int | None" = None

    def matches(self, state, viewer_id, chosen_targets=None):
        return False  # only meaningful with a source

    def matches_for_source(self, state, source) -> bool:
        from mtgz_ir.game import counter_count

        n = counter_count(source, "level")
        return n >= self.lo and (self.hi is None or n <= self.hi)


@dataclass(frozen=True)
class LevelerPT(PTFormula):
    """Structured ``dynamic_pt_formula`` of a leveler (CR 711.2b): the
    base power / toughness printed in the level-symbol section whose
    range holds the creature's level-counter count, else its printed
    P/T. ``ranges`` is ``((lo, hi_or_None, power, toughness), ...)``.
    Read by ``game._dynamic_base_pt`` through :meth:`base_pt`."""

    ranges: tuple = ()

    def base_pt(self, state, obj) -> tuple[int, int]:
        from mtgz_ir.game import counter_count

        n = counter_count(obj, "level")
        for lo, hi, power, toughness in self.ranges:
            if n >= lo and (hi is None or n <= hi):
                return power, toughness
        return obj.printing.power or 0, obj.printing.toughness or 0
