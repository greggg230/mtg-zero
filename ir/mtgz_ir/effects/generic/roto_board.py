"""Engine effects for the Vintage Roto P5 package (board-state prison &
layers): mass effects keyed on mana value / counters, per-player
damage, tap-all effects and the cost atoms the package's activated
abilities print.

Grammar lives in :mod:`mtgz_ir.cards.parser.roto_board`; static
abilities that change the board's rules (combat restrictions, untap
limits, layer contributions) live in :mod:`mtgz_ir.roto_board_statics`.
Shortcuts are documented in ``docs/SEARCH_LIMITATIONS.md`` (Roto P5
section).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, ClassVar

from mtgz_ir.effects.vocab import EventSubject, PerCounterAction, RemovalAction, coerce
from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.pt_formulas import PTFormula
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.generic.targeted_removal import RemoveTargetPermanents
from mtgz_ir.effects.object_predicates import ObjectPredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------------
# Predicates
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class HasManaValueEqualToSourceCounters(ObjectPredicate):
    """"with mana value equal to the number of <kind> counters on
    <this permanent>" (Engineered Explosives' charge counters, The
    Filigree Sylex's oil counters). The source is the resolving
    ability's source; when the ability was activated by sacrificing it,
    the departed object still carries its counters (last-known
    information: ``turn.move_off_battlefield`` keeps counters on an
    object put into a graveyard or exile)."""

    kind: str

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.effects.value_predicates import mana_value_of
        from mtgz_ir.game import counter_count

        source = getattr(state, "resolving_source", None)
        if source is None:
            return False
        return mana_value_of(obj) == counter_count(source, self.kind)


@dataclass(frozen=True)
class IsLegendaryPermanent(ObjectPredicate):
    """"legendary" as a permanent modifier (Minamo's "target legendary
    permanent")."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return "Legendary" in obj.printing.supertypes


@dataclass(frozen=True)
class LacksSubtype(ObjectPredicate):
    """"non-<Subtype>" (Emerald Charm's "non-Aura enchantment"), read
    through the effective subtypes."""

    subtype: str

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_subtypes

        return self.subtype not in effective_subtypes(state, obj)


# ---------------------------------------------------------------------------
# Damage
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DamageEachPlayerPerControlled(Effect):
    """"<This> deals damage to each player equal to [twice] the number
    of <permanents> that player controls." (Price of Progress). Each
    player's amount is counted from their own permanents when the spell
    resolves; all of it is dealt as one simultaneous event (life totals
    are only checked by the state-based actions afterwards)."""

    per_filter: ObjectPredicate
    multiplier: int = 1
    label: str = "permanents"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage

        amounts: list[tuple[int, int]] = []
        for p in state.players:
            if p.has_lost:
                continue
            n = sum(
                1 for o in state.battlefield
                if o.controller_id == p.id and self.per_filter.test(o, state)
            )
            amounts.append((p.id, n * self.multiplier))
        for pid, amount in amounts:
            if amount > 0:
                _apply_damage(state, ("player", pid), amount)

    def describe(self) -> str:
        times = "twice " if self.multiplier == 2 else (
            "" if self.multiplier == 1 else f"{self.multiplier} times "
        )
        return (
            f"deals damage to each player equal to {times}the number of "
            f"{self.label} that player controls"
        )


@dataclass(frozen=True)
class DealDamageToEachTarget(Effect):
    """"<This> deals N damage to each of one or two targets." (Prismari
    Charm): one target slot with a count range; every chosen target that
    is still legal takes the damage."""

    amount: int = 1
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        if not targets or not self.targets:
            return
        slot = self.targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = []
            for chosen in targets:
                if _slot_target_still_legal(state, slot, chosen):
                    legal.append(chosen)
                else:
                    state.log.append(_fizzle_message(state, chosen))
        finally:
            state.resolving_controller_id = prior
        for chosen in legal:
            _apply_damage(state, chosen, self.amount)

    def describe(self) -> str:
        slot = self.targets[0] if self.targets else None
        n = (
            "one or two" if slot is not None and slot.count_min == 1
            and slot.count_max == 2 else "the"
        )
        return f"deals {self.amount} damage to each of {n} targets"


# ---------------------------------------------------------------------------
# Keywords until end of turn
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class LoseKeywordUEOT(Effect):
    """"Target creature loses <keyword> until end of turn." (Emerald
    Charm): a layer-6 ``lose_keyword`` entry, walked in timestamp order
    with keyword grants by ``game.effective_keywords``."""

    keyword: KeywordSpelling
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )

        if not targets or not self.targets:
            return
        slot = self.targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            ok = _slot_target_still_legal(state, slot, targets[0])
        finally:
            state.resolving_controller_id = prior
        if not ok:
            state.log.append(_fizzle_message(state, targets[0], action="effect"))
            return
        obj = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if obj is None:
            return
        src = state.resolving_source or state.resolving_spell
        state.add_continuous_effect(
            source_id=src.id if src is not None else -1,
            layer=6, kind="lose_keyword", target_id=obj.id,
            params=(self.keyword,), expires_at="cleanup",
            description=f"{obj.printing.name} loses {self.keyword} UEOT",
        )
        state.log.append(f"{obj.printing.name} loses {self.keyword}")

    def describe(self) -> str:
        return f"target creature loses {self.keyword} until end of turn"


