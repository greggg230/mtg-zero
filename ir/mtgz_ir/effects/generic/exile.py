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
class Exile(Effect):
    """Unified exile primitive driven by ``targets``.

    Sibling of :class:`Destroy` for the exile family. The unified
    resolve walks each slot's chosen target, re-verifies legality at
    resolution per CR 608.2b, and exiles whichever survive the
    recheck via ``move_off_battlefield(..., destination="exile")``.

    Variants that need extra resolve logic (Mystifying Maze's
    delayed return-trigger, Oblivion-Ring's "until source leaves"
    bookkeeping) override ``resolve`` rather than reusing this base
    — ``Exile.resolve`` covers the bare "exile target X" oracle
    shape, not the until-clause/leave-trigger riders.
    """

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec
        from mtgz_ir.turn import move_off_battlefield

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
                        _fizzle_message(state, chosen, action="exile")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                state.log.append(f"{obj.printing.name} is exiled")
                move_off_battlefield(state, obj, destination="exile")
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return f"exile {_describe_targets(self.targets)}"


@dataclass(frozen=True)
class ReturnExiledPermanentToOwnerTapped(Effect):
    """Return a specific exiled card (``exile_card_id``) to the
    battlefield under its owner's control, tapped.

    Unlike the targeted-by-agent return effects, this one stores the
    exact card id at construction time. Intended for scheduled delayed
    triggers where the card was already selected at schedule time (see
    ExileTargetAttackingCreatureUntilEndStep / Mystifying Maze).
    """

    exile_card_id: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        owner = None
        card = None
        for p in state.players:
            for c in p.exile:
                if c.id == self.exile_card_id:
                    owner = p
                    card = c
                    break
            if card is not None:
                break
        if card is None or owner is None:
            return
        owner.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        put_into_battlefield(state, card, owner.id)
        card.tapped = True
        state.log.append(
            f"{card.printing.name} returns from exile to P{owner.id}'s "
            f"battlefield tapped"
        )

    def describe(self) -> str:
        return (
            f"return exiled card #{self.exile_card_id} to its owner's "
            f"battlefield tapped"
        )



@dataclass(frozen=True)
class WarpExileSelf(Effect):
    """Delayed-triggered exile of a warp-cast creature at end step.

    Scheduled at warp-cast time onto ``state.delayed_triggers`` with
    ``source_id`` set to the warped card's id. When the delayed-trigger
    machinery converts the entry into a real triggered ability and
    pushes it onto the stack, the StackItem's ``source`` is the warp
    permanent (or stays as the original GameObject reference even if
    the permanent has since left). On resolve, ``state.resolving_source``
    is that GameObject — but we re-check the battlefield by id before
    acting because the card may have left between the trigger going on
    the stack and resolving (countered, died, bounced, exiled).

    No-op if the source has left the battlefield — the trigger still
    "fires" per CR 603.6c, it just has no permanent to act on.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        # ``state.resolving_source`` is set to the StackItem's source
        # GameObject — that reference persists even if the warp creature
        # has since left the battlefield. Re-look up by id on the live
        # battlefield to honor the no-op-if-gone rule (CR 603.6c).
        ref = state.resolving_source
        if ref is None:
            return
        obj = next(
            (o for o in state.battlefield if o.id == ref.id), None,
        )
        if obj is None:
            return
        state.log.append(f"{obj.printing.name} is warp-exiled")
        move_off_battlefield(state, obj, destination="exile")
        if not obj.printing.is_token:
            obj.warped = True
        obj.cast_via_warp = False

    def describe(self) -> str:
        return "exile this warp-cast permanent"



@dataclass(frozen=True)
class ExileTargetAttackingCreatureUntilEndStep(Effect):
    """Mystifying Maze: "Exile target attacking creature an opponent
    controls. At the beginning of the next end step, that player
    returns it to the battlefield under its owner's control tapped."

    On resolve: exiles the chosen creature (without the
    Oblivion-Ring-style ``exiled_by_source`` marker so the creature
    stays in exile even if Mystifying Maze later leaves the
    battlefield), then schedules a delayed end-step trigger that
    resolves a ReturnExiledPermanentToOwnerTapped for that card.

    The delayed trigger's ``controller_id`` is the CURRENT active
    player — Maze's activation is instant-speed on an opponent's
    combat, so "next end step" is the active player's own end step.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.attacking
            and o.controller_id != controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        _, tid = targets[0]
        target = next(
            (o for o in state.battlefield if o.id == tid), None,
        )
        if target is None:
            return
        # The Mystifying Maze itself is the source of the delayed
        # triggered ability: per CR 603.7c the ability is created by
        # Maze's resolution and goes on the stack at the next end step
        # under Maze's controller (the active player). We need a
        # source GameObject for the StackItem; ``state.resolving_source``
        # is Maze during this resolve.
        maze_source_id = (
            state.resolving_source.id
            if state.resolving_source is not None else None
        )
        state.log.append(
            f"{target.printing.name} is exiled until end of turn"
        )
        move_off_battlefield(state, target, destination="exile")
        entry: dict = {
            "fire_at_turn": state.turn_number,
            "controller_id": state.active_player().id,
            "effect": ReturnExiledPermanentToOwnerTapped(
                exile_card_id=target.id,
            ),
        }
        if maze_source_id is not None:
            entry["source_id"] = maze_source_id
        state.delayed_triggers.append(entry)

    def describe(self) -> str:
        return (
            "exile target attacking creature an opponent controls; its "
            "controller returns it to the battlefield under its "
            "owner's control tapped at the beginning of the next end "
            "step"
        )



