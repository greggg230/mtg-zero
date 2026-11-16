"""Effect combinators: Sequence, ModalChooseOne, conditional wrappers,
optional-sacrifice wrappers. Compose other Effects into bigger ones.
"""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import CardTypeWord
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.durations import Duration, Permanent
from mtgz_ir.effects.generic import SearchLibraryForBasicLandTapped

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class ThenTargetControllerResolves(Effect):
    """Resolve ``first`` against the chosen target, then resolve
    ``then`` with THAT TARGET'S CONTROLLER (captured before ``first``
    changes anything) as the resolving controller.

    Models the anaphoric "That player ..." follow-up sentence: Boseiju,
    Who Endures' "Destroy target artifact, enchantment, or nonbasic land
    an opponent controls. That player may search their library for a
    land card with a basic land type, put it onto the battlefield, then
    shuffle." Targeting (legality, count, kinds) delegates to
    ``first``. If the target is no longer legal on resolution the whole
    ability fizzles (CR 608.2b) and ``then`` doesn't happen.
    """

    first: Effect
    then: Effect

    def __post_init__(self) -> None:
        object.__setattr__(self, "targets", self.first.targets)

    def requires_target(self) -> bool:
        return self.first.requires_target()

    def legal_targets(self, state, controller_id):
        return self.first.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.first.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.effects.targets import TargetSpec

        if not targets:
            return
        slot = next(
            (t for t in self.first.targets if isinstance(t, TargetSpec)),
            None,
        )
        _, target_id = targets[0]
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None,
        )
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            legal = (
                obj is not None
                and slot is not None
                and _slot_target_still_legal(state, slot, targets[0])
            )
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append("ability fizzles (target no longer legal)")
            return
        that_player = obj.controller_id
        self.first.resolve(state, controller_id, targets)
        self.then.resolve(state, that_player)

    def describe(self) -> str:
        return (
            f"{self.first.describe()}; that player: {self.then.describe()}"
        )


@dataclass(frozen=True)
class ApplyEffect(Effect):
    """Wrap ``inner`` and rewrite the lifetime of every ``ContinuousEffect``
    record it appends to ``state.continuous_effects``.

    Separates WHAT-changes (the inner effect's registry writes) from
    WHEN-it-ends (the ``ends_at`` ``Duration``). The inner effect resolves
    normally — typically writing one or more registry entries with whatever
    ``expires_at`` it always uses — and ``ApplyEffect`` rewrites each
    newly-appended entry's ``expires_at`` / ``expires_at_player_id`` to the
    pair declared by ``ends_at``.

    Implementation: snapshot ``len(state.continuous_effects)`` before
    ``inner.resolve``, then walk indices ``[before, after)`` after it
    returns and replace each entry with a ``dataclasses.replace`` that
    overrides only the lifetime fields. The registry's ``ContinuousEffect``
    is ``frozen=True``; in-place reassignment of the list slot keeps the
    timestamp/source/layer/etc. fields intact.

    Example: ``ApplyEffect(BecomeType(types_to_add=("creature",)),
    ends_at=EndOfTurn())`` writes a layer-4 ``add_type "creature"``
    entry that expires at the next cleanup step.

    Read-path notes:

    - PT and subtype reads (``_compute_effective_pt``,
      ``effective_subtypes``) consult the registry directly, so any
      duration is faithfully respected for those changes.
    - ``is_creature_now`` reads the
      ``GameObject.permanent_is_creature`` write-through cache.
      ``BecomeType`` sets that cache on resolve; ``run_cleanup_step``
      rebuilds it from the registry after expiring cleanup-scoped
      entries, so an ApplyEffect-wrapped ``BecomeType`` with a finite
      duration correctly reverts the cache when its registry entry
      expires. This makes the new pattern viable for manland-style
      animation (``Raging Ravine`` etc.); the existing
      ``BecomeLandCreatureUEOT`` primitive still owns the
      legacy ``temp_*`` path until the manland migration sweep lands.
    """

    inner: Effect
    ends_at: Duration = Permanent()

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.inner.legal_targets(state, controller_id)

    def target_count_range(self) -> tuple[int, int]:
        return self.inner.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ):
        before = len(state.continuous_effects)
        result = self.inner.resolve(state, controller_id, targets)
        expires_at, player_id = self.ends_at.to_expires(controller_id)
        for i in range(before, len(state.continuous_effects)):
            entry = state.continuous_effects[i]
            state.continuous_effects[i] = dataclasses.replace(
                entry,
                expires_at=expires_at,
                expires_at_player_id=player_id,
            )
        return result

    def describe(self) -> str:
        return f"{self.inner.describe()} {self.ends_at.describe()}"


