"""Engine effects for the Vintage Roto P2 package: library, graveyard
and hand (``docs/ROTO_COVERAGE_PLAN.md`` section 2).

Grammar lives in :mod:`mtgz_ir.cards.parser.roto_zones`; every class
here models one printed shape faithfully (shortcuts are listed in
``docs/SEARCH_LIMITATIONS.md`` under "Roto P2").

Library searches made by this module's effects go through
:func:`search_pick`, the one place a P2 search picks a card.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import AffectedPlayers, GraveyardOwner, plain
from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.conditions import Condition as _Condition
from mtgz_ir.effects.predicates import TriggerPredicate as _TriggerPredicate
from mtgz_ir.effects.generic.card_flow import CardFilter
from mtgz_ir.effects.generic.roto_search import (
    library_search,
    run_library_search,
    searched_card_taken,
)
from mtgz_ir.effects.generic.targeted_removal import RemovalRider

if TYPE_CHECKING:
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.game import GameObject, GameState, Player


# ---------------------------------------------------------------------
# Card filters
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class RotoCardFilter(CardFilter):
    """:class:`CardFilter` plus the descriptor words the roto tutors and
    reanimation spells print on top of the base grammar:

    * ``excluded_supertypes`` -- "a nonlegendary card" (Unmarked Grave,
      Persist).
    * ``max_power`` / ``max_toughness`` -- "a creature card with power 2
      or less" (Imperial Recruiter, Reveillark) / "with toughness 2 or
      less" (Recruiter of the Guard). A card with no printed power (a
      ``*`` or a noncreature) has power 0 in every zone but the stack
      and battlefield (CR 208.3), so it matches.
    """

    excluded_supertypes: tuple[str, ...] = ()
    max_power: "int | None" = None
    max_toughness: "int | None" = None

    @property
    def has_quality(self) -> bool:
        return (
            super().has_quality
            or bool(self.excluded_supertypes)
            or self.max_power is not None
            or self.max_toughness is not None
        )

    def matches(self, printing: "CardPrinting", **effective) -> bool:
        if not super().matches(printing, **effective):
            return False
        for s in self.excluded_supertypes:
            if s in printing.supertypes:
                return False
        if self.max_power is not None and (printing.power or 0) > self.max_power:
            return False
        if (
            self.max_toughness is not None
            and (printing.toughness or 0) > self.max_toughness
        ):
            return False
        return True

    def describe(self) -> str:
        text = super().describe()
        for s in self.excluded_supertypes:
            text = f"non{s.lower()} {text}"
        if self.max_power is not None:
            text += f" with power {self.max_power} or less"
        if self.max_toughness is not None:
            text += f" with toughness {self.max_toughness} or less"
        return text


# ---------------------------------------------------------------------
# Library search helper
# ---------------------------------------------------------------------


def search_pick(
    state: "GameState", searcher: "Player", options: list,
    *, may_fail: bool,
) -> "GameObject | None":
    """The searching player's pick among ``options`` (cards in their
    library matching the search's description). ``may_fail`` is CR
    701.19b: a search for a card with a stated quality may find nothing
    even when one is there (the agent returns ``None``); a search for
    just "a card" finds one whenever the library has any."""
    if not options:
        return None
    picked = None
    if searcher.agent is not None:
        picked = searcher.agent.choose_card_from_library(
            state, searcher.id, [c.id for c in options],
        )
    chosen = next((c for c in options if c.id == picked), None)
    if chosen is None and not may_fail:
        chosen = options[0]
    return chosen


# ---------------------------------------------------------------------
# Tutors (F10)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class SearchToHandDiscardAtRandomThenShuffle(Effect):
    """"Search your library for <a card>, put that card into your hand,
    discard a card at random, then shuffle." (Gamble.) The random
    discard is from the whole hand -- it may be the card just found."""

    card_filter: CardFilter = CardFilter()

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.game import Zone, reveal_to, shuffle_library

        p = state.player_by_id(controller_id)
        options = [c for c in p.library if self.card_filter.matches_card(state, c)]
        found = search_pick(
            state, p, options, may_fail=self.card_filter.has_quality,
        )
        if found is not None:
            p.library.remove(found)
            if searched_card_taken(state, controller_id, found):
                found = None
        if found is not None:
            # Into the hand (not the graveyard) before the random discard.
            note_left_library(state, p, found)
            found.zone = Zone.HAND
            p.hand.append(found)
            reveal_to(found, controller_id)
            state.log.append(f"P{controller_id} tutors a card into hand")
        if p.hand:
            card = p.hand[state.rng.randrange(len(p.hand))]
            discard_card(state, p, card)
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            f"search your library for a {self.card_filter.describe()}, put "
            f"it into your hand, discard a card at random, then shuffle"
        )


@dataclass(frozen=True)
class SearchOntoBattlefieldTappedUntapIfLands(Effect):
    """"Search your library for <a basic land card>, put it onto the
    battlefield tapped, then shuffle. Then if you control N or more
    lands, untap that land." (Fabled Passage.) The land count is taken
    after the found land is on the battlefield."""

    card_filter: CardFilter = CardFilter()
    min_lands: int = 4

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        options = [c for c in p.library if self.card_filter.matches_card(state, c)]
        found = search_pick(
            state, p, options, may_fail=self.card_filter.has_quality,
        )
        if found is not None:
            p.library.remove(found)
            if searched_card_taken(state, controller_id, found):
                found = None
        if found is not None:
            state.log.append(
                f"P{controller_id} puts {found.printing.name} onto the "
                f"battlefield tapped"
            )
            put_into_battlefield(state, found, controller_id)
            if found in state.battlefield:
                found.tapped = True
        shuffle_library(state, controller_id)
        if found is None or found not in state.battlefield:
            return
        lands = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_land
        )
        if lands >= self.min_lands:
            found.tapped = False
            state.log.append(f"{found.printing.name} untaps")

    def describe(self) -> str:
        return (
            f"search your library for a {self.card_filter.describe()}, put "
            f"it onto the battlefield tapped, then shuffle; then if you "
            f"control {self.min_lands} or more lands, untap that land"
        )


@dataclass(frozen=True)
class SearchAnyNumberTotalManaValueOntoBattlefield(Effect):
    """"Search your library for any number of <creature> cards with total
    mana value N or less, put them onto the battlefield, then shuffle."
    (Protean Hulk.) Picks go one at a time through
    ``choose_card_from_library`` (``None`` stops); each pick is limited
    to cards that still fit the remaining mana-value budget."""

    card_filter: CardFilter = CardFilter()
    max_total_mv: int = 6

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        budget = self.max_total_mv
        found = []
        while True:
            options = [
                c for c in p.library
                if self.card_filter.matches_card(state, c)
                and c.printing.mana_cost.total <= budget
            ]
            pick = search_pick(state, p, options, may_fail=True)
            if pick is None:
                break
            p.library.remove(pick)
            budget -= pick.printing.mana_cost.total
            if not searched_card_taken(state, controller_id, pick):
                found.append(pick)
        for card in found:
            put_into_battlefield(state, card, controller_id)
        shuffle_library(state, controller_id)
        state.log.append(
            f"P{controller_id} puts {len(found)} card(s) with total mana "
            f"value {self.max_total_mv - budget} onto the battlefield"
        )

    def describe(self) -> str:
        return (
            f"search your library for any number of "
            f"{self.card_filter.describe()}s with total mana value "
            f"{self.max_total_mv} or less, put them onto the battlefield, "
            f"then shuffle"
        )


@dataclass(frozen=True)
class ExileSourceCardFromGraveyard(Effect):
    """"[You may] exile it." in a dies trigger: exile the card that went
    to the graveyard (the trigger's source). Returns ``False`` when the
    card is no longer in its owner's graveyard (it left, so it can't be
    exiled and an "If you do" rider doesn't happen)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        card = state.resolving_source
        if card is None:
            return False
        owner = state.player_by_id(card.owner_id)
        if card not in owner.graveyard:
            return False
        owner.graveyard.remove(card)
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(f"{card.printing.name} is exiled from the graveyard")
        return True

    def describe(self) -> str:
        return "exile it"


@dataclass(frozen=True)
class ControllerMaySearchOntoBattlefield(RemovalRider):
    """"Its controller may search their library for <a basic land card>,
    put it onto the battlefield[ tapped], then shuffle." (Ghost Quarter's
    untapped search; the tapped Path to Exile form is the base's
    ``ControllerSearchesForBasicLand``.) The removed permanent's last
    controller decides (``choose_yes_no``) and picks the card; declining
    skips the search and the shuffle."""

    card_filter: CardFilter = CardFilter()
    tapped: bool = False

    def apply(self, state, info):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        pid = info.controller_id
        p = state.player_by_id(pid)
        if p.has_lost:
            return
        if p.agent is not None and not p.agent.choose_yes_no(
            state, pid, f"search your library for a {self.card_filter.describe()}?",
        ):
            state.log.append(f"P{pid} declines to search")
            return
        def search():
            options = [
                c for c in p.library if self.card_filter.matches_card(state, c)
            ]
            found = search_pick(state, p, options, may_fail=True)
            if found is not None:
                p.library.remove(found)
                if searched_card_taken(state, pid, found):
                    found = None
            if found is not None:
                state.log.append(
                    f"P{pid} puts {found.printing.name} onto the battlefield"
                )
                put_into_battlefield(state, found, pid)
                if self.tapped and found in state.battlefield:
                    found.tapped = True
            shuffle_library(state, pid)

        run_library_search(state, pid, search)

    def describe(self) -> str:
        tapped = " tapped" if self.tapped else ""
        return (
            f"its controller may search their library for a "
            f"{self.card_filter.describe()}, put it onto the battlefield"
            f"{tapped}, then shuffle"
        )


# ---------------------------------------------------------------------
# Graveyard hate & exile-instead replacement (F12)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class GraveyardExileInstead:
    """A ``CardPrinting.zone_abilities`` spec: "If <what> would be put
    into <whose> graveyard[ from anywhere], exile it instead."

    * ``opponents_only`` -- "an opponent's graveyard" (Leyline of the
      Void): only cards owned by the source's controller's opponents.
    * ``tokens`` -- tokens are affected too ("a card or token", Rest in
      Peace; "a ... permanent", Sanctifier en-Vec). A token exiled this
      way never dies; it then ceases to exist.
    * ``colors`` -- only objects of one of these colors ("a black or red
      permanent, spell, or card not on the battlefield"); empty = any.

    Copies of spells are neither cards nor tokens and are never
    affected (they cease to exist anyway, CR 707.10a).
    """

    opponents_only: bool = False
    tokens: bool = False
    colors: frozenset = frozenset()

    def affects(self, state, source, obj) -> bool:
        from mtgz_ir.game import effective_colors

        if getattr(obj, "is_spell_copy", False):
            return False
        if obj.printing.is_token and not self.tokens:
            return False
        if self.opponents_only and obj.owner_id == source.controller_id:
            return False
        if self.colors and not (effective_colors(state, obj) & self.colors):
            return False
        return True

    def affects_player(self, source, player_id: int) -> bool:
        return not (self.opponents_only and player_id == source.controller_id)

    def describe(self) -> str:
        what = "a card or token" if self.tokens else "a card"
        if self.colors:
            what = "a " + " or ".join(sorted(self.colors)) + " object"
        whose = "an opponent's" if self.opponents_only else "a"
        return f"if {what} would be put into {whose} graveyard, exile it instead"


def _exile_instead_sources(state):
    """``(source, spec)`` for every battlefield permanent with an active
    exile-instead replacement (a source that lost all abilities has
    none)."""
    from mtgz_ir.game import _source_layer6_wiped

    out = []
    for o in state.battlefield:
        for spec in o.printing.zone_abilities:
            if isinstance(spec, GraveyardExileInstead) and not (
                state.continuous_effects and _source_layer6_wiped(state, o)
            ):
                out.append((o, spec))
    return out


def graveyard_exile_instead_applies(state, obj) -> bool:
    """Would ``obj`` (a card or token about to be put into a graveyard)
    be exiled instead? Consulted by ``turn.move_off_battlefield`` (so an
    affected creature never dies), ``actions._send_to_graveyard_or_
    library`` (spells leaving the stack) and this module's own
    graveyard moves; :func:`apply_graveyard_exile_instead` catches every
    other path."""
    return any(
        spec.affects(state, src, obj) for src, spec in _exile_instead_sources(state)
    )


def exile_instead_of_graveyard(state, card) -> None:
    """Put ``card`` (already out of its previous zone) into its owner's
    exile, replacing a move to the graveyard."""
    from mtgz_ir.game import Zone

    card.zone = Zone.EXILE
    state.player_by_id(card.owner_id).exile.append(card)
    state.log.append(f"{card.printing.name} is exiled instead of going to a graveyard")


def put_into_graveyard(state, card) -> None:
    """Move ``card`` (already out of its previous zone) to its owner's
    graveyard, or into exile if an exile-instead replacement applies.
    Used by this module's effects so their own graveyard moves honor the
    replacement immediately."""
    from mtgz_ir.game import Zone

    if graveyard_exile_instead_applies(state, card):
        exile_instead_of_graveyard(state, card)
        return
    card.zone = Zone.GRAVEYARD
    state.player_by_id(card.owner_id).graveyard.append(card)


def note_exile_instead_source_entered(state, obj) -> None:
    """An exile-instead source entered the battlefield: the cards
    already in the graveyards it affects stay (the replacement applies
    only from now on); snapshot them for the SBA catch-all."""
    for spec in obj.printing.zone_abilities:
        if not isinstance(spec, GraveyardExileInstead):
            continue
        for p in state.players:
            if spec.affects_player(obj, p.id):
                p.graveyard_exile_instead_snapshot = frozenset(
                    c.id for c in p.graveyard
                )


def apply_graveyard_exile_instead(state) -> bool:
    """State-based catch-all for the exile-instead replacements: an
    affected card that reached a graveyard through a path that doesn't
    consult the replacement directly (discard, mill, ...) is exiled.
    Cards that were already there when the replacement began applying
    (``Player.graveyard_exile_instead_snapshot``) stay; the snapshot is
    pruned to the current graveyard so a card that leaves and comes back
    is caught."""
    sources = _exile_instead_sources(state)
    if not sources:
        return False
    changed = False
    for p in state.players:
        snap = p.graveyard_exile_instead_snapshot
        movers = [
            c for c in p.graveyard
            if c.id not in snap
            and any(spec.affects(state, src, c) for src, spec in sources)
        ]
        for c in movers:
            p.graveyard.remove(c)
            exile_instead_of_graveyard(state, c)
            changed = True
        p.graveyard_exile_instead_snapshot = frozenset(
            c.id for c in p.graveyard if c.id in snap
        )
    return changed


@dataclass(frozen=True)
class ExileTargetPlayersGraveyard(Effect):
    """"Exile target player's graveyard." (Bojuka Bog.)"""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        p = next(
            (state.player_by_id(t[1]) for t in targets if t[0] == "player"),
            None,
        )
        if p is None or p.has_lost:
            return
        n = len(p.graveyard)
        for c in list(p.graveyard):
            p.graveyard.remove(c)
            c.zone = Zone.EXILE
            p.exile.append(c)
        state.log.append(f"P{p.id}'s graveyard is exiled ({n} cards)")

    def describe(self) -> str:
        return "exile target player's graveyard"


@dataclass(frozen=True)
class ExileGraveyardCardsOfColors(Effect):
    """"Exile all cards that are <black or red> from all graveyards."
    (Sanctifier en-Vec.)"""

    colors: frozenset = frozenset()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, effective_colors

        n = 0
        for p in state.players:
            for c in list(p.graveyard):
                if effective_colors(state, c) & self.colors:
                    p.graveyard.remove(c)
                    c.zone = Zone.EXILE
                    p.exile.append(c)
                    n += 1
        state.log.append(f"{n} card(s) exiled from all graveyards")

    def describe(self) -> str:
        return (
            "exile all cards that are " + " or ".join(sorted(self.colors))
            + " from all graveyards"
        )


# ---------------------------------------------------------------------
# Hand disruption & symmetric attrition (F16)
# ---------------------------------------------------------------------


def _player_target(state, targets) -> "Player | None":
    p = next(
        (state.player_by_id(t[1]) for t in targets if t[0] == "player"), None,
    )
    if p is None or p.has_lost:
        return None
    return p


def _pick_from_hand(state, chooser_id: int, options: list):
    """The ``chooser_id`` player's pick among ``options`` (cards in a
    hand they can see), through ``choose_card_from_opponent_hand``; an
    answer outside the options falls back to the first."""
    chooser = state.player_by_id(chooser_id)
    pick = options[0].id
    if chooser.agent is not None:
        pick = chooser.agent.choose_card_from_opponent_hand(
            state, chooser_id, [c.id for c in options],
        )
    return next((c for c in options if c.id == pick), options[0])


@dataclass(frozen=True)
class RevealHandChooseExileUntilSourceLeaves(Effect):
    """"Target opponent reveals their hand. You choose a <noncreature,
    nonland> card from it. Exile that card until this creature leaves
    the battlefield." (Kitesail Freebooter.) The pick is mandatory when
    one exists. If the source already left the battlefield when this
    resolves, the hand is still revealed but nothing is exiled (CR
    610.3c). The card returns to its owner's hand through the base's
    until-leaves release (``exiled_until_leaves_from_hand``)."""

    card_filter: CardFilter = CardFilter()
    opponent_only: bool = True

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and not (self.opponent_only and p.id == controller_id)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all

        p = _player_target(state, targets)
        if p is None:
            return
        for c in p.hand:
            reveal_to_all(state, c)
        src = state.resolving_source
        if src is None or not any(o.id == src.id for o in state.battlefield):
            return
        options = [c for c in p.hand if self.card_filter.matches_card(state, c)]
        if not options:
            return
        card = _pick_from_hand(state, controller_id, options)
        p.hand.remove(card)
        card.zone = Zone.EXILE
        card.exiled_by_source = src.id
        card.exiled_until_leaves_from_hand = True
        p.exile.append(card)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} from P{p.id}'s hand "
            f"until {src.printing.name} leaves the battlefield"
        )

    def describe(self) -> str:
        who = "opponent" if self.opponent_only else "player"
        return (
            f"target {who} reveals their hand; you choose a "
            f"{self.card_filter.describe()} from it; exile it until this "
            f"creature leaves the battlefield"
        )


