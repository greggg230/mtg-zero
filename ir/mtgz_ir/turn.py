"""Turn structure: steps, turn-based actions, and state-based actions."""

from __future__ import annotations

from enum import Enum

from mtgz_ir.game import (
    GameState, Zone, effective_keywords, effective_toughness, has_card_type,
    is_artifact_now, is_creature_now,
)
from mtgz_ir.keyword_rules import attachment_illegal_by_protection


class Step(Enum):
    UNTAP = "untap"
    UPKEEP = "upkeep"
    DRAW = "draw"
    PRECOMBAT_MAIN = "precombat_main"
    BEGINNING_OF_COMBAT = "beginning_of_combat"
    DECLARE_ATTACKERS = "declare_attackers"
    DECLARE_BLOCKERS = "declare_blockers"
    COMBAT_DAMAGE = "combat_damage"
    END_OF_COMBAT = "end_of_combat"
    POSTCOMBAT_MAIN = "postcombat_main"
    END = "end"
    CLEANUP = "cleanup"


STEP_ORDER: tuple[Step, ...] = (
    Step.UNTAP, Step.UPKEEP, Step.DRAW,
    Step.PRECOMBAT_MAIN,
    Step.BEGINNING_OF_COMBAT, Step.DECLARE_ATTACKERS, Step.DECLARE_BLOCKERS,
    Step.COMBAT_DAMAGE, Step.END_OF_COMBAT,
    Step.POSTCOMBAT_MAIN,
    Step.END, Step.CLEANUP,
)

MAIN_STEPS: frozenset[Step] = frozenset({Step.PRECOMBAT_MAIN, Step.POSTCOMBAT_MAIN})
COMBAT_STEPS: frozenset[Step] = frozenset({
    Step.BEGINNING_OF_COMBAT, Step.DECLARE_ATTACKERS, Step.DECLARE_BLOCKERS,
    Step.COMBAT_DAMAGE, Step.END_OF_COMBAT,
})
# Untap and cleanup don't normally grant priority.
STEPS_WITH_PRIORITY: frozenset[Step] = frozenset(set(STEP_ORDER) - {Step.UNTAP, Step.CLEANUP})


def note_life_losses(state: GameState) -> None:
    """Latch ``Player.lost_life_this_turn`` for every player whose life
    total dropped since the last check (damage, life loss and paid life
    all lower it). A loss fully regained between two checks inside one
    resolution is missed."""
    for p in state.players:
        if p.life < p.life_checkpoint:
            p.lost_life_this_turn = True
        p.life_checkpoint = p.life
    # Speed (CR 702.179; roto P6) reads the life-loss latch.
    from mtgz_ir.effects.generic.roto_keywords import update_speed

    update_speed(state)


def next_step(state: GameState, current: Step) -> "Step | None":
    """The step after ``current`` in this turn (``None`` after cleanup).

    Follows ``STEP_ORDER`` except that a combat phase ending while
    ``state.extra_combat_phases`` is pending starts another combat phase
    ("After this phase, there is an additional combat phase." -- Fear of
    Missing Out; CR 500.8). Shared by ``simulate.run_turn`` and the
    search adapter so both turn loops take the same path."""
    if current == Step.CLEANUP:
        return None
    # Roto P4 turn structure: "Skip your draw step." and "End the turn."
    # First, so an ended turn goes straight to cleanup (CR 723.1).
    from mtgz_ir.roto_turns import turn_structure_next_step

    replaced = turn_structure_next_step(state, current)
    if replaced is not None:
        return replaced
    # Vintage Roto P3: an additional combat phase after the postcombat
    # main phase (Grim Reaper's Sprint).
    from mtgz_ir.effects.generic.roto_mana import roto_next_step

    override = roto_next_step(state, current)
    if override is not None:
        return override
    if current == Step.END_OF_COMBAT and state.extra_combat_phases > 0:
        state.extra_combat_phases -= 1
        state.log.append("an additional combat phase begins")
        return Step.BEGINNING_OF_COMBAT
    return STEP_ORDER[STEP_ORDER.index(current) + 1]


def run_untap_step(state: GameState) -> None:
    ap = state.active_player()
    ap.lands_played_this_turn = 0
    ap.turns_started += 1
    if state.day_night:
        # CR 726.3a: day may become night (or night day) as the turn
        # begins, transforming daybound / nightbound permanents.
        from mtgz_ir.layouts import run_day_night_check

        run_day_night_check(state)
    # A new turn: "lost life this turn" starts over for every player.
    for _p in state.players:
        _p.lost_life_this_turn = False
        _p.life_checkpoint = _p.life
    # "During your turn, <this> is a P/T creature" (Kaito).
    from mtgz_ir.cube_glue_statics import apply_creature_during_your_turn

    apply_creature_during_your_turn(state)
    # "… protection from everything until your next turn" (The One Ring)
    # ends as this player's turn begins.
    if ap.protected_from_everything:
        ap.protected_from_everything = False
        state.log.append(f"P{ap.id}'s protection from everything ends")
    # "Until your next turn, whenever ..." delayed triggers end.
    if state.emblems and any(
        e.expires_at_untap_of == ap.id for e in state.emblems
    ):
        state.emblems = [
            e for e in state.emblems if e.expires_at_untap_of != ap.id
        ]
    # "Until your next turn" expiry: drop every continuous-effect
    # registry entry tagged ``expires_at="controller_untap"`` whose
    # ``expires_at_player_id`` is this active player. Matches Mouth of
    # the Storm's "creatures your opponents control get -3/-0 until
    # your next turn." — the bonus survives the cleanup step of the
    # casting turn AND the opponent's full turn, then clears right at
    # the start of the source-controller's next turn. Both the layer-7
    # ``pump_pt`` P/T contributions and layer-6 ``grant_keyword`` grants
    # under this lifetime are dropped here; the legacy
    # ``extended_temp_*`` mirror fields they previously fed have been
    # retired.
    expired = state.expire_continuous_effects(
        "controller_untap", player_id=ap.id,
    )
    # An "until your next turn" animation (Karn, the Great Creator's +1)
    # ends here: rebuild the layer-4 creature cache of its targets.
    from mtgz_ir.game import sync_creature_type_cache

    sync_creature_type_cache(state, {
        ce.target_id for ce in expired
        if ce.layer == 4 and ce.kind == "add_type"
    })
    # Goad (CR 701.38a) lasts until the goading player's next turn.
    from mtgz_ir.designations import clear_goad_for

    clear_goad_for(state, ap.id)
    # Phasing: "until its controller's next turn." At the start of the
    # phased creature's controller's untap step, phase it (and anything
    # attached to it) back in. Matches Robe of Stars' Astral Projection.
    for obj in state.battlefield:
        if obj.phased_out and obj.controller_id == ap.id:
            obj.phased_out = False
            state.log.append(f"{obj.printing.name} phases in")
    # Dominion Bracelet: if this player's next turn was scheduled for
    # an agent override, install it now (saving the original so
    # cleanup can revert).
    if ap.scheduled_agent_override is not None:
        ap.agent_override_saved = ap.agent
        ap.agent = ap.scheduled_agent_override
        ap.scheduled_agent_override = None
        state.log.append(
            f"P{ap.id}'s turn now controlled by "
            f"P{ap.scheduled_agent_override_by}"
        )
    # Pre-compute ids of permanents whose untap is prevented by an
    # attached aura/equipment with attached_prevents_untap=True
    # (Tractor Beam's "doesn't untap during its controller's untap
    # step").
    no_untap_ids = {
        att.attached_to for att in state.battlefield
        if att.attached_to is not None
        and att.printing.attached_prevents_untap
    }
    # Board-rule untap limits (Back to Basics, Winter Orb; Vintage Roto
    # P5): permanents the rules hold tapped this untap step.
    from mtgz_ir.roto_board_statics import untap_step_held

    held_ids = untap_step_held(state, ap.id, no_untap_ids)
    for obj in state.battlefield:
        if obj.controller_id == ap.id:
            if obj.id in held_ids:
                obj.summoning_sick = False
                state.log.append(
                    f"{obj.printing.name} stays tapped (untap-step limit)"
                )
                continue
            # Stun counters: instead of untapping, remove one counter.
            if obj.stun_counters > 0:
                obj.stun_counters -= 1
                state.log.append(
                    f"{obj.printing.name} stays tapped (stun counter "
                    f"removed; {obj.stun_counters} remaining)"
                )
                continue
            if obj.id in no_untap_ids:
                state.log.append(
                    f"{obj.printing.name} stays tapped (attached-"
                    f"prevents-untap effect)"
                )
                continue
            if obj.exerted:
                # Exert (CR 701.39): skip this one untap step.
                obj.exerted = False
                state.log.append(
                    f"{obj.printing.name} stays tapped (exerted)"
                )
                continue
            if obj.printing.doesnt_untap_during_untap_step and obj.tapped:
                # "This artifact doesn't untap during your untap step."
                # (Grim Monolith, Mana Vault, Time Vault). Summoning
                # sickness still clears — the rider only suppresses the
                # untap turn-based action for this permanent.
                obj.summoning_sick = False
                state.log.append(
                    f"{obj.printing.name} stays tapped (doesn't untap "
                    f"during its controller's untap step)"
                )
                continue
            obj.tapped = False
            obj.summoning_sick = False


DREW_FROM_EMPTY_LIBRARY = "drew from empty library"


def _draw_limited_to_one_per_turn(state: GameState, player_id: int) -> bool:
    """True iff an opponent of ``player_id`` controls a permanent with
    the "each opponent can't draw more than one card each turn"
    restriction (Narset, Parter of Veils)."""
    for o in state.battlefield:
        if (
            o.controller_id != player_id
            and o.printing.opponents_cant_draw_more_than_one_card_each_turn
        ):
            return True
    return False


def _draw_from_empty_library_wins(state: GameState, player_id: int) -> bool:
    """CR 614 replacement: "If you would draw a card while your library
    has no cards in it, you win the game instead." (Jace, Wielder of
    Mysteries). Routed through ``dispatch_replacement`` on
    ``WOULD_DRAW_FROM_EMPTY_LIBRARY``; a ``WinTheGameInstead``
    transform sets ``payload["win_game"]``."""
    from mtgz_ir.effects.modifiers import dispatch_replacement
    from mtgz_ir.effects.predicates import Event

    out = dispatch_replacement(
        state, Event.WOULD_DRAW_FROM_EMPTY_LIBRARY,
        affected_player_id=player_id,
        subjects={"player": state.player_by_id(player_id)},
        payload={},
    )
    return bool(out.get("win_game"))