@dataclass(frozen=True)
class ExileEachOpponentsGraveyard(Effect):
    """Move every card in each opponent's graveyard to that player's exile."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        total = 0
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            cards_to_exile = list(p.graveyard)
            for card in cards_to_exile:
                p.graveyard.remove(card)
                card.zone = Zone.EXILE
                p.exile.append(card)
                total += 1
        state.log.append(
            f"P{controller_id} exiles {total} card(s) from opponent graveyards"
        )

    def describe(self) -> str:
        return "exile each opponent's graveyard"



@dataclass(frozen=True)
class ExileTargetCardFromGraveyard(Effect):
    """Exile target card from a graveyard. Either player's graveyard is a
    legal target source — the chosen card moves to its owner's exile zone
    and (when the source supports the linkage) carries
    ``exiled_by_source`` so a sibling activated ability can recall every
    card the same source exiled later. DSK Ghost Vacuum's
    ``{T}: Exile target card from a graveyard.`` is the introducing
    printing.

    Mirrors ``PutCardFromGraveyardOnBottomOfLibrary``'s
    ``("card_in_graveyard", id)`` target-kind shape — the same target
    kind the existing graveyard-bottom effect emits, so the targeting
    pipeline doesn't need a new kind for this primitive.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import Zone

        return [
            ("card_in_graveyard", c.id)
            for p in state.players
            for c in p.graveyard
            if c.zone == Zone.GRAVEYARD
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "card_in_graveyard":
            return
        source = state.resolving_source
        for p in state.players:
            card = next((c for c in p.graveyard if c.id == target_id), None)
            if card is None:
                continue
            p.graveyard.remove(card)
            card.zone = Zone.EXILE
            # Tag the exile site so a sibling activated ability on the
            # same source (Ghost Vacuum's ``{6}, {T}, Sacrifice this
            # artifact`` recall) can find every card the same source
            # exiled later. The link is harmless when the source has no
            # such recall ability; ``move_off_battlefield`` clears the
            # tag on any release path that returns the card from exile.
            if source is not None:
                card.exiled_by_source = source.id
            p.exile.append(card)
            state.log.append(
                f"{card.printing.name} exiled from P{p.id}'s graveyard"
            )
            return

    def describe(self) -> str:
        return "exile target card from a graveyard"



@dataclass(frozen=True)
class ExileTopOfLibraryMayPlayThisTurn(Effect):
    """Controller exiles the top ``amount`` cards of their own library
    and marks each with ``playable_from_exile_this_turn`` so they're
    castable / playable until cleanup of the resolving turn.

    Matches Charred Foyer's upkeep trigger ("At the beginning of your
    upkeep, exile the top card of your library. You may play it this
    turn.") and any other "exile the top N cards of your library; you
    may play them this turn" shape that doesn't carry an
    until-next-turn rider. The flag clears at cleanup of the casting
    turn via the existing turn.run_cleanup loop — same lifetime that
    impulse-cast flag uses everywhere else (Territorial Bruntar,
    Memorial Vault, Possibility Technician).

    ``amount`` defaults to 1 (the Charred Foyer shape); call sites
    that exile multiple cards (Painter's Studio's "exile the top two
    cards" — when "until end of next turn" support lands) pass a
    different value. Running out of cards mid-exile simply stops; no
    empty-library loss is triggered (impulse-cast at sorcery speed
    doesn't draw, so the empty-library loss check doesn't fire here).
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        exiled = 0
        for _ in range(self.amount):
            if not p.library:
                break
            top = p.library.pop(0)
            top.zone = Zone.EXILE
            top.playable_from_exile_this_turn = True
            p.exile.append(top)
            exiled += 1
        state.log.append(
            f"P{controller_id} exiles {exiled} card(s) from top of "
            f"library (may play this turn)"
        )

    def describe(self) -> str:
        if self.amount == 1:
            return (
                "exile the top card of your library; you may play it "
                "this turn"
            )
        return (
            f"exile the top {self.amount} cards of your library; you "
            f"may play them this turn"
        )



@dataclass(frozen=True)
class ExileTopXOfLibraryMayPlayThisTurn(Effect):
    """Formula-driven sibling of ``ExileTopOfLibraryMayPlayThisTurn``.

    Evaluates ``amount`` (a ``Formula``) at resolve time, exiles that
    many cards off the top of the controller's library, and marks each
    with ``playable_from_exile_this_turn`` so the controller can cast /
    play them until cleanup of the resolving turn. Same lifetime and
    castability semantics as the fixed-amount sibling — the cleanup
    loop in ``turn.run_cleanup_step`` clears the flag for every card
    sitting in exile at end of turn.

    Memorial Vault uses ``amount=MVOfCostPaidObjectPlus(constant=1)`` to
    pull X from the sacrificed artifact's mana value at resolve time.
    Any future card whose body says "exile the top X cards of your
    library, where X is <runtime expression>; you may play them this
    turn" composes a different ``Formula`` and reuses this engine
    class. Running out of cards mid-exile simply stops; no empty-
    library loss is triggered (impulse-cast at sorcery speed doesn't
    draw, so the empty-library loss check doesn't fire here).
    """

    amount: "Formula | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if self.amount is None:
            return
        n = self.amount.value(state, controller_id)
        if n < 0:
            n = 0
        p = state.player_by_id(controller_id)
        exiled = 0
        for _ in range(n):
            if not p.library:
                break
            top = p.library.pop(0)
            top.zone = Zone.EXILE
            top.playable_from_exile_this_turn = True
            p.exile.append(top)
            exiled += 1
        state.log.append(
            f"P{controller_id} exiles {exiled} card(s) from top of "
            f"library (may play this turn)"
        )

    def describe(self) -> str:
        return (
            "exile the top X cards of your library; "
            "you may play them this turn"
        )



@dataclass(frozen=True)
class ExileAllArtifactsWithManaValueAtMost(Effect):
    """Exile every artifact on the battlefield with mana value ≤ ``max_mv``.

    Ruinous Rampage's anti-artifact mode. Lander tokens (MV 0) are hit.
    """

    max_mv: int = 3

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now
        from mtgz_ir.turn import move_off_battlefield

        victims = [
            o for o in state.battlefield
            if is_artifact_now(state, o) and o.printing.mana_cost.total <= self.max_mv
        ]
        for obj in victims:
            state.log.append(f"{obj.printing.name} is exiled (mass-artifact)")
            move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return (
            f"exile all artifacts with mana value {self.max_mv} or less"
        )



@dataclass(frozen=True)
class ExileTargetCreatureUntilSourceLeaves(Effect):
    """Exile any target creature until the source leaves the battlefield.

    Matches All-Fates Stalker's ETB and Hardlight Containment's ETB.
    Uses the Oblivion-Ring-style ``exiled_by_source`` bookkeeping so
    the exiled creature returns when the source leaves play.

    ``excluded_subtypes`` filters out creatures whose printed subtypes
    overlap with any member (e.g., ``frozenset({"Assassin"})`` for
    All-Fates Stalker's "non-Assassin creature" clause). Empty = no
    subtype restriction.

    ``opponents_only`` restricts legal targets to creatures controlled
    by a player OTHER than the effect's controller (Hardlight
    Containment: "target creature an opponent controls"). Default False.

    ``yours_only`` restricts legal targets to creatures the effect's
    controller controls (Endless Sands: "target creature you control").
    Mutually exclusive with ``opponents_only``.

    ``up_to_one``: "exile up to one target ..." (All-Fates Stalker): zero
    or one target (CR 115.1); with none the effect does nothing.
    """

    excluded_subtypes: frozenset[str] = frozenset()
    opponents_only: bool = False
    yours_only: bool = False
    up_to_one: bool = False

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1) if self.up_to_one else (1, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and not (
                self.excluded_subtypes
                and any(
                    s in self.excluded_subtypes
                    for s in _eff_subtypes(state, o)
                )
            )
            and not (
                self.opponents_only and o.controller_id == controller_id
            )
            and not (
                self.yours_only and o.controller_id != controller_id
            )
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
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
        from mtgz_ir.turn import move_off_battlefield

        obj.exiled_by_source = source.id
        state.log.append(
            f"{obj.printing.name} is exiled by {source.printing.name}"
        )
        move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return (
            "exile target creature until this creature leaves the "
            "battlefield"
        )



@dataclass(frozen=True)
class ExileOpponentsNonlandPermanentUntilLeaves(Effect):
    """ETB exile target nonland permanent an opponent controls, optionally
    restricted by mana value. The exiled card returns when the source
    enchantment leaves the battlefield (Oblivion Ring / Banishing Light
    pattern). Uses state.resolving_source as the "source" for tracking.

    ``max_mv`` = 0 means no MV filter (Banishing Light); > 0 restricts
    to permanents with mana value ≤ max_mv (Seam Rip).
    """

    max_mv: int = 0

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        out: list[Target] = []
        for o in state.battlefield:
            if o.printing.is_land:
                continue
            if o.controller_id == controller_id:
                continue
            if self.max_mv and o.printing.mana_cost.total > self.max_mv:
                continue
            out.append(("permanent", o.id))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None or obj.printing.is_land:
            return
        if obj.controller_id == controller_id:
            return
        if self.max_mv and obj.printing.mana_cost.total > self.max_mv:
            return
        from mtgz_ir.turn import move_off_battlefield

        obj.exiled_by_source = source.id
        state.log.append(
            f"{obj.printing.name} is exiled by {source.printing.name}"
        )
        move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        if self.max_mv:
            return (
                f"exile target nonland permanent an opponent controls "
                f"with mana value {self.max_mv} or less until this "
                f"enchantment leaves the battlefield"
            )
        return (
            "exile target nonland permanent an opponent controls until "
            "this enchantment leaves the battlefield"
        )



@dataclass(frozen=True)
class ExileAllGraveyards(Effect):
    """Scavenger Grounds: `{2}, {T}, Sacrifice a Desert: Exile all
    graveyards.` Every card in every player's graveyard is moved to
    that player's exile.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        total = 0
        for p in state.players:
            for c in list(p.graveyard):
                p.graveyard.remove(c)
                c.zone = Zone.EXILE
                p.exile.append(c)
                total += 1
        state.log.append(f"all graveyards exiled ({total} cards)")

    def describe(self) -> str:
        return "exile all graveyards"



@dataclass(frozen=True)
class ExileAllCreaturesAndSpacecraft(Effect):
    """Exile every creature on the battlefield, as well as Spacecraft (whether
    or not the Spacecraft has been stationed into creature mode).

    Matches Beyond the Quiet. Mass wrath-style effect: no targets, acts on
    the full battlefield snapshot at resolution. Tokens cease to exist per
    rule 704.5d.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        victims = [
            o for o in state.battlefield
            if o.printing.is_creature or o.printing.is_spacecraft
        ]
        for obj in victims:
            state.log.append(f"{obj.printing.name} is exiled (mass)")
            move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return "exile all creatures and Spacecraft"



@dataclass(frozen=True)
class ExileTargetCreature(Exile):
    """``Exile`` constructor sugar for "exile target creature".

    Filter is :class:`IsCreature` (which reads ``is_creature_now`` so
    Impending-suppressed Overlords aren't legal targets). Tokens
    exiled cease to exist per CR 704.5d.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(self, "targets", (TargetSpec(IsCreature()),))



@dataclass(frozen=True)
class ExileTargetCreatureWithPowerOrToughnessAtMost(Exile):
    """``Exile`` constructor sugar for "exile target creature with power
    or toughness N or less" (Warping Wail mode 1, Scryfall-updated
    oracle).

    Filter is ``IsCreature() & (HasPowerAtMost(n) | HasToughnessAtMost(n))``.
    Both predicates read ``effective_power`` / ``effective_toughness``
    so temp buffs and static anthems are respected at both targeting
    and the at-resolve recheck (a Giant-Growth-style pump between cast
    and resolve will fail the recheck and fizzle the exile).
    """

    max_value: int = 1

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            HasPowerAtMost, HasToughnessAtMost, IsCreature,
        )
        from mtgz_ir.effects.targets import TargetSpec
        f = IsCreature() & (
            HasPowerAtMost(self.max_value)
            | HasToughnessAtMost(self.max_value)
        )
        object.__setattr__(self, "targets", (TargetSpec(f),))

    def describe(self) -> str:
        return (
            f"exile target creature with power or toughness "
            f"{self.max_value} or less"
        )



@dataclass(frozen=True)
class ExileAllMatchingUntilSourceLeaves(Effect):
    """Mass-exile every battlefield permanent matching the type+MV filter
    until the source leaves the battlefield.

    Parameters describe a printed-text type union ("all artifacts and
    creatures") and an optional mana-value cap. When the source leaves
    play, the existing ``exiled_by_source`` bookkeeping returns each
    exiled card to the battlefield (Banishing Light family). Tokens
    cease to exist on exile per CR 704.5d — they are caught in the
    sweep and don't return.

    Used by Pinnacle Starcage's ETB ("Exile all artifacts and creatures
    with mana value 2 or less until this artifact leaves the battlefield"):
    ``match_artifact=True, match_creature=True, max_mv=2``.

    The source itself is excluded from the sweep — it can't exile itself.
    Lands are excluded by ``not is_land`` so an artifact land doesn't get
    caught accidentally; the printed text speaks of "artifacts and
    creatures", which by oracle convention excludes lands.
    """

    match_artifact: bool = False
    match_creature: bool = False
    max_mv: int = 0  # 0 = no MV cap

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now
        from mtgz_ir.turn import move_off_battlefield

        source = state.resolving_source
        if source is None:
            return
        victims = []
        for o in list(state.battlefield):
            if o is source:
                continue
            if o.printing.is_land:
                continue
            type_match = (
                (self.match_artifact and is_artifact_now(state, o))
                or (self.match_creature and o.printing.is_creature)
            )
            if not type_match:
                continue
            if self.max_mv and o.printing.mana_cost.total > self.max_mv:
                continue
            victims.append(o)
        for obj in victims:
            obj.exiled_by_source = source.id
            state.log.append(
                f"{obj.printing.name} exiled by {source.printing.name}"
            )
            move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        types: list[str] = []
        if self.match_artifact:
            types.append("artifacts")
        if self.match_creature:
            types.append("creatures")
        type_phrase = " and ".join(types) if types else "permanents"
        mv_clause = (
            f" with mana value {self.max_mv} or less" if self.max_mv else ""
        )
        return (
            f"exile all {type_phrase}{mv_clause} until this leaves the "
            f"battlefield"
        )


@dataclass(frozen=True)
class MoveEachExiledWithSourceToOwnersGraveyard(Effect):
    """Generic "put each card exiled with this permanent into its owner's
    graveyard" primitive.

    Walks every player's exile zone for cards tagged with
    ``GameObject.exiled_by_source == state.resolving_source.id`` and moves
    each one to its respective owner's graveyard. The
    ``exiled_by_source`` linkage is the same one used by the
    ``ExileAllMatchingUntilSourceLeaves`` and
    ``ExileTargetCreatureUntilSourceLeaves`` exile-side primitives — this
    primitive is the recall-side consumer that turns the linkage into a
    queryable runtime API.

    Records the count of cards moved on
    ``state.last_exile_recall_count`` so a follow-up sequence step
    (``CreateNTokensFromLastExileRecall``) can mint a token per card
    moved without coupling the two operations into a single effect class.
    The count is reset to 0 at the start of every ability resolve in
    ``actions.resolve_top_of_stack`` and overwritten here on every
    invocation, so the value is fresh per resolve and never bleeds
    across ability boundaries.

    Used by Pinnacle Starcage's ``{6}{W}{W}`` activation: "Put each
    card exiled with this artifact into its owner's graveyard, ..."
    Reusable for any future "put each card exiled with <source> into
    its owner's graveyard" printing.

    No-op (silent count of 0, never raises) if ``state.resolving_source``
    is None — the engine's resolution pipeline always sets it for
    ability resolution, so a None here would be programmer error
    upstream rather than a normal flow.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        state.last_exile_recall_count = 0
        source = state.resolving_source
        if source is None:
            return
        moved = 0
        for p in state.players:
            tagged = [c for c in p.exile if c.exiled_by_source == source.id]
            for card in tagged:
                p.exile.remove(card)
                card.exiled_face_down = False  # left exile (CR 400.7)
                card.zone = Zone.GRAVEYARD
                card.exiled_by_source = None
                p.graveyard.append(card)
                moved += 1
        state.last_exile_recall_count = moved
        if source is not None:
            state.log.append(
                f"{source.printing.name} moves {moved} exiled cards to "
                f"their owners' graveyards"
            )

    def describe(self) -> str:
        return (
            "put each card exiled with this permanent into its "
            "owner's graveyard"
        )



@dataclass(frozen=True)
class CreateNTokensFromLastExileRecall(Effect):
    """Generic "create N <token> tokens, where N is the number of cards
    just moved out of exile by the prior sequence step" primitive.

    Reads ``state.last_exile_recall_count`` (set by the immediately
    prior ``MoveEachExiledWithSourceToOwnersGraveyard`` resolve in the
    same ability) and creates that many tokens using the named token
    printing from ``TOKEN_PRINTINGS``. Ordering matters: this primitive
    must follow the recall primitive in a ``Sequence``; running it
    standalone (with no prior recall) creates zero tokens — the same
    behavior as creating zero tokens from a zero-count source.

    Used by Pinnacle Starcage's ``{6}{W}{W}`` activation: "..., then
    create a 2/2 colorless Robot artifact creature token for each card
    put into a graveyard this way." Reusable for any future "create
    one <token> per card just moved" surface — the count source is
    abstracted into the engine state field, so the parser only needs
    to plumb the token name.
    """

    token_name: str
    # The token's printing as the card prints it ("a 2/2 colorless Robot
    # artifact creature token"); ``None`` falls back to the registry
    # entry ``TOKEN_PRINTINGS[token_name]``.
    token_printing: "object | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone

        count = state.last_exile_recall_count
        if count <= 0:
            return
        printing = self.token_printing or TOKEN_PRINTINGS[self.token_name]
        for _ in range(count):
            token = state.new_object(printing, controller_id, Zone.BATTLEFIELD)
            token.entered_battlefield_turn = state.turn_number
            state.battlefield.append(token)
        state.log.append(
            f"P{controller_id} creates {count} {self.token_name} tokens "
            f"(one per card just put into a graveyard from exile)"
        )

    def describe(self) -> str:
        return (
            f"create a {self.token_name} token for each card put into a "
            f"graveyard this way"
        )



@dataclass(frozen=True)
class SacrificeSelfIfNoNamedCounters(Effect):
    """"If there are no <kind> counters on this <permanent>, sacrifice
    it." (Gemstone Mine's mana-ability rider). Checks the resolving
    source's ``named_counters`` and sacrifices it when none of ``kind``
    remain. No-op if the source already left the battlefield."""

    kind: str

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import named_counter_count
        from mtgz_ir.turn import sacrifice as _sacrifice

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        if named_counter_count(source, self.kind) == 0:
            _sacrifice(state, source)

    def describe(self) -> str:
        return (
            f"if there are no {self.kind} counters on this permanent, "
            f"sacrifice it"
        )


@dataclass(frozen=True)
class SacrificeSelf(Effect):
    """Generic "sacrifice this permanent" effect-body primitive.

    Used as the LAST step of an effect Sequence whose printed body ends
    in ``Sacrifice this <permanent type>.`` (Pinnacle Starcage's
    ``{6}{W}{W}: ... Sacrifice this artifact.``). Distinct from
    ``ActivationCost.sacrifice_self`` (which is paid at activation time,
    before the ability hits the stack) and from
    ``SacrificeSourceDelayed`` (which fires from a delayed-trigger
    schedule).

    Reads ``state.resolving_source`` and routes through
    ``mtgz_ir.turn.sacrifice``. No-op if the source has already left
    the battlefield (e.g. an earlier sequence step destroyed it) or if
    ``resolving_source`` is None.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool:
        from mtgz_ir.turn import sacrifice as _sacrifice

        source = state.resolving_source
        if source is None:
            return False
        if source not in state.battlefield:
            return False
        _sacrifice(state, source)
        # ``True`` lets ``IfYouDo`` gate "you may sacrifice this. If you
        # do, …" (The Fantasticar) on the sacrifice actually happening.
        return True

    def describe(self) -> str:
        return "sacrifice this permanent"



@dataclass(frozen=True)
class PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl(Effect):
    """Generic "put each creature card exiled with this permanent onto the
    battlefield under your control" primitive.

    Walks every player's exile zone for cards tagged with
    ``GameObject.exiled_by_source == state.resolving_source.id`` filtered
    to creature cards (``printing.is_creature``) and brings each onto
    the activator's battlefield via ``put_into_battlefield`` (so ETBs
    fire normally). The ``exiled_by_source`` linkage is cleared on each
    recalled card so a future leave-the-battlefield event on the
    source doesn't try to release them again.

    Optional riders parameterize the per-card grants Ghost Vacuum's
    activation prints ("with a flying counter on it" + "Each of them is
    a 1/1 Spirit in addition to its other types"):

    * ``grant_flying_counter=True`` — each recalled creature gets a
      ``flying counter`` (``GameObject.flying_counters`` integer field;
      ``effective_keywords`` adds ``"flying"`` whenever that field is
      positive, per the named-counter grant rule CR 122).
    * ``grant_subtype_and_pt=("Spirit", 1, 1)`` — each recalled
      creature picks up a layer-4 ``add_subtype`` continuous effect
      (so the printed creature's subtype list gains the named subtype
      "in addition to its other types") and a layer-7 sublayer-1
      ``set_base_pt`` continuous effect (overriding the printed P/T
      to the supplied tuple per CR 613 sublayer 7b). Both entries
      have ``expires_at=None`` so the grant persists.

    Used by Ghost Vacuum's ``{6}, {T}, Sacrifice this artifact``
    activation. Reusable for any future "put each creature card exiled
    with <source> onto the battlefield under your control" printing —
    the rider parameters are optional, so the bare recall surface is
    just ``PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYour
    Control()``.

    Non-creature exile-tagged cards are left in exile (the printed
    filter is "each creature card"). No-op when the source is None.
    """

    grant_flying_counter: bool = False
    # Tuple of (subtype, base_power, base_toughness). When set, every
    # recalled creature gets the layer-4/7 grants described above.
    grant_subtype_and_pt: tuple[str, int, int] | None = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        source = state.resolving_source
        if source is None:
            return
        moved = []
        for p in state.players:
            tagged = [c for c in p.exile if c.exiled_by_source == source.id]
            for card in tagged:
                if not card.printing.is_creature:
                    continue
                p.exile.remove(card)
                card.exiled_face_down = False  # left exile (CR 400.7)
                card.exiled_by_source = None
                moved.append(card)
        if not moved:
            state.log.append(
                f"{source.printing.name} resolves: no exiled creature "
                f"cards to recall"
            )
            return
        for card in moved:
            put_into_battlefield(state, card, controller_id)
            if self.grant_flying_counter:
                card.flying_counters += 1
            if self.grant_subtype_and_pt is not None:
                subtype, bp, bt = self.grant_subtype_and_pt
                state.add_continuous_effect(
                    source_id=source.id, layer=4, kind="add_subtype",
                    target_id=card.id, params=(subtype,),
                    expires_at=None,
                    description=(
                        f"{card.printing.name} is a {bp}/{bt} {subtype} "
                        f"({source.printing.name})"
                    ),
                )
                state.add_continuous_effect(
                    source_id=source.id, layer=7, sublayer=1,
                    kind="set_base_pt",
                    target_id=card.id, params=(bp, bt),
                    expires_at=None,
                    description=(
                        f"{card.printing.name} base {bp}/{bt} "
                        f"({source.printing.name})"
                    ),
                )
            state.log.append(
                f"{card.printing.name} returns from exile to "
                f"P{controller_id}'s battlefield"
                + (
                    f" as a {self.grant_subtype_and_pt[1]}/"
                    f"{self.grant_subtype_and_pt[2]} "
                    f"{self.grant_subtype_and_pt[0]}"
                    if self.grant_subtype_and_pt is not None
                    else ""
                )
                + (
                    " with a flying counter"
                    if self.grant_flying_counter else ""
                )
            )

    def describe(self) -> str:
        rider_bits = []
        if self.grant_flying_counter:
            rider_bits.append("with a flying counter on it")
        if self.grant_subtype_and_pt is not None:
            subtype, bp, bt = self.grant_subtype_and_pt
            rider_bits.append(
                f"each is a {bp}/{bt} {subtype} in addition to "
                f"its other types"
            )
        rider = (" " + "; ".join(rider_bits)) if rider_bits else ""
        return (
            "put each creature card exiled with this permanent onto the "
            "battlefield under your control" + rider
        )



@dataclass(frozen=True)
class ReanimateOneExiledWithSourceWithCounters(Effect):
    """Generic "put one creature card exiled with this permanent onto the
    battlefield under your control with N additional +1/+1 counters"
    primitive.

    Walks every player's exile zone for cards tagged with
    ``GameObject.exiled_by_source == state.resolving_source.id`` filtered
    to creature cards (``printing.is_creature``), asks the resolving
    controller's agent to pick ONE via ``choose_target`` (the natural
    "pick one from a list of GameObject ids" prompt — re-used for non-
    spell choices the same way ``choose_sacrifice`` is re-used elsewhere),
    pulls it out of exile, and brings it onto the controller's
    battlefield via ``put_into_battlefield`` (so ETBs fire normally) with
    ``counter_amount`` extra +1/+1 counters added on top of whatever the
    creature's printed body would otherwise place.

    Differs from ``PutEachCreatureCardExiledWithSourceOntoBattlefield
    UnderYourControl`` (which recalls every tagged creature) in two
    ways: (a) only ONE card is recalled, picked by the controller; and
    (b) the recalled creature gets +N +1/+1 counters added at ETB time.
    Matches Sothera, the Supervoid's end-step recall surface verbatim.

    The ``exiled_by_source`` linkage is cleared on the recalled card so
    a future leave-the-battlefield event on the source doesn't try to
    release it again. Other tagged cards remain in exile with their
    linkage intact (so a sibling ability — none in the EoE pool today,
    but the contract is honest — could still find them).

    No-op (no log spam, no error) when ``state.resolving_source`` is
    None, when no tagged creature card exists in any exile zone, or
    when the resolving controller has no agent (defensive — every
    real game seat has one).
    """

    counter_amount: int = 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import add_plus_one_counters

        source = state.resolving_source
        if source is None:
            return
        # Collect every tagged creature card across all exile zones,
        # paired with its owner so we can remove it cleanly later.
        candidates: list[tuple["GameObject", "Player"]] = []  # noqa: F821
        for p in state.players:
            for c in p.exile:
                if (
                    c.exiled_by_source == source.id
                    and c.printing.is_creature
                ):
                    candidates.append((c, p))
        if not candidates:
            return
        # Ask the controller's agent to pick one. Re-use ``choose_target``
        # — the (kind, id) tuple shape is the natural fit for a "pick
        # one from a list" prompt; the ``"exile_creature"`` kind tag
        # is parser-side bookkeeping (the engine never re-validates it
        # because the candidate list is built right above).
        ctrl = state.player_by_id(controller_id)
        legal = [("exile_creature", obj.id) for obj, _ in candidates]
        chosen_obj = candidates[0][0]
        chosen_owner = candidates[0][1]
        if ctrl.agent is not None:
            picked = ctrl.agent.choose_target(state, controller_id, legal)
            if picked is not None:
                pick_id = picked[1]
                for obj, owner in candidates:
                    if obj.id == pick_id:
                        chosen_obj = obj
                        chosen_owner = owner
                        break
        chosen_owner.exile.remove(chosen_obj)
        chosen_obj.exiled_face_down = False  # left exile (CR 400.7)
        chosen_obj.exiled_by_source = None
        state.log.append(
            f"{source.printing.name} puts {chosen_obj.printing.name} onto "
            f"the battlefield under P{controller_id}'s control"
        )
        put_into_battlefield(state, chosen_obj, controller_id)
        if self.counter_amount > 0:
            add_plus_one_counters(state, chosen_obj, self.counter_amount)

    def describe(self) -> str:
        rider = (
            f" with {self.counter_amount} additional +1/+1 counters on it"
            if self.counter_amount > 0 else ""
        )
        return (
            "put a creature card exiled with this permanent onto the "
            "battlefield under your control" + rider
        )



@dataclass(frozen=True)
class SacrificeSelfPreservingExileLink(Effect):
    """Sacrifice this permanent, suppressing the on-leave auto-return of
    cards still tagged ``exiled_by_source = source.id``.

    Companion to ``ReanimateOneExiledWithSourceWithCounters``: Sothera,
    the Supervoid's printed end-step body says "sacrifice Sothera, then
    put a creature card exiled with it onto the battlefield...". A
    naive Sequence(SacrificeSelf, recall) would lose the source link —
    the engine's ``_release_exiled_until_leaves`` hook fires the moment
    Sothera leaves the battlefield, racing every tagged card back to
    its owner's battlefield BEFORE the recall step gets to read the
    exile zone. The engine-level fix is to invert the order at
    composition time (recall first, sac second) AND suppress the
    on-leave hook so the OTHER tagged cards (the ones not picked by the
    recall) don't race back either — the printed text doesn't say they
    should.

    The suppression flag ``state.suppress_exiled_by_source_on_leave_for``
    is one-shot: this effect sets it to the source's id immediately
    before sacrificing; ``_release_exiled_until_leaves`` clears the flag
    the first time it observes the matching source-id leave event and
    skips its auto-return loop for that one event. Subsequent leave
    events for the same source (impossible after a sac, but the contract
    is local) would auto-return any cards still linked, so the
    suppression's blast radius is exactly one leave event.

    No-op when ``state.resolving_source`` is None or the source has
    already left the battlefield (e.g. an earlier sequence step
    destroyed it — defensive; Sothera's body never composes with such
    a step today).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice

        source = state.resolving_source
        if source is None:
            return
        if source not in state.battlefield:
            return
        # Arm the one-shot suppression so the auto-return hook stays
        # idle for the leave event triggered by the sacrifice below.
        state.suppress_exiled_by_source_on_leave_for = source.id
        _sacrifice(state, source)

    def describe(self) -> str:
        return (
            "sacrifice this permanent (preserving the source-link on "
            "any cards still in exile)"
        )
