"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

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
from mtgz_ir.effects.generic.grant_keywords import GrantKeywords

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class PutPlusOneCounters(Effect):
    """Unified put-+1/+1-counters primitive driven by ``targets``.

    Sibling of :class:`Destroy` / :class:`Exile` / :class:`Tap` /
    :class:`Pump` / :class:`Bounce` for the +1/+1 counter family.
    The unified resolve walks each slot, re-verifies legality at
    resolution per CR 608.2b, and adds ``amount`` +1/+1 counters to
    each surviving target via ``add_plus_one_counters`` — the
    counter-doubling replacement effect on the recipient's
    controller is honored inside the helper, not here.

    Variants with extra rider effects (granted keywords UEOT,
    multi-slot up-to-N, sweeps, bespoke amount) override ``resolve``
    or stay on the old-shape ``Effect`` path.
    """

    amount: int = 1
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec
        from mtgz_ir.game import add_plus_one_counters

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
                        _fizzle_message(
                            state, chosen, action="counter placement",
                        )
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                add_plus_one_counters(state, obj, self.amount)
                state.log.append(
                    f"{obj.printing.name} gets {self.amount} +1/+1 "
                    f"counter(s) (now {obj.plus_one_counters})"
                )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        if self.amount == 1:
            return f"put a +1/+1 counter on {_describe_targets(self.targets)}"
        return (
            f"put {self.amount} +1/+1 counters on "
            f"{_describe_targets(self.targets)}"
        )


@dataclass(frozen=True)
class PutPoisonCountersOnTargetPlayer(Effect):
    """Give ``amount`` poison counters to the (pre-selected) target
    player. Reads ``targets[0]`` as ("player", id); silently no-ops on
    mismatched shapes.

    Matches Virulent Silencer's trigger effect: the observer trigger
    wiring in combat._fire_combat_damage_to_player_triggers passes the
    hit player's id as the target.
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, pid = targets[0]
        if kind != "player":
            return
        try:
            p = state.player_by_id(pid)
        except KeyError:
            return
        p.poison_counters += self.amount
        state.log.append(
            f"P{pid} gets {self.amount} poison counter(s) "
            f"(now {p.poison_counters})"
        )

    def describe(self) -> str:
        return f"that player gets {self.amount} poison counter(s)"



@dataclass(frozen=True)
class PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower(Effect):
    """Put X +1/+1 counters on each creature you control, where X is
    this effect's source's effective power. Reads
    ``state.resolving_source`` for the power. No-op if source is None
    or X <= 0.

    Matches Ouroboroid's beginning-of-combat trigger.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters, effective_power

        source = state.resolving_source
        if source is None:
            return
        x = effective_power(state, source)
        if x <= 0:
            return
        count = 0
        for obj in state.battlefield:
            if obj.controller_id != controller_id:
                continue
            if not obj.printing.is_creature:
                continue
            add_plus_one_counters(state, obj, x)
            count += 1
        state.log.append(
            f"P{controller_id}'s {count} creature(s) each get {x} "
            f"+1/+1 counter(s) (source power)"
        )

    def describe(self) -> str:
        return (
            "put X +1/+1 counters on each creature you control, where X "
            "is this creature's power"
        )



@dataclass(frozen=True)
class PutPlusOneCounterOnEachOfUpToNTargetCreatures(Effect):
    """Put a +1/+1 counter on each of up to ``max_n`` target creatures.

    Matches "put a +1/+1 counter on each of up to two target creatures"
    templating (Weftblade Enhancer). Targets are selected by the agent
    via repeated choose_target calls; effect resolves for each valid
    target that's still a creature on the battlefield.
    """

    max_n: int = 2

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
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters

        for kind, target_id in targets:
            if kind != "creature":
                continue
            tgt = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
            if tgt is None:
                continue
            add_plus_one_counters(state, tgt, 1)
            state.log.append(
                f"+1/+1 counter on {tgt.printing.name} "
                f"(now {tgt.plus_one_counters})"
            )

    def describe(self) -> str:
        return (
            f"put a +1/+1 counter on each of up to {self.max_n} target "
            f"creatures"
        )



