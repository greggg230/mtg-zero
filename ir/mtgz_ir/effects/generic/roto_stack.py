"""Effects for the Vintage Roto P1 package (stack & restrictions):
counterspell variants and effects that move spells off the stack
without countering them; cast-trigger effects bound to the cast spell
and its caster; "can't cast / attack this turn"; countering a triggered
ability; cumulative upkeep; recruit; Chancellor of the Annex's
opening-hand reveal; the "remove a counter" cost atom.

Spells on the stack are targeted as ``("stack", index)`` (the
convention of :class:`~mtgz_ir.effects.generic.counterspells.
CounterSpell`). Removing a spell from the middle of the stack goes
through :func:`pop_stack_item`, which keeps every other stack item's
``("stack", j)`` targets pointing at the same objects (a target that
pointed at the removed spell becomes ``("stack", -1)``, i.e. illegal).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, ClassVar

from mtgz_ir.effects.vocab import CantCastScope
from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.conditions import Condition
from mtgz_ir.effects.generic.counterspells import CounterSpell
from mtgz_ir.effects.generic.cube_creatures_ub import CounterTargetStackObject
from mtgz_ir.effects.object_predicates import ObjectPredicate
from mtgz_ir.effects.predicates import TriggerPredicate
from mtgz_ir.opening_hand import AFTER_KEEP

if TYPE_CHECKING:
    from mtgz_ir.game import GameState, StackItem


# ---------------------------------------------------------------------
# Stack helpers
# ---------------------------------------------------------------------


def stack_item_of(state: "GameState", obj) -> "StackItem | None":
    """The spell ``StackItem`` whose card is ``obj`` (``None`` off the
    stack)."""
    for item in state.stack:
        if item.ability is None and item.source is obj:
            return item
    return None


def stack_mana_value(state: "GameState", obj) -> int:
    """CR 202.3e: a spell's mana value on the stack counts X as the
    chosen value; elsewhere X is 0."""
    total = obj.printing.mana_cost.total
    item = stack_item_of(state, obj)
    if item is not None:
        total += obj.printing.mana_cost.x_pips * item.x_value
    return total


def pop_stack_item(state: "GameState", idx: int) -> "StackItem":
    """Remove ``state.stack[idx]`` and re-point every other item's stack
    targets (CR 608.2b: a target that left the stack is illegal --
    ``("stack", -1)``)."""
    item = state.stack.pop(idx)

    def remap(t):
        kind, j = t
        if kind != "stack":
            return t
        if j == idx:
            return ("stack", -1)
        if j > idx:
            return ("stack", j - 1)
        return t

    for other in state.stack:
        if any(k == "stack" and j >= idx for k, j in other.targets):
            other.targets = tuple(remap(t) for t in other.targets)
    return item


def _spell_items(state: "GameState"):
    for i, item in enumerate(state.stack):
        if item.ability is None:
            yield i, item


# ---------------------------------------------------------------------
# Predicates over spells
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class SpellManaValueIs(ObjectPredicate):
    """"with mana value ``n``" for a spell (X counted on the stack)."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return stack_mana_value(state, obj) == self.n


