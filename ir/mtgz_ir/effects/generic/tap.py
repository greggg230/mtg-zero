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
class Tap(Effect):
    """Unified tap primitive driven by ``targets``.

    Sibling of :class:`Destroy` / :class:`Exile` for the tap family.
    The unified resolve walks each slot's chosen target, re-verifies
    legality at resolution per CR 608.2b, and taps whichever survive
    the recheck via ``turn.tap_permanent`` (no-op if already tapped).

    Variants that carry extra rider effects (stun counters, "and
    stun it" clauses) override ``resolve`` rather than reusing this
    base — the bare ``Tap.resolve`` covers the simple "tap target X"
    oracle shape.
    """

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec
        from mtgz_ir.turn import tap_permanent

        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if isinstance(slot, SlotRef):
                    continue
                if not isinstance(slot, TargetSpec):
                    continue
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(
                        _fizzle_message(state, chosen, action="tap")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                tap_permanent(state, obj)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"tap {_describe_targets(self.targets)}"


@dataclass(frozen=True)
class Untap(Effect):
    """Unified untap primitive driven by ``targets``.

    Symmetric counterpart to :class:`Tap`: same target-driven shape,
    inverse mutation (``obj.tapped = False``). Re-verifies each
    slot's filter at resolution per CR 608.2b before flipping the
    target's tap state. Idempotent on already-untapped targets
    (silent no-op, no log entry).

    Bridge-only consumer today: round-23 BLOCKED-BRIDGE-ARM closes
    the cross-engine bridge for Rust's
    ``Effect::UntapTarget { target: TargetFilter }``
    (filter-narrow-by-design to ``any_creature`` per Rust's
    AnyPlayer-raises contract at state_ops_resolve.rs:1429-1434).
    Parser-side untap-target-creature shapes route through
    bespoke / specialized recognizers today; this class exists as
    the round-trippable target of the cross-engine translator.
    """

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec

        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if isinstance(slot, SlotRef):
                    continue
                if not isinstance(slot, TargetSpec):
                    continue
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(
                        _fizzle_message(state, chosen, action="untap")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                if not obj.tapped:
                    continue
                obj.tapped = False
                state.log.append(
                    f"{obj.printing.name} is untapped"
                )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"untap {_describe_targets(self.targets)}"


@dataclass(frozen=True)
class IfGainedLifeThisTurnUntapSelf(Effect):
    """Ragost, Deft Gastronaut's beginning-of-each-end-step rider:
    "If you gained life this turn, untap Ragost."

    Reads state.resolving_source for the permanent to untap. Checks
    the source-controller's life_gained_this_turn counter.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        p = state.player_by_id(source.controller_id)
        if p.life_gained_this_turn <= 0:
            return
        if source.tapped:
            source.tapped = False
            state.log.append(
                f"{source.printing.name} untaps ({p.life_gained_this_turn} "
                f"life gained this turn)"
            )

    def describe(self) -> str:
        return "if you gained life this turn, untap this permanent"



@dataclass(frozen=True)
class UntapTargetLand(Effect):
    """Deserted Temple: `{1}, {T}: Untap target land.`"""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.printing.is_land and o.tapped
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None or not obj.printing.is_land:
            return
        if not obj.tapped:
            return
        obj.tapped = False
        state.log.append(f"{obj.printing.name} is untapped")

    def describe(self) -> str:
        return "untap target land"



@dataclass(frozen=True)
class UntapTargetArtifactOrCreatureConditionalCounter(Effect):
    """Tezzeret, Cruel Captain's ``0`` loyalty ability — "Untap target
    artifact or creature. If it's an artifact creature, put a +1/+1
    counter on it."

    Target filter is artifact-OR-creature (no controller restriction —
    any artifact or creature on the battlefield is legal). On resolve:
    untap the target (silent no-op if already untapped), then — if the
    target is BOTH an artifact and a creature (an "artifact creature") —
    add one +1/+1 counter via ``add_plus_one_counters`` (which routes
    through the standard counter-doubling + counters-added trigger
    plumbing). Re-checks artifact-creature status at resolve time
    (CR 608.2b) so a type-change between activation and resolution is
    honored.

    Mirrors Rust's
    ``Effect::UntapTargetArtifactOrCreatureConditionalCounter``
    (cards.rs / state_ops_resolve.rs).
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_artifact_now

        return [
            ("permanent", o.id)
            for o in state.battlefield
            if is_artifact_now(state, o) or o.printing.is_creature
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
        # CR 608.2b — re-verify the target still matches the printed
        # filter (artifact or creature) at resolution.
        if not (is_artifact_now(state, obj) or obj.printing.is_creature):
            return
        if obj.tapped:
            obj.tapped = False
            state.log.append(f"{obj.printing.name} is untapped")
        # "If it's an artifact creature, put a +1/+1 counter on it."
        if is_artifact_now(state, obj) and obj.printing.is_creature:
            added = add_plus_one_counters(state, obj, 1)
            if added:
                state.log.append(
                    f"{obj.printing.name} gets a +1/+1 counter "
                    f"(now {obj.plus_one_counters})"
                )

    def describe(self) -> str:
        return (
            "untap target artifact or creature; if it's an artifact "
            "creature, put a +1/+1 counter on it"
        )



@dataclass(frozen=True)
class TapTargetOpponentPermanentAndStunIt(Effect):
    """Tap target artifact or creature an opponent controls, then put
    a stun counter on it. Matches Mechanozoa's ETB. Both effects apply
    to the same chosen target.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_artifact_now

        return [
            ("permanent", o.id)
            for o in state.battlefield
            if (is_artifact_now(state, o) or o.printing.is_creature)
            and o.controller_id != controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import tap_permanent

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
        if not obj.tapped:
            tap_permanent(state, obj)
        from mtgz_ir.game import add_stun_counters
        add_stun_counters(state, obj, 1)
        state.log.append(
            f"{obj.printing.name} tapped and gains a stun counter "
            f"(now {obj.stun_counters})"
        )

    def describe(self) -> str:
        return (
            "tap target artifact or creature an opponent controls and "
            "put a stun counter on it"
        )



@dataclass(frozen=True)
class TapAndStunUpToOneTargetCreature(Effect):
    """Tap up to one target creature and put ``stun_amount`` stun
    counters on it. Both effects apply to the same chosen target.

    Matches Meat Locker's "tap up to one target creature and put two
    stun counters on it." (DSK Room — door trigger). The "up to one"
    qualifier makes the target slot optional: ``target_count_range``
    flips to ``(0, 1)`` and ``requires_target`` flips to ``False`` so
    the cast-site target-selection pipeline allows zero-target casts.

    No controller restriction (unlike ``TapTargetOpponentPermanentAndStunIt``,
    which is opponent-only) and no taps-state restriction (unlike
    ``PutStunCounterOnTappedCreature``, which targets only already-tapped
    creatures): Meat Locker can tap any creature, friend or foe, tapped
    or untapped.
    """

    stun_amount: int = 2

    def requires_target(self) -> bool:
        return False

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import tap_permanent

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        if not obj.tapped:
            tap_permanent(state, obj)
        from mtgz_ir.game import add_stun_counters
        add_stun_counters(state, obj, self.stun_amount)
        state.log.append(
            f"{obj.printing.name} tapped and gains "
            f"{self.stun_amount} stun counters "
            f"(now {obj.stun_counters})"
        )

    def describe(self) -> str:
        return (
            f"tap up to one target creature and put {self.stun_amount} "
            f"stun counters on it"
        )



@dataclass(frozen=True)
class TapAnotherTargetCreature(Tap):
    """``Tap`` constructor sugar for "tap another target creature"
    (Starport Security's activated ability).

    Filter is ``IsCreature() & ~IsSource()`` — the ``~IsSource()``
    arm excludes the resolving source from its own legal-target pool
    so "another" reads CR-correct.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature, IsSource
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsCreature() & ~IsSource()),),
        )

    def describe(self) -> str:
        return "tap another target creature"



@dataclass(frozen=True)
class TapUpToOneTargetCreature(Tap):
    """``Tap`` constructor sugar for "tap up to one target creature"
    (Baseball Bat's equipped-attack rider).

    Filter is :class:`IsCreature` with ``count_min=0`` so the
    targeting pipeline treats the slot as optional — a 0-target
    resolution is a silent no-op (the controller chose not to
    target).
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsCreature(), count_min=0, count_max=1),),
        )

    def describe(self) -> str:
        return "tap up to one target creature"



@dataclass(frozen=True)
class TapTargetArtifactOrCreature(Tap):
    """``Tap`` constructor sugar for "tap target artifact or creature"
    (Mental Modulation's middle clause).

    Filter is ``IsArtifact() | IsCreature()``. Already-tapped targets
    are no-ops via the underlying ``turn.tap_permanent``.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsArtifact, IsCreature
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsArtifact() | IsCreature()),),
        )

    def describe(self) -> str:
        return "tap target artifact or creature"



