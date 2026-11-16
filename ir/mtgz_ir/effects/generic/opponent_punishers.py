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
class EachOpponentSacrificesCreature(Effect):
    """Each opponent sacrifices a nontoken creature of their choice.

    Each sacrificing player picks via their agent's ``choose_sacrifice``.
    If an opponent has no legal nontoken creature, they skip — no target
    required to match the standard MTG "of their choice" wording.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            legal = [
                o for o in state.battlefield
                if o.controller_id == p.id
                and o.printing.is_creature
                and not o.printing.is_token
            ]
            if not legal or p.agent is None:
                continue
            chosen_id = p.agent.choose_sacrifice(
                state, p.id, [o.id for o in legal]
            )
            obj = next((o for o in legal if o.id == chosen_id), legal[0])
            state.log.append(f"P{p.id} sacrifices {obj.printing.name}")
            _sacrifice(state, obj)

    def describe(self) -> str:
        return "each opponent sacrifices a nontoken creature of their choice"



@dataclass(frozen=True)
class TargetOpponentSacrificesCreatureOtherThanEnchanted(Effect):
    """"Target opponent sacrifices a creature of their choice other
    than enchanted creature."

    Used by Sporogenic Infection's ETB. Identifies the source Aura's
    enchanted creature via ``state.resolving_source.attached_to`` and
    excludes it from the target's sac options. The target opponent
    picks via their own ``Agent.choose_sacrifice`` — same agent path
    ``EachOpponentSacrificesCreature`` rides.

    Simplification vs. printed ``target player``: targets are
    restricted to opponents only — the standard EoE / DSK ``target
    player`` simplification used by other Caster-controls-target
    effects in this codebase. Self-targeting is never strategically
    correct for the controller of an enchant-an-opponent's-creature
    Aura; narrowing legality doesn't affect rational play. The
    nontoken filter that ``EachOpponentSacrificesCreature`` ships is
    NOT applied here — the printed text says ``a creature of their
    choice``, not ``a nontoken creature``.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        target = state.player_by_id(target_id)
        if target.has_lost:
            return
        # Excluded id: the Aura's enchanted creature, if any. When the
        # Aura is enchanting a creature controlled by the target (the
        # usual case for Sporogenic Infection), the host is excluded
        # from the sac options. When the Aura is enchanting one of
        # the controller's own creatures, the target's own creatures
        # are unaffected by the exclusion (the host isn't theirs).
        aura = state.resolving_source
        excluded_id = aura.attached_to if aura is not None else None
        legal = [
            o for o in state.battlefield
            if o.controller_id == target_id
            and o.printing.is_creature
            and o.id != excluded_id
        ]
        if not legal or target.agent is None:
            return
        chosen_id = target.agent.choose_sacrifice(
            state, target_id, [o.id for o in legal]
        )
        obj = next((o for o in legal if o.id == chosen_id), legal[0])
        state.log.append(
            f"P{target_id} sacrifices {obj.printing.name} "
            f"(Sporogenic Infection)"
        )
        _sacrifice(state, obj)

    def describe(self) -> str:
        return (
            "target opponent sacrifices a creature of their choice "
            "other than enchanted creature"
        )



@dataclass(frozen=True)
class EachOpponentExilesOwnCreature(Effect):
    """Per-opponent independent-choice primitive: each opponent chooses
    one creature they control and exiles it.

    Per-opponent loop walks each non-controller, non-eliminated player in
    turn order; their agent's ``choose_sacrifice`` hook (re-used as a
    "pick one of your battlefield creatures" prompt — same shape) selects
    the creature to exile. Opponents who control no creature, or whose
    agent is missing, are silently skipped (matching CR 700.2 "if a
    player can't make a choice, they don't").

    When ``link_to_source`` is True, each exiled object is tagged with
    ``exiled_by_source = state.resolving_source.id`` so a sibling ability
    on the same source can later locate "creature card exiled with it"
    (Sothera, the Supervoid's end-step recall pattern). When False the
    exile is a plain zone change with no source-link bookkeeping.

    Generic across the per-opponent-exile shape; Sothera is the only
    consumer in the EoE pool today, but a future card with the same
    "each opponent chooses a creature they control and exiles it" body
    (with or without source-link) reuses this primitive verbatim.
    """

    link_to_source: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        source = state.resolving_source
        # When the caller asked for source linkage but the resolving
        # source is missing, we'd silently drop the link — defensively
        # short-circuit so the failure is visible rather than producing
        # exiled-without-link cards that downstream effects can't find.
        if self.link_to_source and source is None:
            return
        for opp in state.players:
            if opp.id == controller_id or opp.has_lost:
                continue
            candidates = [
                o for o in state.battlefield
                if o.controller_id == opp.id and o.printing.is_creature
            ]
            if not candidates or opp.agent is None:
                continue
            pick_id = opp.agent.choose_sacrifice(
                state, opp.id, [o.id for o in candidates]
            )
            chosen = next(
                (o for o in candidates if o.id == pick_id), candidates[0]
            )
            if self.link_to_source and source is not None:
                chosen.exiled_by_source = source.id
                state.log.append(
                    f"P{opp.id} exiles {chosen.printing.name} "
                    f"({source.printing.name})"
                )
            else:
                state.log.append(
                    f"P{opp.id} exiles {chosen.printing.name}"
                )
            move_off_battlefield(state, chosen, destination="exile")

    def describe(self) -> str:
        return (
            "each opponent chooses a creature they control and exiles it"
        )