@dataclass(frozen=True)
class IsColorless(ObjectPredicate):
    """The object is colorless (no effective colors)."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_colors

        return not effective_colors(state, obj)


@dataclass(frozen=True)
class TargetsSpellMatching(Condition):
    """"if it targets a <spell np>" (Mystical Dispute's "a blue spell"):
    one of the spell's chosen ``("stack", i)`` targets is a spell
    matching ``filter``. Without chosen targets (the legal-action
    affordability probe) it holds when any spell on the stack matches,
    like :class:`~mtgz_ir.effects.conditions.TargetMatches`; the cast
    pipeline re-checks with the committed targets."""

    filter: ObjectPredicate

    def matches(self, state, viewer_id, chosen_targets=None):
        if chosen_targets is None:
            return any(
                self.filter.test(item.source, state)
                for _, item in _spell_items(state)
            )
        for kind, idx in chosen_targets:
            if kind != "stack" or not (0 <= idx < len(state.stack)):
                continue
            item = state.stack[idx]
            if item.ability is None and self.filter.test(item.source, state):
                return True
        return False


@dataclass(frozen=True)
class OpponentCastSpellsThisTurn(Condition):
    """"an opponent cast ``n`` or more spells this turn" (Mindbreak
    Trap's alternative-cost gate)."""

    n: int

    def matches(self, state, viewer_id, chosen_targets=None):
        return any(
            p.spells_cast_this_turn >= self.n
            for p in state.players if p.id != viewer_id
        )

    def describe(self) -> str:
        return f"an opponent cast {self.n} or more spells this turn"


def count_controlled(state: "GameState", controller_id: int, predicate) -> int:
    return sum(
        1 for o in state.battlefield
        if o.controller_id == controller_id and predicate.test(o, state)
    )


# ---------------------------------------------------------------------
# Counterspells
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class CounterSpellMVAtMostCount(CounterSpell):
    """"Counter target spell with mana value X or less, where X is the
    number of <permanents> you control." (Spellstutter Sprite). X is
    counted for the ability's controller when targets are chosen and
    again on resolution (CR 608.2b)."""

    count_predicate: "ObjectPredicate | None" = None

    def _mv_ok(self, state, item, controller_id) -> bool:
        x = count_controlled(state, controller_id, self.count_predicate)
        return stack_mana_value(state, item.source) <= x

    def legal_targets(self, state, controller_id):
        return [
            t for t in super().legal_targets(state, controller_id)
            if self._mv_ok(state, state.stack[t[1]], controller_id)
        ]

    def resolve(self, state, controller_id, targets=()):
        if targets:
            kind, idx = targets[0]
            if kind == "stack" and 0 <= idx < len(state.stack):
                item = state.stack[idx]
                if item.ability is None and not self._mv_ok(
                    state, item, controller_id,
                ):
                    state.log.append(
                        f"counterspell fizzles: {item.source.printing.name} "
                        f"is no longer a legal target"
                    )
                    return
        super().resolve(state, controller_id, targets)

    def describe(self) -> str:
        return super().describe() + (
            " with mana value X or less (X = matching permanents you control)"
        )


@dataclass(frozen=True)
class CounterSpellThenControllerEffect(Effect):
    """"<Counter target ... spell>. Its controller <creates a token |
    draws a card>." (Swan Song, Strix Serenade, Glen Elendra Guardian).

    ``rider`` runs for the targeted spell's controller once the counter
    step happened -- as long as the target was still legal on
    resolution (the spell is still on the stack and still matches), even
    if it couldn't be countered or its controller paid to keep it."""

    counter: CounterSpell
    rider: Effect

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return self.counter.legal_targets(state, controller_id)

    def resolve(self, state, controller_id, targets=()):
        if not targets:
            return
        kind, idx = targets[0]
        if kind != "stack" or not (0 <= idx < len(state.stack)):
            return
        item = state.stack[idx]
        if item.ability is not None or item.countered:
            return
        if not self.counter._matches(state, item):
            state.log.append(
                f"counterspell fizzles: {item.source.printing.name} is no "
                f"longer a legal target"
            )
            return
        spell_controller = item.controller_id
        self.counter.resolve(state, controller_id, targets)
        self.rider.resolve(state, spell_controller, ())

    def describe(self) -> str:
        return f"{self.counter.describe()}; its controller: {self.rider.describe()}"


@dataclass(frozen=True)
class CounterTargetStackObjectFiltered(CounterTargetStackObject):
    """"Counter target triggered ability or colorless spell." (Consign
    to Memory) -- the U/B bucket's stack-object counter with a filter on
    the spell kind. A countered ability is removed from the stack
    (``_resolve_top_of_stack_item``)."""

    spell_filter: "ObjectPredicate | None" = None

    def legal_targets(self, state, controller_id):
        out = []
        for t in super().legal_targets(state, controller_id):
            item = state.stack[t[1]]
            if (
                item.ability is None and self.spell_filter is not None
                and not self.spell_filter.test(item.source, state)
            ):
                continue
            out.append(t)
        return out

    def describe(self) -> str:
        return "counter target " + " or ".join(
            "matching spell" if k == "spell" else f"{k} ability"
            for k in self.kinds
        )


# ---------------------------------------------------------------------
# Moving spells off the stack without countering them
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class ReturnTargetSpellOrPermanentToHand(Effect):
    """"Return target spell or permanent to its owner's hand." (Venser,
    Shaper Savant). A spell leaves the stack without being countered (an
    uncounterable spell can be returned); a spell copy ceases to exist;
    a flashback spell is exiled (``move_spell_off_stack``)."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        from mtgz_ir.effects.value_predicates import IsPermanent

        out: list[Target] = [
            ("stack", i) for i, item in _spell_items(state)
        ]
        out.extend(self._enumerate_for_filter(state, controller_id, IsPermanent()))
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
            if state.stack[tid].ability is not None:
                return
            item = pop_stack_item(state, tid)
            move_spell_off_stack(state, item.source, destination="hand")
            return
        obj = next((o for o in state.battlefield if o.id == tid), None)
        if obj is None:
            return
        move_off_battlefield(state, obj, destination="hand")

    def describe(self) -> str:
        return "return target spell or permanent to its owner's hand"


@dataclass(frozen=True)
class ExileTargetSpells(Effect):
    """"Exile any number of target spells." (Mindbreak Trap) -- each
    still-legal target leaves the stack for its owner's exile (a copy
    ceases to exist). Exiling isn't countering, so an uncounterable
    spell is exiled too."""

    count_min: int = 0
    count_max: int = 64

    def requires_target(self) -> bool:
        return True

    def target_count_range(self):
        return (self.count_min, self.count_max)

    def legal_targets(self, state, controller_id):
        return [("stack", i) for i, _ in _spell_items(state)]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import move_spell_off_stack

        items = []
        for kind, idx in targets:
            if kind != "stack" or not (0 <= idx < len(state.stack)):
                continue
            item = state.stack[idx]
            if item.ability is None and item not in items:
                items.append(item)
        for item in items:
            idx = next(
                (i for i, it in enumerate(state.stack) if it is item), None,
            )
            if idx is None:  # pragma: no cover - defensive
                continue
            pop_stack_item(state, idx)
            move_spell_off_stack(state, item.source, destination="exile")

    def describe(self) -> str:
        return "exile any number of target spells"


@dataclass(frozen=True)
class ExileTargetSpellPlotted(Effect):
    """"Exile target spell. It becomes plotted." (Aven Interrupter): the
    spell leaves the stack for its owner's exile (not countered -- an
    uncounterable spell is exiled too) and becomes plotted, so its owner
    may cast it as a sorcery on a later turn without paying its mana cost
    (``GameObject.plotted_turn``). A copy ceases to exist."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(self, state, controller_id):
        return [("stack", i) for i, _ in _spell_items(state)]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.actions import move_spell_off_stack
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, idx = targets[0]
        if kind != "stack" or not (0 <= idx < len(state.stack)):
            return
        if state.stack[idx].ability is not None:
            return
        item = pop_stack_item(state, idx)
        card = item.source
        move_spell_off_stack(state, card, destination="exile")
        if card.zone == Zone.EXILE and not card.is_spell_copy:
            card.plotted_turn = state.turn_number
            state.log.append(f"{card.printing.name} becomes plotted")

    def describe(self) -> str:
        return "exile target spell; it becomes plotted"