@dataclass(frozen=True)
class LookAtHandMayBottomCardThenDraw(Effect):
    """"Look at target player's hand. You may choose a <nonland> card
    from it. If you do, that player reveals the chosen card, puts it on
    the bottom of their library, then draws a card." (Vendilion Clique.)
    Declining is ``choose_yes_no``."""

    card_filter: CardFilter = CardFilter()

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to, reveal_to_all
        from mtgz_ir.turn import draw_n_cards, note_cards_put_into_library

        p = _player_target(state, targets)
        if p is None:
            return
        for c in p.hand:
            reveal_to(c, controller_id)
        options = [c for c in p.hand if self.card_filter.matches_card(state, c)]
        if not options:
            return
        me = state.player_by_id(controller_id)
        if me.agent is not None and not me.agent.choose_yes_no(
            state, controller_id,
            f"choose a {self.card_filter.describe()} from P{p.id}'s hand?",
        ):
            return
        card = _pick_from_hand(state, controller_id, options)
        reveal_to_all(state, card)
        p.hand.remove(card)
        card.zone = Zone.LIBRARY
        p.library.append(card)
        note_cards_put_into_library(state, p.id)
        state.log.append(
            f"P{p.id} puts {card.printing.name} on the bottom of their library"
        )
        draw_n_cards(state, p.id, 1)

    def describe(self) -> str:
        return (
            f"look at target player's hand; you may choose a "
            f"{self.card_filter.describe()} from it; if you do, that player "
            f"puts it on the bottom of their library, then draws a card"
        )


#: ``EachPlayerSequence`` step kinds.
ATTRITION_STEPS = ("lose_life", "discard", "sacrifice_creature", "sacrifice_land")
#: The "a third of ..., rounded up" amount (Pox).
A_THIRD = "third"


