"""Card-flow effects: targeted draw, tutors, hand disruption, library
look-and-select, wheels, and mass zone moves.

Generic primitives introduced for the PowerLSV cube's "spells B" bucket
(Ancestral Recall, Demonic Tutor, Thoughtseize, Brainstorm, Stock Up,
Timetwister, Wheel of Fortune, Upheaval, ...). Every effect here is
parameterized by a small :class:`CardFilter` (printed characteristics of
a card in a hidden zone or graveyard) and / or a
:class:`mtgz_ir.effects.formulas.Formula` amount, so one class covers a
whole printed shape rather than one card.

Decision hooks reuse the existing agent protocol (no new hook):

* tutor / look-at-top picks -> ``choose_card_from_library``
* caster's pick from a revealed hand -> ``choose_card_from_opponent_hand``
* "put a card from your hand on top" (Brainstorm) -> ``choose_discard``
  (the same "which hand card do I give up" decision surface)
* optional riders -> ``choose_yes_no``

Copy-on-write discipline (see ``game._CowList``): every site that moves a
card out of a library / graveyard / exile list removes it from the list
(a list mutator, which forks the share group) BEFORE mutating the card
object, so sibling search states never observe the mutation.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Union

from mtgz_ir.effects.vocab import DistributePile, RestZone, RevealedCardFate, SearchDestination, ZoneName, coerce_tuple
from mtgz_ir.effects.generic.roto_search import (
    library_search,
    searched_card_taken,
)
from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameObject, GameState, Player


Amount = Union[int, "Formula"]


def _amount(amount: Amount, state: "GameState", controller_id: int) -> int:
    """Evaluate an ``int`` or ``Formula`` amount at resolve time."""
    if isinstance(amount, int):
        return max(0, amount)
    return max(0, amount.value(state, controller_id))


def _describe_amount(amount: Amount) -> str:
    return str(amount) if isinstance(amount, int) else "X"


# ---------------------------------------------------------------------
# CardFilter — printed-characteristics filter for non-battlefield cards
# ---------------------------------------------------------------------


_PSEUDO_TYPE_PERMANENT = "Permanent"


def _is_permanent_type_set(card_types: "tuple[str, ...]") -> bool:
    from mtgz_ir.cards.base import is_permanent_card_types

    return is_permanent_card_types(card_types)


@dataclass(frozen=True)
class CardFilter:
    """Filter over a card's printed characteristics.

    Used for cards in libraries, hands and graveyards, where the
    battlefield-oriented ``ObjectPredicate`` atoms (which read layered
    state such as ``is_creature_now``) don't apply.

    * ``any_types`` — the card must have at least one of these card
      types. ``"Permanent"`` is a pseudo-type matching any permanent
      card (``CardPrinting.is_permanent``). Empty = no type restriction.
    * ``excluded_types`` — the card must have none of these card types
      ("noncreature, nonland card").
    * ``max_mv`` — mana value at most N ("with mana value 3 or less").
    * ``color`` — the card must be this color ("a green creature card").
    * ``supertype`` — e.g. ``"Legendary"``.
    * ``subtype`` — e.g. ``"Equipment"`` ("an Equipment card").
    * ``mana_costs`` — the card's printed mana cost is one of these
      ("with mana cost {0} or {1}", CR 202.1; a card with no mana cost
      has none, so it never matches). Empty = no restriction.
    """

    any_types: tuple[str, ...] = ()
    excluded_types: tuple[str, ...] = ()
    max_mv: "int | None" = None
    color: "str | None" = None
    supertype: "str | None" = None
    subtype: "str | None" = None
    mana_costs: tuple[str, ...] = ()

    @property
    def has_quality(self) -> bool:
        """CR 701.19b: a search for a card with a stated quality may
        fail to find; a search for just "a card" may not (unless the
        library is empty)."""
        return bool(
            self.any_types or self.excluded_types
            or self.max_mv is not None or self.color is not None
            or self.supertype is not None or self.subtype is not None
            or bool(self.mana_costs)
        )

    def matches(
        self, printing: "CardPrinting", *,
        types: "tuple[str, ...] | None" = None,
        colors: "frozenset[str] | None" = None,
    ) -> bool:
        """Test ``printing``'s characteristics. ``types`` / ``colors``
        override the printed card types / colors with a game object's
        effective ones (:meth:`matches_card`); a bare printing (a card
        outside the game -- a sideboard entry) uses its printed values."""
        card_types = printing.card_types if types is None else types
        if self.any_types:
            ok = False
            for t in self.any_types:
                if t == _PSEUDO_TYPE_PERMANENT:
                    if _is_permanent_type_set(card_types):
                        ok = True
                        break
                elif t in card_types:
                    ok = True
                    break
            if not ok:
                return False
        for t in self.excluded_types:
            if t in card_types:
                return False
        if self.max_mv is not None and printing.mana_cost.total > self.max_mv:
            return False
        if self.color is not None and self.color not in (
            printing.mana_cost.colors if colors is None else colors
        ):
            return False
        if self.supertype is not None and self.supertype not in printing.supertypes:
            return False
        if self.subtype is not None and self.subtype not in printing.subtypes:
            return False
        if self.mana_costs and printing.mana_cost_str not in self.mana_costs:
            return False
        return True

    def matches_card(self, state: "GameState", card: "GameObject") -> bool:
        """:meth:`matches` against the game object ``card``: its effective
        card types (``game.effective_card_types`` -- layer 4 on the
        battlefield, printed elsewhere) and its effective colors
        (``game.effective_colors`` -- layer 5 in every zone: Mycosynth
        Lattice makes a blue card in hand colorless)."""
        from mtgz_ir.game import effective_card_types, effective_colors

        return self.matches(
            card.printing,
            types=effective_card_types(state, card),
            colors=(
                effective_colors(state, card) if self.color is not None
                else None
            ),
        )

    def describe(self) -> str:
        words: list[str] = []
        if self.supertype:
            words.append(self.supertype.lower())
        if self.color:
            words.append({
                "W": "white", "U": "blue", "B": "black",
                "R": "red", "G": "green",
            }.get(self.color, self.color))
        for t in self.excluded_types:
            words.append(f"non{t.lower()}")
        if self.subtype:
            words.append(self.subtype)
        if self.any_types:
            words.append(" or ".join(t.lower() for t in self.any_types))
        words.append("card")
        text = " ".join(words)
        if self.max_mv is not None:
            text += f" with mana value {self.max_mv} or less"
        if self.mana_costs:
            text += f" with mana cost {' or '.join(self.mana_costs)}"
        return text


ANY_CARD = CardFilter()


@dataclass(frozen=True)
class CardFilterAnyOf:
    """A card matching ANY of ``filters`` ("a basic Plains card or a
    creature card with mana value 1 or less", Starfield Shepherd): one
    search whose card the player picks from either kind (CR 701.19).
    Same ``matches`` / ``matches_card`` protocol as :class:`CardFilter`."""

    filters: tuple[CardFilter, ...] = ()

    @property
    def has_quality(self) -> bool:
        return any(f.has_quality for f in self.filters)

    def matches(self, printing, *, types=None, colors=None) -> bool:
        return any(
            f.matches(printing, types=types, colors=colors) for f in self.filters
        )

    def matches_card(self, state: "GameState", card: "GameObject") -> bool:
        return any(f.matches_card(state, card) for f in self.filters)

    def describe(self) -> str:
        return " or ".join(f.describe() for f in self.filters)


# ---------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------


def _player_targets(
    state: "GameState", controller_id: int, opponent_only: bool,
) -> list[Target]:
    return [
        ("player", p.id) for p in state.players
        if not p.has_lost and not (opponent_only and p.id == controller_id)
    ]


def _target_player(
    state: "GameState", targets: tuple[Target, ...],
) -> "Player | None":
    """Resolve the first ``("player", id)`` target, or ``None`` when the
    spell has no (still-legal) player target."""
    if not targets:
        return None
    kind, pid = targets[0]
    if kind != "player":
        return None
    p = state.player_by_id(pid)
    if p.has_lost:
        return None
    return p


def discard_card(state: "GameState", player: "Player", card: "GameObject") -> None:
    """Move ``card`` from ``player``'s hand to their graveyard as a
    discard and fire the ``DISCARDS_CARD`` observers."""
    from mtgz_ir.game import Zone
    from mtgz_ir.player_protections import opponent_effect_blocked_for
    from mtgz_ir.turn import fire_opponent_discards_triggers

    if opponent_effect_blocked_for(state, player.id):
        return
    player.hand.remove(card)
    card.zone = Zone.GRAVEYARD
    player.graveyard.append(card)
    state.log.append(f"P{player.id} discards {card.printing.name}")
    fire_opponent_discards_triggers(state, player.id, card=card)


def _agent_pick(options: list, picked_id) -> "GameObject | None":
    return next((c for c in options if c.id == picked_id), None)


# ---------------------------------------------------------------------
# Targeted draw / extra turn
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class TargetPlayerDraws(Effect):
    """"Target player draws N cards." (Ancestral Recall.)"""

    amount: Amount = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _player_targets(state, controller_id, opponent_only=False)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        p = _target_player(state, targets)
        if p is None:
            return
        drawn = draw_n_cards(state, p.id, _amount(self.amount, state, controller_id))
        state.log.append(f"P{p.id} draws {drawn} card(s)")

    def describe(self) -> str:
        return f"target player draws {_describe_amount(self.amount)} card(s)"


@dataclass(frozen=True)
class TargetPlayerTakesExtraTurn(Effect):
    """"Target player takes an extra turn after this one." (Time Warp.)

    Queues the target on ``state.extra_turn_queue`` — the same queue
    ``TakeExtraTurnAfterThisOne`` (Nexus of Fate) feeds.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _player_targets(state, controller_id, opponent_only=False)

    def resolve(self, state, controller_id, targets=()):
        p = _target_player(state, targets)
        if p is None:
            return
        state.extra_turn_queue.append(p.id)
        state.log.append(f"P{p.id} is scheduled to take an extra turn")

    def describe(self) -> str:
        return "target player takes an extra turn after this one"


# ---------------------------------------------------------------------
# Hand disruption
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class TargetPlayerDiscardsAtRandom(Effect):
    """"Target player discards N cards at random." (Hymn to Tourach;
    Mind Twist with ``amount=ResolvingX()``.) Random picks draw from
    ``state.rng`` so the result is reproducible under a seeded game."""

    amount: Amount = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _player_targets(state, controller_id, opponent_only=False)

    def resolve(self, state, controller_id, targets=()):
        p = _target_player(state, targets)
        if p is None:
            return
        n = _amount(self.amount, state, controller_id)
        for _ in range(n):
            if not p.hand:
                break
            card = p.hand[state.rng.randrange(len(p.hand))]
            discard_card(state, p, card)

    def describe(self) -> str:
        return (
            f"target player discards {_describe_amount(self.amount)} "
            f"card(s) at random"
        )


@dataclass(frozen=True)
class TargetPlayerRevealsHandYouChooseDiscard(Effect):
    """"Target player reveals their hand. You choose a <filter> card from
    it. That player discards that card." (Thoughtseize, Inquisition of
    Kozilek, Duress.)

    ``opponent_only`` is True for the "Target opponent" wording (Duress).
    Every card in the revealed hand becomes known to the caster
    (``reveal_to``). The caster's pick goes through
    ``choose_card_from_opponent_hand`` (exposed to search).

    ``optional`` models "You may choose a <filter> card from it. If you
    do, that player discards that card" (Revealing Eye): the caster may
    decline (``choose_yes_no``). ``then_draws`` is the discarding
    player's ", then draws N card(s)" rider, drawn only when a card was
    discarded.
    """

    card_filter: CardFilter = ANY_CARD
    opponent_only: bool = False
    optional: bool = False
    then_draws: int = 0
    # ``"exile"``: "... and you choose a <filter> card from it. Exile that
    # card." (Tidehollow Sculler) -- the chosen card is exiled instead of
    # discarded; the parser wraps the effect in ``LinkExiledCardsToSource``
    # when the card prints a consumer of "the exiled card".
    destination: RevealedCardFate = RevealedCardFate.DISCARD

    def __post_init__(self) -> None:
        object.__setattr__(self, "destination", RevealedCardFate(self.destination))

    @property
    def links_exiled_cards(self) -> bool:
        """An exiling effect for ``link_exile_abilities`` (CR 607.2a)."""
        return self.destination == "exile"

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _player_targets(state, controller_id, self.opponent_only)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import reveal_to

        p = _target_player(state, targets)
        if p is None:
            return
        for c in p.hand:
            reveal_to(c, controller_id)
        options = [c for c in p.hand if self.card_filter.matches_card(state, c)]
        if not options:
            state.log.append(
                f"P{p.id} reveals a hand with no "
                f"{self.card_filter.describe()}"
            )
            return
        caster = state.player_by_id(controller_id)
        if (
            self.optional and caster.agent is not None
            and not caster.agent.choose_yes_no(
                state, controller_id,
                f"choose a {self.card_filter.describe()} for P{p.id} to discard?",
            )
        ):
            return
        chosen = options[0]
        if caster.agent is not None:
            picked = caster.agent.choose_card_from_opponent_hand(
                state, controller_id, [c.id for c in options],
            )
            chosen = _agent_pick(options, picked) or options[0]
        if self.destination == "exile":
            from mtgz_ir.effects.generic.cube_creatures_rmc import exile_card

            p.hand.remove(chosen)
            exile_card(state, chosen)
            state.log.append(
                f"P{controller_id} exiles {chosen.printing.name} from "
                f"P{p.id}'s hand"
            )
            return
        discard_card(state, p, chosen)
        if self.then_draws:
            from mtgz_ir.turn import draw_n_cards

            draw_n_cards(state, p.id, self.then_draws)

    def describe(self) -> str:
        who = "opponent" if self.opponent_only else "player"
        fate = (
            "exile that card" if self.destination == "exile"
            else "that player discards it"
        )
        return (
            f"target {who} reveals their hand; you choose a "
            f"{self.card_filter.describe()} from it; {fate}"
        )


@dataclass(frozen=True)
class EachPlayerDiscardsHand(Effect):
    """"Each player discards their hand" (Wheel of Fortune's first half)."""

    def resolve(self, state, controller_id, targets=()):
        for i in range(len(state.players)):
            p = state.players[(state.active_player_idx + i) % len(state.players)]
            if p.has_lost:
                continue
            for card in list(p.hand):
                discard_card(state, p, card)

    def describe(self) -> str:
        return "each player discards their hand"



# ---------------------------------------------------------------------
# Library search (tutors)
# ---------------------------------------------------------------------


SEARCH_DESTINATIONS = ("hand", "graveyard", "top", "battlefield")


@dataclass(frozen=True)
class SearchLibrary(Effect):
    """Search your library for a ``card_filter`` card and put it into
    ``destination``, then shuffle.

    ``destination``:
      * ``"hand"`` — Demonic Tutor.
      * ``"graveyard"`` — Entomb.
      * ``"top"`` — "then shuffle and put that card on top" (Vampiric
        Tutor, Mystical Tutor); the shuffle happens BEFORE the card is
        placed on top.
      * ``"battlefield"`` — Tinker / Natural Order / Crop Rotation;
        enters through ``put_into_battlefield`` so ETBs fire.

    ``reveal`` marks the found card as known to every player; otherwise
    the controller alone knows it (relevant for the top-of-library
    destination). Per CR 701.19b a search for a card with a stated
    quality may fail (the agent may return ``None``); a search for just
    "a card" finds one whenever the library is non-empty.
    """

    # A ``CardFilter`` or the same-protocol ``cube_creatures_rmc.
    # CardTypeOrSubtypeFilter`` ("artifact or Dragon card") or
    # ``CardFilterAnyOf`` ("a basic Plains card or a creature card ...").
    card_filter: "CardFilter | CardTypeOrSubtypeFilter | CardFilterAnyOf" = ANY_CARD
    destination: SearchDestination = SearchDestination.HAND
    reveal: bool = False
    # "put that card onto the battlefield tapped" (Golos, Tireless
    # Pilgrim). Only meaningful for the battlefield destination.
    tapped: bool = False

    def __post_init__(self) -> None:
        if self.destination not in SEARCH_DESTINATIONS:
            raise ValueError(
                f"SearchLibrary destination must be one of "
                f"{SEARCH_DESTINATIONS}, got {self.destination!r}"
            )
        object.__setattr__(self, "destination", SearchDestination(self.destination))

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to, reveal_to_all, shuffle_library

        p = state.player_by_id(controller_id)
        options = [c for c in p.library if self.card_filter.matches_card(state, c)]
        chosen = None
        if options:
            picked = None
            if p.agent is not None:
                picked = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in options],
                )
            chosen = _agent_pick(options, picked)
            if chosen is None and not self.card_filter.has_quality:
                chosen = options[0]
        if chosen is None:
            state.log.append(
                f"P{controller_id} searches for a "
                f"{self.card_filter.describe()} and finds nothing"
            )
            shuffle_library(state, controller_id)
            return
        p.library.remove(chosen)
        if searched_card_taken(state, controller_id, chosen):
            shuffle_library(state, controller_id)
            return
        if self.reveal:
            reveal_to_all(state, chosen)
        dest = self.destination
        if dest == "top":
            shuffle_library(state, controller_id)
            chosen.zone = Zone.LIBRARY
            p.library.insert(0, chosen)
            if self.reveal:
                reveal_to_all(state, chosen)
            else:
                reveal_to(chosen, controller_id)
            state.log.append(
                f"P{controller_id} tutors a card to the top of their library"
                + (f" ({chosen.printing.name})" if self.reveal else "")
            )
            return
        if dest == "hand":
            chosen.zone = Zone.HAND
            p.hand.append(chosen)
            reveal_to(chosen, controller_id)
            state.log.append(f"P{controller_id} tutors a card into hand")
        elif dest == "graveyard":
            chosen.zone = Zone.GRAVEYARD
            p.graveyard.append(chosen)
            reveal_to_all(state, chosen)
            state.log.append(
                f"P{controller_id} puts {chosen.printing.name} into their "
                f"graveyard"
            )
        else:  # battlefield
            state.log.append(
                f"P{controller_id} puts {chosen.printing.name} onto the "
                f"battlefield"
            )
            put_into_battlefield(state, chosen, controller_id)
            if self.tapped:
                chosen.tapped = True
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        where = {
            "hand": "put it into your hand",
            "graveyard": "put it into your graveyard",
            "top": "then shuffle and put it on top",
            "battlefield": "put it onto the battlefield",
        }[self.destination]
        return f"search your library for a {self.card_filter.describe()}, {where}"


