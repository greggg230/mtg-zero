"""Generic effects introduced for the PowerLSV cube artifact bucket.

Card-agnostic primitives (mana production variants, self-untap,
self-damage riders, coin flips, ...) that the oracle-text parser maps
printed artifact clauses onto. Each class names the printed shape it
models in its docstring; none of them is keyed to a single card name.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import DelayedStep
from mtgz_ir.effects.base import Effect, Target, _apply_damage

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


_WUBRG: tuple[str, ...] = ("W", "U", "B", "R", "G")


def _source_name(state: "GameState") -> str:
    source = state.resolving_source
    return source.printing.name if source is not None else "source"


@dataclass(frozen=True)
class SourceDealsDamageToController(Effect):
    """"This <permanent> deals N damage to you." — the resolving source
    deals ``amount`` damage to its controller.

    Rides the Talisman mana abilities (``{T}: Add {U} or {B}. This
    artifact deals 1 damage to you.``), Mana Crypt's coin-flip loss
    branch and Mana Vault's draw-step upkeep tax. Damage to a player is
    routed through the shared ``_apply_damage`` helper so it is logged
    and handled exactly like every other noncombat damage to a player.
    """

    amount: int

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if self.amount <= 0:
            return
        state.log.append(
            f"{_source_name(state)} deals {self.amount} damage to "
            f"P{controller_id}"
        )
        _apply_damage(state, ("player", controller_id), self.amount)

    def describe(self) -> str:
        return f"this permanent deals {self.amount} damage to you"


@dataclass(frozen=True)
class UntapSelf(Effect):
    """"Untap this artifact." — untap the resolving source permanent.

    Grim Monolith / Basalt Monolith / Retrofitter Foundry
    (``{N}: Untap this artifact.``) and Mana Vault's optional upkeep
    payment. A no-op when the source has left the battlefield or is
    already untapped.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        if not source.tapped:
            return
        if source.stun_counters > 0:
            # CR 122.1d: a stunned permanent that would untap loses a
            # stun counter instead (Baloth Prime's "untap this creature").
            source.stun_counters -= 1
            state.log.append(
                f"{source.printing.name} stays tapped (stun counter removed)"
            )
            return
        source.tapped = False
        state.log.append(f"{source.printing.name} untaps")

    def describe(self) -> str:
        return "untap this permanent"


@dataclass(frozen=True)
class AddManaInAnyCombination(Effect):
    """"Add N mana in any combination of <colors>." — the controller
    picks a color from ``options`` independently for each of the N
    coins minted.

    * Relic of Sauron: ``Add two mana in any combination of {U}, {B},
      and/or {R}.`` -> ``amount=2, options=("U", "B", "R")``.
    * Wizard's Rockets: ``Add X mana in any combination of colors.`` ->
      ``amount_is_x=True`` (N is the X paid for the ability, read from
      ``state.resolving_x_value``), ``options=WUBRG``.

    Each coin's color is asked of ``Agent.choose_mana_color``; without
    an agent the first option is used.
    """

    amount: int = 0
    options: tuple[str, ...] = _WUBRG
    amount_is_x: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        n = state.resolving_x_value if self.amount_is_x else self.amount
        if n <= 0:
            return
        p = state.player_by_id(controller_id)
        opts = list(self.options)
        minted: list[str] = []
        for _ in range(n):
            chosen = opts[0]
            if p.agent is not None:
                picked = p.agent.choose_mana_color(state, controller_id, opts)
                if picked in opts:
                    chosen = picked
            p.mana_pool.append(ManaCoin(color=chosen))
            minted.append(chosen)
        state.log.append(
            f"P{controller_id} adds {'+'.join(minted)} to mana pool"
        )

    def describe(self) -> str:
        amount = "X" if self.amount_is_x else str(self.amount)
        return (
            f"add {amount} mana in any combination of "
            f"{'/'.join(self.options)}"
        )


@dataclass(frozen=True)
class FlipCoinIfLost(Effect):
    """"Flip a coin. If you lose the flip, <effect>." (Mana Crypt).

    The controller calls the flip; the engine draws the result from
    ``state.rng`` so it is reproducible from the game seed and forks
    cleanly with search clones. ``on_loss`` resolves only when the flip
    is lost (probability 1/2).
    """

    on_loss: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        won = state.rng.random() < 0.5
        state.log.append(
            f"P{controller_id} {'wins' if won else 'loses'} the coin flip"
        )
        if not won:
            self.on_loss.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"flip a coin; if you lose the flip, {self.on_loss.describe()}"


def _choose_color(state: "GameState", controller_id: int, options) -> str:
    opts = sorted(options)
    p = state.player_by_id(controller_id)
    if p.agent is not None:
        picked = p.agent.choose_mana_color(state, controller_id, opts)
        if picked in opts:
            return picked
    return opts[0]


