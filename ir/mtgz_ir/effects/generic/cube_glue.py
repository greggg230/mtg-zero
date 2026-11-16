"""Engine effects for the cube "glue" bucket.

The last unclaimed PowerLSV cube cards each needed one piece the other
buckets didn't ship. Every effect here is generic (parameterized by the
printed shape), not per-card; the parser side is
``cards/parser/cube_glue_grammar.py``. Shortcuts are documented in the
"glue" section of ``docs/SEARCH_LIMITATIONS.md``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.keyword_vocab import KeywordSpelling
from mtgz_ir.effects.generic.roto_search import (
    library_search,
    searched_card_taken,
)
from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# ---------------------------------------------------------------------------
# Linked "the exiled card" returns (CR 607.2a)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ReturnCardsExiledWithSourceToOwnersHand(Effect):
    """"Return the exiled card to its owner's hand." (Tidehollow
    Sculler's leaves-the-battlefield half). Every card linked to the
    resolving source's battlefield incarnation (the imprint link,
    ``GameObject.imprinted_on``) goes back to its owner's hand. A card
    exiled by the linked ETB AFTER this resolved (the classic "Sculler
    left in response" line) was linked too late and stays exiled."""

    consumes_exile_link = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_wg import cards_exiled_with
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if source is None:
            return
        for card in cards_exiled_with(state, source):
            owner = state.player_by_id(card.owner_id)
            owner.exile.remove(card)
            card.exiled_face_down = False  # left exile (CR 400.7)
            card.imprinted_on = None
            card.zone = Zone.HAND
            owner.hand.append(card)
            state.log.append(
                f"{card.printing.name} returns from exile to P{owner.id}'s "
                f"hand"
            )

    def describe(self) -> str:
        return "return the exiled card to its owner's hand"


@dataclass(frozen=True)
class RevealHandMayExileCardOrChosenCreatureUntilSourceLeaves(Effect):
    """"Choose target opponent and up to one target creature they
    control. They reveal their hand. You may exile a <filter> card from
    their hand or the chosen creature until <this> leave(s) the
    battlefield." (Cloak and Dagger, Entwined.)

    Two target slots: the opponent (``"player"``) and up to one creature
    (``"creature"``) that opponent controls -- with two players "they
    control" is "an opponent controls". On resolution the opponent's
    hand is revealed to the controller; the controller may exile ONE
    object: a matching hand card (it returns to its owner's hand when
    the source leaves -- Deep-Cavern Bat's ``exiled_until_leaves_from_hand``
    link) or the chosen creature (it returns to the battlefield -- the
    Oblivion-Ring ``exiled_by_source`` link). The pick among both kinds
    goes through the generic id picker ``choose_card_from_library``
    after a ``choose_yes_no`` consent. If the source already left the
    battlefield, nothing is exiled (CR 610.3c)."""

    card_filter: object = None

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (1, 2)

    def target_slot_kinds(self) -> tuple[str, ...]:
        return ("player", "creature")

    def legal_targets(self, state, controller_id):
        from mtgz_ir.game import is_creature_now

        out: list[Target] = [
            ("player", p.id) for p in state.players
            if not p.has_lost and p.id != controller_id
        ]
        out.extend(
            ("creature", o.id) for o in state.battlefield
            if is_creature_now(o) and o.controller_id != controller_id
        )
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to
        from mtgz_ir.turn import move_off_battlefield

        opp = next(
            (state.player_by_id(t[1]) for t in targets if t[0] == "player"),
            None,
        )
        if opp is None or opp.has_lost:
            return
        for c in opp.hand:
            reveal_to(c, controller_id)
        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        creature = next(
            (
                o for t in targets if t[0] == "creature"
                for o in state.battlefield
                if o.id == t[1] and o.controller_id == opp.id
            ),
            None,
        )
        options = [
            c for c in opp.hand
            if self.card_filter is None or self.card_filter.matches_card(state, c)
        ]
        choices = [c.id for c in options]
        if creature is not None:
            choices.append(creature.id)
        if not choices:
            return
        me = state.player_by_id(controller_id)
        pick = choices[0]
        if me.agent is not None:
            if not me.agent.choose_yes_no(
                state, controller_id,
                "exile a card from their hand or the chosen creature until "
                "this leaves?",
            ):
                return
            picked = me.agent.choose_card_from_library(
                state, controller_id, choices,
            )
            if picked in choices:
                pick = picked
        if creature is not None and pick == creature.id:
            creature.exiled_by_source = src.id
            state.log.append(
                f"{creature.printing.name} is exiled until "
                f"{src.printing.name} leaves the battlefield"
            )
            move_off_battlefield(state, creature, destination="exile")
            return
        card = next(c for c in options if c.id == pick)
        opp.hand.remove(card)
        card.zone = Zone.EXILE
        card.exiled_by_source = src.id
        card.exiled_until_leaves_from_hand = True
        opp.exile.append(card)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} from P{opp.id}'s "
            f"hand until {src.printing.name} leaves the battlefield"
        )

    def describe(self) -> str:
        return (
            "choose target opponent and up to one target creature they "
            "control; they reveal their hand; you may exile a card from it "
            "or the chosen creature until this leaves the battlefield"
        )


