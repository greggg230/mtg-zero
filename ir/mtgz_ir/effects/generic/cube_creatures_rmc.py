"""Engine pieces for the cube creatures bucket: red / multicolor /
colorless creatures (PowerLSV cube).

Everything here is card-agnostic: each class models one printed shape
(a cost atom, a permission, an effect) that the oracle-text grammar in
:mod:`mtgz_ir.cards.parser.creature_rmc_grammar` composes. Sections:

* **Cost atoms** carried on ``ActivationCost.extra_costs`` —
  ``SacrificePermanentsCost`` ("Sacrifice a Forest", "Sacrifice an
  artifact", "Sacrifice five Treasures"), ``RemoveCountersFromSelfCost``
  ("Remove a +1/+1 counter from this creature"),
  ``TapUntappedPermanentsCost`` ("Tap an untapped Rebel you control"),
  ``ExileSelfFromGraveyardCost`` ("Exile this card from your
  graveyard"). Each exposes ``payable`` / ``pay`` / ``describe``.
* **Exile play permissions** — ``ExilePlayPermission`` rides
  ``GameObject.exile_play_permission`` and is honored by
  ``legal_actions`` / ``_cast_spell`` / ``_play_land`` for cards in ANY
  player's exile (Ragavan casts the opponent's card).
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import EventSubject, ExilePlayDuration, ImpulseLibraryOwner, ZoneName, coerce, coerce_tuple, plain
from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.effects.object_predicates import ObjectPredicate
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------------
# Cost atoms (ActivationCost.extra_costs)
# ---------------------------------------------------------------------------


def _pick_ids(state, player_id: int, options: list, n: int, *, tap: bool = False):
    """Let ``player_id``'s agent pick ``n`` distinct objects out of
    ``options`` (one ``choose_sacrifice`` / ``choose_permanent_to_tap``
    call per pick; an out-of-range answer takes the first remaining)."""
    agent = state.player_by_id(player_id).agent
    remaining = list(options)
    chosen = []
    for _ in range(n):
        if not remaining:
            break
        pick_id = None
        if agent is not None:
            ids = [o.id for o in remaining]
            if tap:
                pick_id = agent.choose_permanent_to_tap(state, player_id, ids)
            else:
                pick_id = agent.choose_sacrifice(state, player_id, ids)
        obj = next((o for o in remaining if o.id == pick_id), remaining[0])
        remaining.remove(obj)
        chosen.append(obj)
    return chosen


@dataclass(frozen=True)
class SacrificePermanentsCost:
    """Sacrifice ``count`` permanents you control matching
    ``permanent_filter`` ("Sacrifice a Forest", "Sacrifice an artifact",
    "Sacrifice five Treasures"). ``another`` excludes the source."""

    permanent_filter: "ObjectPredicate"
    count: int = 1
    another: bool = False
    label: str = "a permanent"

    def candidates(self, state, player_id: int, source) -> list:
        return [
            o for o in state.battlefield
            if o.controller_id == player_id
            and not (self.another and source is not None and o.id == source.id)
            and self.permanent_filter.test(o, state)
        ]

    def payable(self, state, player_id: int, source) -> bool:
        return len(self.candidates(state, player_id, source)) >= self.count

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.turn import sacrifice

        paid = _pick_ids(
            state, player_id, self.candidates(state, player_id, source),
            self.count,
        )
        for obj in paid:
            state.log.append(
                f"P{player_id} sacrifices {obj.printing.name} (cost)"
            )
            sacrifice(state, obj)
        return paid

    def describe(self) -> str:
        return f"Sacrifice {self.label}"


@dataclass(frozen=True)
class RemoveCountersFromSelfCost:
    """Remove ``count`` +1/+1 counters from the source (Walking
    Ballista)."""

    count: int = 1

    def payable(self, state, player_id: int, source) -> bool:
        return source is not None and source.plus_one_counters >= self.count

    def pay(self, state, player_id: int, source) -> list:
        source.plus_one_counters -= self.count
        state.log.append(
            f"P{player_id} removes {self.count} +1/+1 counter(s) from "
            f"{source.printing.name} (cost)"
        )
        return []

    def describe(self) -> str:
        noun = "a +1/+1 counter" if self.count == 1 else (
            f"{self.count} +1/+1 counters"
        )
        return f"Remove {noun} from ~"


@dataclass(frozen=True)
class TapUntappedPermanentsCost:
    """Tap ``count`` untapped permanents you control matching
    ``permanent_filter`` ("Tap an untapped Rebel you control"). Unlike
    ``{T}``, summoning sickness doesn't matter (CR 302.6 only restricts
    ``{T}`` / ``{Q}`` abilities of the creature itself)."""

    permanent_filter: "ObjectPredicate"
    count: int = 1
    label: str = "an untapped permanent you control"

    def candidates(self, state, player_id: int, source) -> list:
        return [
            o for o in state.battlefield
            if o.controller_id == player_id
            and not o.tapped
            and not (source is not None and o.id == source.id)
            and self.permanent_filter.test(o, state)
        ]

    def payable(self, state, player_id: int, source) -> bool:
        return len(self.candidates(state, player_id, source)) >= self.count

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.turn import tap_permanent

        paid = _pick_ids(
            state, player_id, self.candidates(state, player_id, source),
            self.count, tap=True,
        )
        for obj in paid:
            state.log.append(f"P{player_id} taps {obj.printing.name} (cost)")
            tap_permanent(state, obj)
        return paid

    def describe(self) -> str:
        return f"Tap {self.label}"


@dataclass(frozen=True)
class ExileSelfFromGraveyardCost:
    """"Exile this card from your graveyard" — the cost atom of an
    ability that functions from the graveyard (Seasoned Pyromancer)."""

    def payable(self, state, player_id: int, source) -> bool:
        if source is None:
            return False
        return source in state.player_by_id(source.owner_id).graveyard

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.game import Zone

        owner = state.player_by_id(source.owner_id)
        owner.graveyard.remove(source)
        source.zone = Zone.EXILE
        owner.exile.append(source)
        state.log.append(
            f"P{player_id} exiles {source.printing.name} from their "
            f"graveyard (cost)"
        )
        return [source]

    def describe(self) -> str:
        return "Exile this card from your graveyard"


def extra_costs_payable(state, player_id: int, source, cost) -> bool:
    """True when every ``cost.extra_costs`` atom can be paid."""
    return all(
        atom.payable(state, player_id, source) for atom in cost.extra_costs
    )


def pay_extra_costs(state, player_id: int, source, cost) -> list:
    """Pay every ``cost.extra_costs`` atom in printed order; returns the
    paid objects (for ``StackItem.cost_paid_objects``)."""
    paid: list = []
    for atom in cost.extra_costs:
        paid.extend(atom.pay(state, player_id, source))
    return paid


# ---------------------------------------------------------------------------
# Exile play permissions
# ---------------------------------------------------------------------------

# Permission lifetimes.
THIS_TURN = "this_turn"
UNTIL_YOUR_NEXT_END_STEP = "until_your_next_end_step"
UNTIL_END_OF_YOUR_NEXT_TURN = "until_end_of_your_next_turn"
# "While it's exiled, its owner may cast it" (airbend): never expires.
WHILE_EXILED = "while_exiled"


@dataclass(frozen=True)
class ExilePlayPermission:
    """"You may play / cast that card <duration>" on a card sitting in
    exile (any player's exile zone).

    * ``player_id`` — who may play it.
    * ``duration`` — ``THIS_TURN`` (expires at this turn's cleanup),
      ``UNTIL_YOUR_NEXT_END_STEP`` (expires at the cleanup of the next
      turn of ``player_id``, this one included), or
      ``UNTIL_END_OF_YOUR_NEXT_TURN`` (expires at the cleanup of
      ``player_id``'s next turn after the granting one).
    * ``granted_turn`` / ``granted_player_turns`` — ``state.turn_number``
      and ``player_id``'s ``turns_started`` when granted.
    * ``free`` — "without paying its mana cost".
    * ``cast_only`` — "you may cast that card" (a land can't be played).
    * ``requires_attacked_with_subtype`` — "During any turn you attacked
      with a <Subtype>" (Robber of the Rich); never expires on its own.
    * ``any_color_mana`` — "you may spend mana as though it were mana of
      any color to cast that spell".
    """

    player_id: int
    duration: str = THIS_TURN
    granted_turn: int = 0
    granted_player_turns: int = 0
    free: bool = False
    cast_only: bool = False
    requires_attacked_with_subtype: str = ""
    any_color_mana: bool = False
    # "... may cast it for {2} rather than its mana cost" (airbend): the
    # mana cost string paid instead of the card's (empty = its own).
    alt_cost: str = ""
    # "... may cast the creature later from exile" (an Adventure): the
    # card may be cast only with its primary (creature) characteristics.
    primary_only: bool = False

    def expired_after_cleanup(self, state: "GameState") -> bool:
        """Whether this permission ends at the cleanup step that is
        running now (called from ``turn.run_cleanup_step``)."""
        if self.requires_attacked_with_subtype or self.duration == WHILE_EXILED:
            return False
        if self.duration == THIS_TURN:
            return True
        active = state.active_player()
        if active.id != self.player_id:
            return False
        if self.duration == UNTIL_YOUR_NEXT_END_STEP:
            return True
        # UNTIL_END_OF_YOUR_NEXT_TURN: the granting turn itself (if it
        # was this player's) doesn't count.
        return active.turns_started > self.granted_player_turns

    def usable_now(self, state: "GameState", player_id: int) -> bool:
        if player_id != self.player_id:
            return False
        if self.requires_attacked_with_subtype:
            from mtgz_ir.game import effective_subtypes

            p = state.player_by_id(player_id)
            return any(
                self.requires_attacked_with_subtype
                in effective_subtypes(state, o)
                for o in p.attacked_this_turn
            )
        return True


def grant_exile_play(
    state: "GameState", card: "GameObject", player_id: int, **kwargs,
) -> None:
    """Attach an :class:`ExilePlayPermission` for ``player_id`` to
    ``card`` (which must already sit in an exile zone)."""
    p = state.player_by_id(player_id)
    card.exile_play_permission = ExilePlayPermission(
        player_id=player_id,
        granted_turn=state.turn_number,
        granted_player_turns=p.turns_started,
        **kwargs,
    )


def exile_permission_cast_cost(card: "GameObject", perm: ExilePlayPermission):
    """The mana cost to cast ``card`` under ``perm``: nothing when free;
    every colored / hybrid / Phyrexian pip generic when "mana of any
    type" may pay (Robber of the Rich); else the printed cost."""
    from mtgz_ir.cards.base import ManaCost

    cost = card.printing.mana_cost
    if perm.free:
        return ManaCost()
    if perm.alt_cost:
        from mtgz_ir.cards.base import parse_mana_cost

        return parse_mana_cost(perm.alt_cost)
    if perm.any_color_mana:
        return ManaCost(
            generic=cost.generic + sum(n for _, n in cost.colored)
            + len(cost.hybrid) + len(cost.phyrexian),
            x_pips=cost.x_pips,
        )
    return cost


def exile_cards_playable_by(state: "GameState", player_id: int) -> list:
    """Cards in any exile zone whose permission lets ``player_id`` play
    them right now."""
    out = []
    for pl in state.players:
        for card in pl.exile:
            perm = card.exile_play_permission
            if perm is not None and perm.usable_now(state, player_id):
                out.append(card)
    return out


def find_permitted_exile_card(state: "GameState", player_id: int, card_id: int):
    """``(card, owner)`` for a permission-playable exile card, else
    ``(None, None)``."""
    for pl in state.players:
        for card in pl.exile:
            if card.id != card_id:
                continue
            perm = card.exile_play_permission
            if perm is not None and perm.usable_now(state, player_id):
                return card, pl
    return None, None


def expire_exile_play_permissions(state: "GameState") -> None:
    """Cleanup-step hook: drop every permission whose duration ends
    now. Forks the (copy-on-write) exile lists first — the per-card
    attribute write must not leak into sibling search states."""
    from mtgz_ir.game import _CowList

    for pl in state.players:
        if not any(c.exile_play_permission is not None for c in pl.exile):
            continue
        if isinstance(pl.exile, _CowList):
            pl.exile.fork()
        for card in pl.exile:
            perm = card.exile_play_permission
            if perm is not None and perm.expired_after_cleanup(state):
                card.exile_play_permission = None


def exile_card(state: "GameState", card: "GameObject", *, face_down_for: int | None = None) -> None:
    """Move ``card`` (already removed from its previous zone list) into
    its owner's exile. ``face_down_for`` (a player id) keeps the card's
    identity known only to that player ("exile ... face down. You may
    look at ... that card")."""
    from mtgz_ir.game import Zone, reveal_to, reveal_to_all

    card.zone = Zone.EXILE
    state.player_by_id(card.owner_id).exile.append(card)
    card.exiled_face_down = face_down_for is not None
    if face_down_for is None:
        reveal_to_all(state, card)
    else:
        card.publicly_known_to = frozenset()
        reveal_to(card, face_down_for)


# ---------------------------------------------------------------------------
# Static-ability helpers
# ---------------------------------------------------------------------------


def effective_ward_life_cost(state: "GameState", obj: "GameObject") -> int:
    """Total "Ward—Pay N life" on ``obj``: its printed ward plus every
    'have "Ward—Pay N life."' grant from a static that applies to it
    (Hexing Squelcher). Each instance is a separate ward trigger
    (CR 702.21d); the simplified ward model pays them together."""
    total = obj.printing.ward_life_cost
    for source in state.battlefield:
        for sa in source.printing.static_abilities:
            if sa.grants_ward_life_cost and sa.applies_to(source, obj, state):
                total += sa.grants_ward_life_cost
    return total


# ---------------------------------------------------------------------------
# Effects
# ---------------------------------------------------------------------------


def _live_source_in_graveyard(state: "GameState"):
    """The resolving ability's source if it still sits in its owner's
    graveyard (CR 400.7: a card that left and came back is a new
    object; this engine keeps ids, so zone membership is the check)."""
    src = state.resolving_source
    if src is None:
        return None
    owner = state.player_by_id(src.owner_id)
    return src if src in owner.graveyard else None


@dataclass(frozen=True)
class ReturnThisCardFromGraveyardToBattlefield(Effect):
    """"Return this card from your graveyard to the battlefield
    [tapped]." — the effect of an ability that functions from the
    graveyard (Otharri, Suns' Glory). No-op if the card has left the
    graveyard by the time the ability resolves."""

    tapped: bool = False
    functions_from_graveyard = True

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield

        card = _live_source_in_graveyard(state)
        if card is None:
            return
        state.player_by_id(card.owner_id).graveyard.remove(card)
        state.log.append(
            f"{card.printing.name} returns from the graveyard to the "
            f"battlefield" + (" tapped" if self.tapped else "")
        )
        put_into_battlefield(state, card, controller_id)
        if self.tapped:
            card.tapped = True

    def describe(self) -> str:
        return (
            "return this card from your graveyard to the battlefield"
            + (" tapped" if self.tapped else "")
        )


@dataclass(frozen=True)
class ExileTopCardsWithPlayPermission(Effect):
    """"Exile the top N cards of <whose> library[ face down]. <You may
    play / cast that card <duration>>." — the impulse family (Golos,
    Ragavan, Laelia, Headliner Scarlett, Caves of Chaos Adventurer,
    Robber of the Rich). Each exiled card carries an
    :class:`ExilePlayPermission` for the controller.

    * ``whose`` — ``"you"`` (the controller's library),
      ``"that_player"`` (the player in the anaphoric ``targets`` tuple —
      the damaged player of a combat-damage trigger) or
      ``"defending_player"`` (the opponent, for attack triggers).
    * ``free`` / ``free_if`` — "without paying its mana cost" always, or
      only while ``free_if`` (a ``StatePredicate`` for the controller)
      holds at resolution ("If you've completed a dungeon, ...
      Otherwise, ...").
    * ``cast_only`` — "you may cast that card" (lands excluded).
    * ``face_down`` — only the controller learns the card.
    * ``requires_attacked_with_subtype`` / ``any_color_mana`` — Robber
      of the Rich's permission shape.
    """

    count: int = 1
    free: bool = False
    duration: ExilePlayDuration = ExilePlayDuration.THIS_TURN
    whose: ImpulseLibraryOwner = ImpulseLibraryOwner.YOU
    cast_only: bool = False
    face_down: bool = False
    free_if: object = None
    requires_attacked_with_subtype: str = ""
    any_color_mana: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "duration", ExilePlayDuration(self.duration))
        object.__setattr__(self, "whose", ImpulseLibraryOwner(self.whose))

    def _library_owner(self, state, controller_id, targets):
        if self.whose == "you":
            return controller_id
        if self.whose == "that_player":
            return next((tid for kind, tid in targets if kind == "player"), None)
        # defending player: the (single) opponent.
        return next(
            (pl.id for pl in state.players
             if pl.id != controller_id and not pl.has_lost),
            None,
        )

    def resolve(self, state, controller_id, targets=()):
        owner_id = self._library_owner(state, controller_id, targets)
        if owner_id is None:
            return
        p = state.player_by_id(owner_id)
        free = self.free or (
            self.free_if is not None and self.free_if.test(state, controller_id)
        )
        exiled = []
        for _ in range(self.count):
            if not p.library:
                break
            card = p.library.pop(0)
            exile_card(
                state, card,
                face_down_for=controller_id if self.face_down else None,
            )
            grant_exile_play(
                state, card, controller_id,
                duration=plain(self.duration), free=free,
                cast_only=self.cast_only,
                requires_attacked_with_subtype=self.requires_attacked_with_subtype,
                any_color_mana=self.any_color_mana,
            )
            exiled.append("a card" if self.face_down else card.printing.name)
        verb = "cast" if self.cast_only else "play"
        state.log.append(
            f"P{controller_id} exiles {', '.join(exiled) or 'nothing'} from "
            f"P{owner_id}'s library (may {verb} "
            f"{'free ' if free else ''}{self.duration})"
        )

    def describe(self) -> str:
        return (
            f"exile the top {self.count} card(s) of your library; you may "
            f"play them this turn"
            + (" without paying their mana costs" if self.free else "")
        )


# ---------------------------------------------------------------------------
# Dash (CR 702.109)
# ---------------------------------------------------------------------------


def apply_dash_etb(state: "GameState", obj: "GameObject", controller_id: int) -> None:
    """A dashed permanent entering: it has haste for as long as it stays
    on the battlefield, and a delayed trigger returns it to its owner's
    hand at the beginning of the next end step (CR 702.109a)."""
    state.add_continuous_effect(
        source_id=obj.id, layer=6, kind="grant_keyword",
        target_id=obj.id, params=("haste",),
        description=f"{obj.printing.name}: dash (haste)",
    )
    state.delayed_triggers.append({
        "fire_at_turn": state.turn_number,
        "controller_id": controller_id,
        "source_id": obj.id,
        "effect": ReturnDashedPermanentToHand(etb_timestamp=obj.etb_timestamp),
        "any_player_end_step": True,
    })
    state.log.append(f"{obj.printing.name} was dashed (haste; returns at end step)")


@dataclass(frozen=True)
class ReturnDashedPermanentToHand(Effect):
    """Dash's delayed trigger: return the dashed permanent to its
    owner's hand — only if it's the same object that entered
    (``etb_timestamp`` match; CR 400.7)."""

    etb_timestamp: int = 0

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import move_off_battlefield

        ref = state.resolving_source
        if ref is None:
            return
        obj = next((o for o in state.battlefield if o.id == ref.id), None)
        if obj is None or obj.etb_timestamp != self.etb_timestamp:
            return
        state.log.append(f"{obj.printing.name} returns to its owner's hand (dash)")
        move_off_battlefield(state, obj, destination="hand")

    def describe(self) -> str:
        return "return the dashed permanent to its owner's hand"


# ---------------------------------------------------------------------------
# Escape (CR 702.138c: "escaped")
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SacrificeSelfUnlessEscaped(Effect):
    """"When this creature enters, sacrifice it unless it escaped."
    (Phlage, Uro)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import sacrifice

        ref = state.resolving_source
        if ref is None:
            return
        obj = next((o for o in state.battlefield if o.id == ref.id), None)
        if obj is None or obj.escaped:
            return
        state.log.append(f"{obj.printing.name} is sacrificed (didn't escape)")
        sacrifice(state, obj)

    def describe(self) -> str:
        return "sacrifice it unless it escaped"


# ---------------------------------------------------------------------------
# "When you cast this spell" (cast triggers from the stack)
# ---------------------------------------------------------------------------

from mtgz_ir.effects.predicates import TriggerPredicate as _TriggerPredicate  # noqa: E402


@dataclass(frozen=True)
class IsThisSpell(_TriggerPredicate):
    """IS_CAST subject ``spell`` IS the ability's own card — "When you
    cast this spell". Never true for a battlefield permanent observing
    some other spell; the cast pipeline stacks these abilities itself
    (:func:`fire_this_spell_cast_triggers`)."""

    def matches(self, state, subjects, source):
        return subjects.get("spell") is source


def _is_this_spell_ability(ability) -> bool:
    from mtgz_ir.effects.predicates import And, Event

    if ability.event != Event.IS_CAST:
        return False
    pred = ability.predicate
    stack = [pred]
    while stack:
        p = stack.pop()
        if isinstance(p, IsThisSpell):
            return True
        if isinstance(p, And):
            stack.extend((p.left, p.right))
    return False


def fire_this_spell_cast_triggers(state: "GameState", card: "GameObject", controller_id: int) -> None:
    """Stack every "When you cast this spell" ability of ``card``, the
    spell just put on the stack (source = the spell)."""
    from mtgz_ir.actions import push_triggered_ability

    caster = state.player_by_id(controller_id)
    subjects = {"spell": card, "controller": caster}
    for idx, ability in enumerate(card.printing.triggered_abilities):
        if not _is_this_spell_ability(ability):
            continue
        if not ability.matches_event(ability.event, subjects, card, state):
            continue
        push_triggered_ability(
            state, source=card, ability=ability, ability_index=idx,
            controller_id=controller_id, label="cast",
        )


def cast_card_from_exile_for_free(state: "GameState", player_id: int, card: "GameObject") -> bool:
    """Cast ``card`` (in some exile zone) without paying its mana cost
    in the middle of a resolving ability (cascade, CR 608.2g: timing is
    ignored). Returns False — leaving the card where it is — when the
    spell couldn't be cast (no legal targets / unpayable additional
    costs)."""
    from mtgz_ir.actions import _cast_spell
    from mtgz_ir.legal import _additional_costs_payable, _targets_are_satisfiable

    if card.printing.is_land:
        return False
    grant_exile_play(state, card, player_id, free=True, cast_only=True)
    if not (
        _targets_are_satisfiable(state, player_id, card)
        and _additional_costs_payable(state, player_id, card)
    ):
        card.exile_play_permission = None
        return False
    _cast_spell(state, player_id, card.id, cast_for_free=True)
    return True


@dataclass(frozen=True)
class Cascade(Effect):
    """Cascade (CR 702.85a): "When you cast this spell, exile cards from
    the top of your library until you exile a nonland card whose mana
    value is less than this spell's mana value. You may cast that card
    without paying its mana cost. Then put all cards exiled this way that
    weren't cast on the bottom of your library in a random order."

    The spell is the resolving ability's source (its printed mana value
    is read even if it has since left the stack)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        spell = state.resolving_source
        if spell is None:
            return
        threshold = spell.printing.mana_cost.total
        p = state.player_by_id(controller_id)
        exiled: list = []
        hit = None
        while p.library:
            card = p.library.pop(0)
            exile_card(state, card)
            exiled.append(card)
            if not card.printing.is_land and card.printing.mana_cost.total < threshold:
                hit = card
                break
        state.log.append(
            f"P{controller_id} cascades into "
            f"{hit.printing.name if hit else 'nothing'} "
            f"({len(exiled)} card(s) exiled)"
        )
        cast = False
        if hit is not None:
            agent = p.agent
            wants = agent is None or agent.choose_yes_no(
                state, controller_id,
                f"Cascade: cast {hit.printing.name} without paying its mana cost?",
            )
            if wants:
                cast = cast_card_from_exile_for_free(state, controller_id, hit)
        rest = [c for c in exiled if not (cast and c is hit)]
        state.rng.shuffle(rest)
        for c in rest:
            state.player_by_id(c.owner_id).exile.remove(c)
            c.exiled_face_down = False  # left exile (CR 400.7)
            c.exile_play_permission = None
            c.zone = Zone.LIBRARY
            p.library.append(c)

    def describe(self) -> str:
        return "cascade"


# ---------------------------------------------------------------------------
# More impulse variants
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CardTypeOrSubtypeFilter:
    """A card matching ANY of ``types`` (card types) or ``subtypes``
    ("Equipment, instant, or sorcery card"; "artifact or Dragon card").
    Same ``matches(printing)`` protocol as ``card_flow.CardFilter``."""

    types: tuple[str, ...] = ()
    subtypes: tuple[str, ...] = ()

    @property
    def has_quality(self) -> bool:
        return True

    def matches(self, printing, *, types=None, colors=None) -> bool:
        card_types = printing.card_types if types is None else types
        return any(t in card_types for t in self.types) or any(
            s in printing.subtypes for s in self.subtypes
        )

    def matches_card(self, state, card) -> bool:
        """:meth:`matches` against a game object's effective card types
        (``game.effective_card_types``)."""
        from mtgz_ir.game import effective_card_types

        return self.matches(card.printing, types=effective_card_types(state, card))

    def describe(self) -> str:
        return " or ".join((*self.types, *self.subtypes)) + " card"


@dataclass(frozen=True)
class ExileRandomGraveyardCardWithPlayPermission(Effect):
    """"Exile a card at random from your graveyard. You may play that
    card this turn." (Tersa Lightshatter)."""

    duration: ExilePlayDuration = ExilePlayDuration.THIS_TURN

    def __post_init__(self) -> None:
        object.__setattr__(self, "duration", ExilePlayDuration(self.duration))

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        if not p.graveyard:
            return
        card = state.rng.choice(list(p.graveyard))
        p.graveyard.remove(card)
        exile_card(state, card)
        grant_exile_play(state, card, controller_id, duration=plain(self.duration))
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} at random from "
            f"their graveyard (may play {self.duration})"
        )

    def describe(self) -> str:
        return "exile a card at random from your graveyard; you may play it"