@dataclass(frozen=True)
class SacrificeSelfUnlessTapPermanent(Effect):
    """ETB rider: "sacrifice this unless you tap an untapped permanent
    you control." Reads ``state.resolving_source`` for the source
    permanent (typically a land like Command Bridge).

    Resolution:
      - Build the set of legal "alternative cost" picks: untapped
        permanents the controller controls, EXCLUDING the source itself.
      - If the set is empty, the source is sacrificed (no choice).
      - Otherwise the controller's agent picks one via
        ``choose_target`` with kind ``"untapped_permanent"``. Returning
        None (or no agent) declines and the source is sacrificed; any
        valid pick taps that permanent and the source stays.

    Note: the source might already be tapped (Command Bridge enters
    tapped before its ETB resolves) — the source is excluded from the
    pickable set regardless.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice, tap_permanent

        source = state.resolving_source
        if source is None:
            return
        candidates = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and not o.tapped
            and o.id != source.id
        ]
        if not candidates:
            state.log.append(
                f"{source.printing.name}: no untapped permanent to "
                f"tap — sacrificed"
            )
            _sacrifice(state, source)
            return
        legal = [("untapped_permanent", o.id) for o in candidates]
        p = state.player_by_id(controller_id)
        chosen = None
        if p.agent is not None:
            chosen = p.agent.choose_target(state, controller_id, legal)
        if chosen is None or chosen[0] != "untapped_permanent":
            state.log.append(
                f"{source.printing.name}: declined to tap a permanent "
                f"— sacrificed"
            )
            _sacrifice(state, source)
            return
        target = next(
            (o for o in candidates if o.id == chosen[1]), None
        )
        if target is None:
            state.log.append(
                f"{source.printing.name}: chosen permanent missing "
                f"— sacrificed"
            )
            _sacrifice(state, source)
            return
        state.log.append(
            f"P{controller_id} taps {target.printing.name} to keep "
            f"{source.printing.name}"
        )
        tap_permanent(state, target)

    def describe(self) -> str:
        return (
            "sacrifice this unless you tap an untapped permanent you control"
        )



@dataclass(frozen=True)
class SacrificeSelfUnlessVoidActive(Effect):
    """End-step effect: "Sacrifice this unless Void is active." Reads
    state.resolving_source to find the sacrificing permanent. Matches
    Chorale of the Void's upkeep sustain.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_void_active
        from mtgz_ir.turn import sacrifice as _sacrifice

        source = state.resolving_source
        if source is None:
            return
        if is_void_active(state):
            state.log.append(
                f"{source.printing.name}: Void active, no sacrifice"
            )
            return
        state.log.append(
            f"{source.printing.name} sacrificed (Void not active)"
        )
        _sacrifice(state, source)

    def describe(self) -> str:
        return "sacrifice this unless Void is active"



@dataclass(frozen=True)
class OptionalPutCreatureFromHandOntoBattlefield(Effect):
    """"You may put a creature card from your hand onto the battlefield."

    Matches Sledge-Class Seedship's attack trigger (and similar
    "cheat creatures into play" effects). Agent picks via
    ``choose_card_from_library`` (reusing the generic "pick one id
    from a list" hook — the hook is zone-agnostic). Declining is
    legal — the effect silently does nothing.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        p = state.player_by_id(controller_id)
        options = [c for c in p.hand if c.printing.is_creature]
        if not options:
            return
        chosen_id = None
        if p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options]
            )
        if chosen_id is None:
            return
        card = next((c for c in options if c.id == chosen_id), None)
        if card is None:
            return
        p.hand.remove(card)
        put_into_battlefield(state, card, controller_id)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} from hand "
            f"onto the battlefield"
        )

    def describe(self) -> str:
        return "you may put a creature card from your hand onto the battlefield"



@dataclass(frozen=True)
class OptionalPutLandFromHandOntoBattlefield(Effect):
    """"You may put a land card from your hand onto the battlefield."

    Matches Burgeoning's trigger effect. The land enters untapped
    (Burgeoning doesn't say "tapped") and fires landfall triggers via
    ``fire_landfall_triggers`` — "enters the battlefield" isn't the
    same event as "plays a land" so Burgeoning-put lands don't re-fire
    opponent-plays-land chains. Declining is legal.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import fire_landfall_triggers

        p = state.player_by_id(controller_id)
        options = [c for c in p.hand if c.printing.is_land]
        if not options:
            return
        chosen_id = None
        if p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options]
            )
        if chosen_id is None:
            return
        card = next((c for c in options if c.id == chosen_id), None)
        if card is None:
            return
        p.hand.remove(card)
        card.zone = Zone.BATTLEFIELD
        card.controller_id = controller_id
        card.tapped = bool(card.printing.enters_tapped)
        card.entered_battlefield_turn = state.turn_number
        state.battlefield.append(card)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} from hand "
            f"onto the battlefield"
        )
        # ``fire_landfall_triggers`` walks the legacy string-key
        # ``"landfall"`` abilities AND dispatches the predicate-style
        # ENTERS event, so every migrated landfall ability fires.
        fire_landfall_triggers(state, card)

    def describe(self) -> str:
        return "you may put a land card from your hand onto the battlefield"



@dataclass(frozen=True)
class OptionalSacArtifactForEffect(Effect):
    """"You may sacrifice an artifact. If you do, [inner_effect]."

    Similar to OptionalSacAnotherForEffect but doesn't require a
    ``state.resolving_source`` for self-exclusion — the "another"
    clause isn't in the oracle text (Lithobraking: "you may sacrifice
    an artifact"). The "may" is honored via ``Agent.choose_yes_no``;
    default base-Agent behavior is "True" so existing cards that relied
    on "may → always yes" are unaffected. Once the controller agrees
    to sacrifice, ``choose_sacrifice`` picks WHICH artifact.

    Targeting is forwarded from the inner effect so that inner effects
    that require a target (e.g. a counter on a creature) work correctly:
    ``requires_target``, ``legal_targets``, and the ``targets`` tuple
    passed at resolve time are all delegated to ``inner_effect``.
    """

    inner_effect: Effect

    def requires_target(self) -> bool:
        return self.inner_effect.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.inner_effect.legal_targets(state, controller_id)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now

        options = [
            o for o in state.battlefield
            if o.controller_id == controller_id and is_artifact_now(state, o)
        ]
        if not options:
            return
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            "Sacrifice an artifact for the rider?",
        ):
            state.log.append(
                f"P{controller_id} declines optional artifact sacrifice"
            )
            return
        if p.agent is None:
            chosen = options[0]
        else:
            pick_id = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        from mtgz_ir.turn import sacrifice as _sacrifice

        state.log.append(
            f"P{controller_id} optionally sacrifices {chosen.printing.name}"
        )
        _sacrifice(state, chosen)
        self.inner_effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"you may sacrifice an artifact; if you do, "
            f"{self.inner_effect.describe()}"
        )