def _amount_of(amount, total: int) -> int:
    if amount == A_THIRD:
        return -(-total // 3)
    return amount


def _is_creature(obj) -> bool:
    from mtgz_ir.game import is_creature_now

    return is_creature_now(obj)


def _discard_chosen(state, p) -> bool:
    """``p`` discards a card of their choice; ``False`` when nothing was
    discarded (empty hand, or protected from an opponent's discard)."""
    from mtgz_ir.effects.generic.card_flow import discard_card

    if not p.hand:
        return False
    ids = [c.id for c in p.hand]
    pick = ids[0]
    if p.agent is not None:
        pick = p.agent.choose_discard(state, p.id, ids)
    card = next((c for c in p.hand if c.id == pick), p.hand[0])
    discard_card(state, p, card)
    return card not in p.hand


def _sacrifice_chosen(state, p, options: list) -> bool:
    """``p`` sacrifices one of ``options``; ``False`` when it didn't
    happen (protected from an opponent's sacrifice)."""
    from mtgz_ir.turn import sacrifice

    pick = options[0].id
    if p.agent is not None:
        pick = p.agent.choose_sacrifice(state, p.id, [o.id for o in options])
    obj = next((o for o in options if o.id == pick), options[0])
    sacrifice(state, obj)
    return obj not in state.battlefield


@dataclass(frozen=True)
class EachPlayerSequence(Effect):
    """"Each player loses 1 life, discards a card, sacrifices a creature
    of their choice, then sacrifices a land of their choice." (Smallpox;
    Pox with ``A_THIRD`` amounts: "a third of their life / of the cards
    in their hand / of the creatures / lands they control ... Round up
    each time.")

    Each ``(kind, amount)`` step (kinds in ``ATTRITION_STEPS``) is
    performed by every player, in turn order from the active player,
    before the next step starts; a third is counted when its step
    begins. Discards are chosen with ``choose_discard`` and sacrifices
    with ``choose_sacrifice``."""

    steps: tuple = ()

    def resolve(self, state, controller_id, targets=()):
        n = len(state.players)
        order = [
            state.players[(state.active_player_idx + i) % n] for i in range(n)
        ]
        for kind, amount in self.steps:
            for p in order:
                if p.has_lost:
                    continue
                if kind == "lose_life":
                    k = _amount_of(amount, max(0, p.life))
                    p.life -= k
                    state.log.append(f"P{p.id} loses {k} life")
                elif kind == "discard":
                    for _ in range(_amount_of(amount, len(p.hand))):
                        if not _discard_chosen(state, p):
                            break
                else:
                    want_land = kind == "sacrifice_land"
                    mine = [
                        o for o in state.battlefield
                        if o.controller_id == p.id and (
                            o.printing.is_land if want_land else _is_creature(o)
                        )
                    ]
                    for _ in range(_amount_of(amount, len(mine))):
                        mine = [o for o in mine if o in state.battlefield]
                        if not mine or not _sacrifice_chosen(state, p, mine):
                            break

    def describe(self) -> str:
        words = {
            "lose_life": "loses {} life", "discard": "discards {} card(s)",
            "sacrifice_creature": "sacrifices {} creature(s)",
            "sacrifice_land": "sacrifices {} land(s)",
        }
        parts = [
            words[k].format("a third of" if a == A_THIRD else a)
            for k, a in self.steps
        ]
        return "each player " + ", then ".join(parts)


@dataclass(frozen=True)
class EachOpponentDiscardsLoseLifeUnlessNonland(Effect):
    """"Each opponent discards a card, then each opponent who didn't
    discard a nonland card this way loses N life." (Kroxa, Titan of
    Death's Hunger.) An opponent with an empty hand (or whose discard was
    prevented) discarded no nonland card, so loses the life."""

    life: int = 3

    def resolve(self, state, controller_id, targets=()):
        losers = []
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            before = {c.id for c in p.graveyard} | {c.id for c in p.exile}
            if not _discard_chosen(state, p):
                losers.append(p)
                continue
            # The card just discarded (it may have been exiled instead).
            gone = [
                c for c in list(p.graveyard) + list(p.exile)
                if c.id not in before
            ]
            if not any(not c.printing.is_land for c in gone):
                losers.append(p)
        for p in losers:
            p.life -= self.life
            state.log.append(f"P{p.id} loses {self.life} life")

    def describe(self) -> str:
        return (
            f"each opponent discards a card, then each opponent who didn't "
            f"discard a nonland card this way loses {self.life} life"
        )


@dataclass(frozen=True)
class RevealHandChooseFromHandAndGraveyardExile(Effect):
    """"Target opponent reveals their hand. You choose an <artifact or
    creature> card from it, then choose an <artifact or creature> card
    from their graveyard. Exile the chosen cards." (Dreams of Steel and
    Oil.) Each pick is mandatory when one exists; the graveyard pick
    uses ``choose_card_from_library`` (an id among cards in a public
    zone)."""

    card_filter: CardFilter = CardFilter()

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all

        p = _player_target(state, targets)
        if p is None:
            return
        for c in p.hand:
            reveal_to_all(state, c)
        chosen = []
        options = [c for c in p.hand if self.card_filter.matches_card(state, c)]
        if options:
            chosen.append((p.hand, _pick_from_hand(state, controller_id, options)))
        gy = [c for c in p.graveyard if self.card_filter.matches_card(state, c)]
        if gy:
            me = state.player_by_id(controller_id)
            pick = gy[0].id
            if me.agent is not None:
                pick = me.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in gy],
                )
            chosen.append(
                (p.graveyard, next((c for c in gy if c.id == pick), gy[0])),
            )
        for zone, card in chosen:
            zone.remove(card)
            card.zone = Zone.EXILE
            p.exile.append(card)
            state.log.append(f"{card.printing.name} is exiled")

    def describe(self) -> str:
        d = self.card_filter.describe()
        return (
            f"target opponent reveals their hand; you choose a {d} from it "
            f"and a {d} from their graveyard; exile the chosen cards"
        )


# ---------------------------------------------------------------------
# Named discard (Cabal Therapy) and flashback paid by sacrifice
# ---------------------------------------------------------------------


def candidate_card_names(state, player, card_filter: CardFilter) -> list[str]:
    """The card names a "choose a <nonland> card name" choice aimed at
    ``player`` is offered: the distinct names of the matching cards that
    player owns, in every zone. CR 201.3 allows any name; names outside
    their cards can never hit, so they are dominated (the narrowing is in
    ``docs/SEARCH_LIMITATIONS.md``). Sorted, so the option list doesn't
    depend on hidden-zone order."""
    names = set()
    zones = (
        list(player.hand) + list(player.library) + list(player.graveyard)
        + list(player.exile)
        + [o for o in state.battlefield if o.owner_id == player.id]
        + [it.source for it in state.stack if it.source.owner_id == player.id]
    )
    for c in zones:
        if c.printing.is_token or getattr(c, "is_spell_copy", False):
            continue
        if card_filter.matches_card(state, c):
            names.add(c.printing.name)
    return sorted(names)


@dataclass(frozen=True)
class ChooseNameTargetDiscardsAllWithName(Effect):
    """"Choose a <nonland> card name. Target player reveals their hand
    and discards all cards with that name." (Cabal Therapy; Cabal
    Therapist's reflexive trigger.) The name is chosen on resolution
    through ``choose_mode`` over :func:`candidate_card_names`."""

    card_filter: CardFilter = CardFilter()

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.game import reveal_to_all

        p = _player_target(state, targets)
        if p is None:
            return
        names = candidate_card_names(state, p, self.card_filter)
        me = state.player_by_id(controller_id)
        name = None
        if names:
            idx = 0
            if me.agent is not None and len(names) > 1:
                picked = me.agent.choose_mode(state, controller_id, names)
                if isinstance(picked, int) and 0 <= picked < len(names):
                    idx = picked
            name = names[idx]
            state.log.append(f"P{controller_id} names {name}")
        for c in p.hand:
            reveal_to_all(state, c)
        for c in [c for c in p.hand if c.printing.name == name]:
            discard_card(state, p, c)

    def describe(self) -> str:
        return (
            f"choose a {self.card_filter.describe()} name; target player "
            f"reveals their hand and discards all cards with that name"
        )


@dataclass(frozen=True)
class FlashbackSacrificeCost:
    """A ``CardPrinting.zone_abilities`` spec: "Flashback—Sacrifice <N>
    <creatures>." (Cabal Therapy, Dread Return; CR 702.34a with a
    non-mana cost.) The printing's ``flashback_cost`` is the empty mana
    cost; this spec is the rest of the flashback cost, checked by
    ``legal.py``'s flashback offer and paid by the cast pipeline."""

    count: int = 1
    card_filter: CardFilter = CardFilter()

    def options(self, state, player_id: int) -> list:
        from mtgz_ir.alt_casting import permanent_matches_filter

        return [
            o for o in state.battlefield
            if o.controller_id == player_id
            and permanent_matches_filter(state, o, self.card_filter)
        ]

    def describe(self) -> str:
        return f"flashback: sacrifice {self.count} {self.card_filter.describe()}(s)"


def _flashback_sacrifice_specs(card) -> list:
    return [
        s for s in card.printing.zone_abilities
        if isinstance(s, FlashbackSacrificeCost)
    ]


def flashback_extra_costs_payable(state, player_id: int, card) -> bool:
    """Can ``player_id`` pay the non-mana part of ``card``'s flashback
    cost right now?"""
    return all(
        len(spec.options(state, player_id)) >= spec.count
        for spec in _flashback_sacrifice_specs(card)
    )


def pay_flashback_extra_costs(state, player_id: int, card) -> None:
    """Pay the non-mana part of ``card``'s flashback cost: sacrifice the
    printed permanents, each picked with ``choose_sacrifice``."""
    from mtgz_ir.turn import sacrifice

    p = state.player_by_id(player_id)
    for spec in _flashback_sacrifice_specs(card):
        for _ in range(spec.count):
            options = spec.options(state, player_id)
            if not options:
                return
            pick = options[0].id
            if p.agent is not None:
                pick = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in options],
                )
            chosen = next((o for o in options if o.id == pick), options[0])
            state.log.append(
                f"flashback cost: P{player_id} sacrifices {chosen.printing.name}"
            )
            sacrifice(state, chosen)


# ---------------------------------------------------------------------
# Recursion & reanimation (F13)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class ReturnSelfToOwnersHandCost:
    """``ActivationCost.extra_costs`` atom: "Return this <permanent> to
    its owner's hand" (Recurring Nightmare). Payable while the source is
    on the battlefield; paying it bounces the source (CR 602.2: the
    ability still resolves)."""

    def payable(self, state, player_id: int, source) -> bool:
        return source is not None and source in state.battlefield

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.effects.generic.targets_helpers import _bounce_to_hand

        state.log.append(
            f"P{player_id} returns {source.printing.name} to its owner's hand "
            f"(cost)"
        )
        _bounce_to_hand(state, source)
        return []

    def describe(self) -> str:
        return "Return this permanent to its owner's hand"


@dataclass(frozen=True)
class MillThenReturnCardFromGraveyardToHand(Effect):
    """"Mill N cards, then return a <creature or planeswalker> card from
    your graveyard to your hand." (Takenuma, Abandoned Mire's channel.)
    The return is untargeted and chosen on resolution
    (``choose_card_from_library`` over the graveyard ids), so a card just
    milled qualifies; with no matching card nothing returns."""

    count: int = 3
    card_filter: CardFilter = CardFilter()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        milled = 0
        for _ in range(self.count):
            if not p.library:
                break
            put_into_graveyard(state, p.library.pop(0))
            milled += 1
        state.log.append(f"P{controller_id} mills {milled} card(s)")
        options = [c for c in p.graveyard if self.card_filter.matches_card(state, c)]
        if not options:
            return
        pick = options[0].id
        if p.agent is not None:
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
        card = next((c for c in options if c.id == pick), options[0])
        p.graveyard.remove(card)
        card.zone = Zone.HAND
        p.hand.append(card)
        state.log.append(f"P{controller_id} returns {card.printing.name} to hand")

    def describe(self) -> str:
        return (
            f"mill {self.count} cards, then return a "
            f"{self.card_filter.describe()} from your graveyard to your hand"
        )


def _graveyard_card_targets(state, card_filter) -> list:
    return [
        ("card_in_graveyard", c.id)
        for p in state.players for c in p.graveyard
        if card_filter.matches_card(state, c)
    ]


def _graveyard_target_card(state, targets, card_filter):
    """The targeted card if it is still a matching card in a graveyard."""
    for kind, tid in targets:
        if kind != "card_in_graveyard":
            continue
        for p in state.players:
            for c in p.graveyard:
                if c.id == tid and card_filter.matches_card(state, c):
                    return p, c
    return None, None


@dataclass(frozen=True)
class ExileTargetGraveyardCardThen(Effect):
    """"Exile target <land> card from a graveyard. <Then>." (Deathrite
    Shaman's three abilities: add one mana of any color / each opponent
    loses 2 life / you gain 2 life.) ``then`` is untargeted. If the
    target is gone when this resolves, the whole ability does nothing
    (CR 608.2b)."""

    card_filter: CardFilter = CardFilter()
    then: "Effect | None" = None

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _graveyard_card_targets(state, self.card_filter)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        owner, card = _graveyard_target_card(state, targets, self.card_filter)
        if card is None:
            state.log.append("the graveyard target is gone; nothing happens")
            return
        owner.graveyard.remove(card)
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(f"{card.printing.name} is exiled from a graveyard")
        if self.then is not None:
            self.then.resolve(state, controller_id, ())

    def describe(self) -> str:
        text = f"exile target {self.card_filter.describe()} from a graveyard"
        if self.then is not None:
            text += f"; {self.then.describe()}"
        return text