# ---------------------------------------------------------------------
# Look at the top N and select
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class LookAtTopCardsPutIntoHand(Effect):
    """Look at (or reveal) the top ``count`` cards of your library, put
    ``take`` of them matching ``take_filter`` into your hand, and move
    the rest to ``rest_zone``.

    Covers:
      * Stock Up / Dig Through Time — look at top 5/7, put two into
        hand, rest on the bottom in any order.
      * Pillage the Bog — top X (a ``Formula``), one into hand, rest on
        the bottom in a random order.
      * Once Upon a Time — top five, you MAY reveal a creature or land
        card and put it into your hand, rest on the bottom in a random
        order.
      * Malevolent Rumble — reveal the top four, you MAY put a permanent
        card into your hand, rest into your graveyard.

    ``optional`` (the printed "you may") lets the agent decline a pick
    (``choose_card_from_library`` returning ``None``). A mandatory pick
    falls back to the first eligible card when the agent declines.
    ``rest_random`` shuffles the leftovers before bottoming ("in a
    random order"); otherwise they keep their library order ("in any
    order" — the ordering choice is not modeled, see
    ``docs/SEARCH_LIMITATIONS.md``).
    """

    count: Amount = 1
    take: int = 1
    take_filter: CardFilter = ANY_CARD
    optional: bool = False
    reveal: bool = False
    reveal_taken: bool = False
    rest_zone: RestZone = RestZone.BOTTOM
    rest_random: bool = False

    def __post_init__(self) -> None:
        if self.rest_zone not in ("bottom", "graveyard"):
            raise ValueError(
                f"rest_zone must be 'bottom' or 'graveyard', got "
                f"{self.rest_zone!r}"
            )
        object.__setattr__(self, "rest_zone", RestZone(self.rest_zone))

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to, reveal_to_all

        p = state.player_by_id(controller_id)
        n = min(_amount(self.count, state, controller_id), len(p.library))
        looked = [p.library.pop(0) for _ in range(n)]
        for c in looked:
            if self.reveal:
                reveal_to_all(state, c)
            else:
                reveal_to(c, controller_id)
        taken: list = []
        for _ in range(self.take):
            options = [
                c for c in looked
                if c not in taken and self.take_filter.matches_card(state, c)
            ]
            if not options:
                break
            picked = None
            if p.agent is not None:
                picked = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in options],
                )
            chosen = _agent_pick(options, picked)
            if chosen is None:
                if self.optional:
                    break
                chosen = options[0]
            taken.append(chosen)
        for c in taken:
            c.zone = Zone.HAND
            p.hand.append(c)
            if self.reveal_taken:
                reveal_to_all(state, c)
        rest = [c for c in looked if c not in taken]
        if self.rest_random:
            state.rng.shuffle(rest)
        for c in rest:
            if self.rest_zone == "graveyard":
                c.zone = Zone.GRAVEYARD
                p.graveyard.append(c)
            else:
                c.zone = Zone.LIBRARY
                p.library.append(c)
        state.log.append(
            f"P{controller_id} looks at {n} card(s), takes {len(taken)}, "
            f"puts {len(rest)} into {self.rest_zone}"
        )

    def describe(self) -> str:
        verb = "reveal" if self.reveal else "look at"
        may = "you may put" if self.optional else "put"
        return (
            f"{verb} the top {_describe_amount(self.count)} cards of your "
            f"library; {may} {self.take} {self.take_filter.describe()}(s) "
            f"into your hand; the rest go to the {self.rest_zone}"
        )