@dataclass(frozen=True)
class OptionalSacAnotherForEffect(Effect):
    """"You may sacrifice another creature or artifact. If you do,
    [inner_effect]."

    General version of OptionalSacAnotherForPumpSelfUEOT: the inner
    effect (e.g., DrawCards) runs only if the sacrifice is paid. Agent
    decides via choose_sacrifice; no legal option → silent no-op.
    """

    inner_effect: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now

        source = state.resolving_source
        if source is None:
            return
        options = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and o.id != source.id
            and (o.printing.is_creature or is_artifact_now(state, o))
        ]
        if not options:
            return
        p = state.player_by_id(controller_id)
        if p.agent is None:
            chosen = options[0]
        else:
            pick_id = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        from mtgz_ir.turn import sacrifice as _sacrifice

        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name} "
            f"for {source.printing.name}'s trigger"
        )
        _sacrifice(state, chosen)
        # The source itself might have been sacrificed — clear resolving
        # source temporarily? No — state.resolving_source still points to
        # the source object even if it's off the battlefield now. Inner
        # effect can still read it if needed (e.g., for a self-target),
        # but for DrawCards/GainLife etc. we only need controller_id.
        self.inner_effect.resolve(state, controller_id, ())

    def describe(self) -> str:
        return (
            f"you may sacrifice another creature or artifact; if you "
            f"do, {self.inner_effect.describe()}"
        )



@dataclass(frozen=True)
class OptionalSacAnotherForPumpSelfUEOT(Effect):
    """"You may sacrifice another creature or artifact. If you do, this
    creature gets +P/+T until end of turn."

    Agent decides via choose_sacrifice-style hook — we reuse the
    sacrifice agent method and treat a "no legal option" situation as
    decline. For the RandomAgent the heuristic is "pay if an option
    exists", matching the card's near-always-beneficial pump.
    """

    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: frozenset[str] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now

        source = state.resolving_source
        if source is None:
            return
        options = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and o.id != source.id
            and (o.printing.is_creature or is_artifact_now(state, o))
        ]
        if not options:
            return
        p = state.player_by_id(controller_id)
        if p.agent is None:
            # Defensive default: pay the cost when the agent is absent.
            chosen = options[0]
        else:
            pick_id = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        from mtgz_ir.turn import sacrifice as _sacrifice

        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name} "
            f"to pump {source.printing.name}"
        )
        _sacrifice(state, chosen)
        source.temp_power_bonus += self.power_bonus
        source.temp_toughness_bonus += self.toughness_bonus
        if self.granted_keywords:
            from mtgz_ir.effects.durations import EndOfTurn
            from mtgz_ir.effects.generic import GrantKeywords

            ApplyEffect(
                inner=GrantKeywords(keywords=self.granted_keywords),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, ())

    def describe(self) -> str:
        parts = []
        if self.power_bonus or self.toughness_bonus:
            parts.append(
                f"+{self.power_bonus}/+{self.toughness_bonus}"
            )
        if self.granted_keywords:
            parts.append(
                "gains " + ", ".join(sorted(self.granted_keywords))
            )
        body = " and ".join(parts) if parts else "pump"
        return (
            f"you may sacrifice another creature or artifact; if you "
            f"do, this creature gets {body} until end of turn"
        )



@dataclass(frozen=True)
class MayDo(Effect):
    """Wraps an inner effect to make it gated by ``Agent.choose_yes_no``.

    Named ``MayDo`` rather than ``Optional`` to avoid confusion with
    ``typing.Optional``.

    ``resolve`` returns:
    - ``False`` if the agent declined the may.
    - Whatever the inner effect's ``resolve`` returned (``True``,
      ``False``, or ``None``) if the agent accepted.

    Any inner Effect can be wrapped — the "may X" decoration is
    independent of the inner action's specifics. Combine with
    ``IfYouDo`` for the "you may X. If you do, Y." idiom common in
    MTG oracle text.
    """

    inner: Effect
    prompt: str = ""

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.inner.legal_targets(state, controller_id)

    def target_count_range(self) -> tuple[int, int]:
        # Optional inheriting min=0 max=inner_max — caller may decline.
        lo, hi = self.inner.target_count_range()
        return (0, hi)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool | None:
        p = state.player_by_id(controller_id)
        prompt = self.prompt or self.inner.describe()
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, prompt
        ):
            state.log.append(
                f"P{controller_id} declines optional: {self.inner.describe()}"
            )
            return False
        return self.inner.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"you may {self.inner.describe()}"


@dataclass(frozen=True)
class IfYouDo(Effect):
    """Run ``do_action``; run ``then_action`` only if ``do_action``
    accomplished something. The "if you do" gate of MTG oracle text.

    ``do_action.resolve`` is interpreted as:
    - ``True`` or ``None`` (default for effects that don't signal
      completion) → ``then_action`` fires.
    - ``False`` → ``then_action`` is skipped.

    Effects that can fail in legal ways (empty hand, no legal sac
    targets, etc.) should override their ``resolve`` to return
    ``False`` so this gate fires correctly. ``DiscardSelf`` and
    ``Optional`` already do.

    Examples:
    - "You may discard a card. If you do, draw a card."
        IfYouDo(MayDo(DiscardSelf(amount=1)), DrawCards(amount=1))
    - "Sacrifice a creature. If you do, draw a card."
        IfYouDo(SacrificeACreature(), DrawCards(amount=1))
    """

    do_action: Effect
    then_action: Effect

    def requires_target(self) -> bool:
        return (
            self.do_action.requires_target()
            or self.then_action.requires_target()
        )

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        if self.do_action.requires_target():
            return self.do_action.legal_targets(state, controller_id)
        if self.then_action.requires_target():
            return self.then_action.legal_targets(state, controller_id)
        return []

    def target_count_range(self) -> tuple[int, int]:
        a_min, a_max = self.do_action.target_count_range()
        t_min, t_max = self.then_action.target_count_range()
        return (a_min + t_min, a_max + t_max)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool | None:
        result = self.do_action.resolve(state, controller_id, targets)
        # None defaults to "did something" — back-compat for effects
        # that don't yet signal completion explicitly.
        if result is False:
            return False
        # then_action gets empty targets unless it's the targeted arm.
        then_targets = (
            targets if self.then_action.requires_target() else ()
        )
        self.then_action.resolve(state, controller_id, then_targets)
        return True

    def describe(self) -> str:
        return (
            f"{self.do_action.describe()}; if you do, "
            f"{self.then_action.describe()}"
        )