@dataclass(frozen=True)
class ExileTargetGraveyardCardWithPlayPermission(Effect):
    """"Exile target <card filter> card from your graveyard. <Until the
    end of your next turn>, you may play that card." (Thor, God of
    Thunder). ``card_filter`` is a ``CardFilter`` over printings."""

    card_filter: object = None
    duration: ExilePlayDuration = ExilePlayDuration.UNTIL_END_OF_YOUR_NEXT_TURN

    def __post_init__(self) -> None:
        object.__setattr__(self, "duration", ExilePlayDuration(self.duration))

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_card", c.id) for c in p.graveyard
            if self.card_filter is None or self.card_filter.matches_card(state, c)
        ]

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        for kind, tid in targets:
            if kind != "graveyard_card":
                continue
            card = next((c for c in p.graveyard if c.id == tid), None)
            if card is None:
                continue
            p.graveyard.remove(card)
            exile_card(state, card)
            grant_exile_play(state, card, controller_id, duration=plain(self.duration))
            state.log.append(
                f"P{controller_id} exiles {card.printing.name} from their "
                f"graveyard (may play {self.duration})"
            )

    def describe(self) -> str:
        return "exile target card from your graveyard; you may play it"


# ---------------------------------------------------------------------------
# "can't block this turn"
# ---------------------------------------------------------------------------

