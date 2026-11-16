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
class Destroy(Effect):
    """Unified destroy primitive driven by ``targets``.

    Replaces the per-class ``legal_targets`` / ``resolve`` overrides
    on the simple destroy variants (``DestroyTargetArtifact``,
    ``DestroyTargetLand``, ``DestroyTargetNoncreaturePermanent``,
    ``DestroyTargetNonlandPermanent``, ``DestroyTargetArtifactOrTappedCreature``,
    ``DestroyTargetArtifactEnchantmentOrFlyingCreature``). The
    ``targets`` field declares legal-target shape via the
    ``TargetFilter`` grammar; the resolve walk re-verifies legality
    at fire time per CR 608.2b and destroys whichever slots survive
    the recheck.

    ``DestroyTargetCreature`` keeps its rider-class shape (Faller's
    Faithful's "if it wasn't dealt damage this turn, draws N") because
    the rider isn't expressible in the bare-grammar model — the rider
    semantically belongs to a sibling effect that pronouns "that
    creature". When the EffectSequence + SlotRef pillar lands the
    rider can move out into a follow-up effect; for now it's a
    ``Destroy`` subclass with extra fields.
    """

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec
        from mtgz_ir.turn import destroy

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
                        _fizzle_message(state, chosen, action="destroy")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                state.log.append(f"{obj.printing.name} is destroyed")
                destroy(state, obj)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"destroy {_describe_targets(self.targets)}"



@dataclass(frozen=True)
class DestroyTargetCreature(Destroy):
    """``Destroy`` constructor sugar for "destroy target creature" with
    flag-driven filter widening and a Faller's-Faithful-style draw
    rider.

    Filter composition:
      - base: ``IsCreature()``
      - ``or_spacecraft=True``: union with ``HasSubtype("Spacecraft")``
        (Embrace Oblivion's "creature or Spacecraft")
      - ``or_enchantment=True``: union with ``IsEnchantment()``
        (Withering Torment's "creature or enchantment")
      - ``exclude_self=True``: intersect with ``~IsSource()`` so the
        trigger source isn't its own legal target ("another target")

    Rider: ``draw_if_undamaged_amount > 0`` adds a "if it wasn't dealt
    damage this turn, its controller draws N" follow-up — Faller's
    Faithful. The check snapshots ``obj.damaged_this_turn`` BEFORE the
    destroy because leaving the battlefield clears the flag. The
    drawing player is the destroyed creature's controller per the
    printed text. The custom resolve overrides ``Destroy.resolve``
    so the rider runs alongside the destruction; future migration
    will lift the rider into a sibling effect with a ``SlotRef`` back-
    reference once the EffectSequence consumer pillar lands.

    ``optional=True`` flips the slot's ``count_min`` to 0 ("up to one
    other target" / Faller's Faithful's optional shape).
    """

    draw_if_undamaged_amount: int = 0
    or_spacecraft: bool = False
    or_enchantment: bool = False
    optional: bool = False
    exclude_self: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            HasSubtype, IsCreature, IsEnchantment, IsSource,
        )
        from mtgz_ir.effects.target_filter import OneOf
        from mtgz_ir.effects.targets import TargetSpec
        arms: list = [IsCreature()]
        if self.or_spacecraft:
            arms.append(HasSubtype("Spacecraft"))
        if self.or_enchantment:
            arms.append(IsEnchantment())
        f = arms[0] if len(arms) == 1 else OneOf(*arms)
        if self.exclude_self:
            f = f & ~IsSource()
        spec = TargetSpec(
            f,
            count_min=0 if self.optional else 1,
            count_max=1,
        )
        object.__setattr__(self, "targets", (spec,))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec
        from mtgz_ir.turn import destroy, draw_n_cards

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
                        _fizzle_message(state, chosen, action="destroy")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                was_undamaged = not obj.damaged_this_turn
                victim_controller_id = obj.controller_id
                state.log.append(f"{obj.printing.name} is destroyed")
                destroy(state, obj)
                if self.draw_if_undamaged_amount > 0 and was_undamaged:
                    draw_n_cards(
                        state, victim_controller_id,
                        self.draw_if_undamaged_amount,
                    )
                    state.log.append(
                        f"P{victim_controller_id} draws "
                        f"{self.draw_if_undamaged_amount} "
                        f"(target wasn't dealt damage this turn)"
                    )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        suffix_parts: list[str] = []
        if self.or_spacecraft:
            suffix_parts.append("Spacecraft")
        if self.or_enchantment:
            suffix_parts.append("enchantment")
        suffix = (" or " + " or ".join(suffix_parts)) if suffix_parts else ""
        base = f"destroy target creature{suffix}"
        if self.draw_if_undamaged_amount > 0:
            return (
                f"{base}; if it wasn't dealt damage this turn, its "
                f"controller draws {self.draw_if_undamaged_amount} card(s)"
            )
        return base



