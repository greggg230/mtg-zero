"""Modifier-grammar primitives — :class:`ContinuousModifier` (CR 613)
and :class:`ReplacementEffect` (CR 614).

This module is the architectural pillar that replaced the per-card
boolean-flag anti-pattern and literal-line recognizers across the
parser. Decomposition: a modifier is either a
:class:`ContinuousModifier` (CR 613 layer system — P/T mod, keyword
grant/remove, type add) or a :class:`ReplacementEffect` (CR 614 —
intercept events; transforms ``MultiplyCount(×N)``, ``AddToCount(+N)``,
``AddInstances(+N)``, ``Replace(other_event)``, ``Prevent``).

Today every replacement-flag surface in the EoE pool is a native
``StaticAbility.replacement_effect`` declaration on the printing.
Reference cards: Exalted Sunborn (token-doubling MultiplyCount(×2)),
Loading Zone (counter-doubling MultiplyCount(×2)), Starfield
Vocalist (extra ETB trigger AddInstances(+1)), Quantum Riddler (draw
+1 when hand at most one AddToCount(+1)), and Nexus of Fate / Green
Sun's Zenith (Replace destination = library instead of graveyard).
:func:`_collect_replacement_effects` walks both surfaces: a
battlefield pass gates on ``source.controller_id ==
affected_player_id``, and a moving-card pass on ``card.owner_id ==
affected_player_id`` for any event whose ``subjects`` carry a
``"card"`` key. The continuous-effect side has fully landed too —
the anthem family, Mechan Shieldmate, and Illvoi Infiltrator all
flow through ``StaticAbility.continuous_effect: ContinuousModifier``;
:func:`walk_continuous_modifiers` is the consumer-facing dispatcher.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, Iterable, Mapping

from mtgz_ir.effects.vocab import AffectedPlayers, ZoneName
from mtgz_ir.keyword_vocab import KeywordSpelling

if TYPE_CHECKING:
    from mtgz_ir.effects.object_predicates import ObjectPredicate
    from mtgz_ir.effects.predicates import Event
    from mtgz_ir.game import GameObject, GameState


# ────────────────────────────────────────────────────────────────────
# EventTransform — pure payload transformations
# ────────────────────────────────────────────────────────────────────


class EventTransform(ABC):
    """A pure ``payload -> payload`` transformation describing how a
    :class:`ReplacementEffect` modifies an in-flight event.

    Concrete transforms (``MultiplyCount``, ``AddToCount``,
    ``AddInstances``, ``Replace``, ``Prevent``) compose through a
    single :meth:`apply` interface — the dispatcher hands them the
    in-flight event payload and they return the modified copy.
    """

    @abstractmethod
    def apply(self, payload: Mapping[str, Any]) -> dict[str, Any]:
        ...


@dataclass(frozen=True)
class MultiplyCount(EventTransform):
    """Multiply the ``count`` payload key by ``factor``.

    Models token-doubling (Exalted Sunborn ×2, hypothetical Anointed
    Procession-style ×3) and +1/+1-counter-doubling (Doubling Season,
    Loading Zone). The same EventTransform claims every shape that
    multiplies a count payload — the only differentiator is the
    ``factor`` constant on the per-card replacement.
    """

    factor: int

    def apply(self, payload):
        new = dict(payload)
        new["count"] = payload.get("count", 0) * self.factor
        return new


@dataclass(frozen=True)
class AddToCount(EventTransform):
    """Add ``delta`` to the ``count`` payload key.

    Models Hardened-Scales-shape "+1 to +1/+1 counters placed" and
    Quantum Riddler-shape "draw N+1 instead of N". Composes additively
    with other transforms via the dispatcher's CR 616.1 ordering.
    """

    delta: int

    def apply(self, payload):
        new = dict(payload)
        new["count"] = payload.get("count", 0) + self.delta
        return new


@dataclass(frozen=True)
class AddInstances(EventTransform):
    """Add ``delta`` instances to the ``instances`` payload key.

    Models Strionic-Resonator / Starfield-Vocalist-shape "triggered
    abilities trigger an additional time" replacements. The
    triggered-ability-firing path reads the post-replacement
    ``instances`` count to decide how many copies of the trigger to
    push onto the stack.
    """

    delta: int

    def apply(self, payload):
        new = dict(payload)
        new["instances"] = payload.get("instances", 1) + self.delta
        return new


@dataclass(frozen=True)
class Replace(EventTransform):
    """Replace the event's destination zone with ``new_destination``.

    Models Nexus-of-Fate-shape "if it would go to a graveyard, shuffle
    into the library instead". The dispatch site reads
    ``payload["destination"]`` after the transform applies and routes
    the card accordingly.
    """

    new_destination: ZoneName

    def __post_init__(self) -> None:
        object.__setattr__(self, "new_destination", ZoneName(self.new_destination))

    def apply(self, payload):
        new = dict(payload)
        new["destination"] = self.new_destination
        return new


@dataclass(frozen=True)
class Prevent(EventTransform):
    """Prevent the event entirely.

    Sets ``payload["prevented"] = True``; the dispatch site short-
    circuits when this flag is set, skipping the underlying mutation.
    Composes with the count-style transforms — a Prevent ordered first
    short-circuits subsequent transforms (the dispatcher's CR 616.1
    re-evaluation handles this).
    """

    def apply(self, payload):
        new = dict(payload)
        new["prevented"] = True
        return new


@dataclass(frozen=True)
class WinTheGameInstead(EventTransform):
    """Replace the event with "you win the game".

    Sets ``payload["win_game"] = True``; the dispatch site declares the
    affected player the winner instead of performing the event. Models
    Jace, Wielder of Mysteries' "If you would draw a card while your
    library has no cards in it, you win the game instead." (paired with
    ``Event.WOULD_DRAW_FROM_EMPTY_LIBRARY``).
    """

    def apply(self, payload):
        new = dict(payload)
        new["win_game"] = True
        return new


@dataclass(frozen=True)
class InsteadForSourceController(EventTransform):
    """Replace the event with ``effect`` performed by the replacement
    source's controller ("..., instead you create a Treasure token").

    Sets ``payload["prevented"] = True`` and appends
    ``(source_controller_id, effect)`` to ``payload["instead"]``; the
    dispatch site resolves the queued effects in place of the event.
    Needs the source, so the dispatcher calls :meth:`apply_with_source`.
    """

    effect: Any

    def apply(self, payload):  # pragma: no cover - needs a source
        raise TypeError("InsteadForSourceController needs its source")

    def apply_with_source(self, payload, source):
        new = dict(payload)
        new["prevented"] = True
        new["instead"] = tuple(payload.get("instead", ())) + (
            (source.controller_id, self.effect),
        )
        return new


def _apply_transform(transform, payload, source):
    fn = getattr(transform, "apply_with_source", None)
    if fn is not None:
        return fn(payload, source)
    return transform.apply(payload)


# ────────────────────────────────────────────────────────────────────
# EventPattern — what to match
# ────────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class EventPattern:
    """Pattern that matches a triggering event for a replacement.

    ``event`` is one of the existing :class:`Event` enum values
    (specifically the WOULD_X family for replacements — see the enum
    docs). ``subject_filters`` is an optional per-slot specification
    of ``slot_name -> ObjectPredicate`` pairs that further narrow
    which firings the replacement cares about.

    The field is stored as a ``tuple[tuple[str, ObjectPredicate], ...]``
    in canonical (slot-name-sorted) order so the dataclass's structural
    equality and ``__hash__`` work — a plain ``dict`` is unhashable, and
    making the wrapping ``CardPrinting`` hashable transitively requires
    every nested field to be hashable too. For ergonomics the
    constructor accepts a ``Mapping`` *or* an iterable of pairs and
    normalizes both to the canonical tuple form.

    Example: a counter-doubler that only doubles +1/+1 counters going
    onto creatures you control composes:

        EventPattern(
            event=Event.WOULD_PLACE_COUNTER,
            subject_filters={
                "obj": IsCreature() & ControlledByYou(),
            },
        )

    The dispatcher resolves each subject_filter against the matching
    event subject at collection time, falling through replacements
    whose filter rejects the firing.
    """

    event: "Event"
    subject_filters: "tuple[tuple[str, ObjectPredicate], ...]" = field(
        default=()
    )

    def __post_init__(self) -> None:
        # Accept ``Mapping`` or any iterable of ``(slot, predicate)``
        # pairs at construction; canonicalize to a slot-name-sorted
        # tuple of pairs so equal patterns produce equal tuples (and
        # therefore equal hashes) regardless of caller-supplied order.
        raw = self.subject_filters
        if isinstance(raw, Mapping):
            pairs: Iterable[tuple[str, "ObjectPredicate"]] = raw.items()
        else:
            pairs = raw  # already a sequence of pairs
        canonical = tuple(sorted(pairs, key=lambda p: p[0]))
        # frozen dataclass — bypass the immutability guard for the
        # one-time normalization in the constructor.
        object.__setattr__(self, "subject_filters", canonical)


# ────────────────────────────────────────────────────────────────────
# LayerPayload — what a CR 613 continuous effect does
# ────────────────────────────────────────────────────────────────────


class LayerPayload(ABC):
    """A description of what a :class:`ContinuousModifier` does to each
    matching object.

    Concrete payloads (``GrantKeyword``, ``RemoveKeyword``,
    ``PowerToughnessBonus``, ``AddSubtype``) are tagged unions — the
    layer system dispatcher picks the right code path by isinstance
    check. The dispatcher itself lands in PR D when continuous-effect
    consumers migrate.
    """


@dataclass(frozen=True)
class GrantKeyword(LayerPayload):
    """CR 613 layer 6 — grant a keyword ability.

    Models the anthem-family "creatures you control have flying" —
    the wrapping :class:`ContinuousModifier`'s ``scope_filter`` /
    surrounding ``StaticAbility.scope`` chooses who the grant lands
    on. Reserved for *real* MTG keywords that the rules layer treats
    generically (flying, lifelink, defender, trample, reach, …); the
    "this creature can't be blocked" continuous effect is structurally
    distinct and lives on :class:`CantBeBlocked` instead.
    """

    keyword: KeywordSpelling


@dataclass(frozen=True)
class RemoveKeyword(LayerPayload):
    """CR 613 layer 6 — remove a keyword ability.

    Models the Mechan-Shieldmate-shape "while an artifact entered
    this turn, this creature loses defender" — the
    ``ContinuousModifier`` payload removes the keyword from the
    matching object's effective keyword set, the same shape as the
    legacy ``attack_bypass_if_artifact_entered_this_turn`` flag
    expressed structurally.
    """

    keyword: KeywordSpelling


@dataclass(frozen=True)
class PowerToughnessBonus(LayerPayload):
    """CR 613 layer 7c — additive P/T modification.

    Models the anthem family ("creatures you control get +1/+1") and
    the equipment-buff family ("equipped creature gets +N/+M"). The
    bonus stacks across multiple sources within layer 7c — this
    payload is the per-source contribution.
    """

    power_bonus: int = 0
    toughness_bonus: int = 0


@dataclass(frozen=True)
class SetBasePowerToughness(LayerPayload):
    """CR 613 layer 7b — set base power and toughness to specific values.

    Models "target creature has base power and toughness N/N until end
    of turn" (Genemorph Imago's landfall trigger, Lignify, Clone-style
    copy-set-PT effects). Distinct from ``PowerToughnessBonus`` (layer
    7c, which is additive): this payload replaces the base P/T entirely.
    The most-recent-timestamp entry wins within sublayer 7b (CR 613.7e).

    The UEOT resolve path records this via
    ``GameState.grant_set_base_pt_ueot`` which writes a registry entry
    with ``expires_at="cleanup"``. ``_compute_effective_pt``'s layer-7b
    path reads the registry directly via ``_registry_set_base_pt``.
    """

    power: int
    toughness: int


@dataclass(frozen=True)
class CantBeBlocked(LayerPayload):
    """CR 613 layer 6 — the affected creature can't be blocked.

    "Unblockable" is NOT a keyword in real MTG — it's a continuous
    effect that imposes a gameplay-action restriction on the creature
    (CR 509.1b: "the active player declares attackers, then the
    defending player declares blockers; a creature with this effect
    can't be blocked"). Structurally distinct from
    :class:`GrantKeyword` / :class:`RemoveKeyword` which manipulate
    the creature's effective keyword set.

    Models the Illvoi-Infiltrator-shape "while [condition], this
    creature can't be blocked" via a SELF-scope ``StaticAbility``
    carrying ``ContinuousModifier(layer=6, payload=CantBeBlocked())``.
    Read by ``mtgz_ir.game.is_cant_be_blocked`` (and indirectly by
    ``combat.can_block`` / ``combat.declare_blockers``).
    """


@dataclass(frozen=True)
class CanAttackAsThoughNoDefender(LayerPayload):
    """The affected creature can attack as though it didn't have defender
    (CR 702.3b: defender only stops attacking; the creature keeps the
    keyword, so whatever counts or grants defender still sees it).
    Mechan Shieldmate's SELF-scope conditional static. Like
    :class:`CantBeBlocked`, a rule about a gameplay action carried in a
    layer-6 ``ContinuousModifier``; read by
    ``mtgz_ir.game.can_attack_as_though_no_defender``."""


@dataclass(frozen=True)
class AddSubtype(LayerPayload):
    """CR 613 layer 4 — add a permanent / creature subtype.

    Models the Magus-of-the-Moon-shape "all nonbasic lands are
    Mountains" and the Conspiracy-shape "all creatures are Slivers";
    the dispatcher walks the matching objects and adds ``subtype`` to
    their effective subtype set.
    """

    subtype: str


# ────────────────────────────────────────────────────────────────────
# ContinuousModifier / ReplacementEffect — top-level modifier types
# ────────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class ContinuousModifier:
    """A CR 613 continuous effect.

    While the source is on the battlefield (and the wrapping
    :class:`StaticAbility`'s condition is satisfied, when present),
    apply ``payload`` to every object satisfying ``subject_filter`` at
    layer ``layer``. The CR 613 dispatcher
    (:func:`walk_continuous_modifiers`) walks every active
    ``ContinuousModifier`` per object query and folds the contributions
    in layer order.

    ``subject_filter`` is an optional secondary predicate applied AFTER
    the wrapping ``StaticAbility.applies_to`` scope check. When
    ``None`` (the default for SELF-scope abilities), no additional
    filtering is performed — ``applies_to`` is the sole gate.
    """

    layer: int  # 1..7 per CR 613.1
    payload: LayerPayload
    subject_filter: "ObjectPredicate | None" = None


@dataclass(frozen=True)
class ReplacementEffect:
    """A CR 614 replacement effect.

    When an event matching ``event_pattern`` would happen, transform
    its payload via ``transform`` before the underlying mutation
    fires. The dispatcher (:func:`dispatch_replacement`) walks every
    active ``ReplacementEffect`` and applies them in CR 616.1 order
    (the affected player chooses among multiple).

    ``scope_filter`` (optional) constrains which firings the effect
    cares about — for replacements that gate on the recipient's
    controller without being expressible in the per-subject
    ``EventPattern.subject_filters`` (e.g., "you would create one or
    more tokens" gates on the controller of the creating action,
    which is a payload field, not a subject).
    """

    event_pattern: EventPattern
    transform: EventTransform
    scope_filter: "ObjectPredicate | None" = None
    # Whose events this replacement watches, relative to the source's
    # controller: ``"you"`` (the default -- "if you would draw", "tokens
    # created under your control") or ``"opponents"`` ("If an opponent
    # would draw a card ...", Hullbreacher; "a card would be put into an
    # opponent's graveyard", Dauthi Voidwalker).
    affects: AffectedPlayers = AffectedPlayers.YOU

    def __post_init__(self) -> None:
        object.__setattr__(self, "affects", AffectedPlayers(self.affects))


# ────────────────────────────────────────────────────────────────────
# dispatch_replacement — the CR 614 + CR 616.1 dispatcher
# ────────────────────────────────────────────────────────────────────


def dispatch_replacement(
    state: "GameState",
    event: "Event",
    *,
    affected_player_id: int,
    subjects: Mapping[str, Any] | None = None,
    payload: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Walk active :class:`ReplacementEffect`s, apply each matching one
    in CR 616.1 order, and return the final transformed payload.

    The ``affected_player_id`` is the player CR 616.1 names as
    choosing the application order when multiple replacements apply.
    The dispatcher consults ``state.player_by_id(affected_player_id)
    .agent.choose_replacement_order`` for the order; with 0 or 1
    candidates the prompt is skipped.

    ``subjects`` is the per-event subject mapping
    (e.g. ``{"obj": creature, "kind": "+1/+1"}`` for
    ``WOULD_PLACE_COUNTER``). ``payload`` carries the mutable fields
    every transform may modify (``count``, ``instances``,
    ``destination``, ``prevented``).

    Each replacement applies at most once per event (CR 616.1).
    After each application the dispatcher re-walks the candidate list
    to drop replacements whose ``scope_filter`` no longer matches
    (e.g., a ``Prevent`` ordered first invalidates a downstream
    multiply); the loop terminates when no further replacement
    applies.
    """
    subjects = subjects or {}
    cur_payload: dict[str, Any] = dict(payload or {})

    # Snapshot the candidate list ONCE — replacements applied during
    # this dispatch don't recursively trigger further replacement
    # collection on the SAME event. Each replacement applies at most
    # once per event.
    candidates = _collect_replacement_effects(
        state, event, subjects, affected_player_id,
    )

    if not candidates:
        return cur_payload

    if len(candidates) == 1:
        src_id, eff = candidates[0]
        return _apply_transform(
            eff.transform, cur_payload, _source_by_id(state, src_id),
        )

    # CR 616.1 — affected player chooses the order.
    affected = state.player_by_id(affected_player_id)
    agent = getattr(affected, "agent", None)
    if agent is not None and hasattr(agent, "choose_replacement_order"):
        order = agent.choose_replacement_order(
            [eff for _src, eff in candidates],
            state,
            affected_player_id,
        )
        # Sanitise: keep in-range, unique indices preserving order.
        seen: set[int] = set()
        clean: list[int] = []
        for idx in order:
            if 0 <= idx < len(candidates) and idx not in seen:
                clean.append(idx)
                seen.add(idx)
        # Append any missed indices in source-id order.
        for i, _ in enumerate(candidates):
            if i not in seen:
                clean.append(i)
        order = tuple(clean)
    else:
        # Default deterministic ordering: sort by source object id —
        # stable, replay-safe, and predictable for tests.
        order = tuple(
            sorted(range(len(candidates)), key=lambda i: candidates[i][0])
        )

    for idx in order:
        src_id, eff = candidates[idx]
        cur_payload = _apply_transform(
            eff.transform, cur_payload, _source_by_id(state, src_id),
        )
        if cur_payload.get("prevented"):
            # A Prevent short-circuits subsequent transforms — the
            # event has been intercepted.
            break

    return cur_payload


def _source_by_id(state: "GameState", obj_id: int):
    for o in state.battlefield:
        if o.id == obj_id:
            return o
    return None


def _collect_replacement_effects(
    state: "GameState",
    event: "Event",
    subjects: Mapping[str, Any],
    affected_player_id: int,
) -> list[tuple[int, "ReplacementEffect"]]:
    """Walk the battlefield + per-printing flags + dynamic synthesis
    sites, returning ``(source_id, ReplacementEffect)`` pairs whose
    ``event_pattern`` matches and whose subject / scope / source-
    controller filters accept the firing.

    Native ``StaticAbility.replacement_effect`` declarations on a
    printing are gated by the implicit "your X" check ``source.
    controller_id == affected_player_id`` — every replacement-flag
    surface in the EoE pool today reads as "if a token would be
    created under YOUR control, …" / "if YOU would draw a card, …",
    so making this gate the default lets the StaticAbility
    declarations stay free of repetitive controller-match plumbing.
    The wrapping ``StaticAbility.condition`` is also evaluated via
    ``condition.matches(state, source.controller_id)`` so cards with
    state-evaluated gates (Quantum Riddler's hand-size threshold)
    can declare them compositionally.

    Legacy printing-level boolean flags still synthesise matching
    ``ReplacementEffect`` instances for the cards that haven't yet
    migrated to native declarations — the synthesis paths are the
    PR-D-shaped retirement targets, one card / one flag per commit.
    """
    from mtgz_ir.effects.predicates import Event

    out: list[tuple[int, "ReplacementEffect"]] = []
    for obj in state.battlefield:
        # Native replacement effects on the printing's static
        # abilities. Implicit gate: source's controller IS the
        # affected player (``affects="you"``, every "your X would
        # happen" replacement), or is NOT (``affects="opponents"``,
        # "if an opponent would ...").
        yours = obj.controller_id == affected_player_id
        for static in active_static_abilities(obj):
            re = static.replacement_effect
            if re is None:
                continue
            if re.event_pattern.event != event:
                continue
            if (re.affects == "opponents") == yours:
                continue
            # Optional ``StaticAbility.condition`` gate
            # (state-aware, viewer = source's controller).
            cond = getattr(static, "condition", None)
            if cond is not None and not cond.matches(
                state, obj.controller_id,
            ):
                continue
            if not _subject_filters_match(
                re.event_pattern.subject_filters, subjects, state,
            ):
                continue
            if re.scope_filter is not None and not re.scope_filter.test(
                obj, state,
            ):
                continue
            out.append((obj.id, re))
    # Moving-card walker: if subjects carries a ``"card"`` key, the
    # dispatcher is firing on an event whose source is the card itself
    # (mid-flight, not on the battlefield) — Nexus-of-Fate-shape
    # WOULD_GO_TO_GRAVEYARD self-shuffles, future "if you would draw
    # this card …" moments, etc. Walk the moving card's
    # ``static_abilities`` looking for matching ``replacement_effect``
    # declarations. Implicit gate: ``card.owner_id ==
    # affected_player_id`` (every dispatch site setting a ``"card"``
    # subject already passes the owner as ``affected_player_id``, so
    # this gate is effectively always-true today, but keeping it
    # explicit guards against a future site that dispatches on a
    # different player's behalf).
    card = subjects.get("card")
    if card is not None and getattr(
        card, "owner_id", None,
    ) == affected_player_id:
        for static in active_static_abilities(card):
            re = static.replacement_effect
            if re is None:
                continue
            if re.event_pattern.event != event:
                continue
            cond = static.condition
            if cond is not None and not cond.matches(
                state, card.owner_id,
            ):
                continue
            if not _subject_filters_match(
                re.event_pattern.subject_filters, subjects, state,
            ):
                continue
            if re.scope_filter is not None and not re.scope_filter.test(
                card, state,
            ):
                continue
            out.append((getattr(card, "id", -1), re))
    return out


def _subject_filters_match(
    filters: "tuple[tuple[str, ObjectPredicate], ...]",
    subjects: Mapping[str, Any],
    state: "GameState",
) -> bool:
    """Return True iff every filter passes against its named subject.

    A missing subject (the event didn't carry that slot) fails the
    filter — the predicate can't evaluate without an object.

    ``filters`` is the canonical tuple-of-pairs form produced by
    :class:`EventPattern.__post_init__`.
    """
    for slot, pred in filters:
        obj = subjects.get(slot)
        if obj is None:
            return False
        if not pred.test(obj, state):
            return False
    return True


def active_static_abilities(source: "GameObject") -> "tuple":
    """Return the static abilities on ``source`` that are active now.

    THE single accessor for a permanent's (or a moving card's) static
    abilities. Every walk of the static surface — the layer-6 grant
    walk, the layer-7c anthem walk, the CR 613 continuous-modifier
    dispatcher, and the CR 614 replacement-effect dispatcher — routes
    through here so they all see the same set and all fail the same way
    on a malformed printing.

    DSK Room half-locking: a static tagged with a ``room_half`` is inert
    while that half is locked (not in ``source.unlocked_halves``).
    Non-Room statics (``room_half is None``) pass through unchanged.

    Returns the underlying ``static_abilities`` tuple directly for
    non-Room sources (the common case) — no per-call allocation.
    Callers iterate; none mutate. Reads ``printing.room_halves``
    directly rather than going through the ``is_room`` property to skip
    the property-dispatch overhead on this hot path (called ~1.1M times
    per self-play game per the 2026-05-02 perf profile).
    """
    printing = source.printing
    statics = printing.static_abilities
    if not printing.room_halves:
        return statics
    # Explicit loop rather than ``tuple(<genexp>)``: a genexp reading
    # ``unlocked`` would make it a cell variable of this function. See
    # the "closure-free hot path" note on ``mtgz_ir.game
    # ._static_bonuses`` — this helper is called from the same hot
    # layered-read frames.
    unlocked = source.unlocked_halves
    active = []
    for ability in statics:
        if ability.room_half is None or ability.room_half in unlocked:
            active.append(ability)
    return tuple(active)


def walk_continuous_modifiers(
    state: "GameState",
    target: "GameObject",
) -> "list[tuple[int, ContinuousModifier]]":
    """Yield ``(source_id, ContinuousModifier)`` pairs whose wrapping
    ``StaticAbility`` applies to ``target`` right now.

    Walks every permanent on the battlefield. For each permanent,
    walks its ``static_abilities`` looking for ones with a
    ``continuous_effect`` slot populated. Delegates the scope /
    condition / creature-check to ``StaticAbility.applies_to``. If
    the modifier also carries a ``subject_filter``, that predicate is
    evaluated as a secondary gate after ``applies_to`` passes.

    The result list is ordered by source ``etb_timestamp`` so callers
    that need layer-within-6 ordering can sort by source timestamp if
    needed. For the current EoE pool (layer-6 keyword grants/removes)
    the ordering only matters when a wipe and a grant interact — the
    existing ``_layer6_entries_for`` path handles that; this function
    provides a parallel contribution consumed after the existing walk.
    """
    out: list[tuple[int, ContinuousModifier]] = []
    for source in state.battlefield:
        for static in active_static_abilities(source):
            cm = static.continuous_effect
            if cm is None:
                continue
            if not static.applies_to(source, target, state):
                continue
            if cm.subject_filter is not None and not cm.subject_filter.test(
                target, state
            ):
                continue
            out.append((source.id, cm))
    return out


__all__ = [
    "EventTransform",
    "MultiplyCount",
    "AddToCount",
    "AddInstances",
    "Replace",
    "Prevent",
    "WinTheGameInstead",
    "EventPattern",
    "LayerPayload",
    "GrantKeyword",
    "RemoveKeyword",
    "PowerToughnessBonus",
    "SetBasePowerToughness",
    "CantBeBlocked",
    "AddSubtype",
    "ContinuousModifier",
    "ReplacementEffect",
    "dispatch_replacement",
    "active_static_abilities",
    "walk_continuous_modifiers",
]
