"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import ZoneName, coerce_tuple
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
class OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(Effect):
    """Target artifact or creature's owner puts it on their choice of the
    top or bottom of their library.

    Matches Lost in Space's main effect. The OWNER (not the spell's
    controller) makes the top-vs-bottom decision via their agent's
    ``choose_mode`` hook. Tokens cease to exist when bounced (rule
    704.5d) — they'd briefly land in the library, but a non-trivial
    state-based action removes them. Combat / counter state is cleared
    on the way off the battlefield as in other bounce effects.

    ``nonland_permanent`` widens the legal-target pool from "artifact
    or creature" to "any nonland permanent" — Vanish from Sight's
    printed scope. The same engine class is reused with a flag (rather
    than a sibling class) because every line below the legal-target
    filter is identical: same owner-choice top-vs-bottom prompt, same
    bounce-style cleanup, same token-ceases-to-exist handling.
    """

    nonland_permanent: bool = False

    def requires_target(self) -> bool:
        return True

    def _matches(self, state, obj) -> bool:
        from mtgz_ir.game import is_artifact_now

        printing = obj.printing
        if self.nonland_permanent:
            return not printing.is_land
        return is_artifact_now(state, obj) or printing.is_creature

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if self._matches(state, o)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next((o for o in state.battlefield if o.id == target_id), None)
        if obj is None:
            return
        if not self._matches(state, obj):
            return
        owner = state.player_by_id(obj.owner_id)
        # Owner picks top vs bottom via their agent's choose_mode.
        # Default to top when no agent is seated (e.g., in unit tests).
        if owner.agent is not None:
            choice = owner.agent.choose_mode(
                state, owner.id,
                ["top of library", "bottom of library"],
            )
        else:
            choice = 0
        choice = 0 if choice not in (0, 1) else choice

        # Strip the permanent off the battlefield with the same
        # housekeeping as other bounce-style effects.
        if obj.exile_instead_of_leaving:
            from mtgz_ir.turn import move_off_battlefield

            move_off_battlefield(state, obj, destination="exile")
            return
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
            return
        obj.zone = Zone.LIBRARY
        if choice == 0:
            owner.library.insert(0, obj)
            destination = "top"
        else:
            owner.library.append(obj)
            destination = "bottom"
        state.log.append(
            f"{obj.printing.name} put on the {destination} of "
            f"P{owner.id}'s library"
        )
        from mtgz_ir.turn import note_cards_put_into_library

        note_cards_put_into_library(state, owner.id)

    def describe(self) -> str:
        scope = (
            "nonland permanent"
            if self.nonland_permanent
            else "artifact or creature"
        )
        return (
            f"target {scope}'s owner puts it on their choice of the "
            f"top or bottom of their library"
        )