# ---------------------------------------------------------------------------
# Graveyard -> hand for any card descriptor
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ReturnTargetGraveyardCardToHand(Effect):
    """"Return [up to one ]target <descriptor> card from your graveyard
    to your hand." for any ``CardFilter`` descriptor (Wrenn and Six's
    "up to one target land card", Tamiyo, Collector of Tales' "target
    card"). Targets are ``("graveyard_card", id)``; a target that left
    the graveyard before resolution does nothing."""

    card_filter: object = None
    optional: bool = False

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0 if self.optional else 1, 1)

    def legal_targets(self, state, controller_id):
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_card", c.id) for c in p.graveyard
            if self.card_filter is None or self.card_filter.matches_card(state, c)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        for kind, tid in targets:
            if kind != "graveyard_card":
                continue
            card = next((c for c in p.graveyard if c.id == tid), None)
            if card is None:
                continue
            p.graveyard.remove(card)
            card.zone = Zone.HAND
            p.hand.append(card)
            reveal_to_all(state, card)
            state.log.append(
                f"{card.printing.name} returns from P{controller_id}'s "
                f"graveyard to hand"
            )

    def describe(self) -> str:
        upto = "up to one " if self.optional else ""
        return f"return {upto}target card from your graveyard to your hand"


# ---------------------------------------------------------------------------
# "Exile the top card of your library. You may cast that card. If you
# don't, <effect>." (Chandra, Torch of Defiance)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ExileTopCardMayCastOtherwise(Effect):
    """Exile the top card of the controller's library; the controller may
    cast it right now (paying its costs -- the U/B bucket's
    ``creature_casting_ub.cast_now``: the spell goes on the stack above
    this ability, timing restrictions don't apply). If they don't
    (declined, a land, uncastable, or nothing was exiled), ``otherwise``
    resolves. An uncast card stays in exile with no permission."""

    otherwise: Effect | None = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.effects.generic.cube_creatures_rmc import exile_card

        p = state.player_by_id(controller_id)
        cast = False
        if p.library:
            card = p.library.pop(0)
            exile_card(state, card)
            state.log.append(
                f"P{controller_id} exiles {card.printing.name} from the top "
                f"of their library"
            )
            if (
                can_cast_now(state, controller_id, card, free=False)
                and (
                    p.agent is None
                    or p.agent.choose_yes_no(
                        state, controller_id, f"cast {card.printing.name}?",
                    )
                )
            ):
                cast = cast_now(state, controller_id, card, free=False)
        if not cast and self.otherwise is not None:
            self.otherwise.resolve(state, controller_id, ())

    def describe(self) -> str:
        tail = (
            f"; if you don't, {self.otherwise.describe()}"
            if self.otherwise is not None else ""
        )
        return (
            "exile the top card of your library; you may cast that card"
            + tail
        )


# ---------------------------------------------------------------------------
# "Choose a nonland card name, then reveal the top N cards of your
# library. Put all cards with the chosen name from among them into your
# hand and the rest into your graveyard." (Tamiyo, Collector of Tales)
# ---------------------------------------------------------------------------

NAME_NOT_IN_LIBRARY = "(a card name not in your library)"