@dataclass(frozen=True)
class AddOneManaOfColorAmongControlled(Effect):
    """"Add one mana of any color among <group> you control." — the
    color must be one of the (effective) colors of a permanent in the
    group; with no colored permanent in the group, no mana is added.

    Mox Amber: ``group`` = legendary creatures and planeswalkers
    (``legendary=True, card_types=("Creature", "Planeswalker")``).
    """

    card_types: tuple[str, ...] = ()
    legendary: bool = False
    group_description: str = ""

    def _matches(self, state, obj) -> bool:
        if self.legendary and not obj.printing.is_legendary:
            return False
        if self.card_types:
            from mtgz_ir.game import effective_card_types

            if not set(effective_card_types(state, obj)).intersection(
                self.card_types,
            ):
                return False
        return True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin, effective_colors

        colors: set[str] = set()
        for o in state.battlefield:
            if o.controller_id == controller_id and self._matches(state, o):
                colors |= set(effective_colors(state, o))
        if not colors:
            state.log.append(
                f"{_source_name(state)}: no color among "
                f"{self.group_description or 'that group'}; no mana added"
            )
            return
        color = _choose_color(state, controller_id, colors)
        state.player_by_id(controller_id).mana_pool.append(ManaCoin(color=color))
        state.log.append(f"P{controller_id} adds {color} to mana pool")

    def describe(self) -> str:
        return (
            f"add one mana of any color among {self.group_description} "
            f"you control"
        )


@dataclass(frozen=True)
class ImprintCardFromHand(Effect):
    """"You may exile a <filter> card from your hand." as an Imprint
    ETB (Chrome Mox: nonartifact, nonland). The exiled card is linked to
    the source via ``GameObject.imprinted_on`` so the source's other
    abilities (``AddOneManaOfImprintedColors``) can read it.

    The controller picks via ``Agent.choose_card_from_hand``; ``None``
    declines ("you may").
    """

    excluded_types: tuple[str, ...] = ()
    # Any other descriptor ("an instant card with mana value 2 or less",
    # Isochron Scepter): a ``CardFilter`` the exiled card must match.
    card_filter: "object | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if source is None:
            return
        p = state.player_by_id(controller_id)
        options = [
            c.id for c in p.hand
            if not any(t in c.printing.card_types for t in self.excluded_types)
            and (self.card_filter is None or self.card_filter.matches_card(state, c))
        ]
        if not options or p.agent is None:
            return
        picked = p.agent.choose_card_from_hand(state, controller_id, options)
        if picked not in options:
            state.log.append(f"P{controller_id} exiles nothing")
            return
        card = next(c for c in p.hand if c.id == picked)
        p.hand.remove(card)
        card.zone = Zone.EXILE
        card.imprinted_on = (source.id, source.etb_timestamp)
        p.exile.append(card)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} "
            f"(imprinted on {source.printing.name})"
        )

    def describe(self) -> str:
        if self.card_filter is not None:
            return (
                f"you may exile a {self.card_filter.describe()} from your hand"
            )
        kinds = ", ".join(f"non{t.lower()}" for t in self.excluded_types)
        return f"you may exile a {kinds} card from your hand"


def imprinted_cards(state: "GameState", source) -> list:
    """Cards currently exiled with (imprinted on) ``source``."""
    key = (source.id, source.etb_timestamp)
    return [
        c for p in state.players for c in p.exile
        if c.imprinted_on == key
    ]