# ---------------------------------------------------------------------
# Cast triggers: predicates over the cast event
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class SpellManaValueAtMost(ObjectPredicate):
    """"a spell with mana value ``n`` or less" (X counted on the stack)."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return stack_mana_value(state, obj) <= self.n


@dataclass(frozen=True)
class NoManaSpentToCast(ObjectPredicate):
    """"if no mana was spent to cast it" -- read off the spell
    (``GameObject.mana_spent_to_cast``, set by the cast pipeline)."""

    def test(self, obj, state):
        return hasattr(obj, "printing") and obj.mana_spent_to_cast == 0


@dataclass(frozen=True)
class SpellManaValueEqualsSourceCounters(TriggerPredicate):
    """"a spell with mana value equal to the number of <kind> counters
    on this <permanent>" (Chalice of the Void)."""

    kind: str = "charge"

    def matches(self, state, subjects, source):
        from mtgz_ir.game import counter_count

        spell = subjects.get("spell")
        if spell is None or source is None:
            return False
        return stack_mana_value(state, spell) == counter_count(source, self.kind)


@dataclass(frozen=True)
class CastersNoncreatureSpellsThisTurnIs(TriggerPredicate):
    """"their first noncreature spell each turn": the casting player's
    noncreature-spell count (bumped before the cast event fires) is
    ``n``."""

    n: int = 1

    def matches(self, state, subjects, source):
        caster = subjects.get("controller")
        return (
            caster is not None
            and caster.noncreature_spells_cast_this_turn == self.n
        )