def win_the_game(state: GameState, player_id: int, reason: str) -> None:
    """``player_id`` wins the game (CR 104.2a): every opponent loses.

    A player whose only "loss" so far is an attempted draw from an
    empty library during the CURRENT resolution hasn't actually lost
    yet -- that loss is a state-based action (CR 704.5b) and no SBA
    check happens mid-resolution. The engine records the empty-library
    loss eagerly in ``draw_card``, so a win produced later in the same
    resolution (Jace, Wielder of Mysteries' "Draw seven cards. Then if
    your library has no cards in it, you win the game.") clears that
    pending flag before declaring the winner. A game that is already
    over is left untouched.
    """
    if state.is_over():
        return
    from mtgz_ir.effects.generic.roto_keywords import player_cant_win

    if player_cant_win(state, player_id):  # Angel's Grace (roto P6)
        state.log.append(f"P{player_id} can't win the game this turn")
        return
    p = state.player_by_id(player_id)
    if p.has_lost:
        if p.lost_reason != DREW_FROM_EMPTY_LIBRARY:
            return
        p.has_lost = False
        p.lost_reason = ""
    for opp in state.players:
        if opp.id != player_id and not opp.has_lost:
            opp.has_lost = True
            opp.lost_reason = f"P{player_id} won the game ({reason})"
    state.winner_id = player_id
    state.log.append(f"P{player_id} wins the game ({reason})")


def draw_card(state: GameState, player_id: int) -> None:
    p = state.player_by_id(player_id)
    # "Each opponent can't draw more than one card each turn." A draw
    # the player can't perform simply doesn't happen (CR 101.2 -- the
    # "can't" wins), so it can't be a draw from an empty library either.
    if p.cards_drawn_this_turn >= 1 and _draw_limited_to_one_per_turn(
        state, player_id,
    ):
        state.log.append(
            f"P{player_id} can't draw more than one card this turn"
        )
        return
    # Roto P2 draw replacements (dredge, Chains of Mephistopheles): they
    # apply to the draw even from an empty library.
    from mtgz_ir.effects.generic.roto_zones import replace_draw

    if replace_draw(state, p):
        return
    if not p.library:
        if _draw_from_empty_library_wins(state, player_id):
            win_the_game(
                state, player_id,
                reason="would draw from an empty library",
            )
            return
        from mtgz_ir.effects.generic.roto_keywords import player_cant_lose

        if not player_cant_lose(state, player_id):  # Angel's Grace (roto P6)
            p.has_lost = True
            p.lost_reason = DREW_FROM_EMPTY_LIBRARY
        return
    # The first card a player draws in their own draw step (CR 504.1's
    # turn-based draw) is exempt from "except the first one they draw
    # in each of their draw steps" riders.
    first_in_draw_step = p.draw_step_first_draw_pending
    p.draw_step_first_draw_pending = False
    p.drawing_first_in_draw_step = first_in_draw_step
    try:
        if _draw_replaced(state, p):
            return
        top = p.library.pop(0)
        top.zone = Zone.HAND
        p.hand.append(top)
        p.cards_drawn_this_turn += 1
        # "Whenever you draw a card" / "whenever an opponent draws a
        # card" observers (CR 121.2: one event per card).
        from mtgz_ir.actions import dispatch_event
        from mtgz_ir.effects.predicates import Event

        dispatch_event(state, Event.DRAWS_CARD, player=p)
        if (
            top.printing.miracle_cost is not None
            and p.cards_drawn_this_turn == 1
        ):
            from mtgz_ir.creature_casting_ub import offer_miracle

            offer_miracle(state, p, top)
    finally:
        p.drawing_first_in_draw_step = False


def _draw_replaced(state: GameState, p) -> bool:
    """Per-card draw replacement (CR 614.1a / 121.6): walk
    ``WOULD_DRAW_A_CARD`` replacements for ``p``. When one replaces the
    draw (Hullbreacher's "instead you create a Treasure token"), run the
    substituted actions and report True so the draw doesn't happen."""
    from mtgz_ir.effects.modifiers import dispatch_replacement
    from mtgz_ir.effects.predicates import Event

    out = dispatch_replacement(
        state, Event.WOULD_DRAW_A_CARD,
        affected_player_id=p.id,
        subjects={"player": p},
        payload={},
    )
    if not out.get("prevented"):
        return False
    for controller_id, effect in out.get("instead", ()):
        effect.resolve(state, controller_id, ())
    return True


def draw_n_cards(state: GameState, player_id: int, n: int) -> int:
    """Resolve a "draw N cards" event for ``player_id`` with replacement
    effects applied. Returns the effective count actually drawn.

    Use this whenever a single MTG event would draw one or more cards
    (the draw step, "draw a card" ETB triggers, "draw N" spells like
    Decode Transmissions). The Quantum-Riddler-style "draw that many
    plus one instead" replacement adjusts the count once per event,
    not once per card drawn — loop callers must use this helper
    rather than calling ``draw_card`` themselves N times.
    """
    from mtgz_ir.game import effective_draw_count

    effective = effective_draw_count(state, player_id, n)
    for _ in range(effective):
        draw_card(state, player_id)
    return effective