@dataclass(frozen=True)
class ChooseNameRevealTopPutNamedIntoHand(Effect):
    """The name is chosen through ``choose_mode`` over the distinct
    (nonland, when ``nonland``) names in the controller's library plus
    one entry standing for any other name -- the spells-B bucket's
    Demonic Consultation convention (naming a card that isn't in the
    library is equivalent to naming any other absent card). The top
    ``count`` cards are revealed; every card with the chosen name goes
    to the hand, the rest to the graveyard."""

    count: int = 4
    nonland: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        names = sorted({
            c.printing.name for c in p.library
            if not (self.nonland and c.printing.is_land)
        })
        options = names + [NAME_NOT_IN_LIBRARY]
        idx = len(options) - 1
        if p.agent is not None:
            picked = p.agent.choose_mode(state, controller_id, options)
            if isinstance(picked, int) and 0 <= picked < len(options):
                idx = picked
        chosen = options[idx]
        state.log.append(f"P{controller_id} names {chosen}")
        revealed = p.library[: self.count]
        del p.library[: self.count]
        to_hand = 0
        for c in revealed:
            reveal_to_all(state, c)
            if c.printing.name == chosen:
                c.zone = Zone.HAND
                p.hand.append(c)
                to_hand += 1
            else:
                c.zone = Zone.GRAVEYARD
                p.graveyard.append(c)
        state.log.append(
            f"P{controller_id} reveals {len(revealed)} card(s); {to_hand} "
            f"named card(s) to hand, the rest to the graveyard"
        )

    def describe(self) -> str:
        kind = "nonland " if self.nonland else ""
        return (
            f"choose a {kind}card name; reveal the top {self.count} cards; "
            f"put the named ones into your hand and the rest into your "
            f"graveyard"
        )


# ---------------------------------------------------------------------------
# "At the beginning of your next main phase" delayed mana (Mana Drain) and
# counter-removal mana (Coalition Relic)
# ---------------------------------------------------------------------------


def stack_spell_mana_value(item) -> int:
    """Mana value of a spell on the stack (CR 202.3e: X counts)."""
    cost = item.source.printing.mana_cost
    return cost.total + cost.x_pips * max(0, item.x_value)


@dataclass(frozen=True)
class CounterThenDelayedManaEqualToMV(Effect):
    """"Counter target spell. At the beginning of your next main phase,
    add an amount of {<color>} equal to that spell's mana value." (Mana
    Drain.) ``counter`` is the parsed counter effect; its target is this
    effect's target. The countered spell's mana value is taken as it
    last existed on the stack (X included). If the spell wasn't
    countered (it left the stack, or can't be countered) no delayed
    trigger is created. The delayed trigger is a
    ``state.delayed_triggers`` entry with ``"step": "main"``, fired by
    ``turn.run_delayed_main_phase_triggers`` at the controller's next
    main phase (both turn loops)."""

    counter: Effect | None = None
    color: str = "C"

    def requires_target(self) -> bool:
        return self.counter.requires_target()

    def legal_targets(self, state, controller_id):
        return self.counter.legal_targets(state, controller_id)

    def target_count_range(self):
        return self.counter.target_count_range()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.mana import AddManaToPool

        item = None
        if targets and targets[0][0] == "stack":
            idx = targets[0][1]
            if 0 <= idx < len(state.stack):
                item = state.stack[idx]
        was_countered = item is not None and item.countered
        self.counter.resolve(state, controller_id, targets)
        if item is None or was_countered or not item.countered:
            return
        mv = stack_spell_mana_value(item)
        source = state.resolving_spell or state.resolving_source
        state.delayed_triggers.append({
            "step": "main",
            "controller_id": controller_id,
            "source_id": source.id if source is not None else None,
            "effect": AddManaToPool(produces=((self.color, mv),)),
            "fire_at_turn": state.turn_number,
        })
        state.log.append(
            f"P{controller_id} will add {mv} {{{self.color}}} at the "
            f"beginning of their next main phase"
        )

    def describe(self) -> str:
        return (
            f"{self.counter.describe()}; at the beginning of your next main "
            f"phase, add {{{self.color}}} equal to that spell's mana value"
        )


@dataclass(frozen=True)
class RemoveAllCountersAddManaPerCounter(Effect):
    """"Remove all <kind> counters from this <permanent>. Add one mana of
    any color for each <kind> counter removed this way." (Coalition
    Relic.) The mana is ``any``-color coins (the engine's one-mana-of-
    any-color convention)."""

    kind: str = "charge"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.mana import AddManaToPool
        from mtgz_ir.game import counter_count, remove_counters

        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        n = counter_count(src, self.kind)
        removed = remove_counters(src, self.kind, n) if n else 0
        state.log.append(
            f"{src.printing.name}: removes {removed} {self.kind} counter(s)"
        )
        if removed:
            AddManaToPool(produces=(("any", removed),)).resolve(
                state, controller_id,
            )

    def describe(self) -> str:
        return (
            f"remove all {self.kind} counters from this permanent; add one "
            f"mana of any color for each"
        )