@dataclass(frozen=True)
class ExileTargetGraveyardCardThenIfType(Effect):
    """"Exile target card from a graveyard. If it was a <creature> card,
    <A>. Otherwise, <B>." (Cling to Dust.) The type is the exiled card's
    (last known in the graveyard)."""

    card_filter: CardFilter = CardFilter()
    if_filter: CardFilter = CardFilter()
    if_effect: "Effect | None" = None
    else_effect: "Effect | None" = None

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _graveyard_card_targets(state, self.card_filter)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        owner, card = _graveyard_target_card(state, targets, self.card_filter)
        if card is None:
            return
        owner.graveyard.remove(card)
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(f"{card.printing.name} is exiled from a graveyard")
        branch = (
            self.if_effect if self.if_filter.matches_card(state, card)
            else self.else_effect
        )
        branch.resolve(state, controller_id, ())

    def describe(self) -> str:
        return (
            f"exile target {self.card_filter.describe()} from a graveyard; if "
            f"it was a {self.if_filter.describe()}, {self.if_effect.describe()};"
            f" otherwise, {self.else_effect.describe()}"
        )


@dataclass(frozen=True)
class PutTargetGraveyardCardOnTopOfLibrary(Effect):
    """"Put target card from a graveyard on top of its owner's library."
    (Noxious Revival.) The card becomes known to every player."""

    card_filter: CardFilter = CardFilter()

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return _graveyard_card_targets(state, self.card_filter)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all
        from mtgz_ir.turn import note_cards_put_into_library

        owner, card = _graveyard_target_card(state, targets, self.card_filter)
        if card is None:
            return
        owner.graveyard.remove(card)
        card.zone = Zone.LIBRARY
        owner.library.insert(0, card)
        reveal_to_all(state, card)
        note_cards_put_into_library(state, owner.id)
        state.log.append(
            f"{card.printing.name} is put on top of P{owner.id}'s library"
        )

    def describe(self) -> str:
        return (
            f"put target {self.card_filter.describe()} from a graveyard on top "
            f"of its owner's library"
        )


@dataclass(frozen=True)
class ReturnTargetsFromYourGraveyardToBattlefield(Effect):
    """"Return [up to N] target <creature> card[s] from your graveyard to
    the battlefield[ with a -1/-1 counter on it][ and attach this
    Equipment to it]."

    * ``min_n`` / ``max_n`` -- the target count (Reveillark: up to two;
      Persist and Pre-War Formalwear: one).
    * ``minus_one_counter`` -- Persist: the creature gets a -1/-1 counter
      as it arrives (before state-based actions are checked).
    * ``attach_source`` -- Pre-War Formalwear: the resolving Equipment
      becomes attached to the returned creature if both are still on
      the battlefield.

    Each target still in your graveyard and matching is returned; the
    others are skipped."""

    card_filter: CardFilter = CardFilter()
    min_n: int = 1
    max_n: int = 1
    minus_one_counter: bool = False
    attach_source: bool = False

    def requires_target(self) -> bool:
        return self.min_n > 0

    def target_count_range(self) -> tuple[int, int]:
        return (self.min_n, self.max_n)

    def legal_targets(self, state, controller_id):
        p = state.player_by_id(controller_id)
        return [
            ("card_in_graveyard", c.id) for c in p.graveyard
            if self.card_filter.matches_card(state, c)
        ]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.reanimation import reanimate_card

        p = state.player_by_id(controller_id)
        seen: set[int] = set()
        returned = []
        for kind, tid in targets:
            if kind != "card_in_graveyard" or tid in seen:
                continue
            seen.add(tid)
            card = next(
                (c for c in p.graveyard
                 if c.id == tid and self.card_filter.matches_card(state, c)),
                None,
            )
            if card is not None:
                returned.append(card)
        for card in returned[: self.max_n]:
            reanimate_card(state, card, controller_id)
            if card not in state.battlefield:
                continue
            if self.minus_one_counter:
                from mtgz_ir.game import add_minus_one_counters

                add_minus_one_counters(state, card, 1)
            if self.attach_source:
                src = state.resolving_source
                if src is not None and src in state.battlefield:
                    src.attached_to = card.id
                    state.log.append(
                        f"{src.printing.name} becomes attached to "
                        f"{card.printing.name}"
                    )

    def describe(self) -> str:
        count = (
            f"up to {self.max_n} target " if self.min_n == 0
            else "target "
        )
        text = (
            f"return {count}{self.card_filter.describe()}(s) from your "
            f"graveyard to the battlefield"
        )
        if self.minus_one_counter:
            text += " with a -1/-1 counter on it"
        if self.attach_source:
            text += " and attach this Equipment to it"
        return text


@dataclass(frozen=True)
class CopyIfCastFromGraveyard(Effect):
    """"<Effect>. If this spell was cast from a graveyard, you may copy
    this spell and may choose a new target for the copy." (Sevinne's
    Reclamation, cast with flashback.) The copy is put on the stack above
    the original with a new target chosen through ``choose_target`` among
    the inner effect's legal targets (it keeps the original's when the
    agent answers outside them)."""

    inner: "Effect | None" = None

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def target_count_range(self) -> tuple[int, int]:
        return self.inner.target_count_range()

    def legal_targets(self, state, controller_id):
        return self.inner.legal_targets(state, controller_id)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import push_spell_copy

        spell = state.resolving_spell
        self.inner.resolve(state, controller_id, targets)
        if spell is None or spell.is_spell_copy or not spell.cast_via_flashback:
            return
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, f"copy {spell.printing.name}?",
        ):
            return
        new_targets = tuple(targets)
        legal = self.inner.legal_targets(state, controller_id)
        if legal and p.agent is not None:
            pick = p.agent.choose_target(state, controller_id, legal)
            if pick in legal:
                new_targets = (pick,)
        push_spell_copy(state, spell.printing, controller_id, targets=new_targets)
        state.log.append(f"P{controller_id} copies {spell.printing.name}")

    def describe(self) -> str:
        return (
            f"{self.inner.describe()}; if this spell was cast from a graveyard, "
            f"you may copy it and choose a new target for the copy"
        )


# ---------------------------------------------------------------------
# Draw replacements: dredge and Chains of Mephistopheles (F15)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class Dredge:
    """A ``CardPrinting.zone_abilities`` spec: "Dredge N" (CR 702.52): "If
    you would draw a card, you may mill N cards instead. If you do,
    return this card from your graveyard to your hand." Functions only
    while the card is in its owner's graveyard, and only with at least N
    cards in their library."""

    n: int = 1

    def describe(self) -> str:
        return f"dredge {self.n}"


@dataclass(frozen=True)
class DiscardInsteadOfDraw:
    """A ``CardPrinting.zone_abilities`` spec (Chains of Mephistopheles):
    "If <a player> would draw a card[ except the first one they draw in
    each of their draw steps], that player discards a card instead. If
    the player discards a card this way, they draw a card. If the player
    doesn't discard a card this way, they mill a card."

    ``affects`` is ``"any"`` / ``"opponents"`` / ``"you"`` relative to
    the source's controller. The replacement draw is a new draw that
    this same source doesn't replace again (CR 614.5) but every other
    replacement may (a second Chains, dredge)."""

    affects: AffectedPlayers = AffectedPlayers.ANY
    except_first_in_draw_step: bool = True

    def __post_init__(self) -> None:
        object.__setattr__(self, "affects", AffectedPlayers(self.affects))

    def applies_to(self, source, player_id: int) -> bool:
        if self.affects == "opponents":
            return player_id != source.controller_id
        if self.affects == "you":
            return player_id == source.controller_id
        return True

    def describe(self) -> str:
        who = {"any": "a player", "opponents": "an opponent", "you": "you"}[
            self.affects
        ]
        return f"if {who} would draw a card, they discard a card instead"


#: Per game state, per player: the ``DiscardInsteadOfDraw`` sources that
#: already replaced the draw being performed (CR 614.5). Set only for the
#: duration of a nested replacement draw.
_APPLIED_DRAW_REPLACEMENTS: dict = {}

#: The non-dredge option when no mandatory replacement applies.
DRAW_NORMALLY = "draw the card"
#: The option for applying the mandatory discard-instead replacement.
APPLY_CHAINS = "discard instead (Chains)"


def _discard_instead_sources(state, player, applied, first_in_draw_step):
    from mtgz_ir.game import _source_layer6_wiped

    out = []
    for o in state.battlefield:
        if o.id in applied:
            continue
        for spec in o.printing.zone_abilities:
            if not isinstance(spec, DiscardInsteadOfDraw):
                continue
            if spec.except_first_in_draw_step and first_in_draw_step:
                continue
            if not spec.applies_to(o, player.id):
                continue
            if state.continuous_effects and _source_layer6_wiped(state, o):
                continue
            out.append(o)
            break
    return out


def _dredgers(player):
    out = []
    for c in player.graveyard:
        for spec in c.printing.zone_abilities:
            if isinstance(spec, Dredge) and len(player.library) >= spec.n:
                out.append((c, spec))
                break
    return out


def replace_draw(state, player) -> bool:
    """``turn.draw_card`` hook (see :func:`_replace_draw`); a draw that
    isn't replaced takes the top card out of the library, which the
    graveyard zone watch must not later mistake for a mill."""
    if _replace_draw(state, player):
        return True
    if player.library:
        note_left_library(state, player, player.library[0])
    return False


def _replace_draw(state, player) -> bool:
    """``turn.draw_card`` hook: offer the P2 replacements for the draw
    ``player`` is about to make; returns ``True`` when the draw was
    replaced (the caller then performs no draw).

    The affected player orders the replacements (CR 616.1) through one
    ``choose_mode``: dredge one of their dredge cards, or (when a
    discard-instead source applies, which is mandatory) apply it, else
    draw normally. Dredge replaces the whole draw, so nothing else
    applies afterwards."""
    if not any(
        isinstance(s, DiscardInsteadOfDraw)
        for o in state.battlefield for s in o.printing.zone_abilities
    ) and not any(c.printing.zone_abilities for c in player.graveyard):
        return False
    applied = _APPLIED_DRAW_REPLACEMENTS.get((id(state), player.id), frozenset())
    first = player.draw_step_first_draw_pending
    chains = _discard_instead_sources(state, player, applied, first)
    dredgers = _dredgers(player)
    if not chains and not dredgers:
        return False
    options = [f"dredge {c.printing.name} ({s.n})" for c, s in dredgers]
    options.append(APPLY_CHAINS if chains else DRAW_NORMALLY)
    idx = len(options) - 1
    if player.agent is not None and len(options) > 1:
        picked = player.agent.choose_mode(state, player.id, options)
        if isinstance(picked, int) and 0 <= picked < len(options):
            idx = picked
    if idx < len(dredgers):
        card, spec = dredgers[idx]
        player.draw_step_first_draw_pending = False
        _dredge(state, player, card, spec.n)
        return True
    if not chains:
        return False
    player.draw_step_first_draw_pending = False
    _discard_instead(state, player, chains[0], applied)
    return True


def _dredge(state, player, card, n: int) -> None:
    from mtgz_ir.game import Zone

    state.log.append(f"P{player.id} dredges {card.printing.name} ({n})")
    for _ in range(n):
        if not player.library:
            break
        put_into_graveyard(state, player.library.pop(0))
    if card in player.graveyard:
        player.graveyard.remove(card)
        card.zone = Zone.HAND
        player.hand.append(card)