_CANT_BLOCK = "cant_block"
_CREATURES_CANT_BLOCK = "creatures_cant_block"


def creature_cant_block(state: "GameState", obj: "GameObject") -> bool:
    """Whether a "can't block this turn" effect applies to ``obj``: a
    per-creature entry (``TargetCreatureCantBlockThisTurn``) or a
    player-wide one (``CreaturesCantBlockThisTurn`` — every creature the
    named player controls, including ones that arrive later this turn;
    ``params == (None,)`` means every player)."""
    for ce in state.continuous_effects:
        if ce.kind == _CANT_BLOCK and ce.target_id == obj.id:
            return True
        if ce.kind == _CREATURES_CANT_BLOCK and ce.params and (
            ce.params[0] is None or ce.params[0] == obj.controller_id
        ):
            return True
    return False


@dataclass(frozen=True)
class TargetCreatureCantBlockThisTurn(Effect):
    """"[Up to one ]target creature can't block this turn." """

    optional: bool = False
    targets: tuple = ()

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature
        from mtgz_ir.effects.targets import TargetSpec

        object.__setattr__(self, "targets", (TargetSpec(
            IsCreature(), count_min=0 if self.optional else 1, count_max=1,
        ),))

    def resolve(self, state, controller_id, targets=()):
        for kind, tid in targets:
            if kind == "player":
                continue
            obj = next((o for o in state.battlefield if o.id == tid), None)
            if obj is None:
                continue
            src = state.resolving_source
            state.add_continuous_effect(
                source_id=src.id if src is not None else -1,
                layer=6, kind=_CANT_BLOCK, target_id=obj.id,
                expires_at="cleanup",
                description=f"{obj.printing.name} can't block this turn",
            )
            state.log.append(f"{obj.printing.name} can't block this turn")

    def describe(self) -> str:
        return ("up to one " if self.optional else "") + (
            "target creature can't block this turn"
        )


@dataclass(frozen=True)
class CreaturesCantBlockThisTurn(Effect):
    """"Creatures can't block this turn." / "Creatures target player
    controls can't block this turn." (``target_player``)."""

    target_player: bool = False

    def requires_target(self) -> bool:
        return self.target_player

    def target_count_range(self):
        return (1, 1) if self.target_player else (0, 0)

    def legal_targets(self, state, controller_id):
        if not self.target_player:
            return []
        return [("player", pl.id) for pl in state.players if not pl.has_lost]

    def resolve(self, state, controller_id, targets=()):
        pid = None
        if self.target_player:
            pid = next((tid for kind, tid in targets if kind == "player"), None)
            if pid is None:
                return
        src = state.resolving_source
        label = (
            "creatures can't block this turn" if pid is None
            else f"creatures P{pid} controls can't block this turn"
        )
        state.add_continuous_effect(
            source_id=src.id if src is not None else -1,
            layer=6, kind=_CREATURES_CANT_BLOCK, params=(pid,),
            expires_at="cleanup", description=label,
        )
        state.log.append(label)

    def describe(self) -> str:
        return (
            "creatures target player controls can't block this turn"
            if self.target_player else "creatures can't block this turn"
        )


# ---------------------------------------------------------------------------
# Conditions / formulas / formula damage
# ---------------------------------------------------------------------------

from mtgz_ir.effects.conditions import Condition as _Condition  # noqa: E402
from mtgz_ir.effects.formulas import Formula as _Formula  # noqa: E402


@dataclass(frozen=True)
class StateCondition(_Condition):
    """Adapter: a viewer-scoped ``StatePredicate`` as a ``Condition``
    ("if you're the monarch", "if you've completed a dungeon")."""

    predicate: object = None

    def matches(self, state, viewer_id, chosen_targets=None):
        return self.predicate.test(state, viewer_id)