@dataclass(frozen=True)
class ReorderTopCards(Effect):
    """"Look at the top N cards of your library, then put them back in
    any order." (Ponder's first sentence.)

    The new order is built top-down: each position is filled by a
    ``choose_card_from_library`` pick among the not-yet-placed cards.
    The reordered cards stay known to their owner.
    """

    count: int = 3

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        n = min(self.count, len(p.library))
        looked = [p.library.pop(0) for _ in range(n)]
        ordered: list = []
        remaining = list(looked)
        while remaining:
            chosen = remaining[0]
            if p.agent is not None and len(remaining) > 1:
                picked = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in remaining],
                )
                chosen = _agent_pick(remaining, picked) or remaining[0]
            remaining.remove(chosen)
            ordered.append(chosen)
        for c in reversed(ordered):
            c.zone = Zone.LIBRARY
            p.library.insert(0, c)
            reveal_to(c, controller_id)
        state.log.append(f"P{controller_id} reorders the top {n} card(s)")

    def describe(self) -> str:
        return (
            f"look at the top {self.count} cards of your library, then put "
            f"them back in any order"
        )


@dataclass(frozen=True)
class ShuffleYourLibrary(Effect):
    """"Shuffle." / "You may shuffle." (Ponder's second sentence.)

    With ``optional`` the controller decides via ``choose_yes_no``.
    """

    optional: bool = False

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        if self.optional and p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, "Shuffle your library?",
        ):
            state.log.append(f"P{controller_id} declines to shuffle")
            return
        shuffle_library(state, controller_id)
        state.log.append(f"P{controller_id} shuffles their library")

    def describe(self) -> str:
        return "you may shuffle" if self.optional else "shuffle"


