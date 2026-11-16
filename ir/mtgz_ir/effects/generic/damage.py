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
class DealDamage(Effect):
    """Unified deal-damage primitive driven by ``targets``.

    Replaces the eleven pre-PR-#3 deal-damage recognizer-classes at
    the static-amount level (``DealDamageAnyTarget``,
    ``DealDamageToTargetCreature``, ``DealDamageToOpponentCreature``,
    ``DealDamageToTargetTappedCreature``,
    ``DealDamageToTargetOpponent``). The ``targets`` field — typically
    a single ``TargetSpec`` for damage today — declares the legal-
    target shape via the ``TargetFilter`` grammar; the resolve walk
    re-verifies legality at fire time per CR 608.2b and applies the
    damage to whichever slots survive.

    Bespoke-amount damage classes (count-source artifact / cards-in-
    hand, X-base-plus-creature-count Focus Fire, excess-rider
    Lander, Spacecraft-boosted) keep their own engine classes —
    they need bespoke amount logic — but consult ``targets[0].filter``
    via ``Effect._enumerate_for_filter`` to share the same target-
    grammar plumbing.
    """

    amount: int = 0
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        # Re-verify each chosen target against its slot's filter at
        # resolution per CR 608.2b. Slots whose object is no longer
        # legal (creature untapped, blocking creature stopped blocking,
        # etc.) log a fizzle line and skip — the user-visible feedback
        # mirrors the legacy per-class messages ("is no longer tapped
        # — damage fizzles") with a generic phrasing the unified
        # dispatcher can produce without introspecting the filter.
        from mtgz_ir.effects.targets import SlotRef, TargetSpec
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if isinstance(slot, SlotRef):
                    # Back-references aren't in the targets tuple
                    # under PR #3 — defensively skip.
                    continue
                if not isinstance(slot, TargetSpec):
                    continue
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(_fizzle_message(state, chosen))
                    continue
                _apply_damage(state, chosen, self.amount)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"deals {self.amount} damage to {_describe_targets(self.targets)}"


@dataclass(frozen=True)
class DealDamageToTargetOpponent(DealDamage):
    """``DealDamage`` constructor sugar for "deals N damage to target
    opponent".

    Filter is ``IsPlayer() & ControlledByOpponent()``; the
    ``ControlledByOpponent`` arm makes the spell's caster (the
    "you" in MTG parlance) ineligible.

    NB: ``amount`` defaults to 1 to mirror the legacy class's default;
    most call sites pass an explicit value but a few constructed it
    bare.
    """

    amount: int = 1

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            ControlledByOpponent, IsPlayer,
        )
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsPlayer() & ControlledByOpponent()),),
        )



@dataclass(frozen=True)
class DealNDamageToEachCreature(Effect):
    """Deal ``amount`` damage to each creature on the battlefield.

    Snapshot semantics: the victim list is captured before any damage
    is applied, so any deaths mid-pass don't affect what gets damaged.
    Marks damage via _apply_damage (the same helper used by targeted
    burn), which routes through creature toughness for SBAs.
    """

    amount: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        victims = [
            o for o in state.battlefield if o.printing.is_creature
        ]
        for o in victims:
            _apply_damage(state, ("creature", o.id), self.amount)

    def describe(self) -> str:
        return f"deals {self.amount} damage to each creature"