@dataclass(frozen=True)
class ResolvingDamageAmount(_Formula):
    """"that much damage" — the damage amount carried by the resolving
    dealt-damage / combat-damage trigger."""

    def value(self, state, controller_id):
        return max(0, state.resolving_combat_damage_amount)


@dataclass(frozen=True)
class DealDamageByFormula(Effect):
    """"<source> deals damage equal to <formula> to <target>" — the
    unified ``DealDamage`` resolve with a runtime amount (Broadside
    Bombardiers: 2 plus the sacrificed permanent's mana value)."""

    amount: object = None
    targets: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )
        from mtgz_ir.effects.targets import TargetSpec

        n = self.amount.value(state, controller_id)
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if not isinstance(slot, TargetSpec):
                    continue
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(_fizzle_message(state, chosen))
                    continue
                if n > 0:
                    _apply_damage(state, chosen, n)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return "deals damage equal to X to target"


# ---------------------------------------------------------------------------
# Event back-references, formulas and formula effects (batch 3)
# ---------------------------------------------------------------------------


def find_object_anywhere(state: "GameState", obj_id: int):
    """The object with ``obj_id`` on the battlefield, the stack or in any
    zone (last-known information for "that spell" / "that creature")."""
    for o in state.battlefield:
        if o.id == obj_id:
            return o
    for item in state.stack:
        if item.source.id == obj_id:
            return item.source
    for pl in state.players:
        for zone in (pl.graveyard, pl.exile, pl.hand, pl.library):
            for o in zone:
                if o.id == obj_id:
                    return o
    return None


@dataclass(frozen=True)
class EventSubjectManaValue(_Formula):
    """"that spell's mana value" — the triggering spell (the resolving
    ability's ``event_subject_id``)."""

    def value(self, state, controller_id):
        obj = find_object_anywhere(state, state.resolving_event_subject_id)
        return obj.printing.mana_cost.total if obj is not None else 0


@dataclass(frozen=True)
class EventSubjectPower(_Formula):
    """"its power" of the triggering creature ("that creature deals
    damage equal to its power"); its last known power if it left."""

    def value(self, state, controller_id):
        from mtgz_ir.game import effective_power

        obj = find_object_anywhere(state, state.resolving_event_subject_id)
        if obj is None:
            return 0
        if obj in state.battlefield:
            return max(0, effective_power(state, obj))
        return max(0, (obj.printing.power or 0) + obj.plus_one_counters)


@dataclass(frozen=True)
class ExperienceCounters(_Formula):
    """"for each experience counter you have"."""

    def value(self, state, controller_id):
        return state.player_by_id(controller_id).experience_counters


@dataclass(frozen=True)
class CountPermanentsOfPlayer(_Formula):
    """Number of permanents matching ``predicate`` that ``whose``
    controls: ``"defending"`` = the (single) opponent of the controller
    ("the number of artifacts they control" in an attack trigger)."""

    predicate: object = None

    def value(self, state, controller_id):
        opp = next(
            (pl.id for pl in state.players if pl.id != controller_id), None,
        )
        return sum(
            1 for o in state.battlefield
            if o.controller_id == opp and self.predicate.test(o, state)
        )


@dataclass(frozen=True)
class DealDamageFromEvent(Effect):
    """Formula damage with an event back-reference: ``subject_slot`` names
    the dispatch subject the stack item remembers (``"spell"`` for
    "that spell's mana value", ``"obj"`` for "that creature ... its
    power"). ``targets`` is the chosen-target slot tuple (any target /
    any other target); ``to_defending_player`` instead hits the
    controller's opponent (no target). ``no_lifegain_if_player_hit``:
    "If a player is dealt damage this way, they can't gain life for
    the rest of the game." (Screaming Nemesis)."""

    amount: object = None
    targets: tuple = ()
    subject_slot: "EventSubject | None" = None
    to_defending_player: bool = False
    no_lifegain_if_player_hit: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "subject_slot", coerce(EventSubject, self.subject_slot or None))

    def event_subject_id(self, subjects) -> int:
        if not self.subject_slot:
            return -1
        obj = subjects.get(self.subject_slot)
        return getattr(obj, "id", -1) if obj is not None else -1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.effects.generic.targets_helpers import (
            _fizzle_message,
            _slot_target_still_legal,
        )
        from mtgz_ir.effects.targets import TargetSpec

        n = self.amount.value(state, controller_id)
        if self.to_defending_player:
            opp = next(
                (pl.id for pl in state.players if pl.id != controller_id), None,
            )
            if opp is not None and n > 0:
                _apply_damage(state, ("player", opp), n)
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if not isinstance(slot, TargetSpec):
                    continue
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(_fizzle_message(state, chosen))
                    continue
                if n <= 0:
                    continue
                _apply_damage(state, chosen, n)
                if self.no_lifegain_if_player_hit and chosen[0] == "player":
                    state.player_by_id(chosen[1]).cant_gain_life = True
                    state.log.append(
                        f"P{chosen[1]} can't gain life for the rest of the game"
                    )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return "deals damage equal to X"


# ---------------------------------------------------------------------------
# Hand / discard shapes
# ---------------------------------------------------------------------------


def _discard_chosen(state, player_id: int, n: int) -> list:
    """Discard ``n`` cards chosen by ``player_id`` (fewer if the hand
    runs out); returns the discarded cards."""
    from mtgz_ir.game import Zone
    from mtgz_ir.turn import fire_opponent_discards_triggers

    from mtgz_ir.player_protections import opponent_effect_blocked_for

    p = state.player_by_id(player_id)
    out = []
    for _ in range(n):
        if not p.hand or opponent_effect_blocked_for(state, player_id):
            break
        ids = [c.id for c in p.hand]
        pick = p.agent.choose_discard(state, player_id, ids) if p.agent else ids[0]
        card = next((c for c in p.hand if c.id == pick), p.hand[0])
        p.hand.remove(card)
        card.zone = Zone.GRAVEYARD
        p.graveyard.append(card)
        state.log.append(f"P{player_id} discards {card.printing.name}")
        fire_opponent_discards_triggers(state, player_id, card=card)
        out.append(card)
    return out


@dataclass(frozen=True)
class DiscardYourHand(Effect):
    """"discard your hand"."""

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        _discard_chosen(state, controller_id, len(p.hand))

    def describe(self) -> str:
        return "discard your hand"


@dataclass(frozen=True)
class DiscardThenDrawTokensPerNonland(Effect):
    """"discard N cards, then draw M cards. For each nonland card
    discarded this way, <create token>." (Seasoned Pyromancer)."""

    discard: int = 2
    draw: int = 2
    token_effect: object = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        discarded = _discard_chosen(state, controller_id, self.discard)
        draw_n_cards(state, controller_id, self.draw)
        nonland = sum(1 for c in discarded if not c.printing.is_land)
        for _ in range(nonland):
            self.token_effect.resolve(state, controller_id, ())

    def describe(self) -> str:
        return (
            f"discard {self.discard} cards, then draw {self.draw}; a token "
            f"per nonland card discarded"
        )


@dataclass(frozen=True)
class DiscardUpToNThenDrawThatMany(Effect):
    """"discard up to N cards, then draw that many cards" (Tersa
    Lightshatter). The count is the controller's ``choose_x``."""

    n: int = 2

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        p = state.player_by_id(controller_id)
        cap = min(self.n, len(p.hand))
        k = cap
        if p.agent is not None and cap > 0:
            k = max(0, min(cap, p.agent.choose_x(
                state, controller_id, "discard up to", cap,
            )))
        discarded = _discard_chosen(state, controller_id, k)
        if discarded:
            draw_n_cards(state, controller_id, len(discarded))

    def describe(self) -> str:
        return f"discard up to {self.n} cards, then draw that many"


# ---------------------------------------------------------------------------
# Counters / tokens / pumps
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GetExperienceCounter(Effect):
    """"you get an experience counter"."""

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        p.experience_counters += 1
        state.log.append(
            f"P{controller_id} gets an experience counter "
            f"({p.experience_counters})"
        )

    def describe(self) -> str:
        return "you get an experience counter"


@dataclass(frozen=True)
class CreateTokenPerFormula(Effect):
    """"create <token> for each <formula unit>" — the wrapped
    ``CreateToken`` resolves with its count replaced by the formula."""

    token_effect: object = None
    amount: object = None

    def resolve(self, state, controller_id, targets=()):
        n = self.amount.value(state, controller_id)
        if n <= 0:
            return
        replace(self.token_effect, count=n).resolve(state, controller_id, ())

    def describe(self) -> str:
        return f"{self.token_effect.describe()} for each X"


@dataclass(frozen=True)
class TargetOpponentCreatesToken(Effect):
    """"target opponent creates <token>" — the token is created under
    the targeted opponent's control."""

    token_effect: object = None

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", pl.id) for pl in state.players
            if pl.id != controller_id and not pl.has_lost
        ]

    def resolve(self, state, controller_id, targets=()):
        pid = next((tid for kind, tid in targets if kind == "player"), None)
        if pid is None:
            return
        self.token_effect.resolve(state, pid, ())

    def describe(self) -> str:
        return f"target opponent {self.token_effect.describe()}"


@dataclass(frozen=True)
class MayDoThenReflexive(Effect):
    """"You may <first>. When you do, <then>." — a reflexive trigger
    (CR 603.12). ``then``'s targets are chosen with the enclosing
    ability (the Minsc & Boo precedent; see SEARCH_LIMITATIONS) and it
    resolves right after ``first`` when the controller chose to."""

    first: object = None
    then: object = None

    def requires_target(self) -> bool:
        return self.then.requires_target()

    def target_count_range(self):
        return self.then.target_count_range()

    def legal_targets(self, state, controller_id):
        return self.then.legal_targets(state, controller_id)

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, f"You may {self.first.describe()}",
        ):
            return
        self.first.resolve(state, controller_id, ())
        self.then.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"you may {self.first.describe()}; when you do, {self.then.describe()}"


@dataclass(frozen=True)
class DoThenReflexive(Effect):
    """"<first>. When you do, <then>." — ``first`` is done, then a
    reflexive triggered ability (CR 603.12) whose effect is ``then``
    triggers (Ajani, Nacatl Avenger's 0). An intervening "if" of the
    reflexive ability ("When you do, if you control ...") is ``then``'s
    ``IfCondition``. The non-optional twin of :class:`MayDoThenReflexive`,
    with the same engine shortcut: ``then``'s targets are chosen with
    the enclosing ability and it resolves right after ``first``
    (docs/SEARCH_LIMITATIONS.md)."""

    first: object = None
    then: object = None

    def requires_target(self) -> bool:
        return self.then.requires_target()

    def target_count_range(self):
        return self.then.target_count_range()

    def target_slot_kinds(self):
        return self.then.target_slot_kinds()

    def legal_targets(self, state, controller_id):
        return self.then.legal_targets(state, controller_id)

    def resolve(self, state, controller_id, targets=()):
        self.first.resolve(state, controller_id, ())
        self.then.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"{self.first.describe()}; when you do, {self.then.describe()}"


