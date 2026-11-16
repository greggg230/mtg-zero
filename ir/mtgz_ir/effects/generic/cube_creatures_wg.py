"""Effects for the cube W/G creature bucket.

Each class models one printed effect shape claimed by
:mod:`mtgz_ir.cards.parser.creature_wg_effects`; the docstrings name the
cards that print it.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target, TriggeredAbility
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.object_predicates import ObjectPredicate
from mtgz_ir.effects.predicates import TriggerPredicate
from mtgz_ir.effects.pt_formulas import PTFormula

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


def _living_players(state: "GameState") -> list[Target]:
    return [("player", p.id) for p in state.players if not p.has_lost]


def _chosen_player(state: "GameState", targets):
    """The still-legal target player (``None`` when absent / lost)."""
    for kind, pid in targets:
        if kind == "player":
            p = state.player_by_id(pid)
            return None if p.has_lost else p
    return None


@dataclass(frozen=True)
class TargetPlayerPutsGraveyardOnBottomRandom(Effect):
    """"[Up to one] target player puts all the cards from their graveyard
    on the bottom of their library in a random order." (Endurance.)"""

    up_to_one: bool = True

    def requires_target(self) -> bool:
        return not self.up_to_one

    def target_count_range(self) -> tuple[int, int]:
        return (0 if self.up_to_one else 1, 1)

    def legal_targets(self, state, controller_id):
        return _living_players(state)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reset_public_knowledge

        p = _chosen_player(state, targets)
        if p is None:
            return
        cards = list(p.graveyard)
        p.graveyard.clear()
        state.rng.shuffle(cards)
        for c in cards:
            c.zone = Zone.LIBRARY
            reset_public_knowledge(c)
            p.library.append(c)
        state.log.append(
            f"P{p.id} puts {len(cards)} graveyard card(s) on the bottom of "
            f"their library"
        )

    def describe(self) -> str:
        who = "up to one target player" if self.up_to_one else "target player"
        return (
            f"{who} puts all the cards from their graveyard on the bottom of "
            f"their library in a random order"
        )


# ---------------------------------------------------------------------
# Linked "exiled with this permanent" abilities (CR 607.2a)
# ---------------------------------------------------------------------


def exile_link_key(source) -> tuple:
    """The ``GameObject.imprinted_on`` value naming ``source``'s current
    battlefield incarnation (the base's imprint / "exiled with" link)."""
    return (source.id, source.etb_timestamp)


def cards_exiled_with(state: "GameState", source) -> list:
    """Cards in exile that ``source``'s linked exiling ability put there
    (this incarnation of ``source`` only)."""
    from mtgz_ir.effects.generic.artifact_mechanics import imprinted_cards

    return imprinted_cards(state, source)


@dataclass(frozen=True)
class LinkExiledCardsToSource(Effect):
    """Wraps the exiling half of a linked ability pair (CR 607.2a): every
    card the inner effect moves into exile is tagged as "exiled with"
    the resolving source (``GameObject.imprinted_on``), so the other half
    ("the exiled card", "cards exiled with this creature") can find it.

    Targeting delegates to ``inner`` unchanged."""

    inner: Effect

    @property
    def targets(self):  # type: ignore[override]
        return self.inner.targets

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(self, state, controller_id):
        return self.inner.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.inner.target_count_range()

    def target_slot_kinds(self):
        return self.inner.target_slot_kinds()

    def resolve(self, state, controller_id, targets=()):
        source = state.resolving_source
        before = {c.id for p in state.players for c in p.exile}
        result = self.inner.resolve(state, controller_id, targets)
        if source is not None:
            key = exile_link_key(source)
            for p in state.players:
                for c in p.exile:
                    if c.id not in before:
                        c.imprinted_on = key
        return result

    def describe(self) -> str:
        return self.inner.describe()


@dataclass(frozen=True)
class ExiledCardOwnerCreatesManaValueToken(Effect):
    """"The exiled card's owner creates an X/X <color> <Subtype> creature
    token, where X is the mana value of the exiled card." (Skyclave
    Apparition's leaves-the-battlefield half.) Reads the cards linked to
    the source's last battlefield incarnation and unlinks them."""

    consumes_exile_link = True

    color: str = "U"
    subtype: str = "Illusion"

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.cards.base import make_token_creature
        from mtgz_ir.effects.generic.tokens import CreateToken

        source = state.resolving_source
        if source is None:
            return
        for card in cards_exiled_with(state, source):
            card.imprinted_on = None
            x = card.printing.mana_cost.total
            printing = make_token_creature(
                name=self.subtype, power=x, toughness=x,
                subtypes=(self.subtype,), colors=(self.color,),
            )
            CreateToken(token_printing=printing).resolve(state, card.owner_id)

    def describe(self) -> str:
        return (
            f"the exiled card's owner creates an X/X {self.subtype} token, "
            f"where X is its mana value"
        )


# ---------------------------------------------------------------------
# "Exile ... until an opponent becomes the monarch" (Palace Jailer)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class ExileUntilOpponentBecomesMonarch(Effect):
    """"Exile target <permanent> until an opponent becomes the monarch."
    The exile ends -- the card returns under its owner's control -- the
    next time a player other than this ability's controller becomes the
    monarch (``designations.become_monarch`` calls
    :func:`release_exiled_until_opponent_monarch`), whether or not the
    source is still on the battlefield."""

    targets: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import move_off_battlefield

        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            for slot, chosen in zip(self.targets, targets):
                if not _slot_target_still_legal(state, slot, chosen):
                    continue
                obj = next(o for o in state.battlefield if o.id == chosen[1])
                obj.exiled_until_opponent_monarch_of = controller_id
                state.log.append(
                    f"{obj.printing.name} is exiled until an opponent of "
                    f"P{controller_id} becomes the monarch"
                )
                move_off_battlefield(state, obj, destination="exile")
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return "exile target permanent until an opponent becomes the monarch"