def _discard_instead(state, player, source, applied) -> None:
    from mtgz_ir.turn import draw_card

    state.log.append(
        f"{source.printing.name}: P{player.id} discards instead of drawing"
    )
    if _discard_chosen_unprotected(state, player):
        key = (id(state), player.id)
        _APPLIED_DRAW_REPLACEMENTS[key] = applied | {source.id}
        try:
            draw_card(state, player.id)
        finally:
            if applied:
                _APPLIED_DRAW_REPLACEMENTS[key] = applied
            else:
                _APPLIED_DRAW_REPLACEMENTS.pop(key, None)
        return
    if player.library:
        state.log.append(f"P{player.id} mills a card instead")
        put_into_graveyard(state, player.library.pop(0))


def _discard_chosen_unprotected(state, player) -> bool:
    """``player`` discards a card of their choice (a replacement's own
    discard: no "opponent's spell or ability" protection applies)."""
    from mtgz_ir.game import Zone
    from mtgz_ir.turn import fire_opponent_discards_triggers

    if not player.hand:
        return False
    ids = [c.id for c in player.hand]
    pick = ids[0]
    if player.agent is not None:
        pick = player.agent.choose_discard(state, player.id, ids)
    card = next((c for c in player.hand if c.id == pick), player.hand[0])
    player.hand.remove(card)
    card.zone = Zone.GRAVEYARD
    if graveyard_exile_instead_applies(state, card):
        exile_instead_of_graveyard(state, card)
    else:
        player.graveyard.append(card)
    state.log.append(f"P{player.id} discards {card.printing.name}")
    fire_opponent_discards_triggers(state, player.id, card=card)
    return True


@dataclass(frozen=True)
class DestroyEventCreature(Effect):
    """"Destroy that creature." where "that creature" is the creature the
    trigger's event names, delivered as its fixed target (Stinkweed Imp's
    "Whenever this creature deals combat damage to a creature"). No-op if
    it already left the battlefield; indestructible and regeneration
    apply (CR 701.8)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.targeted_removal import destroy_permanent

        obj = next(
            (o for t in targets for o in state.battlefield if o.id == t[1]),
            None,
        )
        if obj is None:
            return
        destroy_permanent(state, obj)

    def describe(self) -> str:
        return "destroy that creature"


# ---------------------------------------------------------------------
# Look at / reveal-until / library order (F11)
# ---------------------------------------------------------------------


def _defending_player(state, controller_id):
    """"Defending player" of an attack trigger: the attacking controller's
    opponent (two-player engine; the base's attack-trigger convention)."""
    return next(
        (p for p in state.players if p.id != controller_id and not p.has_lost),
        None,
    )


def _exile_top(state, player) -> "GameObject | None":
    from mtgz_ir.game import Zone

    if not player.library:
        return None
    card = player.library.pop(0)
    card.zone = Zone.EXILE
    player.exile.append(card)
    return card


@dataclass(frozen=True)
class DefendingPlayerRevealsTopPutsIntoHandIf(Effect):
    """"Defending player reveals the top card of their library. If it's a
    <land> card, that player puts it into their hand." (Goblin Guide.)"""

    card_filter: CardFilter = CardFilter()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all

        p = _defending_player(state, controller_id)
        if p is None or not p.library:
            return
        top = p.library[0]
        reveal_to_all(state, top)
        state.log.append(f"P{p.id} reveals {top.printing.name}")
        if self.card_filter.matches_card(state, top):
            p.library.pop(0)
            top.zone = Zone.HAND
            p.hand.append(top)
            state.log.append(f"P{p.id} puts {top.printing.name} into their hand")

    def describe(self) -> str:
        return (
            f"defending player reveals the top card of their library; if it's "
            f"a {self.card_filter.describe()}, they put it into their hand"
        )


@dataclass(frozen=True)
class DefendingPlayerExilesTopN(Effect):
    """"Defending player exiles the top N cards of their library."
    (Ulamog, the Ceaseless Hunger.)"""

    count: int = 1

    def resolve(self, state, controller_id, targets=()):
        p = _defending_player(state, controller_id)
        if p is None:
            return
        n = 0
        for _ in range(self.count):
            if _exile_top(state, p) is None:
                break
            n += 1
        state.log.append(f"P{p.id} exiles the top {n} card(s) of their library")

    def describe(self) -> str:
        return f"defending player exiles the top {self.count} cards of their library"


@dataclass(frozen=True)
class EachOpponentExilesUntilTotalManaValue(Effect):
    """"Each opponent exiles cards from the top of their library until
    that player has exiled cards with total mana value N or greater."
    (Tasha's Hideous Laughter.) Stops early on an empty library."""

    total: int = 20

    def resolve(self, state, controller_id, targets=()):
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            mv = n = 0
            while mv < self.total:
                card = _exile_top(state, p)
                if card is None:
                    break
                mv += card.printing.mana_cost.total
                n += 1
            state.log.append(
                f"P{p.id} exiles {n} card(s) (total mana value {mv})"
            )

    def describe(self) -> str:
        return (
            f"each opponent exiles cards from the top of their library until "
            f"they've exiled cards with total mana value {self.total} or greater"
        )


@dataclass(frozen=True)
class RevealUntilPutIntoHandRestIntoGraveyard(Effect):
    """"Reveal cards from the top of your library until you reveal a
    <basic land> card. Put that card into your hand and all other cards
    revealed this way into your graveyard." (Hermit Druid.) With no such
    card, every card is revealed and put into the graveyard."""

    card_filter: CardFilter = CardFilter()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        revealed = []
        found = None
        while p.library:
            card = p.library.pop(0)
            reveal_to_all(state, card)
            if self.card_filter.matches_card(state, card):
                found = card
                break
            revealed.append(card)
        if found is not None:
            found.zone = Zone.HAND
            p.hand.append(found)
        for card in revealed:
            put_into_graveyard(state, card)
        state.log.append(
            f"P{controller_id} reveals {len(revealed) + (found is not None)} "
            f"card(s)" + (f", keeps {found.printing.name}" if found else "")
        )

    def describe(self) -> str:
        return (
            f"reveal cards from the top of your library until you reveal a "
            f"{self.card_filter.describe()}; put it into your hand and the "
            f"rest into your graveyard"
        )


@dataclass(frozen=True)
class LookAtTopPutUpToOntoBattlefield(Effect):
    """"Look at the top N cards of your library. Put up to K <creature
    cards with mana value 3 or less> from among them onto the
    battlefield. Put the rest on the bottom of your library in any
    order." (Collected Company.) Picks go through
    ``choose_card_from_library`` (``None`` stops); the rest keep their
    library order on the bottom (the base's "any order" shortcut)."""

    count: int = 6
    take: int = 2
    card_filter: CardFilter = CardFilter()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        looked = [p.library.pop(0) for _ in range(min(self.count, len(p.library)))]
        for c in looked:
            reveal_to(c, controller_id)
        chosen = []
        for _ in range(self.take):
            options = [
                c for c in looked
                if c not in chosen and self.card_filter.matches_card(state, c)
            ]
            pick = search_pick(state, p, options, may_fail=True)
            if pick is None:
                break
            chosen.append(pick)
        for c in looked:
            if c not in chosen:
                c.zone = Zone.LIBRARY
                p.library.append(c)
        for c in chosen:
            put_into_battlefield(state, c, controller_id)
        state.log.append(
            f"P{controller_id} puts {len(chosen)} card(s) onto the battlefield"
        )

    def describe(self) -> str:
        return (
            f"look at the top {self.count} cards of your library; put up to "
            f"{self.take} {self.card_filter.describe()}(s) from among them "
            f"onto the battlefield; the rest on the bottom"
        )


@dataclass(frozen=True)
class LookAtTopPutIntoHandMoreIfInstantAndSorcery(Effect):
    """"Look at the top N cards of your library. Put K of them into your
    hand and the rest on the bottom of your library in any order. If
    there is an instant card and a sorcery card in your graveyard,
    instead put K' of them into your hand ..." (Flow State.) The
    graveyard condition is read on resolution."""

    count: int = 3
    take: int = 1
    take_if: int = 2

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        gy_types = {t for c in p.graveyard for t in c.printing.card_types}
        k = self.take_if if {"Instant", "Sorcery"} <= gy_types else self.take
        looked = [p.library.pop(0) for _ in range(min(self.count, len(p.library)))]
        for c in looked:
            reveal_to(c, controller_id)
        taken = []
        for _ in range(min(k, len(looked))):
            options = [c for c in looked if c not in taken]
            taken.append(search_pick(state, p, options, may_fail=False))
        for c in looked:
            if c in taken:
                c.zone = Zone.HAND
                p.hand.append(c)
            else:
                c.zone = Zone.LIBRARY
                p.library.append(c)
        state.log.append(f"P{controller_id} puts {len(taken)} card(s) into hand")

    def describe(self) -> str:
        return (
            f"look at the top {self.count} cards; put {self.take} into your "
            f"hand ({self.take_if} if there is an instant and a sorcery card in "
            f"your graveyard), the rest on the bottom"
        )


@dataclass(frozen=True)
class ExileTopMayTakeUnlessSameName(Effect):
    """"Exile the top card of your library. You may put that card into
    your hand unless it has the same name as another card exiled this
    way. Repeat this process until you put a card into your hand or you
    exile two cards with the same name, whichever comes first." (Tainted
    Pact.) Each "may" is a ``choose_yes_no``; an empty library ends the
    process."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        names: set[str] = set()
        while True:
            card = _exile_top(state, p)
            if card is None:
                return
            name = card.printing.name
            if name in names:
                state.log.append(f"P{controller_id} exiles a second {name}")
                return
            names.add(name)
            if p.agent is None or p.agent.choose_yes_no(
                state, controller_id, f"put {name} into your hand?",
            ):
                p.exile.remove(card)
                card.exiled_face_down = False  # left exile (CR 400.7)
                card.zone = Zone.HAND
                p.hand.append(card)
                state.log.append(f"P{controller_id} puts {name} into hand")
                return

    def describe(self) -> str:
        return (
            "exile the top card of your library; you may put it into your "
            "hand unless its name repeats; repeat until you do or a name "
            "repeats"
        )


# ---------------------------------------------------------------------
# More tutors (F10): bargain, sacrifice-scaled, multi-zone, opponent-split
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class Bargain:
    """A ``CardPrinting.zone_abilities`` spec: "Bargain" (CR 702.166): "As
    an additional cost to cast this spell, you may sacrifice an
    artifact, enchantment, or token." Offered while casting (one
    ``choose_yes_no`` when such a permanent exists, then
    ``choose_sacrifice``); recorded on the spell as
    ``GameObject.cast_bargained``."""

    def options(self, state, player_id: int) -> list:
        from mtgz_ir.game import is_artifact_now

        return [
            o for o in state.battlefield
            if o.controller_id == player_id and (
                o.printing.is_token or is_artifact_now(state, o)
                or "Enchantment" in o.printing.card_types
            )
        ]

    def describe(self) -> str:
        return "bargain"


def pay_zone_cast_costs(state, player_id: int, card, *, use_flashback: bool) -> None:
    """Cast-pipeline hook (``actions._cast_spell``, with the other
    additional costs): the non-mana part of a flashback cost and the
    optional bargain cost."""
    if use_flashback:
        pay_flashback_extra_costs(state, player_id, card)
    card.cast_bargained = False
    for spec in card.printing.zone_abilities:
        if not isinstance(spec, Bargain):
            continue
        options = spec.options(state, player_id)
        p = state.player_by_id(player_id)
        if not options or (p.agent is not None and not p.agent.choose_yes_no(
            state, player_id, f"bargain {card.printing.name}?",
        )):
            return
        _sacrifice_chosen(state, p, options)
        card.cast_bargained = True
        state.log.append(f"P{player_id} bargains {card.printing.name}")


@dataclass(frozen=True)
class SearchExileFaceDownCastFreeIfBargained(Effect):
    """"Search your library for a card, exile it face down, then shuffle.
    If this spell was bargained, you may cast the exiled card without
    paying its mana cost if that spell's mana value is N or less. Put
    the exiled card into your hand if it wasn't cast this way." (Beseech
    the Mirror.) Face down = known only to its owner. The free cast goes
    through the base's ``cast_now`` (the spell lands above this one on
    the stack)."""

    max_mv: int = 4

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.game import Zone, reveal_to, shuffle_library

        p = state.player_by_id(controller_id)
        found = search_pick(state, p, list(p.library), may_fail=False)
        if found is not None:
            p.library.remove(found)
            if searched_card_taken(state, controller_id, found):
                found = None
        if found is not None:
            found.zone = Zone.EXILE
            found.exiled_face_down = True
            p.exile.append(found)
            reveal_to(found, controller_id)
            state.log.append(f"P{controller_id} exiles a card face down")
        shuffle_library(state, controller_id)
        if found is None:
            return
        spell = state.resolving_spell
        if (
            spell is not None and getattr(spell, "cast_bargained", False)
            and not found.printing.is_land
            and found.printing.mana_cost.total <= self.max_mv
            and can_cast_now(state, controller_id, found, free=True)
            and (p.agent is None or p.agent.choose_yes_no(
                state, controller_id, f"cast {found.printing.name} free?",
            ))
            and cast_now(state, controller_id, found, free=True)
        ):
            return
        if found in p.exile:
            p.exile.remove(found)
            found.exiled_face_down = False  # left exile (CR 400.7)
            found.zone = Zone.HAND
            p.hand.append(found)

    def describe(self) -> str:
        return (
            f"search your library for a card, exile it face down, then "
            f"shuffle; if bargained, you may cast it free if its mana value "
            f"is {self.max_mv} or less; otherwise put it into your hand"
        )


@dataclass(frozen=True)
class SearchManaValueOnePlusSacrificed(Effect):
    """"Search your library for a <creature> card with mana value equal to
    1 plus the sacrificed <creature>'s mana value, put that card onto the
    battlefield, then shuffle." (Birthing Pod, Oswald Fiddlebender.) The
    sacrificed permanent is the ability's cost-paid object (last-known
    mana value; an {X} in its cost counts as 0)."""

    card_filter: CardFilter = CardFilter()
    plus: int = 1

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.formulas import MVOfCostPaidObjectPlus
        from mtgz_ir.game import shuffle_library

        mv = MVOfCostPaidObjectPlus(constant=self.plus).value(state, controller_id)
        p = state.player_by_id(controller_id)
        options = [
            c for c in p.library
            if self.card_filter.matches_card(state, c)
            and c.printing.mana_cost.total == mv
        ]
        found = search_pick(state, p, options, may_fail=True)
        if found is not None:
            p.library.remove(found)
            if searched_card_taken(state, controller_id, found):
                found = None
        if found is not None:
            state.log.append(
                f"P{controller_id} puts {found.printing.name} onto the battlefield"
            )
            put_into_battlefield(state, found, controller_id)
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            f"search your library for a {self.card_filter.describe()} with "
            f"mana value {self.plus} plus the sacrificed permanent's, put it "
            f"onto the battlefield, then shuffle"
        )


@dataclass(frozen=True)
class DiscardCardCost:
    """``ActivationCost.extra_costs`` atom: "Discard a <creature> card"
    (Survival of the Fittest). The card is picked with ``choose_discard``
    among the matching cards in hand."""

    card_filter: CardFilter = CardFilter()

    def _options(self, state, player_id: int) -> list:
        return [
            c for c in state.player_by_id(player_id).hand
            if self.card_filter.matches_card(state, c)
        ]

    def payable(self, state, player_id: int, source) -> bool:
        return bool(self._options(state, player_id))

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.effects.generic.card_flow import discard_card

        p = state.player_by_id(player_id)
        options = self._options(state, player_id)
        pick = options[0].id
        if p.agent is not None:
            pick = p.agent.choose_discard(state, player_id, [c.id for c in options])
        card = next((c for c in options if c.id == pick), options[0])
        discard_card(state, p, card)
        return [card]

    def describe(self) -> str:
        return f"Discard a {self.card_filter.describe()}"


@dataclass(frozen=True)
class SacrificeLandsSearchThatMany(Effect):
    """"Sacrifice any number of lands. Search your library for up to that
    many land cards, put them onto the battlefield tapped, then shuffle."
    (Scapeshift.) How many is ``choose_x`` (0 to the lands you control),
    which ones ``choose_sacrifice``; the search picks one land at a time
    (``None`` stops)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)

        def lands():
            return [
                o for o in state.battlefield
                if o.controller_id == controller_id and o.printing.is_land
            ]

        n = len(lands())
        if p.agent is not None and n:
            n = max(0, min(n, p.agent.choose_x(
                state, controller_id, "lands to sacrifice", n,
            )))
        sacrificed = 0
        for _ in range(n):
            options = lands()
            if not options or not _sacrifice_chosen(state, p, options):
                break
            sacrificed += 1
        found = []

        def search():
            for _ in range(sacrificed):
                options = [c for c in p.library if c.printing.is_land]
                pick = search_pick(state, p, options, may_fail=True)
                if pick is None:
                    break
                p.library.remove(pick)
                if not searched_card_taken(state, controller_id, pick):
                    found.append(pick)
            for card in found:
                put_into_battlefield(state, card, controller_id)
                if card in state.battlefield:
                    card.tapped = True
            shuffle_library(state, controller_id)

        run_library_search(state, controller_id, search)
        state.log.append(
            f"P{controller_id} sacrifices {sacrificed} land(s), fetches "
            f"{len(found)}"
        )

    def describe(self) -> str:
        return (
            "sacrifice any number of lands; search your library for up to "
            "that many land cards, put them onto the battlefield tapped, then "
            "shuffle"
        )