@dataclass(frozen=True)
class PutLoyaltyCounterOnSelf(Effect):
    """Put ``amount`` loyalty counter(s) on the ability's source.

    Used by planeswalker "whenever ..." triggers that grow their own
    loyalty (Tezzeret's "whenever an artifact you control enters, put
    a loyalty counter on Tezzeret"). Reads state.resolving_source.
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        source.loyalty_counters += self.amount
        state.log.append(
            f"{source.printing.name} gains {self.amount} loyalty "
            f"counter(s) (now {source.loyalty_counters})"
        )

    def describe(self) -> str:
        return f"put {self.amount} loyalty counter(s) on ~"



@dataclass(frozen=True)
class MoveSelfCountersToTargetCreatureYouControl(Effect):
    """Move all +1/+1 counters from the source to target creature you
    control.

    Matches Dockworker Drone's dies trigger ("put its counters on target
    creature you control"). Reads state.resolving_source for the count;
    no-op if the source has zero counters or wasn't set.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature and o.controller_id == controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        count = source.plus_one_counters
        if count <= 0:
            return
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
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, obj, count)
        # Clear source counters — they've been transferred.
        source.plus_one_counters = 0
        state.log.append(
            f"{count} +1/+1 counter(s) moved from {source.printing.name} "
            f"to {obj.printing.name}"
        )

    def describe(self) -> str:
        return (
            "move this creature's +1/+1 counters to target creature you "
            "control"
        )



@dataclass(frozen=True)
class MovePlusOneCounterBetweenPermanentsYouControl(Effect):
    """Nesting Grounds: "Move a counter from target permanent you
    control onto a second target permanent."

    Dual-target effect: targets[0] is the donor, targets[1] is the
    recipient. Both must be permanents the controller controls.

    Simplification: oracle says "a counter" (any type — +1/+1,
    charge, loyalty, stun, etc.). This implementation moves exactly
    one +1/+1 counter. If the donor has none, the ability fizzles.
    Charge/loyalty/stun counter movement isn't modeled — those
    counter types are tied to specific permanent types (Spacecraft,
    planeswalker, tapped creatures) where the recipient might not
    be a valid holder anyway.
    """

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.controller_id == controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if len(targets) < 2:
            return
        (_, donor_id), (_, recipient_id) = targets[0], targets[1]
        if donor_id == recipient_id:
            return
        donor = next(
            (o for o in state.battlefield if o.id == donor_id), None
        )
        recipient = next(
            (o for o in state.battlefield if o.id == recipient_id), None
        )
        if donor is None or recipient is None:
            return
        if donor.plus_one_counters <= 0:
            state.log.append(
                f"Nesting Grounds fizzles — "
                f"{donor.printing.name} has no +1/+1 counters"
            )
            return
        from mtgz_ir.game import add_plus_one_counters

        donor.plus_one_counters -= 1
        add_plus_one_counters(state, recipient, 1)
        state.log.append(
            f"+1/+1 counter moved from {donor.printing.name} "
            f"(now {donor.plus_one_counters}) to {recipient.printing.name}"
        )

    def describe(self) -> str:
        return (
            "move a +1/+1 counter from target permanent you control "
            "onto a second target permanent"
        )