@dataclass(frozen=True)
class GrantKeywordsToEachUEOT(Effect):
    """"<Permanents> you control gain <keywords> until end of turn."
    (Boros Charm: permanents gain indestructible): every matching
    permanent at resolution (the set is locked in, CR 611.2c) gets a
    layer-6 grant that expires at cleanup."""

    permanent_filter: ObjectPredicate
    keywords: frozenset[KeywordSpelling] = frozenset()
    label: str = "permanents you control"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            matching = [
                o for o in state.battlefield
                if self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        src = state.resolving_source or state.resolving_spell
        for obj in matching:
            for kw in sorted(self.keywords):
                state.add_continuous_effect(
                    source_id=src.id if src is not None else -1,
                    layer=6, kind="grant_keyword", target_id=obj.id,
                    params=(kw,), expires_at="cleanup",
                    description=f"{obj.printing.name} gains {kw} UEOT",
                )
        state.log.append(
            f"{len(matching)} {self.label} gain "
            f"{', '.join(sorted(self.keywords))} until end of turn"
        )

    def describe(self) -> str:
        return (
            f"{self.label} gain {', '.join(sorted(self.keywords))} until "
            f"end of turn"
        )


# ---------------------------------------------------------------------------
# Tap / untap all
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TapAllMatching(Effect):
    """"Tap all <permanents>." (Cryptic Command's "Tap all creatures
    your opponents control."). Controller-relative filters are read
    against the resolving ability's controller."""

    permanent_filter: ObjectPredicate
    label: str = "permanents"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import tap_permanent

        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            victims = [
                o for o in state.battlefield
                if not o.tapped and self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        for obj in victims:
            if obj in state.battlefield:
                tap_permanent(state, obj)
        state.log.append(f"tap all {self.label}: {len(victims)} tapped")

    def describe(self) -> str:
        return f"tap all {self.label}"


# ---------------------------------------------------------------------------
# Upkeep prison: "that player" acts once per counter on the source
# ---------------------------------------------------------------------------


def _event_player_id(state, targets) -> int | None:
    for kind, pid in targets:
        if kind == "player":
            return pid
    return None


@dataclass(frozen=True)
class EventPlayerActsPerSourceCounter(Effect):
    """"that player sacrifices a <permanent> of their choice for each
    <kind> counter on this <permanent>" (Smokestack) / "that player taps
    an untapped <permanent> they control for each <kind> counter on this
    <permanent>" (Tangle Wire).

    "That player" is the player of the triggering event (each player's
    upkeep): it is bound as the trigger goes on the stack
    (``targets_from_event``), so it is not a target. The count is the
    source's counters as the ability resolves (last-known information
    once the source is gone). The player picks every permanent first
    (``choose_sacrifice`` / ``choose_permanent_to_tap``, one per
    counter), then they are sacrificed / tapped together; with fewer
    candidates than counters, every candidate is used."""

    action: PerCounterAction
    kind: str
    permanent_filter: ObjectPredicate
    label: str = "permanent"

    def __post_init__(self) -> None:
        if self.action not in ("sacrifice", "tap"):
            raise ValueError(f"unknown action: {self.action!r}")
        object.__setattr__(self, "action", PerCounterAction(self.action))

    def targets_from_event(self, state, subjects):
        player = subjects.get("player")
        if player is None:
            return None
        return (("player", player.id),)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import counter_count

        pid = _event_player_id(state, targets)
        source = state.resolving_source
        if pid is None or source is None:
            return
        n = counter_count(source, self.kind)
        if n <= 0:
            return
        player = state.player_by_id(pid)
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = pid
            options = [
                o for o in state.battlefield
                if o.controller_id == pid
                and not (self.action == "tap" and o.tapped)
                and self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        chosen = []
        while options and len(chosen) < n:
            pick = options[0]
            if player.agent is not None and len(options) > 1:
                ids = [o.id for o in options]
                if self.action == "tap":
                    pick_id = player.agent.choose_permanent_to_tap(state, pid, ids)
                else:
                    pick_id = player.agent.choose_sacrifice(state, pid, ids)
                pick = next((o for o in options if o.id == pick_id), options[0])
            options.remove(pick)
            chosen.append(pick)
        if self.action == "tap":
            from mtgz_ir.turn import tap_permanent

            for obj in chosen:
                if obj in state.battlefield:
                    state.log.append(f"P{pid} taps {obj.printing.name}")
                    tap_permanent(state, obj)
        else:
            from mtgz_ir.turn import sacrifice

            for obj in chosen:
                if obj in state.battlefield:
                    state.log.append(f"P{pid} sacrifices {obj.printing.name}")
                    sacrifice(state, obj)

    def describe(self) -> str:
        if self.action == "tap":
            return (
                f"that player taps an untapped {self.label} they control for "
                f"each {self.kind} counter on this permanent"
            )
        return (
            f"that player sacrifices a {self.label} of their choice for each "
            f"{self.kind} counter on this permanent"
        )


@dataclass(frozen=True)
class DestroySourceUnlessPay(Effect):
    """"destroy this creature unless you pay <cost>" (the ability The
    Tabernacle at Pendrell Vale grants every creature). "You" is the
    ability's controller -- the creature's controller; the payment is
    offered mid-resolution (``choose_yes_no`` when affordable). The
    creature may regenerate."""

    cost: str

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.cards.base import parse_mana_cost
        from mtgz_ir.effects.generic.cheat_into_play import (
            pay_mana_during_resolution,
        )
        from mtgz_ir.effects.generic.targeted_removal import destroy_permanent

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        if pay_mana_during_resolution(
            state, controller_id, parse_mana_cost(self.cost),
            f"Pay {self.cost} to keep {source.printing.name}?",
        ):
            state.log.append(
                f"P{controller_id} pays {self.cost} for {source.printing.name}"
            )
            return
        destroy_permanent(state, source)

    def describe(self) -> str:
        return f"destroy this creature unless you pay {self.cost}"


# ---------------------------------------------------------------------------
# Equipment
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PumpEquippedCreatureUEOT(Effect):
    """"Equipped creature gets +P/+T until end of turn." (Umezawa's
    Jitte's first mode): the creature the source Equipment is attached
    to as the ability resolves; nothing if it's unattached."""

    power_bonus: int = 0
    toughness_bonus: int = 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None or source.attached_to is None:
            state.log.append("no equipped creature -- pump does nothing")
            return
        host = next(
            (o for o in state.battlefield if o.id == source.attached_to), None,
        )
        if host is None:
            return
        state.grant_pt_pump_ueot(
            source_id=source.id, target_id=host.id,
            power=self.power_bonus, toughness=self.toughness_bonus,
            description=(
                f"{source.printing.name}: equipped creature gets "
                f"{self.power_bonus:+d}/{self.toughness_bonus:+d} UEOT"
            ),
        )
        state.log.append(
            f"{host.printing.name} gets {self.power_bonus:+d}/"
            f"{self.toughness_bonus:+d} until end of turn"
        )

    def describe(self) -> str:
        return (
            f"equipped creature gets {self.power_bonus:+d}/"
            f"{self.toughness_bonus:+d} until end of turn"
        )


@dataclass(frozen=True)
class EventSubjectDealsDamageToEachOpponent(Effect):
    """"it deals N damage to each opponent" where "it" is the creature
    the triggering event is about (Hawkeye's Bow: "Whenever equipped
    creature becomes tapped, it deals 1 damage to each opponent." -- the
    equipped creature deals the damage, not the Equipment). The creature
    is bound as the trigger goes on the stack (``event_subject_id``)
    and found wherever it is at resolution (last-known information);
    "each opponent" is relative to the ability's controller."""

    amount: int = 1
    subject_slot: EventSubject = EventSubject.OBJ

    def __post_init__(self) -> None:
        object.__setattr__(self, "subject_slot", EventSubject(self.subject_slot))

    def event_subject_id(self, subjects) -> int:
        obj = subjects.get(self.subject_slot)
        return getattr(obj, "id", -1) if obj is not None else -1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.turn import _find_object_by_id

        creature = _find_object_by_id(state, state.resolving_event_subject_id)
        if creature is None:
            return
        prior = state.resolving_source
        state.resolving_source = creature
        try:
            for p in state.players:
                if p.id == controller_id or p.has_lost:
                    continue
                _apply_damage(state, ("player", p.id), self.amount)
        finally:
            state.resolving_source = prior

    def describe(self) -> str:
        return f"it deals {self.amount} damage to each opponent"


# ---------------------------------------------------------------------------
# Conditions over the controller's graveyard (static P/T)
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class CardsInYourGraveyard(Condition):
    """"there are N or more <type> cards in your graveyard" (Elvish
    Reclaimer: three land cards) and, as a variable amount, "for each
    <type> card in your graveyard" (Knight of the Reliquary).
    ``matches`` is "at least ``at_least``"; ``multiplier`` is the count."""

    card_filter: object
    at_least: int = 1

    def _count(self, state, viewer_id) -> int:
        p = state.player_by_id(viewer_id)
        return sum(1 for c in p.graveyard if self.card_filter.matches_card(state, c))

    def matches(self, state, viewer_id, chosen_targets=None):
        return self._count(state, viewer_id) >= self.at_least

    def multiplier(self, state, viewer_id, chosen_targets=None):
        return self._count(state, viewer_id)


@dataclass(frozen=True)
class CardTypesInYourGraveyardPT(PTFormula):
    """``dynamic_pt_formula`` for "<This>'s power is equal to the number
    of card types among cards in your graveyard and its toughness is
    equal to that number plus K." (Nethergoyf): "your" is the creature's
    controller; card types counted by the shared
    ``effects.card_types`` helper (CR 205.2a)."""

    plus: int = 1

    def base_pt(self, state, obj):
        from mtgz_ir.effects.card_types import count_card_types

        n = count_card_types(state.player_by_id(obj.controller_id).graveyard)
        return n, n + self.plus


# ---------------------------------------------------------------------------
# One-off resolutions
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PumpEachYouControlByCountUEOT(Effect):
    """"<creatures> you control gain <keywords> and get +X/+X until end
    of turn, where X is the number of <permanents> you control"
    (Craterhoof Behemoth). X is counted and the affected set fixed as the
    ability resolves (CR 608.2h / 611.2c)."""

    permanent_filter: ObjectPredicate
    count_filter: ObjectPredicate
    keywords: frozenset[KeywordSpelling] = frozenset()
    label: str = "creatures you control"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            x = sum(
                1 for o in state.battlefield
                if o.controller_id == controller_id
                and self.count_filter.test(o, state)
            )
            affected = [
                o for o in state.battlefield
                if o.controller_id == controller_id
                and self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        src = state.resolving_source or state.resolving_spell
        sid = src.id if src is not None else -1
        for obj in affected:
            state.grant_pt_pump_ueot(
                source_id=sid, target_id=obj.id, power=x, toughness=x,
                description=f"{obj.printing.name} gets +{x}/+{x} UEOT",
            )
            for kw in sorted(self.keywords):
                state.add_continuous_effect(
                    source_id=sid, layer=6, kind="grant_keyword",
                    target_id=obj.id, params=(kw,), expires_at="cleanup",
                    description=f"{obj.printing.name} gains {kw} UEOT",
                )
        state.log.append(
            f"{len(affected)} {self.label} get +{x}/+{x}"
            + (f" and gain {', '.join(sorted(self.keywords))}" if self.keywords else "")
        )

    def describe(self) -> str:
        kw = f"gain {', '.join(sorted(self.keywords))} and " if self.keywords else ""
        return (
            f"{self.label} {kw}get +X/+X until end of turn, where X is the "
            f"number of creatures you control"
        )


@dataclass(frozen=True)
class IfResolvingInYourMainPhase(Effect):
    """"<effect>. If you cast this spell during your main phase,
    <stronger effect> instead." (Might of Old Krosa). A step never ends
    with an object on the stack (CR 500.2), so a spell cast during its
    controller's main phase resolves during that same main phase and the
    cast-time condition is read on resolution. Both effects share the
    spell's one target."""

    main_phase: Effect
    otherwise: Effect

    @property
    def targets(self):  # type: ignore[override]
        return self.otherwise.targets

    def requires_target(self) -> bool:
        return self.otherwise.requires_target()

    def legal_targets(self, state, controller_id):
        return self.otherwise.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.otherwise.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import MAIN_STEPS

        in_main = (
            state.step in MAIN_STEPS
            and state.active_player().id == controller_id
        )
        (self.main_phase if in_main else self.otherwise).resolve(
            state, controller_id, targets,
        )

    def describe(self) -> str:
        return (
            f"{self.otherwise.describe()}; if cast during your main phase, "
            f"{self.main_phase.describe()} instead"
        )


@dataclass(frozen=True)
class PutCreatureWithMVEqualToCountersFromHand(Effect):
    """"You may put a creature card with mana value equal to the number
    of <kind> counters on this <permanent> from your hand onto the
    battlefield." (Aether Vial). The card is picked with
    ``choose_card_from_hand`` (``None`` declines); the count is the
    source's counters on resolution (last-known information)."""

    kind: str = "charge"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.value_predicates import mana_value_of
        from mtgz_ir.game import counter_count

        source = state.resolving_source
        if source is None:
            return
        n = counter_count(source, self.kind)
        p = state.player_by_id(controller_id)
        options = [
            c for c in p.hand
            if c.printing.is_creature and mana_value_of(c) == n
        ]
        if not options or p.agent is None:
            return
        picked = p.agent.choose_card_from_hand(
            state, controller_id, [c.id for c in options],
        )
        card = next((c for c in options if c.id == picked), None)
        if card is None:
            state.log.append(f"P{controller_id} puts nothing onto the battlefield")
            return
        p.hand.remove(card)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the battlefield"
        )
        put_into_battlefield(state, card, controller_id)

    def describe(self) -> str:
        return (
            f"you may put a creature card with mana value equal to the "
            f"number of {self.kind} counters on this permanent from your "
            f"hand onto the battlefield"
        )


@dataclass(frozen=True)
class DestroyTargetsControllersGetTokens(Effect):
    """"[You may ]destroy up to N target <permanents>. For each
    permanent put into a graveyard this way, its controller creates
    <token>." (Terastodon). Each still-legal target is destroyed; for
    each one that actually went to a graveyard (not indestructible, not
    regenerated) its controller -- as it last existed -- creates the
    token."""

    target_filter: ObjectPredicate
    token_printing: object
    count_min: int = 0
    count_max: int = 1
    tokens_each: int = 1
    targets: tuple = ()

    def __post_init__(self) -> None:
        from mtgz_ir.effects.targets import TargetSpec

        object.__setattr__(self, "targets", (TargetSpec(
            self.target_filter, count_min=self.count_min,
            count_max=self.count_max,
        ),))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targeted_removal import destroy_permanent
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )
        from mtgz_ir.effects.generic.tokens import CreateToken

        slot = self.targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = []
            for chosen in targets:
                if _slot_target_still_legal(state, slot, chosen):
                    obj = next(
                        (o for o in state.battlefield if o.id == chosen[1]), None,
                    )
                    if obj is not None:
                        legal.append(obj)
                else:
                    state.log.append(
                        _fizzle_message(state, chosen, action="destroy")
                    )
        finally:
            state.resolving_controller_id = prior
        for obj in legal:
            owner_side = obj.controller_id
            if destroy_permanent(state, obj):
                CreateToken(
                    token_printing=self.token_printing, count=self.tokens_each,
                ).resolve(state, owner_side)

    def describe(self) -> str:
        return (
            f"destroy up to {self.count_max} target permanents; for each put "
            f"into a graveyard this way, its controller creates "
            f"{self.tokens_each} {self.token_printing.name} token(s)"
        )


@dataclass(frozen=True)
class ExileEachUntilSourceLeaves(Effect):
    """"exile each <permanent> until this <permanent> leaves the
    battlefield" (Temporary Lockdown). Every matching permanent is
    exiled with the Oblivion Ring bookkeeping (``exiled_by_source``) and
    returns when the source leaves; nothing is exiled if the source left
    before this resolved (CR 610.3c)."""

    permanent_filter: ObjectPredicate
    label: str = "permanents"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            victims = [
                o for o in state.battlefield
                if self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        for obj in victims:
            if obj not in state.battlefield:
                continue
            obj.exiled_by_source = source.id
            state.log.append(f"{obj.printing.name} is exiled")
            move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return f"exile each {self.label} until this permanent leaves the battlefield"


@dataclass(frozen=True)
class TapOrUntapAllOfChosenType(Effect):
    """"Choose <type>, <type>, or <type>. Tap all untapped permanents of
    the chosen type target player controls, or untap all tapped
    permanents of that type that player controls." (Turnabout). The type
    and the tap / untap choice are made as the spell resolves (two
    ``choose_mode`` calls)."""

    types: tuple[str, ...] = ("Artifact", "Creature", "Land")

    def requires_target(self) -> bool:
        return True

    def target_count_range(self):
        return (1, 1)

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import has_card_type
        from mtgz_ir.turn import tap_permanent

        pid = _event_player_id(state, targets)
        if pid is None or state.player_by_id(pid).has_lost:
            return
        agent = state.player_by_id(controller_id).agent
        t_idx, verb = 0, 0
        if agent is not None:
            t_idx = agent.choose_mode(
                state, controller_id, [t.lower() for t in self.types],
            )
            verb = agent.choose_mode(state, controller_id, ["tap", "untap"])
        t_idx = max(0, min(t_idx, len(self.types) - 1))
        card_type = self.types[t_idx]

        def has_type(o) -> bool:
            return has_card_type(state, o, card_type)

        mine = [o for o in state.battlefield if o.controller_id == pid and has_type(o)]
        if verb == 1:
            for o in mine:
                o.tapped = False
            state.log.append(f"untap all {card_type.lower()}s P{pid} controls")
        else:
            for o in mine:
                if o in state.battlefield:
                    tap_permanent(state, o)
            state.log.append(f"tap all {card_type.lower()}s P{pid} controls")

    def describe(self) -> str:
        return (
            "choose artifact, creature, or land; tap all untapped or untap "
            "all tapped permanents of that type target player controls"
        )


@dataclass(frozen=True)
class SacrificeSourceUnlessDiscard(Effect):
    """"sacrifice this creature unless you discard a card[. When you
    discard a card this way, <destroy|exile> target <permanent> with
    mana value less than or equal to the mana value of the discarded
    card]" (Argentum Masticore).

    The controller decides as it resolves (``choose_yes_no``, then the
    card with ``choose_discard``); with an empty hand, or declining, the
    source is sacrificed. A discard -- even with the source already gone
    -- triggers the reflexive ability (CR 603.12), which goes on the
    stack as a new triggered ability whose target is chosen then, among
    permanents matching ``reflexive_filter`` with mana value at most the
    discarded card's."""

    reflexive_action: "RemovalAction | None" = None
    reflexive_filter: "ObjectPredicate | None" = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "reflexive_action", coerce(RemovalAction, self.reflexive_action or None))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import push_triggered_ability
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.effects.generic.targeted_removal import RemoveTargetPermanents
        from mtgz_ir.effects.value_predicates import (
            HasManaValueAtMost,
            mana_value_of,
        )
        from mtgz_ir.turn import sacrifice

        source = state.resolving_source
        p = state.player_by_id(controller_id)
        name = source.printing.name if source is not None else "it"
        discarded = None
        if p.hand and (
            p.agent is None
            or p.agent.choose_yes_no(
                state, controller_id, f"Discard a card to keep {name}?",
            )
        ):
            pick_id = p.hand[0].id
            if p.agent is not None:
                pick_id = p.agent.choose_discard(
                    state, controller_id, [c.id for c in p.hand],
                )
            card = next((c for c in p.hand if c.id == pick_id), p.hand[0])
            discard_card(state, p, card)
            if card in p.graveyard:
                discarded = card
        if discarded is None:
            if source is not None and source in state.battlefield:
                state.log.append(f"P{controller_id} sacrifices {name}")
                sacrifice(state, source)
            return
        if not self.reflexive_action or self.reflexive_filter is None:
            return
        mv = mana_value_of(discarded)
        reflexive = TriggeredAbility(effect=RemoveTargetPermanents(
            action=self.reflexive_action,
            target_filter=self.reflexive_filter & HasManaValueAtMost(mv),
        ))
        push_triggered_ability(
            state, source=source, ability=reflexive, ability_index=-1,
            controller_id=controller_id, label="reflexive",
        )

    def describe(self) -> str:
        text = "sacrifice this creature unless you discard a card"
        if self.reflexive_action:
            text += (
                f"; when you do, {self.reflexive_action} target permanent "
                f"with mana value at most the discarded card's"
            )
        return text


@dataclass(frozen=True)
class ReturnAnyNumberDrawPerOwned(Effect):
    """"Return any number of target <permanents> you control to their
    owners' hands. Draw a card for each card returned to your hand this
    way." (Paradoxical Outcome). Every still-legal target is returned;
    the draw counts only cards that went to the caster's own hand (not a
    permanent another player owns; tokens aren't cards)."""

    target_filter: ObjectPredicate
    targets: tuple = ()

    def __post_init__(self) -> None:
        from mtgz_ir.effects.targets import TargetSpec

        # "Any number": a bound no game reaches (the target loop stops
        # when the candidates run out or the caster declines).
        object.__setattr__(self, "targets", (TargetSpec(
            self.target_filter, count_min=0, count_max=ANY_NUMBER_OF_TARGETS,
        ),))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _bounce_to_hand,
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import draw_n_cards

        slot = self.targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = [
                next(o for o in state.battlefield if o.id == t[1])
                for t in targets
                if _slot_target_still_legal(state, slot, t)
            ]
        finally:
            state.resolving_controller_id = prior
        returned_to_me = 0
        for obj in legal:
            if obj not in state.battlefield:
                continue
            mine = obj.owner_id == controller_id and not obj.printing.is_token
            _bounce_to_hand(state, obj)
            if mine and obj in state.player_by_id(controller_id).hand:
                returned_to_me += 1
        if returned_to_me:
            draw_n_cards(state, controller_id, returned_to_me)

    def describe(self) -> str:
        return (
            "return any number of target permanents you control to their "
            "owners' hands; draw a card for each returned to your hand"
        )


# A target count no game reaches ("any number of target ...").
ANY_NUMBER_OF_TARGETS = 10_000


@dataclass(frozen=True)
class BecomeColoredSubtypeBasePTUEOT(Effect):
    """"Until end of turn, target <creature> becomes a <color> <Subtype>
    with base power and toughness P/T." (Scale Up; its overloaded "each"
    version uses ``each_filter``). Cleanup-expiring registry entries:
    layer 4 ``set_creature_subtypes`` (CR 205.1b: the new creature types
    replace the old), layer 5 ``set_colors`` and layer 7b
    ``set_base_pt``, all with this resolution's timestamps."""

    colors: frozenset = frozenset()
    creature_subtypes: tuple = ()
    base_pt: tuple = (0, 0)
    each_filter: "ObjectPredicate | None" = None
    targets: tuple = ()

    def _apply(self, state, obj, source_id: int) -> None:
        name = obj.printing.name
        state.add_continuous_effect(
            source_id=source_id, layer=4, kind="set_creature_subtypes",
            target_id=obj.id, params=tuple(self.creature_subtypes),
            expires_at="cleanup",
            description=f"{name} becomes a {' '.join(self.creature_subtypes)}",
        )
        if self.colors:
            state.add_continuous_effect(
                source_id=source_id, layer=5, kind="set_colors",
                target_id=obj.id, params=(frozenset(self.colors),),
                expires_at="cleanup",
                description=f"{name} becomes {'/'.join(sorted(self.colors))}",
            )
        p, t = self.base_pt
        state.add_continuous_effect(
            source_id=source_id, layer=7, sublayer=1, kind="set_base_pt",
            target_id=obj.id, params=(p, t), expires_at="cleanup",
            description=f"{name} base {p}/{t} UEOT",
        )
        state.log.append(
            f"{name} becomes a {' '.join(self.creature_subtypes)} with base "
            f"P/T {p}/{t} until end of turn"
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        src = state.resolving_source or state.resolving_spell
        sid = src.id if src is not None else -1
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            if self.each_filter is not None:
                affected = [
                    o for o in state.battlefield
                    if self.each_filter.test(o, state)
                ]
            elif targets and _slot_target_still_legal(
                state, self.targets[0], targets[0],
            ):
                affected = [
                    o for o in state.battlefield if o.id == targets[0][1]
                ]
            else:
                affected = []
        finally:
            state.resolving_controller_id = prior
        for obj in affected:
            self._apply(state, obj, sid)

    def describe(self) -> str:
        who = "each matching creature" if self.each_filter is not None else (
            "target creature"
        )
        return (
            f"until end of turn, {who} becomes a "
            f"{' '.join(self.creature_subtypes)} with base power and "
            f"toughness {self.base_pt[0]}/{self.base_pt[1]}"
        )


@dataclass(frozen=True)
class PumpPerTypeCountUEOT(Effect):
    """"Until end of turn, target <creature> gets +1/+1 for each
    supertype, card type, and subtype it has." (Embiggen). Counted as
    the spell resolves (CR 608.2h): supertypes as printed (a token isn't
    a supertype), card types including a creature type an effect added,
    subtypes through the layered ``effective_subtypes``."""

    per_power: int = 1
    per_toughness: int = 1
    targets: tuple = ()

    @staticmethod
    def type_count(state, obj) -> int:
        from mtgz_ir.game import effective_card_types, effective_subtypes

        supertypes = [s for s in obj.printing.supertypes if s != "Token"]
        card_types = set(effective_card_types(state, obj))
        return (
            len(supertypes) + len(card_types)
            + len(effective_subtypes(state, obj))
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        if not targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            ok = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not ok:
            return
        obj = next(o for o in state.battlefield if o.id == targets[0][1])
        n = self.type_count(state, obj)
        src = state.resolving_source or state.resolving_spell
        state.grant_pt_pump_ueot(
            source_id=src.id if src is not None else -1, target_id=obj.id,
            power=n * self.per_power, toughness=n * self.per_toughness,
            description=f"{obj.printing.name} +{n}/+{n} (types) UEOT",
        )
        state.log.append(f"{obj.printing.name} gets +{n}/+{n} until end of turn")

    def describe(self) -> str:
        return (
            "target creature gets +1/+1 for each supertype, card type, and "
            "subtype it has until end of turn"
        )


# Registry kind for "can't be the target of spells or abilities your
# opponents control this turn" (params: the protecting player's id),
# read by ``roto_board_statics.untargetable_by``.
UNTARGETABLE_BY_OPPONENTS_OF = "untargetable_by_opponents_of"


@dataclass(frozen=True)
class UntargetableByOpponentsThisTurn(Effect):
    """"Target <creature> can't be the target of spells or abilities your
    opponents control this turn.[ If this spell was kicked, that creature
    gets +P/+T until end of turn.]" (Vines of Vastwood). "Your opponents"
    are the caster's -- unlike hexproof, which protects from the
    creature's controller's opponents -- so the restriction carries the
    caster's id. The kicked pump applies to the same target."""

    kicked_power: int = 0
    kicked_toughness: int = 0
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        if not targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            ok = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not ok:
            return
        obj = next(o for o in state.battlefield if o.id == targets[0][1])
        src = state.resolving_source or state.resolving_spell
        sid = src.id if src is not None else -1
        state.add_continuous_effect(
            source_id=sid, layer=6, kind=UNTARGETABLE_BY_OPPONENTS_OF,
            target_id=obj.id, params=(controller_id,), expires_at="cleanup",
            description=(
                f"{obj.printing.name} can't be targeted by P{controller_id}'s "
                f"opponents this turn"
            ),
        )
        state.log.append(
            f"{obj.printing.name} can't be the target of spells or abilities "
            f"P{controller_id}'s opponents control this turn"
        )
        if state.resolving_kicked and (self.kicked_power or self.kicked_toughness):
            state.grant_pt_pump_ueot(
                source_id=sid, target_id=obj.id, power=self.kicked_power,
                toughness=self.kicked_toughness,
                description=f"{obj.printing.name} kicked pump UEOT",
            )

    def describe(self) -> str:
        text = (
            "target creature can't be the target of spells or abilities "
            "your opponents control this turn"
        )
        if self.kicked_power or self.kicked_toughness:
            text += (
                f"; if kicked, it gets +{self.kicked_power}/"
                f"+{self.kicked_toughness} until end of turn"
            )
        return text


@dataclass(frozen=True)
class PowerPumpThenDestroyIfAttacked(Effect):
    """"Target creature gains <keywords> and gets +X/+0 until end of
    turn, where X is its power. At the beginning of the next end step,
    destroy that creature if it attacked this turn." (Berserk). X is the
    creature's power as the spell resolves; the delayed trigger is the
    engine's ``delayed_triggers`` entry (fires at the next end step,
    whoever's it is) with the creature as its source, so it does nothing
    if the creature is gone."""

    keywords: frozenset[KeywordSpelling] = frozenset()
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.game import effective_power

        if not targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            ok = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not ok:
            return
        obj = next(o for o in state.battlefield if o.id == targets[0][1])
        src = state.resolving_source or state.resolving_spell
        sid = src.id if src is not None else -1
        x = effective_power(state, obj)
        state.grant_pt_pump_ueot(
            source_id=sid, target_id=obj.id, power=x, toughness=0,
            description=f"{obj.printing.name} +{x}/+0 UEOT",
        )
        for kw in sorted(self.keywords):
            state.add_continuous_effect(
                source_id=sid, layer=6, kind="grant_keyword",
                target_id=obj.id, params=(kw,), expires_at="cleanup",
                description=f"{obj.printing.name} gains {kw} UEOT",
            )
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number,
            "controller_id": controller_id,
            "source_id": obj.id,
            "any_player_end_step": True,
            "effect": DestroySourceIfAttackedThisTurn(),
        })
        state.log.append(
            f"{obj.printing.name} gets +{x}/+0 until end of turn; destroyed "
            f"at the next end step if it attacked"
        )

    def describe(self) -> str:
        kw = ", ".join(sorted(self.keywords))
        return (
            f"target creature gains {kw} and gets +X/+0 until end of turn, "
            f"where X is its power; destroy it at the next end step if it "
            f"attacked this turn"
        )


@dataclass(frozen=True)
class DestroySourceIfAttackedThisTurn(Effect):
    """The delayed half of Berserk: "destroy that creature if it attacked
    this turn" (the creature is the delayed trigger's source)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targeted_removal import destroy_permanent

        obj = state.resolving_source
        if obj is None or obj not in state.battlefield:
            return
        attacked = any(
            o.id == obj.id for p in state.players for o in p.attacked_this_turn
        )
        if not attacked:
            state.log.append(f"{obj.printing.name} didn't attack -- survives")
            return
        destroy_permanent(state, obj)

    def describe(self) -> str:
        return "destroy that creature if it attacked this turn"


@dataclass(frozen=True)
class ByResolutionCountThisTurn(Effect):
    """"<first> if this is the first time this ability has resolved this
    turn. If it's the second time, <second>. If it's the third time,
    <third>." (Omnath, Locus of Creation). The count lives on the source
    object (``GameObject.ability_resolutions``), restarting each turn and
    for a new object; a fourth resolution does nothing."""

    by_count: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_source
        if src is None:
            return
        turn, stamp, count = src.ability_resolutions
        if (turn, stamp) != (state.turn_number, src.etb_timestamp):
            count = 0
        count += 1
        src.ability_resolutions = (state.turn_number, src.etb_timestamp, count)
        state.log.append(f"{src.printing.name}'s ability resolves (#{count} this turn)")
        if count <= len(self.by_count):
            self.by_count[count - 1].resolve(state, controller_id, targets)

    def describe(self) -> str:
        names = ("first", "second", "third", "fourth")
        return "; ".join(
            f"{names[i]} time this turn: {e.describe()}"
            for i, e in enumerate(self.by_count)
        )


@dataclass(frozen=True)
class DealDamageToEachOpponentAndTheirPlaneswalkers(Effect):
    """"<This> deals N damage to each opponent and each planeswalker you
    don't control." (Omnath's third landfall)."""

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage

        victims: list = [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]
        victims += [
            ("planeswalker", o.id) for o in state.battlefield
            if o.printing.is_planeswalker and o.controller_id != controller_id
        ]
        for v in victims:
            _apply_damage(state, v, self.amount)

    def describe(self) -> str:
        return (
            f"deals {self.amount} damage to each opponent and each "
            f"planeswalker you don't control"
        )


@dataclass(frozen=True)
class DestroyLeastPowerCreature(Effect):
    """"destroy the creature with the least power. It can't be
    regenerated. If two or more creatures are tied for least power, you
    choose one of them." (Drop of Honey). Ties are broken by the
    controller (``choose_target`` over the tied creatures)."""

    can_be_regenerated: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targeted_removal import destroy_permanent
        from mtgz_ir.game import effective_power, is_creature_now

        creatures = [o for o in state.battlefield if is_creature_now(o)]
        if not creatures:
            return
        least = min(effective_power(state, o) for o in creatures)
        tied = [o for o in creatures if effective_power(state, o) == least]
        pick = tied[0]
        agent = state.player_by_id(controller_id).agent
        if agent is not None and len(tied) > 1:
            choice = agent.choose_target(
                state, controller_id, [("creature", o.id) for o in tied],
            )
            pick = next((o for o in tied if choice == ("creature", o.id)), pick)
        destroy_permanent(state, pick, can_be_regenerated=self.can_be_regenerated)

    def describe(self) -> str:
        tail = "" if self.can_be_regenerated else "; it can't be regenerated"
        return f"destroy the creature with the least power{tail}"


@dataclass(frozen=True)
class HasManaValueAtLeast(ObjectPredicate):
    """"with mana value N or greater" (Zoyowa's Justice)."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.effects.value_predicates import mana_value_of

        return mana_value_of(obj) >= self.n


def discover(state: "GameState", player_id: int, x: int) -> None:
    """Discover X (CR 701.57a): exile cards from the top of your library
    until you exile a nonland card with mana value X or less. You may
    cast it without paying its mana cost; if you don't, put it into your
    hand. Put the rest on the bottom of your library in a random order
    (``state.rng``, like cascade)."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        cast_card_from_exile_for_free,
        exile_card,
    )
    from mtgz_ir.game import Zone

    p = state.player_by_id(player_id)
    exiled: list = []
    hit = None
    while p.library:
        card = p.library.pop(0)
        exile_card(state, card)
        exiled.append(card)
        if not card.printing.is_land and card.printing.mana_cost.total <= x:
            hit = card
            break
    state.log.append(
        f"P{player_id} discovers {x}: "
        f"{hit.printing.name if hit else 'nothing'} ({len(exiled)} exiled)"
    )
    cast = False
    if hit is not None:
        wants = p.agent is None or p.agent.choose_yes_no(
            state, player_id,
            f"Discover: cast {hit.printing.name} without paying its mana cost?",
        )
        if wants:
            cast = cast_card_from_exile_for_free(state, player_id, hit)
        if not cast and hit in p.exile:
            p.exile.remove(hit)
            hit.exiled_face_down = False  # left exile (CR 400.7)
            hit.exile_play_permission = None
            hit.zone = Zone.HAND
            p.hand.append(hit)
            state.log.append(f"P{player_id} puts {hit.printing.name} into hand")
    rest = [c for c in exiled if c is not hit]
    state.rng.shuffle(rest)
    for c in rest:
        state.player_by_id(c.owner_id).exile.remove(c)
        c.exiled_face_down = False  # left exile (CR 400.7)
        c.exile_play_permission = None
        c.zone = Zone.LIBRARY
        p.library.append(c)


@dataclass(frozen=True)
class OwnerShufflesTargetThenDiscovers(Effect):
    """"The owner of target <permanent> shuffles it into their library.
    Then that player discovers X, where X is its mana value." (Zoyowa's
    Justice). X is the permanent's mana value as it last existed."""

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.effects.value_predicates import mana_value_of
        from mtgz_ir.game import shuffle_library
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            ok = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not ok:
            return
        obj = next(o for o in state.battlefield if o.id == targets[0][1])
        x = mana_value_of(obj)
        owner = obj.owner_id
        state.log.append(f"P{owner} shuffles {obj.printing.name} into their library")
        move_off_battlefield(state, obj, destination="library")
        shuffle_library(state, owner)
        discover(state, owner, x)

    def describe(self) -> str:
        return (
            "the owner of target permanent shuffles it into their library, "
            "then discovers X (its mana value)"
        )


# ---------------------------------------------------------------------------
# Role tokens (CR 111.10j: Aura tokens with the Role subtype)
# ---------------------------------------------------------------------------


def _role_printing(name: str, power: int, toughness: int, keywords=()):
    from dataclasses import replace

    from mtgz_ir.cards.base import make_aura

    aura = make_aura(
        f"{name} Role", "", power_bonus=power, toughness_bonus=toughness,
        granted_keywords=frozenset(keywords),
    )
    return replace(
        aura, type_line="Token Enchantment — Aura Role",
        supertypes=("Token",), subtypes=("Aura", "Role"),
    )


# Role name -> token printing. "Monster" (Monstrous Rage): "Enchanted
# creature gets +1/+1 and has trample." Adding a Role is one entry.
ROLE_TOKENS: dict = {
    "Monster": _role_printing("Monster", 1, 1, ("trample",)),
}


@dataclass(frozen=True)
class PumpThenCreateRoleAttached(Effect):
    """"Target creature gets +P/+T until end of turn. Create a <Name>
    Role token attached to it." (Monstrous Rage). The Role enters
    attached to the (still legal) target; Role uniqueness (CR 704.5y) is
    the state-based check in ``roto_board_statics``."""

    role: str = "Monster"
    power_bonus: int = 0
    toughness_bonus: int = 0
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.game import Zone

        if not targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            ok = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not ok:
            return
        obj = next(o for o in state.battlefield if o.id == targets[0][1])
        src = state.resolving_source or state.resolving_spell
        state.grant_pt_pump_ueot(
            source_id=src.id if src is not None else -1, target_id=obj.id,
            power=self.power_bonus, toughness=self.toughness_bonus,
            description=f"{obj.printing.name} pump UEOT",
        )
        token = state.new_object(
            ROLE_TOKENS[self.role], owner_id=controller_id, zone=Zone.BATTLEFIELD,
        )
        token.attached_to = obj.id
        put_into_battlefield(state, token, controller_id)
        token.attached_to = obj.id
        state.log.append(
            f"P{controller_id} creates a {self.role} Role attached to "
            f"{obj.printing.name}"
        )

    def describe(self) -> str:
        return (
            f"target creature gets +{self.power_bonus}/+{self.toughness_bonus} "
            f"until end of turn; create a {self.role} Role token attached to it"
        )


# ---------------------------------------------------------------------------
# Veil of Summer
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DrawIfOpponentCastColorThisTurn(Effect):
    """"Draw N cards if an opponent has cast a <color> or <color> spell
    this turn." (Veil of Summer; the spells' colors as they were cast,
    recorded by ``roto_board_statics.note_spell_cast``)."""

    colors: frozenset = frozenset()
    amount: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.roto_board_statics import spell_colors_cast_this_turn
        from mtgz_ir.turn import draw_n_cards

        if any(
            spell_colors_cast_this_turn(state, p) & self.colors
            for p in state.players if p.id != controller_id
        ):
            draw_n_cards(state, controller_id, self.amount)
        else:
            state.log.append("no opponent cast a spell of those colors this turn")

    def describe(self) -> str:
        return (
            f"draw {self.amount} if an opponent has cast a "
            f"{'/'.join(sorted(self.colors))} spell this turn"
        )


@dataclass(frozen=True)
class YourSpellsCantBeCounteredThisTurn(Effect):
    """"Spells you control can't be countered this turn." -- spells
    already on the stack and those cast later this turn."""

    def resolve(self, state, controller_id, targets=()):
        state.player_by_id(controller_id).spells_uncounterable_turn = (
            state.turn_number
        )
        state.log.append(
            f"P{controller_id}'s spells can't be countered this turn"
        )

    def describe(self) -> str:
        return "spells you control can't be countered this turn"


@dataclass(frozen=True)
class YouAndPermanentsGainHexproofFromUEOT(Effect):
    """"You and permanents you control gain hexproof from <quality> and
    from <quality> until end of turn." (Veil of Summer): the player
    (``Player.hexproof_from_this_turn``) and every permanent they
    control as it resolves (layer-6 ``hexproof from <quality>`` keyword
    grants expiring at cleanup)."""

    qualities: frozenset = frozenset()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.keyword_rules import HEXPROOF_FROM_PREFIX

        p = state.player_by_id(controller_id)
        turn, have = p.hexproof_from_this_turn
        if turn != state.turn_number:
            have = frozenset()
        p.hexproof_from_this_turn = (state.turn_number, have | self.qualities)
        src = state.resolving_source or state.resolving_spell
        sid = src.id if src is not None else -1
        for obj in list(state.battlefield):
            if obj.controller_id != controller_id:
                continue
            for q in sorted(self.qualities):
                state.add_continuous_effect(
                    source_id=sid, layer=6, kind="grant_keyword",
                    target_id=obj.id, params=(HEXPROOF_FROM_PREFIX + q,),
                    expires_at="cleanup",
                    description=f"{obj.printing.name} gains hexproof from {q}",
                )
        state.log.append(
            f"P{controller_id} and their permanents gain hexproof from "
            f"{', '.join(sorted(self.qualities))} until end of turn"
        )

    def describe(self) -> str:
        return (
            "you and permanents you control gain hexproof from "
            f"{' and from '.join(sorted(self.qualities))} until end of turn"
        )


@dataclass(frozen=True)
class NextSpellThisTurnCantBeCountered(Effect):
    """"The next spell you cast this turn can't be countered." (Mistrise
    Village): the controller's next cast this turn is marked
    uncounterable as it is cast (``roto_board_statics.note_spell_cast``);
    unused, it lapses with the turn."""

    def resolve(self, state, controller_id, targets=()):
        state.player_by_id(controller_id).next_spell_uncounterable_turn = (
            state.turn_number
        )
        state.log.append(
            f"P{controller_id}'s next spell this turn can't be countered"
        )

    def describe(self) -> str:
        return "the next spell you cast this turn can't be countered"


@dataclass(frozen=True)
class TargetOpponentDraws(Effect):
    """"Target opponent draws N cards." """

    amount: int = 1

    def requires_target(self) -> bool:
        return True

    def target_count_range(self):
        return (1, 1)

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        pid = _target_player_id(targets)
        if pid is None or pid == controller_id:
            return
        if state.player_by_id(pid).has_lost:
            return
        draw_n_cards(state, pid, self.amount)

    def describe(self) -> str:
        return f"target opponent draws {self.amount} card(s)"


@dataclass(frozen=True)
class ExileThenSourceGainsTrigger(Effect):
    """"exile up to one target <permanent>. If you do, <this> gains
    "<triggered ability>"." (Thief of Existence's cast trigger). "If
    you do" = a target was exiled. The source -- the spell, still on the
    stack when its cast trigger resolves -- gains the ability through a
    marked layer-6 registry entry that lasts for the permanent the spell
    becomes (``roto_board_statics.GRANT_MARK``); a spell no longer on
    the stack gains nothing (CR 400.7)."""

    inner: "Effect | None" = None
    ability: object = None

    @property
    def targets(self):  # type: ignore[override]
        return self.inner.targets

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def target_count_range(self):
        return self.inner.target_count_range()

    def legal_targets(self, state, controller_id):
        return self.inner.legal_targets(state, controller_id)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone
        from mtgz_ir.roto_board_statics import GRANT_MARK
        from mtgz_ir.turn import _find_object_by_id

        self.inner.resolve(state, controller_id, targets)
        exiled = any(
            (obj := _find_object_by_id(state, tid)) is not None
            and obj.zone == Zone.EXILE
            for _kind, tid in targets
        )
        source = state.resolving_source
        if not exiled or source is None:
            return
        on_stack = any(item.source is source for item in state.stack)
        if not (on_stack or source in state.battlefield):
            return
        state.add_continuous_effect(
            source_id=-1, layer=6, kind="grant_triggered_ability",
            target_id=source.id, params=(self.ability, GRANT_MARK),
            description=f"{source.printing.name} gains a triggered ability",
        )
        state.log.append(f"{source.printing.name} gains a triggered ability")

    def describe(self) -> str:
        return (
            f"{self.inner.describe()}; if you do, this gains "
            f"\"{self.ability.effect.describe()}\""
        )


# ---------------------------------------------------------------------------
# X-scaled modes (Kozilek's Command)
# ---------------------------------------------------------------------------


def _target_player_id(targets) -> "int | None":
    return next((tid for kind, tid in targets if kind == "player"), None)


class _TargetsPlayer:
    """Mixin: one target player."""

    def requires_target(self) -> bool:
        return True

    def target_count_range(self):
        return (1, 1)

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]