# ---------------------------------------------------------------------------
# Hideaway (CR 702.75) and "play the exiled card without paying its mana
# cost" (Shelldock Isle)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class HideawayLookExileOneFaceDown(Effect):
    """Hideaway N's enters trigger: look at the top ``n`` cards of your
    library, exile one of them face down, then put the rest on the
    bottom in a random order. The exiled card is linked to the source's
    battlefield incarnation (``GameObject.imprinted_on``) and known only
    to the controller. The pick goes through ``choose_card_from_library``.
    """

    n: int = 4

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import exile_card
        from mtgz_ir.effects.generic.cube_creatures_wg import exile_link_key
        from mtgz_ir.game import reveal_to

        p = state.player_by_id(controller_id)
        looked = p.library[: self.n]
        if not looked:
            return
        del p.library[: len(looked)]
        for c in looked:
            reveal_to(c, controller_id)
        pick = looked[0]
        if p.agent is not None:
            picked = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in looked],
            )
            pick = next((c for c in looked if c.id == picked), looked[0])
        exile_card(state, pick, face_down_for=controller_id)
        src = state.resolving_source
        if src is not None:
            pick.imprinted_on = exile_link_key(src)
        rest = [c for c in looked if c is not pick]
        state.rng.shuffle(rest)
        p.library.extend(rest)
        state.log.append(
            f"P{controller_id} hides a card away (looked at {len(looked)})"
        )

    def describe(self) -> str:
        return (
            f"look at the top {self.n} cards of your library, exile one face "
            f"down, then put the rest on the bottom in a random order"
        )


@dataclass(frozen=True)
class PlayExiledCardFreeIfAnyLibraryAtMost(Effect):
    """"You may play the exiled card without paying its mana cost if a
    library has <N> or fewer cards in it." (Shelldock Isle.) The
    condition is checked on resolution; the exiled card is the one
    linked to this source by Hideaway. A spell is cast right away
    (``creature_casting_ub.cast_now``, free) -- timing restrictions don't
    apply; a land can be played only on the controller's turn with a
    land play left (it counts as the land play)."""

    library_at_most: int = 20

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.effects.generic.cube_creatures_rmc import grant_exile_play
        from mtgz_ir.effects.generic.cube_creatures_wg import cards_exiled_with

        src = state.resolving_source
        if src is None:
            return
        if not any(
            len(pl.library) <= self.library_at_most for pl in state.players
        ):
            state.log.append(
                f"{src.printing.name}: no library has "
                f"{self.library_at_most} or fewer cards"
            )
            return
        p = state.player_by_id(controller_id)
        for card in cards_exiled_with(state, src):
            if card.printing.is_land:
                if not self._land_play_available(state, controller_id):
                    continue
            elif not can_cast_now(state, controller_id, card, free=True):
                continue
            if p.agent is not None and not p.agent.choose_yes_no(
                state, controller_id, f"play {card.printing.name} for free?",
            ):
                continue
            card.imprinted_on = None
            if card.printing.is_land:
                from mtgz_ir.actions import _play_land

                grant_exile_play(state, card, controller_id)
                _play_land(state, controller_id, card.id)
            else:
                cast_now(state, controller_id, card, free=True)
            return

    @staticmethod
    def _land_play_available(state, controller_id) -> bool:
        p = state.player_by_id(controller_id)
        if state.active_player().id != controller_id:
            return False
        allowed = 1 + sum(
            o.printing.grants_additional_land_per_turn
            for o in state.battlefield if o.controller_id == controller_id
        )
        return p.lands_played_this_turn < allowed

    def describe(self) -> str:
        return (
            "you may play the exiled card without paying its mana cost if a "
            f"library has {self.library_at_most} or fewer cards in it"
        )