@dataclass(frozen=True)
class LookAtTargetPlayersHand(Effect):
    """"Look at target player's hand." Every card in the target's hand
    becomes known to the controller (``reveal_to``)."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _player_targets(state, controller_id, opponent_only=False)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import reveal_to

        p = _target_player(state, targets)
        if p is None:
            return
        for c in p.hand:
            reveal_to(c, controller_id)
        state.log.append(f"P{controller_id} looks at P{p.id}'s hand")

    def describe(self) -> str:
        return "look at target player's hand"


@dataclass(frozen=True)
class ExileThisSpell(Effect):
    """"Exile <this spell's name>." as part of a spell's own resolution
    (Time Spiral). Moves ``state.resolving_spell`` to its owner's exile;
    ``resolve_top_of_stack`` sees the zone change and skips the usual
    graveyard routing."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        card = state.resolving_spell
        if card is None:
            return
        card.cast_via_flashback = False
        card.zone = Zone.EXILE
        state.player_by_id(card.owner_id).exile.append(card)
        state.log.append(f"{card.printing.name} exiles itself")

    def describe(self) -> str:
        return "exile this spell"


@dataclass(frozen=True)
class ShuffleThisSpellIntoOwnersLibrary(Effect):
    """"Shuffle <this spell's name> into its owner's library." as the
    last instruction of a spell's own resolution (Green Sun's Zenith;
    CR 608.2c). Moves ``state.resolving_spell`` from the stack into its
    owner's library and shuffles it; ``resolve_top_of_stack`` sees the
    zone change and skips the usual graveyard routing. Only as the spell
    resolves: countered, discarded or milled, the card goes to the
    graveyard as usual (unlike Nexus of Fate's "if it would be put into
    a graveyard from anywhere" replacement). A spell cast with flashback
    is exiled instead (CR 702.34a); a copy ceases to exist as it leaves
    the stack (CR 707.10a), so it is left to the usual routing."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, shuffle_library
        from mtgz_ir.turn import note_cards_put_into_library

        card = state.resolving_spell
        if card is None or card.zone is not Zone.STACK or card.is_spell_copy:
            return
        owner = state.player_by_id(card.owner_id)
        if card.cast_via_flashback:
            card.cast_via_flashback = False
            card.zone = Zone.EXILE
            owner.exile.append(card)
            state.log.append(f"{card.printing.name} is exiled (flashback)")
            return
        card.zone = Zone.LIBRARY
        owner.library.append(card)
        shuffle_library(state, owner.id)
        note_cards_put_into_library(state, owner.id)
        state.log.append(
            f"{card.printing.name} is shuffled into P{owner.id}'s library"
        )

    def describe(self) -> str:
        return "shuffle this spell into its owner's library"


# ---------------------------------------------------------------------
# Wheels / mass zone moves
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class EachPlayerShufflesZonesIntoLibrary(Effect):
    """"Each player shuffles their hand and graveyard into their library"
    (Timetwister, Echo of Eons, Time Spiral). ``zones`` is a subset of
    ``("hand", "graveyard")``. Cards leave their zone lists before their
    ``zone`` field is rewritten (copy-on-write safe)."""

    zones: tuple[ZoneName, ...] = (ZoneName.HAND, ZoneName.GRAVEYARD)

    def __post_init__(self) -> None:
        object.__setattr__(self, "zones", coerce_tuple(ZoneName, self.zones))

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, shuffle_library

        for i in range(len(state.players)):
            p = state.players[(state.active_player_idx + i) % len(state.players)]
            if p.has_lost:
                continue
            moved: list = []
            if "hand" in self.zones:
                moved.extend(p.hand)
                p.hand.clear()
            if "graveyard" in self.zones:
                moved.extend(p.graveyard)
                p.graveyard.clear()
            for c in moved:
                c.zone = Zone.LIBRARY
                p.library.append(c)
            shuffle_library(state, p.id)
            state.log.append(
                f"P{p.id} shuffles {len(moved)} card(s) into their library"
            )
            if moved:
                from mtgz_ir.turn import note_cards_put_into_library

                note_cards_put_into_library(state, p.id)

    def describe(self) -> str:
        return (
            "each player shuffles their "
            + " and ".join(self.zones) + " into their library"
        )


@dataclass(frozen=True)
class UntapUpToNLands(Effect):
    """"Untap up to N lands." (Frantic Search, Time Spiral.)

    Untaps up to ``amount`` tapped lands the controller controls,
    preferring lands that produce the most colors. Untapping an
    opponent's land is legal but never in the caster's interest, so the
    choice is restricted to the controller's own lands (documented in
    ``docs/SEARCH_LIMITATIONS.md``).
    """

    amount: int = 1

    def resolve(self, state, controller_id, targets=()):
        tapped = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and o.printing.is_land and o.tapped
        ]
        tapped.sort(key=lambda o: (-len(o.printing.produces), o.id))
        for o in tapped[: self.amount]:
            o.tapped = False
        state.log.append(
            f"P{controller_id} untaps {min(self.amount, len(tapped))} land(s)"
        )

    def describe(self) -> str:
        return f"untap up to {self.amount} lands"


@dataclass(frozen=True)
class ReturnAllPermanentsToOwnersHands(Effect):
    """"Return all permanents to their owners' hands." (Upheaval.)

    Attached permanents (Auras, Equipment) are returned first so an
    Aura isn't buried as a state-based consequence of its host leaving
    — every permanent leaves simultaneously. Tokens cease to exist.
    """

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import move_off_battlefield

        snapshot = sorted(
            state.battlefield, key=lambda o: (o.attached_to is None, o.id),
        )
        for obj in snapshot:
            if obj in state.battlefield:
                move_off_battlefield(state, obj, destination="hand")
        state.log.append(
            f"{len(snapshot)} permanent(s) return to their owners' hands"
        )

    def describe(self) -> str:
        return "return all permanents to their owners' hands"


@dataclass(frozen=True)
class ReturnTargetSpellToOwnersHand(Effect):
    """"Return target spell to its owner's hand." (Reprieve.)

    Targets a spell on the stack by index (the ``"stack"`` target kind
    ``CounterTargetSpell`` uses). The spell leaves the stack without
    resolving; a spell cast with flashback is exiled instead (CR
    702.34a) via :func:`mtgz_ir.actions.move_spell_off_stack`.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("stack", i) for i, item in enumerate(state.stack)
            if item.ability is None and not item.countered
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import move_spell_off_stack

        if not targets:
            return
        kind, idx = targets[0]
        if kind != "stack" or not (0 <= idx < len(state.stack)):
            return
        item = state.stack[idx]
        if item.ability is not None or item.countered:
            return
        del state.stack[idx]
        move_spell_off_stack(state, item.source, destination="hand")

    def describe(self) -> str:
        return "return target spell to its owner's hand"