@dataclass(frozen=True)
class TargetPlayerCreatesXTokens(_TargetsPlayer, Effect):
    """"Target player creates X <tokens>." -- X is the spell's X."""

    token_printing: object = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.tokens import CreateToken

        pid = _target_player_id(targets)
        x = state.resolving_x_value
        if pid is None or x <= 0 or state.player_by_id(pid).has_lost:
            return
        CreateToken(token_printing=self.token_printing, count=x).resolve(state, pid)

    def describe(self) -> str:
        return f"target player creates X {self.token_printing.name} tokens"


@dataclass(frozen=True)
class TargetPlayerScriesXThenDraws(_TargetsPlayer, Effect):
    """"Target player scries X, then draws a card." -- that player makes
    the scry decisions (``Scry(bottom_choice=True)`` as them)."""

    draws: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.library_lookat import Scry
        from mtgz_ir.turn import draw_n_cards

        pid = _target_player_id(targets)
        if pid is None or state.player_by_id(pid).has_lost:
            return
        x = state.resolving_x_value
        if x > 0:
            Scry(amount=x, bottom_choice=True).resolve(state, pid)
        draw_n_cards(state, pid, self.draws)

    def describe(self) -> str:
        return "target player scries X, then draws a card"


@dataclass(frozen=True)
class ManaValueAtMostX(ObjectPredicate):
    """"with mana value X or less" where X is the resolving spell's X
    (``state.resolving_x_value``; ``ChooseModes`` sets it while targets
    are chosen)."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.effects.value_predicates import mana_value_of

        return mana_value_of(obj) <= state.resolving_x_value


@dataclass(frozen=True)
class RemoveTargetWithMVAtMostX(RemoveTargetPermanents):
    """"exile target <np> with mana value X or less": a
    ``RemoveTargetPermanents`` whose filter reads the spell's X
    (``ManaValueAtMostX``); ``targets_use_x`` makes the cast pipeline
    announce X before targets."""

    targets_use_x: ClassVar[bool] = True


@dataclass(frozen=True)
class ExileUpToXGraveyardCards(Effect):
    """"Exile up to X target cards from graveyards." -- ``count_max`` is
    the spell's X, substituted at cast time (``cast_time_shape``)."""

    count_max: int = 0
    count_from_x: bool = True

    def cast_time_shape(self, *, kicked=False, alt_cost_paid=False, x_value=None):
        if not self.count_from_x or x_value is None:
            return self
        import dataclasses as _dc

        return _dc.replace(self, count_max=x_value, count_from_x=False)

    def requires_target(self) -> bool:
        return False

    def target_count_range(self):
        return (0, self.count_max)

    def legal_targets(self, state, controller_id):
        return [
            ("card_in_graveyard", c.id)
            for p in state.players for c in p.graveyard
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        for kind, cid in targets:
            if kind != "card_in_graveyard":
                continue
            for p in state.players:
                card = next((c for c in p.graveyard if c.id == cid), None)
                if card is None:
                    continue
                p.graveyard.remove(card)
                card.zone = Zone.EXILE
                p.exile.append(card)
                state.log.append(
                    f"{card.printing.name} exiled from P{p.id}'s graveyard"
                )

    def describe(self) -> str:
        return "exile up to X target cards from graveyards"


# ---------------------------------------------------------------------------
# Cost atoms (``ActivationCost.extra_costs``)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RemoveCountersFromAmongPermanentsCost:
    """"Remove N <kind> counters from among permanents you control"
    (The Filigree Sylex: ten oil counters). Payable while the
    activator's permanents carry at least N such counters in total;
    the activator picks a permanent for each counter removed (the
    ``choose_target`` hook over ``("permanent", id)`` options)."""

    kind: str
    count: int

    def _holders(self, state, player_id: int) -> list:
        from mtgz_ir.game import counter_count

        return [
            o for o in state.battlefield
            if o.controller_id == player_id and counter_count(o, self.kind) > 0
        ]

    def payable(self, state, player_id: int, source) -> bool:
        from mtgz_ir.game import counter_count

        return sum(
            counter_count(o, self.kind) for o in self._holders(state, player_id)
        ) >= self.count

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.game import remove_counters

        agent = state.player_by_id(player_id).agent
        for _ in range(self.count):
            holders = self._holders(state, player_id)
            if not holders:
                break
            pick = holders[0]
            if agent is not None and len(holders) > 1:
                options = [("permanent", o.id) for o in holders]
                chosen = agent.choose_target(state, player_id, options)
                pick = next(
                    (o for o in holders if chosen == ("permanent", o.id)),
                    holders[0],
                )
            remove_counters(pick, self.kind, 1)
        state.log.append(
            f"P{player_id} removes {self.count} {self.kind} counter(s) from "
            f"among their permanents (cost)"
        )
        return []

    def describe(self) -> str:
        return (
            f"Remove {self.count} {self.kind} counters from among permanents "
            f"you control"
        )


@dataclass(frozen=True)
class PutMinusOneCounterOnSelfCost:
    """"Put a -1/-1 counter on this creature" as a cost (Devoted Druid).
    Always payable while the source is on the battlefield -- even when
    the counter will kill it (the ability still resolves)."""

    def payable(self, state, player_id: int, source) -> bool:
        return source is not None and source in state.battlefield

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.game import add_minus_one_counters

        add_minus_one_counters(state, source, 1)
        state.log.append(
            f"P{player_id} puts a -1/-1 counter on {source.printing.name} (cost)"
        )
        return []

    def describe(self) -> str:
        return "Put a -1/-1 counter on ~"


__all__ = [
    "ANY_NUMBER_OF_TARGETS",
    "ExileThenSourceGainsTrigger",
    "TargetOpponentDraws",
    "NextSpellThisTurnCantBeCountered",
    "DrawIfOpponentCastColorThisTurn",
    "YouAndPermanentsGainHexproofFromUEOT",
    "YourSpellsCantBeCounteredThisTurn",
    "ExileUpToXGraveyardCards",
    "ManaValueAtMostX",
    "RemoveTargetWithMVAtMostX",
    "TargetPlayerCreatesXTokens",
    "TargetPlayerScriesXThenDraws",
    "PumpThenCreateRoleAttached",
    "ROLE_TOKENS",
    "ByResolutionCountThisTurn",
    "DealDamageToEachOpponentAndTheirPlaneswalkers",
    "DestroyLeastPowerCreature",
    "HasManaValueAtLeast",
    "OwnerShufflesTargetThenDiscovers",
    "discover",
    "DestroySourceIfAttackedThisTurn",
    "PowerPumpThenDestroyIfAttacked",
    "UNTARGETABLE_BY_OPPONENTS_OF",
    "UntargetableByOpponentsThisTurn",
    "BecomeColoredSubtypeBasePTUEOT",
    "PumpPerTypeCountUEOT",
    "CardTypesInYourGraveyardPT",
    "ReturnAnyNumberDrawPerOwned",
    "SacrificeSourceUnlessDiscard",
    "CardsInYourGraveyard",
    "DestroyTargetsControllersGetTokens",
    "ExileEachUntilSourceLeaves",
    "IfResolvingInYourMainPhase",
    "PumpEachYouControlByCountUEOT",
    "PutCreatureWithMVEqualToCountersFromHand",
    "PutMinusOneCounterOnSelfCost",
    "TapOrUntapAllOfChosenType",
    "DamageEachPlayerPerControlled",
    "DealDamageToEachTarget",
    "GrantKeywordsToEachUEOT",
    "LoseKeywordUEOT",
    "DestroySourceUnlessPay",
    "EventPlayerActsPerSourceCounter",
    "EventSubjectDealsDamageToEachOpponent",
    "RemoveCountersFromAmongPermanentsCost",
    "HasManaValueEqualToSourceCounters",
    "IsLegendaryPermanent",
    "LacksSubtype",
    "PumpEquippedCreatureUEOT",
    "TapAllMatching",
]