# ---------------------------------------------------------------------------
# "Target <np> [loses all abilities and ]becomes a <color> <Subtype>
# creature with base power and toughness P/T." (Oko, Thief of Crowns)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TargetBecomesCreatureIndefinitely(Effect):
    """A duration-less characteristic change on one target (CR 611.2a:
    it lasts as long as the permanent stays on the battlefield, even
    after the source leaves). Registry entries are SELF-sourced (source
    = the affected permanent, the Earthbend precedent), so they end
    only when the permanent leaves:

    * layer 4 ``add_type "creature"`` (card types and supertypes are
      kept -- an Oko'd Food is an artifact creature) and
      ``set_creature_subtypes`` (creature types replaced, CR 205.1b);
    * layer 5 ``set_colors``;
    * layer 6 ``lose_all_abilities`` when ``lose_all_abilities``;
    * layer 7b ``set_base_pt``.

    Later timestamps win, so a second Oko activation re-applies over the
    first; an effect that later grants abilities still applies."""

    targets: tuple = ()
    lose_all_abilities: bool = False
    colors: frozenset = frozenset()
    creature_subtypes: tuple = ()
    base_pt: tuple = (0, 0)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        if not targets:
            return
        chosen = targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], chosen)
        finally:
            state.resolving_controller_id = prior
        if not legal:
            return
        obj = next(o for o in state.battlefield if o.id == chosen[1])
        name = obj.printing.name
        if self.lose_all_abilities:
            state.add_continuous_effect(
                source_id=obj.id, layer=6, kind="lose_all_abilities",
                target_id=obj.id, description=f"{name} loses all abilities",
            )
        state.add_continuous_effect(
            source_id=obj.id, layer=4, kind="add_type", target_id=obj.id,
            params=("creature",), description=f"{name} becomes a creature",
        )
        if (
            not obj.printing.is_creature
            and obj.entered_battlefield_turn == state.turn_number
        ):
            # CR 302.6: a noncreature that entered this turn becomes a
            # creature that hasn't been controlled since the turn began.
            obj.summoning_sick = True
        obj.permanent_is_creature = True
        state.add_continuous_effect(
            source_id=obj.id, layer=4, kind="set_creature_subtypes",
            target_id=obj.id, params=tuple(self.creature_subtypes),
            description=f"{name} becomes a {' '.join(self.creature_subtypes)}",
        )
        if self.colors:
            state.add_continuous_effect(
                source_id=obj.id, layer=5, kind="set_colors",
                target_id=obj.id, params=(frozenset(self.colors),),
                description=f"{name} becomes {'/'.join(sorted(self.colors))}",
            )
        p, t = self.base_pt
        state.add_continuous_effect(
            source_id=obj.id, layer=7, sublayer=1, kind="set_base_pt",
            target_id=obj.id, params=(p, t),
            description=f"{name} base {p}/{t}",
        )
        state.log.append(
            f"{name} becomes a {' '.join(self.creature_subtypes)} {p}/{t}"
            + (" with no abilities" if self.lose_all_abilities else "")
        )

    def describe(self) -> str:
        lose = "loses all abilities and " if self.lose_all_abilities else ""
        return (
            f"target {lose}becomes a {' '.join(self.creature_subtypes)} "
            f"creature with base power and toughness "
            f"{self.base_pt[0]}/{self.base_pt[1]}"
        )


@dataclass(frozen=True)
class ExchangeControlOfTargets(Effect):
    """"Exchange control of target <A> and target <B>." (Oko's -5:
    A = artifact or creature you control, B = creature an opponent
    controls with power 3 or less). Two ``TargetSpec`` slots enumerated
    per slot (``legal_targets_for_slot``, the Saheeli precedent). If
    either target is illegal on resolution, or both have the same
    controller, nothing happens (CR 701.10). The exchange is indefinite
    and sets ``controller_id`` directly (Dack Fayden's permanent-steal
    precedent); each permanent is summoning-sick for its new
    controller."""

    targets: tuple = ()

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def legal_targets_for_slot(self, state, controller_id, slot_idx):
        return self._enumerate_for_filter(
            state, controller_id, self.targets[slot_idx].filter,
        )

    def legal_targets(self, state, controller_id):
        first = self.legal_targets_for_slot(state, controller_id, 0)
        second = self.legal_targets_for_slot(state, controller_id, 1)
        for a in first:
            if any(b != a for b in second):
                return first
        return []

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )

        if len(targets) < 2 or targets[0] == targets[1]:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = all(
                _slot_target_still_legal(state, slot, chosen)
                for slot, chosen in zip(self.targets, targets)
            )
        finally:
            state.resolving_controller_id = prior
        if not legal:
            state.log.append("exchange of control fizzles")
            return
        a = next(o for o in state.battlefield if o.id == targets[0][1])
        b = next(o for o in state.battlefield if o.id == targets[1][1])
        if a.controller_id == b.controller_id:
            return
        a.controller_id, b.controller_id = b.controller_id, a.controller_id
        a.summoning_sick = True
        b.summoning_sick = True
        state._invalidate_layered_cache()
        state.log.append(
            f"{a.printing.name} and {b.printing.name} exchange controllers"
        )

    def describe(self) -> str:
        return "exchange control of two target permanents"