# ---------------------------------------------------------------------
# Cast triggers: effects bound to the cast event
# ---------------------------------------------------------------------


class _CastEventBound(Effect):
    """Binds "that spell" (the cast event's ``spell`` subject, through
    ``event_subject_id``) and "that player" (its ``controller``, handed
    to the stack item as the fixed target ``("player", id)``)."""

    def event_subject_id(self, subjects) -> int:
        spell = subjects.get("spell")
        return getattr(spell, "id", -1) if spell is not None else -1

    def targets_from_event(self, state, subjects):
        caster = subjects.get("controller")
        return (("player", caster.id),) if caster is not None else ()

    @staticmethod
    def _that_player(targets) -> "int | None":
        for kind, pid in targets:
            if kind == "player":
                return pid
        return None

    @staticmethod
    def _that_spell_item(state):
        sid = state.resolving_event_subject_id
        for item in state.stack:
            if item.ability is None and item.source.id == sid:
                return item
        return None


def _pay_mana_or_decline(state, payer_id: int, cost, label: str) -> bool:
    """``payer_id`` may pay ``cost``; True when they do (their
    ``choose_yes_no``; an agent-less player pays whenever able)."""
    from mtgz_ir.actions import auto_tap_for_cost, can_pay_cost

    if not can_pay_cost(state, payer_id, cost):
        return False
    payer = state.player_by_id(payer_id)
    if payer.agent is not None and not payer.agent.choose_yes_no(
        state, payer_id, f"pay {label}",
    ):
        return False
    auto_tap_for_cost(state, payer_id, cost)
    state.log.append(f"P{payer_id} pays {label}")
    return True


def _pay_or_decline(state, payer_id: int, cost_str: str, label: str) -> bool:
    from mtgz_ir.cards import parse_mana_cost

    return _pay_mana_or_decline(
        state, payer_id, parse_mana_cost(cost_str), f"{cost_str} {label}",
    )


@dataclass(frozen=True)
class CounterEventSpell(_CastEventBound):
    """"counter that spell" / "counter it unless that player pays {N}"
    (Chalice of the Void, Vexing Bauble, Nether Void, Chancellor of the
    Annex). Not a target; a spell that already left the stack or can't
    be countered is unaffected."""

    unless_pays: str = ""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.stack_control import CounterTargetSpell

        item = self._that_spell_item(state)
        if item is None or item.countered:
            return
        name = item.source.printing.name
        if CounterTargetSpell._is_uncounterable(state, item):
            state.log.append(f"{name} can't be countered")
            return
        if self.unless_pays and _pay_or_decline(
            state, item.controller_id, self.unless_pays,
            f"so {name} isn't countered",
        ):
            return
        item.countered = True
        state.log.append(f"{name} is countered")

    def describe(self) -> str:
        text = "counter that spell"
        if self.unless_pays:
            text += f" unless that player pays {self.unless_pays}"
        return text


@dataclass(frozen=True)
class SourceDealsDamageToCaster(_CastEventBound):
    """"this creature deals N damage to that player" (Eidolon of the
    Great Revel)."""

    amount: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.base import _apply_damage

        pid = self._that_player(targets)
        if pid is not None:
            _apply_damage(state, ("player", pid), self.amount)

    def describe(self) -> str:
        return f"deal {self.amount} damage to that player"