# ---------------------------------------------------------------------
# Distribute / reveal-until / repeat-look (Expressive Iteration,
# Demonic Consultation, Lim-Dul's Vault, Occult Epiphany)
# ---------------------------------------------------------------------


DISTRIBUTE_PILES = ("hand", "bottom", "exile_playable")


@dataclass(frozen=True)
class LookAtTopCardsDistribute(Effect):
    """Look at the top ``count`` cards and put one into each of
    ``piles`` in order (Expressive Iteration: "Put one of them into your
    hand, put one of them on the bottom of your library, and exile one
    of them. You may play the exiled card this turn.").

    ``"exile_playable"`` exiles the card with
    ``playable_from_exile_this_turn`` set (the existing impulse-play
    permission, cleared at cleanup). Each pick goes through
    ``choose_card_from_library`` among the cards still unplaced.
    """

    count: int = 3
    piles: tuple[DistributePile, ...] = (
        DistributePile.HAND, DistributePile.BOTTOM, DistributePile.EXILE_PLAYABLE,
    )

    def __post_init__(self) -> None:
        for pile in self.piles:
            if pile not in DISTRIBUTE_PILES:
                raise ValueError(f"unknown pile {pile!r}")
        object.__setattr__(self, "piles", coerce_tuple(DistributePile, self.piles))

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        n = min(self.count, len(p.library))
        remaining = [p.library.pop(0) for _ in range(n)]
        for c in remaining:
            reveal_to(c, controller_id)
        for pile in self.piles:
            if not remaining:
                break
            chosen = remaining[0]
            if p.agent is not None and len(remaining) > 1:
                picked = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in remaining],
                )
                chosen = _agent_pick(remaining, picked) or remaining[0]
            remaining.remove(chosen)
            if pile == "hand":
                chosen.zone = Zone.HAND
                p.hand.append(chosen)
            elif pile == "bottom":
                chosen.zone = Zone.LIBRARY
                p.library.append(chosen)
            else:
                chosen.zone = Zone.EXILE
                chosen.playable_from_exile_this_turn = True
                p.exile.append(chosen)
                state.log.append(
                    f"P{controller_id} exiles {chosen.printing.name} "
                    f"(may play it this turn)"
                )
        # Leftovers (only when fewer piles than cards) go back on top.
        for c in reversed(remaining):
            c.zone = Zone.LIBRARY
            p.library.insert(0, c)

    def describe(self) -> str:
        return (
            f"look at the top {self.count} cards of your library and "
            f"distribute them: {', '.join(self.piles)}"
        )