@dataclass(frozen=True)
class OwnerPutsTargetWarpedExileCardOnBottomOfLibrary(Effect):
    """Put target exiled card with warp on the bottom of its owner's library.

    Matches mode 1 of Blade of the Swarm. Target pool is every card in
    any player's exile zone whose ``warped`` flag is set (the flag is
    raised when a warp-cast creature is exiled at the end step). The
    chosen card is moved to the bottom of its owner's library and its
    warp-related state is cleared (it's now a normal library card).

    Sibling of ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary``,
    but without the top/bottom owner choice (always bottom) and sourcing
    from exile rather than the battlefield. Targets carry kind
    ``"exile_card"`` — distinct from "permanent" because the card isn't
    on the battlefield and the cast pipeline shouldn't try to look it up
    there.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        out: list[Target] = []
        for p in state.players:
            for c in p.exile:
                if c.warped:
                    out.append(("exile_card", c.id))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "exile_card":
            return
        # Locate the target across every player's exile.
        obj = None
        owner = None
        for p in state.players:
            for c in p.exile:
                if c.id == target_id:
                    obj = c
                    owner = p
                    break
            if obj is not None:
                break
        if obj is None or owner is None:
            return
        # Re-check the warped flag at resolution — if something stripped
        # it (or the card moved zones in response), the spell fizzles.
        if not obj.warped:
            return
        owner.exile.remove(obj)
        obj.exiled_face_down = False  # left exile (CR 400.7)
        obj.warped = False
        obj.zone = Zone.LIBRARY
        owner.library.append(obj)
        state.log.append(
            f"{obj.printing.name} put on the bottom of P{owner.id}'s "
            f"library (was warped in exile)"
        )
        from mtgz_ir.turn import note_cards_put_into_library

        note_cards_put_into_library(state, owner.id)

    def describe(self) -> str:
        return (
            "put target exiled card with warp on the bottom of "
            "its owner's library"
        )



@dataclass(frozen=True)
class SacrificeNLandsYouControl(Effect):
    """Sacrifice ``count`` lands the effect's controller controls.

    Matches Lotus Field's "When this land enters, sacrifice two lands."
    Excludes the source (the ETB trigger is on the entering land
    itself, which is already on the battlefield at trigger time;
    oracle allows sac'ing the source in principle, but the engine
    never models that as a desirable choice).

    Simplification: the agent's choose_sacrifice picks; if the agent
    is None we pick the first ``count`` legal lands. If fewer than
    ``count`` legal lands exist, sacrifices all available.
    """

    count: int = 2

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice

        source = state.resolving_source
        source_id = source.id if source is not None else -1
        p = state.player_by_id(controller_id)
        remaining = self.count
        while remaining > 0:
            options = [
                o for o in state.battlefield
                if o.controller_id == controller_id
                and o.printing.is_land
                and o.id != source_id
            ]
            if not options:
                break
            chosen = options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, controller_id, [o.id for o in options]
                )
                chosen = next(
                    (o for o in options if o.id == pick_id), options[0]
                )
            state.log.append(
                f"P{controller_id} sacrifices {chosen.printing.name} "
                f"(Lotus Field ETB)"
            )
            _sacrifice(state, chosen)
            remaining -= 1

    def describe(self) -> str:
        if self.count == 1:
            return "sacrifice a land"
        return f"sacrifice {self.count} lands"



@dataclass(frozen=True)
class SacrificeFirstPermanentYouControl(Effect):
    """Generic "sacrifice a permanent" first-slice deterministic primitive.

    The bridge-shape Python mirror of Rust's
    ``Effect::Sacrifice { target: TargetFilter::AnyCreature }`` (cards.rs:100;
    parse arm at cards.rs:3405-3412, resolve arm at
    state_ops_resolve.rs:1504-1583, to_dict at cards.rs:5620-5623). Rust's
    first-slice resolve is a deterministic shortcut: it picks
    ``state.players[controller].battlefield[0]`` regardless of card-type
    filter (the AnyCreature filter is a label-only narrowness convention
    shared with the round-15 AddCounters / round-18 TapTarget / round-23
    UntapTarget arms; the actual creature filter is not enforced at
    resolve time -- ``bf[0]`` of any permanent type is sacrificed).
    Empty controller bf == silent fizzle; mirrors the bounce / tap /
    untap / destroy fizzle convention.

    This Python primitive mirrors the Rust deterministic-shortcut shape
    EXACTLY -- no agent.choose_target plumbing, no PendingDecision pipe,
    no creature-only filter at resolve time. The bridge constructor
    enforces the AnyCreature filter narrowness at bridge-time (mirrors
    Rust's parse_target_filter coverage of any_player vs any_creature),
    but the resolve body picks ``state.battlefield[0]`` filtered by
    controller_id without any card-type constraint -- bit-identical to
    Rust's first-slice resolve.

    **Distinct from the rich Python sacrifice family.** Python has
    ``SacrificeSelf`` (source-targeting, used by Pinnacle Starcage),
    ``SacrificeNLandsYouControl`` (Lotus Field's land-only shape with
    agent.choose_sacrifice plumbing), ``EachOpponentSacrificesCreature``
    (multi-player iteration), ``TargetOpponentSacrificesCreatureOther
    ThanEnchanted`` (target opponent picks via their agent), and
    ``SacrificeSourceDelayed`` (delayed-trigger schedule). All five
    carry richer semantics than Rust's bare AddCounters-shape
    Sacrifice arm. This primitive is the bridge-only translator
    target -- direct Python card defs that want richer behavior should
    use the existing classes; this class exists to give the cross-
    engine bridge a deterministic Rust-shape Python target that
    doesn't drag PendingDecision / agent.choose_sacrifice / multi-
    target slot machinery into the bridge surface.

    **Controller resolution.** Reads ``controller_id`` (the source-
    controller per CR 119.5 / 608.2 -- effects resolve under the
    SOURCE's controller, not the active player). Mirrors Rust's I66
    follow-up that swapped the hardcoded ``state.active_player`` for
    ``state.current_resolve_source_controller``. The Python resolve
    pipeline already passes the source-controller as ``controller_id``,
    so this primitive uses it directly.

    **No source-exclusion convention.** Rust's resolve picks
    ``bf[0]`` of the controller's battlefield with no source filter.
    Sister to Rust's destroy_target / tap_target / untap_target
    conventions. The source IS allowed to be the picked permanent
    when it sits at index 0 (e.g. an ETB-triggered self-sacrifice).
    Distinct from ``SacrificeNLandsYouControl`` which excludes the
    source via ``source_id != obj.id``.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice

        # Mirror Rust state_ops_resolve.rs:1539-1543 exactly: scan the
        # controller's battlefield, fizzle silently on empty bf, pick
        # bf[0] regardless of card-type filter (the AnyCreature label is
        # bridge-time narrowness, not resolve-time filter).
        bf = [o for o in state.battlefield if o.controller_id == controller_id]
        if not bf:
            return  # silent fizzle (matches Rust state_ops_resolve.rs:1541)
        chosen = bf[0]
        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name}"
        )
        _sacrifice(state, chosen)

    def describe(self) -> str:
        return "sacrifice a permanent you control"