def _source_power(state) -> int:
    from mtgz_ir.game import effective_power

    src = state.resolving_source
    if src is None:
        return 0
    if src in state.battlefield:
        return max(0, effective_power(state, src))
    return max(0, (src.printing.power or 0) + src.plus_one_counters)


@dataclass(frozen=True)
class DrawUnlessCasterPays(_CastEventBound):
    """"[you may ]draw a card unless that player pays {N}" (Mystic
    Remora) / "... pays {X}, where X is this creature's power" (Esper
    Sentinel). That player decides first; if they don't pay, the
    ability's controller draws (or may draw, when ``optional``)."""

    cost: str = ""
    cost_is_source_power: bool = False
    optional: bool = False

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.turn import draw_n_cards

        pid = self._that_player(targets)
        if self.cost_is_source_power:
            cost = "{" + str(_source_power(state)) + "}"
        else:
            cost = self.cost
        if pid is not None and _pay_or_decline(
            state, pid, cost, "so the opponent doesn't draw",
        ):
            return
        me = state.player_by_id(controller_id)
        if self.optional and me.agent is not None and not me.agent.choose_yes_no(
            state, controller_id, "draw a card",
        ):
            return
        draw_n_cards(state, controller_id, 1)

    def describe(self) -> str:
        if self.cost_is_source_power:
            cost = "{X} (this creature's power)"
        else:
            cost = self.cost
        may = "you may " if self.optional else ""
        return f"{may}draw a card unless that player pays {cost}"


@dataclass(frozen=True)
class GainLifePerSpellYouCastThisTurn(Effect):
    """"you gain 1 life for each spell you've cast this turn"
    (Aetherflux Reservoir)."""

    per: int = 1

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.life import GainLife

        n = state.player_by_id(controller_id).spells_cast_this_turn * self.per
        if n > 0:
            GainLife(amount=n).resolve(state, controller_id, ())

    def describe(self) -> str:
        return f"gain {self.per} life for each spell you've cast this turn"


# ---------------------------------------------------------------------
# Cumulative upkeep (CR 702.24)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class CumulativeUpkeep(Effect):
    """"At the beginning of your upkeep, put an age counter on this
    permanent, then you may pay <cost> for each age counter on it. If
    you don't, sacrifice it." (Mystic Remora's {1})."""

    cost: str = "{1}"

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.cards import parse_mana_cost
        from mtgz_ir.cards.base import ManaCost
        from mtgz_ir.game import named_counter_count, set_named_counters
        from mtgz_ir.turn import sacrifice

        source = state.resolving_source
        if source is None or source not in state.battlefield:
            return
        age = named_counter_count(source, "age") + 1
        set_named_counters(source, "age", age)
        unit = parse_mana_cost(self.cost)
        total = ManaCost(
            generic=unit.generic * age,
            colored=tuple((c, n * age) for c, n in unit.colored),
        )
        if _pay_mana_or_decline(
            state, controller_id, total,
            f"cumulative upkeep {self.cost} x{age}",
        ):
            return
        state.log.append(
            f"P{controller_id} doesn't pay cumulative upkeep; "
            f"{source.printing.name} is sacrificed"
        )
        sacrifice(state, source)

    def describe(self) -> str:
        return f"cumulative upkeep {self.cost}"


# ---------------------------------------------------------------------
# "... can't cast spells / can't attack this turn"
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class CantCastSpellsThisTurn(Effect):
    """"Your opponents can't cast [noncreature] spells this turn."
    (Silence, Ranger-Captain of Eos) / "Target player can't cast spells
    this turn." (Orim's Chant): a cleanup-expiring emblem with a
    ``CastProhibition``."""

    who: CantCastScope = CantCastScope.OPPONENTS
    spell_filter: "ObjectPredicate | None" = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "who", CantCastScope(self.who))

    def requires_target(self) -> bool:
        return self.who == "target_player"

    def legal_targets(self, state, controller_id):
        if self.who != "target_player":
            return []
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.roto_restrictions import (
            CastProhibition,
            create_this_turn_emblem,
        )

        if self.who == "target_player":
            if not targets or targets[0][0] != "player":
                return
            static = CastProhibition(
                who="player", player_id=targets[0][1],
                spell_filter=self.spell_filter,
            )
            who = f"P{targets[0][1]}"
        else:
            static = CastProhibition(who="opponents", spell_filter=self.spell_filter)
            who = f"P{controller_id}'s opponents"
        create_this_turn_emblem(state, controller_id, (static,), "can't cast")
        state.log.append(f"{who} can't cast {self._noun()} this turn")

    def _noun(self) -> str:
        return "spells" if self.spell_filter is None else "those spells"

    def describe(self) -> str:
        subject = "target player" if self.who == "target_player" else "your opponents"
        return f"{subject} can't cast {self._noun()} this turn"