NAME_NOT_IN_LIBRARY = "(a card name not in your library)"


@dataclass(frozen=True)
class ChooseNameExileThenRevealUntilNamed(Effect):
    """Demonic Consultation: "Choose a card name. Exile the top N cards
    of your library, then reveal cards from the top of your library
    until you reveal a card with the chosen name. Put that card into
    your hand and exile all other cards revealed this way."

    The name is chosen through ``choose_mode`` over the distinct names
    in the caster's library plus one entry standing for any name not in
    it (which exiles the whole library). Only one card in the pool has
    this shape; the exile count is the parameter.
    """

    exile_first: int = 6

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        names = sorted({c.printing.name for c in p.library})
        options = names + [NAME_NOT_IN_LIBRARY]
        idx = len(options) - 1
        if p.agent is not None:
            picked = p.agent.choose_mode(state, controller_id, options)
            if isinstance(picked, int) and 0 <= picked < len(options):
                idx = picked
        chosen_name = options[idx]
        state.log.append(f"P{controller_id} names {chosen_name}")
        exiled = 0
        for _ in range(min(self.exile_first, len(p.library))):
            c = p.library.pop(0)
            c.zone = Zone.EXILE
            p.exile.append(c)
            exiled += 1
        found = None
        while p.library:
            c = p.library.pop(0)
            reveal_to_all(state, c)
            if c.printing.name == chosen_name:
                found = c
                break
            c.zone = Zone.EXILE
            p.exile.append(c)
            exiled += 1
        if found is not None:
            found.zone = Zone.HAND
            p.hand.append(found)
        state.log.append(
            f"P{controller_id} exiles {exiled} card(s)"
            + (f" and puts {found.printing.name} into hand" if found else "")
        )

    def describe(self) -> str:
        return (
            f"choose a card name; exile the top {self.exile_first} cards, "
            f"then reveal until that name; put it into your hand and exile "
            f"the rest"
        )


