"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import LibraryCardFilter
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
class LookAtTopNRevealOrCounter(Effect):
    """Look at the top ``n`` cards of your library. The controller may
    reveal one card matching ``filter_name`` and put it into hand.
    Put the rest on the bottom of the library in a random order. If no
    card was revealed (either because none matched or the agent
    declined), put a +1/+1 counter on the source.

    Matches Pulsar Squadron Ace's ETB with ``n=5``, ``filter_name=
    "spacecraft"``. Reads ``state.resolving_source`` for the counter
    placement.

    The "random order" shuffle is seeded deterministically on the turn
    number + controller + library length so tests remain reproducible.
    """

    n: int = 5
    filter_name: LibraryCardFilter = LibraryCardFilter.SPACECRAFT
    # Inclusive upper bound on printed power for the ``creature`` filter
    # — used by Living Phone's ``creature card with power 2 or less``
    # tutor body. ``0`` (default) means "no power cap" so the existing
    # Pulsar Squadron Ace ``creature`` use-site keeps its unrestricted
    # behaviour. Ignored for non-creature filters.
    max_power: int = 0
    # When ``True`` (default, matches Pulsar Squadron Ace), failing to
    # reveal a card puts a +1/+1 counter on the resolving source. Living
    # Phone has no counter rider — its oracle text is the bare
    # ``Put the rest on the bottom of your library in a random order``
    # tail with no ``Otherwise, put a +1/+1 counter`` clause — so its
    # printing carries ``counter_on_miss=False`` and the resolution
    # short-circuits to bottoming the remainder when no card is revealed.
    counter_on_miss: bool = True

    def __post_init__(self) -> None:
        object.__setattr__(self, "filter_name", LibraryCardFilter(self.filter_name))

    def _matches(self, printing) -> bool:
        fn = self.filter_name
        if fn == "spacecraft":
            return printing.is_spacecraft
        if fn == "creature":
            if not printing.is_creature:
                return False
            if self.max_power > 0 and printing.power > self.max_power:
                return False
            return True
        if fn == "artifact":
            return printing.is_artifact
        return False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        examined = p.library[: self.n]
        options = [c for c in examined if self._matches(c.printing)]
        chosen_id = None
        if options and p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options]
            )
        revealed_card = None
        if chosen_id is not None:
            revealed_card = next(
                (c for c in options if c.id == chosen_id), None
            )
        if revealed_card is not None:
            from mtgz_ir.game import reveal_to_all
            p.library.remove(revealed_card)
            revealed_card.zone = Zone.HAND
            p.hand.append(revealed_card)
            reveal_to_all(state, revealed_card)
            state.log.append(
                f"P{controller_id} reveals {revealed_card.printing.name} "
                f"and puts it into hand"
            )
        else:
            state.log.append(
                f"P{controller_id} reveals nothing from the top "
                f"{len(examined)} cards"
            )
            # Pulsar Squadron Ace's ``Otherwise, put a +1/+1 counter
            # on it`` rider. Living Phone's printing carries
            # ``counter_on_miss=False`` so a missed reveal there just
            # bottoms the remainder without a counter — matching its
            # printed text which has no such fallback clause.
            if self.counter_on_miss:
                source = state.resolving_source
                if source is not None and source in state.battlefield:
                    from mtgz_ir.game import add_plus_one_counters
                    add_plus_one_counters(state, source, 1)
                    state.log.append(
                        f"{source.printing.name} gets a +1/+1 counter "
                        f"(now {source.plus_one_counters})"
                    )
        # Bottom the remainder in a random order. Take up to n cards off
        # the top (those we examined except the revealed one if any) and
        # shuffle them onto the bottom.
        from mtgz_ir.game import reset_public_knowledge
        remaining_examined = [
            c for c in examined if c is not revealed_card
        ]
        for c in remaining_examined:
            p.library.remove(c)
            # Disturbance: any prior public knowledge of these cards'
            # top-of-library positions is now stale.
            reset_public_knowledge(c)
        state.rng.shuffle(remaining_examined)
        p.library.extend(remaining_examined)

    def describe(self) -> str:
        if self.filter_name == "creature" and self.max_power > 0:
            filter_phrase = f"creature card with power {self.max_power} or less"
        else:
            filter_phrase = f"{self.filter_name} card"
        base = (
            f"look at the top {self.n} cards of your library; you may "
            f"reveal a {filter_phrase} from among them and put "
            f"it into your hand; put the rest on the bottom of your "
            f"library in a random order"
        )
        if self.counter_on_miss:
            base += (
                "; if you didn't put a card into your hand this way, "
                "put a +1/+1 counter on ~"
            )
        return base