@dataclass(frozen=True)
class IfNot(Effect):
    """Run ``then`` only if ``condition`` is False at resolve time.
    The "unless <condition>" gate of MTG oracle text.

    Inverse of an ``if <condition>, do X`` rider: when
    ``condition.matches(state, controller_id)`` is True, the effect is
    a no-op (the unless-clause "dodges" the inner action). When the
    condition is False, ``then`` resolves normally.

    The viewer for the ``Condition.matches`` call is the effect's
    controller (the player resolving the trigger / spell). The
    ``chosen_targets`` parameter is left at its default (None) — these
    "unless you control X" clauses are evaluated against static state at
    resolve time, not against the spell's chosen targets.

    Examples:
    - "Discard a card unless you control another creature with power 2
      or less." (Splitskin Doll)
        IfNot(
            condition=ControlsAnotherCreatureWithPowerAtMost(2),
            then=DiscardSelf(amount=1),
        )

    Note that ``IfNot`` carries a Condition (a structural state predicate
    from ``effects.conditions``) rather than another Effect — so it
    composes with the same gate vocabulary used for cost reductions and
    static continuous effects, rather than re-implementing predicates
    inside the combinator.
    """

    condition: Condition
    then: Effect

    def requires_target(self) -> bool:
        return self.then.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.then.legal_targets(state, controller_id)

    def target_count_range(self) -> tuple[int, int]:
        return self.then.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> bool | None:
        if self.condition.matches(state, controller_id):
            state.log.append(
                f"P{controller_id} skips '{self.then.describe()}' "
                f"(unless-condition met)"
            )
            return False
        return self.then.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"{self.then.describe()} unless {self.condition!r}"


# OptionalDiscardCardThenDrawN was deleted in the cleanup pass: the
# generic ``IfYouDo(MayDo(DiscardSelf(1)), DrawCards(N))`` composes
# the same shape directly. ``DiscardSelf`` returns a bool (``True`` if
# any card was discarded, ``False`` on empty hand) so the IfYouDo gate
# fizzles correctly. Any callsite that previously instantiated
# ``OptionalDiscardCardThenDrawN(amount=N)`` should compose the
# combinators directly.



@dataclass(frozen=True)
class OptionalSacLandOrLanderForBasicLandRamp(Effect):
    """"You may sacrifice a land or Lander. If you do, search your
    library for up to ``land_count`` basic land cards, put them onto
    the battlefield tapped, then shuffle."

    Matches Larval Scoutlander's ETB. A "land or Lander" is: a land
    permanent, or a Lander token artifact. The agent picks via
    choose_sacrifice; if no legal option, the effect is a no-op.
    Following OptionalSac precedent, the "may" is always yes when
    possible.
    """

    land_count: int = 2

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_subtypes
        from mtgz_ir.turn import sacrifice as _sacrifice

        options = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and (
                o.printing.is_land
                or "Lander" in effective_subtypes(state, o)
            )
        ]
        if not options:
            return
        p = state.player_by_id(controller_id)
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
            f"(land/Lander ramp)"
        )
        _sacrifice(state, chosen)
        SearchLibraryForBasicLandTapped(count=self.land_count).resolve(
            state, controller_id,
        )

    def describe(self) -> str:
        return (
            f"you may sacrifice a land or Lander; if you do, search "
            f"your library for up to {self.land_count} basic land cards, "
            f"put them onto the battlefield tapped, then shuffle"
        )



@dataclass(frozen=True)
class ConditionalIfOpponentHasMoreLands(Effect):
    """Run ``effect`` only if an opponent controls more lands than the
    controller at resolution.

    Matches EoE's "if an opponent controls more lands than you" ETB
    riders (Sunstar Expansionist). Untargeted and untargeted-only.
    """

    effect: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        my_lands = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_land
        )
        enemy_max = 0
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            count = sum(
                1 for o in state.battlefield
                if o.controller_id == p.id and o.printing.is_land
            )
            if count > enemy_max:
                enemy_max = count
        if enemy_max <= my_lands:
            state.log.append(
                "conditional skipped (no opponent has more lands)"
            )
            return
        self.effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"if an opponent controls more lands than you, "
            f"{self.effect.describe()}"
        )



@dataclass(frozen=True)
class ConditionalIfYouControlAtLeast(Effect):
    """Run ``inner`` only if the effect's controller controls at least
    ``threshold`` permanents matching ``predicate`` at resolution.

    Predicates:
      - ``"artifact"`` — any permanent with the Artifact type.
      - ``"creature"`` — any permanent that counts as a creature now
        (printed creature + stationed Spacecraft + animated manland).
      - ``"land"`` — any permanent with the Land type.

    Matches "At the beginning of your upkeep, if you control three or
    more artifacts, ..." (Inventors' Fair) and kin.
    """

    threshold: int
    predicate: CardTypeWord
    inner: Effect

    def __post_init__(self) -> None:
        object.__setattr__(self, "predicate", CardTypeWord(self.predicate))

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def target_count_range(self) -> tuple[int, int]:
        return self.inner.target_count_range()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.inner.legal_targets(state, controller_id)

    def _count(self, state: "GameState", controller_id: int) -> int:
        from mtgz_ir.game import is_artifact_now, is_creature_now

        if self.predicate == "artifact":
            return sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id
                and is_artifact_now(state, o)
            )
        if self.predicate == "creature":
            return sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id and is_creature_now(o)
            )
        if self.predicate == "land":
            return sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id and o.printing.is_land
            )
        return 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if self._count(state, controller_id) < self.threshold:
            state.log.append(
                f"conditional skipped (you control fewer than "
                f"{self.threshold} {self.predicate}s)"
            )
            return
        self.inner.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"if you control {self.threshold} or more {self.predicate}s, "
            f"{self.inner.describe()}"
        )