@dataclass(frozen=True)
class DealDamageEachOpponent(Effect):
    """Deal ``amount`` damage to each living opponent. Untargeted — mass
    burn half of Ruinous Rampage's choice.
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import player_damage_prevented
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            if player_damage_prevented(state, p.id):
                continue
            p.life -= self.amount
            state.log.append(
                f"P{p.id} takes {self.amount} (life: {p.life})"
            )

    def describe(self) -> str:
        return f"deals {self.amount} damage to each opponent"



@dataclass(frozen=True)
class DealDamageToControllerAndAddMana(Effect):
    """Ancient Tomb's mana ability: "{T}: Add {C}{C}. This land deals
    2 damage to you." Two-step: self-damage + pool fill. Paired with
    is_mana_ability=True so it bypasses the stack.
    """

    damage: int = 2
    produces: tuple[tuple[str, int], ...] = (("C", 2),)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin, player_damage_prevented

        p = state.player_by_id(controller_id)
        if not player_damage_prevented(state, controller_id):
            p.life -= self.damage
        state.log.append(
            f"{state.resolving_source.printing.name if state.resolving_source else 'land'} "
            f"deals {self.damage} to P{controller_id} (life: {p.life})"
        )
        for color, count in self.produces:
            for _ in range(count):
                p.mana_pool.append(ManaCoin(color=color))

    def describe(self) -> str:
        return (
            f"add "
            + "".join("{" + c + "}" for c, n in self.produces for _ in range(n))
            + f"; this land deals {self.damage} damage to you"
        )



@dataclass(frozen=True)
class DealDamageEqualToSourcePowerAnyTarget(Effect):
    """Source deals damage equal to its (effective) power to any target
    (creature or player).

    Matches "this creature deals damage equal to its power to any
    target" — Infernal Phantom's dies trigger in DSK. The amount is
    snapshot from ``state.resolving_source.effective_power`` at resolve
    time. For a dies-trigger source the GameObject is already in the
    graveyard but ``state.resolving_source`` still references the
    pre-death object so its layer-7 power can be read; if the power is
    nonpositive the damage is a no-op (CR 119.5: "if the amount of
    damage that would be dealt is 0 or less, no damage is dealt").

    Bespoke amount (driven by source power) so the class survives the
    PR #3 collapse, but the legal-target pool now consults
    ``self.targets[0].filter`` via the structural ``TargetFilter``
    grammar — same any-target expansion ``DealDamage`` uses.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.targets import TargetSpec, any_target_filter
            object.__setattr__(
                self, "targets",
                (TargetSpec(any_target_filter()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power
        if not targets:
            return
        source = state.resolving_source
        if source is None:
            return
        amount = effective_power(state, source)
        if amount <= 0:
            return
        if not _slot_target_still_legal(state, self.targets[0], targets[0]):
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            _apply_damage(state, targets[0], amount)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to this creature's power to any target"
        )



@dataclass(frozen=True)
class DealDamageAnyTarget(DealDamage):
    """``DealDamage`` constructor sugar for "deals N damage to (up to
    one) any target".

    Identical at the engine level to
    ``DealDamage(amount=N, targets=(TargetSpec(any_target_filter()),))``;
    the subclass is sugar so the parser can emit a self-describing
    name in the AST and existing parser tests' equality assertions
    keep their per-shape granularity. No behavioural overrides — every
    code path is the unified ``DealDamage`` resolve.

    ``optional=True`` flips the slot's ``count_min`` to 0 so the
    targeting pipeline treats the slot as ``0..1`` ("up to one
    target"); default ``False`` is the bare 1..1 "any target" form.
    Mirrors the optional-slot pattern on
    ``DealDamageToTargetCreature`` (CR 601.2c "up to N targets"
    reading); extends that shape to the wider "any target" filter for
    cards like Pinnacle Kill-Ship ("deals 10 damage to up to one
    target creature" — the creature-only restriction is observably
    indistinguishable from "any target" on probes that drive a single
    opponent creature on bf, because Rust's deterministic pick scans
    for the first creature first and only falls back to defender-
    player when no creature exists).
    """

    optional: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter
        # Frozen dataclass: bypass __setattr__ via __dict__ write.
        spec = TargetSpec(
            any_target_filter(),
            count_min=0 if self.optional else 1,
            count_max=1,
        )
        object.__setattr__(self, "targets", (spec,))


@dataclass(frozen=True)
class DealDamageToTargetCreature(DealDamage):
    """``DealDamage`` constructor sugar for "deals N damage to (up to
    one) target creature".

    ``optional=True`` flips the slot's ``count_min`` to 0 so the
    targeting pipeline treats the slot as ``0..1`` ("up to one
    target creature"); default ``False`` is the bare 1..1 form.
    """

    optional: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature
        from mtgz_ir.effects.targets import TargetSpec
        spec = TargetSpec(
            IsCreature(),
            count_min=0 if self.optional else 1,
            count_max=1,
        )
        object.__setattr__(self, "targets", (spec,))



@dataclass(frozen=True)
class DealDamageToTargetCreatureWithExcessLanderRider(Effect):
    """Orbital Plunge `{3}{R}`:
    "Orbital Plunge deals 6 damage to target creature. If excess damage
    was dealt this way, create a Lander token."

    Excess damage is the portion of the 6 that exceeded what was needed
    to mark lethal on the target — ``damage_amount - max(0, effective
    toughness - damage already marked)``. We check >0 after the damage
    is applied. Lifelink/deathtouch are not modeled here; in real MTG
    "excess" is resolved through rule 120.3e which handles that, but
    for our simplified model, comparing amount to remaining toughness
    is sufficient.

    Bespoke amount-and-rider so the class survives PR #3's damage
    collapse; legal-target pool now consults
    ``self.targets[0].filter`` (``IsCreature()``) via the structural
    target-grammar plumbing.
    """

    amount: int = 6
    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import IsCreature
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets", (TargetSpec(IsCreature()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone, effective_toughness

        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            kind, target_id = targets[0]
            obj = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
            excess = 0
            if obj is not None:
                remaining = max(0, effective_toughness(state, obj) - obj.damage)
                excess = max(0, self.amount - remaining)
            _apply_damage(state, targets[0], self.amount)
            if excess > 0:
                lander = state.new_object(
                    TOKEN_PRINTINGS["Lander"],
                    owner_id=controller_id,
                    zone=Zone.BATTLEFIELD,
                )
                put_into_battlefield(state, lander, controller_id)
                state.log.append(
                    f"Orbital Plunge dealt {excess} excess damage; "
                    f"P{controller_id} creates a Lander token"
                )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            f"deals {self.amount} damage to target creature; if excess "
            f"damage was dealt this way, create a Lander token"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreatureByArtifactCount(Effect):
    """Deal damage equal to the number of artifacts you control to
    target creature an opponent controls.

    Matches Warmaker Gunship's ETB. Counts include lands that are also
    artifacts (there aren't any in EoE) and tokens like Lander.

    Bespoke amount (artifact count) so the class survives PR #3's
    damage collapse, but the legal-target pool now consults
    ``self.targets[0].filter`` via the structural ``TargetFilter``
    grammar — ``IsCreature() & ControlledByOpponent()``.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                ControlledByOpponent, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(IsCreature() & ControlledByOpponent()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now

        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            artifact_count = sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id and is_artifact_now(state, o)
            )
            if artifact_count <= 0:
                return
            _apply_damage(state, targets[0], artifact_count)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to the number of artifacts you control to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreatureByCardsInHand(Effect):
    """Deal damage equal to the number of cards in your hand to target
    creature an opponent controls.

    Matches Roaring Furnace's door trigger oracle: "this Room deals
    damage equal to the number of cards in your hand to target creature
    an opponent controls". The hand size is read at resolve time from
    the controller's ``hand`` (post any cast/pay-cost effects, mirroring
    MTG's resolution-time evaluation of the count).

    The Room itself is NOT in the controller's hand at resolution time
    (it's already on the battlefield by the time the door trigger
    resolves), so a hand-of-N + the cast Room produces N damage, not
    N+1 — same hand count an opponent or a third-party observer would
    see at resolve time.

    Bespoke amount (hand size) so the class survives PR #3's damage
    collapse, but the legal-target pool now consults
    ``self.targets[0].filter`` — ``IsCreature() & ControlledByOpponent()``.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                ControlledByOpponent, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(IsCreature() & ControlledByOpponent()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            hand_size = len(state.player_by_id(controller_id).hand)
            if hand_size <= 0:
                return
            _apply_damage(state, targets[0], hand_size)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to the number of cards in your hand to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreatureByLandsControlled(Effect):
    """Deal damage equal to the number of lands you control to target
    creature an opponent controls.

    BLOCKED-RUST-EFFECT slice (round 36) -- sister of
    ``DealDamageToOpponentCreatureByArtifactCount`` (round 32) and
    ``DealDamageToOpponentCreatureByCardsInHand`` (round 35). Same
    apply substrate (damage to first eligible opponent creature,
    target legality re-checked on slot), NEW denominator (count of
    permanents the controller controls whose ``printing.is_land`` is
    True, in place of artifact-count / hand-size). Establishes a
    THIRD orthogonal X-from-count denominator on the round-32 damage
    substrate, completing the artifact-count / hand-size /
    lands-controlled triad on this apply substrate.

    Dual-typed artifact-lands count toward BOTH the artifact-count and
    the lands-controlled denominators independently (mirrors Scryfall
    typeline conventions; the ``is_land`` and ``is_artifact`` printing
    properties are independent disjunctions over ``card_types``).

    No EoE consumers today -- ships per round-27/31
    round-trip-surface-coverage rule (each bridge arm is load-bearing
    for the round-trip + the Rust resolve substrate completes the
    denominator-axis triad). Bespoke amount (lands count) so the class
    survives PR #3's damage collapse, but the legal-target pool now
    consults ``self.targets[0].filter`` via the structural
    ``TargetFilter`` grammar -- ``IsCreature() & ControlledByOpponent()``.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                ControlledByOpponent, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(IsCreature() & ControlledByOpponent()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            lands_count = sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id and o.printing.is_land
            )
            if lands_count <= 0:
                return
            _apply_damage(state, targets[0], lands_count)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to the number of lands you control to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreatureByCreaturesYouControl(Effect):
    """Deal damage equal to the number of creatures you control to target
    creature an opponent controls.

    BLOCKED-RUST-EFFECT slice (round 37) -- sister of
    ``DealDamageToOpponentCreatureByArtifactCount`` (round 32),
    ``DealDamageToOpponentCreatureByCardsInHand`` (round 35), and
    ``DealDamageToOpponentCreatureByLandsControlled`` (round 36). Same
    apply substrate (damage to first eligible opponent creature,
    target legality re-checked on slot), NEW denominator (count of
    permanents the controller controls whose ``printing.is_creature``
    is True, in place of artifact-count / hand-size / lands-controlled).
    Establishes a FOURTH orthogonal X-from-count denominator on the
    round-32 damage substrate, completing the artifact-count /
    hand-size / lands-controlled / creatures-controlled quadruplet on
    this apply substrate.

    Dual-typed artifact-creatures count toward BOTH the artifact-count
    and the creatures-controlled denominators independently (mirrors
    Scryfall typeline conventions; the ``is_artifact`` and
    ``is_creature`` printing properties are independent disjunctions
    over ``card_types``).

    The source itself counts toward the denominator if it is a Creature
    still on the source-controller's bf at resolve time (vanilla walk
    over ``state.battlefield`` -- no source-exclusion clause).

    No EoE consumers today -- ships per round-27/31
    round-trip-surface-coverage rule (each bridge arm is load-bearing
    for the round-trip + the Rust resolve substrate completes the
    denominator-axis quadruplet). Bespoke amount (creatures count) so
    the class survives PR #3's damage collapse, but the legal-target
    pool now consults ``self.targets[0].filter`` via the structural
    ``TargetFilter`` grammar -- ``IsCreature() & ControlledByOpponent()``.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                ControlledByOpponent, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(IsCreature() & ControlledByOpponent()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            creatures_count = sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id and o.printing.is_creature
            )
            if creatures_count <= 0:
                return
            _apply_damage(state, targets[0], creatures_count)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to the number of creatures you control to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreatureByCardsInGraveyard(Effect):
    """Deal damage equal to the number of cards in your graveyard to
    target creature an opponent controls.

    BLOCKED-RUST-EFFECT slice (round 46) -- sister of
    ``DealDamageToOpponentCreatureByArtifactCount`` (round 32),
    ``DealDamageToOpponentCreatureByCardsInHand`` (round 35),
    ``DealDamageToOpponentCreatureByLandsControlled`` (round 36), and
    ``DealDamageToOpponentCreatureByCreaturesYouControl`` (round 37).
    Same apply substrate (damage to first eligible opponent creature,
    target legality re-checked on slot), NEW denominator (size of the
    controller's graveyard zone, in place of artifact-count / hand-size
    / lands-controlled / creatures-controlled). Establishes a FIFTH
    orthogonal X-from-count denominator on the round-32 damage
    substrate, completing the artifact-count / hand-size /
    lands-controlled / creatures-controlled / cards-in-graveyard
    quintuplet on this apply substrate. SIXTH overall X-from-count
    denominator across the substrate matrix (prior five:
    artifact-count / hand-size / lands-controlled /
    creatures-controlled / life-total -- round-42 mill uses life
    total). FIRST zone-size denominator pulled from a non-battlefield,
    non-hand source.

    The source itself is NOT in the controller's graveyard at
    resolution time for an ETB-style trigger (it's on the battlefield),
    so a graveyard-of-N + the cast source produces N damage, not N+1.
    For LTB-style or graveyard-resolve triggers where the source has
    moved to the graveyard before this arm runs, the source WOULD
    contribute +1 by virtue of the move (no source-exclusion clause,
    mirroring round-32 / 35 / 36 / 37 sister "vanilla walk -- no
    source-exclusion clause" convention).

    No EoE consumers today -- ships per round-27/31/37/45
    round-trip-surface-coverage rule (each bridge arm is load-bearing
    for the round-trip + the Rust resolve substrate completes the
    denominator-axis quintuplet). Bespoke amount (graveyard size) so
    the class survives PR #3's damage collapse, but the legal-target
    pool now consults ``self.targets[0].filter`` via the structural
    ``TargetFilter`` grammar -- ``IsCreature() & ControlledByOpponent()``.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                ControlledByOpponent, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(IsCreature() & ControlledByOpponent()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            gy_size = len(state.player_by_id(controller_id).graveyard)
            if gy_size <= 0:
                return
            _apply_damage(state, targets[0], gy_size)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to the number of cards in your graveyard to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreatureByLifeTotal(Effect):
    """Deal damage equal to your life total to target creature an
    opponent controls.

    BLOCKED-RUST-EFFECT slice (round 47) -- sister of
    ``DealDamageToOpponentCreatureByArtifactCount`` (round 32),
    ``DealDamageToOpponentCreatureByCardsInHand`` (round 35),
    ``DealDamageToOpponentCreatureByLandsControlled`` (round 36),
    ``DealDamageToOpponentCreatureByCreaturesYouControl`` (round 37),
    and ``DealDamageToOpponentCreatureByCardsInGraveyard`` (round 46).
    Same apply substrate (damage to first eligible opponent creature,
    target legality re-checked on slot), NEW denominator (the
    controller's clamped life total, in place of artifact-count /
    hand-size / lands-controlled / creatures-controlled /
    cards-in-graveyard). Establishes a SIXTH (final) orthogonal
    X-from-count denominator on the round-32 damage substrate,
    COMPLETING the artifact-count / hand-size / lands-controlled /
    creatures-controlled / cards-in-graveyard / life-total sextuplet
    on this apply substrate -- the damage-to-opponent-creature row of
    the 6-substrate x 6-denominator X-from-count matrix is now fully
    saturated.

    Life-total is the FIRST non-zone-size, scalar-read denominator on
    the damage substrate (the prior five all read a collection size;
    this class reads a scalar player attribute and clamps it). The
    clamp ``max(0, life)`` mirrors the round-42
    ``MillTargetPlayerEqualToYourLifeTotal`` precedent
    (``draw_mill.py:187``) -- negative life totals (arbitrary
    lose-life-past-zero / infect-style scenarios; an SBA would
    normally have ended the game) clamp to zero, a silent no-op.

    Reads the *controller's* life total, not the target's -- a "deals
    damage equal to your life total" effect reads *your* (the
    controller's) life.

    No EoE consumers today -- ships per round-27/31/37/45/46
    round-trip-surface-coverage rule (each bridge arm is load-bearing
    for the round-trip + the Rust resolve substrate completes the
    denominator-axis sextuplet). Bespoke amount (life total) so the
    class survives PR #3's damage collapse, but the legal-target pool
    now consults ``self.targets[0].filter`` via the structural
    ``TargetFilter`` grammar -- ``IsCreature() & ControlledByOpponent()``.
    """

    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                ControlledByOpponent, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(IsCreature() & ControlledByOpponent()),),
            )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            life_total = max(0, state.player_by_id(controller_id).life)
            if life_total <= 0:
                return
            _apply_damage(state, targets[0], life_total)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            "deals damage equal to your life total to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class DealDamageToTargetCreatureSpacecraftBoosted(Effect):
    """Deal ``base`` damage to a target creature; ``boosted`` instead
    if you control a Spacecraft.

    Matches Invasive Maneuvers (3/5). Any-target creature.

    Bespoke amount (Spacecraft-conditional) so the class survives
    PR #3's damage collapse; legal-target pool now consults
    ``self.targets[0].filter`` (``IsCreature()``) via the structural
    target-grammar plumbing.
    """

    base: int = 3
    boosted: int = 5
    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import IsCreature
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets", (TargetSpec(IsCreature()),),
            )

    def _amount(self, state: "GameState", controller_id: int) -> int:
        has_spacecraft = any(
            o.controller_id == controller_id and o.printing.is_spacecraft
            for o in state.battlefield
        )
        return self.boosted if has_spacecraft else self.base

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            _apply_damage(
                state, targets[0], self._amount(state, controller_id),
            )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            f"deals {self.base} damage to target creature, {self.boosted} "
            f"instead if you control a Spacecraft"
        )



@dataclass(frozen=True)
class DealDamageToOpponentCreature(DealDamage):
    """``DealDamage`` constructor sugar for "deals N damage to target
    creature an opponent controls".

    The filter ``IsCreature() & ControlledByOpponent()`` reads
    ``state.resolving_controller_id`` (pinned by
    ``Effect._enumerate_for_filter``) so the legal-target pool is
    restricted to creatures the spell's caster doesn't control.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            ControlledByOpponent, IsCreature,
        )
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsCreature() & ControlledByOpponent()),),
        )