def release_exiled_until_opponent_monarch(
    state: "GameState", new_monarch_id: int,
) -> None:
    """``new_monarch_id`` just became the monarch: return every card
    exiled "until an opponent becomes the monarch" by one of their
    opponents to the battlefield under its owner's control."""
    from mtgz_ir.actions import put_into_battlefield
    from mtgz_ir.turn import _reset_left_battlefield_object

    for p in state.players:
        for c in list(p.exile):
            exiler = c.exiled_until_opponent_monarch_of
            if exiler is None or exiler == new_monarch_id:
                continue
            c.exiled_until_opponent_monarch_of = None
            p.exile.remove(c)
            c.exiled_face_down = False  # left exile (CR 400.7)
            state.log.append(
                f"{c.printing.name} returns from exile (P{new_monarch_id} "
                f"became the monarch)"
            )
            _reset_left_battlefield_object(c)
            put_into_battlefield(state, c, p.id)


# ---------------------------------------------------------------------
# Ravenous / Mobilize / tokens scaled by power
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class EnteringWithXAtLeast(TriggerPredicate):
    """Trigger gate: the entering spell's X (``state.resolving_x_value``
    while it resolves onto the battlefield) is at least ``n`` -- the
    Ravenous "If X is 5 or more, draw a card when it enters." rider. A
    permanent that enters without being cast has X = 0."""

    n: int = 5

    def matches(self, state, subjects, source):
        return getattr(state, "resolving_x_value", 0) >= self.n


def ravenous_draw_trigger() -> TriggeredAbility:
    """Ravenous (CR 702.156): "This creature enters with X +1/+1 counters
    on it. If X is 5 or more, draw a card when it enters." The counters
    are the printing's ``enters_with_counters_from_x="x"``; this is the
    draw half."""
    from mtgz_ir.effects.generic.draw_mill import DrawCards
    from mtgz_ir.effects.predicates import Event, IsSelf

    return TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & EnteringWithXAtLeast(5),
        effect=DrawCards(amount=1),
    )


def _new_objects(state: "GameState", before: set) -> list:
    return [o for o in state.battlefield if o.id not in before]


@dataclass(frozen=True)
class Mobilize(Effect):
    """Mobilize N (CR 702.181): "Whenever this creature attacks, create N
    tapped and attacking 1/1 red Warrior creature tokens. Sacrifice them
    at the beginning of the next end step." -- the attack trigger's
    effect. Token doublers apply; each created token gets its own
    delayed sacrifice trigger."""

    n: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.cards.base import make_token_creature
        from mtgz_ir.effects.generic.delayed_actions import (
            SacrificeSourceDelayed,
            schedule_at_next_end_step,
        )
        from mtgz_ir.effects.generic.tokens import CreateToken

        before = {o.id for o in state.battlefield}
        warrior = make_token_creature(
            name="Warrior", power=1, toughness=1, subtypes=("Warrior",),
            colors=("R",),
        )
        CreateToken(
            token_printing=warrior, count=self.n, tapped=True, attacking=True,
        ).resolve(state, controller_id)
        for tok in _new_objects(state, before):
            schedule_at_next_end_step(
                state, controller_id, tok.id, SacrificeSourceDelayed(),
            )

    def describe(self) -> str:
        return (
            f"mobilize {self.n}: create {self.n} tapped and attacking 1/1 red "
            f"Warrior tokens; sacrifice them at the next end step"
        )


@dataclass(frozen=True)
class CreateTokensEqualToSourcePower(Effect):
    """"Create a number of <token> tokens equal to this creature's power."
    (Jacked Rabbit.) Reads the source's current power (last known power
    if it has left the battlefield)."""

    token_printing: "object" = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.tokens import CreateToken
        from mtgz_ir.game import effective_power

        source = state.resolving_source
        if source is None:
            return
        n = max(0, effective_power(state, source))
        if n:
            CreateToken(token_printing=self.token_printing, count=n).resolve(
                state, controller_id,
            )

    def describe(self) -> str:
        return (
            f"create a number of {self.token_printing.name} tokens equal to "
            f"this creature's power"
        )


# ---------------------------------------------------------------------
# Card flow (Loran, Stoneforge Mystic)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class YouAndTargetOpponentEachDraw(Effect):
    """"You and target opponent each draw N cards." (Loran of the Third
    Path.)"""

    amount: int = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        draw_n_cards(state, controller_id, self.amount)
        opp = _chosen_player(state, targets)
        if opp is not None:
            draw_n_cards(state, opp.id, self.amount)

    def describe(self) -> str:
        return f"you and target opponent each draw {self.amount} card(s)"


@dataclass(frozen=True)
class PutCardFromHandOntoBattlefield(Effect):
    """"[You may ]put <a filter> card from your hand onto the battlefield."
    (Stoneforge Mystic's Equipment.) The card is picked with
    ``choose_card_from_library`` (the zone-agnostic pick hook the other
    put-from-hand effects use); with ``optional`` the pick may be
    declined (``None``)."""

    card_filter: "object" = None
    optional: bool = True
    tapped: bool = False

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield

        p = state.player_by_id(controller_id)
        options = [c for c in p.hand if self.card_filter.matches_card(state, c)]
        if not options:
            return False
        picked = None
        if p.agent is not None:
            picked = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
        card = next((c for c in options if c.id == picked), None)
        if card is None:
            if self.optional:
                state.log.append(
                    f"P{controller_id} puts nothing onto the battlefield"
                )
                return False
            card = options[0]
        p.hand.remove(card)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the battlefield"
        )
        put_into_battlefield(state, card, controller_id)
        if self.tapped:
            card.tapped = True
        return True

    def describe(self) -> str:
        may = "you may " if self.optional else ""
        return (
            f"{may}put {self.card_filter.describe()} from your hand onto the "
            f"battlefield"
        )