@dataclass(frozen=True)
class ConditionalIfYouControlSubtype(Effect):
    """Run ``then_effect`` if the effect's controller controls at least one
    permanent with ``subtype`` at resolution; otherwise run ``else_effect``.

    Reads each battlefield permanent's effective subtypes (printed plus
    any layer-3/4 registry-granted entries via ``effective_subtypes``) so
    a creature whose subtype was added by a Conspiracy-style or Cytoshape
    effect counts. Matches DSK's
    "If you control a Demon, ..., otherwise, ..." gate on Unholy Annex
    (Unholy Annex // Ritual Chamber): the upkeep / end-step rider
    "If you control a Demon, each opponent loses 2 life and you gain
    2 life. Otherwise, you lose 2 life."

    Mirrors ``ConditionalIfVoid`` / ``ConditionalIfYouControlAtLeast`` —
    forwards ``requires_target`` / ``legal_targets`` to whichever branch
    requires a target. Both branches may opt out of targeting (the
    Unholy Annex case has no targets), which is the simple shape this
    combinator's tests cover. Either branch may be ``None`` (the
    "otherwise" arm is ``None`` for cards whose conditional has no else
    clause); a ``None`` branch is a silent no-op when picked.
    """

    subtype: str
    then_effect: Effect
    else_effect: "Effect | None" = None

    def requires_target(self) -> bool:
        then_req = self.then_effect.requires_target()
        else_req = (
            self.else_effect.requires_target()
            if self.else_effect is not None
            else False
        )
        return then_req or else_req

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # Mirrors Sequence's single-target constraint: only one branch
        # may require a target. If both do, they'd need to share a
        # common target space (caller picks once).
        if self.then_effect.requires_target():
            return self.then_effect.legal_targets(state, controller_id)
        if (
            self.else_effect is not None
            and self.else_effect.requires_target()
        ):
            return self.else_effect.legal_targets(state, controller_id)
        return []

    def _has_subtype(
        self, state: "GameState", controller_id: int,
    ) -> bool:
        from mtgz_ir.game import effective_subtypes

        for o in state.battlefield:
            if o.controller_id != controller_id:
                continue
            if self.subtype in effective_subtypes(state, o):
                return True
        return False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        branch = (
            self.then_effect
            if self._has_subtype(state, controller_id)
            else self.else_effect
        )
        if branch is None:
            state.log.append(
                f"conditional (you control a {self.subtype}) — "
                f"no else branch; nothing happens"
            )
            return
        branch.resolve(state, controller_id, targets)

    def describe(self) -> str:
        else_desc = (
            f" Otherwise, {self.else_effect.describe()}."
            if self.else_effect is not None
            else ""
        )
        return (
            f"if you control a {self.subtype}, "
            f"{self.then_effect.describe()}.{else_desc}"
        )



@dataclass(frozen=True)
class ConditionalIfYouGainedLifeAtLeast(Effect):
    """Run ``inner`` only if the effect's controller has gained at least
    ``threshold`` life this turn at resolution.

    Matches "draw a card if you've gained 3 or more life this turn"
    end-step riders (Haliya, Guided by Light).
    """

    threshold: int
    inner: Effect

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.inner.legal_targets(state, controller_id)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        gained = state.player_by_id(controller_id).life_gained_this_turn
        if gained < self.threshold:
            state.log.append(
                f"conditional skipped (gained {gained} life, need "
                f"{self.threshold})"
            )
            return
        self.inner.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"if you've gained {self.threshold} or more life this turn, "
            f"{self.inner.describe()}"
        )



@dataclass(frozen=True)
class ConditionalOnTappedCreatures(Effect):
    """Run ``effect`` only if the controller has at least ``min_count``
    tapped creatures on the battlefield at resolution.

    Matches the "if you control two or more tapped creatures" intervening-if
    clause on EoE cards like Flight-Deck Coordinator and Sunstar Chaplain.
    """

    effect: Effect
    min_count: int = 2

    def requires_target(self) -> bool:
        return self.effect.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.effect.legal_targets(state, controller_id)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_creature_now

        tapped_count = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id
            and is_creature_now(o)
            and o.tapped
        )
        if tapped_count < self.min_count:
            state.log.append(
                f"conditional skipped (need {self.min_count} tapped creatures, "
                f"have {tapped_count})"
            )
            return
        self.effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"if you control {self.min_count} or more tapped creatures, "
            f"{self.effect.describe()}"
        )



@dataclass(frozen=True)
class ConditionalIfSourcePowerAtLeast(Effect):
    """Run ``effect`` only if the triggering source's effective power is
    at least ``threshold`` at resolution. Reads state.resolving_source
    for the source (typically set by the trigger dispatcher).

    Matches oracle wording like "while its power is 4 or greater" on
    Syr Vondam, Sunstar Exemplar's leaves-battlefield trigger.
    """

    effect: Effect
    threshold: int = 4

    def requires_target(self) -> bool:
        return self.effect.requires_target()

    def target_count_range(self) -> "tuple[int, float]":
        """Delegate to the inner effect so "up to one target" wrappers
        (e.g. ``DestroyUpToOneTargetNonlandPermanent`` inside this gate)
        correctly report ``(0, 1)`` rather than the default ``(0, 0)``
        that the base ``Effect.target_count_range`` would infer from the
        empty ``self.targets`` tuple.
        """
        return self.effect.target_count_range()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.effect.legal_targets(state, controller_id)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        source = state.resolving_source
        if source is None:
            state.log.append("power-threshold conditional has no source")
            return
        power = effective_power(state, source)
        if power < self.threshold:
            state.log.append(
                f"conditional skipped (need power ≥ {self.threshold}, "
                f"have {power})"
            )
            return
        self.effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"if this permanent's power is {self.threshold} or greater, "
            f"{self.effect.describe()}"
        )