@dataclass(frozen=True)
class DestroyEnchantedPermanent(Effect):
    """Destroy the permanent enchanted by the resolving Aura.

    Untargeted: the source is the aura (read from
    ``state.resolving_source``), and the affected permanent is whatever
    that aura is currently attached to. Used by Cryoshatter's
    "when enchanted creature becomes tapped, destroy it" trigger.

    If the aura is no longer on the battlefield or has no host (e.g.,
    the host already left), the effect fizzles.
    """

    def requires_target(self) -> bool:
        return False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import destroy

        aura = state.resolving_source
        if aura is None or aura.attached_to is None:
            return
        host = next(
            (o for o in state.battlefield if o.id == aura.attached_to),
            None,
        )
        if host is None:
            return
        state.log.append(
            f"{host.printing.name} is destroyed by {aura.printing.name}"
        )
        destroy(state, host)

    def describe(self) -> str:
        return "destroy enchanted permanent"



@dataclass(frozen=True)
class DestroyTargetArtifact(Destroy):
    """``Destroy`` constructor sugar for "destroy target artifact".

    Filter is :class:`IsArtifact`. The unified ``Destroy.resolve``
    re-verifies the slot's filter at resolution per CR 608.2b — a
    target that lost the Artifact card type between cast and resolve
    fails the recheck and the destruction is skipped.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsArtifact
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(self, "targets", (TargetSpec(IsArtifact()),))



@dataclass(frozen=True)
class DestroyTargetNoncreaturePermanent(Destroy):
    """``Destroy`` constructor sugar for "destroy target noncreature
    permanent".

    Filter is ``~IsCreature() & ~IsPlayer()``. ``IsCreature`` reads
    ``is_creature_now``, so an Impending-suppressed Overlord (which
    is currently an enchantment, not a creature) IS a legal target —
    matches the legacy ``not is_creature_now(o)`` enumeration.
    Players are excluded by the ``~IsPlayer()`` arm.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature, IsPlayer
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(~IsCreature() & ~IsPlayer()),),
        )

    def describe(self) -> str:
        return "destroy target noncreature permanent"



@dataclass(frozen=True)
class DestroyTargetLand(Destroy):
    """``Destroy`` constructor sugar for "destroy target land" /
    "destroy target nonbasic land".

    ``nonbasic_only=True`` narrows the filter from ``IsLand()`` to
    ``IsLand() & ~IsBasic()`` — Strip Mine's bare form vs Dust Bowl's
    nonbasic-restricted form. The unified ``Destroy.resolve`` re-
    verifies the slot's filter at resolution.
    """

    nonbasic_only: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsBasic, IsLand
        from mtgz_ir.effects.targets import TargetSpec
        f = IsLand() & ~IsBasic() if self.nonbasic_only else IsLand()
        object.__setattr__(self, "targets", (TargetSpec(f),))

    def describe(self) -> str:
        return (
            "destroy target nonbasic land"
            if self.nonbasic_only
            else "destroy target land"
        )