def run_draw_step(state: GameState) -> None:
    """The draw turn-based action (CR 504.1), then the "at the
    beginning of your draw step" triggers (CR 504.2) — Mana Vault.

    Both turn loops (``simulate.run_step`` and the search adapter)
    call this only when the active player actually draws, so the
    starting player's skipped first draw step fires no draw-step
    triggers (documented in docs/SEARCH_LIMITATIONS.md).

    The turn-based draw is marked as the first card drawn in this draw
    step ("except the first one they draw in each of their draw
    steps", Orcish Bowmasters / Hullbreacher).
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    ap = state.active_player()
    ap.draw_step_first_draw_pending = True
    try:
        draw_n_cards(state, ap.id, 1)
    finally:
        ap.draw_step_first_draw_pending = False
    dispatch_event(
        state, Event.BEGINNING_OF_DRAW_STEP,
        label="beginning-of-draw-step", player=ap,
    )


def note_cards_put_into_library(state: GameState, player_id: int | None = None) -> None:
    """Fire "whenever one or more cards are put into a library from
    anywhere" observers once for a batch of zone changes into a library
    (``player_id`` = the library's owner, when known). A cheap no-op
    unless some permanent observes the event."""
    from mtgz_ir.effects.predicates import Event

    if not any(
        t.event is Event.CARDS_PUT_INTO_LIBRARY
        for o in state.battlefield for t in o.printing.triggered_abilities
    ):
        return
    from mtgz_ir.actions import dispatch_event

    player = (
        state.player_by_id(player_id) if player_id is not None
        else state.active_player()
    )
    dispatch_event(
        state, Event.CARDS_PUT_INTO_LIBRARY, label="put-into-library",
        player=player,
    )


def _reset_left_battlefield_object(obj: "GameObject") -> None:
    """Clear per-permanent state on an object that left the battlefield
    for a hidden zone (hand / library): counters, temporary P/T bonuses,
    regeneration shield. CR 400.7 — it is a new object there."""
    obj.plus_one_counters = 0
    obj.minus_one_counters = 0
    obj.charge_counters = 0
    obj.stun_counters = 0
    obj.time_counters = 0
    obj.loyalty_counters = 0
    obj.lore_counters = 0
    obj.flying_counters = 0
    obj.named_counters = ()
    obj.temp_power_bonus = 0
    obj.temp_toughness_bonus = 0
    obj.regeneration_shield = False
    obj.impending_active = False
    obj.face_down = False


def move_off_battlefield(
    state: GameState, obj: "GameObject", destination: str = "graveyard",
    library_index: int = 0, *, _hold_departure_lki: bool = False,
) -> None:
    """Remove ``obj`` from the battlefield and place it in ``destination``.

    ``destination`` is ``"graveyard"``, ``"exile"``, ``"hand"`` or
    ``"library"``. Tokens cease to exist per rule 704.5d regardless of
    destination. Combat state (damage, tapped, attacking, blocking) is
    cleared.

    ``"hand"`` (bounce — Upheaval) reveals the card to every player.
    ``"library"`` inserts the card at ``library_index`` of its owner's
    library (0 = top; Oust's "second from the top" = 1; Unexpectedly
    Absent's "just beneath the top X cards" = X; clamped to the library
    size). Both hidden-zone destinations drop counters and temporary
    bonuses — the card becomes a new object (CR 400.7).

    If the object was a creature (or a stationed Spacecraft) and
    ``destination`` is ``"graveyard"``, its "when this creature dies"
    triggered abilities fire after the zone change.

    Note: Indestructible is NOT checked here — the caller (destroy effect
    or SBA) is responsible for that gate. Exile bypasses indestructible
    normally.

    Last-known card types (CR 603.10a): the object's effective card
    types are snapshot before it moves (``game.snapshot_departure_card_
    types``) so the departure triggers see what it was -- a land that was
    an artifact under Mycosynth Lattice dies as an artifact -- and cleared
    once they have been dispatched, together with the layer-4 creature
    cache (every registry entry targeting the object is gone by then).
    ``_hold_departure_lki`` (``sacrifice``) leaves both for the caller to
    clear after its own ``IS_SACRIFICED`` dispatch.
    """
    if obj not in state.battlefield:
        return
    if obj.exile_instead_of_leaving and destination != "exile":
        # From the Catacombs: "If that creature would leave the
        # battlefield, exile it instead of putting it anywhere else."
        destination = "exile"
    if (
        destination == "graveyard"
        and not obj.printing.is_token
        and state.player_by_id(obj.owner_id).graveyard_exile_replacement_this_turn
    ):
        # Yawgmoth's Will: exiled instead (it never dies).
        destination = "exile"
    if destination == "graveyard" and obj.printing.watched_in_graveyard:
        # "If <Name> would be put into a graveyard from anywhere, ...
        # shuffle it into its owner's library instead." (Blightsteel).
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            graveyard_self_replacement_destination,
        )

        if graveyard_self_replacement_destination(state, obj) == "library":
            state.log.append(
                f"{obj.printing.name} is shuffled into its owner's library "
                f"instead"
            )
            move_off_battlefield(
                state, obj, destination="library",
                _hold_departure_lki=_hold_departure_lki,
            )
            from mtgz_ir.game import shuffle_library

            shuffle_library(state, obj.owner_id)
            return
    if destination == "graveyard":
        from mtgz_ir.game import named_counter_count

        if named_counter_count(obj, "finality") > 0:
            # A permanent with a finality counter that would be put into
            # a graveyard from the battlefield is exiled instead.
            destination = "exile"
    if destination == "graveyard":
        # Roto P2 exile-instead replacements (Rest in Peace, Leyline of
        # the Void): exiled instead, so it never dies.
        from mtgz_ir.effects.generic.roto_zones import (
            graveyard_exile_instead_applies,
        )

        if graveyard_exile_instead_applies(state, obj):
            destination = "exile"
    void_redirect = False
    if destination == "graveyard" and not obj.printing.is_token:
        from mtgz_ir.game import void_exile_replacement_active

        if void_exile_replacement_active(state, obj.owner_id):
            # Dauthi Voidwalker: exiled with a void counter (never dies).
            destination = "exile"
            void_redirect = True
    from mtgz_ir.game import snapshot_departure_card_types

    snapshot_departure_card_types(state, obj)
    was_creature = is_creature_now(obj)
    if obj.exile_if_dies_this_turn:
        obj.exile_if_dies_this_turn = False
        if destination == "graveyard" and was_creature:
            # Replacement: "If that creature would die this turn,
            # exile it instead." (Suplex).
            state.log.append(
                f"{obj.printing.name} would die — exiled instead"
            )
            destination = "exile"
    # Persist LKI (CR 702.79a): read before the object leaves.
    persists = (
        destination == "graveyard"
        and was_creature
        and not obj.printing.is_token
        and obj.minus_one_counters == 0
        and "persist" in effective_keywords(state, obj)
    )
    # Mark Void state: any non-land permanent leaving the battlefield this
    # turn (including tokens ceasing to exist) enables Void triggers.
    if not obj.printing.is_land:
        state.permanent_left_battlefield_this_turn = True
    # Revolt: any permanent (lands and tokens included) leaving under
    # its controller's control.
    state.player_by_id(obj.controller_id).permanent_left_battlefield_this_turn = True
    # LKI for "loses all abilities" (Oko's +1): recorded before the
    # object's self-sourced registry entries are dropped below.
    from mtgz_ir.game import _source_layer6_wiped

    obj.left_without_abilities = bool(
        state.continuous_effects and _source_layer6_wiped(state, obj)
    )
    # Layer-2 control-grant reversion: drop every continuous-effect
    # entry sourced by THIS leaving object, then refresh the
    # ``controller_id`` cache on each affected target via
    # ``effective_controller``. When a target's last control_change
    # entry goes away, also clear its baseline-snapshot
    # ``controller_id_before_any_aura_swap`` so a future aura grant
    # captures a fresh baseline. Doing this BEFORE the cascading
    # aura-detach + bury logic keeps any downstream observers
    # reading the post-revert controller.
    from mtgz_ir.game import effective_controller

    # ``only_static=True`` so UEOT grants this object made (e.g.,
    # Perigee Beckoner's granted dies-rider on itself) survive into
    # the dies-trigger fire below; cleanup wipes them at end of
    # turn.
    dropped = state.remove_continuous_effects_from_source(
        obj.id, only_static=True,
    )
    # A static type change this object made (Unable to Scream's
    # "is a ... creature") ends with it: rebuild its targets' creature
    # cache.
    from mtgz_ir.game import sync_creature_type_cache

    sync_creature_type_cache(state, {
        ce.target_id for ce in dropped
        if ce.layer == 4 and ce.kind == "add_type"
    })
    revert_targets: set[int] = {
        ce.target_id for ce in dropped
        if ce.layer == 2
        and ce.kind == "control_change"
        and ce.target_id is not None
    }
    for tid in revert_targets:
        target = next(
            (o for o in state.battlefield if o.id == tid), None
        )
        if target is None:
            continue
        new_controller = effective_controller(state, target)
        # If no more control_change entries remain for this target,
        # clear the captured baseline so a future aura grant starts
        # fresh.
        any_remaining = any(
            ce.layer == 2
            and ce.kind == "control_change"
            and ce.target_id == target.id
            for ce in state.continuous_effects
        )
        if not any_remaining:
            target.controller_id_before_any_aura_swap = None
        if new_controller != target.controller_id:
            target.controller_id = new_controller
            state.log.append(
                f"{target.printing.name} reverts to P{new_controller} "
                f"(no longer enchanted by {obj.printing.name})"
            )
    # Any attachment on this object: Equipment falls off (stays on
    # battlefield, attached_to cleared); Auras are sent to their
    # owners' graveyards.
    auras_to_bury: list = []
    detached_equipment: list = []
    for att in state.battlefield:
        if att.attached_to != obj.id:
            continue
        if att.printing.is_aura or att.aura_enchant_override is not None:
            auras_to_bury.append(att)
        else:
            att.attached_to = None
            att.last_attached_to = obj.id
            detached_equipment.append(att)
    for aura in auras_to_bury:
        state.log.append(
            f"{aura.printing.name} is put into the graveyard "
            f"(enchanted permanent left the battlefield)"
        )
        move_off_battlefield(state, aura, destination="graveyard")
    state.battlefield.remove(obj)
    if obj.prepared_copy_id >= 0:
        # The copy of its prepare spell in exile ceases to exist.
        from mtgz_ir.layouts import stop_being_prepared

        stop_being_prepared(state, obj)
    from mtgz_ir.game import restore_mdfc_front

    restore_mdfc_front(obj)
    obj.exile_instead_of_leaving = False
    # Per-permanent state of the cube creatures bucket (CR 400.7: the
    # card is a new object in its next zone).
    obj.exhausted_abilities = frozenset()
    obj.escaped = False
    obj.dashed = False
    obj.monstrous = False
    obj.entered_via_cast = False
    obj.kick_count = 0
    obj.evoked = False
    obj.bestowed = False
    obj.damage = 0
    obj.marked_lethal = False
    obj.damaged_this_turn = False
    obj.tapped = False
    obj.attacking = False
    obj.blocking = None
    obj.blocked_by = []
    # If the departing permanent itself was Equipment, clear its
    # attachment state.
    obj.attached_to = None
    obj.aura_enchant_override = None
    if obj.printing.is_token:
        state.log.append(f"{obj.printing.name} token ceases to exist")
    elif destination == "graveyard":
        obj.zone = Zone.GRAVEYARD
        state.player_by_id(obj.owner_id).graveyard.append(obj)
    elif destination == "exile" and void_redirect:
        from mtgz_ir.game import exile_with_void_counter

        exile_with_void_counter(state, obj)
    elif destination == "exile":
        obj.zone = Zone.EXILE
        state.player_by_id(obj.owner_id).exile.append(obj)
    elif destination in ("hand", "library"):
        _reset_left_battlefield_object(obj)
        owner = state.player_by_id(obj.owner_id)
        if destination == "hand":
            from mtgz_ir.game import reveal_to_all

            obj.zone = Zone.HAND
            owner.hand.append(obj)
            reveal_to_all(state, obj)
        else:
            obj.zone = Zone.LIBRARY
            idx = max(0, min(library_index, len(owner.library)))
            owner.library.insert(idx, obj)
            note_cards_put_into_library(state, owner.id)
    else:
        raise ValueError(f"unknown destination: {destination!r}")

    # Fire "when this creature dies" triggers if applicable.
    if destination == "graveyard" and was_creature:
        # Bump the per-turn death tracker BEFORE firing dies triggers
        # so an in-trigger ETB-replacement that reads
        # ``state.creatures_died_this_turn`` (via the
        # ``enters_with_plus_one_counter_if_creature_died_this_turn``
        # printing flag, e.g. Cackling Slasher) sees this very death
        # too. The tracker counts EVERY creature death — both players,
        # token or non-token — matching Cackling Slasher's
        # unqualified "if a creature died this turn" surface.
        state.creatures_died_this_turn += 1
        _fire_dies_triggers(state, obj)
        if persists:
            from mtgz_ir.actions import push_persist_trigger

            push_persist_trigger(state, obj)

    # Predicate-style ``Event.DIES`` dispatch. Phase-4 widens the
    # legacy creature-only DIES fire path to also cover artifacts so
    # the migrated ``your_creature_or_artifact_dies`` observer
    # (Susurian Voidborn) keeps firing on artifact deaths the same
    # way the legacy ``_fire_your_creature_or_artifact_dies_triggers``
    # used to. Predicates handle type-filtering downstream — a
    # ``IsCreature() & ...`` predicate won't match an artifact-only
    # death.
    # Lands too (cube W/G bucket: Titania's "Whenever a land you control
    # is put into a graveyard from the battlefield"); every DIES
    # observer's predicate is typed or self-referential.
    if destination == "graveyard" and (
        was_creature or has_card_type(state, obj, "Artifact")
        or obj.printing.is_land
    ):
        from mtgz_ir.actions import dispatch_event as _dispatch_event_dies
        from mtgz_ir.effects.predicates import Event as _Event_dies

        _dispatch_event_dies(
            state,
            _Event_dies.DIES,
            label="dies",
            extra_observers=[obj],
            obj=obj,
        )

    # Earthbend's return rider (Badgermole Cub).
    if obj.earthbent:
        obj.earthbent = False
        if destination in ("graveyard", "exile") and not obj.printing.is_token:
            from mtgz_ir.actions import push_triggered_ability
            from mtgz_ir.effects.base import TriggeredAbility
            from mtgz_ir.effects.generic.cube_creatures_wg import (
                ReturnEarthbentLandTapped,
            )
            from mtgz_ir.effects.predicates import Always, Event

            push_triggered_ability(
                state, source=obj,
                ability=TriggeredAbility(
                    event=Event.DIES, predicate=Always(),
                    effect=ReturnEarthbentLandTapped(),
                ),
                ability_index=-1, controller_id=obj.controller_id,
                label="earthbend return",
            )

    # Serra Paragon's granted rider ("When this permanent is put into a
    # graveyard from the battlefield, exile it and you gain 2 life.").
    if obj.leaves_to_graveyard_exile_gain_life:
        if destination == "graveyard" and not obj.printing.is_token:
            from mtgz_ir.graveyard_casting_wg import (
                fire_leaves_to_graveyard_rider,
            )

            fire_leaves_to_graveyard_rider(state, obj)
        obj.leaves_to_graveyard_exile_gain_life = 0

    # Generic "when this leaves the battlefield" triggers — fires for
    # artifacts, enchantments, and anything else with a matching
    # triggered ability. Used by Cryogen Relic ("enters or leaves →
    # draw a card").
    _fire_leaves_battlefield_triggers(state, obj)
    for att in detached_equipment:
        att.last_attached_to = None

    # Return any cards this object exiled with "until this leaves" to
    # their owner's battlefield. Applies whether the source is destroyed
    # or exiled itself.
    _release_exiled_until_leaves(state, obj.id)

    # When an object leaves the battlefield, every continuous
    # effect that targets it ceases to apply (CR 400.7 — it's a
    # new object once it changes zones). Drop registry entries
    # where ``target_id == obj.id``. Done AFTER dies-trigger
    # firing so granted-trigger entries (Perigee Beckoner's
    # self-granted dies-rider) are still readable when the
    # trigger machinery walks them. Layer-1 copy entries from
    # sorceries (Cytoshape) get cleaned up here since their
    # source spell never goes through ``move_off_battlefield``.
    state.continuous_effects = [
        ce for ce in state.continuous_effects
        if ce.target_id != obj.id
    ]
    state._invalidate_layered_cache()
    # Layer-1 copy revert: if the leaving object was a copy
    # (Clone, Cytoshape, Mirage Mirror), restore the printing
    # from the captured ``original_printing`` snapshot so a
    # future reanimation enters as the printed object.
    from mtgz_ir.game import revert_printing_if_no_layer1_copies
    revert_printing_if_no_layer1_copies(state, obj)
    if not _hold_departure_lki:
        _end_departure(obj)


def _end_departure(obj: "GameObject") -> None:
    """The departure triggers have been dispatched: drop the last-known
    card types and the layer-4 creature cache (it mirrors the registry,
    and every entry targeting the object was dropped above -- an animated
    manland that leaves and comes back the same turn is a new object, CR
    400.7, and isn't a creature)."""
    from mtgz_ir.game import clear_departure_card_types

    clear_departure_card_types(obj)
    obj.permanent_is_creature = False


def fire_counters_added_to_your_creature_triggers(
    state: GameState, controller_id: int, amount: int, obj=None,
) -> None:
    """Dispatch the phase-8 predicate-style ``Event.COUNTERS_ADDED``
    event so observer triggers (Terrasymbiosis's "draw that many
    cards") fire. ``amount`` is the effective count added (post-
    doubling); it threads through ``counter_amount`` to each pushed
    StackItem and resolving effects read it via
    ``state.resolving_counter_amount``.

    The retained name and ``controller_id`` parameter are scaffolding
    from the dual-path migration era — the legacy walk over
    ``counters_added_to_your_creature`` string-key abilities was
    dropped once Terrasymbiosis migrated to the predicate path.
    Calls without ``obj`` are no-ops today (no observer can fire);
    kept for API stability with the single in-tree caller
    (``game.add_plus_one_counters``).
    """
    if obj is None:
        return
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event as _Event

    dispatch_event(
        state, _Event.COUNTERS_ADDED,
        label=f"counters-added (amount {amount})",
        counter_amount=amount,
        obj=obj,
    )


def _fire_leaves_battlefield_triggers(
    state: GameState, obj: "GameObject"
) -> None:
    """Dispatch the ``Event.LEAVES_BATTLEFIELD`` event for ``obj``.

    Phase 4 migrated every static card from the legacy string-key
    ``trigger="leaves_battlefield"`` and
    ``"another_creature_you_control_dies_or_exiled"`` (the latter
    collapses cleanly to LEAVES_BATTLEFIELD because both death and
    exile leave the battlefield). The dispatcher walks battlefield
    observers AND ``extra_observers=[obj]`` so the departing object's
    own self-trigger (``IsSelf()``) is checked even though it's
    already been moved off the battlefield by the caller.
    ``resolving_source`` exposes the dearly departed during resolution.
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    dispatch_event(
        state,
        Event.LEAVES_BATTLEFIELD,
        label="leaves-battlefield",
        extra_observers=[obj],
        obj=obj,
    )


def _release_exiled_until_leaves(state: GameState, source_id: int) -> None:
    """Move any objects from exile back to the battlefield under their
    owner's control if they were exiled by the given source (Oblivion-
    Ring-style). Returns them as fresh ETBs so ETB triggers fire again.

    Honors the one-shot ``state.suppress_exiled_by_source_on_leave_for``
    flag: when it equals ``source_id``, the auto-return is skipped for
    THIS leave event and the flag is cleared. Used by
    ``SacrificeSelfPreservingExileLink`` to defeat the auto-return
    during Sothera, the Supervoid's end-step recall (the recall step
    pulls one tagged card out of exile manually BEFORE the source is
    sacrificed; without suppression, the remaining tagged cards would
    race back to their owners' battlefields, which the printed text
    doesn't say).
    """
    from mtgz_ir.actions import put_into_battlefield

    if state.suppress_exiled_by_source_on_leave_for == source_id:
        # One-shot suppression: clear and skip.
        state.suppress_exiled_by_source_on_leave_for = None
        return
    for p in state.players:
        released = [c for c in p.exile if c.exiled_by_source == source_id]
        for c in released:
            p.exile.remove(c)
            c.exiled_face_down = False  # left exile (CR 400.7)
            c.exiled_by_source = None
            if c.exiled_until_leaves_from_hand:
                # Deep-Cavern Bat: a card exiled from a hand goes back
                # to its owner's hand.
                c.exiled_until_leaves_from_hand = False
                c.zone = Zone.HAND
                p.hand.append(c)
                state.log.append(
                    f"{c.printing.name} returns from exile to P{p.id}'s hand"
                )
                continue
            state.log.append(
                f"{c.printing.name} returns from exile to "
                f"P{p.id}'s battlefield"
            )
            put_into_battlefield(state, c, p.id)


def fire_opponent_discards_triggers(
    state: GameState, discarding_player_id: int, card=None,
) -> None:
    """Dispatch the phase-8 predicate-style ``Event.DISCARDS_CARD``
    event so opponent-discards observer triggers (Entropic
    Battlecruiser, predicate ``Controlled(by=OPPONENT)``) fire.
    Called after any single-card discard event.

    The discarding player flows through as
    ``fixed_targets=(("player", id),)`` so the resolving effect can
    damage them without target selection. The legacy
    ``opponent_discards_a_card`` string-key walk was dropped once
    Entropic Battlecruiser migrated to the predicate path; the
    function name is preserved because ``effects/generic.py`` and
    ``effects/bespoke.py`` per-discard sites import it directly.
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event as _Event

    if card is not None:
        # Mayhem ("if you discarded it this turn").
        card.discarded_turn = state.turn_number
    discarding_player = state.player_by_id(discarding_player_id)
    dispatch_event(
        state, _Event.DISCARDS_CARD, label="discards-card",
        fixed_targets=(("player", discarding_player_id),),
        player=discarding_player,
    )
    # Card-aware twin (Currency Converter's "Whenever you discard a
    # card, you may exile that card from your graveyard"): the discarded
    # card rides along as the fixed target so "that card" can address
    # it. A separate event keeps DISCARDS_CARD's player-target contract
    # (Entropic Battlecruiser) untouched.
    if card is not None:
        dispatch_event(
            state, _Event.CARD_DISCARDED, label="card-discarded",
            fixed_targets=(("card_in_graveyard", card.id),),
            player=discarding_player, card=card,
        )


def should_fire_trigger(
    state: GameState,
    source_id: int,
    ability_index: int,
    ability,
    *,
    skip_once_per_turn: bool = False,
) -> bool:
    """True if ``ability`` on the permanent with id ``source_id`` may fire.

    Honors three independent gates:

    - ``ability.requires_void``: EoE Void rider — only fires while
      ``is_void_active(state)`` (a non-land permanent left the
      battlefield this turn or a spell was warped this turn).
    - ``ability.tier_threshold``: EoE Spacecraft tier gate — source must
      have at least that many charge counters.
    - ``ability.once_per_turn``: records the fire in
      ``state.once_per_turn_fired`` so the trigger fires at most once
      per turn. Eager record matches MTG semantics: the ability has
      "triggered" the moment its event happens, even if it later fails
      to find legal targets and fizzles off the stack.

    ``skip_once_per_turn`` suppresses the once_per_turn ledger check when
    the caller provides its own deduplication (e.g. the aggregated
    counter-creature-combat-damage fire path in ``combat.py`` uses the
    ``aggregated_pairs`` set for deduplication and must not be constrained
    by the per-ability once_per_turn gate).
    """
    from mtgz_ir.game import is_void_active

    if getattr(ability, "requires_void", False) and not is_void_active(state):
        return False
    tier = getattr(ability, "tier_threshold", 0)
    if tier > 0:
        source = next((o for o in state.battlefield if o.id == source_id), None)
        if source is None or source.charge_counters < tier:
            return False
    if skip_once_per_turn or not getattr(ability, "once_per_turn", False):
        return True
    key = (source_id, ability_index)
    if key in state.once_per_turn_fired:
        return False
    state.once_per_turn_fired.add(key)
    return True


def sacrifice(state: GameState, obj: "GameObject") -> None:
    """Sacrifice ``obj``: move it to its owner's graveyard and fire any
    "whenever you sacrifice another creature or artifact" triggers on
    other permanents the sacrificed object's controller controls.

    The fire path also routes "whenever you sacrifice an artifact"
    triggers (Biotech Specialist) when the sacrificed object was an
    artifact.

    Sacrifice bypasses indestructible (MTG rule 701.16a — sacrifice is
    not a "destroy" effect).

    A sacrifice an opponent's resolving spell / ability would cause is
    skipped under Tamiyo, Collector of Tales
    (``mtgz_ir.player_protections``).
    """
    from mtgz_ir.player_protections import opponent_effect_blocked_for

    if opponent_effect_blocked_for(state, obj.controller_id):
        return
    from mtgz_ir.game import snapshot_departure_card_types

    if obj in state.battlefield:
        snapshot_departure_card_types(state, obj)
    was_creature = obj.printing.is_creature
    was_artifact = is_artifact_now(state, obj)
    # Lands too: "Whenever you sacrifice a land" (Baloth Prime; cube W/G
    # bucket). Every IS_SACRIFICED observer's predicate is typed.
    was_land = obj.printing.is_land
    was_on_battlefield = obj in state.battlefield
    move_off_battlefield(
        state, obj, destination="graveyard", _hold_departure_lki=True,
    )
    try:
        if was_creature or was_artifact or was_land:
            _dispatch_is_sacrificed(state, obj)
    finally:
        if was_on_battlefield:
            _end_departure(obj)


def _dispatch_is_sacrificed(state: GameState, obj: "GameObject") -> None:
    # Phase-8 predicate-style dispatch: fire IS_SACRIFICED with the
    # sacrificed permanent as subject. The legacy per-key walks for
    # ``sacrifice_creature_or_artifact`` (Lightless Evangel) and
    # ``sacrifice_artifact`` (Biotech Specialist) were dropped once
    # both migrated to the predicate path — the predicate
    # ``(IsCreature() | IsArtifact()) & Controlled(YOU) & ~IsSelf()``
    # (Lightless) and ``IsArtifact() & Controlled(YOU) & ~IsSelf()``
    # (Biotech) reproduce the same firing scope. The sacrificed
    # object has already been moved to the graveyard so we include it
    # via ``extra_observers`` (no observer in scope today triggers off
    # its own sacrifice, but parity with DIES / LEAVES_BATTLEFIELD
    # keeps the dispatch surface uniform).
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event as _Event

    dispatch_event(
        state, _Event.IS_SACRIFICED, label="sacrifice",
        extra_observers=[obj],
        obj=obj,
    )


def destroy(state: GameState, obj: "GameObject") -> bool:
    """Attempt to destroy ``obj``. Respects indestructible — the source is
    unaffected and True is returned only if the permanent actually moved
    to the graveyard.

    This is the entry point for EFFECT-based "destroy target X" clauses.
    State-based-action lethal damage uses its own indestructible gate in
    state_based_actions; sacrifice ignores indestructible entirely (it
    calls move_off_battlefield directly).
    """
    kws = effective_keywords(state, obj)
    if "indestructible" in kws:
        state.log.append(
            f"{obj.printing.name} is indestructible — destroy fizzles"
        )
        return False
    move_off_battlefield(state, obj, destination="graveyard")
    return True


def tap_permanent(state: GameState, obj: "GameObject") -> None:
    """Tap ``obj`` and push any becomes-tapped triggered abilities onto
    the stack.

    Phase-7 commit 3 migrated every static
    ``trigger="becomes_tapped"`` and
    ``trigger="enchanted_permanent_becomes_tapped"`` site to
    predicate-style ``event=Event.BECOMES_TAPPED`` with either
    ``IsSelf()`` (this creature becomes tapped — the tapped permanent
    itself fires) or ``IsHost()`` (enchanted permanent becomes tapped
    — an attached aura fires; Cryoshatter), so the two legacy walks
    that previously hardcode-matched those string keys had no live
    entries left and were dropped. The function is now a thin
    ``dispatch_event`` forwarder — both ``IsSelf`` and ``IsHost``
    flow through the single dispatcher sweep because the predicates
    key off independent attributes (``subject is source`` for
    ``IsSelf``; ``source.attached_to == subject.id`` for ``IsHost``).

    Already-tapped objects are still a no-op (no re-trigger).
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    if obj.tapped:
        return
    obj.tapped = True
    dispatch_event(
        state, Event.BECOMES_TAPPED,
        label="becomes-tapped", obj=obj,
    )
    # Granted "whenever this creature becomes tapped" (roto P6).
    from mtgz_ir.effects.generic.roto_keywords import (
        fire_granted_becomes_tapped_triggers,
    )

    fire_granted_becomes_tapped_triggers(state, obj)


def fire_landfall_triggers(state: GameState, new_land: "GameObject") -> None:
    """Push landfall triggers onto the stack for permanents the new
    land's controller controls.

    Called from bespoke effects that put a land onto the battlefield
    OUTSIDE of the standard ``_play_land`` / ``_trigger_etb_abilities``
    pipeline (Burgeoning's "put a land card from your hand",
    fetchland-style "search your library for a basic land", and so
    on). The standard ``_play_land`` path drives landfall through
    ``_trigger_etb_abilities`` instead — calling this on top of that
    would double-fire.

    Fires both legacy ``trigger="landfall"`` string-key abilities
    (dead after the phase-2 migration but kept here in step with the
    rest of the ``_fire_*`` family — phase 3 sweeps the legacy
    branch) and predicate-style ``event=Event.ENTERS,
    predicate=IsLand() & Controlled(YOU)`` abilities via
    ``dispatch_event``. The dispatcher itself applies
    ``etb_trigger_multiplier`` for ENTERS, so the legacy loop here
    multiplies its own firings independently.
    """
    from mtgz_ir.actions import dispatch_event, push_triggered_ability
    from mtgz_ir.effects.predicates import Event
    from mtgz_ir.game import etb_trigger_multiplier

    controller = new_land.controller_id
    # Snapshot to avoid mid-iteration surprises if a trigger ETBs something.
    for obj in list(state.battlefield):
        if obj.controller_id != controller:
            continue
        for i, ability in enumerate(obj.printing.triggered_abilities):
            if ability.trigger != "landfall":
                continue
            firings = etb_trigger_multiplier(
                state, controller, ability_source=obj, entering_obj=new_land,
            )
            for _ in range(firings):
                push_triggered_ability(
                    state,
                    source=obj,
                    ability=ability,
                    ability_index=i,
                    controller_id=controller,
                    label="landfall",
                )
    # Dispatch the predicate-style ENTERS event so phase-2-migrated
    # landfall abilities fire from this code path as well.
    dispatch_event(state, Event.ENTERS, label="landfall", obj=new_land)


def fire_opponent_plays_land_triggers(
    state: GameState, land_controller_id: int, land=None,
) -> None:
    """Dispatch the phase-8 predicate-style ``Event.PLAYS_LAND`` event
    so every land-play observer (Burgeoning's
    ``Controlled(by=OPPONENT)`` and The Endstone's land-half
    ``Controlled(YOU)``) fires from a single sweep.

    Called from ``actions.play_land`` only (NOT from effects that
    merely put a land onto the battlefield — "plays a land" is the
    land-drop action per 305.1). The predicate dispatcher walks every
    battlefield permanent; ``Controlled(by=OPPONENT,
    subject="controller")`` evaluates true for sources whose
    controller is NOT the land's controller (so Burgeoning's "you may
    put a land from your hand onto the battlefield" resolves under the
    enchantment's controller).

    ``land`` is the land permanent that was played (the dispatch
    subject). The retained name and the optional ``land=`` keyword
    are scaffolding from the dual-path migration era — direct callers
    in tests pre-date the dispatch infrastructure and pass
    ``land_controller_id`` only; once Burgeoning's tests pivoted to
    pass a synthetic land via the ``_fire_played_land`` helper the
    legacy ``opponent_plays_land`` string-key walk had no live
    entries left and was dropped. Calls without ``land`` are no-ops
    today (no observer can fire); kept for API stability.
    """
    if land is None:
        return
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event as _Event

    land_controller = state.player_by_id(land_controller_id)
    dispatch_event(
        state, _Event.PLAYS_LAND, label="plays-land",
        land=land, controller=land_controller,
    )


def _fire_dies_triggers(state: GameState, dead: "GameObject") -> None:
    """Resolve UEOT-granted dies-rider effects on ``dead``.

    Phase 4 migrated every static card from the legacy string-key
    ``trigger="dies"`` (and the family of
    ``"creature_you_control_with_counter_dies"`` /
    ``"nontoken_creature_you_control_with_counter_dies"`` /
    ``"your_creature_or_artifact_dies"`` keys) to predicate-style
    ``event=Event.DIES`` abilities. Those fire from
    ``move_off_battlefield`` via ``dispatch_event``, so all that
    remains here is the UEOT-granted-rider path.

    The granted rider lives on a layer-6 ``grant_triggered_ability``
    registry entry against the dying object (Perigee Beckoner's pump
    grants exactly this). The rider isn't represented in the
    printed-ability ledger so we resolve it directly rather than
    push it through
    ``push_triggered_ability`` (which would require broadening the
    ledger key type). ``resolving_source`` is set to the dying object
    so the granted effect can read it. Mirrors the dealt-damage path
    in ``combat._fire_enchanted_dealt_damage_triggers``.

    Both legacy ``trigger="dies"`` and predicate-style
    ``event=Event.DIES`` granted shapes are recognized so any future
    granted rider on either shape continues to work.
    """
    from mtgz_ir.effects.predicates import Event
    from mtgz_ir.game import effective_triggered_abilities

    granted_only = [
        ab for ab in effective_triggered_abilities(state, dead)
        if ab not in dead.printing.triggered_abilities
    ]
    for ability in granted_only:
        is_legacy_dies = ability.trigger == "dies"
        is_predicate_dies = ability.matches_event(
            Event.DIES, {"obj": dead}, dead, state,
        )
        if not (is_legacy_dies or is_predicate_dies):
            continue
        state.log.append(
            f"{dead.printing.name}'s granted dies trigger fires"
        )
        state.resolving_source = dead
        try:
            ability.effect.resolve(state, dead.controller_id, ())
        finally:
            state.resolving_source = None


def run_beginning_of_upkeep_triggers(state: GameState) -> None:
    """Push "At the beginning of your upkeep" triggered abilities onto
    the stack for permanents the active player controls.

    Phase-6 commit 2 migrated every static
    ``trigger="beginning_of_upkeep"`` site to predicate-style
    ``event=Event.BEGINNING_OF_UPKEEP, predicate=Controlled(YOU)``,
    so the legacy walk over ``obj.printing.triggered_abilities`` here
    has no live entries left. The function is now a thin
    ``dispatch_event`` forwarder — ``Controlled(YOU)`` against the
    active-player subject filters to permanents the active player
    controls (the legacy ``obj.controller_id != ap_id`` skip), and
    the dispatcher's ``source.controller_id`` push matches the
    legacy ``controller_id=ap_id`` resolution because the predicate
    already gates ``source.controller_id == ap_id``.
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    dispatch_event(
        state, Event.BEGINNING_OF_UPKEEP,
        label="beginning-of-upkeep", player=state.active_player(),
    )
    from mtgz_ir.designations import on_beginning_of_upkeep

    on_beginning_of_upkeep(state)
    # Vintage Roto P3: suspend's "remove a time counter" upkeep trigger.
    from mtgz_ir.effects.generic.roto_mana import push_suspend_upkeep_triggers

    push_suspend_upkeep_triggers(state)
    run_delayed_upkeep_triggers(state)


def run_beginning_of_combat_triggers(state: GameState) -> None:
    """Push "At the beginning of combat on your turn" triggered
    abilities onto the stack for permanents the active player
    controls.

    Phase-6 commit 2 migrated every static
    ``trigger="beginning_of_combat"`` site to predicate-style
    ``event=Event.BEGINNING_OF_COMBAT, predicate=Controlled(YOU)``;
    the legacy walk is gone. See ``run_beginning_of_upkeep_triggers``
    for the controller/predicate equivalence rationale.
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    dispatch_event(
        state, Event.BEGINNING_OF_COMBAT,
        label="beginning-of-combat", player=state.active_player(),
    )


def _find_object_by_id(state: GameState, obj_id: int):
    """Find a GameObject by id across every zone (battlefield + each
    player's library/hand/graveyard/exile). Returns None if no
    GameObject with that id exists anywhere — only possible for
    tokens that ceased to exist (CR 704.5d).
    """
    for o in state.battlefield:
        if o.id == obj_id:
            return o
    for p in state.players:
        for zone in (p.library, p.hand, p.graveyard, p.exile):
            for o in zone:
                if o.id == obj_id:
                    return o
    return None


def run_delayed_end_step_triggers(state: GameState) -> None:
    """Push fires-now delayed triggers onto the stack as real
    triggered abilities.

    Each entry in ``state.delayed_triggers`` whose ``fire_at_turn``
    matches the current turn AND whose ``controller_id`` matches the
    active player gets converted into a synthetic
    ``TriggeredAbility(event=BEGINNING_OF_END_STEP, predicate=Always(),
    effect=<entry-effect>)`` and pushed via ``push_triggered_ability``.
    Players then get priority during the end step's priority loop, and
    the standard ``resolve_top_of_stack`` path drains the stack.

    Push order is the same as the pre-existing iteration order over
    ``state.delayed_triggers`` (FIFO scheduling order). Items are
    pushed onto a LIFO stack, so they resolve in reverse-scheduling
    order — the most recently scheduled delayed trigger resolves
    first. MTG's APNAP rule for simultaneous triggers from a single
    player would order these per-controller-choice, but our engine's
    delayed-trigger collection is single-player anyway (the active
    player), so the LIFO-of-scheduling-order is a deterministic stand-in.

    The ``state.resolving_source`` plumbing happens automatically
    through the standard ``resolve_top_of_stack`` path: when a
    StackItem's ability resolves, ``state.resolving_source`` is set
    to ``item.source`` for the duration of the effect's resolve. The
    delayed-trigger effects (``WarpExileSelf``, ``SacrificeSourceDelayed``,
    ``RemoveChargeCountersFromSourceDelayed``,
    ``ReturnExiledPermanentToOwnerTapped``) all read from there or
    from their own stored ids.

    Entries with a missing source (only possible for tokens that
    ceased to exist before fire time) consume the entry without
    pushing — there's no observable game state difference vs.
    pushing a no-op trigger.

    Legacy string-action entries (``"sacrifice"`` / ``"remove_charge_counters"``)
    are no longer supported here — their producers were migrated to
    the effect-shape path in the same commit that introduced this
    refactor. Any leftover string-action entry is a bug; we raise
    ``ValueError`` rather than silently bypass the stack.
    """
    from mtgz_ir.effects.predicates import Event

    ap_id = state.active_player().id
    remaining: list[dict] = []
    to_push: list[dict] = []
    for entry in state.delayed_triggers:
        # Upkeep-scheduled entries ("at the beginning of the next turn's
        # upkeep") are fired by ``run_delayed_upkeep_triggers``.
        if entry.get("step", "end") != "end":
            remaining.append(entry)
            continue
        # ``any_player_end_step`` entries ("at the beginning of the
        # next end step") fire on whichever player's end step comes
        # first; the rest fire on their controller's end step.
        # ``own_next_end_step`` entries ("at the beginning of your next
        # end step", ``roto_turns.schedule_at_your_next_end_step``) fire
        # at their controller's first end step from ``fire_at_turn`` on,
        # however many extra turns intervene.
        fires_now = (
            (
                entry["fire_at_turn"] == state.turn_number
                or (
                    entry.get("own_next_end_step", False)
                    and entry["fire_at_turn"] <= state.turn_number
                )
            )
            and (
                entry.get("any_player_end_step", False)
                or entry["controller_id"] == ap_id
            )
        )
        if fires_now:
            to_push.append(entry)
        else:
            remaining.append(entry)
    state.delayed_triggers = remaining
    _push_delayed_entries(state, to_push, Event.BEGINNING_OF_END_STEP)


def run_delayed_upkeep_triggers(state: GameState) -> None:
    """Push delayed triggers scheduled for "the beginning of the next
    turn's upkeep" (Mishra's Bauble, Urza's Bauble). An entry carries
    ``"step": "upkeep"`` and ``fire_at_turn`` = the turn after the one
    it was scheduled in; it fires at that turn's upkeep whoever the
    active player is. Called from ``run_beginning_of_upkeep_triggers``.
    """
    from mtgz_ir.effects.predicates import Event

    remaining: list[dict] = []
    to_push: list[dict] = []
    ap_id = state.active_player().id
    for entry in state.delayed_triggers:
        if (
            entry.get("step") == "upkeep"
            and entry["fire_at_turn"] <= state.turn_number
            # ``own_upkeep`` ("your next upkeep" -- Rebound) waits for
            # its controller's upkeep.
            and (
                not entry.get("own_upkeep", False)
                or entry["controller_id"] == ap_id
            )
        ):
            to_push.append(entry)
        else:
            remaining.append(entry)
    if not to_push:
        return
    state.delayed_triggers = remaining
    _push_delayed_entries(state, to_push, Event.BEGINNING_OF_UPKEEP)


def _push_delayed_entries(state: GameState, to_push: list[dict], event) -> None:
    """Convert fired ``state.delayed_triggers`` entries into synthetic
    triggered abilities on the stack (shared by the end-step and upkeep
    runners)."""
    from mtgz_ir.actions import push_triggered_ability
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import Always

    for entry in to_push:
        if "effect" not in entry:
            raise ValueError(
                "delayed_triggers entry without 'effect' key — legacy "
                "string-action entries are no longer supported. Migrate "
                "the producer to push an Effect instance with source_id."
            )
        effect = entry["effect"]
        source_id = entry.get("source_id")
        source = (
            _find_object_by_id(state, source_id)
            if source_id is not None else None
        )
        if source is None:
            # Token ceased to exist (or no source_id was scheduled);
            # the trigger has nothing to act on. Mirrors pre-refactor
            # silent-no-op behavior. Skip the push.
            continue
        ability = TriggeredAbility(
            event=event,
            predicate=Always(),
            effect=effect,
        )
        push_triggered_ability(
            state,
            source=source,
            ability=ability,
            ability_index=-1,  # not a real index — synthetic ability
            controller_id=entry["controller_id"],
            label="delayed",
        )


def run_beginning_of_postcombat_main_triggers(state: GameState) -> None:
    """Push "At the beginning of your second main phase" triggered
    abilities onto the stack for permanents the active player controls.

    Mirrors ``run_beginning_of_upkeep_triggers`` /
    ``run_beginning_of_combat_triggers`` exactly — the
    ``Controlled(YOU)`` predicate against the active-player subject
    filters to permanents the active player controls, and the
    dispatcher's ``source.controller_id`` push matches the legacy
    upkeep/combat resolution rationale (the predicate already gates
    ``source.controller_id == ap_id``).

    Models the DSK Survival ability-word trigger family — every
    printed Survival creature in Duskmourn carries a
    ``TriggeredAbility(event=BEGINNING_OF_POSTCOMBAT_MAIN,
    predicate=IsSelf() & IsTapped(), …)`` shape. Called from
    ``simulate.run_step`` (and its search-engine equivalent) at the
    start of the post-combat main phase, before priority opens.
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    dispatch_event(
        state, Event.BEGINNING_OF_POSTCOMBAT_MAIN,
        label="beginning-of-postcombat-main",
        player=state.active_player(),
    )
    run_delayed_main_phase_triggers(state, Event.BEGINNING_OF_POSTCOMBAT_MAIN)


def run_beginning_of_precombat_main_triggers(state: GameState) -> bool:
    """Push "At the beginning of your first main phase" triggered
    abilities (Coalition Relic) and any delayed "at the beginning of
    your next main phase" triggers of the active player (Mana Drain).
    Called by ``simulate.run_step`` and the search adapter's
    ``_enter_step`` as the precombat main phase begins, before priority
    opens -- the ``run_beginning_of_postcombat_main_triggers`` pattern.

    First the Saga turn-based action (CR 714.3b: "as your precombat main
    phase begins", the active player puts a lore counter on each Saga
    they control), so chapter triggers and first-main-phase triggers are
    put on the stack together."""
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event
    from mtgz_ir.layouts import run_saga_lore_counters

    stack_before = len(state.stack)
    lore_before = sum(o.lore_counters for o in state.battlefield)
    run_saga_lore_counters(state)
    dispatch_event(
        state, Event.BEGINNING_OF_PRECOMBAT_MAIN,
        label="beginning-of-precombat-main",
        player=state.active_player(),
    )
    run_delayed_main_phase_triggers(state, Event.BEGINNING_OF_PRECOMBAT_MAIN)
    # Report whether anything happened, so both turn loops run the
    # follow-up state-based-action check only then.
    return (
        len(state.stack) != stack_before
        or sum(o.lore_counters for o in state.battlefield) != lore_before
    )


def run_delayed_main_phase_triggers(state: GameState, event) -> None:
    """Fire ``state.delayed_triggers`` entries scheduled for "the
    beginning of your next main phase" (``"step": "main"``) whose
    controller is the active player: the next main phase of that
    player that begins after the entry was created -- the same turn's
    second main phase, or their next turn's first one."""
    ap_id = state.active_player().id
    remaining: list[dict] = []
    to_push: list[dict] = []
    for entry in state.delayed_triggers:
        if entry.get("step") == "main" and entry["controller_id"] == ap_id:
            to_push.append(entry)
        else:
            remaining.append(entry)
    if not to_push:
        return
    state.delayed_triggers = remaining
    _push_delayed_entries(state, to_push, event)


def run_impending_decrement_turn_based_action(state: GameState) -> None:
    """Remove one time counter from each impending-active permanent
    the active player controls. When the count hits 0, clear
    ``impending_active`` and the permanent becomes a creature again
    (its printed creature type stops being suppressed by
    ``is_creature_now``).

    Modeled as a turn-based action at the beginning of each end
    step (CR 705-style turn-based bookkeeping, mirroring the printed
    Impending reminder text "At the beginning of your end step,
    remove a time counter from it"). Runs BEFORE
    ``run_beginning_of_end_step_triggers`` so an Overlord whose
    last counter is removed this end step is a creature in time
    for any "at the beginning of your end step" trigger that keys
    off creature presence on the same player's end step.

    Only the active player's permanents tick — the Impending
    reminder is "At the beginning of YOUR end step" — so an
    Overlord's controller has to reach their own end step to
    advance it. The active-player gate matches the printed text.
    """
    ap_id = state.active_player().id
    for obj in state.battlefield:
        if not obj.impending_active:
            continue
        if obj.controller_id != ap_id:
            continue
        obj.time_counters -= 1
        if obj.time_counters <= 0:
            obj.time_counters = 0
            obj.impending_active = False
            state.log.append(
                f"{obj.printing.name} loses its last time counter; "
                f"becomes a creature"
            )
        else:
            state.log.append(
                f"{obj.printing.name} loses a time counter "
                f"({obj.time_counters} remaining)"
            )


def run_beginning_of_end_step_triggers(state: GameState) -> None:
    """Push end-step triggers for the current end step onto the stack.

    Handles two predicate shapes that share ``Event.BEGINNING_OF_END_STEP``:

    - ``predicate=Controlled(YOU)`` — fires only on the active
      player's end step for permanents they control (standard oracle
      "At the beginning of your end step").
    - ``predicate=Always()`` — fires on every player's end step
      regardless of which player controls the source (Ragost's "At
      the beginning of each end step, if you gained life this turn,
      untap Ragost").

    Phase-6 commit 2 migrated every static
    ``trigger="beginning_of_end_step"`` and
    ``trigger="beginning_of_each_end_step"`` site to one of the two
    predicate shapes above; the legacy ``trigger`` walk is gone. The
    dispatcher's ``source.controller_id`` push matches the legacy
    resolver for BOTH variants — for the "your" variant the predicate
    already gates ``source.controller_id == ap_id``, and for the
    each-end-step variant the legacy code already resolved under
    ``source.controller_id`` (not the active player), so collapsing
    onto the dispatcher preserves both behaviors verbatim.
    """
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    dispatch_event(
        state, Event.BEGINNING_OF_END_STEP,
        label="end-step", player=state.active_player(),
    )
    from mtgz_ir.designations import on_beginning_of_end_step

    on_beginning_of_end_step(state)


def run_cleanup_step(state: GameState) -> None:
    ap = state.active_player()
    # CR 402.2: by default the active player discards down to 7 at
    # cleanup. Steaming Sauna's static ``You have no maximum hand
    # size`` (and any future ``grants_no_max_hand_size``-flagged
    # static) suspends that rule for its controller while active.
    # Walk the battlefield for any source under ``ap``'s control
    # whose ``_active_static_abilities`` (room_half-aware) includes
    # a ``grants_no_max_hand_size=True`` entry; skip the discard
    # loop when found.
    from mtgz_ir.game import _active_static_abilities
    no_max_hand_size = any(
        getattr(static, "grants_no_max_hand_size", False)
        # Emblems too ("You have no maximum hand size." emblems).
        for obj in (*state.battlefield, *state.emblems)
        if obj.controller_id == ap.id
        for static in _active_static_abilities(obj)
    )
    if not no_max_hand_size:
        while len(ap.hand) > 7:
            card = ap.hand.pop()
            card.zone = Zone.GRAVEYARD
            ap.graveyard.append(card)
    # Layer-6/7 UEOT continuous effects expire at cleanup. Drop every
    # registry entry tagged ``expires_at="cleanup"`` (Phase 2+
    # migration: keyword grants from pump effects, eventually
    # P/T pumps and granted abilities). Effects still using the
    # bespoke ``temp_*`` fields are cleared in the per-object loop
    # below; both code paths run together until full migration.
    state.expire_continuous_effects("cleanup")
    # UEOT layer-1 copy effects (Cytoshape, Mirage Mirror): when
    # the layer-1 entry expired above, the affected object's
    # ``printing`` mutation needs to revert. Walk the battlefield
    # and revert any object that has ``original_printing`` set
    # but no remaining layer-1 ``is_copy`` entries.
    from mtgz_ir.game import (
        effective_controller, revert_printing_if_no_layer1_copies,
    )
    for obj in state.battlefield:
        revert_printing_if_no_layer1_copies(state, obj)
        obj.saddled = False
    # UEOT layer-2 control_change effects (Mutinous Massacre,
    # Systems Override): when the layer-2 entry expired above,
    # the cached ``obj.controller_id`` is stale. Recompute via
    # ``effective_controller`` and clear
    # ``controller_id_before_any_aura_swap`` for any object whose
    # last control_change entry is now gone.
    for obj in state.battlefield:
        obj.controller_id = effective_controller(state, obj)
        any_remaining = any(
            ce.layer == 2
            and ce.kind == "control_change"
            and ce.target_id == obj.id
            for ce in state.continuous_effects
        )
        if not any_remaining:
            obj.controller_id_before_any_aura_swap = None
    # Layer-4 ``add_type "creature"`` cache rebuild: when an
    # ApplyEffect-rewritten ``BecomeType`` entry expired above, the
    # cached ``permanent_is_creature`` flag is stale. The registry is
    # the source of truth — keep the cache True only while at least
    # one layer-4 ``add_type "creature"`` entry still targets the
    # object. Without this rebuild, a finite-lifetime ``BecomeType
    # (types_to_add=("creature",))`` (manland animation, e.g. Raging
    # Ravine via the new pattern) would leave the permanent stuck as
    # a creature after cleanup.
    from mtgz_ir.game import sync_creature_type_cache

    sync_creature_type_cache(state)
    for obj in state.battlefield:
        obj.damage = 0
        obj.marked_lethal = False
        obj.damaged_this_turn = False
        obj.temp_power_bonus = 0
        obj.temp_toughness_bonus = 0
        # Regenerate shield: unconsumed shields wear off at cleanup.
        obj.regeneration_shield = False
        # "If it would die this turn, exile it instead" ends too.
        obj.exile_if_dies_this_turn = False
        # UEOT "gain control" reverts are handled above by the layer-2
        # ``control_change`` registry path (``expire_continuous_effects
        # ("cleanup")`` + the controller-refresh loop). The legacy
        # ``temp_original_controller_id`` field has been retired.
    # Expire impulse-cast flags on exiled cards — "may play this
    # turn" lasts only until the end of the turn.
    #
    # The loop iterates ``p.exile`` and mutates each ELEMENT in place
    # (no list mutator). Under ``GameState.clone``'s CoW share-group
    # design, ``p.exile`` may share its GameObjects with one or more
    # peer states (parent + sibling rollouts in MCTS). Element-level
    # attribute writes don't trigger ``_fork_if_shared`` — peers
    # would silently see the flag flip too. That's the long-tail
    # corruption that surfaced as ``ValueError: card N not castable
    # by PX`` past a few hundred PIMC self-play games (todo
    # e3b7d9c1): a rollout that walked through cleanup cleared an
    # impulse-castable card's flag on the source state, then the
    # source's heuristic chose to cast that very card and the apply
    # path couldn't find it under any of the legal cast-source
    # predicates. ``fork()`` (a no-op when already owned) gives self
    # an isolated GameObject pool before the in-place mutation runs.
    from mtgz_ir.game import _CowList

    for p in state.players:
        if isinstance(p.exile, _CowList):
            p.exile.fork()
        for c in p.exile:
            c.playable_from_exile_this_turn = False
    # Exile play permissions (``GameObject.exile_play_permission``) whose
    # duration ends with this turn (cube creatures bucket).
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        expire_exile_play_permissions,
    )

    expire_exile_play_permissions(state)
    # Empty each player's mana pool. MTG's real rule empties at end of
    # each step/phase; our simplified engine holds mana across phases
    # within a turn and clears at cleanup.
    for p in state.players:
        if p.mana_pool:
            state.log.append(
                f"P{p.id}'s mana pool empties "
                f"({len(p.mana_pool)} coin(s) lost)"
            )
            p.mana_pool.clear()
    # Reset first-time-tokens-created gate (Moonlit Meditation).
    state.players_who_created_tokens_this_turn.clear()
    # Unused extra combat phases don't carry into the next turn.
    state.extra_combat_phases = 0
    # "Whenever <event> this turn, ..." delayed triggers end with the
    # turn (CreateThisTurnTriggeredAbility, Forth Eorlingas!).
    if state.emblems and any(e.expires_at_cleanup for e in state.emblems):
        state.emblems = [e for e in state.emblems if not e.expires_at_cleanup]
    # Reset turn-scoped Void tracking so next turn starts fresh.
    state.permanent_left_battlefield_this_turn = False
    for _p in state.players:
        _p.permanent_left_battlefield_this_turn = False
    state.spell_warped_this_turn = False
    # Reset the per-turn creature-death tracker (Cackling Slasher's
    # "if a creature died this turn" rider). The tracker is incremented
    # in ``move_off_battlefield`` for every creature dies event the
    # engine fires; clearing it here scopes the rider to a single turn.
    state.creatures_died_this_turn = 0
    # Darkness-style combat-damage prevention expires at end of turn.
    state.prevent_combat_damage_this_turn = False
    # Day / night (CR 726.3a) reads the ending turn's active player's
    # spell count at the next untap step.
    state.previous_turn_active_spells = ap.spells_cast_this_turn
    # Reset per-player spell counters for "your Nth spell" triggers.
    for p in state.players:
        p.spells_cast_this_turn = 0
        p.noncreature_spells_cast_this_turn = 0
        p.cards_drawn_this_turn = 0
        p.may_play_from_graveyard_this_turn = False
        p.graveyard_exile_replacement_this_turn = False
        if any(
            c.flashback_granted_until_eot or c.castable_from_graveyard_this_turn
            for c in p.graveyard
        ):
            from mtgz_ir.game import _CowList

            if isinstance(p.graveyard, _CowList):
                p.graveyard.fork()
            for c in p.graveyard:
                c.flashback_granted_until_eot = False
                c.castable_from_graveyard_this_turn = False
                # Jace, Telepath Unbound's rider ends with the permission.
                c.exile_if_put_into_graveyard_from_stack = False
        p.attacked_this_turn = []
        p.life_gained_this_turn = 0
        p.lands_entered_this_turn = 0
        p.artifacts_entered_under_your_control_this_turn = 0
        # Revert Dominion Bracelet agent override (applied at untap).
        if p.agent_override_saved is not None:
            state.log.append(
                f"P{p.id}'s agent override expires"
            )
            p.agent = p.agent_override_saved
            p.agent_override_saved = None
            p.scheduled_agent_override_by = None
            # Roto P4: "After that turn, that player takes an extra
            # turn." (Emrakul, the Promised End).
            from mtgz_ir.effects.generic.roto_walkers import (
                extra_turn_after_controlled_turn,
            )

            extra_turn_after_controlled_turn(state, p)
    # Reset once-per-turn trigger tracking.
    state.once_per_turn_fired.clear()
    # Reset per-turn loyalty-ability activation tracking (CR 606.3).
    state.loyalty_activated_this_turn.clear()
    # Reset per-turn ability-resolution counters.
    state.ability_resolutions_this_turn.clear()
    # "At the beginning of the next cleanup step" delayed triggers
    # (Necromancy's instant-speed rider).
    if state.delayed_triggers:
        from mtgz_ir.effects.generic.enchantment_mechanics import (
            run_delayed_cleanup_triggers,
        )
        run_delayed_cleanup_triggers(state)


def state_based_actions(state: GameState) -> bool:
    """Apply state-based actions until quiescent (CR 704.3 — SBAs are
    checked continuously and applied repeatedly until none apply).
    Returns True if any pass made changes.

    Internally calls ``_state_based_actions_pass`` in a loop. The
    iteration cap (``_SBA_MAX_PASSES``) defends against pathological
    cycles — none should occur with the rules we model, but the
    cap converts a runaway into an early bailout instead of a hang.
    """
    any_changed = False
    note_life_losses(state)
    for _ in range(_SBA_MAX_PASSES):
        if not _state_based_actions_pass(state):
            break
        any_changed = True
    if apply_graveyard_exile_replacement(state):
        any_changed = True
    # "... would be put into a graveyard from anywhere" permanents that
    # reached a graveyard by discard / mill (cube creatures bucket).
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        sweep_graveyard_watch,
        sweep_library_graveyard_exile_watch,
    )

    # Roto P2 exile-instead catch-all: before any graveyard watcher, so
    # an exiled-instead card never looks like a graveyard arrival.
    from mtgz_ir.effects.generic.roto_zones import run_zone_sweeps

    if run_zone_sweeps(state):
        any_changed = True
    if sweep_graveyard_watch(state):
        any_changed = True
    if sweep_library_graveyard_exile_watch(state):
        any_changed = True
    if apply_void_exile_replacement(state):
        any_changed = True
    # Ascend (CR 702.131; ``mtgz_ir.creature_statics_wg``).
    from mtgz_ir.creature_statics_wg import apply_ascend

    apply_ascend(state)
    # Role uniqueness (CR 704.5y) and state triggers (CR 603.8) --
    # Vintage Roto P5 (``mtgz_ir.roto_board_statics``).
    from mtgz_ir.roto_board_statics import apply_board_state_checks

    if apply_board_state_checks(state):
        any_changed = True
    return any_changed