@dataclass(frozen=True)
class ModalChooseOne(Effect):
    """A "choose one" spell or ability effect. Delegates mode pick to the
    controller's agent via ``choose_mode``, then resolves that mode.

    With ``or_both=True``, oracle text reads "Choose one or both" — the
    controller may pick either mode alone or both modes. The agent is
    asked via ``choose_modes`` (plural) for the ordered list of modes to
    resolve; modes resolve in that order (per CR 700.2d, "the controller
    of a modal spell or ability chooses the order in which the modes are
    resolved").

    Currently supports modes that are EITHER untargeted OR require a
    single target; the chosen mode's ``legal_targets`` is surfaced through
    the spell-cast pipeline via ``requires_target``.

    Sequencing: we exit early at cast time if the caller only wants to
    know about targets — the chosen mode is cached on a mutable box since
    dataclasses are frozen. Simpler alternative: call choose_mode at
    resolve time and discard any pre-picked target if the mode is
    untargeted. We take the simpler path here.
    """

    modes: tuple[Effect, ...]
    or_both: bool = False

    def requires_target(self) -> bool:
        # Castability gate: the spell should be castable as long as ANY
        # mode is playable. If at least one mode is untargeted, the
        # player can always pick that mode — so don't gate on targets.
        # Only require a target when every mode needs one.
        return all(m.requires_target() for m in self.modes)

    def target_count_range(self) -> tuple[int, int]:
        any_targeted = any(m.requires_target() for m in self.modes)
        if not any_targeted:
            return (0, 0)
        all_targeted = all(m.requires_target() for m in self.modes)
        if self.or_both:
            # "Choose one or both": at most one target per targeted
            # mode (we don't model multi-target sub-modes). When every
            # mode is targeted, picking both modes needs one target
            # apiece — max_t equals the count of targeted modes. min_t
            # is 0 when any mode is untargeted (you can pick only that
            # mode); 1 when every mode is targeted (you must resolve
            # at least one mode and that mode needs a target).
            targeted_count = sum(
                1 for m in self.modes if m.requires_target()
            )
            return (1 if all_targeted else 0, targeted_count)
        # min_t=0 for mixed modes (you might pick the untargeted mode);
        # min_t=min-of-targeted-modes when every mode needs a target.
        # max_t=max across modes — supports multi-target modes (e.g.
        # Archenemy's Charm mode 1 returns up to 2 cards).
        # Defensive: not every mode-like object subclasses Effect (some
        # tests pass minimal stubs), so fall back to the base default
        # (1, 1) when target_count_range isn't implemented.
        def _range(m):
            tcr = getattr(m, "target_count_range", None)
            return tcr() if callable(tcr) else (1, 1)
        targeted_ranges = [
            _range(m) for m in self.modes if m.requires_target()
        ]
        max_t = max(r[1] for r in targeted_ranges)
        if not all_targeted:
            return (0, max_t)
        min_t = min(r[0] for r in targeted_ranges)
        return (min_t, max_t)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # Union of all modes' legal targets. The picked mode's resolve
        # will ignore targets it doesn't use.
        out: list[Target] = []
        seen: set[Target] = set()
        for m in self.modes:
            if not m.requires_target():
                continue
            for t in m.legal_targets(state, controller_id):
                if t not in seen:
                    seen.add(t)
                    out.append(t)
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        p = state.player_by_id(controller_id)
        # CR 601.2e: modes are picked at cast time, replayed via
        # state.resolving_modes during resolution. Fallback to a
        # resolve-time agent prompt only when this effect is being
        # used as a sub-effect outside the spell-cast pipeline (e.g.
        # nested inside a triggered-ability effect that didn't carry
        # cast-time mode picks).
        cast_modes = state.resolving_modes
        if self.or_both:
            self._resolve_or_both(
                state, controller_id, targets, p, cast_modes,
            )
            return
        if cast_modes:
            idx = cast_modes[0]
            idx = max(0, min(idx, len(self.modes) - 1))
        elif p.agent is None:
            idx = 0
        else:
            idx = p.agent.choose_mode(
                state, controller_id, [m.describe() for m in self.modes]
            )
            idx = max(0, min(idx, len(self.modes) - 1))
        chosen = self.modes[idx]
        state.log.append(
            f"modal: P{controller_id} chooses mode {idx} ({chosen.describe()})"
        )
        # If the chosen mode requires a target, filter the passed targets
        # down to those legal for THIS mode (the cast pipeline may have
        # collected targets across the union of all modes' pools). Fizzle
        # silently if none survive.
        if chosen.requires_target():
            legal = chosen.legal_targets(state, controller_id)
            filtered = tuple(t for t in targets if t in legal)
            if not filtered:
                state.log.append(
                    "modal fizzle: chosen mode requires a legal target"
                )
                return
            chosen.resolve(state, controller_id, filtered)
            return
        chosen.resolve(state, controller_id, targets)

    def _resolve_or_both(
        self,
        state: "GameState",
        controller_id: int,
        targets: tuple[Target, ...],
        p,
        cast_modes: tuple[int, ...] = (),
    ) -> None:
        """Resolve a "Choose one or both" modal effect. Asks the agent
        for an ordered list of mode indices via ``choose_modes`` (plural)
        and resolves each in turn, dispatching legal targets per mode.

        ``cast_modes`` is the cast-time pick (CR 601.2e); when present
        we replay it instead of prompting again at resolve time.

        Target dispatch: for each chosen mode that requires a target,
        we consume the first remaining target whose value appears in
        that mode's ``legal_targets``. A mode that requires a target but
        has no legal target available silently fizzles (logged), matching
        the single-mode fizzle path.
        """
        if cast_modes:
            order = list(cast_modes)
        elif p.agent is None:
            order = [0]
        else:
            order = p.agent.choose_modes(
                state, controller_id, [m.describe() for m in self.modes]
            )
        # Sanitise: keep unique, in-range indices preserving order.
        seen: set[int] = set()
        clean: list[int] = []
        for idx in order:
            if 0 <= idx < len(self.modes) and idx not in seen:
                clean.append(idx)
                seen.add(idx)
        if not clean:
            # Empty / all-invalid pick → default to mode 0 so the spell
            # still does something (matches the agent-absent default).
            clean = [0]
        state.log.append(
            f"modal: P{controller_id} chooses mode(s) "
            f"{clean} ({', '.join(self.modes[i].describe() for i in clean)})"
        )
        remaining = list(targets)
        for idx in clean:
            mode = self.modes[idx]
            if mode.requires_target():
                legal = mode.legal_targets(state, controller_id)
                picked = None
                for t in remaining:
                    if t in legal:
                        picked = t
                        break
                if picked is None:
                    state.log.append(
                        f"modal fizzle: mode {idx} requires a legal target"
                    )
                    continue
                remaining.remove(picked)
                mode.resolve(state, controller_id, (picked,))
            else:
                mode.resolve(state, controller_id, ())

    def describe(self) -> str:
        joiner = " AND/OR " if self.or_both else " OR "
        return joiner.join(m.describe() for m in self.modes)



