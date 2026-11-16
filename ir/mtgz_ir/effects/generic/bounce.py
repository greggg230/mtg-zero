"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import GraveyardReturnDestination
from mtgz_ir.effects.base import (
    Effect, Target, TriggeredAbility,
    _apply_damage, _multicolored_permanents_count,
    _find_moonlit_replacement_source,
)
from mtgz_ir.effects.generic.graveyard import ReturnFromGraveyard
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
class Bounce(Effect):
    """Unified return-to-owner's-hand primitive driven by ``targets``.

    Sibling of :class:`Destroy` / :class:`Exile` / :class:`Tap` /
    :class:`Pump` for the bounce family. The unified resolve walks
    each slot, re-verifies legality at resolution per CR 608.2b, and
    bounces each surviving target via :func:`_bounce_to_hand` (which
    handles token cease-to-exist, hand placement, reveal, and the
    permanent-left-battlefield-this-turn bookkeeping for Void).

    Variants with riders that need to snapshot battlefield state
    BEFORE the bounce (e.g., the "if it was attacking, create a
    Drone token" rider on Desculpting Blast) override ``resolve`` so
    they can capture ``obj.attacking`` before ``_bounce_to_hand``
    clears it.
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
                        _fizzle_message(state, chosen, action="bounce")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                _bounce_to_hand(state, obj)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"return {_describe_targets(self.targets)} to its owner's hand"


@dataclass(frozen=True)
class ReturnTargetCreatureToOwnersHand(Bounce):
    """``Bounce`` constructor sugar for "return target creature to its
    owner's hand" with optional widening / rider flags.

    Filter composition:
      - default: ``IsCreature()``
      - ``allow_nonland_permanent=True``: ``~IsLand() & ~IsPlayer()``
        (Desculpting Blast — broadens to any nonland permanent)
      - ``optional=True``: slot ``count_min=0`` ("up to one target")

    Rider: ``if_was_attacking`` ("If it was attacking, <effect>",
    Desculpting Blast's Drone token, its printing inline) snapshots
    ``obj.attacking`` BEFORE the unified ``_bounce_to_hand`` clears
    it, then resolves the rider for the spell's controller after the
    bounce — needs a custom resolve since the snapshot can't happen
    inside the bare base.
    """

    allow_nonland_permanent: bool = False
    if_was_attacking: "Effect | None" = None
    optional: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            IsCreature, IsLand, IsPlayer,
        )
        from mtgz_ir.effects.targets import TargetSpec
        if self.allow_nonland_permanent:
            f = ~IsLand() & ~IsPlayer()
        else:
            f = IsCreature()
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
        if self.if_was_attacking is None:
            super().resolve(state, controller_id, targets)
            return
        # Drone-rider variant: snapshot ``obj.attacking`` before the
        # unified bounce clears it, then create the token after the
        # bounce settles.
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
                        _fizzle_message(state, chosen, action="bounce")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                was_attacking = obj.attacking
                _bounce_to_hand(state, obj)
                if was_attacking:
                    self.if_was_attacking.resolve(state, controller_id, ())
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        scope = "nonland permanent" if self.allow_nonland_permanent else "creature"
        base = f"return target {scope} to its owner's hand"
        if self.if_was_attacking is not None:
            base += f"; if it was attacking, {self.if_was_attacking.describe()}"
        return base



@dataclass(frozen=True)
class ReturnTargetPermanentYouControlToOwnersHand(Bounce):
    """``Bounce`` constructor sugar for "return target permanent you
    control to its owner's hand" (Exosuit Savior's bounce arm).

    Filter is ``ControlledByYou() & ~IsPlayer()`` — covers any
    battlefield permanent (lands too) the controller controls.
    ``exclude_self=True`` adds ``& ~IsSource()`` for the "other
    target" qualifier. ``optional=True`` flips the slot's
    ``count_min`` to 0 for "up to one target" semantics.
    """

    optional: bool = False
    exclude_self: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            ControlledByYou, IsPlayer, IsSource,
        )
        from mtgz_ir.effects.targets import TargetSpec
        f = ControlledByYou() & ~IsPlayer()
        if self.exclude_self:
            f = f & ~IsSource()
        spec = TargetSpec(
            f,
            count_min=0 if self.optional else 1,
            count_max=1,
        )
        object.__setattr__(self, "targets", (spec,))

    def describe(self) -> str:
        return "return target permanent you control to its owner's hand"



@dataclass(frozen=True)
class ReturnUpToNTargetCreaturesToOwnersHands(Effect):
    """Return up to ``max_n`` target creatures to their owners' hands.

    Matches Specimen Freighter's "return up to two target non-Spacecraft
    creatures to their owners' hands". Set ``exclude_spacecraft=True`` to
    enforce the non-Spacecraft filter at target-selection time.
    """

    max_n: int = 2
    exclude_spacecraft: bool = False

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0, self.max_n)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and (not self.exclude_spacecraft or not o.printing.is_spacecraft)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        for kind, target_id in targets:
            if kind != "creature":
                continue
            obj = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
            if obj is None:
                continue
            if obj.exile_instead_of_leaving:
                from mtgz_ir.turn import move_off_battlefield

                move_off_battlefield(state, obj, destination="exile")
                continue
            state.battlefield.remove(obj)
            from mtgz_ir.game import restore_mdfc_front

            restore_mdfc_front(obj)
            obj.damage = 0
            obj.marked_lethal = False
            obj.damaged_this_turn = False
            obj.tapped = False
            obj.attacking = False
            obj.blocking = None
            obj.blocked_by = []
            obj.plus_one_counters = 0
            obj.charge_counters = 0
            obj.temp_power_bonus = 0
            obj.temp_toughness_bonus = 0
            obj.attached_to = None
            if not obj.printing.is_land:
                state.permanent_left_battlefield_this_turn = True
            state.player_by_id(
                obj.controller_id
            ).permanent_left_battlefield_this_turn = True
            if obj.printing.is_token:
                state.log.append(
                    f"{obj.printing.name} token ceases to exist (bounced)"
                )
                continue
            from mtgz_ir.game import reveal_to_all
            obj.zone = Zone.HAND
            state.player_by_id(obj.owner_id).hand.append(obj)
            reveal_to_all(state, obj)
            state.log.append(
                f"{obj.printing.name} returns to P{obj.owner_id}'s hand"
            )

    def describe(self) -> str:
        return (
            f"return up to {self.max_n} target creatures to their "
            f"owners' hands"
        )



@dataclass(frozen=True)
class ReturnUpToNSmallCreaturesFromGraveyardWithCounters(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for "return up to N
    target creature cards with total mana value M or less from your
    graveyard to the battlefield, with a +1/+1 counter on each".
    Matches Scout for Survivors.

    The unified base's ``max_total_mv`` axis enforces the SUM-of-MV
    budget at resolve time (skipping any pick that would exceed the
    remaining budget). The ``plus_one_counter`` rider applies the
    +1/+1 counter after each return-to-battlefield. The legacy
    legal-target filter (creature cards with MV ≤ ``max_total_mv``
    individually, since a card bigger than the budget can never
    appear in any legal multi-pick) is replicated here by setting
    ``max_mv = max_total_mv`` so the per-card cap and the sum cap
    coincide for the single-card-too-big edge case.
    """

    max_n: int = 3
    max_total_mv: int = 3

    def __post_init__(self) -> None:
        object.__setattr__(self, "creature", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.BATTLEFIELD)
        object.__setattr__(self, "min_n", 0)
        # Per-card MV cap mirrors the legacy single-target legality
        # filter ("a card bigger than the budget can never appear in
        # any legal multi-pick"). Without this the legal-target pool
        # would include too-big cards that the resolve-time budget
        # check would silently drop.
        object.__setattr__(self, "max_mv", self.max_total_mv)
        object.__setattr__(self, "plus_one_counter", True)

    def describe(self) -> str:
        return (
            f"return up to {self.max_n} target creature cards with total "
            f"mana value {self.max_total_mv} or less from your graveyard "
            f"to the battlefield with a +1/+1 counter on each"
        )