def apply_void_exile_replacement(state: GameState) -> bool:
    """Catch-all for Dauthi Voidwalker's replacement: cards that reached
    an affected player's graveyard through a path that doesn't consult
    it directly (discard / mill / ...) are exiled with a void counter.
    Permanents leaving the battlefield and spells leaving the stack are
    redirected before they land (so they never "die"). Cards already in
    the graveyard when the replacement began applying
    (``Player.void_graveyard_snapshot``, refreshed when a void source
    enters) stay put."""
    if not any(o.printing.void_exile_replacement for o in state.battlefield):
        return False
    from mtgz_ir.game import exile_with_void_counter, void_exile_replacement_active

    changed = False
    for p in state.players:
        if not void_exile_replacement_active(state, p.id):
            continue
        movers = [c for c in p.graveyard if c.id not in p.void_graveyard_snapshot]
        for c in movers:
            p.graveyard.remove(c)
            exile_with_void_counter(state, c)
            changed = True
    return changed


def apply_graveyard_exile_replacement(state: GameState) -> bool:
    """Catch-all for the Yawgmoth's Will replacement: every card that
    reached an affected player's graveyard through a path that doesn't
    consult the replacement directly (discard / mill / ...) is moved to
    exile. The main paths — permanents leaving the battlefield
    (``move_off_battlefield``) and spells leaving the stack
    (``actions._send_to_graveyard_or_library``) — redirect before the
    card ever lands, so "dies" triggers don't fire for them. Cards that
    were already in the graveyard when the replacement started were
    recorded in ``Player.graveyard_exile_snapshot`` and stay put.
    """
    changed = False
    for p in state.players:
        if not p.graveyard_exile_replacement_this_turn:
            continue
        keep = p.graveyard_exile_snapshot
        movers = [c for c in p.graveyard if c.id not in keep]
        for c in movers:
            p.graveyard.remove(c)
            c.zone = Zone.EXILE
            p.exile.append(c)
            state.log.append(
                f"{c.printing.name} is exiled instead of staying in "
                f"P{p.id}'s graveyard"
            )
            changed = True
    return changed