@dataclass(frozen=True)
class Sequence(Effect):
    """Run multiple effects in order as a single composite effect.

    Target selection: only one of the sub-effects may require a target (this
    keeps the cast-time target-selection pipeline single-choice). That one
    sub-effect's legal_targets becomes the Sequence's. At resolution, the
    chosen target(s) are passed to the targeted sub-effect; untargeted sub-
    effects receive an empty tuple.
    """

    effects: tuple[Effect, ...]

    def __post_init__(self) -> None:
        targeted = [e for e in self.effects if e.requires_target()]
        if len(targeted) > 1:
            raise ValueError(
                "Sequence supports at most one targeting sub-effect "
                f"(got {len(targeted)})"
            )

    def requires_target(self) -> bool:
        return any(e.requires_target() for e in self.effects)

    def target_count_range(self) -> tuple[int, int]:
        """Return the combined (min, max) target range from sub-effects.

        The base ``Effect.target_count_range()`` uses ``self.targets``
        (which ``Sequence`` doesn't carry) and falls back to
        ``(1, 1) if self.requires_target() else (0, 0)``.  That
        fallback misses the ``count_min=0, count_max=1`` case produced
        by optional sub-effects (e.g. ``ReturnTargetCreatureToOwnersHand
        (optional=True)`` in a Sequence with ``ManifestDread()``): the
        old fallback returns ``(0, 0)`` because ``requires_target()``
        is False, causing the cast pipeline to pick zero targets even
        though the printed "up to one" text says the player may choose.

        This override walks the sub-effects directly, summing their
        counts from ``target_count_range()``.  New-shape effects expose
        accurate (min, max) values; old-shape effects whose
        ``requires_target()`` returns True contribute (1, 1).  Because
        ``Sequence.__post_init__`` enforces at most one targeting sub-
        effect, the sum is still at most (1, 1) for mandatory targets
        or (0, 1) for optional ones — no multi-target ambiguity is
        introduced.
        """
        total_min = 0
        total_max = 0
        for e in self.effects:
            lo, hi = e.target_count_range()
            total_min += lo
            total_max += hi
        return (total_min, total_max)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # Iterate sub-effects in order, returning the first one that
        # can take targets (count_max > 0). This covers both mandatory
        # sub-effects (requires_target() = True → count_max >= 1) and
        # optional sub-effects (count_min=0, count_max=1 — the
        # "up to one" family). The existing constraint (at most one
        # targeting sub-effect per Sequence, enforced by __post_init__)
        # means at most one sub-effect has count_max > 0.
        for e in self.effects:
            lo, hi = e.target_count_range()
            if hi > 0:
                return e.legal_targets(state, controller_id)
        return []

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        # Sub-effects with ``hi > 0`` (a target slot of their own) get
        # the chosen ``targets`` tuple — that's the standard targeting
        # path. Sub-effects with ``hi == 0`` declare themselves
        # untargeted but may still consume the same target tuple as an
        # ANAPHORIC reference to a target chosen by an upstream sub-
        # effect (e.g. Requiem Monolith's ``MayHaveSourceDealDamage
        # ToTarget`` reads the same creature target the upstream
        # ``GrantTriggeredAbilityUEOT`` resolved against — "that
        # creature's controller may have this artifact deal 1 damage
        # to it"). Passing ``targets`` to those sub-effects is safe
        # because non-anaphoric untargeted effects (``DrawCards``,
        # ``SacrificeSelf``, ``GainLife``, etc.) accept the parameter
        # via ``targets: tuple[Target, ...] = ()`` and ignore it.
        for e in self.effects:
            e.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return ". ".join(e.describe() for e in self.effects) + "."



@dataclass(frozen=True)
class ConditionalIfVoid(Effect):
    """Run ``then_effect`` when Void is active on resolution; else ``else_effect``.

    Checks ``is_void_active(state)`` at resolution time (MTG-accurate for
    intervening-if clauses on spell resolution). Either branch may be None
    (the condition simply does nothing that side).

    Matches the "Void —" rider on spell effects like Decode Transmissions
    ("instead...") or Hymn of the Faller ("...draw another card").
    """

    then_effect: Effect
    else_effect: Effect | None = None

    def requires_target(self) -> bool:
        then_req = self.then_effect.requires_target()
        else_req = self.else_effect.requires_target() if self.else_effect else False
        return then_req or else_req

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # Matches Sequence's constraint: only one branch may require a target.
        if self.then_effect.requires_target():
            return self.then_effect.legal_targets(state, controller_id)
        if self.else_effect is not None and self.else_effect.requires_target():
            return self.else_effect.legal_targets(state, controller_id)
        return []

    def __post_init__(self) -> None:
        # Both branches can require a target if they'd accept the same
        # target space — the caller picks once at cast time and resolve
        # forwards that single target tuple to whichever branch fires.
        # No eager check here; incompatible targets would be caught at
        # resolve time as a silent no-op.
        return

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_void_active

        branch = self.then_effect if is_void_active(state) else self.else_effect
        if branch is None:
            return
        branch.resolve(state, controller_id, targets)

    def describe(self) -> str:
        then_desc = self.then_effect.describe()
        if self.else_effect is None:
            return f"Void — {then_desc}"
        return (
            f"if Void is active, {then_desc}; otherwise, "
            f"{self.else_effect.describe()}"
        )