@dataclass(frozen=True)
class AddOneManaOfImprintedColors(Effect):
    """"Add one mana of any of the exiled card's colors." (Chrome Mox).
    With no imprinted card, or a colorless one, no mana is added."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        source = state.resolving_source
        if source is None:
            return
        from mtgz_ir.game import effective_colors

        colors: set[str] = set()
        for card in imprinted_cards(state, source):
            colors |= set(effective_colors(state, card))
        if not colors:
            state.log.append(
                f"{source.printing.name}: no colored imprinted card; "
                f"no mana added"
            )
            return
        color = _choose_color(state, controller_id, colors)
        state.player_by_id(controller_id).mana_pool.append(ManaCoin(color=color))
        state.log.append(f"P{controller_id} adds {color} to mana pool")

    def describe(self) -> str:
        return "add one mana of any of the exiled card's colors"


@dataclass(frozen=True)
class UntapXTargets(Effect):
    """"Untap X target <permanents>." (Candelabra of Tawnos: lands).

    The number of targets equals the X paid for the ability:
    ``target_count_is_x`` tells ``actions._activate_ability`` to cap
    target selection at the chosen X (the selection runs after the {X}
    mana is paid). Every chosen target is re-checked against the filter
    at resolution and untapped.
    """

    targets: tuple = ()
    target_count_is_x: bool = True

    def target_count_range(self) -> tuple[int, int]:
        return (0, 10 ** 6)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets or not self.targets:
            return
        spec = self.targets[0]
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for _, target_id in targets:
                obj = next(
                    (o for o in state.battlefield if o.id == target_id), None,
                )
                if obj is None or not spec.filter.test(obj, state):
                    continue
                if obj.tapped:
                    obj.tapped = False
                    state.log.append(f"{obj.printing.name} is untapped")
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return "untap X target permanents"


@dataclass(frozen=True)
class ScheduleDelayedEffect(Effect):
    """"<effect> at the beginning of the next turn's upkeep" (the
    Baubles' delayed draw) / "At the beginning of the next end step,
    <effect>".

    Resolving this schedules ``inner`` as a delayed triggered ability on
    ``state.delayed_triggers`` sourced from the resolving source (which
    may already be in the graveyard — the Baubles sacrifice themselves
    as a cost). ``step`` is ``"upkeep"`` (fires at the next turn's
    upkeep, whoever's turn it is), ``"your_upkeep"`` (fires at the
    beginning of the scheduling controller's next upkeep: "at the beginning
    of your next upkeep", Pact of Negation; CR 603.7) or ``"end"`` (fires at
    the next end step, whoever's turn it is).
    """

    inner: Effect
    step: DelayedStep = DelayedStep.UPKEEP

    def __post_init__(self) -> None:
        object.__setattr__(self, "step", DelayedStep(self.step))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import Step

        source = state.resolving_source
        if source is None:
            return
        if self.step == "upkeep":
            entry = {
                "step": "upkeep",
                "fire_at_turn": state.turn_number + 1,
                "controller_id": controller_id,
                "source_id": source.id,
                "effect": self.inner,
            }
        elif self.step == "your_upkeep":
            # The controller's next upkeep after this one (an upkeep in
            # progress has begun already): ``own_upkeep`` waits for a turn
            # of theirs from the next turn on.
            entry = {
                "step": "upkeep",
                "own_upkeep": True,
                "fire_at_turn": state.turn_number + 1,
                "controller_id": controller_id,
                "source_id": source.id,
                "effect": self.inner,
            }
        else:
            past_end = state.step in (Step.END, Step.CLEANUP)
            entry = {
                "fire_at_turn": state.turn_number + (1 if past_end else 0),
                "controller_id": controller_id,
                "source_id": source.id,
                "any_player_end_step": True,
                "effect": self.inner,
            }
        state.delayed_triggers.append(entry)
        state.log.append(
            f"{source.printing.name}: delayed trigger scheduled "
            f"({self.inner.describe()})"
        )

    def describe(self) -> str:
        when = {
            "upkeep": "at the beginning of the next turn's upkeep",
            "your_upkeep": "at the beginning of your next upkeep",
        }.get(self.step, "at the beginning of the next end step")
        return f"{self.inner.describe()} {when}"


def _own_zone(zone) -> None:
    """Detach a copy-on-write zone list (library / graveyard / exile)
    from search-clone siblings before mutating one of its GameObjects
    in place (see ``game._CowList``)."""
    from mtgz_ir.game import _CowList

    if isinstance(zone, _CowList):
        zone.fork()


def _player_target_spec():
    from mtgz_ir.effects.object_predicates import IsPlayer
    from mtgz_ir.effects.targets import TargetSpec
    return (TargetSpec(IsPlayer()),)


def _target_player(state: "GameState", targets):
    if not targets:
        return None
    kind, pid = targets[0]
    if kind != "player":
        return None
    p = state.player_by_id(pid)
    return None if p.has_lost else p


@dataclass(frozen=True)
class LookAtTopCardOfTargetPlayersLibrary(Effect):
    """"Look at the top card of target player's library." (Mishra's
    Bauble). Information only: the top card becomes known to the
    controller (``reveal_to``) — it isn't revealed to anyone else and
    doesn't move."""

    def __post_init__(self) -> None:
        object.__setattr__(self, "targets", _player_target_spec())

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import reveal_to

        p = _target_player(state, targets)
        if p is None or not p.library:
            return
        _own_zone(p.library)
        reveal_to(p.library[0], controller_id)
        state.log.append(
            f"P{controller_id} looks at the top card of P{p.id}'s library"
        )

    def describe(self) -> str:
        return "look at the top card of target player's library"


@dataclass(frozen=True)
class LookAtRandomCardInTargetPlayersHand(Effect):
    """"Look at a card at random in target player's hand." (Urza's
    Bauble). The random card (drawn from ``state.rng``) becomes known
    to the controller."""

    def __post_init__(self) -> None:
        object.__setattr__(self, "targets", _player_target_spec())

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import reveal_to

        p = _target_player(state, targets)
        if p is None or not p.hand:
            return
        card = p.hand[state.rng.randrange(len(p.hand))]
        reveal_to(card, controller_id)
        state.log.append(
            f"P{controller_id} looks at a random card in P{p.id}'s hand"
        )

    def describe(self) -> str:
        return "look at a card at random in target player's hand"


@dataclass(frozen=True)
class PutSourceOnTopOfOwnersLibrary(Effect):
    """"Put this artifact on top of its owner's library." (Sensei's
    Divining Top). Moves the resolving source from the battlefield to
    the top of its owner's library via ``move_off_battlefield``
    (leaves-the-battlefield bookkeeping applies; a token ceases to
    exist). No-op if the source already left the battlefield."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        from mtgz_ir.game import reveal_to_all

        move_off_battlefield(
            state, source, destination="library", library_index=0,
        )
        # It was public on the battlefield; its place on top stays known.
        reveal_to_all(state, source)
        state.log.append(
            f"{source.printing.name} is put on top of P{source.owner_id}'s "
            f"library"
        )

    def describe(self) -> str:
        return "put this permanent on top of its owner's library"


@dataclass(frozen=True)
class ReturnExiledCardsAndDiscardHands(Effect):
    """Delayed half of Memory Jar: "each player discards their hand and
    returns to their hand each card they exiled this way". ``card_ids``
    are the cards exiled by the first half (per-resolution link)."""

    card_ids: tuple[int, ...] = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import fire_opponent_discards_triggers

        wanted = set(self.card_ids)
        for p in state.players:
            if p.has_lost:
                continue
            from mtgz_ir.player_protections import opponent_effect_blocked_for

            discarded = (
                [] if opponent_effect_blocked_for(state, p.id) else list(p.hand)
            )
            for card in discarded:
                p.hand.remove(card)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
                fire_opponent_discards_triggers(state, p.id, card=card)
            back = [c for c in p.exile if c.id in wanted]
            for card in back:
                p.exile.remove(card)
                card.exiled_face_down = False  # left exile (CR 400.7)
                card.zone = Zone.HAND
                p.hand.append(card)
            state.log.append(
                f"P{p.id} discards {len(discarded)} card(s) and returns "
                f"{len(back)} exiled card(s) to hand"
            )

    def describe(self) -> str:
        return (
            "each player discards their hand and returns to their hand each "
            "card they exiled this way"
        )


@dataclass(frozen=True)
class EachPlayerExilesHandDrawsNReturnsLater(Effect):
    """Memory Jar: "Each player exiles all cards from their hand face
    down and draws N cards. At the beginning of the next end step, each
    player discards their hand and returns to their hand each card they
    exiled this way."

    Exiled cards are hidden from everyone but their owner (face down);
    the delayed half is scheduled with the exact exiled card ids."""

    amount: int = 7

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import draw_n_cards

        exiled: list[int] = []
        live = [p for p in state.players if not p.has_lost]
        for p in live:
            for card in list(p.hand):
                p.hand.remove(card)
                card.zone = Zone.EXILE
                card.exiled_face_down = True
                card.publicly_known_to = frozenset({p.id})
                p.exile.append(card)
                exiled.append(card.id)
        for p in live:
            draw_n_cards(state, p.id, self.amount)
        ScheduleDelayedEffect(
            inner=ReturnExiledCardsAndDiscardHands(card_ids=tuple(exiled)),
            step="end",
        ).resolve(state, controller_id, ())

    def describe(self) -> str:
        return (
            f"each player exiles their hand face down and draws "
            f"{self.amount} cards; at the next end step each player discards "
            f"their hand and returns the exiled cards"
        )


def _share_a_color(state, a, b) -> bool:
    from mtgz_ir.game import effective_colors

    return bool(effective_colors(state, a) & effective_colors(state, b))


@dataclass(frozen=True)
class MillTwoRepeatWhileShareColor(Effect):
    """Grindstone: "Target player mills two cards. If two cards that
    share a color were milled this way, repeat this process." Stops
    when fewer than two cards were milled or the pair shares no
    color."""

    def __post_init__(self) -> None:
        object.__setattr__(self, "targets", _player_target_spec())

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = _target_player(state, targets)
        if p is None:
            return
        total = 0
        while True:
            milled = []
            for _ in range(2):
                if not p.library:
                    break
                card = p.library.pop(0)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
                milled.append(card)
            total += len(milled)
            if len(milled) < 2 or not _share_a_color(state, *milled):
                break
        state.log.append(f"P{p.id} mills {total} cards")

    def describe(self) -> str:
        return (
            "target player mills two cards; if two cards that share a color "
            "were milled this way, repeat this process"
        )


@dataclass(frozen=True)
class MayTapOrUntapTarget(Effect):
    """"You may tap or untap target <permanent>." (Sewer-veillance
    Cam). The target is chosen when the ability goes on the stack; on
    resolution the controller chooses tap / untap / neither
    (``choose_mode`` over the three options)."""

    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import tap_permanent

        if not targets or not self.targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            still_legal = _slot_target_still_legal(
                state, self.targets[0], targets[0],
            )
        finally:
            state.resolving_controller_id = prior
        if not still_legal:
            return
        obj = next(
            (o for o in state.battlefield if o.id == targets[0][1]), None,
        )
        if obj is None:
            return
        p = state.player_by_id(controller_id)
        options = ["tap it", "untap it", "do nothing"]
        choice = 0 if p.agent is None else p.agent.choose_mode(
            state, controller_id, options,
        )
        if choice == 0:
            tap_permanent(state, obj)
        elif choice == 1 and obj.tapped:
            obj.tapped = False
            state.log.append(f"{obj.printing.name} is untapped")

    def describe(self) -> str:
        return "you may tap or untap target permanent"


@dataclass(frozen=True)
class EachOpponentSacrificesN(Effect):
    """"Each opponent sacrifices N <type>s of their choice." (Portal to
    Phyrexia: three creatures). Each opponent picks one at a time via
    ``Agent.choose_sacrifice``; with fewer than N legal permanents they
    sacrifice all they have."""

    amount: int
    card_type: str = "Creature"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import has_card_type, is_creature_now
        from mtgz_ir.turn import sacrifice as _sacrifice

        def _legal(o, pid):
            if o.controller_id != pid:
                return False
            if self.card_type == "Creature":
                return is_creature_now(o)
            return has_card_type(state, o, self.card_type)

        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            for _ in range(self.amount):
                legal = [o for o in state.battlefield if _legal(o, p.id)]
                if not legal:
                    break
                chosen = legal[0]
                if p.agent is not None:
                    pick = p.agent.choose_sacrifice(
                        state, p.id, [o.id for o in legal],
                    )
                    chosen = next((o for o in legal if o.id == pick), legal[0])
                state.log.append(f"P{p.id} sacrifices {chosen.printing.name}")
                _sacrifice(state, chosen)

    def describe(self) -> str:
        return (
            f"each opponent sacrifices {self.amount} "
            f"{self.card_type.lower()}s of their choice"
        )


@dataclass(frozen=True)
class ReanimateTargetCreatureCardFromAGraveyard(Effect):
    """"Put target creature card from a graveyard onto the battlefield
    under your control[. It's a <Subtype> in addition to its other
    types]." (Portal to Phyrexia's upkeep trigger: any graveyard, adds
    Phyrexian).

    Target kind ``("graveyard_creature", id)`` over every player's
    graveyard. The added subtype is a layer-4 ``add_subtype`` entry
    sourced by the reanimated permanent itself, so it lasts as long as
    that permanent stays on the battlefield (independent of the source
    of the ability)."""

    add_subtype: str = ""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("graveyard_creature", c.id)
            for p in state.players
            for c in p.graveyard
            if c.printing.is_creature
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "graveyard_creature":
            return
        for p in state.players:
            card = next((c for c in p.graveyard if c.id == target_id), None)
            if card is None or not card.printing.is_creature:
                continue
            p.graveyard.remove(card)
            state.log.append(
                f"P{controller_id} puts {card.printing.name} onto the "
                f"battlefield from P{p.id}'s graveyard"
            )
            put_into_battlefield(state, card, controller_id)
            if self.add_subtype and card in state.battlefield:
                state.add_continuous_effect(
                    source_id=card.id, layer=4, kind="add_subtype",
                    target_id=card.id, params=(self.add_subtype,),
                    description=(
                        f"{card.printing.name} is a {self.add_subtype}"
                    ),
                )
            return

    def describe(self) -> str:
        tail = (
            f"; it's a {self.add_subtype} in addition to its other types"
            if self.add_subtype else ""
        )
        return (
            "put target creature card from a graveyard onto the battlefield "
            "under your control" + tail
        )


@dataclass(frozen=True)
class OpponentGainsControlOfSource(Effect):
    """"An opponent gains control of this artifact." (Wishclaw
    Talisman). The controller picks the opponent when there is more
    than one (``choose_mode`` over the opponents); control changes
    indefinitely (same permanent reassignment as Sliver Overlord's
    steal)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        opponents = [
            p for p in state.players
            if p.id != controller_id and not p.has_lost
        ]
        if not opponents:
            return
        new = opponents[0]
        me = state.player_by_id(controller_id)
        if len(opponents) > 1 and me.agent is not None:
            idx = me.agent.choose_mode(
                state, controller_id, [f"P{p.id}" for p in opponents],
            )
            if 0 <= idx < len(opponents):
                new = opponents[idx]
        source.controller_id = new.id
        state.log.append(
            f"P{new.id} gains control of {source.printing.name}"
        )

    def describe(self) -> str:
        return "an opponent gains control of this permanent"


@dataclass(frozen=True)
class CreateTokenCopyOfTargetTokenYouControl(Effect):
    """"Create a token that's a copy of target token you control."
    (Esika's Chariot's attack trigger). Any token you control — creature
    or not (a Treasure copies fine) — is a legal target; the copy is
    created through ``CreateToken`` so token doublers / ETBs apply."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_creature_now

        return [
            ("creature" if is_creature_now(o) else "permanent", o.id)
            for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_token
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.tokens import CreateToken

        if not targets:
            return
        _, tid = targets[0]
        target = next((o for o in state.battlefield if o.id == tid), None)
        if (
            target is None
            or not target.printing.is_token
            or target.controller_id != controller_id
        ):
            return
        CreateToken(token_printing=target.printing, count=1).resolve(
            state, controller_id, (),
        )

    def describe(self) -> str:
        return "create a token that's a copy of target token you control"


@dataclass(frozen=True)
class IfSourceHasNoCounters(Effect):
    """"Then if there are no <kind> counters on this <permanent>,
    <effect>." (Reckoner Bankbuster). Checked at resolution against the
    resolving source; a source that left the battlefield has no
    counters on it (the condition holds)."""

    kind: str
    inner: Effect

    def requires_target(self) -> bool:
        return self.inner.requires_target()

    def legal_targets(self, state: "GameState", controller_id: int):
        return self.inner.legal_targets(state, controller_id)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import counter_count

        source = state.resolving_source
        if (
            source is not None
            and source in state.battlefield
            and counter_count(source, self.kind) > 0
        ):
            return
        self.inner.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"if there are no {self.kind} counters on this permanent, "
            f"{self.inner.describe()}"
        )


@dataclass(frozen=True)
class GainProtectionFromEverythingUntilYourNextTurn(Effect):
    """"You gain protection from everything until your next turn." (The
    One Ring). Sets ``Player.protected_from_everything``: damage to the
    player is prevented and they can't be targeted, until the start of
    their next turn (cleared in ``turn.run_untap_step``)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.player_by_id(controller_id).protected_from_everything = True
        state.log.append(
            f"P{controller_id} gains protection from everything until "
            f"their next turn"
        )

    def describe(self) -> str:
        return "you gain protection from everything until your next turn"


@dataclass(frozen=True)
class PutNamedCountersOnSelf(Effect):
    """"Put N <kind> counters on this <permanent>." for counter kinds
    without a dedicated primitive (burden, story, influence, …)."""

    kind: str
    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import add_counters, counter_count

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        add_counters(state, source, self.kind, self.amount)
        state.log.append(
            f"{source.printing.name} gets {self.amount} {self.kind} "
            f"counter(s) (now {counter_count(source, self.kind)})"
        )

    def describe(self) -> str:
        return f"put {self.amount} {self.kind} counter(s) on this permanent"


def _source_counter_count(state: "GameState", kind: str) -> int:
    from mtgz_ir.game import counter_count

    source = state.resolving_source
    if source is None:
        return 0
    return counter_count(source, kind)


@dataclass(frozen=True)
class DrawCardsPerCounterOnSource(Effect):
    """"Draw a card for each <kind> counter on this <permanent>." (The
    One Ring's burden counters). Counted at resolution."""

    kind: str

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        n = _source_counter_count(state, self.kind)
        if n > 0:
            draw_n_cards(state, controller_id, n)

    def describe(self) -> str:
        return f"draw a card for each {self.kind} counter on this permanent"


@dataclass(frozen=True)
class LoseLifePerCounterOnSource(Effect):
    """"You lose N life for each <kind> counter on this <permanent>."
    (The One Ring's upkeep burden)."""

    kind: str
    per: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        loss = self.per * _source_counter_count(state, self.kind)
        if loss <= 0:
            return
        p = state.player_by_id(controller_id)
        p.life -= loss
        state.log.append(f"P{controller_id} loses {loss} life (life: {p.life})")

    def describe(self) -> str:
        return (
            f"you lose {self.per} life for each {self.kind} counter on this "
            f"permanent"
        )


@dataclass(frozen=True)
class ThatPlayerDrawsAndGainsControlOfSource(Effect):
    """Coveted Jewel: "that player draws N cards and gains control of
    this artifact. Untap it." "That player" is the event's player,
    delivered as the fixed target ``("player", id)`` by the
    ATTACKERS_UNBLOCKED dispatch. Control changes indefinitely; the
    artifact is untapped under its new controller."""

    amount: int = 3

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards

        p = _target_player(state, targets)
        if p is None:
            return
        draw_n_cards(state, p.id, self.amount)
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        source.controller_id = p.id
        source.tapped = False
        state.log.append(
            f"P{p.id} gains control of {source.printing.name} and untaps it"
        )

    def describe(self) -> str:
        return (
            f"that player draws {self.amount} cards and gains control of "
            f"this artifact; untap it"
        )


@dataclass(frozen=True)
class TargetOpponentMayLetYouDrawElseMill(Effect):
    """Palantír of Orthanc: "Target opponent may have you draw a card.
    If that player doesn't, you mill X cards, where X is the number of
    <kind> counters on this permanent, and that player loses life equal
    to the total mana value of those cards."

    The opponent decides via their agent's ``choose_yes_no``."""

    kind: str = "influence"

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import ControlledByOpponent, IsPlayer
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets", (TargetSpec(IsPlayer() & ControlledByOpponent()),),
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import draw_n_cards

        opp = _target_player(state, targets)
        if opp is None or opp.id == controller_id:
            return
        me = state.player_by_id(controller_id)
        lets_draw = True
        if opp.agent is not None:
            lets_draw = opp.agent.choose_yes_no(
                state, opp.id, f"Have P{controller_id} draw a card?",
            )
        if lets_draw:
            draw_n_cards(state, controller_id, 1)
            return
        x = _source_counter_count(state, self.kind)
        milled = []
        for _ in range(x):
            if not me.library:
                break
            card = me.library.pop(0)
            card.zone = Zone.GRAVEYARD
            me.graveyard.append(card)
            milled.append(card)
        total_mv = sum(c.printing.mana_cost.total for c in milled)
        opp.life -= total_mv
        state.log.append(
            f"P{controller_id} mills {len(milled)}; P{opp.id} loses "
            f"{total_mv} life (life: {opp.life})"
        )

    def describe(self) -> str:
        return (
            "target opponent may have you draw a card; if they don't, you "
            f"mill X (X = {self.kind} counters) and they lose life equal to "
            "the milled cards' total mana value"
        )


@dataclass(frozen=True)
class HarnessSelf(Effect):
    """"Harness <this permanent>." (CR 701.64, The Mind Stone): the
    source becomes harnessed, switching on its ∞ abilities."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        source.harnessed = True
        state.log.append(f"{source.printing.name} is harnessed")

    def describe(self) -> str:
        return "harness this permanent"


@dataclass(frozen=True)
class FlickerUpToOneOtherTargetNonlandPermanentYouControl(Effect):
    """"Exile up to one other target nonland permanent you control, then
    return that card to the battlefield under its owner's control."
    (The Mind Stone's ∞ ability). The returned card is a new object
    (fresh ETB, no counters). Tokens exiled this way cease to exist.
    ``other=False`` drops the "other" restriction (Displacer Kitten's
    "exile up to one target nonland permanent you control ...")."""

    other: bool = True

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import (
            ControlledByYou, IsLand, IsPlayer, IsSource,
        )
        from mtgz_ir.effects.targets import TargetSpec
        f = ~IsLand() & ~IsPlayer() & ControlledByYou()
        if self.other:
            f = f & ~IsSource()
        object.__setattr__(self, "targets", (
            TargetSpec(f, count_min=0, count_max=1),
        ))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.targets_helpers import (
            _slot_target_still_legal,
        )
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            legal = _slot_target_still_legal(state, self.targets[0], targets[0])
        finally:
            state.resolving_controller_id = prior
        if not legal:
            return
        obj = next(
            (o for o in state.battlefield if o.id == targets[0][1]), None,
        )
        if obj is None:
            return
        move_off_battlefield(state, obj, destination="exile")
        if obj.printing.is_token:
            return
        owner = state.player_by_id(obj.owner_id)
        if obj in owner.exile:
            owner.exile.remove(obj)
            obj.exiled_face_down = False  # left exile (CR 400.7)
        state.log.append(f"{obj.printing.name} returns to the battlefield")
        put_into_battlefield(state, obj, obj.owner_id)

    def describe(self) -> str:
        other = "other " if self.other else ""
        return (
            f"exile up to one {other}target nonland permanent you control, "
            "then return it to the battlefield under its owner's control"
        )


@dataclass(frozen=True)
class MayExileThatCardFromYourGraveyard(Effect):
    """"You may exile that card from your graveyard." (Currency
    Converter's discard trigger). "That card" is the fixed target
    ``("card_in_graveyard", id)`` supplied by the CARD_DISCARDED event.
    The exiled card is linked to the source ("exiled with this
    artifact") through ``GameObject.imprinted_on``. ``optional=False``
    is the mandatory form ("exile that card from your graveyard." --
    Necropotence)."""

    optional: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if not targets or source is None:
            return
        kind, card_id = targets[0]
        if kind != "card_in_graveyard":
            return
        p = state.player_by_id(controller_id)
        card = next((c for c in p.graveyard if c.id == card_id), None)
        if card is None:
            return
        if self.optional and p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, f"Exile {card.printing.name}?",
        ):
            return
        p.graveyard.remove(card)
        card.zone = Zone.EXILE
        card.imprinted_on = (source.id, source.etb_timestamp)
        p.exile.append(card)
        state.log.append(
            f"P{controller_id} exiles {card.printing.name} with "
            f"{source.printing.name}"
        )

    def describe(self) -> str:
        if not self.optional:
            return "exile that card from your graveyard"
        return "you may exile that card from your graveyard"


@dataclass(frozen=True)
class ReturnExiledWithSourceToGraveyardThenToken(Effect):
    """Currency Converter: "Put a card exiled with this artifact into
    its owner's graveyard. If it's a land card, <land_effect>. If it's a
    nonland card, <nonland_effect>." The controller picks the card
    (``choose_card_from_library`` over the linked exiled cards; ``None``
    takes the first)."""

    land_effect: Effect
    nonland_effect: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if source is None:
            return
        cards = imprinted_cards(state, source)
        if not cards:
            return
        card = cards[0]
        me = state.player_by_id(controller_id)
        if me.agent is not None and len(cards) > 1:
            pick = me.agent.choose_card_from_library(
                state, controller_id, [c.id for c in cards],
            )
            card = next((c for c in cards if c.id == pick), cards[0])
        owner = state.player_by_id(card.owner_id)
        owner.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        card.zone = Zone.GRAVEYARD
        card.imprinted_on = None
        owner.graveyard.append(card)
        state.log.append(
            f"{card.printing.name} is put into P{owner.id}'s graveyard"
        )
        follow = self.land_effect if card.printing.is_land else self.nonland_effect
        follow.resolve(state, controller_id, ())

    def describe(self) -> str:
        return (
            "put a card exiled with this artifact into its owner's graveyard; "
            f"if it's a land card, {self.land_effect.describe()}; otherwise "
            f"{self.nonland_effect.describe()}"
        )


@dataclass(frozen=True)
class ReturnThisCardFromGraveyardThenAttachToThatCreature(Effect):
    """Sword of the Meek: "return this card from your graveyard to the
    battlefield, then attach it to that creature." Functions from the
    graveyard (``functions_from_graveyard`` marks the enclosing trigger
    ``functions_in_graveyard``); "that creature" is the fixed target the
    graveyard-trigger dispatch supplies (the entering creature)."""

    functions_from_graveyard = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import is_creature_now

        card = state.resolving_source
        if card is None:
            return
        owner = state.player_by_id(card.owner_id)
        if card not in owner.graveyard:
            return
        owner.graveyard.remove(card)
        state.log.append(
            f"{card.printing.name} returns to the battlefield from "
            f"P{owner.id}'s graveyard"
        )
        put_into_battlefield(state, card, controller_id)
        if not targets:
            return
        host = next(
            (o for o in state.battlefield if o.id == targets[0][1]), None,
        )
        if host is None or not is_creature_now(host):
            return
        card.attached_to = host.id
        state.log.append(
            f"{card.printing.name} is attached to {host.printing.name}"
        )

    def describe(self) -> str:
        return (
            "return this card from your graveyard to the battlefield, then "
            "attach it to that creature"
        )


@dataclass(frozen=True)
class ExileThatCreature(Effect):
    """"Exile that creature." where "that creature" is the event object
    delivered as the fixed target (Kaldra Compleat's granted combat-
    damage trigger). No-op if it already left the battlefield."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        obj = next(
            (o for o in state.battlefield if o.id == targets[0][1]), None,
        )
        if obj is None:
            return
        state.log.append(f"{obj.printing.name} is exiled")
        move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return "exile that creature"


@dataclass(frozen=True)
class CreateTokenThenMayAttachSelf(Effect):
    """"Create <token>. You may attach this Equipment to it." (Cori-Steel
    Cutter's Flurry trigger). The token is created through
    ``CreateToken`` (doublers / ETBs apply); if the Equipment is still on
    the battlefield the controller may attach it (``choose_yes_no``) to
    the token just created (the first new token if a doubler made more).
    """

    create: Effect

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        before = {o.id for o in state.battlefield}
        self.create.resolve(state, controller_id, targets)
        new = [
            o for o in state.battlefield
            if o.id not in before and o.printing.is_token
            and o.controller_id == controller_id
        ]
        source = state.resolving_source
        if not new or source is None or source not in state.battlefield:
            return
        if "Equipment" not in source.printing.subtypes:
            return
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            f"Attach {source.printing.name} to {new[0].printing.name}?",
        ):
            return
        source.attached_to = new[0].id
        state.log.append(
            f"{source.printing.name} is attached to {new[0].printing.name}"
        )

    def describe(self) -> str:
        return f"{self.create.describe()}; you may attach this Equipment to it"
