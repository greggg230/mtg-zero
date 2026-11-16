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
class GrantUnblockable(Effect):
    """Unified can't-be-blocked primitive driven by ``targets``.

    Sibling of :class:`Destroy` / :class:`Exile` / :class:`Tap` /
    :class:`Pump` / :class:`Bounce` / :class:`PutPlusOneCounters`
    for the can't-be-blocked family. The unified resolve walks each
    slot, re-verifies legality at resolution per CR 608.2b, and writes
    a layer-6 ``cant_be_blocked`` registry entry with empty ``params``
    and ``expires_at=None`` (permanent until source leaves play) for
    each surviving target. Wrap with ``ApplyEffect(
    GrantUnblockable(...), EndOfTurn())`` (or use the
    :class:`GrantUnblockableUEOT` sugar) to give the grants a UEOT
    lifetime — ``ApplyEffect`` rewrites the ``expires_at`` of every
    entry written here.

    "Can't be blocked" is NOT a keyword in MTG; it is a layer-6
    continuous effect that imposes a blocker-declaration restriction
    (CR 509.1b). The registry kind ``cant_be_blocked`` is therefore
    structurally distinct from ``grant_keyword``, and is read by
    :func:`mtgz_ir.game.is_cant_be_blocked` (consumed by
    ``combat.can_block`` / ``combat.declare_blockers``).
    """

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec

        source = state.resolving_source
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            # Two callers shapes — mirroring ``GrantKeywords``:
            # 1. Targeted spell/ability path: ``self.targets`` carries
            #    ``TargetSpec`` slots, ``targets`` carries the resolved
            #    ``(kind, id)`` choices; re-verify legality at resolve.
            # 2. Direct/self-target path (manland animation, Atomic
            #    Microsizer attack trigger): ``self.targets`` is empty
            #    or shorter than the passed ``targets``, and the caller
            #    has already chosen the affected permanent. In this
            #    case there is no target-pick to re-verify (CR 608.2b
            #    only requires re-check for chosen targets); write the
            #    registry entry against each passed ``(kind, id)``.
            if not self.targets:
                slots = (None,) * len(targets)
            else:
                slots = self.targets
            for slot, chosen in zip(slots, targets):
                if isinstance(slot, SlotRef):
                    continue
                if isinstance(slot, TargetSpec):
                    if not _slot_target_still_legal(state, slot, chosen):
                        state.log.append(
                            _fizzle_message(
                                state, chosen,
                                action="unblockable grant",
                            )
                        )
                        continue
                elif slot is not None:
                    # Unknown slot shape — be conservative.
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                source_id = source.id if source is not None else obj.id
                source_name = (
                    source.printing.name if source is not None
                    else obj.printing.name
                )
                state.add_continuous_effect(
                    source_id=source_id, layer=6, kind="cant_be_blocked",
                    target_id=obj.id, params=(),
                    expires_at=None,
                    description=(
                        f"{source_name} makes {obj.printing.name} "
                        f"unblockable"
                    ),
                )
                state.log.append(
                    f"{obj.printing.name} can't be blocked this turn"
                )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"{_describe_targets(self.targets)} can't be blocked this turn"


@dataclass(frozen=True)
class GrantUnblockableUEOT(GrantUnblockable):
    """``GrantUnblockable`` constructor sugar for "target creature can't
    be blocked this turn" with optional filter narrowing flags. The
    UEOT lifetime is composed by wrapping the parent's registry-write
    in ``ApplyEffect(_, EndOfTurn())`` at resolve time.

    Filter composition:
      - base: ``IsCreature()``
      - ``attacking_only=True``: ``& IsAttacking()`` (Clammy Prowler's
        attacks-trigger pool — declare-attackers-step shape)
      - ``exclude_self=True``: ``& ~IsSource()`` ("another target")
      - ``power_at_most=N``: ``& HasPowerAtMost(N)`` (Ragged Playmate's
        "with power 2 or less")
    """

    attacking_only: bool = False
    exclude_self: bool = False
    power_at_most: int | None = None

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            HasPowerAtMost, IsAttacking, IsCreature, IsSource,
        )
        from mtgz_ir.effects.targets import TargetSpec
        f = IsCreature()
        if self.attacking_only:
            f = f & IsAttacking()
        if self.exclude_self:
            f = f & ~IsSource()
        if self.power_at_most is not None:
            f = f & HasPowerAtMost(self.power_at_most)
        object.__setattr__(self, "targets", (TargetSpec(f),))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn

        ApplyEffect(
            inner=GrantUnblockable(targets=self.targets),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, targets)

    def describe(self) -> str:
        if self.attacking_only and self.exclude_self:
            return (
                "another target attacking creature can't be blocked this turn"
            )
        if self.attacking_only:
            return "target attacking creature can't be blocked this turn"
        if self.power_at_most is not None:
            return (
                f"target creature with power {self.power_at_most} or less "
                f"can't be blocked this turn"
            )
        return "target creature can't be blocked this turn"


