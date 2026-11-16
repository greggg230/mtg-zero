"""Engine primitives for the PowerLSV cube enchantment bucket
(``feature/cube-enchantments``).

Each class is a generic, card-agnostic primitive; the cards that
motivated it are named in its docstring. Parser wiring lives in
:mod:`mtgz_ir.cards.parser.cube_enchantments`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import CardTypeWord
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.generic.exile import Exile

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------------
# "Sacrifice a <card type>" activation cost
# ---------------------------------------------------------------------------


def permanent_has_type(obj: "GameObject", type_name: str) -> bool:
    """True if ``obj`` currently has the lowercase card type ``type_name``
    (``creature`` reads the animated/layered state)."""
    from mtgz_ir.game import is_creature_now

    if type_name == "creature":
        return is_creature_now(obj)
    return bool(getattr(obj.printing, f"is_{type_name}", False))


def sacrifice_type_candidates(
    state: "GameState", player_id: int, type_name: str,
) -> list:
    """Permanents ``player_id`` controls that can pay a "Sacrifice a
    <type_name>" cost. The ability's source qualifies when it has the
    type (CR 602.2: any permanent of that type you control)."""
    return [
        o for o in state.battlefield
        if o.controller_id == player_id and permanent_has_type(o, type_name)
    ]


# ---------------------------------------------------------------------------
# Squandered Resources
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class AddOneManaSacrificedLandCouldProduce(Effect):
    """"Add one mana of any type the sacrificed land could produce."
    (Squandered Resources). Reads the land paid as the ability's
    "Sacrifice a land" cost from ``state.resolving_cost_paid_objects``
    and mints one coin of a type that land's mana abilities produce
    (colorless included), chosen via ``choose_mana_color``. A land that
    produces no mana adds nothing."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin, effective_land_produces

        lands = [
            o for o in getattr(state, "resolving_cost_paid_objects", ())
            if o.printing.is_land
        ]
        if not lands:
            return
        options = sorted(effective_land_produces(state, lands[0]))
        if not options:
            state.log.append(
                f"{lands[0].printing.name} could produce no mana"
            )
            return
        p = state.player_by_id(controller_id)
        color = options[0]
        if p.agent is not None and len(options) > 1:
            picked = p.agent.choose_mana_color(state, controller_id, options)
            if picked in options:
                color = picked
        p.mana_pool.append(ManaCoin(color=color))
        state.log.append(f"P{controller_id} adds {color} to mana pool")

    def describe(self) -> str:
        return "add one mana of any type the sacrificed land could produce"


# ---------------------------------------------------------------------------
# Fading (Parallax Wave)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RemoveNamedCounterFromSelfOrSacrifice(Effect):
    """Fading's upkeep trigger (CR 702.32a): "remove a <kind> counter from
    it. If you can't, sacrifice it." No-op once the source has left the
    battlefield."""

    kind: str

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import named_counter_count, set_named_counters
        from mtgz_ir.turn import sacrifice

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        have = named_counter_count(source, self.kind)
        if have > 0:
            set_named_counters(source, self.kind, have - 1)
            state.log.append(
                f"remove a {self.kind} counter from {source.printing.name} "
                f"({have - 1} left)"
            )
            return
        state.log.append(
            f"{source.printing.name} has no {self.kind} counters; "
            f"sacrificed"
        )
        sacrifice(state, source)

    def describe(self) -> str:
        return (
            f"remove a {self.kind} counter from this permanent; if you "
            f"can't, sacrifice it"
        )


# ---------------------------------------------------------------------------
# Targeted exile variants (Touch the Spirit Realm, Parallax Wave)
# ---------------------------------------------------------------------------


def _legal_exile_targets(effect, state: "GameState", controller_id: int, targets):
    """The chosen battlefield targets of a new-shape exile effect that
    are still legal on resolution (CR 608.2b); fizzles are logged."""
    from mtgz_ir.effects.generic.targets_helpers import (
        _fizzle_message,
        _slot_target_still_legal,
    )
    from mtgz_ir.effects.targets import TargetSpec

    out = []
    prior = getattr(state, "resolving_controller_id", None)
    try:
        state.resolving_controller_id = controller_id
        for slot, chosen in zip(effect.targets, targets):
            if not isinstance(slot, TargetSpec):  # pragma: no cover - parser builds TargetSpecs only
                continue
            if not _slot_target_still_legal(state, slot, chosen):
                state.log.append(
                    _fizzle_message(state, chosen, action="exile")
                )
                continue
            obj = next(
                (o for o in state.battlefield if o.id == chosen[1]), None,
            )
            if obj is not None:
                out.append(obj)
    finally:
        state.resolving_controller_id = prior
    return out


def _exile_permanent(state: "GameState", obj) -> None:
    from mtgz_ir.turn import move_off_battlefield

    state.log.append(f"{obj.printing.name} is exiled")
    move_off_battlefield(state, obj, destination="exile")