@dataclass(frozen=True)
class PumpTargetCreatureByFormulaUEOT(Effect):
    """"Target creature gets <±X>/<±X> until end of turn, where X is
    <formula>." ``power_sign`` / ``toughness_sign`` are -1/0/+1."""

    amount: object = None
    power_sign: int = -1
    toughness_sign: int = -1
    targets: tuple = ()

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature
        from mtgz_ir.effects.targets import TargetSpec

        object.__setattr__(self, "targets", (TargetSpec(IsCreature()),))

    def resolve(self, state, controller_id, targets=()):
        x = self.amount.value(state, controller_id)
        for kind, tid in targets:
            obj = next((o for o in state.battlefield if o.id == tid), None)
            if obj is None:
                continue
            src = state.resolving_source
            state.grant_pt_pump_ueot(
                source_id=src.id if src is not None else -1,
                target_id=obj.id,
                power=self.power_sign * x, toughness=self.toughness_sign * x,
                description=f"{obj.printing.name} gets X/X (X={x})",
            )

    def describe(self) -> str:
        return "target creature gets ±X/±X until end of turn"


from mtgz_ir.effects.generic.exile import (  # noqa: E402
    ExileTargetCardFromGraveyard as _ExileTargetCardFromGraveyard,
)


class ExileUpToOneTargetCardFromGraveyard(_ExileTargetCardFromGraveyard):
    """"Exile up to one target card from a graveyard." — zero or one
    target. ``requires_target`` stays True so a ``ModalChooseOne``
    around it still surfaces the graveyard pool when the trigger is put
    on the stack (the modal combinator keys "targeted mode" on it); the
    ``(0, 1)`` count range lets the controller choose no target."""

    def target_count_range(self):
        return (0, 1)

    def describe(self) -> str:
        return "exile up to one target card from a graveyard"


# ---------------------------------------------------------------------------
# Batch 4: attack requirements, enters-tapped statics, reveal effects,
# magecraft, fights, random destruction, level-up style "becomes",
# graveyard self-replacement
# ---------------------------------------------------------------------------


def must_attack_ids(state: "GameState", player_id: int, eligible: list) -> list[int]:
    """Ids among ``eligible`` (attack-eligible creatures of
    ``player_id``) that "attack each combat if able" through an
    applying static (Dragon's Rage Channeler's delirium clause)."""
    out: list[int] = []
    for obj in eligible:
        for source in state.battlefield:
            if any(
                sa.must_attack_each_combat and sa.applies_to(source, obj, state)
                for sa in source.printing.static_abilities
            ):
                out.append(obj.id)
                break
    return out


def enters_tapped_by_opponent_static(state: "GameState", obj: "GameObject", controller_id: int) -> bool:
    """Whether a static of a permanent an opponent of ``controller_id``
    controls makes ``obj`` enter tapped ("Artifacts and creatures your
    opponents control enter tapped." — Spider-Woman)."""
    for source in state.battlefield:
        if source.controller_id == controller_id:
            continue
        for sa in source.printing.static_abilities:
            types = sa.opponents_enter_tapped_types
            if not types:
                continue
            # The entering permanent's types as it would exist on the
            # battlefield (CR 614.12): ``obj.zone`` is already the
            # battlefield here, so layer 4 (Mycosynth Lattice) applies.
            from mtgz_ir.game import has_card_type

            for t in types:
                if has_card_type(state, obj, t):
                    return True
    return False


@dataclass(frozen=True)
class RevealUntilCreatureOntoBattlefieldAttacking(Effect):
    """Raph & Mikey: reveal cards from the top of your library until you
    reveal a creature card; put that card onto the battlefield tapped
    and attacking (the single opponent) and the rest on the bottom of
    your library in a random order."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        revealed: list = []
        hit = None
        while p.library:
            card = p.library.pop(0)
            reveal_to_all(state, card)
            if card.printing.is_creature:
                hit = card
                break
            revealed.append(card)
        if hit is not None:
            state.log.append(
                f"P{controller_id} reveals {hit.printing.name}; it enters "
                f"tapped and attacking"
            )
            put_into_battlefield(state, hit, controller_id)
            hit.tapped = True
            hit.attacking = True
        state.rng.shuffle(revealed)
        for c in revealed:
            c.zone = Zone.LIBRARY
            p.library.append(c)

    def describe(self) -> str:
        return (
            "reveal until a creature card; it enters tapped and attacking, "
            "the rest go to the bottom"
        )




@dataclass(frozen=True)
class RevealTopNOnePerCardTypeToHand(Effect):
    """Atraxa, Grand Unifier: reveal the top N cards; for each card type
    you may put a card of that type from among them into your hand; the
    rest go to the bottom in a random order. Card types are walked in
    CR 205.2a order; for each one the controller picks one of the
    still-unpicked revealed cards with that type
    (``choose_card_from_library``; ``None`` declines)."""

    count: int = 10

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to, reveal_to_all

        p = state.player_by_id(controller_id)
        revealed = []
        for _ in range(self.count):
            if not p.library:
                break
            card = p.library.pop(0)
            reveal_to_all(state, card)
            revealed.append(card)
        from mtgz_ir.effects.card_types import CARD_TYPES, card_types_of

        taken: list = []
        for card_type in CARD_TYPES:
            options = [
                c for c in revealed
                if c not in taken and card_type in card_types_of(c)
            ]
            if not options:
                continue
            pick = options[0].id
            if p.agent is not None:
                pick = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in options],
                )
            chosen = next((c for c in options if c.id == pick), None)
            if chosen is not None:
                taken.append(chosen)
        for c in taken:
            c.zone = Zone.HAND
            p.hand.append(c)
            reveal_to(c, controller_id)
        rest = [c for c in revealed if c not in taken]
        state.rng.shuffle(rest)
        for c in rest:
            c.zone = Zone.LIBRARY
            p.library.append(c)
        state.log.append(
            f"P{controller_id} reveals {len(revealed)} and takes "
            f"{', '.join(c.printing.name for c in taken) or 'nothing'}"
        )

    def describe(self) -> str:
        return f"reveal the top {self.count} cards; take one of each card type"


@dataclass(frozen=True)
class CastOrCopy(_TriggerPredicate):
    """Marker conjunct for magecraft-style "Whenever you cast or copy a
    <spell>": true for the cast dispatch; ``fire_copy_triggers`` stacks
    the same abilities when a spell is copied (CR 707.10: copies aren't
    cast)."""

    def matches(self, state, subjects, source):
        return True


def _has_cast_or_copy(pred) -> bool:
    from mtgz_ir.effects.predicates import And

    stack = [pred]
    while stack:
        p = stack.pop()
        if isinstance(p, CastOrCopy):
            return True
        if isinstance(p, And):
            stack.extend((p.left, p.right))
    return False


def fire_copy_triggers(state: "GameState", copy_obj: "GameObject", controller_id: int) -> None:
    """A spell copy was put on the stack: stack every battlefield
    "Whenever you cast or copy ..." ability whose predicate accepts the
    copy (same subject mapping as the cast dispatch)."""
    from mtgz_ir.actions import push_triggered_ability
    from mtgz_ir.effects.predicates import Event

    subjects = {"spell": copy_obj, "controller": state.player_by_id(controller_id)}
    for source in list(state.battlefield):
        for idx, ability in enumerate(source.printing.triggered_abilities):
            if ability.event != Event.IS_CAST or not _has_cast_or_copy(ability.predicate):
                continue
            if not ability.matches_event(Event.IS_CAST, subjects, source, state):
                continue
            push_triggered_ability(
                state, source=source, ability=ability, ability_index=idx,
                controller_id=source.controller_id, label="copy",
            )


@dataclass(frozen=True)
class SourceFightsTarget(Effect):
    """"<This creature> fights [up to one ][other ]target creature
    [<qualifier>]." (CR 701.12): each deals damage equal to its power to
    the other; nothing happens if either creature has left the
    battlefield. ``np_filter`` replaces the bare ``IsCreature`` target
    filter with a parsed noun phrase ("creature an opponent controls").
    ``exile_if_dies_this_turn`` is Mawloc's rider ("If that creature
    would die this turn, exile it instead."): the fought creature gets
    the spells-A bucket's per-object ``exile_if_dies_this_turn`` flag
    before it is dealt damage, so the replacement applies to the fight
    damage itself."""

    optional: bool = False
    other: bool = True
    targets: tuple = ()
    np_filter: object = None
    exile_if_dies_this_turn: bool = False

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsCreature, IsSource
        from mtgz_ir.effects.targets import TargetSpec

        pred = IsCreature() if self.np_filter is None else self.np_filter
        if self.other:
            pred = pred & ~IsSource()
        object.__setattr__(self, "targets", (TargetSpec(
            pred, count_min=0 if self.optional else 1, count_max=1,
        ),))

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.game import effective_power

        src = state.resolving_source
        me = next(
            (o for o in state.battlefield if src is not None and o.id == src.id),
            None,
        )
        for _kind, tid in targets:
            them = next((o for o in state.battlefield if o.id == tid), None)
            if me is None or them is None:
                return
            p_me = effective_power(state, me)
            p_them = effective_power(state, them)
            state.log.append(f"{me.printing.name} fights {them.printing.name}")
            if self.exile_if_dies_this_turn:
                them.exile_if_dies_this_turn = True
            if p_me > 0:
                _apply_damage(state, ("creature", them.id), p_me)
            if p_them > 0:
                _apply_damage(state, ("creature", me.id), p_them)

    def describe(self) -> str:
        return "fights " + ("up to one " if self.optional else "") + (
            "other " if self.other else ""
        ) + "target creature"


def heal_before_damage(obj: "GameObject") -> None:
    """Wolverine's replacement ("If damage would be dealt to <it>,
    instead that damage is dealt, but all other damage already dealt to
    him is healed."), applied right before new damage is marked."""
    if obj.printing.heals_other_damage_when_dealt_damage:
        obj.damage = 0
        obj.marked_lethal = False