@dataclass(frozen=True)
class CreaturesCantAttackThisTurn(Effect):
    """"Creatures can't attack this turn." (Orim's Chant, kicked)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.roto_restrictions import (
            AttackProhibition,
            create_this_turn_emblem,
        )

        create_this_turn_emblem(
            state, controller_id, (AttackProhibition(),), "can't attack",
        )
        state.log.append("creatures can't attack this turn")

    def describe(self) -> str:
        return "creatures can't attack this turn"


# ---------------------------------------------------------------------
# Countering a triggered ability (Strict Proctor)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class CounterTriggerUnlessPays(Effect):
    """"counter that ability unless its controller pays <cost>" -- the
    triggered ability at the fixed ``("stack", i)`` (not a target)."""

    unless_pays: str = ""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects import ActivatedAbility

        if not targets:
            return
        kind, idx = targets[0]
        if kind != "stack" or not (0 <= idx < len(state.stack)):
            return
        item = state.stack[idx]
        if (
            item.ability is None or isinstance(item.ability, ActivatedAbility)
            or item.countered
        ):
            return
        name = item.source.printing.name
        if self.unless_pays and _pay_or_decline(
            state, item.controller_id, self.unless_pays,
            f"so {name}'s ability isn't countered",
        ):
            return
        item.countered = True
        state.log.append(f"{name}'s triggered ability is countered")

    def describe(self) -> str:
        text = "counter that ability"
        if self.unless_pays:
            text += f" unless its controller pays {self.unless_pays}"
        return text


# ---------------------------------------------------------------------
# Recruit (The Queen of Dale)
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class Recruit(Effect):
    """"you recruit" -- "Draw a card, then discard a card. If you
    discarded a nonland card, create a 1/1 white Human Soldier creature
    token." (the keyword action's reminder text)."""

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.effects.generic.tokens import CreateToken
        from mtgz_ir.turn import draw_n_cards

        draw_n_cards(state, controller_id, 1)
        p = state.player_by_id(controller_id)
        if not p.hand:
            return
        chosen = p.hand[0]
        if p.agent is not None:
            pick = p.agent.choose_discard(state, controller_id, [c.id for c in p.hand])
            chosen = next((c for c in p.hand if c.id == pick), p.hand[0])
        nonland = not chosen.printing.is_land
        discard_card(state, p, chosen)
        if nonland and chosen in p.graveyard:
            CreateToken(token_printing=_human_soldier_token()).resolve(
                state, controller_id, (),
            )

    def describe(self) -> str:
        return "recruit"


def _human_soldier_token():
    from mtgz_ir.cards.base import make_token_creature

    return make_token_creature(
        name="Human Soldier", power=1, toughness=1,
        subtypes=("Human", "Soldier"), colors=("W",),
    )