# ---------------------------------------------------------------------------
# "Search your library for any number of <desc> cards, exile them, then
# shuffle. Until end of turn, you may cast those cards without paying
# their mana costs." (Ugin, Eye of the Storms' -11)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SearchAnyNumberExileCastFreeThisTurn(Effect):
    """Each pick goes through ``choose_card_from_library`` over the
    remaining matching cards; ``None`` stops the search ("any number",
    CR 701.19b lets it find none). Every exiled card gets the R/multi
    bucket's ``ExilePlayPermission`` (free, cast only, this turn)."""

    card_filter: object = None
    colorless: bool = False

    def _matches(self, state, card) -> bool:
        from mtgz_ir.game import effective_colors

        if self.card_filter is not None and not self.card_filter.matches(
            card.printing,
        ):
            return False
        return not self.colorless or not effective_colors(state, card)

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            THIS_TURN,
            exile_card,
            grant_exile_play,
        )
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        found = 0
        while True:
            options = [c for c in p.library if self._matches(state, c)]
            if not options:
                break
            pick = options[0].id
            if p.agent is not None:
                pick = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in options],
                )
            card = next((c for c in options if c.id == pick), None)
            if card is None:
                break
            p.library.remove(card)
            if searched_card_taken(state, controller_id, card):
                continue
            exile_card(state, card)
            grant_exile_play(
                state, card, controller_id, duration=THIS_TURN, free=True,
                cast_only=True,
            )
            found += 1
        shuffle_library(state, controller_id)
        state.log.append(
            f"P{controller_id} exiles {found} card(s); may cast them free "
            f"this turn"
        )

    def describe(self) -> str:
        return (
            "search your library for any number of cards, exile them, then "
            "shuffle; until end of turn you may cast them without paying "
            "their mana costs"
        )


# ---------------------------------------------------------------------------
# Nissa, Who Shakes the World
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PutCountersUntapItBecomesCreature(Effect):
    """"Put <N> <kind> counters on up to one target <np>. Untap it. It
    becomes a P/T <Subtypes> creature with <keywords> that's still a
    <type>." (Nissa, Who Shakes the World's +1.) The animation has no
    duration: self-sourced registry entries (layer 4 ``add_type`` /
    ``add_subtype``, layer 6 ``grant_keyword``, layer 7b ``set_base_pt``)
    last until the permanent leaves the battlefield -- the permanent
    counterpart of the base's UEOT ``BecomeLandCreatureUEOT``. A land
    that entered this turn becomes a summoning-sick creature (haste
    usually makes that moot)."""

    targets: tuple = ()
    counter_kind: str = "+1/+1"
    counter_count: int = 0
    base_pt: tuple = (0, 0)
    subtypes: tuple = ()
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.game import add_counters

        if not targets:
            return
        chosen = targets[0]
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], chosen)
        finally:
            state.resolving_controller_id = prior
        if not legal:
            return
        obj = next(o for o in state.battlefield if o.id == chosen[1])
        name = obj.printing.name
        if self.counter_count:
            add_counters(state, obj, self.counter_kind, self.counter_count)
        obj.tapped = False
        if not obj.printing.is_creature and (
            obj.entered_battlefield_turn == state.turn_number
        ):
            obj.summoning_sick = True
        state.add_continuous_effect(
            source_id=obj.id, layer=4, kind="add_type", target_id=obj.id,
            params=("creature",), description=f"{name} becomes a creature",
        )
        obj.permanent_is_creature = True
        for sub in self.subtypes:
            state.add_continuous_effect(
                source_id=obj.id, layer=4, kind="add_subtype",
                target_id=obj.id, params=(sub,),
                description=f"{name} becomes a {sub}",
            )
        for kw in sorted(self.keywords):
            state.add_continuous_effect(
                source_id=obj.id, layer=6, kind="grant_keyword",
                target_id=obj.id, params=(kw,), description=f"{name} has {kw}",
            )
        p, t = self.base_pt
        state.add_continuous_effect(
            source_id=obj.id, layer=7, sublayer=1, kind="set_base_pt",
            target_id=obj.id, params=(p, t), description=f"{name} base {p}/{t}",
        )
        state.log.append(
            f"{name} gets {self.counter_count} {self.counter_kind} counter(s), "
            f"untaps and becomes a {p}/{t} {' '.join(self.subtypes)} creature"
        )

    def describe(self) -> str:
        return (
            f"put {self.counter_count} {self.counter_kind} counters on up to "
            f"one target; untap it; it becomes a {self.base_pt[0]}/"
            f"{self.base_pt[1]} {' '.join(self.subtypes)} creature"
        )