@dataclass(frozen=True)
class UntapTargetOtherPermanent(Effect):
    """Untap target permanent other than the source.

    Matches Nanoform Sentinel's "untap another target permanent" trigger.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        source = state.resolving_source
        source_id = source.id if source is not None else -1
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.id != source_id and o.tapped
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
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
        if not obj.tapped:
            return
        obj.tapped = False
        state.log.append(f"{obj.printing.name} is untapped")

    def describe(self) -> str:
        return "untap another target permanent"



@dataclass(frozen=True)
class PhaseOutEquippedCreature(Effect):
    """"Equipped creature phases out." — Robe of Stars' Astral Projection.

    Reads the Equipment source from ``state.resolving_source`` and
    phases out whatever creature it's attached to. Treat the creature
    (and anything attached to it) as though they don't exist until its
    controller's next turn — simplified here to filtering out of
    combat damage only; triggers/anthems/targeting still see phased
    objects in this engine's current scope.

    If the Equipment isn't attached to anything, the ability silently
    does nothing.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None or source.attached_to is None:
            return
        creature = next(
            (o for o in state.battlefield if o.id == source.attached_to),
            None,
        )
        if creature is None:
            return
        creature.phased_out = True
        # Remove it from ongoing combat so the filter in
        # _resolve_damage_step is redundant-but-safe.
        creature.attacking = False
        creature.blocking = None
        creature.blocked_by = []
        state.log.append(
            f"{creature.printing.name} phases out (Astral Projection)"
        )

    def describe(self) -> str:
        return "equipped creature phases out"



@dataclass(frozen=True)
class TapEnchantedPermanent(Effect):
    """Tap the source Aura's enchanted permanent.

    Used by Stay Hidden, Stay Silent's ETB ``When this Aura enters,
    tap enchanted creature.`` Identifies the host via
    ``state.resolving_source.attached_to`` and routes through
    ``mtgz_ir.turn.tap_permanent`` so the tap goes through the same
    pipeline an action-tap would. Mirrors
    ``DestroyEnchantedPermanent`` 's host-resolution pattern; safe to
    re-use for any Aura ETB that taps the host.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import tap_permanent

        aura = state.resolving_source
        if aura is None or aura.attached_to is None:
            return
        host = next(
            (o for o in state.battlefield if o.id == aura.attached_to),
            None,
        )
        if host is None:
            return
        if host.tapped:
            return
        state.log.append(
            f"{host.printing.name} is tapped by {aura.printing.name}"
        )
        tap_permanent(state, host)

    def describe(self) -> str:
        return "tap enchanted permanent"