@dataclass(frozen=True)
class DestroyTargetNonlandPermanent(Destroy):
    """``Destroy`` constructor sugar for "destroy target nonland
    permanent".

    Filter is ``~IsLand() & ~IsPlayer()`` so the target pool covers
    every battlefield permanent that isn't a land while excluding
    players (which the unified target pipeline walks alongside the
    battlefield). Indestructible permanents survive at resolve time
    via the ``destroy`` helper's indestructible check.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsLand, IsPlayer
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(~IsLand() & ~IsPlayer()),),
        )

    def describe(self) -> str:
        return "destroy target nonland permanent"


@dataclass(frozen=True)
class DestroyUpToOneTargetNonlandPermanent(Destroy):
    """``Destroy`` constructor sugar for "destroy up to one target nonland
    permanent".

    Filter is ``~IsLand() & ~IsPlayer()`` with ``count_min=0`` so the
    target is optional (0 or 1). When the controller selects no target the
    Destroy.resolve early-return fires cleanly — no crash, no wasted state.
    Syr Vondam, Sunstar Exemplar's self-leaves trigger uses this shape.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsLand, IsPlayer
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(~IsLand() & ~IsPlayer(), count_min=0),),
        )

    def describe(self) -> str:
        return "destroy up to one target nonland permanent"


@dataclass(frozen=True)
class RegenerateTargetCreature(Effect):
    """"Regenerate target creature" — sets ``regeneration_shield``
    on the target. The next time this turn state-based actions would
    destroy the creature from lethal damage (or a destroy effect;
    NOT from toughness <= 0), the destruction is replaced by:
    remove all damage, tap the creature, remove it from combat.
    The shield is consumed on use and cleared at cleanup if unused.

    ``allowed_subtypes``: when non-empty, restricts legal targets to
    creatures whose printed subtypes overlap with the set. Matches
    Swarmyard's "target Rat, Spider, Squirrel, Insect, or Snake."
    Empty tuple = any creature (not used by current cards).
    """

    allowed_subtypes: frozenset[str] = frozenset()

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and (
                not self.allowed_subtypes
                or any(
                    s in self.allowed_subtypes
                    for s in _eff_subtypes(state, o)
                )
            )
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        _, tid = targets[0]
        obj = next(
            (o for o in state.battlefield if o.id == tid), None
        )
        if obj is None:
            return
        obj.regeneration_shield = True
        state.log.append(
            f"{obj.printing.name} gains a regeneration shield"
        )

    def describe(self) -> str:
        if self.allowed_subtypes:
            subs = ", ".join(sorted(self.allowed_subtypes))
            return f"regenerate target {subs}"
        return "regenerate target creature"



@dataclass(frozen=True)
class DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters(Effect):
    """Blast Zone payoff: "Destroy each nonland permanent with mana
    value equal to the number of charge counters on this land."

    Reads ``state.resolving_source.charge_counters`` (snapshotted at
    resolution time). Tokens count too — they have a printed mana
    value of 0 and are destroyed when the source has 0 charge
    counters (rare edge case but oracle-compliant).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        source = state.resolving_source
        if source is None:
            return
        target_mv = source.charge_counters
        # Snapshot the list — destroying mutates state.battlefield.
        for obj in list(state.battlefield):
            if obj.printing.is_land:
                continue
            if obj.printing.mana_cost.total != target_mv:
                continue
            if "indestructible" in obj.printing.keywords:
                continue
            state.log.append(
                f"{obj.printing.name} is destroyed by Blast Zone "
                f"(MV={target_mv})"
            )
            move_off_battlefield(state, obj, destination="graveyard")

    def describe(self) -> str:
        return (
            "destroy each nonland permanent with mana value equal to "
            "the number of charge counters on this land"
        )



@dataclass(frozen=True)
class DestroyTargetArtifactOrTappedCreature(Destroy):
    """``Destroy`` constructor sugar for "destroy target artifact, or
    tapped creature".

    Filter is ``IsArtifact() | (IsCreature() & IsTapped())``. The
    unified ``Destroy.resolve`` re-verifies the slot's filter at
    resolution per CR 608.2b — a creature that untapped between cast
    and resolve fails the recheck (an artifact target is unaffected
    by the untap and stays legal). Matches Radiant Strike's destroy
    clause.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            IsArtifact, IsCreature, IsTapped,
        )
        from mtgz_ir.effects.targets import TargetSpec
        f = IsArtifact() | (IsCreature() & IsTapped())
        object.__setattr__(self, "targets", (TargetSpec(f),))

    def describe(self) -> str:
        return "destroy target artifact or tapped creature"