@dataclass(frozen=True)
class SearchLibraryAndOrGraveyardXThenPump(Effect):
    """"Search your library and/or graveyard for a <creature> card with
    mana value X or less and put it onto the battlefield. If you search
    your library this way, shuffle. If X is N or more, creatures you
    control get +X/+X and gain haste until end of turn." (Finale of
    Devastation.) Whether to search the library is a ``choose_yes_no``;
    the card is picked among the graveyard (and library) matches."""

    card_filter: CardFilter = CardFilter()
    pump_threshold: int = 10

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.pump import PumpCreaturesYouControlUntilEOT
        from mtgz_ir.game import shuffle_library

        x = state.resolving_x_value
        p = state.player_by_id(controller_id)
        search_library = p.agent is None or p.agent.choose_yes_no(
            state, controller_id, "search your library too?",
        )

        def search():
            pool = list(p.graveyard) + (list(p.library) if search_library else [])
            options = [
                c for c in pool
                if self.card_filter.matches_card(state, c)
                and c.printing.mana_cost.total <= x
            ]
            found = search_pick(state, p, options, may_fail=True)
            if found is not None:
                (p.library if found in p.library else p.graveyard).remove(found)
                if search_library and searched_card_taken(
                    state, controller_id, found,
                ):
                    found = None
            if found is not None:
                put_into_battlefield(state, found, controller_id)
            if search_library:
                shuffle_library(state, controller_id)

        if search_library:
            run_library_search(state, controller_id, search)
        else:
            search()
        if x >= self.pump_threshold:
            PumpCreaturesYouControlUntilEOT(
                power_bonus=x, toughness_bonus=x,
                granted_keywords=frozenset({"haste"}),
            ).resolve(state, controller_id)

    def describe(self) -> str:
        return (
            f"search your library and/or graveyard for a "
            f"{self.card_filter.describe()} with mana value X or less and put "
            f"it onto the battlefield; if X is {self.pump_threshold} or more, "
            f"creatures you control get +X/+X and gain haste until end of turn"
        )


@dataclass(frozen=True)
class SearchDifferentNamesOpponentSplits(Effect):
    """"Search your library for up to N cards with different names and
    reveal them. Target opponent chooses K of those cards. Put the
    chosen cards into your graveyard and the rest into your hand. Then
    shuffle." (Gifts Ungiven.) Your picks: one at a time
    (``choose_card_from_library``, ``None`` stops); the opponent's: K
    picks with their own ``choose_card_from_library`` (all of them when
    K or fewer were found)."""

    count: int = 4
    opponent_picks: int = 2

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to_all, shuffle_library

        opp = _player_target(state, targets)
        p = state.player_by_id(controller_id)
        found = []
        for _ in range(self.count):
            names = {c.printing.name for c in found}
            options = [c for c in p.library if c.printing.name not in names]
            pick = search_pick(state, p, options, may_fail=True)
            if pick is None:
                break
            p.library.remove(pick)
            reveal_to_all(state, pick)
            if not searched_card_taken(state, controller_id, pick):
                found.append(pick)
        to_graveyard = []
        if len(found) <= self.opponent_picks or opp is None:
            to_graveyard = list(found) if opp is not None else []
        else:
            for _ in range(self.opponent_picks):
                options = [c for c in found if c not in to_graveyard]
                pick = options[0].id
                if opp.agent is not None:
                    pick = opp.agent.choose_card_from_library(
                        state, opp.id, [c.id for c in options],
                    )
                to_graveyard.append(
                    next((c for c in options if c.id == pick), options[0]),
                )
        for card in found:
            if card in to_graveyard:
                put_into_graveyard(state, card)
            else:
                card.zone = Zone.HAND
                p.hand.append(card)
        shuffle_library(state, controller_id)
        state.log.append(
            f"P{controller_id} finds {len(found)}; {len(to_graveyard)} to the "
            f"graveyard"
        )

    def describe(self) -> str:
        return (
            f"search your library for up to {self.count} cards with different "
            f"names; target opponent chooses {self.opponent_picks} of them to "
            f"go to your graveyard, the rest to your hand"
        )