@dataclass(frozen=True)
class LookTopNPutOneInHandRestToGraveyard(Effect):
    """Look at the top ``n`` cards of your library; put one of them into
    your hand and the rest into your graveyard.

    Parameterized primitive covering both shapes:

    * **Commune with Evil** (``{2}{B}`` Duskmourn Sorcery), ``n=4``,
      ``reveal_chosen=True`` (default): "Look at the top four cards of
      your library. Put one of them into your hand and the rest into
      your graveyard."  The chosen card is revealed to all opponents.
    * **Codecracker Hound** (creature ETB, ``{1}{U}``), ``n=2``,
      ``reveal_chosen=False``: "Look at the top two cards of your
      library. Put one into your hand and the other into your
      graveyard."  The chosen card is NOT revealed.

    The Codecracker Hound surface form was previously its own engine
    class ``LookAtTopTwoSplitHandGraveyard`` (deleted in the
    refined-Cycle-1 fold). That class used a binary
    ``choose_surveil``-on-top-card decision channel; this folded
    primitive uses the canonical ``choose_card_from_library`` id-pick
    instead — semantically equivalent at n=2 (a binary choice between
    two examined cards is observationally the same as picking one of
    two card ids).

    The agent's ``choose_card_from_library`` hook picks which of the
    examined cards goes to hand (the others go to graveyard). If the
    library has fewer than ``n`` cards, all available cards are examined.
    With 0 cards the effect is a no-op.
    """

    n: int = 4
    # When ``True`` (default, matches Commune-with-Evil's parser path),
    # the chosen card is revealed to all opponents post-move via
    # ``reveal_to_all``. Codecracker Hound's printed text does NOT
    # include "Reveal that card" — the n=2 surface form passes
    # ``reveal_chosen=False`` so the post-resolve observable matches the
    # printed text (and matches Rust's no-reveal resolve at
    # ``state_ops_resolve.rs:6366-6394``, where the variant has no
    # reveal step at all).
    reveal_chosen: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all

        p = state.player_by_id(controller_id)
        examined = p.library[:self.n]
        if not examined:
            return
        # Agent picks one card to put into hand; default to the top card
        # when no agent is seated or the agent declines / picks an out-
        # of-pool id.
        chosen_id: int | None = None
        if p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in examined],
            )
        hand_card = next(
            (c for c in examined if c.id == chosen_id), None
        )
        if hand_card is None:
            hand_card = examined[0]
        # Move the chosen card to hand.
        p.library.remove(hand_card)
        hand_card.zone = Zone.HAND
        p.hand.append(hand_card)
        if self.reveal_chosen:
            reveal_to_all(state, hand_card)
        state.log.append(
            f"P{controller_id} puts {hand_card.printing.name} into "
            f"hand (look at top {self.n})"
        )
        # Move the rest of the examined cards to graveyard.
        for c in [c for c in examined if c is not hand_card]:
            p.library.remove(c)
            c.zone = Zone.GRAVEYARD
            p.graveyard.append(c)
            state.log.append(
                f"P{controller_id} puts {c.printing.name} into "
                f"graveyard (look at top {self.n})"
            )

    def describe(self) -> str:
        # Codecracker-Hound n=2 cosmetic tail "the other" matches the
        # printed surface form for n=2; the parameterized "the rest"
        # tail otherwise.
        tail = "the other" if self.n == 2 and not self.reveal_chosen else "the rest"
        return (
            f"look at the top {self.n} cards of your library; put one "
            f"into your hand and {tail} into your graveyard"
        )