@dataclass(frozen=True)
class SearchAnyNumberOntoBattlefield(Effect):
    """"Search your library for any number of <desc> cards, put them
    onto the battlefield[ tapped], then shuffle." Picks go through
    ``choose_card_from_library`` until ``None`` or no match is left."""

    card_filter: object = None
    tapped: bool = False

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        found = []
        while True:
            options = [
                c for c in p.library
                if self.card_filter is None or self.card_filter.matches_card(state, c)
            ]
            if not options:
                break
            pick = options[0].id
            if p.agent is not None:
                pick = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in options],
                )
            card = next((c for c in options if c.id == pick), None)
            if card is None:
                break
            p.library.remove(card)
            if searched_card_taken(state, controller_id, card):
                continue
            found.append(card)
        for card in found:
            put_into_battlefield(state, card, controller_id)
            if self.tapped and card in state.battlefield:
                card.tapped = True
        shuffle_library(state, controller_id)
        state.log.append(
            f"P{controller_id} puts {len(found)} card(s) onto the battlefield"
            + (" tapped" if self.tapped else "")
        )

    def describe(self) -> str:
        tapped = " tapped" if self.tapped else ""
        return (
            f"search your library for any number of cards, put them onto the "
            f"battlefield{tapped}, then shuffle"
        )