# ---------------------------------------------------------------------
# Token copies of the source (Vaultborn Tyrant)
# ---------------------------------------------------------------------


def token_copy_printing(base, extra_card_types: tuple = ()):
    """A token printing copying ``base``'s copiable values, with the
    Token supertype and ``extra_card_types`` ("except it's an artifact in
    addition to its other types") added."""
    import dataclasses as _dc

    supertypes = tuple(base.supertypes) + (
        () if "Token" in base.supertypes else ("Token",)
    )
    card_types = tuple(
        t for t in extra_card_types if t not in base.card_types
    ) + tuple(base.card_types)
    left = " ".join(supertypes + card_types)
    type_line = f"{left} — {' '.join(base.subtypes)}" if base.subtypes else left
    return _dc.replace(
        base, supertypes=supertypes, card_types=card_types, type_line=type_line,
    )


def eternalized_printing(base):
    """Eternalize's token (CR 702.129a): "a copy of it, except it's a 4/4
    black Zombie <its other creature types> with no mana cost"."""
    import dataclasses as _dc

    from mtgz_ir.cards.base import ManaCost

    tok = token_copy_printing(base)
    subtypes = ("Zombie",) + tuple(s for s in tok.subtypes if s != "Zombie")
    left = tok.type_line.split(" — ")[0]
    return _dc.replace(
        tok, power=4, toughness=4, subtypes=subtypes,
        type_line=f"{left} — {' '.join(subtypes)}",
        mana_cost=ManaCost(), mana_cost_str="",
        color_indicator=frozenset({"B"}),
    )


@dataclass(frozen=True)
class CreateTokenCopyOfSource(Effect):
    """"Create a token that's a copy of it[, except it's an <type> in
    addition to its other types]." -- "it" being this ability's source
    (Vaultborn Tyrant's dies trigger: last known copiable values)."""

    extra_card_types: tuple = ()
    # Eternalize (Fanatic of Rhonas): "..., except it's a 4/4 black
    # Zombie <types> with no mana cost" (``eternalized_printing``).
    eternalize: bool = False

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.tokens import CreateToken

        source = state.resolving_source
        if source is None:
            return
        if self.eternalize:
            printing = eternalized_printing(source.printing)
        else:
            printing = token_copy_printing(source.printing, self.extra_card_types)
        CreateToken(token_printing=printing).resolve(state, controller_id)

    def describe(self) -> str:
        if self.eternalize:
            return (
                "create a token that's a copy of it, except it's a 4/4 black "
                "Zombie with no mana cost"
            )
        extra = " and ".join(t.lower() for t in self.extra_card_types)
        tail = f", except it's also {extra}" if extra else ""
        return f"create a token that's a copy of it{tail}"


# ---------------------------------------------------------------------
# Untapping (White Plume Adventurer)
# ---------------------------------------------------------------------


def _matching_permanents(state, controller_id, permanent_filter) -> list:
    prior = getattr(state, "resolving_controller_id", None)
    state.resolving_controller_id = controller_id
    try:
        return [o for o in state.battlefield if permanent_filter.test(o, state)]
    finally:
        state.resolving_controller_id = prior


@dataclass(frozen=True)
class UntapChosenPermanents(Effect):
    """"Untap a <permanent>." -- untargeted: the controller picks ``n``
    matching permanents (via ``choose_permanent_to_tap``; a declined
    pick falls back to the first tapped option, since the printed "a"
    isn't optional). Untapping an untapped permanent is a no-op, so the
    tapped ones are offered."""

    n: int = 1
    permanent_filter: "object" = None

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        for _ in range(self.n):
            options = [
                o for o in _matching_permanents(
                    state, controller_id, self.permanent_filter,
                )
                if o.tapped
            ]
            if not options:
                return
            pick = None
            if p.agent is not None:
                pick = p.agent.choose_permanent_to_tap(
                    state, controller_id, [o.id for o in options],
                )
            obj = next((o for o in options if o.id == pick), options[0])
            obj.tapped = False
            state.log.append(f"P{controller_id} untaps {obj.printing.name}")

    def describe(self) -> str:
        return f"untap {self.n} chosen permanent(s)"


@dataclass(frozen=True)
class UntapAllMatching(Effect):
    """"Untap all <permanents>." (White Plume Adventurer's dungeon
    rider: "untap all creatures you control")."""

    permanent_filter: "object" = None

    def resolve(self, state, controller_id, targets=()):
        for o in _matching_permanents(state, controller_id, self.permanent_filter):
            o.tapped = False
        state.log.append(f"P{controller_id} untaps all matching permanents")

    def describe(self) -> str:
        return "untap all matching permanents"


# ---------------------------------------------------------------------
# Questing Beast / Worldspine Wurm
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class DealCombatDamageAmountToTarget(Effect):
    """"<It> deals that much damage to target <X>." -- the amount is the
    triggering combat damage (``state.resolving_combat_damage_amount``);
    Questing Beast's planeswalker redirect."""

    targets: tuple = ()

    def targets_from_event(self, state, subjects):
        """The trigger's own target (a planeswalker), not the damaged
        player the combat-damage dispatch would pre-wire: returning
        ``None`` lets ``push_triggered_ability`` prompt for it."""
        return None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        amount = state.resolving_combat_damage_amount
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            for slot, chosen in zip(self.targets, targets):
                if amount > 0 and _slot_target_still_legal(state, slot, chosen):
                    _apply_damage(state, chosen, amount)
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return "deals that much damage to the target"