@dataclass(frozen=True)
class ReturnTargetArtifactCardFromYourGraveyardToHand(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for "return target
    artifact card from your graveyard to your hand". Matches Scour for
    Scrap's second mode. Uses target-kind ``graveyard_artifact`` so the
    pool is distinct from creature-grave / land-grave variants.
    """

    def __post_init__(self) -> None:
        object.__setattr__(self, "artifact", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.HAND)
        object.__setattr__(self, "target_kind", "graveyard_artifact")

    def describe(self) -> str:
        return "return target artifact card from your graveyard to your hand"



@dataclass(frozen=True)
class ReturnCreatureFromYourGraveyardToHand(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for "return target
    creature card from your graveyard to your hand" with optional
    widening / mv-cap riders.

    ``max_mv`` (default 0 = "no cap") — narrows the legal-target pool
    to cards whose printed mana value is ≤ the threshold (Savior of
    the Small).

    ``or_spacecraft`` — widens the legal-target pool to also include
    Spacecraft cards (Fell Gravship's "return target creature or
    Spacecraft card").

    ``or_artifact`` — widens the pool to also include artifact cards
    (Interceptor Mechan's "return target artifact or creature card").

    All three rider fields live on the unified base; ``__post_init__``
    only pins ``creature=True``. Constructor and equality semantics
    are preserved.
    """

    def __post_init__(self) -> None:
        object.__setattr__(self, "creature", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.HAND)

    def describe(self) -> str:
        # Preserve the legacy printed phrasing exactly. Extras live
        # in the rider order honored by ``ReturnFromGraveyard.describe``
        # (parent — graveyard.py:299): or_artifact widens to
        # "artifact or creature", or_enchantment to "creature or
        # enchantment", or_spacecraft to "creature or Spacecraft".
        extras = []
        if self.or_artifact:
            extras.append("artifact")
        if self.or_enchantment:
            extras.append("enchantment")
        if self.or_spacecraft:
            extras.append("Spacecraft")
        if extras:
            if "artifact" in extras:
                type_phrase = "artifact or creature"
            elif "enchantment" in extras:
                type_phrase = "creature or enchantment"
            else:
                type_phrase = "creature or Spacecraft"
        else:
            type_phrase = "creature"
        if self.max_mv > 0:
            return (
                f"return target {type_phrase} card with mana value "
                f"{self.max_mv} or less from your graveyard to your hand"
            )
        return (
            f"return target {type_phrase} card from your graveyard "
            f"to your hand"
        )



@dataclass(frozen=True)
class ReturnUpToNCreaturesFromYourGraveyardToHand(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for "return up to N
    target creature cards from your graveyard to your hand". Models
    Archenemy's Charm mode 1 ("return one or two target creature [or
    planeswalker] cards"). Planeswalker cards aren't modeled in the
    current card pool; they would surface through the same code path
    automatically once the legality check sees them as creature cards.
    """

    max_n: int = 2
    min_n: int = 1

    def __post_init__(self) -> None:
        object.__setattr__(self, "creature", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.HAND)

    def describe(self) -> str:
        return (
            f"return up to {self.max_n} target creature cards from your "
            f"graveyard to your hand"
        )



@dataclass(frozen=True)
class ReturnUpToNNonlandPermanentCardsFromGraveyardToHand(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for "return up to N
    target nonland permanent cards from your graveyard to your hand".
    Models the first clause of Pull Through the Weft (the second
    "lands to battlefield tapped" clause is dropped at parse time —
    no engine primitive yet).

    Legal targets: every card in the controller's graveyard whose
    printing is NOT a land. Uses the shared ``graveyard_creature``
    target kind to match the legacy emit semantics.
    """

    max_n: int = 2
    min_n: int = 0

    def __post_init__(self) -> None:
        object.__setattr__(self, "nonland_permanent", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.HAND)

    def describe(self) -> str:
        return (
            f"return up to {self.max_n} target nonland permanent cards "
            f"from your graveyard to your hand"
        )


@dataclass(frozen=True)
class ReturnTargetLandFromGraveyardToHand(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for "return target
    land card from your graveyard to your hand". Used by Petrified
    Field's tap-sacrifice activated ability. Uses target-kind
    ``graveyard_land`` and tags the resolve log line with the
    "(Petrified Field)" suffix matching the legacy printout.
    """

    def __post_init__(self) -> None:
        object.__setattr__(self, "land", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.HAND)
        object.__setattr__(self, "target_kind", "graveyard_land")
        object.__setattr__(self, "log_suffix", "(Petrified Field)")

    def describe(self) -> str:
        return "return target land card from your graveyard to your hand"