@dataclass(frozen=True)
class OptionalManaCostThenEffect(Effect):
    """"You may pay {cost}. If you do, [inner_effect]."

    Wraps an inner Effect with an optional mana-cost gate. Following the
    OptionalSac/Kavaron Harrier precedent in this codebase, the "may"
    is treated as YES whenever the controller can afford ``cost_str``;
    otherwise the trigger does nothing.

    Used for triggers like Oreplate Pangolin's "Whenever another artifact
    you control enters, you may pay {1}. If you do, put a +1/+1 counter
    on this creature."
    """

    cost_str: str
    inner_effect: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import auto_tap_for_cost, can_pay_cost
        from mtgz_ir.cards import parse_mana_cost

        cost = parse_mana_cost(self.cost_str)
        if not can_pay_cost(state, controller_id, cost):
            source = state.resolving_source
            name = source.printing.name if source is not None else "ability"
            state.log.append(
                f"{name}: P{controller_id} can't pay {self.cost_str}; "
                f"trigger does nothing"
            )
            return
        auto_tap_for_cost(state, controller_id, cost)
        self.inner_effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"you may pay {self.cost_str}; if you do, "
            f"{self.inner_effect.describe()}"
        )



@dataclass(frozen=True)
class ConditionalIfCastFromHand(Effect):
    """Run ``inner_effect`` only if the resolving source was cast as a
    spell from its controller's hand, mirroring the printed "if you
    cast it" gate on triggers like Weftwalking's ETB.

    Today this engine puts enchantments onto the battlefield only via
    the cast-from-hand path (no flicker-to-battlefield, no
    enchantment-token creation), so the gate is structurally a no-op:
    every Weftwalking ETB that fires was indeed cast. The wrapper
    still honors the printed text — a future engine that puts an
    enchantment onto the battlefield without casting (a hypothetical
    flicker-an-enchantment effect) would correctly suppress the inner
    effect because ``state.resolving_source.cast_via_hand`` would be
    False on that object.

    The check reads ``cast_via_hand`` on ``state.resolving_source``
    even though that flag is cleared in ``put_into_battlefield`` after
    the cast-from-hand-only ETB replacements but BEFORE ETB triggers
    fire. That's a known engine-side gap: the trigger fires after the
    flag is consumed. Until the engine grows a persistent
    "this-permanent-was-cast-as-a-spell" provenance flag, this
    wrapper degrades to "always run the inner effect" — same outcome
    as the bespoke ``WeftwalkingETBEffect`` it replaces, which also
    documents the same simplification.
    """

    inner_effect: Effect

    def requires_target(self) -> bool:
        return self.inner_effect.requires_target()

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return self.inner_effect.legal_targets(state, controller_id)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        # Engine simplification (see class docstring): every enchantment
        # ETB observable today reaches resolve via the cast path, so the
        # gate is unconditionally satisfied. When/if a non-cast ETB path
        # exists for enchantments, this branch will need to consult a
        # persistent "this permanent entered via cast" flag rather than
        # the ephemeral ``cast_via_hand`` (already cleared at this point).
        self.inner_effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"if you cast it, {self.inner_effect.describe()}"





@dataclass(frozen=True)
class IfConditionElse(Effect):
    """Resolve ``then_effect`` if ``condition`` holds for the controller
    at resolve time, else ``else_effect`` (which may be ``None``).

    The generic "<effect> instead if <condition>" replacement rider
    (Cabal Ritual's ``Threshold — Add {B}{B}{B}{B}{B} instead if there
    are seven or more cards in your graveyard``): ``then_effect`` is the
    rider's effect, ``else_effect`` the base clause. Targeting delegates
    to whichever branch requires a target (the branches of an
    "instead" rider share the base clause's targets).
    """

    # A ``Condition`` (``matches(state, viewer_id)``) or a
    # ``StatePredicate`` (``test(state, viewer_id)`` — Threshold's
    # ``CardsInGraveyardAtLeast``), evaluated for the controller.
    condition: "object"
    then_effect: Effect
    else_effect: "Effect | None" = None

    def _targeting_branch(self) -> "Effect | None":
        for eff in (self.then_effect, self.else_effect):
            if eff is not None and eff.requires_target():
                return eff
        return None

    def requires_target(self) -> bool:
        return self._targeting_branch() is not None

    def cast_time_shape(
        self, *, kicked: bool = False, alt_cost_paid: bool = False,
        x_value: "int | None" = None,
    ) -> Effect:
        """For conditions fixed at cast time — "If this spell was
        kicked" (Tear Asunder), an alternative cost / Overload — the
        branch, and so the target shape, is known when targets are
        chosen (CR 601.2b-c; ``actions.cast_time_effect``)."""
        from mtgz_ir.effects.generic.cast_conditionals import (
            AlternativeCostWasPaid, SpellWasKicked,
        )

        if isinstance(self.condition, SpellWasKicked):
            fired = kicked
        elif isinstance(self.condition, AlternativeCostWasPaid):
            fired = alt_cost_paid
        else:
            return self
        if fired:
            return self.then_effect
        return self.else_effect if self.else_effect is not None else self

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        branch = self._targeting_branch()
        return branch.legal_targets(state, controller_id) if branch else []

    def target_count_range(self) -> tuple[int, int]:
        branch = self._targeting_branch()
        return branch.target_count_range() if branch else (0, 0)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ):
        cond = self.condition
        holds = (
            cond.test(state, controller_id) if hasattr(cond, "test")
            else cond.matches(state, controller_id)
        )
        if holds:
            return self.then_effect.resolve(state, controller_id, targets)
        if self.else_effect is not None:
            return self.else_effect.resolve(state, controller_id, targets)
        return None

    def describe(self) -> str:
        text = f"if {self.condition!r}: {self.then_effect.describe()}"
        if self.else_effect is not None:
            text += f"; otherwise {self.else_effect.describe()}"
        return text