@dataclass(frozen=True)
class ShuffleSourceIntoOwnersLibrary(Effect):
    """"Shuffle it into its owner's library." -- the resolving source
    card, if it's still in its owner's graveyard (Worldspine Wurm's
    put-into-a-graveyard trigger, stacked by the R/multi/colorless
    bucket's ``cube_creatures_rmc.sweep_graveyard_watch``)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, shuffle_library

        card = state.resolving_source
        if card is None:
            return
        owner = state.player_by_id(card.owner_id)
        if card not in owner.graveyard:
            return
        owner.graveyard.remove(card)
        card.zone = Zone.LIBRARY
        owner.library.append(card)
        shuffle_library(state, owner.id)
        state.log.append(
            f"{card.printing.name} is shuffled into P{owner.id}'s library"
        )

    def describe(self) -> str:
        return "shuffle it into its owner's library"


# ---------------------------------------------------------------------
# Counters (Arwen, Bristly Bill, Scythecat Cub)
# ---------------------------------------------------------------------


def _put_counters(state, obj, counters) -> None:
    from mtgz_ir.game import add_counters

    for kind, n in counters:
        add_counters(state, obj, kind, n)


def _target_object(state, slot, chosen):
    from mtgz_ir.effects.generic.targets_helpers import _slot_target_still_legal

    if not _slot_target_still_legal(state, slot, chosen):
        return None
    return next((o for o in state.battlefield if o.id == chosen[1]), None)


@dataclass(frozen=True)
class GrantKeywordsUEOTAndPutCounters(Effect):
    """"<Target> gains <keywords> until end of turn. Put <counters> on
    that creature and <counters> on <this>." (Arwen, Mortal Queen.)
    ``target_counters`` / ``self_counters`` are ``((kind, n), ...)``."""

    targets: tuple = ()
    keywords: frozenset[KeywordSpelling] = frozenset()
    target_counters: tuple = ()
    self_counters: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantKeywords

        source = state.resolving_source
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            obj = (
                _target_object(state, self.targets[0], targets[0])
                if targets else None
            )
        finally:
            state.resolving_controller_id = prior
        if obj is not None:
            if self.keywords:
                ApplyEffect(
                    inner=GrantKeywords(keywords=self.keywords),
                    ends_at=EndOfTurn(),
                ).resolve(state, controller_id, (("creature", obj.id),))
            _put_counters(state, obj, self.target_counters)
        if source is not None and source in state.battlefield:
            _put_counters(state, source, self.self_counters)

    def describe(self) -> str:
        return (
            "target gains keywords until end of turn; put counters on it "
            "and on this creature"
        )


@dataclass(frozen=True)
class DoublePlusOneCountersOnEach(Effect):
    """"Double the number of +1/+1 counters on each <permanent>."
    (Bristly Bill.) Doubling puts that many more counters (CR 122.7)."""

    permanent_filter: "object" = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import add_plus_one_counters

        for o in _matching_permanents(state, controller_id, self.permanent_filter):
            if o.plus_one_counters > 0:
                add_plus_one_counters(state, o, o.plus_one_counters)

    def describe(self) -> str:
        return "double the number of +1/+1 counters on each matching permanent"


def _bump_resolution_count(state, source) -> int:
    """Count this resolution of ``source``'s ability for the current
    turn (``GameObject.resolutions_this_turn``); returns the new count."""
    turn, n = source.resolutions_this_turn or (state.turn_number, 0)
    n = n + 1 if turn == state.turn_number else 1
    source.resolutions_this_turn = (state.turn_number, n)
    return n


@dataclass(frozen=True)
class PutCounterDoublingOnSecondResolution(Effect):
    """"Put a +1/+1 counter on <target>. If this is the second time this
    ability has resolved this turn, double the number of +1/+1 counters
    on that creature instead." (Scythecat Cub.) The source counts its
    ability's resolutions per turn."""

    targets: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import add_plus_one_counters

        source = state.resolving_source
        n = _bump_resolution_count(state, source) if source is not None else 1
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            obj = _target_object(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if obj is None:
            return
        if n == 2:
            if obj.plus_one_counters > 0:
                add_plus_one_counters(state, obj, obj.plus_one_counters)
        else:
            add_plus_one_counters(state, obj, 1)

    def describe(self) -> str:
        return (
            "put a +1/+1 counter on target creature; on this ability's "
            "second resolution this turn, double its counters instead"
        )


# ---------------------------------------------------------------------
# Reflexive "you may <cost>. When you do, <effect>" (Leatherhead, Guide
# of Souls)
# ---------------------------------------------------------------------


def _counter_kinds_on(obj) -> list:
    from mtgz_ir.game import _DEDICATED_COUNTER_FIELDS

    kinds = [
        kind for kind, attr in _DEDICATED_COUNTER_FIELDS.items()
        if getattr(obj, attr, 0) > 0
    ]
    kinds += [k for k, n in obj.named_counters if n > 0]
    return kinds


@dataclass(frozen=True)
class RemoveACounterFromSelf(Effect):
    """"Remove a counter from <this>." -- any kind (the controller picks
    the kind with ``choose_mode``). Returns whether one was removed."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import remove_counters

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return False
        kinds = _counter_kinds_on(source)
        if not kinds:
            return False
        idx = 0
        p = state.player_by_id(controller_id)
        if len(kinds) > 1 and p.agent is not None:
            pick = p.agent.choose_mode(state, controller_id, kinds)
            if isinstance(pick, int) and 0 <= pick < len(kinds):
                idx = pick
        remove_counters(source, kinds[idx], 1)
        state.log.append(
            f"P{controller_id} removes a {kinds[idx]} counter from "
            f"{source.printing.name}"
        )
        return True

    def describe(self) -> str:
        return "remove a counter from this creature"


@dataclass(frozen=True)
class PayEnergy(Effect):
    """"Pay {E}{E}{E}." -- returns whether the energy was paid (the
    controller must have enough)."""

    amount: int = 1

    def resolve(self, state, controller_id, targets=()):
        p = state.player_by_id(controller_id)
        if p.energy < self.amount:
            return False
        p.energy -= self.amount
        state.log.append(f"P{controller_id} pays {self.amount} energy")
        return True

    def describe(self) -> str:
        return f"pay {self.amount} energy"


@dataclass(frozen=True)
class ReflexiveWhenYouDo(Effect):
    """"You may <cost>. When you do, <effect>." -- a reflexive triggered
    ability (CR 603.12): if the optional ``cost`` is paid, ``effect``'s
    targets are chosen then and it resolves. Engine shortcut: the
    reflexive trigger resolves immediately instead of using the stack
    (``docs/SEARCH_LIMITATIONS.md``).

    ``event_player_controls``: the effect's target must be controlled by
    the player the triggering event fixed as this ability's target
    ("... that player controls" after "deals combat damage to a
    player")."""

    cost: "Effect | None" = None
    effect: "Effect | None" = None
    event_player_controls: bool = False

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.combinators import MayDo

        paid = MayDo(inner=self.cost).resolve(state, controller_id, ())
        if paid is False or paid is None:
            return False
        chosen: tuple = ()
        if self.effect.target_count_range()[1] > 0:
            legal = self.effect.legal_targets(state, controller_id)
            if self.event_player_controls:
                pid = next((t[1] for t in targets if t[0] == "player"), None)
                legal = [
                    t for t in legal
                    if t[0] != "player" and any(
                        o.id == t[1] and o.controller_id == pid
                        for o in state.battlefield
                    )
                ]
            from mtgz_ir.keyword_rules import filter_targetable

            legal = filter_targetable(
                state, legal, state.resolving_source, controller_id,
            )
            if not legal:
                state.log.append("reflexive trigger has no legal targets")
                return True
            p = state.player_by_id(controller_id)
            pick = legal[0]
            if p.agent is not None:
                choice = p.agent.choose_target(state, controller_id, legal)
                if choice in legal:
                    pick = choice
            chosen = (pick,)
        self.effect.resolve(state, controller_id, chosen)
        return True

    def describe(self) -> str:
        return (
            f"you may {self.cost.describe()}; when you do, "
            f"{self.effect.describe()}"
        )


@dataclass(frozen=True)
class PutCountersOnTargetAndAddSubtype(Effect):
    """"Put <counters> on target <creature>. It becomes a <Subtype> in
    addition to its other types." (Guide of Souls.) The subtype is a
    permanent layer-4 addition."""

    targets: tuple = ()
    counters: tuple = ()
    subtype: str = ""

    def resolve(self, state, controller_id, targets=()):
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            obj = _target_object(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if obj is None:
            return
        _put_counters(state, obj, self.counters)
        if self.subtype:
            # Sourced by the creature itself so the type change outlasts
            # the ability's source (it ends only when the creature
            # leaves the battlefield).
            state.add_continuous_effect(
                source_id=obj.id,
                layer=4, kind="add_subtype", target_id=obj.id,
                params=(self.subtype,), expires_at=None,
                description=f"{obj.printing.name} becomes a {self.subtype}",
            )

    def describe(self) -> str:
        return f"put counters on target creature; it becomes a {self.subtype}"


@dataclass(frozen=True)
class GrantKeywordsUEOTThenExplores(Effect):
    """"Target <creature> gains <keywords> until end of turn. It
    explores." (Seasoned Dungeoneer.)"""

    targets: tuple = ()
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantKeywords
        from mtgz_ir.effects.generic.keyword_actions import explore

        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            obj = _target_object(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if obj is None:
            return
        ApplyEffect(
            inner=GrantKeywords(keywords=self.keywords), ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", obj.id),))
        explore(state, controller_id, obj)

    def describe(self) -> str:
        return "target creature gains keywords until end of turn; it explores"


# ---------------------------------------------------------------------
# Keen-Eyed Curator
# ---------------------------------------------------------------------




@dataclass(frozen=True)
class CardTypesAmongExiledWithSourceAtLeast(Condition):
    """"As long as there are N or more card types among cards exiled
    with this creature" -- a source-aware static condition
    (``StaticAbility.applies_to`` calls :meth:`matches_for_source`). Its
    presence links the card's exiling abilities (CR 607.2a)."""

    consumes_exile_link = True

    n: int = 4

    def matches(self, state, viewer_id, chosen_targets=None):
        return False  # only meaningful with a source

    def matches_for_source(self, state, source) -> bool:
        from mtgz_ir.effects.card_types import count_card_types

        return count_card_types(cards_exiled_with(state, source)) >= self.n


# ---------------------------------------------------------------------
# Adeline, Resplendent Cathar
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class PowerEqualsCount(PTFormula):
    """Structured ``dynamic_pt_formula``: "<This>'s power is equal to
    the number of <permanents>" -- base power is the count of
    battlefield permanents matching ``count_filter`` (evaluated for the
    creature's controller); toughness stays printed. Read by
    ``game._dynamic_base_pt`` through :meth:`base_pt`."""

    count_filter: "object" = None

    def base_pt(self, state, obj) -> tuple[int, int]:
        n = len(_matching_permanents(state, obj.controller_id, self.count_filter))
        return n, obj.printing.toughness or 0


@dataclass(frozen=True)
class CreateAttackingTokenForEachOpponent(Effect):
    """"For each opponent, create <token> that's tapped and attacking that
    player or a planeswalker they control." (Adeline.) The engine's
    attack model has one defending player; each token attacks that
    player (attacking a planeswalker instead isn't offered)."""

    token_printing: "object" = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.tokens import CreateToken

        opponents = [
            p for p in state.players if p.id != controller_id and not p.has_lost
        ]
        for _ in opponents:
            CreateToken(
                token_printing=self.token_printing, tapped=True, attacking=True,
            ).resolve(state, controller_id)

    def describe(self) -> str:
        return (
            f"for each opponent, create a {self.token_printing.name} token "
            f"that's tapped and attacking"
        )


# ---------------------------------------------------------------------
# Ursine Monstrosity
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class MillChooseOpponentAttackPump(Effect):
    """Ursine Monstrosity's beginning-of-combat body: "Mill a card and
    choose an opponent at random. This creature attacks that player
    this combat if able. Until end of turn, this creature gains
    <keywords> and gets +1/+1 for each card type among cards in your
    graveyard." The opponent is chosen with ``state.rng`` (two players:
    the opponent); the attack requirement is
    ``GameObject.must_attack_turn`` (``designations.forced_attackers``);
    the bonus is counted as the ability resolves."""

    mill: int = 1
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.pump import PumpSelfUntilEOT
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        for _ in range(self.mill):
            if p.library:
                card = p.library.pop(0)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
        opponents = [
            o for o in state.players if o.id != controller_id and not o.has_lost
        ]
        if opponents:
            chosen = state.rng.choice(opponents)
            state.log.append(f"P{controller_id} chooses P{chosen.id} at random")
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        if opponents:
            source.must_attack_turn = state.turn_number
        from mtgz_ir.effects.card_types import count_card_types

        n = count_card_types(p.graveyard)
        PumpSelfUntilEOT(
            power_bonus=n, toughness_bonus=n, granted_keywords=self.keywords,
        ).resolve(state, controller_id)

    def describe(self) -> str:
        return (
            f"mill {self.mill}; this creature attacks a random opponent if "
            f"able and gets +1/+1 per card type in your graveyard UEOT"
        )


# ---------------------------------------------------------------------
# Okoye, Mighty and Adored
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class IsObjectWithId(ObjectPredicate):
    """The object is the one with id ``obj_id`` ("that creature")."""

    obj_id: int = -1

    def test(self, obj, state):
        return getattr(obj, "id", None) == self.obj_id


@dataclass(frozen=True)
class DefendingPlayerIsMonarch(TriggerPredicate):
    """The attacking creature attacks the monarch: its defending player
    (an opponent of its controller -- the engine's one defending
    player) is the monarch."""

    def matches(self, state, subjects, source):
        attacker = subjects.get("attacker")
        if attacker is None:
            return False
        return any(
            p.is_monarch and p.id != attacker.controller_id
            for p in state.players
        )


@dataclass(frozen=True)
class GrantKeywordsToObjectUEOT(Effect):
    """"<That creature> gains <keywords> until end of turn." for a fixed
    object id (a delayed trigger's "that creature")."""

    obj_id: int = -1
    keywords: frozenset = frozenset()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantKeywords

        if not any(o.id == self.obj_id for o in state.battlefield):
            return
        ApplyEffect(
            inner=GrantKeywords(keywords=self.keywords), ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", self.obj_id),))

    def describe(self) -> str:
        return "that creature gains keywords until end of turn"


@dataclass(frozen=True)
class CounterThenWhenItAttacksMonarch(Effect):
    """"Put a +1/+1 counter on target creature. Whenever that creature
    attacks the monarch this turn, it gains <keywords> until end of
    turn." (Okoye.) The delayed trigger is a this-turn command-zone
    holder (``spell_misc.CreateThisTurnTriggeredAbility``)."""

    targets: tuple = ()
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.spell_misc import (
            CreateThisTurnTriggeredAbility,
        )
        from mtgz_ir.effects.predicates import Event, Subject
        from mtgz_ir.game import add_plus_one_counters

        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            obj = _target_object(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if obj is None:
            return
        add_plus_one_counters(state, obj, 1)
        CreateThisTurnTriggeredAbility(triggered_abilities=(TriggeredAbility(
            event=Event.ATTACKS,
            predicate=Subject("attacker", IsObjectWithId(obj.id))
            & DefendingPlayerIsMonarch(),
            effect=GrantKeywordsToObjectUEOT(obj_id=obj.id, keywords=self.keywords),
        ),)).resolve(state, controller_id)

    def describe(self) -> str:
        return (
            "put a +1/+1 counter on target creature; when it attacks the "
            "monarch this turn it gains keywords"
        )


# ---------------------------------------------------------------------
# Backup N (CR 702.165)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class Backup(Effect):
    """Backup N: "When this creature enters, put N +1/+1 counters on
    target creature. If that's another creature, it gains the following
    abilities until end of turn." ``keywords`` are the keyword abilities
    printed after the Backup line."""

    n: int = 1
    keywords: frozenset[KeywordSpelling] = frozenset()
    targets: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import add_plus_one_counters

        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        state.resolving_controller_id = controller_id
        try:
            obj = _target_object(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if obj is None:
            return
        add_plus_one_counters(state, obj, self.n)
        source = state.resolving_source
        if self.keywords and (source is None or obj.id != source.id):
            GrantKeywordsToObjectUEOT(
                obj_id=obj.id, keywords=self.keywords,
            ).resolve(state, controller_id)

    def describe(self) -> str:
        return f"backup {self.n}"


# ---------------------------------------------------------------------
# Ocelot Pride
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class CopyEachTokenYouControlEnteredThisTurn(Effect):
    """"For each token you control that entered this turn, create a
    token that's a copy of it." (Ocelot Pride's city's-blessing rider.)
    The set is fixed before any copy is created."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.tokens import CreateToken

        entered = [
            o for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_token
            and o.entered_battlefield_turn == state.turn_number
        ]
        for tok in entered:
            CreateToken(token_printing=token_copy_printing(tok.printing)).resolve(
                state, controller_id,
            )

    def describe(self) -> str:
        return "copy each token you control that entered this turn"


# ---------------------------------------------------------------------
# Elite Spellbinder
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class LookAtHandExileNonlandOwnerMayPlayTaxed(Effect):
    """"Look at target opponent's hand. You may exile a nonland card from
    it. For as long as that card remains exiled, its owner may play it.
    A spell cast this way costs {N} more to cast." (Elite Spellbinder.)
    Rides the Lightstall exile-play permission
    (``cast_from_exile_with_lightstall_surcharge``, {1}) plus
    ``exile_cast_extra_surcharge`` for the rest of N."""

    surcharge: int = 2

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        opp = _chosen_player(state, targets)
        if opp is None:
            return
        for c in opp.hand:
            reveal_to(c, controller_id)
        options = [c for c in opp.hand if not c.printing.is_land]
        if not options:
            return
        me = state.player_by_id(controller_id)
        if me.agent is not None and not me.agent.choose_yes_no(
            state, controller_id, "exile a nonland card from their hand?",
        ):
            return
        pick = options[0].id
        if me.agent is not None:
            pick = me.agent.choose_card_from_opponent_hand(
                state, controller_id, [c.id for c in options],
            )
        card = next((c for c in options if c.id == pick), options[0])
        opp.hand.remove(card)
        card.zone = Zone.EXILE
        card.cast_from_exile_with_lightstall_surcharge = True
        card.exile_cast_extra_surcharge = max(0, self.surcharge - 1)
        opp.exile.append(card)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} from P{opp.id}'s "
            f"hand (castable from exile for {{{self.surcharge}}} more)"
        )

    def describe(self) -> str:
        return (
            f"look at target opponent's hand; you may exile a nonland card "
            f"from it, which its owner may cast for {{{self.surcharge}}} more"
        )


# ---------------------------------------------------------------------
# Six / Serra Paragon
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class ExileSourceCardGainLife(Effect):
    """"Exile it and you gain N life." -- Serra Paragon's granted rider,
    resolving with the card that went to the graveyard as its source."""

    life: int = 2

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, gain_life

        card = state.resolving_source
        if card is not None:
            owner = state.player_by_id(card.owner_id)
            if card in owner.graveyard:
                owner.graveyard.remove(card)
                card.zone = Zone.EXILE
                owner.exile.append(card)
                state.log.append(f"{card.printing.name} is exiled")
        gain_life(state, controller_id, self.life)

    def describe(self) -> str:
        return f"exile it and you gain {self.life} life"


@dataclass(frozen=True)
class MillThenMayPutLandIntoHand(Effect):
    """"Mill N cards. You may put a land card from among them into your
    hand." (Six.) Picked with ``choose_card_from_library`` (``None``
    declines)."""

    n: int = 3

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        milled = []
        for _ in range(self.n):
            if not p.library:
                break
            card = p.library.pop(0)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            milled.append(card)
        state.log.append(f"P{controller_id} mills {len(milled)} card(s)")
        lands = [c for c in milled if c.printing.is_land and c in p.graveyard]
        if not lands or p.agent is None:
            return
        pick = p.agent.choose_card_from_library(
            state, controller_id, [c.id for c in lands],
        )
        card = next((c for c in lands if c.id == pick), None)
        if card is None:
            return
        p.graveyard.remove(card)
        card.zone = Zone.HAND
        p.hand.append(card)
        state.log.append(f"P{controller_id} puts {card.printing.name} into hand")

    def describe(self) -> str:
        return (
            f"mill {self.n} cards; you may put a land card from among them "
            f"into your hand"
        )


# ---------------------------------------------------------------------
# Lion Sash (Reconfigure)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class ReconfigureAttach(Effect):
    """Reconfigure's "Attach to target creature you control" (CR
    702.151a): another creature you control; while attached this
    Equipment creature isn't a creature (``game.is_creature_now``)."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        from mtgz_ir.game import is_creature_now

        source = state.resolving_source
        return [
            ("creature", o.id) for o in state.battlefield
            if o.controller_id == controller_id and is_creature_now(o)
            and (source is None or o.id != source.id)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import is_creature_now

        source = state.resolving_source
        if source is None or source not in state.battlefield or not targets:
            return
        target = next((o for o in state.battlefield if o.id == targets[0][1]), None)
        if (
            target is None or target.id == source.id
            or target.controller_id != controller_id or not is_creature_now(target)
        ):
            return
        source.attached_to = target.id
        source.attacking = False
        source.blocking = None
        state.log.append(
            f"{source.printing.name} is attached to {target.printing.name}"
        )

    def describe(self) -> str:
        return "reconfigure: attach to target creature you control"


@dataclass(frozen=True)
class ReconfigureUnattach(Effect):
    """Reconfigure's "Unattach this permanent" (activate only while
    attached -- ``creature_statics_wg.filter_unusable_abilities``)."""

    def resolve(self, state, controller_id, targets=()):
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        if source.attached_to is not None:
            source.attached_to = None
            state.log.append(f"{source.printing.name} is unattached")

    def describe(self) -> str:
        return "reconfigure: unattach this permanent"


@dataclass(frozen=True)
class ExileGraveyardCardCounterIfPermanent(Effect):
    """"Exile target card from a graveyard. If it was a permanent card,
    put a +1/+1 counter on this permanent." (Lion Sash.)"""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("card_in_graveyard", c.id)
            for p in state.players for c in p.graveyard
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, add_plus_one_counters

        if not targets:
            return
        for p in state.players:
            card = next((c for c in p.graveyard if c.id == targets[0][1]), None)
            if card is None:
                continue
            p.graveyard.remove(card)
            card.zone = Zone.EXILE
            p.exile.append(card)
            state.log.append(f"{card.printing.name} is exiled from a graveyard")
            source = state.resolving_source
            if (
                card.printing.is_permanent and source is not None
                and source in state.battlefield
            ):
                add_plus_one_counters(state, source, 1)
            return

    def describe(self) -> str:
        return (
            "exile target card from a graveyard; if it was a permanent card, "
            "put a +1/+1 counter on this permanent"
        )


# ---------------------------------------------------------------------
# Springheart Nantuko (Bestow)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class PayToCopyEnchantedCreatureElseToken(Effect):
    """"You may pay <cost> if this permanent is attached to a creature you
    control. If you do, create a token that's a copy of that creature. If
    you didn't create a token this way, create <token>." (Springheart
    Nantuko's landfall.)"""

    cost: "object" = None
    cost_str: str = ""
    token_printing: "object" = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.cheat_into_play import (
            pay_mana_during_resolution,
        )
        from mtgz_ir.effects.generic.tokens import CreateToken
        from mtgz_ir.game import is_creature_now

        source = state.resolving_source
        host = None
        if source is not None and source in state.battlefield and source.attached_to is not None:
            host = next(
                (o for o in state.battlefield if o.id == source.attached_to), None,
            )
        if (
            host is not None and is_creature_now(host)
            and host.controller_id == controller_id
            and pay_mana_during_resolution(
                state, controller_id, self.cost,
                f"Pay {self.cost_str} to copy {host.printing.name}?",
            )
        ):
            CreateToken(token_printing=token_copy_printing(host.printing)).resolve(
                state, controller_id,
            )
            return
        CreateToken(token_printing=self.token_printing).resolve(state, controller_id)

    def describe(self) -> str:
        return (
            f"you may pay {self.cost_str} to copy the enchanted creature; "
            f"otherwise create a {self.token_printing.name} token"
        )


# ---------------------------------------------------------------------
# Badgermole Cub (earthbend)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class Earthbend(Effect):
    """Earthbend N: "Target land you control becomes a 0/0 creature with
    haste that's still a land. Put N +1/+1 counters on it. When it dies
    or is exiled, return it to the battlefield tapped." The type change
    is permanent and sourced by the land itself (it outlives the
    earthbender); the return rider is ``GameObject.earthbent`` (fired
    from ``move_off_battlefield``)."""

    n: int = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("permanent", o.id) for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_land
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.become_type import BecomeType
        from mtgz_ir.game import add_plus_one_counters

        if not targets:
            return
        land = next(
            (o for o in state.battlefield
             if o.id == targets[0][1] and o.printing.is_land
             and o.controller_id == controller_id),
            None,
        )
        if land is None:
            return
        prior = state.resolving_source
        state.resolving_source = land
        try:
            BecomeType(types_to_add=("creature",), base_pt=(0, 0)).resolve(
                state, controller_id,
            )
        finally:
            state.resolving_source = prior
        state.add_continuous_effect(
            source_id=land.id, layer=6, kind="grant_keyword",
            target_id=land.id, params=("haste",),
            description=f"{land.printing.name} has haste (earthbend)",
        )
        add_plus_one_counters(state, land, self.n)
        land.earthbent = True
        state.log.append(f"P{controller_id} earthbends {land.printing.name}")

    def describe(self) -> str:
        return f"earthbend {self.n}"


@dataclass(frozen=True)
class ReturnEarthbentLandTapped(Effect):
    """Earthbend's "When it dies or is exiled, return it to the
    battlefield tapped." -- the resolving source is the land card."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.turn import _reset_left_battlefield_object

        card = state.resolving_source
        if card is None:
            return
        owner = state.player_by_id(card.owner_id)
        for zone in (owner.graveyard, owner.exile):
            if card in zone:
                zone.remove(card)
                _reset_left_battlefield_object(card)
                card.permanent_is_creature = False
                put_into_battlefield(state, card, owner.id)
                card.tapped = True
                state.log.append(f"{card.printing.name} returns tapped")
                return

    def describe(self) -> str:
        return "return it to the battlefield tapped"


# ---------------------------------------------------------------------
# "When you cast this spell" riders (Sage of the Skies, Sowing Mycospawn)
# ---------------------------------------------------------------------


def _own_spell_item(state, source):
    """The stack item of ``source`` as a spell (not an ability), or
    ``None`` once it has left the stack."""
    return next(
        (
            i for i in state.stack
            if i.ability is None and i.source is source
        ),
        None,
    )


@dataclass(frozen=True)
class ThisSpellWasKicked(TriggerPredicate):
    """Intervening-if "if it was kicked" on a "When you cast this spell"
    trigger (Sowing Mycospawn): the source spell's stack item was cast
    with its kicker paid. On a permanent's trigger (Thieving Skydiver's
    ETB) the source is no longer on the stack: its ``kick_count``
    (copied from the resolving spell) answers instead."""

    def matches(self, state, subjects, source):
        item = _own_spell_item(state, source)
        if item is None:
            return getattr(source, "kick_count", 0) > 0
        return bool(item.kicked or item.kick_count)


@dataclass(frozen=True)
class CastAnotherSpellThisTurn(TriggerPredicate):
    """Intervening-if "if you've cast another spell this turn" on a
    "When you cast this spell" trigger (Sage of the Skies). Those
    triggers are stacked before the cast is counted
    (``actions._cast_spell``), so any spell already counted this turn
    is "another" spell."""

    def matches(self, state, subjects, source):
        try:
            player = state.player_by_id(source.controller_id)
        except (KeyError, AttributeError):
            return False
        return getattr(player, "spells_cast_this_turn", 0) >= 1


@dataclass(frozen=True)
class CopyThisSpell(Effect):
    """"Copy this spell." -- the ability's source spell, if it's still on
    the stack (CR 707.10: the copy isn't cast; it keeps the spell's
    targets and X). A copy of a permanent spell becomes a token
    (CR 707.10f): the copy is put on the stack with the token printing
    (``token_copy_printing``), so it resolves into a token."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import push_spell_copy

        source = state.resolving_source
        item = _own_spell_item(state, source) if source is not None else None
        if item is None:
            state.log.append("copy this spell: the spell is gone")
            return
        printing = item.source.printing
        if printing.is_permanent:
            printing = token_copy_printing(printing)
        push_spell_copy(
            state, printing, controller_id, item.targets,
            x_value=item.x_value,
        )

    def describe(self) -> str:
        return "copy this spell"