@dataclass(frozen=True)
class PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact(Effect):
    """Put a +1/+1 counter on this creature for each OTHER creature and/or
    artifact you control.

    Matches Luxknight Breacher's "enters with" clause (modeled as an ETB
    trigger since we don't implement replacement effects). Counts are
    sampled at resolution; the source itself is excluded (it's on the
    battlefield by the time the ETB trigger resolves).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now

        source = state.resolving_source
        if source is None:
            return
        count = 0
        for o in state.battlefield:
            if o.id == source.id:
                continue
            if o.controller_id != controller_id:
                continue
            if o.printing.is_creature or is_artifact_now(state, o):
                count += 1
        if count <= 0:
            return
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, source, count)
        state.log.append(
            f"{source.printing.name} enters with {count} +1/+1 counters"
        )

    def describe(self) -> str:
        return (
            "put a +1/+1 counter on this creature for each other creature "
            "and/or artifact you control"
        )



@dataclass(frozen=True)
class PutChargeCountersOnTargetSpacecraftYouControl(Effect):
    """Put ``amount`` charge counters on a Spacecraft you control.

    Spacecraft-only variant. Drill Too Deep uses the broader
    ``PutChargeCountersOnTargetSpacecraftOrPlanetYouControl`` (Planet is
    a Land subtype that IS modeled — see ``make_planet_land``).
    """

    amount: int = 5

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.printing.is_spacecraft
            and o.controller_id == controller_id
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
        if obj is None or not obj.printing.is_spacecraft:
            return
        from mtgz_ir.game import add_charge_counters
        added = add_charge_counters(state, obj, self.amount)
        state.log.append(
            f"{obj.printing.name} gets {added} charge counters "
            f"(now {obj.charge_counters})"
        )

    def describe(self) -> str:
        return (
            f"put {self.amount} charge counters on target Spacecraft you "
            f"control"
        )



@dataclass(frozen=True)
class PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(Effect):
    """Put ``amount`` charge counters on target Spacecraft or Planet you
    control.

    Matches Drill Too Deep's first mode in its full printed form. Planet
    is a Land subtype in this codebase (see ``make_planet_land``); a
    permanent qualifies if it's a Spacecraft (artifact subtype) OR has
    the ``Planet`` subtype, AND the controller matches.
    """

    amount: int = 5

    @staticmethod
    def _qualifies(printing) -> bool:
        return printing.is_spacecraft or "Planet" in printing.subtypes

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if self._qualifies(o.printing)
            and o.controller_id == controller_id
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
        if obj is None or not self._qualifies(obj.printing):
            return
        if obj.controller_id != controller_id:
            return
        obj.charge_counters += self.amount
        state.log.append(
            f"{obj.printing.name} gets {self.amount} charge counters "
            f"(now {obj.charge_counters})"
        )

    def describe(self) -> str:
        return (
            f"put {self.amount} charge counters on target Spacecraft or "
            f"Planet you control"
        )



@dataclass(frozen=True)
class PutChargeCountersOnSelf(Effect):
    """Put N charge counters on the resolving source.

    ``amount`` is the fixed count when ``amount_from_x`` is False.
    When ``amount_from_x`` is True, reads ``state.resolving_x_value``
    instead — used by Blast Zone's "{X}{X}, {T}: Put X charge counters
    on this land".

    Untargeted; the source is whatever's currently resolving
    (``state.resolving_source``). Used both as an ETB effect (entering
    with N charge counters) and as an activated-ability payload.
    """

    amount: int = 1
    amount_from_x: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        n = state.resolving_x_value if self.amount_from_x else self.amount
        if n <= 0:
            return
        from mtgz_ir.game import add_charge_counters
        added = add_charge_counters(state, source, n)
        state.log.append(
            f"{source.printing.name} gets {added} charge counter"
            f"{'s' if added != 1 else ''} (now {source.charge_counters})"
        )

    def describe(self) -> str:
        if self.amount_from_x:
            return "put X charge counters on this permanent"
        return (
            f"put {self.amount} charge counter"
            f"{'s' if self.amount != 1 else ''} on this permanent"
        )



@dataclass(frozen=True)
class PutStunCounterOnTappedCreature(Effect):
    """Put a stun counter on up to one target tapped creature. Matches
    Cryogen Relic's activated ability. "Up to one" → required target
    in our simplified pipeline; silently fizzles if none legal."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature and o.tapped
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None or not obj.tapped:
            return
        from mtgz_ir.game import add_stun_counters
        add_stun_counters(state, obj, 1)
        state.log.append(
            f"{obj.printing.name} gets a stun counter "
            f"(now {obj.stun_counters})"
        )

    def describe(self) -> str:
        return "put a stun counter on up to one target tapped creature"



@dataclass(frozen=True)
class PutPlusOneCounterOnTargetCreature(PutPlusOneCounters):
    """``PutPlusOneCounters`` constructor sugar for "put N +1/+1
    counter(s) on target creature" (Honor's bare form).

    Filter is :class:`IsCreature` (& :class:`IsAttacking` when
    ``attacking_only`` is True — Hospital Room half).
    """

    attacking_only: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsAttacking, IsCreature
        from mtgz_ir.effects.targets import TargetSpec
        f = IsCreature() & IsAttacking() if self.attacking_only else IsCreature()
        object.__setattr__(self, "targets", (TargetSpec(f),))

    def describe(self) -> str:
        scope = "target attacking creature" if self.attacking_only else "target creature"
        if self.amount == 1:
            return f"put a +1/+1 counter on {scope}"
        return f"put {self.amount} +1/+1 counters on {scope}"