@dataclass(frozen=True)
class ExileTargetUntilSourceLeaves(Exile):
    """"Exile [up to one ]target <permanent> until this <permanent> leaves
    the battlefield." for any target noun phrase (Touch the Spirit
    Realm's "artifact or creature"). Uses the Oblivion-Ring
    ``exiled_by_source`` bookkeeping: the card returns under its owner's
    control when the source leaves (``turn._release_exiled_until_leaves``).
    If the source already left before resolution, nothing is exiled
    (CR 610.3c)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        for obj in _legal_exile_targets(self, state, controller_id, targets):
            obj.exiled_by_source = source.id
            _exile_permanent(state, obj)

    def describe(self) -> str:
        return (
            f"{super().describe()} until this permanent leaves the "
            f"battlefield"
        )


@dataclass(frozen=True)
class ReturnResolvingSourceFromExileUnderOwner(Effect):
    """Delayed half of a flicker: returns the scheduled card (the
    delayed trigger's source) from exile to the battlefield under its
    owner's control, as a new object (CR 400.7: no counters). No-op if
    it is no longer in exile.

    ``counter_on_id``: "If it entered under your control, put a +1/+1
    counter on <that permanent>." (Phelia, Exuberant Shepherd) -- when
    the card returns under this ability's controller's control and that
    permanent is still on the battlefield."""

    counter_on_id: "int | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import add_plus_one_counters
        from mtgz_ir.turn import _reset_left_battlefield_object

        card = state.resolving_source
        if card is None:
            return
        owner = state.player_by_id(card.owner_id)
        if card not in owner.exile:
            return
        owner.exile.remove(card)
        card.exiled_face_down = False  # left exile (CR 400.7)
        state.log.append(
            f"{card.printing.name} returns to the battlefield under "
            f"P{owner.id}'s control"
        )
        _reset_left_battlefield_object(card)
        put_into_battlefield(state, card, owner.id)
        if self.counter_on_id is not None and owner.id == controller_id:
            host = next(
                (o for o in state.battlefield if o.id == self.counter_on_id),
                None,
            )
            if host is not None:
                add_plus_one_counters(state, host, 1)

    def describe(self) -> str:
        return "return it to the battlefield under its owner's control"


@dataclass(frozen=True)
class ExileTargetReturnAtNextEndStep(Exile):
    """"Exile target <permanent>. Return it to the battlefield under its
    owner's control at the beginning of the next end step." (Touch the
    Spirit Realm's Channel ability; Flickerwisp; Phelia, Exuberant
    Shepherd with ``counter_on_source_if_yours``). A token exiled this
    way ceases to exist and never returns."""

    counter_on_source_if_yours: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.delayed_actions import (
            schedule_at_next_end_step,
        )

        source = state.resolving_source
        counter_on_id = (
            source.id
            if self.counter_on_source_if_yours and source is not None else None
        )
        for obj in _legal_exile_targets(self, state, controller_id, targets):
            _exile_permanent(state, obj)
            if obj.printing.is_token:
                continue
            schedule_at_next_end_step(
                state, controller_id, obj.id,
                ReturnResolvingSourceFromExileUnderOwner(
                    counter_on_id=counter_on_id,
                ),
            )

    def describe(self) -> str:
        return (
            f"{super().describe()}; return it to the battlefield under its "
            f"owner's control at the beginning of the next end step"
        )


@dataclass(frozen=True)
class ExileLinkedToSource(Exile):
    """An exile whose cards are "exiled with" the source (CR 607 linked
    abilities): Parallax Wave's "Exile target creature." paired with its
    "cards ... exiled with it" leaves trigger. The link is the imprint
    link (``GameObject.imprinted_on`` = source id + ETB timestamp), which
    dies with the source object and, unlike ``exiled_by_source``, never
    auto-returns the card."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        for obj in _legal_exile_targets(self, state, controller_id, targets):
            _exile_permanent(state, obj)
            if source is not None and not obj.printing.is_token:
                obj.imprinted_on = (source.id, source.etb_timestamp)


