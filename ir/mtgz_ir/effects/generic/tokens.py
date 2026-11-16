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
from mtgz_ir.effects.generic.delayed_actions import SacrificeSourceDelayed
from mtgz_ir.effects.generic.grant_keywords import GrantKeywords

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class CreateLanderAndScheduleSacrifice(Effect):
    """Create a Lander token for the controller, then schedule a delayed
    trigger to sacrifice that specific Lander at the controller's next
    end step.

    Matches Kav Landseeker's ETB. "Next end step on your next turn" is
    interpreted in a 2-player game as the controller's next end step
    that isn't the current turn — turn_number+2 if it's currently the
    controller's turn, else turn_number+1.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone

        lander = state.new_object(
            TOKEN_PRINTINGS["Lander"],
            owner_id=controller_id,
            zone=Zone.BATTLEFIELD,
        )
        put_into_battlefield(state, lander, controller_id)
        offset = 2 if state.active_player().id == controller_id else 1
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number + offset,
            "controller_id": controller_id,
            "source_id": lander.id,
            "effect": SacrificeSourceDelayed(),
        })
        state.log.append(
            f"P{controller_id} creates a Lander token (scheduled to be "
            f"sacrificed at end step of turn "
            f"{state.turn_number + offset})"
        )

    def describe(self) -> str:
        return (
            "create a Lander token; at the beginning of the end step on "
            "your next turn, sacrifice that token"
        )



@dataclass(frozen=True)
class CreateTokenAndAttachSelf(Effect):
    """Create a token and attach the source (Equipment) to it.

    Matches Auxiliary Boosters' ETB. Reads state.resolving_source for
    the Equipment. No-op if source isn't Equipment.
    """

    token_printing: "CardPrinting"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if source is None:
            return
        if "Equipment" not in source.printing.subtypes:
            return
        token = state.new_object(
            self.token_printing, owner_id=controller_id,
            zone=Zone.BATTLEFIELD,
        )
        state.log.append(
            f"P{controller_id} creates a {self.token_printing.name} token"
        )
        put_into_battlefield(state, token, controller_id)
        source.attached_to = token.id
        state.log.append(
            f"{source.printing.name} is attached to {token.printing.name}"
        )

    def describe(self) -> str:
        return (
            f"create a {self.token_printing.name} token and attach this "
            f"Equipment to it"
        )



@dataclass(frozen=True)
class CreateTokenPerMulticoloredPermanent(Effect):
    """Create a ``token_printing`` token (optionally tapped) for each
    multicolored permanent the controller controls.

    Matches Infinite Guideline Station's ETB.
    """

    token_printing: "CardPrinting"
    tapped: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        n = _multicolored_permanents_count(state, controller_id)
        if n <= 0:
            state.log.append(
                f"P{controller_id} controls 0 multicolored permanents "
                f"— no tokens created"
            )
            return
        for _ in range(n):
            obj = state.new_object(
                self.token_printing, owner_id=controller_id,
                zone=Zone.BATTLEFIELD,
            )
            put_into_battlefield(state, obj, controller_id)
            if self.tapped:
                obj.tapped = True
        state.log.append(
            f"P{controller_id} creates {n} "
            f"{self.token_printing.name} tokens "
            f"(multicolored-permanent count)"
        )

    def describe(self) -> str:
        return (
            f"create a tapped {self.token_printing.name} token for each "
            f"multicolored permanent you control"
        )



@dataclass(frozen=True)
class CreateToken(Effect):
    """Create ``count`` creature tokens of the given printing under the
    ability's controller.

    ``token_printing`` must be a CardPrinting with the Token supertype (built
    via ``cards.make_token_creature``). Tokens enter the battlefield through
    the same helper as spells, so any ETBs they themselves have fire
    correctly.

    Set ``tapped=True`` for effects whose oracle text says "enters tapped"
    (e.g. Gravpack Monoist's "create a tapped 2/2 Robot token").

    Set ``attacking=True`` for effects whose oracle text says "that's tapped
    and attacking" — the token enters declared as an attacker in the current
    combat (summoning sickness cleared). Only meaningful when the trigger
    fires during the declare-attackers step or a combat trigger.

    Set ``schedule_end_of_combat_sacrifice=True`` when the oracle text also
    says "Sacrifice that token at end of combat." — each created token's id
    is appended to ``state.pending_end_of_combat_sacrifices`` immediately
    after ETB, so ``clear_combat_state`` will sacrifice it at end of combat.
    This flag exists to model the paired creation+sacrifice oracle printed on
    Kavaron Harrier as a single engine operation, since the two sentences
    share "that token" anaphora that can't be tracked across a ``Sequence``.
    """

    token_printing: "CardPrinting"
    count: int = 1
    tapped: bool = False
    each_player: bool = False  # if True, every living player creates the token
    attacking: bool = False  # token enters declared as attacker
    schedule_end_of_combat_sacrifice: bool = False  # sac at end of combat

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        import dataclasses as _dc
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        if self.each_player:
            # Every living player creates the token(s) under their own control.
            recipients = [p.id for p in state.players if not p.has_lost]
        else:
            recipients = [controller_id]

        for recipient in recipients:
            # Moonlit Meditation replacement: "The first time you would
            # create one or more tokens each turn, you may instead
            # create that many tokens that are copies of enchanted
            # permanent." Use enchanted-permanent's printing (with
            # Token supertype added) for this batch if applicable.
            printing = self.token_printing
            moonlit = None
            if (
                recipient not in state.players_who_created_tokens_this_turn
            ):
                moonlit = _find_moonlit_replacement_source(state, recipient)
            if moonlit is not None:
                enchanted = next(
                    (
                        o for o in state.battlefield
                        if o.id == moonlit.attached_to
                    ),
                    None,
                )
                if enchanted is not None:
                    base = enchanted.printing
                    printing = _dc.replace(
                        base, supertypes=tuple(
                            list(base.supertypes)
                            + (["Token"] if "Token" not in base.supertypes else [])
                        ),
                    )
                    state.log.append(
                        f"Moonlit Meditation: tokens become copies of "
                        f"{base.name}"
                    )
            # CR 614 token-creation replacement effects (Exalted
            # Sunborn ×2, future Anointed Procession ×3, …) flow
            # through the unified ``dispatch_replacement``. The
            # dispatcher walks the battlefield collecting matching
            # ``StaticAbility.replacement_effect`` declarations,
            # applies them in CR 616.1 order, and returns the post-
            # replacement count. The "under your control" qualifier
            # is enforced by the dispatcher's implicit
            # ``source.controller_id == affected_player_id`` gate.
            from mtgz_ir.effects.modifiers import dispatch_replacement
            from mtgz_ir.effects.predicates import Event as _Event
            replaced = dispatch_replacement(
                state, _Event.WOULD_CREATE_TOKEN,
                affected_player_id=recipient,
                subjects={
                    "controller": state.player_by_id(recipient),
                    "kind": printing.name,
                },
                payload={"count": self.count},
            )
            if replaced.get("prevented"):
                continue
            effective_count = replaced.get("count", 0)
            doubled = effective_count != self.count
            for _ in range(effective_count):
                obj = state.new_object(
                    printing, owner_id=recipient, zone=Zone.BATTLEFIELD,
                )
                state.log.append(
                    f"P{recipient} creates a {printing.name} token"
                    + (" (tapped)" if self.tapped else "")
                    + (" (attacking)" if self.attacking else "")
                    + (
                        f" (replaced x{effective_count}/{self.count})"
                        if doubled else ""
                    )
                )
                put_into_battlefield(state, obj, recipient)
                if self.tapped:
                    obj.tapped = True
                if self.attacking:
                    obj.attacking = True
                    obj.summoning_sick = False
                if self.schedule_end_of_combat_sacrifice:
                    state.pending_end_of_combat_sacrifices.append(obj.id)
            if effective_count > 0:
                state.players_who_created_tokens_this_turn.add(recipient)

    def describe(self) -> str:
        p = self.token_printing
        color_word = ""  # we don't track color on tokens explicitly yet
        pt = f"{p.power}/{p.toughness}" if p.is_creature else ""
        subtype = " ".join(p.subtypes)
        if self.count == 1:
            article = "a"
        else:
            article = str(self.count)
        pieces = [article, pt, color_word, subtype, "creature token"]
        desc = "create " + " ".join(x for x in pieces if x)
        if self.attacking:
            desc += " that's tapped and attacking"
        if self.schedule_end_of_combat_sacrifice:
            desc += "; sacrifice that token at end of combat"
        return desc



@dataclass(frozen=True)
class CreateTokenCopyOfTargetCreatureYouControl(Effect):
    """Mirrorpool mode 2: "Create a token that's a copy of target
    creature you control."

    The token shares the target's printing with the Token supertype
    injected. Uses the existing CreateToken machinery so doublers and
    ETB triggers work normally.

    ``additional_subtype`` (default ``None``) injects a fixed extra
    creature subtype into the token's printing, on top of the
    target's printed subtypes. Matches Mirror Room's "create a token
    that's a copy of target creature you control, except it's a
    Reflection in addition to its other creature types" — the
    Reflection subtype is added to the copy's subtype tuple so
    type-line, oracle and any HasSubtype predicate sees the merged
    list. Idempotent: if the target's printing already lists the
    subtype, the injection is a no-op.
    """

    additional_subtype: "str | None" = None

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_creature_now
        return [
            ("creature", o.id)
            for o in state.battlefield
            if is_creature_now(o) and o.controller_id == controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        import dataclasses as _dc

        if not targets:
            return
        _, tid = targets[0]
        target = next(
            (o for o in state.battlefield if o.id == tid), None,
        )
        if target is None:
            return
        base = target.printing
        # Inject Token supertype if not already present.
        new_supertypes = tuple(
            list(base.supertypes)
            + (["Token"] if "Token" not in base.supertypes else [])
        )
        # Inject the additional creature subtype, if requested and
        # not already present on the target's printing — Mirror Room
        # adds "Reflection" to whatever the target already is.
        if (
            self.additional_subtype is not None
            and self.additional_subtype not in base.subtypes
        ):
            new_subtypes = tuple(
                list(base.subtypes) + [self.additional_subtype]
            )
        else:
            new_subtypes = base.subtypes
        token_printing = _dc.replace(
            base,
            supertypes=new_supertypes,
            subtypes=new_subtypes,
        )
        CreateToken(
            token_printing=token_printing, count=1,
        ).resolve(state, controller_id, ())

    def describe(self) -> str:
        if self.additional_subtype is not None:
            return (
                f"create a token that's a copy of target creature you "
                f"control, except it's a {self.additional_subtype} in "
                f"addition to its other creature types"
            )
        return (
            "create a token that's a copy of target creature you control"
        )



@dataclass(frozen=True)
class CreateXTokensOfTargetPermanentWithHasteEndStepSac(Effect):
    """Devastating Onslaught `{X}{X}{R}`:
    "Create X tokens that are copies of target artifact or creature you
    control. Those tokens gain haste until end of turn. Sacrifice them
    at the beginning of the next end step."

    Reads X from ``state.resolving_x_value``. Each token copies the
    target's printing directly (shared CardPrinting) and enters with a
    ``haste`` temp keyword. A delayed "sacrifice at end of turn"
    trigger is scheduled per token so they're cleaned up automatically.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_artifact_now

        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.controller_id == controller_id
            and (is_artifact_now(state, o) or o.printing.is_creature)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, tid = targets[0]
        target = next((o for o in state.battlefield if o.id == tid), None)
        if target is None:
            return
        x = state.resolving_x_value
        if x <= 0:
            return
        for _ in range(x):
            token = state.new_object(
                target.printing,
                owner_id=controller_id,
                zone=Zone.BATTLEFIELD,
            )
            # Temporarily suppress the "enters with X counters" replacement
            # when putting token copies into play — tokens copy printed
            # characteristics but X is 0 for them (they weren't cast).
            saved_x = state.resolving_x_value
            state.resolving_x_value = 0
            try:
                put_into_battlefield(state, token, controller_id)
            finally:
                state.resolving_x_value = saved_x
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn

            ApplyEffect(
                inner=GrantKeywords(keywords=frozenset({"haste"})),
                ends_at=EndOfTurn(),
            ).resolve(
                state, controller_id, (("creature", token.id),),
            )
            state.delayed_triggers.append({
                "fire_at_turn": state.turn_number,
                "controller_id": controller_id,
                "source_id": token.id,
                "effect": SacrificeSourceDelayed(),
            })
        state.log.append(
            f"Devastating Onslaught creates {x} copies of "
            f"{target.printing.name} with haste (sac at end step)"
        )

    def describe(self) -> str:
        return (
            "create X tokens that are copies of target artifact or "
            "creature you control; they gain haste until end of turn; "
            "sacrifice them at the beginning of the next end step"
        )