@dataclass(frozen=True)
class LookAtTopRepeatPayLifeThenStackTop(Effect):
    """Lim-Dul's Vault: look at the top ``count`` cards; any number of
    times, pay ``life_per_repeat`` life to put them on the bottom (in
    their current order) and look at the next ``count``; then shuffle
    and put the last cards looked at on top in any order.

    Each repeat is a ``choose_yes_no``; the offer stops once another
    payment would drop the controller to 0 life or every card of the
    library has been seen (further windows would only repeat). The final
    top-of-library order is built with ``choose_card_from_library``
    picks, like :class:`ReorderTopCards`.
    """

    count: int = 5
    life_per_repeat: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to, shuffle_library

        p = state.player_by_id(controller_id)
        n = min(self.count, len(p.library))
        looked = [p.library.pop(0) for _ in range(n)]
        max_repeats = len(p.library) // max(1, self.count) + 1 if p.library else 0
        repeats = 0
        while (
            repeats < max_repeats
            and p.library
            and p.life > self.life_per_repeat
            and p.agent is not None
            and p.agent.choose_yes_no(
                state, controller_id,
                f"Pay {self.life_per_repeat} life to bottom these and look "
                f"at the next {self.count}?",
            )
        ):
            p.life -= self.life_per_repeat
            for c in looked:
                c.zone = Zone.LIBRARY
                p.library.append(c)
            n = min(self.count, len(p.library))
            looked = [p.library.pop(0) for _ in range(n)]
            repeats += 1
        state.log.append(
            f"P{controller_id} pays {repeats * self.life_per_repeat} life "
            f"to look at new cards"
        )
        shuffle_library(state, controller_id)
        ordered: list = []
        remaining = list(looked)
        while remaining:
            chosen = remaining[0]
            if p.agent is not None and len(remaining) > 1:
                picked = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in remaining],
                )
                chosen = _agent_pick(remaining, picked) or remaining[0]
            remaining.remove(chosen)
            ordered.append(chosen)
        for c in reversed(ordered):
            c.zone = Zone.LIBRARY
            p.library.insert(0, c)
            reveal_to(c, controller_id)

    def describe(self) -> str:
        return (
            f"look at the top {self.count} cards; repeatedly pay "
            f"{self.life_per_repeat} life to bottom them and look again; "
            f"then shuffle and put the last ones on top"
        )