@dataclass(frozen=True)
class EachOpponentChoiceDestroyOneAtRandom(Effect):
    """Chaos Defiler: for each opponent, choose a nonland permanent that
    player controls; destroy one of them chosen at random. The choice
    isn't targeted (hexproof doesn't stop it)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import destroy

        p = state.player_by_id(controller_id)
        chosen = []
        for opp in state.players:
            if opp.id == controller_id or opp.has_lost:
                continue
            options = [
                o for o in state.battlefield
                if o.controller_id == opp.id and not o.printing.is_land
            ]
            if not options:
                continue
            pick = options[0].id
            if p.agent is not None:
                picked = p.agent.choose_sacrifice(
                    state, controller_id, [o.id for o in options],
                )
                if picked in {o.id for o in options}:
                    pick = picked
            chosen.append(next(o for o in options if o.id == pick))
        if not chosen:
            return
        victim = state.rng.choice(chosen)
        state.log.append(f"{victim.printing.name} is destroyed (chosen at random)")
        destroy(state, victim)

    def describe(self) -> str:
        return (
            "for each opponent, choose a nonland permanent they control; "
            "destroy one of them at random"
        )


@dataclass(frozen=True)
class SourceBecomesIfSubtype(Effect):
    """Figure of Destiny: "[If this creature is a <Req>, ]it becomes a
    <Subtypes> with base power and toughness P/T[, <keywords>]." — a
    permanent change (no duration): the creature types are replaced
    (CR 205.1b; layer 4 ``set_creature_subtypes``), keywords are added
    and the base P/T set (latest timestamp wins). Also Figure of Fable
    ("... 7/8 and protection from each of your opponents")."""

    subtypes: tuple[str, ...] = ()
    base_pt: tuple = (0, 0)
    keywords: frozenset[KeywordSpelling] = frozenset()
    requires_subtype: str = ""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.become_type import BecomeType
        from mtgz_ir.game import effective_subtypes

        src = state.resolving_source
        me = next(
            (o for o in state.battlefield if src is not None and o.id == src.id),
            None,
        )
        if me is None:
            return
        current = effective_subtypes(state, me)
        if self.requires_subtype and self.requires_subtype not in current:
            state.log.append(
                f"{me.printing.name} isn't a {self.requires_subtype}; nothing happens"
            )
            return
        state.add_continuous_effect(
            source_id=me.id, layer=4, kind="set_creature_subtypes",
            target_id=me.id, params=tuple(self.subtypes),
            description=f"{me.printing.name} becomes a {' '.join(self.subtypes)}",
        )
        BecomeType(base_pt=self.base_pt).resolve(state, controller_id, ())
        for kw in sorted(self.keywords):
            state.add_continuous_effect(
                source_id=me.id, layer=6, kind="grant_keyword",
                target_id=me.id, params=(kw,),
                description=f"{me.printing.name} has {kw}",
            )

    def describe(self) -> str:
        cond = f"if a {self.requires_subtype}, " if self.requires_subtype else ""
        return (
            f"{cond}becomes a {' '.join(self.subtypes)} "
            f"{self.base_pt[0]}/{self.base_pt[1]}"
        )


def graveyard_self_replacement_destination(state: "GameState", card: "GameObject") -> str:
    """``"graveyard"`` or where a self-replacement on ``card`` ("If
    <Name> would be put into a graveyard from anywhere, ... shuffle it
    into its owner's library instead.") redirects it."""
    from mtgz_ir.effects.modifiers import dispatch_replacement
    from mtgz_ir.effects.predicates import Event

    if not any(
        sa.replacement_effect is not None
        and sa.replacement_effect.event_pattern.event == Event.WOULD_GO_TO_GRAVEYARD
        for sa in card.printing.static_abilities
    ):
        return "graveyard"
    out = dispatch_replacement(
        state, Event.WOULD_GO_TO_GRAVEYARD,
        affected_player_id=card.owner_id,
        subjects={"card": card},
        payload={"destination": "graveyard"},
    )
    return out.get("destination", "graveyard")


def sweep_graveyard_watch(state: "GameState") -> bool:
    """State-based sweep over graveyard cards whose printing sets
    ``watched_in_graveyard`` (the "... put into a graveyard from
    anywhere" family; cheap no-op for every other card):

    * a "would be put into a graveyard from anywhere, ... shuffle it
      into its owner's library instead" permanent (Blightsteel Colossus)
      that got there by a path that doesn't consult the replacement
      (discard, mill, ...) is shuffled into its owner's library
      (``move_off_battlefield`` redirects before the card lands);
    * a card with a "When <this> is put into a graveyard from anywhere"
      trigger (Emrakul, the Aeons Torn) that newly arrived since the
      last sweep (``Player.graveyard_arrival_watch``) has that trigger
      put on the stack.
    """
    from mtgz_ir.game import Zone, shuffle_library

    changed = False
    for p in state.players:
        watched = [c for c in p.graveyard if c.printing.watched_in_graveyard]
        if not watched and not p.graveyard_arrival_watch:
            continue
        for c in watched:
            if (
                c.printing.is_permanent
                and graveyard_self_replacement_destination(state, c) == "library"
            ):
                p.graveyard.remove(c)
                c.zone = Zone.LIBRARY
                p.library.append(c)
                shuffle_library(state, p.id)
                state.log.append(
                    f"{c.printing.name} is shuffled into P{p.id}'s library "
                    f"instead of staying in the graveyard"
                )
                changed = True
        current = tuple(
            c.id for c in p.graveyard
            if c.printing.watched_in_graveyard
            and any(_is_graveyard_arrival_ability(a) for a in c.printing.triggered_abilities)
        )
        if current == p.graveyard_arrival_watch:
            continue
        previous = set(p.graveyard_arrival_watch)
        p.graveyard_arrival_watch = current
        for c in p.graveyard:
            if c.id not in current or c.id in previous:
                continue
            _push_graveyard_arrival_triggers(state, c)
            changed = True
    return changed


class PutIntoGraveyardFromAnywhere(_TriggerPredicate):
    """Marker predicate of a "When <this> is put into a graveyard from
    anywhere" trigger. Never matches a regular dispatch — the graveyard
    watch sweep stacks these abilities itself when the card arrives."""

    def matches(self, state, subjects, source):
        return False

    def __eq__(self, other):
        return isinstance(other, PutIntoGraveyardFromAnywhere)

    def __hash__(self):
        return hash("PutIntoGraveyardFromAnywhere")

    def __repr__(self):
        return "PutIntoGraveyardFromAnywhere()"


def _is_graveyard_arrival_ability(ability) -> bool:
    return isinstance(ability.predicate, PutIntoGraveyardFromAnywhere)


def _push_graveyard_arrival_triggers(state, card) -> None:
    from mtgz_ir.actions import push_triggered_ability

    for idx, ability in enumerate(card.printing.triggered_abilities):
        if _is_graveyard_arrival_ability(ability):
            push_triggered_ability(
                state, source=card, ability=ability, ability_index=idx,
                controller_id=card.owner_id, label="put into graveyard",
            )


@dataclass(frozen=True)
class OwnerShufflesGraveyardIntoLibrary(Effect):
    """"its owner shuffles their graveyard into their library" — the
    source card's owner (Emrakul, the Aeons Torn)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, shuffle_library

        src = state.resolving_source
        owner_id = src.owner_id if src is not None else controller_id
        p = state.player_by_id(owner_id)
        cards = list(p.graveyard)
        for c in cards:
            p.graveyard.remove(c)
            c.zone = Zone.LIBRARY
            p.library.append(c)
        shuffle_library(state, owner_id)
        state.log.append(
            f"P{owner_id} shuffles their graveyard ({len(cards)} cards) into "
            f"their library"
        )

    def describe(self) -> str:
        return "its owner shuffles their graveyard into their library"


# ---------------------------------------------------------------------------
# Batch 5: annihilator, monstrosity, connive, sacrifice-a-permanent,
# activation cost reduction, Lurrus's graveyard casting
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DefendingPlayerSacrificesPermanents(Effect):
    """Annihilator N (CR 702.86a): "Whenever this creature attacks,
    defending player sacrifices N permanents." The defending player
    (the single opponent) chooses which (``choose_sacrifice``)."""

    count: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import sacrifice

        opp = next(
            (pl for pl in state.players
             if pl.id != controller_id and not pl.has_lost),
            None,
        )
        if opp is None:
            return
        for _ in range(self.count):
            options = [o for o in state.battlefield if o.controller_id == opp.id]
            if not options:
                break
            pick = options[0]
            if opp.agent is not None:
                picked = opp.agent.choose_sacrifice(
                    state, opp.id, [o.id for o in options],
                )
                pick = next((o for o in options if o.id == picked), options[0])
            state.log.append(f"P{opp.id} sacrifices {pick.printing.name} (annihilator)")
            sacrifice(state, pick)

    def describe(self) -> str:
        return f"defending player sacrifices {self.count} permanents"


@dataclass(frozen=True)
class Monstrosity(Effect):
    """Monstrosity N (CR 701.37): "If this creature isn't monstrous, put
    N +1/+1 counters on it and it becomes monstrous." Becoming monstrous
    dispatches ``Event.BECOMES_MONSTROUS``."""

    count: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import dispatch_event
        from mtgz_ir.effects.predicates import Event
        from mtgz_ir.game import add_plus_one_counters

        src = state.resolving_source
        me = next(
            (o for o in state.battlefield if src is not None and o.id == src.id),
            None,
        )
        if me is None or me.monstrous:
            return
        add_plus_one_counters(state, me, self.count)
        me.monstrous = True
        state.log.append(f"{me.printing.name} becomes monstrous")
        dispatch_event(state, Event.BECOMES_MONSTROUS, label="monstrous", obj=me)

    def describe(self) -> str:
        return f"monstrosity {self.count}"


@dataclass(frozen=True)
class CountAttackingCreatures(_Formula):
    """"the number of attacking creatures"."""

    def value(self, state, controller_id):
        return sum(1 for o in state.battlefield if o.attacking)


@dataclass(frozen=True)
class TargetConnives(Effect):
    """"target <creature> connives X" (CR 701.50): its controller draws
    X cards, then discards X cards; for each nonland card discarded this
    way, put a +1/+1 counter on that creature (if it's still on the
    battlefield — the draw and discard happen regardless)."""

    amount: object = None
    targets: tuple = ()
    attacking_only: bool = True

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsAttacking, IsCreature
        from mtgz_ir.effects.targets import TargetSpec

        pred = IsCreature() & IsAttacking() if self.attacking_only else IsCreature()
        object.__setattr__(self, "targets", (TargetSpec(pred),))

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import add_plus_one_counters
        from mtgz_ir.turn import draw_n_cards

        for _kind, tid in targets:
            obj = next((o for o in state.battlefield if o.id == tid), None)
            if obj is None:
                return
            x = self.amount.value(state, controller_id)
            who = obj.controller_id
            draw_n_cards(state, who, x)
            discarded = _discard_chosen(state, who, x)
            nonland = sum(1 for c in discarded if not c.printing.is_land)
            if nonland and obj in state.battlefield:
                add_plus_one_counters(state, obj, nonland)
            state.log.append(
                f"{obj.printing.name} connives {x} ({nonland} counter(s))"
            )

    def describe(self) -> str:
        return "target attacking creature connives X"


@dataclass(frozen=True)
class SacrificeAPermanent(Effect):
    """"sacrifice [another ]<permanent descriptor>" as an effect (the
    first half of "You may sacrifice ... If you do, ..." — Gut, True-Soul
    Zealot). Returns False when there was nothing to sacrifice."""

    permanent_filter: object = None
    another: bool = False
    label: str = "a permanent"

    def resolve(self, state, controller_id, targets=()):
        cost = SacrificePermanentsCost(
            permanent_filter=self.permanent_filter, another=self.another,
            label=self.label,
        )
        src = state.resolving_source
        if not cost.payable(state, controller_id, src):
            return False
        cost.pay(state, controller_id, src)
        return True

    def describe(self) -> str:
        return f"sacrifice {self.label}"


def activation_cost_reduction(state: "GameState", activator_id: int, ability, cost):
    """Zirda, the Dawnwaker: "Abilities you activate that aren't mana
    abilities cost {N} less to activate. This effect can't reduce the
    mana in that cost to less than one mana." Returns ``cost`` reduced by
    every such static the activator controls."""
    if cost is None or ability.is_mana_ability:
        return cost
    n = 0
    for o in state.battlefield:
        if o.controller_id != activator_id:
            continue
        for sa in o.printing.static_abilities:
            n += sa.activation_cost_reduction
    if n <= 0:
        return cost
    reducible = min(n, cost.generic, max(0, cost.total - 1))
    return cost.reduced(reducible) if reducible > 0 else cost


def lurrus_casters(state: "GameState", player_id: int) -> list:
    """Permanents ``player_id`` controls whose "Once during each of your
    turns, you may cast a permanent spell with mana value N or less from
    your graveyard" permission is unused this turn (it's only usable
    during their own turn)."""
    if state.active_player().id != player_id:
        return []
    return [
        o for o in state.battlefield
        if o.controller_id == player_id
        and any(sa.graveyard_permanent_cast_mv_max >= 0 for sa in o.printing.static_abilities)
        and o.graveyard_cast_used_turn != state.turn_number
    ]


def lurrus_castable(state: "GameState", player_id: int, card) -> "GameObject | None":
    """The unused Lurrus-style permission that lets ``player_id`` cast
    ``card`` (a permanent card in their graveyard), or ``None``."""
    pr = card.printing
    if not pr.is_permanent or pr.is_land:
        return None
    for o in lurrus_casters(state, player_id):
        cap = max(
            sa.graveyard_permanent_cast_mv_max for sa in o.printing.static_abilities
        )
        if pr.mana_cost.total <= cap:
            return o
    return None


from mtgz_ir.effects.state_predicates import StatePredicate as _StatePredicate  # noqa: E402


@dataclass(frozen=True)
class OpponentHasMoreCardsInHand(_StatePredicate):
    """"if defending player has more cards in hand than you" (Robber of
    the Rich) — the viewer's (single) opponent holds more cards."""

    def test(self, state, viewer_id):
        mine = len(state.player_by_id(viewer_id).hand)
        return any(
            len(p.hand) > mine for p in state.players
            if p.id != viewer_id and not p.has_lost
        )