_SBA_MAX_PASSES = 50


def _state_based_actions_pass(state: GameState) -> bool:
    """Single SBA application pass. Returns True if any changes were
    made. Public callers should use ``state_based_actions``, which
    loops this until quiescent.

    CR 704.5 SBAs implemented (audit numbering):
    - 704.5a: player at 0 or less life loses.
    - 704.5b: drawing from empty library (handled in
      ``draw_card``, not here).
    - 704.5c: 10+ poison counters loses.
    - 704.5d: tokens cease to exist (in
      ``move_off_battlefield``).
    - 704.5f: creature with toughness ≤ 0 → graveyard.
    - 704.5g: creature with damage ≥ toughness → destroyed.
    - 704.5h: marked-lethal → destroyed.
    - 704.5i: planeswalker with 0 loyalty → graveyard.
    - 704.5j: legend rule (per-controller, per-name).
    - 704.5n (audit) / 704.5r (current CR): +1/+1 vs -1/-1
      counter cancellation.
    - 704.5n (audit) / 704.5q (current CR): aura on illegal
      target → graveyard.
    - 704.5p (audit): equipment on illegal target → unattach.
    - 704.5r (audit): saga at chapter count → graveyard.
    - 701.15: regeneration shield (handled in the destroyed
      pass — not strictly an SBA).

    - 704.5k (audit) / 704.5o (current CR): world rule
      (``roto_restrictions.apply_world_rule``).

    KNOWN GAPS:
    - 704.5s (audit) / 704.5u (current CR): battle with 0
      defense → graveyard. Not implemented. No Battles in EoE.
    - 21+ commander damage from one source: not implemented.
      Engine doesn't track commander damage by source.
    - Token in non-battlefield zone other than via leave-play:
      not actively scrubbed. Cards moved to a hand/library
      from a non-battlefield zone are not currently a token-
      bearing path.
    """
    changed = False

    from mtgz_ir.effects.generic.roto_keywords import player_cant_lose

    for p in state.players:
        if not p.has_lost and p.life <= 0 and not player_cant_lose(state, p.id):
            p.has_lost = True
            p.lost_reason = "life total <= 0"
            changed = True
        if (
            not p.has_lost and p.poison_counters >= 10
            and not player_cant_lose(state, p.id)
        ):
            p.has_lost = True
            p.lost_reason = "10+ poison counters"
            changed = True

    # Planeswalker SBA (rule 704.5i): a planeswalker with 0 loyalty
    # counters is put into its owner's graveyard.
    planeswalkers_to_bury: list = []
    for obj in state.battlefield:
        if obj.printing.is_planeswalker and obj.loyalty_counters <= 0:
            planeswalkers_to_bury.append(obj)
    for pw in planeswalkers_to_bury:
        state.log.append(
            f"{pw.printing.name} has 0 loyalty and is put into "
            f"graveyard"
        )
        move_off_battlefield(state, pw, destination="graveyard")
        changed = True

    # Legend rule (CR 704.5j): if a player controls two or more
    # legendary permanents with the same name, that player chooses
    # one of them, and the rest are put into their owners'
    # graveyards. Group by (controller_id, printed_name) over
    # legendary permanents only.
    legend_groups: dict[tuple[int, str], list] = {}
    for obj in state.battlefield:
        if not obj.printing.is_legendary:
            continue
        key = (obj.controller_id, obj.printing.name)
        legend_groups.setdefault(key, []).append(obj)
    for (ctrl_id, _name), group in legend_groups.items():
        if len(group) < 2:
            continue
        controller = state.player_by_id(ctrl_id)
        if controller.agent is not None:
            keep_id = controller.agent.choose_legend_keep(
                state, ctrl_id, [o.id for o in group],
            )
        else:
            keep_id = group[0].id
        for obj in group:
            if obj.id == keep_id:
                continue
            state.log.append(
                f"{obj.printing.name} put into graveyard "
                f"(legend rule; P{ctrl_id} kept #{keep_id})"
            )
            move_off_battlefield(state, obj, destination="graveyard")
            changed = True
    # World rule (CR 704.5k; roto P1 -- Nether Void).
    from mtgz_ir.roto_restrictions import apply_world_rule

    if apply_world_rule(state):
        changed = True

    destroyed = []
    for obj in state.battlefield:
        if not is_creature_now(obj):
            continue
        tough = effective_toughness(state, obj)
        kws = effective_keywords(state, obj)
        # Indestructible: lethal damage and "destroy" effects don't kill it,
        # but toughness <= 0 still does (state-based action, rule 704.5f).
        if tough <= 0:
            destroyed.append(obj)
        elif "indestructible" in kws:
            continue
        elif obj.damage >= tough or obj.marked_lethal:
            destroyed.append(obj)

    for obj in destroyed:
        # Regenerate (Swarmyard et al.): if the creature has a
        # regeneration shield, the destruction is replaced — remove
        # all damage, tap, remove from combat, consume the shield.
        # Rule 701.15: toughness-based destruction (tough <= 0) is
        # NOT regeneratable; only lethal-damage / marked_lethal /
        # "destroy" paths are. Our SBA path here lumps those together;
        # we gate on not-zero-toughness to match the rule.
        tough = effective_toughness(state, obj)
        if obj.regeneration_shield and tough > 0:
            obj.damage = 0
            obj.marked_lethal = False
            obj.tapped = True
            obj.attacking = False
            obj.blocking = None
            obj.blocked_by = []
            obj.regeneration_shield = False
            state.log.append(
                f"{obj.printing.name} regenerates "
                "(tapped, damage cleared)"
            )
            changed = True
            continue
        # Log before moving (move_off_battlefield logs for tokens too).
        if not obj.printing.is_token:
            state.log.append(f"{obj.printing.name} destroyed (lethal damage)")
        move_off_battlefield(state, obj, destination="graveyard")
        changed = True

    # Saga age (CR 704.5r in audit numbering): if a saga has lore
    # counters greater than or equal to its final chapter number,
    # its controller sacrifices it (goes to owner's graveyard).
    # CR 714.4: not while one of its chapter abilities has triggered and
    # hasn't left the stack yet.
    from mtgz_ir.layouts import chapter_ability_on_stack

    sagas_to_bury: list = []
    for obj in state.battlefield:
        if (
            obj.printing.is_saga
            and obj.lore_counters >= obj.printing.chapter_count
            and not chapter_ability_on_stack(state, obj)
        ):
            sagas_to_bury.append(obj)
    for saga in sagas_to_bury:
        state.log.append(
            f"{saga.printing.name} sacrificed "
            f"(lore counter ≥ chapter count {saga.printing.chapter_count})"
        )
        move_off_battlefield(state, saga, destination="graveyard")
        changed = True

    # +1/+1 vs -1/-1 counter cancellation (CR 704.5r in current CR;
    # labelled 704.5n in this codebase's audit). When a permanent
    # has both kinds, N pairs are removed where N is the smaller
    # count. Cancellation can drive toughness back above 0 / below
    # 0 → run BEFORE the destroyed-creature pass above? No, after,
    # because cancellation is a separate SBA pass. The
    # state_based_actions LOOP handles cascading: after this
    # cancellation, the next pass re-evaluates 704.5f/g.
    for obj in state.battlefield:
        if obj.plus_one_counters > 0 and obj.minus_one_counters > 0:
            n = min(obj.plus_one_counters, obj.minus_one_counters)
            obj.plus_one_counters -= n
            obj.minus_one_counters -= n
            state.log.append(
                f"{obj.printing.name}: {n} +1/+1 and {n} -1/-1 "
                f"counters cancel"
            )
            changed = True

    # Aura attachment legality (CR 704.5q in current CR; this
    # codebase historically labelled it 704.5n): an Aura attached to
    # an
    # illegal target (or unattached) goes to its owner's graveyard.
    # The target is illegal if it's no longer on the battlefield, or
    # no longer satisfies the Aura's printed enchant restriction
    # (e.g., enchant creature with the target turned non-creature
    # by a Lignified manland's animation expiring).
    auras_to_bury: list = []
    for obj in state.battlefield:
        kind = obj.aura_enchant_override
        if kind is None:
            if not obj.printing.is_aura:
                continue
            kind = obj.printing.aura_enchant_type
        if obj.attached_to is None:
            auras_to_bury.append(obj)
            continue
        if kind == "creature_card_in_graveyard":
            # Animate Dead before its enters trigger resolves: it
            # enchants a creature card in a graveyard.
            from mtgz_ir.effects.generic.enchantment_mechanics import (
                enchanted_graveyard_card,
            )
            if enchanted_graveyard_card(state, obj) is None:
                auras_to_bury.append(obj)
            continue
        target = next(
            (o for o in state.battlefield if o.id == obj.attached_to),
            None,
        )
        if target is None:
            auras_to_bury.append(obj)
            continue
        if kind == "creature":
            if not is_creature_now(target):
                auras_to_bury.append(obj)
        elif kind == "artifact":
            if not is_artifact_now(state, target):
                auras_to_bury.append(obj)
        elif kind == "land":
            from mtgz_ir.effects.generic.roto_mana import enchantable_land

            if not enchantable_land(state, obj.printing, target):
                auras_to_bury.append(obj)
        # Other kinds (e.g., player-enchant) aren't in scope.
        # Protection (CR 702.16c): a permanent with protection from a
        # quality the Aura has can't be enchanted by it.
        if obj not in auras_to_bury and attachment_illegal_by_protection(
            state, obj, target,
        ):
            auras_to_bury.append(obj)
    for aura in auras_to_bury:
        state.log.append(
            f"{aura.printing.name} put into graveyard "
            f"(no legal enchant target)"
        )
        move_off_battlefield(state, aura, destination="graveyard")
        changed = True

    # Equipment attachment legality (CR 704.5p): an Equipment
    # attached to an illegal permanent (non-creature, gone) becomes
    # unattached but stays on the battlefield. Default scope:
    # Equipment must be attached to a creature.
    for obj in state.battlefield:
        if obj.printing.is_aura or obj.aura_enchant_override is not None:
            continue
        if "Equipment" not in obj.printing.subtypes:
            continue
        if obj.attached_to is None:
            continue
        target = next(
            (o for o in state.battlefield if o.id == obj.attached_to),
            None,
        )
        if target is None or not is_creature_now(target):
            obj.attached_to = None
            state.log.append(
                f"{obj.printing.name} unattaches "
                f"(no longer attached to a creature)"
            )
            changed = True
        elif attachment_illegal_by_protection(state, obj, target):
            # CR 702.16d: protection from a quality the Equipment has.
            obj.attached_to = None
            state.log.append(
                f"{obj.printing.name} unattaches "
                f"({target.printing.name} has protection from it)"
            )
            changed = True

    alive = [p for p in state.players if not p.has_lost]
    if len(alive) == 0 and not state.is_draw:
        state.is_draw = True
        changed = True
    elif (
        len(alive) == 1
        and state.winner_id is None
        and len(state.players) > 1
    ):
        state.winner_id = alive[0].id
        changed = True

    return changed