@dataclass(frozen=True)
class GrantTriggeredAbilityUEOT(Effect):
    """Grant a triggered ability to target creature until end of turn.

    Thin targeting wrapper around ``ApplyEffect(GrantTriggeredAbility(
    ability=...), EndOfTurn())``. The targeting/legal-targets shape
    (``you_control_only`` / ``opponents_only``) lives here; the actual
    registry write delegates to the ``GrantTriggeredAbility`` primitive.
    Use for "Until end of turn, target creature gains 'Whenever ~ does
    X, Y.'" — Requiem Monolith is the canonical EoE example.
    """

    ability: "TriggeredAbility | None" = None
    you_control_only: bool = False
    opponents_only: bool = False

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and (not self.you_control_only or o.controller_id == controller_id)
            and (not self.opponents_only or o.controller_id != controller_id)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn

        if self.ability is None or not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        ApplyEffect(
            inner=GrantTriggeredAbility(ability=self.ability),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, targets)
        state.log.append(
            f"{obj.printing.name} gains a granted triggered ability "
            f"until end of turn"
        )

    def describe(self) -> str:
        if self.ability is None:
            return "grant a triggered ability until end of turn"
        return (
            f"until end of turn, target creature gains "
            f"\"{self.ability.describe()}\""
        )



@dataclass(frozen=True)
class GrantKeywords(Effect):
    """Target gains the listed keywords.

    Always writes layer-6 ``grant_keyword`` registry entries with
    ``expires_at=None`` (the entries persist until the source leaves
    play). Wrap with ``ApplyEffect(GrantKeywords(...), ends_at=
    <Duration>)`` to give the grants a finite lifetime — ``ApplyEffect``
    rewrites the lifetime fields of every entry written here. The
    common UEOT shape is ``ApplyEffect(GrantKeywords(keywords=...),
    EndOfTurn())``.

    Registry writes (one per keyword, in sorted order for stable
    ``description`` output): layer 6 ``grant_keyword`` with
    ``params=(keyword,)``. Read by ``effective_keywords`` walking the
    registry in CR-613 timestamp order, so any duration is faithfully
    respected.

    Target selection: when ``targets`` carries a ``(kind, id)`` pair,
    the registry entries target that object — the spell/ability source
    (``state.resolving_source``) is still recorded as the
    registry-entry ``source_id``, so the entry is dropped when the
    spell/ability source leaves play. With no targets, the effect
    falls back to self-target (manland animation grants haste to
    itself, etc.): both ``source_id`` and ``target_id`` are
    ``state.resolving_source.id``.

    No-op when ``keywords`` is empty, or when neither ``targets`` nor
    ``state.resolving_source`` yields a battlefield object.

    The registry path is the only writer for keyword grants: the
    legacy ``GameObject.temp_keywords`` field has been retired.
    ``effective_keywords`` reads layer-6 ``grant_keyword`` entries in
    CR-613 timestamp order, so any duration declared via
    ``ApplyEffect`` is faithfully respected.
    """

    keywords: frozenset[str] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not self.keywords:
            return
        source = state.resolving_source
        if targets:
            _, target_id = targets[0]
            target = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
        else:
            target = source
        if target is None:
            return
        # Registry ``source_id`` = the spell/ability source (so its
        # leave-play drops the entry); falls back to the target itself
        # for self-target callers that resolve outside a stack frame.
        source_id = source.id if source is not None else target.id
        source_name = (
            source.printing.name if source is not None
            else target.printing.name
        )
        for kw in sorted(self.keywords):
            state.add_continuous_effect(
                source_id=source_id, layer=6, kind="grant_keyword",
                target_id=target.id, params=(kw,),
                expires_at=None,
                description=(
                    f"{source_name} grants {kw} to {target.printing.name}"
                ),
            )

    def describe(self) -> str:
        if not self.keywords:
            return "gains no keywords"
        return f"gains {', '.join(sorted(self.keywords))}"