@dataclass(frozen=True)
class EachPlayerReturnsCardsExiledWithSource(Effect):
    """"Each player returns to the battlefield all cards they own exiled
    with it." (Parallax Wave's leaves trigger). Every card linked to the
    source through ``imprinted_on`` returns under its owner's control."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.generic.artifact_mechanics import imprinted_cards

        source = state.resolving_source
        if source is None:
            return
        for card in imprinted_cards(state, source):
            owner = state.player_by_id(card.owner_id)
            owner.exile.remove(card)
            card.exiled_face_down = False  # left exile (CR 400.7)
            card.imprinted_on = None
            state.log.append(
                f"{card.printing.name} returns to the battlefield under "
                f"P{owner.id}'s control"
            )
            put_into_battlefield(state, card, owner.id)

    def describe(self) -> str:
        return (
            "each player returns to the battlefield all cards they own "
            "exiled with this permanent"
        )


# ---------------------------------------------------------------------------
# Oath of Druids
# ---------------------------------------------------------------------------


def _creatures_controlled(state: "GameState", player_id: int) -> int:
    from mtgz_ir.game import is_creature_now

    return sum(
        1 for o in state.battlefield
        if o.controller_id == player_id and is_creature_now(o)
    )


@dataclass(frozen=True)
class OathRevealUntilCreature(Effect):
    """Oath of Druids' upkeep trigger: "that player [the upkeep's active
    player] chooses target player who controls more creatures than they
    do and is their opponent. The first player may reveal cards from the
    top of their library until they reveal a creature card. If the first
    player does, that player puts that card onto the battlefield and all
    other cards revealed this way into their graveyard."

    The legal targets are the active player's opponents controlling more
    creatures than the active player; with none, the trigger is removed
    from the stack (CR 603.3d). The target is rechecked on resolution
    (CR 608.2b). The active player decides whether to reveal
    (``choose_yes_no``). If no ``card_type`` card is found the revealed
    cards stay in the library in order ("If the first player does"
    fails, so nothing moves)."""

    card_type: CardTypeWord = CardTypeWord.CREATURE

    def __post_init__(self) -> None:
        object.__setattr__(self, "card_type", CardTypeWord(self.card_type))

    def requires_target(self) -> bool:
        return True

    def _legal(self, state: "GameState") -> list[Target]:
        ap = state.active_player()
        mine = _creatures_controlled(state, ap.id)
        return [
            ("player", p.id) for p in state.players
            if p.id != ap.id and not p.has_lost
            and _creatures_controlled(state, p.id) > mine
        ]

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return self._legal(state)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to_all

        if not targets or targets[0] not in self._legal(state):
            state.log.append("Oath trigger fizzles (illegal target)")
            return
        ap = state.active_player()
        if ap.agent is None or not ap.agent.choose_yes_no(
            state, ap.id,
            f"Reveal cards until you reveal a {self.card_type} card?",
        ):
            return
        revealed = []
        found = None
        for card in ap.library:
            reveal_to_all(state, card)
            if getattr(card.printing, f"is_{self.card_type}", False):
                found = card
                break
            revealed.append(card)
        if found is None:
            state.log.append(
                f"P{ap.id} reveals their library: no {self.card_type} card"
            )
            return
        ap.library.remove(found)
        for card in revealed:
            ap.library.remove(card)
            card.zone = Zone.GRAVEYARD
            ap.graveyard.append(card)
        state.log.append(
            f"P{ap.id} reveals {len(revealed) + 1} card(s); "
            f"{found.printing.name} enters the battlefield"
        )
        put_into_battlefield(state, found, ap.id)

    def describe(self) -> str:
        return (
            f"that player may reveal cards until a {self.card_type} card; "
            f"it enters the battlefield, the rest go to the graveyard"
        )



# ---------------------------------------------------------------------------
# Reanimation Auras (Animate Dead, Necromancy)
# ---------------------------------------------------------------------------

_GRAVEYARD_CREATURE = "graveyard_creature"


def graveyard_creature_card_targets(state: "GameState") -> list[Target]:
    """Every creature card in any graveyard, as a
    ``("graveyard_creature", id)`` target (Animate Dead's "Enchant
    creature card in a graveyard"; Necromancy's "target creature card
    from a graveyard")."""
    return [
        (_GRAVEYARD_CREATURE, c.id)
        for p in state.players
        for c in p.graveyard
        if c.printing.is_creature
    ]


def enchanted_graveyard_card(state: "GameState", aura) -> "GameObject | None":
    """The creature card in a graveyard ``aura`` enchants, or ``None``
    if it no longer is one (it left the graveyard)."""
    for p in state.players:
        for c in p.graveyard:
            if c.id == aura.attached_to:
                return c if c.printing.is_creature else None
    return None


def _take_from_graveyard(state: "GameState", card_id: int):
    for p in state.players:
        for c in p.graveyard:
            if c.id == card_id and c.printing.is_creature:
                p.graveyard.remove(c)
                return c
    return None


@dataclass(frozen=True)
class ReanimateAndAttachSource(Effect):
    """The reanimation-Aura enters trigger, "if it's on the battlefield"
    (CR 603.4 -- checked on resolution):

    * ``from_target=False`` (Animate Dead): "it loses 'enchant creature
      card in a graveyard' and gains 'enchant creature put onto the
      battlefield with this Aura.' Return enchanted creature card to the
      battlefield under your control and attach this Aura to it."
    * ``from_target=True`` (Necromancy): "it becomes an Aura with
      'enchant creature put onto the battlefield with <this>.' Put
      target creature card from a graveyard onto the battlefield under
      your control and attach this enchantment to it."

    The new enchant restriction rides ``GameObject.aura_enchant_override``
    (``"creature"``; the Aura is only ever attached to the creature it
    returned). The returned creature is linked through
    ``linked_creature`` for the leaves trigger. If the card is gone, the
    Aura is left enchanting nothing and state-based actions put it into
    the graveyard."""

    from_target: bool = False

    def requires_target(self) -> bool:
        return self.from_target

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        if not self.from_target:
            return []
        return graveyard_creature_card_targets(state)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        source.linked_creature = None
        source.aura_enchant_override = "creature"
        if self.from_target:
            card = None
            if targets and targets[0][0] == _GRAVEYARD_CREATURE:
                card = _take_from_graveyard(state, targets[0][1])
        else:
            card = (
                _take_from_graveyard(state, source.attached_to)
                if source.attached_to is not None else None
            )
        source.attached_to = None
        if card is None:
            state.log.append(
                f"{source.printing.name} finds no creature card to return"
            )
            return
        state.log.append(
            f"P{controller_id} returns {card.printing.name} to the "
            f"battlefield with {source.printing.name}"
        )
        put_into_battlefield(state, card, controller_id)
        if card in state.battlefield:
            source.attached_to = card.id
            source.linked_creature = (card.id, card.etb_timestamp)

    def describe(self) -> str:
        what = (
            "target creature card from a graveyard" if self.from_target
            else "the enchanted creature card"
        )
        return (
            f"return {what} to the battlefield under your control and "
            f"attach this to it"
        )


@dataclass(frozen=True)
class LinkedCreatureControllerSacrifices(Effect):
    """"When this <Aura> leaves the battlefield, that creature's
    controller sacrifices it." -- the creature this permanent returned
    (``linked_creature``), if it is still the same object on the
    battlefield."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice

        source = state.resolving_source
        link = source.linked_creature if source is not None else None
        if link is None:
            return
        source.linked_creature = None
        creature = next(
            (
                o for o in state.battlefield
                if o.id == link[0] and o.etb_timestamp == link[1]
            ),
            None,
        )
        if creature is None:
            return
        state.log.append(
            f"P{creature.controller_id} sacrifices {creature.printing.name} "
            f"({source.printing.name} left the battlefield)"
        )
        sacrifice(state, creature)

    def describe(self) -> str:
        return "that creature's controller sacrifices it"


# ---------------------------------------------------------------------------
# "At the beginning of the next cleanup step" (Necromancy)
# ---------------------------------------------------------------------------


def schedule_sacrifice_at_next_cleanup(state: "GameState", obj) -> None:
    """Necromancy's rider: "the controller of the permanent it becomes
    sacrifices it at the beginning of the next cleanup step." Scheduled
    on ``state.delayed_triggers`` with ``"step": "cleanup"``; fired by
    :func:`run_delayed_cleanup_triggers`."""
    from mtgz_ir.effects.generic.delayed_actions import SacrificeSourceDelayed

    state.delayed_triggers.append({
        "fire_at_turn": state.turn_number,
        "controller_id": obj.controller_id,
        "source_id": obj.id,
        "effect": SacrificeSourceDelayed(),
        "step": "cleanup",
    })
    state.log.append(
        f"{obj.printing.name} will be sacrificed at the beginning of the "
        f"next cleanup step"
    )


def run_delayed_cleanup_triggers(state: "GameState") -> None:
    """Fire this turn's ``"step": "cleanup"`` delayed triggers at the
    beginning of the cleanup step, then resolve whatever they put on the
    stack (leaves-the-battlefield triggers) and check state-based
    actions. CR 514.3a would give players priority here and run another
    cleanup step; the engine's cleanup has no priority window, so the
    triggered abilities resolve in stack order without responses
    (``docs/SEARCH_LIMITATIONS.md``, cube enchantments)."""
    from mtgz_ir.actions import resolve_top_of_stack
    from mtgz_ir.turn import _find_object_by_id, state_based_actions

    due = [
        e for e in state.delayed_triggers
        if e.get("step") == "cleanup" and e["fire_at_turn"] <= state.turn_number
    ]
    if not due:
        return
    state.delayed_triggers = [
        e for e in state.delayed_triggers
        if not any(e is d for d in due)
    ]
    base = len(state.stack)
    for entry in due:
        source = _find_object_by_id(state, entry["source_id"])
        if source is None:
            continue
        state.resolving_source = source
        try:
            entry["effect"].resolve(state, entry["controller_id"], ())
        finally:
            state.resolving_source = None
    while len(state.stack) > base and not state.is_over():
        resolve_top_of_stack(state)
        state_based_actions(state)
    state_based_actions(state)