# ---------------------------------------------------------------------------
# Batch 6: "becomes the target" (Leovold, Nadu), real spell copies (Lutri),
# Goblin Welder's swap, "one or more" discard batches (Inti)
# ---------------------------------------------------------------------------


_PERMANENT_TARGET_KINDS = frozenset({
    "creature", "permanent", "my_creature", "untapped_permanent",
    "planeswalker", "battle",
})


def fire_becomes_target(
    state: "GameState", targets, source_obj, controller_id: int,
) -> None:
    """A spell or ability controlled by ``controller_id`` (source
    ``source_obj``) just got its targets: dispatch
    ``Event.BECOMES_TARGET`` once per distinct targeted player /
    permanent, and stack the "Whenever this creature becomes the target
    ..." abilities granted by statics (Nadu, Winged Wisdom).

    A targeted spell on the stack (``"stack"`` / ``"stack_spell"``
    targets) dispatches with the spell card as the ``target_spell``
    subject instead of ``target`` -- so "a permanent you control
    becomes the target" observers never see it, while "a creature
    spell you control becomes the target" ones (Surrak, Elusive Hunter;
    cube W/G bucket) do."""
    from mtgz_ir.actions import dispatch_event
    from mtgz_ir.effects.predicates import Event

    if not targets:
        return
    caster = state.player_by_id(controller_id)
    # The targeting stack object (pushed just before this call): its
    # spell-vs-ability kind is the ``targeting`` subject ("becomes the
    # target of a spell", "of an ability you control"; roto P6).
    targeting = (
        state.stack[-1]
        if state.stack and state.stack[-1].source is source_obj else None
    )
    seen: set = set()
    for kind, tid in targets:
        if (kind, tid) in seen:
            continue
        seen.add((kind, tid))
        if kind == "player":
            subject = state.player_by_id(tid)
        elif kind in _PERMANENT_TARGET_KINDS:
            subject = next((o for o in state.battlefield if o.id == tid), None)
            if subject is None:
                continue
        elif kind in ("stack", "stack_spell"):
            spell = _targeted_spell(state, kind, tid)
            if spell is not None:
                dispatch_event(
                    state, Event.BECOMES_TARGET, label="becomes-target",
                    target_spell=spell, controller=caster,
                    targeting=targeting,
                )
            continue
        else:  # graveyard / exile / card targets aren't permanents
            continue
        dispatch_event(
            state, Event.BECOMES_TARGET, label="becomes-target",
            target=subject, controller=caster, targeting=targeting,
        )
        if kind != "player":
            _fire_granted_becomes_target_triggers(state, subject)


def _targeted_spell(state, kind: str, tid: int):
    """The spell card a ``"stack"`` (stack index) / ``"stack_spell"``
    (card id) target names, or ``None`` (an ability, or gone)."""
    if kind == "stack":
        if not 0 <= tid < len(state.stack):
            return None
        item = state.stack[tid]
    else:
        item = next((i for i in state.stack if i.source.id == tid), None)
    if item is None or item.ability is not None:
        return None
    return item.source


def _fire_granted_becomes_target_triggers(state, creature) -> None:
    """Statics that grant creatures 'Whenever this creature becomes the
    target of a spell or ability, <effect>. This ability triggers only
    N times each turn.' (``StaticAbility.granted_becomes_target_effect``,
    with the per-turn cap in ``granted_trigger_limit_per_turn``)."""
    from mtgz_ir.actions import push_triggered_ability
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import Event, IsSelf

    for source in state.battlefield:
        for idx, sa in enumerate(source.printing.static_abilities):
            eff = sa.granted_becomes_target_effect
            if eff is None or not sa.applies_to(source, creature, state):
                continue
            key = ("granted_becomes_target", source.id, idx, creature.id)
            fired = state.ability_resolutions_this_turn.get(key, 0)
            if sa.granted_trigger_limit_per_turn and fired >= sa.granted_trigger_limit_per_turn:
                continue
            state.ability_resolutions_this_turn[key] = fired + 1
            push_triggered_ability(
                state, source=creature,
                ability=TriggeredAbility(
                    event=Event.BECOMES_TARGET, predicate=IsSelf(subject="target"),
                    effect=eff,
                ),
                ability_index=3000 + idx,
                controller_id=creature.controller_id,
                label="becomes-target (granted)",
            )


@dataclass(frozen=True)
class RevealTopLandToBattlefieldElseHand(Effect):
    """"Reveal the top card of your library. If it's a land card, put it
    onto the battlefield. Otherwise, put it into your hand." (Nadu's
    granted trigger)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        if not p.library:
            return
        card = p.library.pop(0)
        reveal_to_all(state, card)
        if card.printing.is_land:
            state.log.append(f"P{controller_id} reveals {card.printing.name}; onto the battlefield")
            put_into_battlefield(state, card, controller_id)
        else:
            state.log.append(f"P{controller_id} reveals {card.printing.name}; into hand")
            card.zone = Zone.HAND
            p.hand.append(card)

    def describe(self) -> str:
        return "reveal the top card; a land goes onto the battlefield, else into your hand"


@dataclass(frozen=True)
class CopyTargetInstantOrSorcerySpellYouControl(Effect):
    """"Copy target instant or sorcery spell you control. You may choose
    new targets for the copy." — a real copy on the stack
    (``push_spell_copy``, CR 707.10), unlike the Mirrorpool
    short-circuit ``CopyTargetInstantOrSorceryYouControl``."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("stack_spell", item.source.id) for item in state.stack
            if item.ability is None
            and item.controller_id == controller_id
            and (item.source.printing.is_instant or item.source.printing.is_sorcery)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import push_spell_copy
        from mtgz_ir.effects.generic.spell_copies import _choose_copy_targets

        for kind, sid in targets:
            item = next(
                (i for i in state.stack if i.ability is None and i.source.id == sid),
                None,
            )
            if kind != "stack_spell" or item is None:
                continue
            new_targets = _choose_copy_targets(
                state, controller_id, item.source.printing, item.targets,
            )
            push_spell_copy(
                state, item.source.printing, controller_id, new_targets,
                x_value=item.x_value,
            )

    def describe(self) -> str:
        return "copy target instant or sorcery spell you control"


@dataclass(frozen=True)
class EnteredViaCast(_TriggerPredicate):
    """Intervening-if "if you cast it" on an ETB trigger: the source
    permanent resolved from a spell (``GameObject.entered_via_cast``)."""

    def matches(self, state, subjects, source):
        return bool(getattr(source, "entered_via_cast", False))