# ---------------------------------------------------------------------------
# Kaito, Bane of Nightmares
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DrawPerOpponentWhoLostLifeThisTurn(Effect):
    """"Draw a card for each opponent who lost life this turn." --
    ``Player.lost_life_this_turn`` (latched by ``turn.note_life_losses``,
    refreshed here first)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards, note_life_losses

        note_life_losses(state)
        n = sum(
            1 for p in state.players
            if p.id != controller_id and p.lost_life_this_turn
        )
        if n:
            draw_n_cards(state, controller_id, n)
        state.log.append(
            f"P{controller_id} draws {n} (opponents who lost life this turn)"
        )

    def describe(self) -> str:
        return "draw a card for each opponent who lost life this turn"


@dataclass(frozen=True)
class TapTargetPutStunCounters(Effect):
    """"Tap target <np>. Put <N> stun counters on it." (Kaito's -2.)"""

    targets: tuple = ()
    count: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.game import add_stun_counters
        from mtgz_ir.turn import tap_permanent

        if not targets:
            return
        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            return
        obj = next(o for o in state.battlefield if o.id == targets[0][1])
        if not obj.tapped:
            tap_permanent(state, obj)
        added = add_stun_counters(state, obj, self.count)
        state.log.append(
            f"{obj.printing.name} is tapped and gets {added} stun counter(s)"
        )

    def describe(self) -> str:
        return f"tap target creature; put {self.count} stun counters on it"



# ---------------------------------------------------------------------------
# Comet, Stellar Pup: die-roll loyalty ability pieces
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class AdjustSourceLoyalty(Effect):
    """"[+N]" / "[-N]" inside an effect (Comet's die table): put N
    loyalty counters on / remove N from the resolving planeswalker. Not a
    cost -- removal takes what's there (CR 122.8), and a walker that left
    the battlefield is unaffected."""

    delta: int = 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        src.loyalty_counters = max(0, src.loyalty_counters + self.delta)
        state.log.append(
            f"{src.printing.name}: {self.delta:+d} loyalty "
            f"(now {src.loyalty_counters})"
        )

    def describe(self) -> str:
        return f"[{self.delta:+d}]"


@dataclass(frozen=True)
class CreateThenTheyGainKeywordsUEOT(Effect):
    """"<create tokens>. They gain <keywords> until end of turn." -- the
    tokens the inner creation made (new battlefield objects under the
    controller) get UEOT keyword grants."""

    create: Effect | None = None
    keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        before = {o.id for o in state.battlefield}
        self.create.resolve(state, controller_id, targets)
        for o in state.battlefield:
            if o.id in before or o.controller_id != controller_id:
                continue
            for kw in sorted(self.keywords):
                state.add_continuous_effect(
                    source_id=o.id, layer=6, kind="grant_keyword",
                    target_id=o.id, params=(kw,), expires_at="cleanup",
                    description=f"{o.printing.name} gains {kw} until end of turn",
                )

    def describe(self) -> str:
        return (
            f"{self.create.describe()}; they gain "
            f"{', '.join(sorted(self.keywords))} until end of turn"
        )


@dataclass(frozen=True)
class ReturnChosenGraveyardCardToHand(Effect):
    """"Return a <descriptor> card from your graveyard to your hand." --
    untargeted: the card is chosen on resolution
    (``choose_card_from_library`` as the generic id picker; the
    instruction is mandatory, so a declining agent gets the first)."""

    card_filter: object = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        options = [
            c for c in p.graveyard
            if self.card_filter is None or self.card_filter.matches_card(state, c)
        ]
        if not options:
            return
        card = options[0]
        if p.agent is not None:
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            card = next((c for c in options if c.id == pick), options[0])
        p.graveyard.remove(card)
        card.zone = Zone.HAND
        p.hand.append(card)
        reveal_to_all(state, card)
        state.log.append(
            f"{card.printing.name} returns from P{controller_id}'s graveyard "
            f"to hand"
        )

    def describe(self) -> str:
        return "return a card from your graveyard to your hand"


@dataclass(frozen=True)
class SourceDealsLoyaltyDamageToChosen(Effect):
    """"<This> deals damage equal to the number of loyalty counters on
    <it> to a creature or player." -- untargeted: the recipient is
    chosen on resolution (hexproof doesn't stop it) through the agent's
    ``choose_target`` hook over every creature and living player."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.game import is_creature_now

        src = state.resolving_source
        if src is None:
            return
        amount = max(0, src.loyalty_counters)
        options: list = [
            ("creature", o.id) for o in state.battlefield if is_creature_now(o)
        ]
        options += [("player", p.id) for p in state.players if not p.has_lost]
        if amount == 0:
            return
        # Default: an opponent.
        pick = next(
            (t for t in options if t[0] == "player" and t[1] != controller_id),
            options[0],
        )
        me = state.player_by_id(controller_id)
        if me.agent is not None:
            chosen = me.agent.choose_target(state, controller_id, options)
            if chosen in options:
                pick = chosen
        state.log.append(
            f"{src.printing.name} deals {amount} damage (its loyalty)"
        )
        _apply_damage(state, pick, amount)

    def describe(self) -> str:
        return (
            "deal damage equal to this planeswalker's loyalty to a creature "
            "or player"
        )


@dataclass(frozen=True)
class GrantExtraLoyaltyActivationsThisTurn(Effect):
    """"You may activate <this>'s loyalty abilities N more times this
    turn." -- ``GameObject.extra_loyalty_activations`` =
    ``(turn_number, N)``, consumed by ``actions._activate_ability`` when
    the walker's once-per-turn ledger entry already exists and read by
    ``legal.loyalty_activation_available``."""

    count: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        turn, left = src.extra_loyalty_activations or (0, 0)
        if turn != state.turn_number:
            left = 0
        src.extra_loyalty_activations = (state.turn_number, left + self.count)
        state.log.append(
            f"{src.printing.name}: loyalty abilities may be activated "
            f"{self.count} more time(s) this turn"
        )

    def describe(self) -> str:
        return (
            f"you may activate its loyalty abilities {self.count} more "
            f"times this turn"
        )



# ---------------------------------------------------------------------------
# "After this phase, there is an additional combat phase." (Fear of
# Missing Out)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class AdditionalCombatPhaseAfterThisPhase(Effect):
    """Schedules an additional combat phase right after the current
    combat phase (``GameState.extra_combat_phases``, consumed by
    ``turn.next_step`` in both turn loops). The engine has no
    additional main phase here (the card doesn't print one), so the
    extra combat is followed by the turn's normal postcombat main
    phase. Resolving outside a combat phase schedules the extra combat
    after the turn's next combat phase instead of "this phase"."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.extra_combat_phases += 1
        state.log.append("an additional combat phase will follow this one")

    def describe(self) -> str:
        return "after this phase, there is an additional combat phase"