@dataclass(frozen=True)
class DoomsdayPile(Effect):
    """"Search your library and graveyard for N cards and exile the rest.
    Put the chosen cards on top of your library in any order. You lose
    half your life, rounded up." (Doomsday.) The N picks are mandatory
    when available (all of them with fewer, per its ruling) and go
    through ``choose_card_from_library``; the pick order is the top-down
    order of the new library."""

    count: int = 5

    @library_search
    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        pool = list(p.library) + list(p.graveyard)
        chosen = []
        for _ in range(min(self.count, len(pool))):
            options = [c for c in pool if c not in chosen]
            chosen.append(search_pick(state, p, options, may_fail=False))
        for c in pool:
            (p.library if c in p.library else p.graveyard).remove(c)
            if c not in chosen:
                c.zone = Zone.EXILE
                p.exile.append(c)
        chosen = [
            c for c in chosen if not searched_card_taken(state, controller_id, c)
        ]
        for c in reversed(chosen):
            c.zone = Zone.LIBRARY
            p.library.insert(0, c)
            reveal_to(c, controller_id)
        loss = -(-max(0, p.life) // 2)
        p.life -= loss
        state.log.append(
            f"P{controller_id} builds a {len(chosen)}-card library and loses "
            f"{loss} life"
        )

    def describe(self) -> str:
        return (
            f"search your library and graveyard for {self.count} cards and "
            f"exile the rest; put them on top in any order; you lose half "
            f"your life, rounded up"
        )


# ---------------------------------------------------------------------
# Put permanents from hand (F17)
# ---------------------------------------------------------------------


def _turn_order_from(state, first_id: int) -> list:
    n = len(state.players)
    start = next(i for i, p in enumerate(state.players) if p.id == first_id)
    return [state.players[(start + i) % n] for i in range(n)]


def _hand_pick(state, p, options) -> "GameObject | None":
    if not options:
        return None
    if p.agent is None:
        return options[0]
    pick = p.agent.choose_card_from_hand(state, p.id, [c.id for c in options])
    return next((c for c in options if c.id == pick), None)


@dataclass(frozen=True)
class EachPlayerMayPutFromHand(Effect):
    """"Each player may put an <artifact, creature, enchantment, or land>
    card from their hand onto the battlefield." (Show and Tell.) Players
    choose in turn order from the active player
    (``choose_card_from_hand``, ``None`` declines); then the chosen cards
    are put onto the battlefield."""

    card_filter: CardFilter = CardFilter()

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield

        active = state.players[state.active_player_idx].id
        chosen = []
        for p in _turn_order_from(state, active):
            if p.has_lost:
                continue
            options = [c for c in p.hand if self.card_filter.matches_card(state, c)]
            card = _hand_pick(state, p, options)
            if card is not None:
                chosen.append((p, card))
        for p, card in chosen:
            p.hand.remove(card)
            state.log.append(f"P{p.id} puts {card.printing.name} onto the battlefield")
            put_into_battlefield(state, card, p.id)

    def describe(self) -> str:
        return (
            f"each player may put a {self.card_filter.describe()} from their "
            f"hand onto the battlefield"
        )


@dataclass(frozen=True)
class EurekaLoop(Effect):
    """"Starting with you, each player may put a permanent card from their
    hand onto the battlefield. Repeat this process until no one puts a
    card onto the battlefield." (Eureka.) Rounds go in turn order from
    the controller; the process ends after a full round in which nobody
    puts a card (a player who declined may put one in a later round)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield

        while True:
            put_any = False
            for p in _turn_order_from(state, controller_id):
                if p.has_lost:
                    continue
                options = [c for c in p.hand if c.printing.is_permanent]
                card = _hand_pick(state, p, options)
                if card is None:
                    continue
                p.hand.remove(card)
                state.log.append(
                    f"P{p.id} puts {card.printing.name} onto the battlefield"
                )
                put_into_battlefield(state, card, p.id)
                put_any = True
            if not put_any:
                return

    def describe(self) -> str:
        return (
            "starting with you, each player may put a permanent card from "
            "their hand onto the battlefield; repeat until no one does"
        )


@dataclass(frozen=True)
class EachPlayerChoosesLowestCreatureEnters(Effect):
    """"Each player chooses a card in their hand. Then each player
    reveals their chosen card. The owner of each creature card revealed
    this way with the lowest mana value puts it onto the battlefield."
    (Stronghold Gambit.) Choices go through ``choose_card_from_hand`` in
    turn order (a player with cards must choose one); every creature
    card tied for the lowest mana value enters."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import reveal_to_all

        active = state.players[state.active_player_idx].id
        chosen = []
        for p in _turn_order_from(state, active):
            if p.has_lost or not p.hand:
                continue
            card = _hand_pick(state, p, list(p.hand)) or p.hand[0]
            chosen.append((p, card))
        for _, card in chosen:
            reveal_to_all(state, card)
        creatures = [(p, c) for p, c in chosen if c.printing.is_creature]
        if not creatures:
            return
        low = min(c.printing.mana_cost.total for _, c in creatures)
        for p, card in creatures:
            if card.printing.mana_cost.total == low and card in p.hand:
                p.hand.remove(card)
                put_into_battlefield(state, card, p.id)

    def describe(self) -> str:
        return (
            "each player chooses a card in their hand and reveals it; each "
            "revealed creature card with the lowest mana value enters"
        )


# ---------------------------------------------------------------------
# Library-search payoffs (F18)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class PutXCountersOnSelfThenDrawHalfX(Effect):
    """"Put X +1/+1 counters on him. Then draw half X cards, rounded
    down." (Wan Shi Tong, Librarian's ETB; X is the X its spell was cast
    with -- 0 when it entered another way.)"""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import add_counters
        from mtgz_ir.turn import draw_n_cards

        src = state.resolving_source
        x = getattr(src, "entered_x_value", 0) if src is not None else 0
        if src is not None and src in state.battlefield and x > 0:
            add_counters(state, src, "+1/+1", x)
        if x // 2:
            draw_n_cards(state, controller_id, x // 2)

    def describe(self) -> str:
        return "put X +1/+1 counters on this creature; draw half X cards, rounded down"



@dataclass(frozen=True)
class PerCardDrawnThisTurn(_Condition):
    """Variable-amount count for "This spell costs {1} less to cast for
    each card you've drawn this turn." (Deem Inferior):
    ``Player.cards_drawn_this_turn``."""

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return self.multiplier(state, viewer_id) > 0

    def multiplier(self, state, viewer_id, chosen_targets=None) -> int:
        try:
            return state.player_by_id(viewer_id).cards_drawn_this_turn
        except (KeyError, AttributeError, StopIteration):
            return 0


# ---------------------------------------------------------------------
# Graveyard-functioning triggers (Bridge from Below), self-to-graveyard
# piles (Triumph of Saint Katherine), Scroll Rack, Winota
# ---------------------------------------------------------------------


class CreaturePutIntoGraveyardOf(_TriggerPredicate):
    """``Event.DIES`` predicate: "a [nontoken] creature is put into
    <your / an opponent's> graveyard from the battlefield", where "your"
    is the source card's owner (Bridge from Below works from its owner's
    graveyard; who controlled the creature doesn't matter -- only which
    graveyard it went to, i.e. its owner)."""

    def __init__(self, whose: str = "yours", nontoken: bool = False) -> None:
        self.whose = GraveyardOwner(whose)
        self.nontoken = nontoken

    def matches(self, state, subjects, source) -> bool:
        obj = subjects.get("obj")
        if obj is None or source is None or not obj.printing.is_creature:
            return False
        if self.nontoken and obj.printing.is_token:
            return False
        same = obj.owner_id == source.owner_id
        return same if self.whose == "yours" else not same

    def __eq__(self, other):
        return (
            isinstance(other, CreaturePutIntoGraveyardOf)
            and (other.whose, other.nontoken) == (self.whose, self.nontoken)
        )

    def __hash__(self):
        return hash(("CreaturePutIntoGraveyardOf", self.whose, self.nontoken))

    def __repr__(self):
        return f"CreaturePutIntoGraveyardOf({plain(self.whose)!r}, {self.nontoken!r})"


class SelfPutIntoYourGraveyard(_TriggerPredicate):
    """``Event.DIES`` predicate: "When this creature is put into your
    graveyard from the battlefield" -- the source itself died while its
    owner controlled it (so it went to its controller's graveyard)."""

    def matches(self, state, subjects, source) -> bool:
        obj = subjects.get("obj")
        return (
            obj is not None and source is not None and obj.id == source.id
            and obj.controller_id == obj.owner_id
        )

    def __eq__(self, other):
        return isinstance(other, SelfPutIntoYourGraveyard)

    def __hash__(self):
        return hash("SelfPutIntoYourGraveyard")

    def __repr__(self):
        return "SelfPutIntoYourGraveyard()"


@dataclass(frozen=True)
class IfSourceInGraveyard(Effect):
    """The intervening "if this card is in your graveyard" rechecked as
    the ability resolves (CR 603.4)."""

    inner: "Effect | None" = None

    def resolve(self, state, controller_id, targets=()):
        src = state.resolving_source
        if src is None or src not in state.player_by_id(src.owner_id).graveyard:
            return
        self.inner.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"if this card is in your graveyard, {self.inner.describe()}"


@dataclass(frozen=True)
class ExileSelfAndTopNShuffleOnTop(Effect):
    """"Exile it and the top N cards of your library in a face-down pile.
    If you do, shuffle that pile and put it back on top of your
    library." (Triumph of Saint Katherine.) Nothing happens unless the
    card is still in its owner's graveyard and the library has N cards
    (its ruling)."""

    count: int = 6

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        card = state.resolving_source
        if card is None:
            return
        p = state.player_by_id(card.owner_id)
        if card not in p.graveyard or len(p.library) < self.count:
            return
        p.graveyard.remove(card)
        pile = [card] + [p.library.pop(0) for _ in range(self.count)]
        state.rng.shuffle(pile)
        for c in reversed(pile):
            c.zone = Zone.LIBRARY
            p.library.insert(0, c)
        reveal_to(card, card.owner_id)
        state.log.append(
            f"{card.printing.name} and the top {self.count} cards are "
            f"shuffled back on top of P{p.id}'s library"
        )

    def describe(self) -> str:
        return (
            f"exile it and the top {self.count} cards of your library; "
            f"shuffle them and put them back on top"
        )


@dataclass(frozen=True)
class ScrollRackSwap(Effect):
    """"Exile any number of cards from your hand face down. Put that many
    cards from the top of your library into your hand. Then look at the
    exiled cards and put them on top of your library in any order."
    (Scroll Rack.) Cards to exile are picked one at a time
    (``choose_card_from_hand``, ``None`` stops); the order they go back
    is a ``choose_card_from_library`` pick per position, top first. Not
    a draw: with too few cards in the library you just get those."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        exiled = []
        while p.hand and p.agent is not None:
            pick = p.agent.choose_card_from_hand(
                state, controller_id, [c.id for c in p.hand],
            )
            card = next((c for c in p.hand if c.id == pick), None)
            if card is None:
                break
            p.hand.remove(card)
            card.zone = Zone.EXILE
            card.exiled_face_down = True
            p.exile.append(card)
            exiled.append(card)
        for _ in range(min(len(exiled), len(p.library))):
            top = p.library.pop(0)
            top.zone = Zone.HAND
            p.hand.append(top)
            reveal_to(top, controller_id)
        order = []
        while len(order) < len(exiled):
            options = [c for c in exiled if c not in order]
            order.append(search_pick(state, p, options, may_fail=False))
        for c in reversed(order):
            p.exile.remove(c)
            c.exiled_face_down = False  # left exile (CR 400.7)
            c.zone = Zone.LIBRARY
            p.library.insert(0, c)
        # Face down: the other players can't tell which cards went back.
        from mtgz_ir.game import hide_hand_moved_into_library

        hide_hand_moved_into_library(state, controller_id, order)
        state.log.append(f"P{controller_id} swaps {len(exiled)} card(s)")

    def describe(self) -> str:
        return (
            "exile any number of cards from your hand face down; put that "
            "many from the top of your library into your hand; put the "
            "exiled cards on top in any order"
        )


class AttackerYouControlWithoutSubtype(_TriggerPredicate):
    """``Event.ATTACKS`` predicate: "a non-<Subtype> creature you control
    attacks" (Winota: non-Human)."""

    def __init__(self, subtype: str) -> None:
        self.subtype = subtype

    def matches(self, state, subjects, source) -> bool:
        from mtgz_ir.game import effective_subtypes

        attacker = subjects.get("attacker")
        return (
            attacker is not None and source is not None
            and attacker.controller_id == source.controller_id
            and self.subtype not in effective_subtypes(state, attacker)
        )

    def __eq__(self, other):
        return (
            isinstance(other, AttackerYouControlWithoutSubtype)
            and other.subtype == self.subtype
        )

    def __hash__(self):
        return hash(("AttackerYouControlWithoutSubtype", self.subtype))

    def __repr__(self):
        return f"AttackerYouControlWithoutSubtype({self.subtype!r})"


@dataclass(frozen=True)
class LookTopMayPutAttackingGainKeyword(Effect):
    """"Look at the top N cards of your library. You may put a <Human
    creature> card from among them onto the battlefield tapped and
    attacking. It gains <indestructible> until end of turn. Put the rest
    of the cards on the bottom of your library in a random order."
    (Winota.) It attacks the attacking player's opponent (two-player);
    it was never declared as an attacker, so attack triggers don't
    fire for it."""

    count: int = 6
    card_filter: CardFilter = CardFilter()
    keyword: KeywordSpelling = "indestructible"

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantKeywords
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        looked = [p.library.pop(0) for _ in range(min(self.count, len(p.library)))]
        for c in looked:
            reveal_to(c, controller_id)
        options = [c for c in looked if self.card_filter.matches_card(state, c)]
        pick = search_pick(state, p, options, may_fail=True)
        rest = [c for c in looked if c is not pick]
        state.rng.shuffle(rest)
        for c in rest:
            c.zone = Zone.LIBRARY
            p.library.append(c)
        if pick is None:
            return
        put_into_battlefield(state, pick, controller_id)
        if pick not in state.battlefield:
            return
        pick.tapped = True
        pick.attacking = True
        ApplyEffect(
            inner=GrantKeywords(keywords=frozenset({self.keyword})),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", pick.id),))
        state.log.append(
            f"{pick.printing.name} enters tapped and attacking with "
            f"{self.keyword}"
        )

    def describe(self) -> str:
        return (
            f"look at the top {self.count} cards; you may put a "
            f"{self.card_filter.describe()} onto the battlefield tapped and "
            f"attacking with {self.keyword} until end of turn; the rest on "
            f"the bottom in a random order"
        )


# ---------------------------------------------------------------------
# Graveyard zone-change watch (F14): Narcomoeba, Slogurk, Kishla
# Skimmer, Gau
# ---------------------------------------------------------------------


class _WatchMarker(_TriggerPredicate):
    """Base of the graveyard-watch marker predicates: they never match a
    dispatched event; :func:`sweep_graveyard_zone_watch` stacks their
    abilities."""

    def matches(self, state, subjects, source) -> bool:
        return False

    def __eq__(self, other):
        return type(other) is type(self)

    def __hash__(self):
        return hash(type(self).__name__)

    def __repr__(self):
        return f"{type(self).__name__}()"


class PutIntoYourGraveyardFromYourLibrary(_WatchMarker):
    """"When this card is put into your graveyard from your library"
    (Narcomoeba) -- the card's own ability, stacked as it arrives."""


class LandCardPutIntoYourGraveyard(_WatchMarker):
    """"Whenever a land card is put into your graveyard from anywhere"
    (Slogurk) -- a battlefield permanent's ability."""


class CardLeftYourGraveyardDuringYourTurn(_WatchMarker):
    """"Whenever a card leaves your graveyard during your turn" (Kishla
    Skimmer; its "only once each turn" is the ability's
    ``once_per_turn``)."""


class CardLeftYourGraveyardThisTurn(_TriggerPredicate):
    """Intervening "if a card left your graveyard this turn" on an
    end-step trigger (Gau, Feral Youth): checked as it triggers, from
    ``Player.card_left_graveyard_turn``."""

    def matches(self, state, subjects, source) -> bool:
        if source is None:
            return False
        p = state.player_by_id(source.controller_id)
        return p.card_left_graveyard_turn == state.turn_number

    def __eq__(self, other):
        return isinstance(other, CardLeftYourGraveyardThisTurn)

    def __hash__(self):
        return hash("CardLeftYourGraveyardThisTurn")

    def __repr__(self):
        return "CardLeftYourGraveyardThisTurn()"


_WATCH_PREDICATES = (
    PutIntoYourGraveyardFromYourLibrary, LandCardPutIntoYourGraveyard,
    CardLeftYourGraveyardDuringYourTurn, CardLeftYourGraveyardThisTurn,
)


def _all_objects(state):
    for p in state.players:
        yield from p.hand
        yield from p.library
        yield from p.graveyard
        yield from p.exile
    yield from state.battlefield
    for item in state.stack:
        yield item.source


def zone_watch_active(state) -> bool:
    """Is any card in this game watching graveyard zone changes? Decided
    once per game (the first sweep scans every zone) so games without
    such cards pay nothing."""
    active = state.p2_zone_watch
    if active is None:
        active = any(
            isinstance(t.predicate, _WATCH_PREDICATES)
            for o in _all_objects(state)
            for t in o.printing.triggered_abilities
        )
        state.p2_zone_watch = active
    return active


def note_left_library(state, player, card) -> None:
    """``card`` left ``player``'s library some other way than being put
    into the graveyard (drawn, found by a search): if it reaches the
    graveyard before the next sweep it didn't come from the library."""
    if state.p2_zone_watch:
        player.left_library_ids = player.left_library_ids | {card.id}


def _push_watch_triggers(state, source, marker, controller_id, label) -> bool:
    from mtgz_ir.actions import push_triggered_ability

    fired = False
    for idx, ability in enumerate(source.printing.triggered_abilities):
        if isinstance(ability.predicate, marker):
            fired = push_triggered_ability(
                state, source=source, ability=ability, ability_index=idx,
                controller_id=controller_id, label=label,
            ) or fired
    return fired


def sweep_graveyard_zone_watch(state) -> bool:
    """State-based sweep (with the exile-instead catch-all) comparing
    each player's library and graveyard with the previous sweep:

    * a card that left the graveyard marks ``card_left_graveyard_turn``
      and, during that player's turn, stacks their "whenever a card
      leaves your graveyard during your turn" abilities;
    * a card that arrived from the library (it was there at the last
      sweep and wasn't drawn or found since) stacks its own "put into
      your graveyard from your library" ability;
    * a land card that arrived stacks the owner's "whenever a land card
      is put into your graveyard from anywhere" abilities.
    """
    if not zone_watch_active(state):
        return False
    fired = False
    active_id = state.players[state.active_player_idx].id
    for p in state.players:
        prev = p.zone_watch
        library = frozenset(c.id for c in p.library)
        graveyard = frozenset(c.id for c in p.graveyard)
        p.zone_watch = (library, graveyard)
        not_milled = p.left_library_ids
        p.left_library_ids = frozenset()
        if not prev:
            continue
        prev_library, prev_graveyard = prev
        if prev_graveyard - graveyard:
            p.card_left_graveyard_turn = state.turn_number
            if p.id == active_id:
                for o in list(state.battlefield):
                    if o.controller_id == p.id:
                        fired = _push_watch_triggers(
                            state, o, CardLeftYourGraveyardDuringYourTurn,
                            p.id, "card left your graveyard",
                        ) or fired
        for c in list(p.graveyard):
            if c.id in prev_graveyard:
                continue
            if c.id in prev_library and c.id not in not_milled:
                fired = _push_watch_triggers(
                    state, c, PutIntoYourGraveyardFromYourLibrary, p.id,
                    "put into your graveyard from your library",
                ) or fired
            if c.printing.is_land:
                for o in list(state.battlefield):
                    if o.controller_id == p.id:
                        fired = _push_watch_triggers(
                            state, o, LandCardPutIntoYourGraveyard, p.id,
                            "land card put into your graveyard",
                        ) or fired
    return fired


def run_zone_sweeps(state) -> bool:
    """``turn.state_based_actions`` hook: the P2 sweeps, exile-instead
    first (an exiled-instead card never looks like an arrival)."""
    changed = apply_graveyard_exile_instead(state)
    if sweep_graveyard_zone_watch(state):
        changed = True
    return changed


@dataclass(frozen=True)
class SelfDealsDamageEqualToPowerToEachOpponent(Effect):
    """"<This creature> deals damage equal to its power to each
    opponent." (Gau, Feral Youth.) Last-known power if it already left
    the battlefield."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.game import effective_power

        src = state.resolving_source
        if src is None:
            return
        n = max(0, effective_power(state, src))
        for p in state.players:
            if p.id != controller_id and not p.has_lost and n:
                _apply_damage(state, ("player", p.id), n)
        state.log.append(f"{src.printing.name} deals {n} to each opponent")

    def describe(self) -> str:
        return "this creature deals damage equal to its power to each opponent"


@dataclass(frozen=True)
class ReturnSourceToOwnersHand(Effect):
    """"Return this creature to its owner's hand." -- the resolving
    ability's source, if it is still on the battlefield."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.targets_helpers import _bounce_to_hand

        src = state.resolving_source
        if src is not None and src in state.battlefield:
            _bounce_to_hand(state, src)
            state.log.append(f"{src.printing.name} returns to its owner's hand")

    def describe(self) -> str:
        return "return this creature to its owner's hand"


# ---------------------------------------------------------------------
# Entry replacements and self-regeneration (Golgari Grave-Troll)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class EntersWithCountersPerGraveyardCard:
    """A ``CardPrinting.zone_abilities`` spec: "This creature enters with
    a +1/+1 counter on it for each <creature> card in your graveyard."
    (Golgari Grave-Troll.) Counted as it enters (CR 614.1c)."""

    card_filter: CardFilter = CardFilter()

    def describe(self) -> str:
        return (
            f"enters with a +1/+1 counter for each "
            f"{self.card_filter.describe()} in your graveyard"
        )


def on_enter_battlefield(state, obj) -> None:
    """``actions.put_into_battlefield`` hook for a permanent with P2 zone
    abilities, as it enters (before its enters triggers)."""
    from mtgz_ir.game import add_plus_one_counters

    note_exile_instead_source_entered(state, obj)
    for spec in obj.printing.zone_abilities:
        if isinstance(spec, EntersWithCountersPerGraveyardCard):
            p = state.player_by_id(obj.controller_id)
            n = sum(1 for c in p.graveyard if spec.card_filter.matches_card(state, c))
            if n:
                add_plus_one_counters(state, obj, n)
                state.log.append(
                    f"{obj.printing.name} enters with {n} +1/+1 counter(s)"
                )


@dataclass(frozen=True)
class RegenerateSource(Effect):
    """"Regenerate this creature." -- the resolving ability's source gets
    the base's regeneration shield (one shield; it is used up by the next
    destruction this turn and cleared at cleanup)."""

    def resolve(self, state, controller_id, targets=()):
        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        src.regeneration_shield = True
        state.log.append(f"{src.printing.name} gains a regeneration shield")

    def describe(self) -> str:
        return "regenerate this creature"