@dataclass(frozen=True)
class SwapArtifactWithGraveyardArtifact(Effect):
    """Goblin Welder: "Choose target artifact a player controls and
    target artifact card in that player's graveyard. If both targets are
    still legal as this ability resolves, that player simultaneously
    sacrifices the artifact and returns the artifact card to the
    battlefield." Slot 1's pool depends on slot 0's controller
    (``legal_targets_for_slot`` receives the targets chosen so far)."""

    slot_targets_depend_on_chosen = True

    def requires_target(self) -> bool:
        return True

    def target_count_range(self):
        return (2, 2)

    def _artifact_ok(self, state, o) -> bool:
        from mtgz_ir.game import is_artifact_now

        pl = state.player_by_id(o.controller_id)
        return is_artifact_now(state, o) and any(
            is_artifact_now(state, c) for c in pl.graveyard
        )

    def legal_targets(self, state, controller_id):
        return [
            ("permanent", o.id) for o in state.battlefield if self._artifact_ok(state, o)
        ]

    def legal_targets_for_slot(self, state, controller_id, slot_idx, chosen=()):
        if slot_idx == 0:
            return self.legal_targets(state, controller_id)
        if not chosen:
            return []
        first = next((o for o in state.battlefield if o.id == chosen[0][1]), None)
        if first is None:
            return []
        from mtgz_ir.game import is_artifact_now

        pl = state.player_by_id(first.controller_id)
        return [
            ("graveyard_card", c.id) for c in pl.graveyard
            if is_artifact_now(state, c)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.turn import sacrifice

        if len(targets) < 2:
            return
        from mtgz_ir.game import is_artifact_now

        art = next(
            (o for o in state.battlefield
             if o.id == targets[0][1] and is_artifact_now(state, o)),
            None,
        )
        if art is None:
            return
        player = state.player_by_id(art.controller_id)
        card = next(
            (c for c in player.graveyard
             if c.id == targets[1][1] and is_artifact_now(state, c)),
            None,
        )
        if card is None:
            return
        state.log.append(
            f"P{player.id} swaps {art.printing.name} for {card.printing.name}"
        )
        player.graveyard.remove(card)
        sacrifice(state, art)
        put_into_battlefield(state, card, player.id)

    def describe(self) -> str:
        return "swap target artifact with target artifact card in its controller's graveyard"


@dataclass(frozen=True)
class OneOrMoreBatch(_TriggerPredicate):
    """Conjunct for "Whenever you <do something to> one or more cards"
    observers fed by a per-card event (Inti's discards): no second
    instance while an instance from the same source is still waiting on
    the stack (the per-card dispatches of one discard all happen before
    anyone gets priority).

    ``per_target_player``: "one or more creatures you control deal combat
    damage to a player" (Haliya, Elegy Acolyte) triggers once for each
    player dealt damage at the same time; only a waiting instance about
    the same player (the event's ``target`` subject, the item's fixed
    player target) blocks another."""

    per_target_player: bool = False

    def matches(self, state, subjects, source):
        player = subjects.get("target") if self.per_target_player else None
        for item in state.stack:
            if item.source is source and item.ability is not None and _contains_identity(
                item.ability.predicate, self,
            ):
                if player is not None and item.targets != (
                    ("player", player.id),
                ):
                    continue
                return False
        return True


def _contains_identity(pred, needle) -> bool:
    from mtgz_ir.effects.predicates import And

    stack = [pred]
    while stack:
        p = stack.pop()
        if p is needle:
            return True
        if isinstance(p, And):
            stack.extend((p.left, p.right))
    return False


# ---------------------------------------------------------------------------
# Batch 7: library/graveyard -> exile watcher (Laelia), die rolls
# ("Name Sticker" Goblin), multikicker (Batroc the Leaper)
# ---------------------------------------------------------------------------


class CardsExiledFromLibraryOrGraveyard(_TriggerPredicate):
    """Marker predicate of "Whenever one or more cards are put into exile
    from your library and/or your graveyard" (Laelia). Never matches a
    regular dispatch; ``sweep_library_graveyard_exile_watch`` stacks the
    ability once per state-based-action pass that sees one or more of
    the controller's library / graveyard cards newly in exile."""

    def matches(self, state, subjects, source):
        return False

    def __eq__(self, other):
        return isinstance(other, CardsExiledFromLibraryOrGraveyard)

    def __hash__(self):
        return hash("CardsExiledFromLibraryOrGraveyard")

    def __repr__(self):
        return "CardsExiledFromLibraryOrGraveyard()"


def sweep_library_graveyard_exile_watch(state: "GameState") -> bool:
    """For every permanent with a library/graveyard-exile watcher
    (``CardPrinting.watches_library_graveyard_exile``): if some card that
    was in its controller's library or graveyard at the previous sweep
    is now in exile, stack the watcher's trigger (one per sweep = one per
    batch). ``Player.library_graveyard_watch`` holds the snapshot and is
    cleared while no watcher is on the battlefield."""
    from mtgz_ir.actions import push_triggered_ability

    watchers = [o for o in state.battlefield if o.printing.watches_library_graveyard_exile]
    fired = False
    for p in state.players:
        mine = [o for o in watchers if o.controller_id == p.id]
        if not mine:
            if p.library_graveyard_watch:
                p.library_graveyard_watch = frozenset()
            continue
        prev = p.library_graveyard_watch
        current = frozenset(c.id for c in p.library) | frozenset(
            c.id for c in p.graveyard
        )
        moved = bool(prev) and any(c.id in prev for c in p.exile)
        if current != prev:
            p.library_graveyard_watch = current
        if not moved:
            continue
        for watcher in mine:
            for idx, ability in enumerate(watcher.printing.triggered_abilities):
                if isinstance(ability.predicate, CardsExiledFromLibraryOrGraveyard):
                    fired = push_triggered_ability(
                        state, source=watcher, ability=ability,
                        ability_index=idx, controller_id=p.id,
                        label="cards exiled",
                    ) or fired
    return fired


@dataclass(frozen=True)
class RollDieTable(Effect):
    """"Roll a d<N>." followed by a result table ``((lo, hi, effect),
    ...)`` ("1-6 | Add {R}{R}{R}{R}.") — the matching row's effect
    resolves. ``gate`` (a ``Condition``) is rechecked on resolution
    ("if it's on the battlefield and you control 9 or fewer ...")."""

    sides: int = 20
    table: tuple = ()
    gate: object = None

    def resolve(self, state, controller_id, targets=()):
        if self.gate is not None and not self.gate.matches(state, controller_id):
            return
        roll = state.rng.randint(1, self.sides)
        state.log.append(f"P{controller_id} rolls a d{self.sides}: {roll}")
        for lo, hi, effect in self.table:
            if lo <= roll <= hi:
                effect.resolve(state, controller_id, targets)
                return

    def describe(self) -> str:
        return f"roll a d{self.sides}"


@dataclass(frozen=True)
class NameStickerGate(_Condition):
    """"if it's on the battlefield and you control N or fewer creatures
    named <name>" — rechecked on resolution against the resolving
    source."""

    name: str = ""
    at_most: int = 9

    def matches(self, state, viewer_id, chosen_targets=None):
        from mtgz_ir.game import is_creature_now

        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return False
        n = sum(
            1 for o in state.battlefield
            if o.controller_id == viewer_id and is_creature_now(o)
            and o.printing.name == self.name
        )
        return n <= self.at_most


@dataclass(frozen=True)
class EnteredNotFromZones(_TriggerPredicate):
    """"enters from anywhere other than a graveyard or exile" — the
    entering source's previous zone (``GameObject.entered_from_zone``)
    isn't one of ``zones``."""

    zones: tuple[ZoneName, ...] = (ZoneName.GRAVEYARD, ZoneName.EXILE)

    def __post_init__(self) -> None:
        object.__setattr__(self, "zones", coerce_tuple(ZoneName, self.zones))

    def matches(self, state, subjects, source):
        return getattr(source, "entered_from_zone", "") not in self.zones


@dataclass(frozen=True)
class ControlsAtMostNamed(_TriggerPredicate):
    """Trigger-time half of the "you control N or fewer creatures named
    <name>" gate."""

    name: str = ""
    at_most: int = 9

    def matches(self, state, subjects, source):
        from mtgz_ir.game import is_creature_now

        n = sum(
            1 for o in state.battlefield
            if o.controller_id == source.controller_id and is_creature_now(o)
            and o.printing.name == self.name
        )
        return n <= self.at_most


@dataclass(frozen=True)
class DealPowerDamageToEachOfUpToKickedTargets(Effect):
    """Batroc the Leaper: "<it> deals damage equal to <its> power to each
    of up to X targets, where X is the number of times <it> was kicked."
    The target cap is read off the source (``GameObject.kick_count``) by
    ``dynamic_target_count_range`` when the trigger is put on the
    stack."""

    targets: tuple = ()

    def __post_init__(self) -> None:
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter

        object.__setattr__(self, "targets", (TargetSpec(
            any_target_filter(), count_min=0, count_max=1,
        ),))

    def dynamic_target_count_range(self, state, source) -> tuple[int, int]:
        return (0, max(0, getattr(source, "kick_count", 0)))

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.game import effective_power

        src = state.resolving_source
        me = next(
            (o for o in state.battlefield if src is not None and o.id == src.id),
            None,
        )
        power = effective_power(state, me) if me is not None else (
            (src.printing.power or 0) + src.plus_one_counters if src is not None else 0
        )
        if power <= 0:
            return
        spec = self.targets[0]
        from mtgz_ir.effects.generic.targets_helpers import _slot_target_still_legal

        for chosen in dict.fromkeys(targets):
            if _slot_target_still_legal(state, spec, chosen):
                _apply_damage(state, chosen, power)

    def describe(self) -> str:
        return "deals damage equal to its power to each of up to X targets"


def choose_extra_kicks(state: "GameState", player_id: int, card, cost):
    """Multikicker (CR 702.33c): ``cost`` already includes one kicker
    payment; ask the caster (``choose_x`` over the affordable range, capped
    at 10) how many additional times to pay it. Returns ``(new_cost,
    extra)``."""
    from mtgz_ir.actions import can_pay_cost
    from mtgz_ir.alt_casting import cost_plus

    kc = card.printing.kicker_cost
    upper = 0
    total = cost
    while upper < 10:
        nxt = cost_plus(total, kc)
        if not can_pay_cost(state, player_id, nxt, sac_for_mana=True):
            break
        total = nxt
        upper += 1
    extra = 0
    agent = state.player_by_id(player_id).agent
    if agent is not None and upper > 0:
        extra = max(0, min(upper, agent.choose_x(
            state, player_id, f"{card.printing.name} (multikicker)", upper,
        )))
    for _ in range(extra):
        cost = cost_plus(cost, kc)
    if extra:
        state.log.append(
            f"P{player_id} kicks {card.printing.name} {extra} more time(s)"
        )
    return cost, extra