@dataclass(frozen=True)
class PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl(Effect):
    """Put N +1/+1 counters on target creature you control AND grant it
    keywords until end of turn (a single chosen target).

    Models the third mode of Archenemy's Charm ("Put two +1/+1 counters
    on target creature you control. It gains lifelink until end of
    turn."). Done as a single combined effect rather than a Sequence
    because Sequence currently allows at most one targeting sub-effect.
    """

    amount: int = 2
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    you_control_only: bool = True

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
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
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
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, obj, self.amount)
        if self.granted_keywords:
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn

            ApplyEffect(
                inner=GrantKeywords(keywords=self.granted_keywords),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, (("creature", obj.id),))
        state.log.append(
            f"{obj.printing.name} gets {self.amount} +1/+1 counter(s) "
            f"(now {obj.plus_one_counters})"
            + (
                f" and gains {', '.join(sorted(self.granted_keywords))} "
                f"until end of turn"
                if self.granted_keywords else ""
            )
        )

    def describe(self) -> str:
        kw = (
            f". It gains {', '.join(sorted(self.granted_keywords))} "
            f"until end of turn"
            if self.granted_keywords else ""
        )
        return (
            f"put {self.amount} +1/+1 counters on target creature you "
            f"control{kw}"
        )



@dataclass(frozen=True)
class PutPlusOneCounterOnEachCreatureYouControl(Effect):
    """Put ``amount`` +1/+1 counter(s) on each creature the controller
    controls at resolution. Matches Atmospheric Greenhouse's ETB.

    ``exclude_source=True`` models "each other creature you control" — reads
    ``state.resolving_source`` to skip the triggering permanent (e.g.
    Dawnstrike Vanguard's end-step trigger).
    """

    amount: int = 1
    exclude_source: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters
        source = state.resolving_source if self.exclude_source else None
        count = 0
        for obj in state.battlefield:
            if obj.controller_id != controller_id:
                continue
            if not obj.printing.is_creature:
                continue
            if source is not None and obj is source:
                continue
            add_plus_one_counters(state, obj, self.amount)
            count += 1
        scope = "other creature(s)" if self.exclude_source else "creature(s)"
        state.log.append(
            f"P{controller_id}'s {count} {scope} each get "
            f"a +1/+1 counter"
        )

    def describe(self) -> str:
        scope = (
            "each other creature you control"
            if self.exclude_source
            else "each creature you control"
        )
        if self.amount == 1:
            return f"put a +1/+1 counter on {scope}"
        return f"put {self.amount} +1/+1 counters on {scope}"



@dataclass(frozen=True)
class PutPlusOneCounterOnSelf(Effect):
    """Put ``amount`` +1/+1 counter(s) on the ability's source ("this creature").

    Reads ``state.resolving_source`` to find the source — it's set by
    ``resolve_top_of_stack`` during ability resolution.
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters
        source = state.resolving_source
        if source is None:
            return
        add_plus_one_counters(state, source, self.amount)
        state.log.append(
            f"{source.printing.name} gets a +1/+1 counter "
            f"(now {source.plus_one_counters})"
        )

    def describe(self) -> str:
        if self.amount == 1:
            return "put a +1/+1 counter on this creature"
        return f"put {self.amount} +1/+1 counters on this creature"



@dataclass(frozen=True)
class PutPlusOneCounterOnOpponentsCreature(PutPlusOneCounters):
    """``PutPlusOneCounters`` constructor sugar for "put a +1/+1 counter
    on target creature an opponent controls" (Sunset Saboteur's
    attacks trigger).

    Filter is ``IsCreature() & ControlledByOpponent()``. The counter-
    doubling replacement (Loading Zone-style) is gated on the
    recipient's controller per the ``add_plus_one_counters`` helper —
    a doubler controlled by the spell's caster (the opponent of the
    target's controller) doesn't fire because doublers check the
    receiving creature's controller.
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

    def describe(self) -> str:
        return "put a +1/+1 counter on target creature an opponent controls"



@dataclass(frozen=True)
class PutPlusOneCounterOnCreatureYouControl(PutPlusOneCounters):
    """``PutPlusOneCounters`` constructor sugar for "put N +1/+1
    counter(s) on target creature you control" (Rayblade Trooper).

    Filter is ``IsCreature() & ControlledByYou()``.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            ControlledByYou, IsCreature,
        )
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsCreature() & ControlledByYou()),),
        )

    def describe(self) -> str:
        if self.amount == 1:
            return "put a +1/+1 counter on target creature you control"
        return (
            f"put {self.amount} +1/+1 counters on target creature you control"
        )