@dataclass(frozen=True)
class DrawDiscardThenTokensPerDiscardedType(Effect):
    """Occult Epiphany: "Draw X cards, then discard X cards. Create a
    <token> for each card type among cards discarded this way."

    ``amount`` is an int or ``Formula`` (``ResolvingX``); the discards
    are the controller's own picks (``choose_discard``)."""

    amount: Amount = 1
    token_printing: "object" = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.tokens import CreateToken
        from mtgz_ir.turn import draw_n_cards

        p = state.player_by_id(controller_id)
        n = _amount(self.amount, state, controller_id)
        draw_n_cards(state, controller_id, n)
        discarded: list = []
        for _ in range(n):
            if not p.hand:
                break
            chosen = p.hand[0]
            if p.agent is not None:
                picked = p.agent.choose_discard(
                    state, controller_id, [c.id for c in p.hand],
                )
                chosen = _agent_pick(p.hand, picked) or p.hand[0]
            discard_card(state, p, chosen)
            discarded.append(chosen)
        from mtgz_ir.effects.card_types import count_card_types

        n_types = count_card_types(discarded)
        if n_types and self.token_printing is not None:
            CreateToken(
                token_printing=self.token_printing, count=n_types,
            ).resolve(state, controller_id)

    def describe(self) -> str:
        return (
            f"draw {_describe_amount(self.amount)} cards, then discard "
            f"{_describe_amount(self.amount)} cards; create a token for "
            f"each card type among the discarded cards"
        )


@dataclass(frozen=True)
class TargetPlayerDiscards(Effect):
    """"Target player discards N cards." — the discarding player picks
    (``choose_discard``). Any player may be targeted (Chain of Smog)."""

    amount: Amount = 1

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _player_targets(state, controller_id, opponent_only=False)

    def resolve(self, state, controller_id, targets=()):
        p = _target_player(state, targets)
        if p is None:
            return
        for _ in range(_amount(self.amount, state, controller_id)):
            if not p.hand:
                break
            chosen = p.hand[0]
            if p.agent is not None:
                picked = p.agent.choose_discard(state, p.id, [c.id for c in p.hand])
                chosen = _agent_pick(p.hand, picked) or p.hand[0]
            discard_card(state, p, chosen)

    def describe(self) -> str:
        return f"target player discards {_describe_amount(self.amount)} card(s)"


@dataclass(frozen=True)
class ReturnTargetSpellOrNonlandPermanentToHand(Effect):
    """"Return target spell or nonland permanent [an opponent controls]
    to its owner's hand." (Sink into Stupor.) A spell target is a
    ``("stack", index)``; a permanent target bounces through
    ``move_off_battlefield(destination="hand")``."""

    opponent_only: bool = True

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        from mtgz_ir.game import is_creature_now

        out = [
            ("stack", i) for i, item in enumerate(state.stack)
            if item.ability is None and not item.countered
            and not (self.opponent_only and item.controller_id == controller_id)
        ]
        for o in state.battlefield:
            if o.printing.is_land or o.phased_out:
                continue
            if self.opponent_only and o.controller_id == controller_id:
                continue
            out.append(("creature" if is_creature_now(o) else "permanent", o.id))
        return out

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import move_spell_off_stack
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        kind, tid = targets[0]
        if kind == "stack":
            if not (0 <= tid < len(state.stack)):
                return
            item = state.stack[tid]
            if item.ability is not None or item.countered:
                return
            del state.stack[tid]
            move_spell_off_stack(state, item.source, destination="hand")
            return
        obj = next((o for o in state.battlefield if o.id == tid), None)
        if obj is None or obj.printing.is_land:
            return
        if self.opponent_only and obj.controller_id == controller_id:
            return
        move_off_battlefield(state, obj, destination="hand")

    def describe(self) -> str:
        who = " an opponent controls" if self.opponent_only else ""
        return f"return target spell or nonland permanent{who} to its owner's hand"