# ---------------------------------------------------------------------
# Chancellor of the Annex's opening-hand reveal
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class RevealForFirstSpellTax:
    """"You may reveal this card from your opening hand. If you do, when
    each opponent casts their first spell of the game, counter that
    spell unless that player pays {N}." (CR 103.6; Chancellor of the
    Annex). The reveal leaves a command-zone emblem holding the delayed
    trigger -- an opponent's first spell of the game happens once, so
    it fires once per opponent."""

    unless_pays: str = "{1}"

    # Offered after the final keep (the opening-hand action protocol P6
    # added for Serum Powder's mulligan-window action).
    timing: ClassVar[str] = AFTER_KEEP

    def available_to(self, player_id: int, starting_player_id: int) -> bool:
        return True

    def describe(self, name: str) -> str:
        return f"reveal {name} from your opening hand"

    def apply(self, state, player, card) -> None:
        from mtgz_ir.cards.base import make_emblem
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.predicates import OPPONENT, Controlled, Event
        from mtgz_ir.game import Zone, reveal_to_all

        reveal_to_all(state, card)
        trigger = TriggeredAbility(
            event=Event.IS_CAST,
            predicate=(
                Controlled(OPPONENT, subject="controller")
                & CastersFirstSpellOfTheGame()
            ),
            effect=CounterEventSpell(unless_pays=self.unless_pays),
        )
        printing = make_emblem(
            name=f"{card.printing.name} (revealed)",
            triggered_abilities=(trigger,),
        )
        holder = state.new_object(printing, owner_id=player.id, zone=Zone.COMMAND)
        holder.etb_timestamp = state.next_timestamp
        state.next_timestamp += 1
        state.emblems.append(holder)
        state.log.append(
            f"P{player.id} reveals {card.printing.name} from their opening hand"
        )


@dataclass(frozen=True)
class CastersFirstSpellOfTheGame(TriggerPredicate):
    """The spell being cast is its caster's first spell of the game (the
    count is bumped before the cast event fires)."""

    def matches(self, state, subjects, source):
        caster = subjects.get("controller")
        return caster is not None and caster.spells_cast_this_game == 1


# ---------------------------------------------------------------------
# Cost atoms
# ---------------------------------------------------------------------


def _counter_kinds_on(obj) -> list[str]:
    from mtgz_ir.game import _DEDICATED_COUNTER_FIELDS

    kinds = [k for k, attr in _DEDICATED_COUNTER_FIELDS.items() if getattr(obj, attr)]
    kinds.extend(k for k, n in obj.named_counters if n > 0)
    return kinds


@dataclass(frozen=True)
class RemoveAnyCounterFromSelfCost:
    """"Remove a counter from this <permanent>" -- a counter of any kind
    (Glen Elendra Guardian). With several kinds on the source the
    controller picks which (``choose_mode``)."""

    def payable(self, state, player_id: int, source) -> bool:
        return source is not None and bool(_counter_kinds_on(source))

    def pay(self, state, player_id: int, source) -> list:
        from mtgz_ir.game import remove_counters

        kinds = _counter_kinds_on(source)
        kind = kinds[0]
        agent = state.player_by_id(player_id).agent
        if len(kinds) > 1 and agent is not None:
            pick = agent.choose_mode(
                state, player_id, [f"remove a {k} counter" for k in kinds],
            )
            if isinstance(pick, int) and 0 <= pick < len(kinds):
                kind = kinds[pick]
        remove_counters(source, kind, 1)
        state.log.append(
            f"P{player_id} removes a {kind} counter from "
            f"{source.printing.name} (cost)"
        )
        return []

    def describe(self) -> str:
        return "Remove a counter from ~"


__all__ = [
    "ExileTargetSpellPlotted",
    "CastersFirstSpellOfTheGame",
    "CounterTriggerUnlessPays",
    "Recruit",
    "RevealForFirstSpellTax",
    "CantCastSpellsThisTurn",
    "CreaturesCantAttackThisTurn",
    "CastersNoncreatureSpellsThisTurnIs",
    "CounterEventSpell",
    "CumulativeUpkeep",
    "DrawUnlessCasterPays",
    "GainLifePerSpellYouCastThisTurn",
    "NoManaSpentToCast",
    "SourceDealsDamageToCaster",
    "SpellManaValueAtMost",
    "SpellManaValueEqualsSourceCounters",
    "CounterSpellMVAtMostCount",
    "CounterSpellThenControllerEffect",
    "CounterTargetStackObjectFiltered",
    "ExileTargetSpells",
    "IsColorless",
    "OpponentCastSpellsThisTurn",
    "RemoveAnyCounterFromSelfCost",
    "ReturnTargetSpellOrPermanentToHand",
    "SpellManaValueIs",
    "TargetsSpellMatching",
    "count_controlled",
    "pop_stack_item",
    "stack_item_of",
    "stack_mana_value",
]