@dataclass(frozen=True)
class EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage(Effect):
    """Per-opponent independent-choice primitive: each opponent picks
    among (sacrifice a nonland permanent, discard a card, decline);
    opponents who declined OR had no legal sac/discard option take damage
    equal to the source's effective power.

    The per-opponent loop walks each non-controller, non-eliminated
    player in turn order. The agent's ``choose_mode`` hook is presented
    only with the modes that are legal for that opponent at resolve time
    (sac is offered only when they control a nonland permanent; discard
    only when they have at least one card in hand; decline is always
    offered). Resolution branches:

    - **sac**: ``choose_sacrifice`` picks a victim from the legal nonland
      permanents, ``mtgz_ir.turn.sacrifice`` fires the sac through the
      standard pipeline.
    - **discard**: ``choose_discard`` picks a hand card; the card moves
      to the graveyard and ``fire_opponent_discards_triggers`` fires.
    - **decline**: the opponent is recorded as non-compliant and takes
      damage at the end of the loop.

    A defensive missing-agent counts as decline (same as the bespoke
    behavior). The damage step uses ``_apply_damage`` so the source's
    lifelink keyword (Osseous Sticktwister has lifelink) interacts with
    the damage cleanly.

    Generic across the per-opponent three-mode shape with conditional
    damage to non-compliers; Osseous Sticktwister is the only EoE/DSK
    consumer today. A future card with a different damage formula
    (fixed N, source toughness, etc.) would parameterize this primitive
    via a callable amount-resolver.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, effective_power
        from mtgz_ir.turn import (
            fire_opponent_discards_triggers, sacrifice as _sacrifice,
        )

        source = state.resolving_source
        if source is None:
            return
        non_compliant: list[int] = []
        for opp in state.players:
            if opp.id == controller_id or opp.has_lost:
                continue
            if opp.agent is None:
                # Defensive: a missing agent counts as decline.
                non_compliant.append(opp.id)
                continue
            sac_candidates = [
                o for o in state.battlefield
                if o.controller_id == opp.id
                and not o.printing.is_land
            ]
            modes: list[str] = []
            options: list[str] = []
            if sac_candidates:
                modes.append(
                    "sacrifice a nonland permanent of your choice"
                )
                options.append("sac")
            if opp.hand:
                modes.append("discard a card")
                options.append("discard")
            modes.append("decline (take damage)")
            options.append("decline")
            choice = opp.agent.choose_mode(state, opp.id, modes)
            choice = max(0, min(choice, len(modes) - 1))
            chosen_tag = options[choice]
            if chosen_tag == "sac":
                pick_id = opp.agent.choose_sacrifice(
                    state, opp.id, [o.id for o in sac_candidates],
                )
                chosen = next(
                    (o for o in sac_candidates if o.id == pick_id),
                    sac_candidates[0],
                )
                state.log.append(
                    f"P{opp.id} sacrifices {chosen.printing.name} "
                    f"({source.printing.name})"
                )
                _sacrifice(state, chosen)
            elif chosen_tag == "discard":
                from mtgz_ir.player_protections import (
                    opponent_effect_blocked_for,
                )

                if opponent_effect_blocked_for(state, opp.id):
                    continue
                hand_ids = [c.id for c in opp.hand]
                pick_id = opp.agent.choose_discard(
                    state, opp.id, hand_ids,
                )
                card = next(
                    (c for c in opp.hand if c.id == pick_id), None,
                )
                if card is not None:
                    opp.hand.remove(card)
                    card.zone = Zone.GRAVEYARD
                    opp.graveyard.append(card)
                    state.log.append(
                        f"P{opp.id} discards {card.printing.name} "
                        f"({source.printing.name})"
                    )
                    fire_opponent_discards_triggers(state, opp.id, card=card)
            else:
                non_compliant.append(opp.id)
        if not non_compliant:
            return
        damage = max(0, effective_power(state, source))
        if damage <= 0:
            return
        for opp_id in non_compliant:
            _apply_damage(state, ("player", opp_id), damage)
            state.log.append(
                f"{source.printing.name} deals {damage} to P{opp_id}"
            )

    def describe(self) -> str:
        return (
            "each opponent may sacrifice a nonland permanent or discard "
            "a card; this creature deals damage equal to its power to "
            "each opponent who did neither"
        )