@dataclass(frozen=True)
class PutBasicLandFromHandOntoBattlefieldTapped(Effect):
    """Terrain Generator's {2}, {T} activated: "You may put a basic
    land card from your hand onto the battlefield tapped."
    Simplification: "may" always yes when a basic is in hand.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import fire_landfall_triggers

        p = state.player_by_id(controller_id)
        basic = next(
            (
                c for c in p.hand
                if c.printing.is_land and "Basic" in c.printing.supertypes
            ),
            None,
        )
        if basic is None:
            return
        p.hand.remove(basic)
        basic.zone = Zone.BATTLEFIELD
        basic.controller_id = controller_id
        basic.tapped = True
        basic.entered_battlefield_turn = state.turn_number
        state.battlefield.append(basic)
        state.log.append(
            f"P{controller_id} puts {basic.printing.name} onto the "
            f"battlefield tapped (Terrain Generator)"
        )
        # ``fire_landfall_triggers`` covers both the legacy
        # string-key path and the predicate-style ENTERS dispatch.
        fire_landfall_triggers(state, basic)

    def describe(self) -> str:
        return (
            "you may put a basic land card from your hand onto the "
            "battlefield tapped"
        )



@dataclass(frozen=True)
class ShuffleZonesIntoLibrary(Effect):
    """Shuffle one or more of the controller's owned zones into their
    library, then re-shuffle the library.

    ``zones`` is a tuple of zone-name strings — recognized values are
    ``"hand"`` and ``"graveyard"``. Each card in those zones is moved
    to the library, ``Zone.LIBRARY`` is stamped on the card's zone
    field, and ``shuffle_library`` is called once at the end.

    Used by Weftwalking's ETB ("shuffle your hand and graveyard into
    your library, then draw seven cards") composed with ``DrawCards``
    in a ``Sequence``. Future printings (e.g. shuffle-just-graveyard
    or shuffle-just-hand variants) reuse the same primitive with a
    different ``zones`` tuple.
    """

    zones: tuple[ZoneName, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "zones", coerce_tuple(ZoneName, self.zones))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, shuffle_library

        p = state.player_by_id(controller_id)
        moved = 0
        if "hand" in self.zones:
            for c in list(p.hand):
                c.zone = Zone.LIBRARY
                p.library.append(c)
                moved += 1
            p.hand.clear()
        if "graveyard" in self.zones:
            for c in list(p.graveyard):
                c.zone = Zone.LIBRARY
                p.library.append(c)
                moved += 1
            p.graveyard.clear()
        shuffle_library(state, controller_id)
        if moved:
            from mtgz_ir.turn import note_cards_put_into_library

            note_cards_put_into_library(state, controller_id)
        zone_phrase = " and ".join(self.zones) if self.zones else "(none)"
        state.log.append(
            f"P{controller_id} shuffles {moved} cards from "
            f"{zone_phrase} into their library"
        )

    def describe(self) -> str:
        if not self.zones:
            return "shuffle (no zones) into your library"
        zone_phrase = " and ".join(f"your {z}" for z in self.zones)
        return f"shuffle {zone_phrase} into your library"