@dataclass(frozen=True)
class GrantActivatedAbility(Effect):
    """Grant an activated ability to a permanent via a layer-6
    ``"grant_activated_ability"`` registry entry.

    Registry-only primitive: writes a single layer-6 entry with
    ``expires_at=None`` (the entry persists until the source leaves
    play). Wrap with ``ApplyEffect(GrantActivatedAbility(...),
    ends_at=<Duration>)`` to give the grant a finite lifetime —
    ``ApplyEffect`` rewrites the entry's lifetime fields.

    Target selection mirrors ``BecomeType``: when ``targets`` carries
    a ``(kind, id)`` pair, the registry entry targets that object;
    with no targets, the effect falls back to self-target reading
    ``state.resolving_source``. Registry ``source_id`` = the
    spell/ability source (so its leave-play drops the entry); falls
    back to the target's id when no resolving source is set
    (manland-style self-target during ability resolution).

    No-op when neither ``targets`` nor ``state.resolving_source``
    yields a battlefield object. Reads via
    :func:`mtgz_ir.game.effective_activated_abilities`.
    """

    ability: "ActivatedAbility | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if self.ability is None:
            return
        source = state.resolving_source
        if targets:
            _, target_id = targets[0]
            target = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
        else:
            target = source
        if target is None:
            return
        source_id = source.id if source is not None else target.id
        state.add_continuous_effect(
            source_id=source_id,
            layer=6,
            kind="grant_activated_ability",
            target_id=target.id,
            params=(self.ability,),
            expires_at=None,
            description=(
                f"{target.printing.name} gains an activated ability"
            ),
        )

    def describe(self) -> str:
        return "gains an activated ability"


@dataclass(frozen=True)
class GrantTriggeredAbility(Effect):
    """Grant a triggered ability to a permanent via a layer-6
    ``"grant_triggered_ability"`` registry entry.

    Registry-only primitive: writes a single layer-6 entry with
    ``expires_at=None``. Wrap with ``ApplyEffect(
    GrantTriggeredAbility(...), ends_at=<Duration>)`` to give the
    grant a finite lifetime — ``ApplyEffect`` rewrites the entry's
    lifetime fields.

    Target selection mirrors ``BecomeType`` / ``GrantActivatedAbility``:
    when ``targets`` carries a ``(kind, id)`` pair, the registry entry
    targets that object; with no targets, the effect falls back to
    self-target reading ``state.resolving_source``. Registry
    ``source_id`` = the spell/ability source (so its leave-play
    drops the entry); falls back to the target's id when no resolving
    source is set.

    No-op when neither ``targets`` nor ``state.resolving_source``
    yields a battlefield object. Reads via
    :func:`mtgz_ir.game.effective_triggered_abilities`.
    """

    ability: "TriggeredAbility | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if self.ability is None:
            return
        source = state.resolving_source
        if targets:
            _, target_id = targets[0]
            target = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
        else:
            target = source
        if target is None:
            return
        source_id = source.id if source is not None else target.id
        state.add_continuous_effect(
            source_id=source_id,
            layer=6,
            kind="grant_triggered_ability",
            target_id=target.id,
            params=(self.ability,),
            expires_at=None,
            description=(
                f"{target.printing.name} gains a triggered ability"
            ),
        )

    def describe(self) -> str:
        return "gains a triggered ability"
