"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import CardTypeWord
from mtgz_ir.keyword_vocab import KeywordSpelling
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
from mtgz_ir.effects.generic.grant_keywords import (
    GrantActivatedAbility,
    GrantKeywords,
    GrantTriggeredAbility,
    GrantUnblockable,
)

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class TargetArtifactBecomesCreatureUEOT(Effect):
    """Synthesizer Labship's 2+ tier trigger: ``up to one other target
    artifact you control becomes an artifact creature with base power
    and toughness ``power``/``toughness`` and gains ``granted_keywords``
    until end of turn.``

    Composes ``ApplyEffect(BecomeType(...), EndOfTurn())`` for the
    type/PT side and ``ApplyEffect(GrantKeywords(...), EndOfTurn())``
    for the keyword grants — both threaded through the chosen target.
    Targets exclude the source (``"other"``); both creature and
    non-creature artifacts are valid targets (a non-creature artifact
    gains creature status until end of turn; an already-creature
    artifact has its base P/T overridden — which can shrink a
    stationed Spacecraft).
    """

    power: int = 0
    toughness: int = 0
    granted_keywords: frozenset[KeywordSpelling] = frozenset()

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        # "Up to one" — declining is legal.
        return (0, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_artifact_now

        source = state.resolving_source
        source_id = source.id if source is not None else -1
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.controller_id == controller_id
            and is_artifact_now(state, o)
            and o.id != source_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn

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
        ApplyEffect(
            inner=BecomeType(
                types_to_add=("creature",),
                base_pt=(self.power, self.toughness),
            ),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, targets)
        if self.granted_keywords:
            ApplyEffect(
                inner=GrantKeywords(keywords=self.granted_keywords),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, targets)
        kw_note = (
            f" and gains {', '.join(sorted(self.granted_keywords))}"
            if self.granted_keywords else ""
        )
        state.log.append(
            f"{obj.printing.name} becomes a {self.power}/{self.toughness}"
            f" artifact creature{kw_note} until end of turn"
        )

    def describe(self) -> str:
        kw = ""
        if self.granted_keywords:
            kw = f" and gains {', '.join(sorted(self.granted_keywords))}"
        return (
            f"up to one other target artifact you control becomes a "
            f"{self.power}/{self.toughness} artifact creature{kw} until "
            f"end of turn"
        )



@dataclass(frozen=True)
class BecomeLandCreatureUEOT(Effect):
    """Animate a land into a creature until end of turn.

    Composes the new ``ApplyEffect(BecomeType(...), EndOfTurn())``
    primitive for the type/subtype/PT side: writes layer-4
    ``add_type "creature"`` (+ optional ``add_subtype``) and layer-7
    ``set_base_pt`` registry entries with ``expires_at="cleanup"``,
    flipping ``permanent_is_creature`` for the duration. The
    cleanup-step rebuild in ``run_cleanup_step`` reverts the cache to
    False once the registry entries expire, so the land returns to
    its non-creature state at end of turn.

    Keyword grants ride ``ApplyEffect(GrantKeywords(...),
    EndOfTurn())`` (layer-6 ``grant_keyword`` registry entries). The
    ``unblockable`` rider for Creeping Tar Pit is structurally
    distinct from real keywords — "can't be blocked" is a continuous
    gameplay-action restriction, not an ability — so it composes via
    ``ApplyEffect(GrantUnblockable(...), EndOfTurn())`` (layer-6
    ``cant_be_blocked`` registry entry), separately from the keyword
    grants. Granted activated / triggered abilities ride
    ``ApplyEffect(GrantActivatedAbility(...), EndOfTurn())`` /
    ``ApplyEffect(GrantTriggeredAbility(...), EndOfTurn())`` (layer-6
    ``grant_*_ability`` registry entries).
    """

    power: int = 0
    toughness: int = 0
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    unblockable: bool = False
    # Subtype the animated creature gains UEOT (e.g., "Blinkmoth" for
    # Blinkmoth Nexus, "Elemental" for Lavaclaw Reaches). Written as
    # a layer-4 ``add_subtype`` registry entry (read by
    # ``effective_subtypes``); reverts when the entry expires at
    # cleanup.
    granted_subtype: str | None = None
    # Further card types and creature types the animated land gains
    # beside ``creature`` and ``granted_subtype`` (CR 205.1b): "a 1/1
    # Blinkmoth artifact creature" (Blinkmoth Nexus: ``("Artifact",)``),
    # "a 1/1 Phyrexian Blinkmoth artifact creature" (Inkmoth Nexus:
    # ``granted_subtypes=("Phyrexian", "Blinkmoth")``).
    granted_card_types: tuple[str, ...] = ()
    granted_subtypes: tuple[str, ...] = ()
    # "A 2/2 creature with all creature types" (Mutavault; CR 205.3m:
    # every creature type, until end of turn). The Python engine doesn't
    # model it (``docs/SEARCH_LIMITATIONS.md``); the exported IR carries
    # it for mtg-zero.
    all_creature_types: bool = False
    # Activated / triggered abilities granted UEOT to the source while
    # animated. Composed via
    # ``ApplyEffect(GrantActivatedAbility(...), EndOfTurn())`` /
    # ``ApplyEffect(GrantTriggeredAbility(...), EndOfTurn())`` — each
    # writes a layer-6 registry entry with ``expires_at="cleanup"``.
    # Used by manlands whose printed text grants per-card abilities
    # only while the land is a creature (Lavaclaw Reaches, Wandering
    # Fumarole, Raging Ravine).
    granted_activated_abilities: tuple = ()
    granted_triggered_abilities: tuple = ()
    # Colors the animated land BECOMES until end of turn ("a 5/5 green
    # and blue Plant creature" — Restless Vinestalk): a layer-5
    # ``set_colors`` registry entry expiring at cleanup. Empty = the
    # land keeps its own (colorless) color set.
    set_colors: frozenset[str] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn

        source = state.resolving_source
        if source is None:
            return
        if self.set_colors:
            state.add_continuous_effect(
                source_id=source.id,
                layer=5,
                kind="set_colors",
                target_id=source.id,
                params=(frozenset(self.set_colors),),
                expires_at="cleanup",
                description=(
                    f"{source.printing.name} becomes "
                    f"{'/'.join(sorted(self.set_colors))} until end of turn"
                ),
            )
        subtypes = (
            (self.granted_subtype,) if self.granted_subtype else ()
        ) + tuple(
            s for s in self.granted_subtypes if s != self.granted_subtype
        )
        ApplyEffect(
            inner=BecomeType(
                types_to_add=("creature",) + tuple(
                    t.lower() for t in self.granted_card_types
                ),
                subtypes_to_add=subtypes,
                base_pt=(self.power, self.toughness),
            ),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, ())
        kws = self.granted_keywords
        if kws:
            ApplyEffect(
                inner=GrantKeywords(keywords=kws),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, ())
        if self.unblockable:
            # "Can't be blocked" is a continuous effect, not a keyword
            # — composes via the dedicated ``GrantUnblockable``
            # primitive (direct / self-target shape: ``self.targets``
            # empty, runtime ``targets`` carries the animated land),
            # which writes a layer-6 ``cant_be_blocked`` registry
            # entry (read by ``game.is_cant_be_blocked``).
            ApplyEffect(
                inner=GrantUnblockable(),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, (("permanent", source.id),))
        for ab in self.granted_activated_abilities:
            ApplyEffect(
                inner=GrantActivatedAbility(ability=ab),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, ())
        for ab in self.granted_triggered_abilities:
            ApplyEffect(
                inner=GrantTriggeredAbility(ability=ab),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, ())
        kw_parts = sorted(kws)
        if self.unblockable:
            kw_parts.append("unblockable")
        kw_note = (
            f" with {', '.join(kw_parts)}" if kw_parts else ""
        )
        state.log.append(
            f"{source.printing.name} becomes a {self.power}/{self.toughness}"
            f" creature UEOT{kw_note}"
        )

    def describe(self) -> str:
        kws = (
            " with " + ", ".join(sorted(self.granted_keywords))
            if self.granted_keywords else ""
        )
        unb = " (unblockable)" if self.unblockable else ""
        return (
            f"until end of turn, this land becomes a "
            f"{self.power}/{self.toughness} creature{kws}{unb}; "
            f"it's still a land"
        )


@dataclass(frozen=True)
class BecomesCopyOfTargetLand(Effect):
    """Thespian's Stage: "This land becomes a copy of target land,
    except it has this ability." Continuous effect — persists until the
    source leaves play or another copy effect overrides.

    Implementation: mutates ``source.printing`` to point at a fresh
    CardPrinting derived from the target's, with the source's own
    activated abilities appended (so "it has this ability" is
    preserved across repeated re-copies).

    Simplifications:
    - Re-copying from the newly-mutated printing still re-derives
      "retained own abilities" from ``source.original_printing``, which
      is snapshotted on the first resolve. Prevents accidental
      ability accumulation across multiple copies.
    - Copying a token target (rare but possible) carries the token
      supertype through unchanged; the source itself doesn't become a
      token.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.printing.is_land
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        import dataclasses as _dc

        source = state.resolving_source
        if source is None or not targets:
            return
        _, tid = targets[0]
        target = next(
            (o for o in state.battlefield if o.id == tid), None,
        )
        if target is None:
            return
        # Snapshot the original printing the first time we copy — so
        # re-copy activations don't lose "this ability".
        if source.original_printing is None:
            source.original_printing = source.printing
        retained = source.original_printing.activated_abilities
        source.printing = _dc.replace(
            target.printing,
            activated_abilities=tuple(target.printing.activated_abilities)
            + tuple(retained),
        )
        state.log.append(
            f"{source.original_printing.name} becomes a copy of "
            f"{target.printing.name}"
        )

    def describe(self) -> str:
        return (
            "this land becomes a copy of target land, except it has "
            "this ability"
        )



@dataclass(frozen=True)
class BecomeType(Effect):
    """Source gains the listed card-types and/or subtypes, optionally with
    a new base P/T, IN ADDITION TO ITS OTHER TYPES.

    Always writes registry entries with ``expires_at=None`` (the entry
    persists until the source leaves play). Wrap with
    ``ApplyEffect(BecomeType(...), ends_at=<Duration>)`` to give the
    change a finite lifetime — ``ApplyEffect`` rewrites the lifetime
    fields of every entry written here.

    Registry writes (each gated on its parameter being non-empty):

      - For each entry in ``types_to_add``: layer 4 ``add_type``. When
        the type is ``"creature"``, also flips the
        ``GameObject.permanent_is_creature`` write-through cache that
        ``is_creature_now`` reads. ``run_cleanup_step`` rebuilds the
        cache from the registry after expiring cleanup-scoped entries,
        so an ``ApplyEffect``-wrapped ``BecomeType`` with a finite
        duration correctly reverts the cache when its entry expires.
      - For each entry in ``subtypes_to_add``: layer 4 ``add_subtype``.
        Read by ``effective_subtypes`` walking the registry, so any
        duration is faithfully respected.
      - When ``base_pt`` is set: layer 7 sublayer 1 ``set_base_pt``.
        Read by ``_compute_effective_pt`` via
        ``_registry_set_base_pt``, so any duration is faithfully
        respected.

    Target selection: when ``targets`` carries a ``(kind, id)`` pair,
    the registry entries target that object — the spell/ability source
    (``state.resolving_source``) is still recorded as the
    registry-entry ``source_id``, so the entry is dropped when the
    spell/ability source leaves play. With no targets, the effect
    falls back to self-target (Haunted Screen, manland animation):
    both ``source_id`` and ``target_id`` are
    ``state.resolving_source.id``.

    No-op when neither ``targets`` nor ``state.resolving_source``
    yields a battlefield object.
    """

    types_to_add: tuple[CardTypeWord, ...] = ()
    subtypes_to_add: tuple[str, ...] = ()
    base_pt: "tuple[int, int] | None" = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "types_to_add", tuple(CardTypeWord(t.lower()) for t in self.types_to_add))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
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
        for type_name in self.types_to_add:
            state.add_continuous_effect(
                source_id=source_id, layer=4, kind="add_type",
                target_id=target.id, params=(type_name,),
                expires_at=None,
                description=(
                    f"{target.printing.name} becomes a {type_name}"
                ),
            )
            if type_name.lower() == "creature":
                target.permanent_is_creature = True
        for subtype in self.subtypes_to_add:
            state.add_continuous_effect(
                source_id=source_id, layer=4, kind="add_subtype",
                target_id=target.id, params=(subtype,),
                expires_at=None,
                description=(
                    f"{target.printing.name} becomes a {subtype}"
                ),
            )
        if self.base_pt is not None:
            base_p, base_t = self.base_pt
            state.add_continuous_effect(
                source_id=source_id, layer=7, sublayer=1,
                kind="set_base_pt",
                target_id=target.id, params=(base_p, base_t),
                expires_at=None,
                description=(
                    f"{target.printing.name} base {base_p}/{base_t}"
                ),
            )
        state.log.append(
            f"{target.printing.name} {self.describe()}"
        )

    def describe(self) -> str:
        parts: list[str] = []
        if self.base_pt is not None:
            parts.append(f"becomes a {self.base_pt[0]}/{self.base_pt[1]}")
        else:
            parts.append("becomes")
        if self.subtypes_to_add:
            parts.append(" ".join(self.subtypes_to_add))
        if self.types_to_add:
            parts.append(" ".join(self.types_to_add))
        body = " ".join(parts)
        return f"{body} in addition to its other types"