@dataclass(frozen=True)
class DestroyTargetArtifactEnchantmentOrFlyingCreature(Destroy):
    """``Destroy`` constructor sugar for "destroy target artifact,
    enchantment, or creature with flying".

    Filter is ``IsArtifact() | IsEnchantment() | (IsCreature() &
    HasKeyword("flying"))``. ``HasKeyword`` reads
    ``effective_keywords`` so anthem-granted / temp-granted flying
    counts toward legality. Matches Shattered Wings' destroy clause.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            HasKeyword, IsArtifact, IsCreature, IsEnchantment,
        )
        from mtgz_ir.effects.targets import TargetSpec
        f = (
            IsArtifact()
            | IsEnchantment()
            | (IsCreature() & HasKeyword("flying"))
        )
        object.__setattr__(self, "targets", (TargetSpec(f),))

    def describe(self) -> str:
        return (
            "destroy target artifact, enchantment, or creature with flying"
        )



@dataclass(frozen=True)
class DestroyAllCreatures(Effect):
    """Destroy every creature on the battlefield. Wrath-style effect.

    Indestructible creatures survive (routes each kill through
    turn.destroy()). Tokens cease to exist normally.

    ``tapped_only`` / ``untapped_only`` narrow the wrath to only
    creatures matching that tap-state (Split Up's modal "destroy all
    tapped creatures" / "destroy all untapped creatures"). Mutually
    exclusive — at most one may be True.

    ``min_power`` (default 0 = no filter) narrows the wrath to only
    creatures whose ``effective_power`` is at least that threshold —
    Restricted Office's "destroy all creatures with power 3 or
    greater". The threshold is read against ``effective_power`` so
    any layer-7 pump or counters apply at resolve time. The default
    (all filters off) is the unrestricted Wrath shape.
    """

    tapped_only: bool = False
    untapped_only: bool = False
    min_power: int = 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import destroy
        from mtgz_ir.game import effective_power

        victims = [o for o in state.battlefield if o.printing.is_creature]
        if self.tapped_only:
            victims = [o for o in victims if o.tapped]
        elif self.untapped_only:
            victims = [o for o in victims if not o.tapped]
        if self.min_power > 0:
            victims = [
                o for o in victims
                if effective_power(state, o) >= self.min_power
            ]
        for obj in victims:
            state.log.append(f"{obj.printing.name} is destroyed (wrath)")
            destroy(state, obj)

    def describe(self) -> str:
        if self.tapped_only:
            return "destroy all tapped creatures"
        if self.untapped_only:
            return "destroy all untapped creatures"
        if self.min_power > 0:
            return (
                f"destroy all creatures with power {self.min_power} or "
                f"greater"
            )
        return "destroy all creatures"



@dataclass(frozen=True)
class DestroyTargetNonlandPermanentWithLanderForController(Effect):
    """Destroy target nonland permanent. Its (former) controller creates a
    Lander token.

    Matches Emergency Eject's oracle text. Uses a new "permanent" target kind
    (any non-land battlefield object) so this can hit creatures, artifacts,
    and enchantments uniformly.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if not o.printing.is_land
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import destroy

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
        former_controller = obj.controller_id
        state.log.append(f"{obj.printing.name} is destroyed")
        if not destroy(state, obj):
            return  # indestructible: no Lander consolation
        lander = state.new_object(
            TOKEN_PRINTINGS["Lander"],
            owner_id=former_controller,
            zone=Zone.BATTLEFIELD,
        )
        state.log.append(f"P{former_controller} creates a Lander token")
        put_into_battlefield(state, lander, former_controller)

    def describe(self) -> str:
        return (
            "destroy target nonland permanent; its controller creates a "
            "Lander token"
        )