@dataclass(frozen=True)
class DealDamageToTargetTappedCreature(DealDamage):
    """``DealDamage`` constructor sugar for "deals N damage to target
    tapped creature".

    Filter is ``IsCreature() & IsTapped()``; the unified
    ``DealDamage.resolve`` re-verifies the slot's filter against the
    current battlefield at resolution time, so a target that
    untapped between cast and resolve fails the recheck and the
    damage application is skipped per CR 608.2b.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature, IsTapped
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsCreature() & IsTapped()),),
        )



@dataclass(frozen=True)
class FightTargetCreatureWithTargetCreature(Effect):
    """Bilateral fight effect: "Target creature you control fights
    target creature an opponent controls" (CR 701.12).

    Sibling of the Aura-shape ``EnchantedCreatureFightsTargetEffect``
    in ``effects/bespoke.py``: that class derives the source-side
    fighter from the resolving Aura's ``attached_to`` field; this
    generic class derives BOTH fighters from a deterministic battle-
    field scan that matches the Rust
    ``Effect::FightTargetCreatureWithTargetCreature`` arm
    (``rust/src/state_ops_resolve.rs``):

      * Source-side fighter = FIRST creature on the source-controller's
        battlefield (smallest list index).
      * Opponent-side fighter = FIRST creature on the opponent's
        battlefield (smallest list index).

    The deterministic-pick shape mirrors Rust's first-slice "no
    explicit target choice" convention (same pattern as
    ``Effect::PumpTargetCreatureUntilEOT`` and
    ``Effect::DealDamageAnyTarget``). The richer target-choice
    machinery (multi-slot ``targets`` tuple with ``ControlledByYou``
    and ``ControlledByOpponent`` filters, agent-driven push-time
    pick) is the future-slice extension once the cross-engine
    decision-callback substrate plumbs explicit target picks
    bilaterally.

    **Mechanism.** Compute BOTH fighters' effective power BEFORE
    applying any damage (CR 701.12 — simultaneous-damage event;
    a fighter that takes lethal still deals its damage first). If
    either fighter is missing at resolve time, the fight does NOT
    deal damage on either side (CR 701.12c — silent fizzle). Routes
    through ``_apply_damage`` so any "when ~ is dealt damage" auras
    on either fighter trigger correctly. State-based actions then
    handle creatures with lethal damage on the next SBA pass.

    **Consumer.** No real-EoE card uses this exact generic shape
    today — the only EoE Fight consumer (Meltstrider's Resolve) is
    Aura-shape and routes through
    ``EnchantedCreatureFightsTargetEffect``. This class exists as
    the Rust-side ``Effect::FightTargetCreatureWithTargetCreature``
    sibling so the bridge has a Python target for the new effect
    arm; synthetic probes drive end-to-end coverage. Future cards
    matching the bilateral generic-fight shape (Pounce, Prey Upon,
    Rabid Bite — none in EoE) drop in cleanly.

    **Deferred riders** (documented in
    ``docs/EOE_PROBE_READINESS_AUDIT.md``):
    - "Up to one target creature you control fights..." (optional
      target slot, no-fizzle on missing source-side).
    - "Target creature you control fights target creature... and
      gains trample / deathtouch UEOT" (composes via Sequence with
      a future Pump+Keyword arm).
    - "Target creature fights twice" (would need a counter or a
      sequence of two FightTarget arms).
    - The Aura-attached-creature shape (Meltstrider's Resolve) —
      kept on the bespoke ``EnchantedCreatureFightsTargetEffect``
      class until the Aura ETB-trigger substrate routes
      ``attached_to`` through the cross-engine bridge end-to-end.
    """

    def requires_target(self) -> bool:
        # Deterministic-pick shape: no explicit target choice. Mirrors
        # Rust's first-slice convention.
        return False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        opponent_id = 1 - controller_id
        # Same-side fighter: FIRST creature on source-controller's bf.
        caster_fighter = next(
            (
                o for o in state.battlefield
                if o.controller_id == controller_id and o.printing.is_creature
            ),
            None,
        )
        # Opponent-side fighter: FIRST creature on opponent's bf.
        opponent_fighter = next(
            (
                o for o in state.battlefield
                if o.controller_id == opponent_id and o.printing.is_creature
            ),
            None,
        )
        # CR 701.12c: missing fighter on either side = silent fizzle.
        if caster_fighter is None or opponent_fighter is None:
            return
        # Compute BOTH effective powers BEFORE applying damage so the
        # simultaneous-exchange semantics hold (a fighter that takes
        # lethal still deals its damage first).
        caster_power = effective_power(state, caster_fighter)
        opponent_power = effective_power(state, opponent_fighter)
        state.log.append(
            f"{caster_fighter.printing.name} fights "
            f"{opponent_fighter.printing.name}"
        )
        if caster_power > 0:
            _apply_damage(
                state, ("creature", opponent_fighter.id), caster_power,
            )
        if opponent_power > 0:
            _apply_damage(
                state, ("creature", caster_fighter.id), opponent_power,
            )

    def describe(self) -> str:
        return (
            "target creature you control fights target creature an "
            "opponent controls"
        )


@dataclass(frozen=True)
class DrawCardsAndLoseLifeEqualToResolvingDamage(Effect):
    """"You draw that many cards and lose that much life." — the granted
    UEOT trigger body Requiem Monolith installs on the target creature.

    Reads ``state.resolving_combat_damage_amount`` (set by the dealt-
    damage fire helper to the incoming damage amount, regardless of
    whether the damage was combat or non-combat) and applies it to the
    granted creature's controller — ``state.resolving_source`` is set
    to the granted creature itself by ``_fire_enchanted_dealt_damage_
    triggers``, so the creature's controller drives both the draw and
    the life-loss step.

    No-op if the resolving amount is 0 or the resolving source is None
    (defensive — the helper sets both correctly).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_card

        amount = state.resolving_combat_damage_amount
        if amount <= 0:
            return
        source = state.resolving_source
        if source is None:
            return
        owner = state.player_by_id(source.controller_id)
        for _ in range(amount):
            draw_card(state, owner.id)
        owner.life -= amount
        state.log.append(
            f"P{owner.id} draws {amount} and loses {amount} life "
            f"(life: {owner.life})"
        )

    def describe(self) -> str:
        return "you draw that many cards and lose that much life"


@dataclass(frozen=True)
class MayHaveSourceDealDamageToTarget(Effect):
    """"That creature's controller may have <source> deal N damage to
    it." — Requiem Monolith's optional-ping body.

    The TARGET's controller (NOT the activator/resolving controller)
    chooses whether to take the optional damage. Anaphoric to the
    target chosen by an upstream sub-effect in the same ``Sequence``
    (typically ``GrantTriggeredAbilityUEOT``); the target tuple is
    threaded through by ``Sequence.resolve``'s pass-through (Sequence
    forwards its ``targets`` to every sub-effect, untargeted ones
    accept the parameter via ``targets: tuple = ()`` and ignore it,
    anaphoric ones like this primitive read ``targets[0]`` directly).

    Declares ``requires_target=False`` so ``Sequence.__post_init__``'s
    "at most one targeting sub-effect" check still permits the
    upstream targeter alongside this primitive.

    On accept, the source deals ``amount`` damage to the chosen
    creature via direct ``damage`` write + ``_fire_enchanted_dealt_
    damage_triggers`` (which fires any UEOT-granted dealt-damage
    triggers on the target — e.g. the trigger the preceding
    ``GrantTriggeredAbilityUEOT`` just installed).
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.combat import _fire_enchanted_dealt_damage_triggers

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        tgt = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if tgt is None:
            return
        source = state.resolving_source
        if source is None:
            return
        from mtgz_ir.keyword_rules import is_protected_from

        if is_protected_from(state, tgt, source):
            return
        target_controller = state.player_by_id(tgt.controller_id)
        if target_controller.has_lost or target_controller.agent is None:
            return
        accept = target_controller.agent.choose_yes_no(
            state, target_controller.id,
            f"have {source.printing.name} deal {self.amount} damage "
            f"to {tgt.printing.name}",
        )
        if not accept:
            state.log.append(
                f"P{target_controller.id} declines the optional "
                f"{self.amount} damage from {source.printing.name}"
            )
            return
        tgt.damage += self.amount
        tgt.damaged_this_turn = True
        state.log.append(
            f"{source.printing.name} deals {self.amount} to "
            f"{tgt.printing.name}"
        )
        _fire_enchanted_dealt_damage_triggers(state, tgt, self.amount)

    def describe(self) -> str:
        return (
            f"that creature's controller may have this artifact "
            f"deal {self.amount} damage to it"
        )