@dataclass(frozen=True)
class PeekTopMayBattlefieldIfLandElseHand(Effect):
    """Peek-and-route primitive for the printed surface "Look at the top
    card of your library. If it's a land card, you may put it onto the
    battlefield. If you don't put the card onto the battlefield, put it
    into your hand."

    Resolution flow:

    1. Peek the top card of the controller's library. If the library is
       empty, silent no-op.
    2. If the top card is a land, ask the controller's agent via
       ``choose_yes_no`` whether to put it onto the battlefield. On
       accept, the land enters untapped (no printed "tapped" rider) via
       ``put_into_battlefield`` so landfall and other ETB triggers fire
       cleanly.
    3. Otherwise (non-land top, OR controller declines the optional
       battlefield-put), the card goes to the controller's hand.

    Wickerfolk Thresher's Delirium attack body is the canonical surface
    today. Named generically so future cards reusing the
    "peek-then-may-route-to-battlefield-if-land-else-hand" shape can
    share the primitive — no per-card class needed.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        if not p.library:
            return
        top = p.library[0]
        is_land = top.printing.is_land
        put_on_battlefield = False
        if is_land and p.agent is not None:
            put_on_battlefield = p.agent.choose_yes_no(
                state, controller_id,
                f"Put {top.printing.name} onto the battlefield?",
            )
        # Pop the card now that the decision is made.
        p.library.pop(0)
        if is_land and put_on_battlefield:
            # Route through ``put_into_battlefield`` so landfall (and
            # any other ETB triggers on this entering land) fire via
            # the standard pipeline.
            put_into_battlefield(state, top, controller_id)
            state.log.append(
                f"P{controller_id} puts {top.printing.name} onto the "
                f"battlefield (peek-top)"
            )
        else:
            top.zone = Zone.HAND
            p.hand.append(top)
            state.log.append(
                f"P{controller_id} puts {top.printing.name} into "
                f"hand (peek-top)"
            )

    def describe(self) -> str:
        return (
            "look at the top card of your library; if it's a land "
            "card, you may put it onto the battlefield; if you don't "
            "put the card onto the battlefield, put it into your hand"
        )



@dataclass(frozen=True)
class ManifestDread(Effect):
    """CR 701.55: To manifest dread, look at the top two cards of your
    library, manifest one of them, then put the other into your
    graveyard. To manifest a card (CR 701.34) is to put that card onto
    the battlefield face down. It's a 2/2 creature with no name, no
    subtypes, and no abilities.

    Repeats ``count`` times — for "Manifest dread X times" / "Manifest
    dread twice" surfaces. The agent's ``choose_card_from_library``
    hook picks which of the two examined cards is manifested
    (face-down) on each iteration; the other goes to the graveyard.
    With fewer than two cards left in the library the effect manifests
    one (if available) and skips the graveyard step — the simplest
    least-surprising fallback.
    """

    count: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        n = self.count
        # ``count`` may be ``None`` for the X-times surface — read the
        # X-value off the state when present (Valgavoth's Onslaught).
        if n is None:
            n = state.resolving_x_value
        for _ in range(n):
            p = state.player_by_id(controller_id)
            if not p.library:
                return
            examined = p.library[:2]
            # With fewer than two cards left, manifest the only card
            # available and skip the graveyard step.
            if len(examined) == 1:
                card = examined[0]
                p.library.remove(card)
                card.face_down = True
                put_into_battlefield(state, card, controller_id)
                state.log.append(
                    f"P{controller_id} manifests a face-down card "
                    f"(library exhausted)"
                )
                continue
            # Two cards examined. Ask the agent which to manifest;
            # the other goes to graveyard. Default fallback: manifest
            # the top card.
            options = [c.id for c in examined]
            manifest_id = options[0]
            if p.agent is not None:
                chosen = p.agent.choose_card_from_library(
                    state, p.id, options
                )
                if chosen in options:
                    manifest_id = chosen
            manifest_card = next(c for c in examined if c.id == manifest_id)
            gy_card = next(c for c in examined if c.id != manifest_id)
            p.library.remove(manifest_card)
            manifest_card.face_down = True
            put_into_battlefield(state, manifest_card, controller_id)
            p.library.remove(gy_card)
            gy_card.zone = Zone.GRAVEYARD
            p.graveyard.append(gy_card)
            state.log.append(
                f"P{controller_id} manifests a face-down card; "
                f"{gy_card.printing.name} goes to the graveyard"
            )

    def describe(self) -> str:
        if self.count == 1:
            return "manifest dread"
        if self.count is None:
            return "manifest dread X times"
        if self.count == 2:
            return "manifest dread twice"
        if self.count == 3:
            return "manifest dread three times"
        return f"manifest dread {self.count} times"


@dataclass(frozen=True)
class ManifestDreadThenPutPlusOneCountersOnIt(Effect):
    """Manifest dread, then put ``counter_amount`` +1/+1 counter(s) on
    that creature.

    Matches the door triggers on Slimy Aquarium ("manifest dread, then
    put a +1/+1 counter on that creature" — counter_amount=1) and
    Weight Room ("manifest dread, then put three +1/+1 counters on
    that creature" — counter_amount=3) on Duskmourn Rooms.

    "That creature" refers to the just-manifested face-down 2/2 — the
    other examined card goes to the graveyard per ``ManifestDread``'s
    semantics. To track which permanent receives the counter we
    snapshot the battlefield ids before the manifest and pick the
    single id that wasn't there before. With fewer than two cards
    left in the library ``ManifestDread`` may manifest one or zero
    cards — the counter step then either lands on the lone manifested
    face-down (one new id) or no-ops (no new id).
    """

    counter_amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_plus_one_counters

        pre_ids = {o.id for o in state.battlefield}
        ManifestDread(count=1).resolve(state, controller_id, ())
        new_ids = [o.id for o in state.battlefield if o.id not in pre_ids]
        if not new_ids:
            return
        # Manifest dread enters exactly one face-down creature when it
        # successfully resolves, so pick that single new id.
        manifested_id = new_ids[-1]
        manifested = next(
            (o for o in state.battlefield if o.id == manifested_id), None,
        )
        if manifested is None:
            return
        add_plus_one_counters(state, manifested, self.counter_amount)
        state.log.append(
            f"{manifested.printing.name or 'manifested face-down creature'}"
            f" gets {self.counter_amount} +1/+1 counter(s) "
            f"(now {manifested.plus_one_counters})"
        )

    def describe(self) -> str:
        if self.counter_amount == 1:
            return (
                "manifest dread, then put a +1/+1 counter on that creature"
            )
        return (
            f"manifest dread, then put {self.counter_amount} +1/+1 "
            f"counters on that creature"
        )


@dataclass(frozen=True)
class ManifestDreadThenAttachSelf(Effect):
    """Manifest dread, then attach the source Equipment to that creature.

    Matches the Duskmourn Equipment ETB family — Conductive Machete,
    Killer's Mask, Cursed Windbreaker, Dissection Tools — whose oracle
    text reads ``"When this Equipment enters, manifest dread, then
    attach this Equipment to that creature."``. "That creature" refers
    to the just-manifested face-down 2/2 (the same back-reference
    ``ManifestDreadThenPutPlusOneCountersOnIt`` resolves), so the
    Equipment auto-attaches without a printed-target choice — no
    ``requires_target`` machinery needed.

    Implementation parallels ``ManifestDreadThenPutPlusOneCountersOnIt``:
    snapshot the battlefield ids before manifesting, run
    ``ManifestDread(count=1)``, then identify the new face-down
    permanent by id-set difference. With fewer than two cards left in
    the library, ``ManifestDread`` may manifest one or zero cards —
    the attach step then either lands on the lone manifested face-down
    (one new id) or no-ops (no new id, Equipment stays unattached).
    No-ops if the source isn't an Equipment (defensive — only Equipment
    builds wire this engine class today).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        if "Equipment" not in source.printing.subtypes:
            return
        pre_ids = {o.id for o in state.battlefield}
        ManifestDread(count=1).resolve(state, controller_id, ())
        new_ids = [o.id for o in state.battlefield if o.id not in pre_ids]
        if not new_ids:
            return
        # Manifest dread enters exactly one face-down creature when it
        # successfully resolves; pick that single new id as the attach
        # target.
        manifested_id = new_ids[-1]
        manifested = next(
            (o for o in state.battlefield if o.id == manifested_id), None,
        )
        if manifested is None:
            return
        source.attached_to = manifested.id
        state.log.append(
            f"{source.printing.name} is attached to "
            f"{manifested.printing.name or 'manifested face-down creature'}"
        )

    def describe(self) -> str:
        return (
            "manifest dread, then attach this Equipment to that creature"
        )


@dataclass(frozen=True)
class Surveil(Effect):
    """Surveil N: look at the top N cards of your library; for each one, you
    may put it into your graveyard. The rest stay on top in their original
    order. Decision delegated to the controller's agent (choose_surveil).

    If ``use_artifact_count`` is True the amount is recomputed at
    resolution as the controller's current artifact count — matches
    Cerebral Download's "Surveil X, where X is the number of artifacts
    you control".
    """

    amount: int = 1
    use_artifact_count: bool = False

    def _amount(self, state: "GameState", controller_id: int) -> int:
        from mtgz_ir.game import is_artifact_now

        if not self.use_artifact_count:
            return self.amount
        return sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id and is_artifact_now(state, o)
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        n = self._amount(state, controller_id)
        milled = 0
        # Examine the top N cards in order. Mill ones the agent chooses;
        # keep the rest on top in their original order.
        examined = p.library[: n]
        keep: list = []
        for card in examined:
            if p.agent is not None and p.agent.choose_surveil(state, p.id, card.id):
                p.library.remove(card)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
                milled += 1
            else:
                keep.append(card)
        state.log.append(
            f"P{controller_id} surveils {n} ({milled} milled)"
        )

    def describe(self) -> str:
        if self.use_artifact_count:
            return "surveil X, where X is the number of artifacts you control"
        return f"surveil {self.amount}"


@dataclass(frozen=True)
class Scry(Effect):
    """Scry N (CR 701.18): controller looks at the top ``count`` cards of
    their library and may put any number of them on the bottom of their
    library; the rest stay on top in any order.

    **First-slice deterministic shortcut: keep all on top.** Mirrors the
    Rust ``Effect::ScryN`` resolve arm in
    ``rust/src/state_ops_resolve.rs:1777-1792`` which inlines the same
    keep-all-on-top semantic without invoking the round-4 ``apply_scry``
    primitive. This satisfies "scry happened" semantics without
    requiring a decision callback to choose which cards go to the
    bottom; a future slice that plumbs a real "which to bottom"
    decision surface (sister to ``choose_surveil``, e.g.
    ``choose_scry_to_bottom``) can grow the resolve body without
    breaking the class shape.

    Keep-on-top is mechanically a no-op on the library (no zone moves,
    no order change), so resolution mutates only the log. This matches
    Rust's behavior bit-for-bit on the cross-engine bridge.

    Sister effect to :class:`Surveil` — same look-at-top-N pattern,
    differs in destination zone for the chosen cards (library bottom
    for scry vs graveyard for surveil).

    **Negative ``amount`` raises** loudly (CR 701.18 doesn't define
    Scry -1; surface it as a schema error). Matches Rust's
    ``ScryN count must be non-negative`` raise.

    **count > library size** clamps to library length per CR 701.18d
    ("if there aren't enough cards in your library, look at all the
    cards"). With keep-on-top, the clamp is also a no-op — we just
    don't error.
    """

    amount: int = 1
    # ``bottom_choice=True`` runs the real CR 701.18 decision: each
    # looked-at card goes through the controller's ``choose_surveil``
    # hook (the shared keep-on-top-vs-remove decision; ``True`` = put
    # it on the bottom). Cards kept stay on top in their original
    # order; bottomed cards go to the bottom in their original order
    # (reordering is not offered — ``docs/SEARCH_LIMITATIONS.md``).
    # The default ``False`` keeps the first-slice keep-all-on-top
    # shortcut that the Rust ``Effect::ScryN`` arm mirrors, so the
    # EoE cross-engine bridge stays bit-equal.
    bottom_choice: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if self.amount < 0:
            raise ValueError(
                f"Scry amount must be non-negative, got {self.amount}"
            )
        p = state.player_by_id(controller_id)
        # Clamp to library size (CR 701.18d). With keep-all-on-top,
        # neither path mutates the library — only the log changes.
        n = min(self.amount, len(p.library))
        if self.bottom_choice and n > 0:
            from mtgz_ir.game import reveal_to

            examined = list(p.library[:n])
            bottom = [
                c for c in examined
                if p.agent is not None
                and p.agent.choose_surveil(state, p.id, c.id)
            ]
            for c in bottom:
                p.library.remove(c)
            for c in bottom:
                p.library.append(c)
            for c in examined:
                reveal_to(c, controller_id)
            state.log.append(
                f"P{controller_id} scries {n} ({len(bottom)} to the bottom)"
            )
            return
        state.log.append(f"P{controller_id} scries {n} (kept all on top)")

    def describe(self) -> str:
        return f"scry {self.amount}"


@dataclass(frozen=True)
class SelfMill(Effect):
    """Controller mills ``amount`` cards from the top of their own library.

    Matches oracle text like "mill three cards" on triggered abilities,
    where no target selection is needed.
    """

    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        milled = 0
        for _ in range(self.amount):
            if not p.library:
                break
            card = p.library.pop(0)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            milled += 1
        state.log.append(f"P{controller_id} mills {milled} cards")

    def describe(self) -> str:
        return f"mill {self.amount} cards"
