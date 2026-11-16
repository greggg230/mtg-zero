"""Board-rule statics of the Vintage Roto P5 package (board-state prison
& layers): the rule objects a printing carries in
``CardPrinting.board_rules`` and the engine reads that consult them.

A board rule is a static ability that changes how the game is played
rather than a characteristic of an object: who may attack or block,
what untaps during the untap step, ... Every read goes through
:func:`rules_in_play`, which skips a source whose abilities are gone:

* a layer-6 ``"lose_all_abilities"`` registry entry on the source
  (Lignify-style Auras, Oko's Elk);
* a creature source while a "creatures lose all abilities" permanent
  is on the battlefield (``CardPrinting.wipes_all_creature_abilities``);
* a land source whose basic land type is set by Blood Moon-style
  effects (CR 305.7: it loses the abilities from its rules text).

Consumers (one hook each): ``combat.eligible_attackers`` /
``combat.eligible_blockers`` (attack / block restrictions),
``turn.run_untap_step`` (untap limits) and ``actions.dispatch_event``
(triggered abilities granted to creatures).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Iterator

from mtgz_ir.keyword_vocab import KeywordSpelling

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------------
# Source suppression
# ---------------------------------------------------------------------------


def abilities_suppressed(state: "GameState", source: "GameObject") -> bool:
    """Whether ``source`` currently has none of its printed abilities
    (see the module docstring for the three ways)."""
    from mtgz_ir.game import (
        _has_basic_land_type_set_effect,
        _source_layer6_wiped,
        is_creature_now,
    )

    if state.continuous_effects and _source_layer6_wiped(state, source):
        return True
    if is_creature_now(source):
        for o in state.battlefield:
            if o.printing.wipes_all_creature_abilities:
                return True
    if source.printing.is_land and _has_basic_land_type_set_effect(state, source):
        return True
    return False


def lands_without_rules_text(state: "GameState") -> frozenset:
    """Ids of battlefield lands whose basic land type is SET by a Blood
    Moon-style effect (CR 305.7: they lose every ability from their
    rules text). Empty -- after one battlefield scan -- when no such
    effect exists."""
    from mtgz_ir.game import _has_basic_land_type_set_effect

    moon = any(
        o.printing.nonbasic_lands_are_mountains for o in state.battlefield
    ) or any(
        ce.layer == 4 and ce.kind == "set_basic_land_subtype"
        for ce in state.continuous_effects
    )
    if not moon:
        return frozenset()
    return frozenset(
        o.id for o in state.battlefield
        if o.printing.is_land and _has_basic_land_type_set_effect(state, o)
    )


def land_enters_without_rules_text(state: "GameState", card) -> bool:
    """Whether a land about to enter the battlefield will be one of
    :func:`lands_without_rules_text` -- a nonbasic land while Blood Moon
    / Magus of the Moon is out. Its own "enters tapped" / "as this
    enters" replacement effects are then gone before they can apply
    (CR 614.12; the Blood Moon rulings)."""
    if not card.printing.is_land or card.printing.is_basic_land:
        return False
    return any(
        o.printing.nonbasic_lands_are_mountains for o in state.battlefield
    )


def rules_in_play(state: "GameState", kind: type) -> Iterator[tuple]:
    """``(source, rule)`` for every active board rule of type ``kind``
    on the battlefield."""
    for source in state.battlefield:
        rules = source.printing.board_rules
        if not rules:
            continue
        matching = [r for r in rules if isinstance(r, kind)]
        if not matching or abilities_suppressed(state, source):
            continue
        for rule in matching:
            yield source, rule


def any_rules(state: "GameState") -> bool:
    for o in state.battlefield:
        if o.printing.board_rules:
            return True
    return False


# ---------------------------------------------------------------------------
# Combat restrictions
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class WithoutKeyword:
    """"without <keyword>" (Moat: "Creatures without flying")."""

    keyword: KeywordSpelling

    def holds(self, state, source, creature) -> bool:
        from mtgz_ir.game import effective_keywords

        return self.keyword not in effective_keywords(state, creature)

    def describe(self) -> str:
        return f"without {self.keyword}"


@dataclass(frozen=True)
class PowerGreaterThanControllersHandSize:
    """"with power greater than the number of cards in your hand"
    (Ensnaring Bridge; "your" is the source's controller)."""

    def holds(self, state, source, creature) -> bool:
        from mtgz_ir.game import effective_power

        hand = len(state.player_by_id(source.controller_id).hand)
        return effective_power(state, creature) > hand

    def describe(self) -> str:
        return "with power greater than the number of cards in your hand"


@dataclass(frozen=True)
class CantAttack:
    """"Creatures [<condition>] can't attack." (``self_only=False``,
    every creature on the battlefield) / "This creature can't attack."
    (``self_only=True``)."""

    condition: object = None
    self_only: bool = False

    def applies(self, state, source, creature) -> bool:
        if self.self_only and creature.id != source.id:
            return False
        return self.condition is None or self.condition.holds(
            state, source, creature,
        )

    def describe(self) -> str:
        who = "This creature" if self.self_only else "Creatures"
        cond = f" {self.condition.describe()}" if self.condition else ""
        return f"{who}{cond} can't attack."


@dataclass(frozen=True)
class CantBlock:
    """"This creature can't block." (``self_only=True``) / "Creatures
    [<condition>] can't block." """

    condition: object = None
    self_only: bool = False

    def applies(self, state, source, creature) -> bool:
        if self.self_only and creature.id != source.id:
            return False
        return self.condition is None or self.condition.holds(
            state, source, creature,
        )

    def describe(self) -> str:
        who = "This creature" if self.self_only else "Creatures"
        cond = f" {self.condition.describe()}" if self.condition else ""
        return f"{who}{cond} can't block."


def _forbidden(state, creatures: list, kind: type) -> list:
    if not creatures or not any_rules(state):
        return creatures
    active = list(rules_in_play(state, kind))
    if not active:
        return creatures
    return [
        c for c in creatures
        if not any(rule.applies(state, src, c) for src, rule in active)
    ]


def filter_attackers(state: "GameState", creatures: list) -> list:
    """Drop the creatures a "can't attack" board rule forbids."""
    return _forbidden(state, creatures, CantAttack)


def filter_blockers(state: "GameState", creatures: list) -> list:
    """Drop the creatures a "can't block" board rule forbids."""
    return _forbidden(state, creatures, CantBlock)


# ---------------------------------------------------------------------------
# Granted triggered abilities (layer 6)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GrantsTriggerToCreatures:
    """'All creatures have "<triggered ability>".' (The Tabernacle at
    Pendrell Vale): every creature on the battlefield has ``ability``;
    each creature is the ability's source and its controller the
    ability's controller."""

    ability: object

    def describe(self) -> str:
        return f'All creatures have "{self.ability.effect.describe()}".'


def _wiped_after(state, creature, timestamp: int) -> bool:
    """Whether a "loses all abilities" effect with a later timestamp
    than ``timestamp`` applies to ``creature`` (CR 613.7: within layer
    6, a later wipe removes an earlier grant; an earlier wipe doesn't
    remove a later grant)."""
    for ce in state.continuous_effects:
        if (
            ce.layer == 6 and ce.kind == "lose_all_abilities"
            and ce.target_id == creature.id and ce.timestamp > timestamp
        ):
            return True
    for o in state.battlefield:
        if (
            o.printing.wipes_all_creature_abilities
            and o.etb_timestamp > timestamp
        ):
            return True
    return False


def granted_triggers(state: "GameState") -> list[tuple]:
    """``(creature, ability)`` for every triggered ability a board rule
    grants right now."""
    from mtgz_ir.game import is_creature_now

    grants = list(rules_in_play(state, GrantsTriggerToCreatures))
    if not grants:
        return []
    out = []
    for creature in state.battlefield:
        if not is_creature_now(creature):
            continue
        for src, rule in grants:
            if _wiped_after(state, creature, src.etb_timestamp):
                continue
            out.append((creature, rule.ability))
    return out


# Marker in a layer-6 ``grant_triggered_ability`` registry entry's params
# for a P5 grant that ``fire_granted_triggers`` fires ("<this> gains
# <quoted triggered ability>" -- Thief of Existence). Other granted
# triggers keep their own fire paths.
GRANT_MARK = "roto_board"


def _registry_granted(state, observers) -> list[tuple]:
    """``(object, ability)`` for P5-marked registry grants on
    ``observers`` still in effect (CR 613.7 order against
    "loses all abilities": a later wipe removes the grant)."""
    from mtgz_ir.game import effective_triggered_abilities

    out = []
    for ce in state.continuous_effects:
        if (
            ce.kind != "grant_triggered_ability" or len(ce.params) < 2
            or ce.params[1] != GRANT_MARK
        ):
            continue
        obj = next((o for o in observers if o.id == ce.target_id), None)
        if obj is None:
            continue
        ability = ce.params[0]
        if ability not in effective_triggered_abilities(state, obj):
            continue
        if _wiped_after(state, obj, ce.timestamp):
            continue
        out.append((obj, ability))
    return out


def fire_granted_triggers(
    state, event, subjects, label, push, extra_observers=None,
) -> None:
    """``actions.dispatch_event``'s hook: push every triggered ability
    a board rule grants (Tabernacle) or a P5 effect granted an object
    (Thief of Existence; the departing object of a leaves event is among
    ``extra_observers``) that matches ``event`` (``push`` is
    ``push_triggered_ability``)."""
    if state.continuous_effects:
        observers = list(state.battlefield) + [
            o for o in (extra_observers or ()) if o not in state.battlefield
        ]
        for obj, ability in _registry_granted(state, observers):
            if not ability.matches_event(event, subjects, obj, state):
                continue
            push(
                state, source=obj, ability=ability, ability_index=-1,
                controller_id=obj.controller_id, label=label,
            )
    if not any_rules(state):
        return
    for creature, ability in granted_triggers(state):
        if not ability.matches_event(event, subjects, creature, state):
            continue
        bind = getattr(ability.effect, "targets_from_event", None)
        push(
            state, source=creature, ability=ability, ability_index=-1,
            controller_id=creature.controller_id, label=label,
            fixed_targets=bind(state, subjects) if bind is not None else None,
        )


# ---------------------------------------------------------------------------
# Combat events
# ---------------------------------------------------------------------------


def _observed(state, event) -> bool:
    for o in (*state.battlefield, *state.emblems):
        for ab in o.printing.triggered_abilities:
            if ab.event is event:
                return True
    return False


def fire_combat_damage_dealt(state, events) -> None:
    """``Event.DEALS_COMBAT_DAMAGE_ANY`` once per source that dealt
    combat damage in this step (``events`` are ``(source_id, target_id,
    amount)``; amounts summed). Skipped unless something observes it."""
    from mtgz_ir.effects.predicates import Event

    if not events or not _observed(state, Event.DEALS_COMBAT_DAMAGE_ANY):
        return
    from mtgz_ir.actions import dispatch_event

    totals: dict[int, int] = {}
    for source_id, _target, amount in events:
        if amount > 0:
            totals[source_id] = totals.get(source_id, 0) + amount
    for source_id, amount in totals.items():
        source = next((o for o in state.battlefield if o.id == source_id), None)
        if source is None:
            continue
        dispatch_event(
            state, Event.DEALS_COMBAT_DAMAGE_ANY,
            label="deals combat damage", combat_damage_amount=amount,
            source=source,
        )


def fire_becomes_blocked(state, assignments: dict) -> None:
    """``Event.BECOMES_BLOCKED`` once for each attacker that this
    declaration blocked (however many blockers it got)."""
    from mtgz_ir.effects.predicates import Event

    if not assignments or not _observed(state, Event.BECOMES_BLOCKED):
        return
    from mtgz_ir.actions import dispatch_event

    seen: list[int] = []
    for attacker_id in assignments.values():
        if attacker_id in seen:
            continue
        seen.append(attacker_id)
        attacker = next(
            (o for o in state.battlefield if o.id == attacker_id), None,
        )
        if attacker is None or not attacker.blocked_by:
            continue
        dispatch_event(
            state, Event.BECOMES_BLOCKED, label="becomes blocked",
            attacker=attacker,
        )


# ---------------------------------------------------------------------------
# Targeting and casting restrictions
# ---------------------------------------------------------------------------


def untargetable_by(state: "GameState", obj, controller_id: int) -> bool:
    """Whether ``obj`` can't be the target of spells or abilities
    ``controller_id`` controls because of a "can't be the target of
    spells or abilities your opponents control this turn" effect
    (Vines of Vastwood) of one of ``controller_id``'s opponents
    (``keyword_rules.can_be_targeted_by``'s hook)."""
    from mtgz_ir.effects.generic.roto_board import UNTARGETABLE_BY_OPPONENTS_OF

    for ce in state.continuous_effects:
        if (
            ce.kind == UNTARGETABLE_BY_OPPONENTS_OF
            and ce.target_id == obj.id
            and ce.params and ce.params[0] != controller_id
        ):
            return True
    return False


@dataclass(frozen=True)
class CastOnlyBeforeStep:
    """"Cast this spell only before the <step>." (Berserk: the combat
    damage step) -- a rule of the card itself: it can be cast only in a
    step of the turn that precedes ``step``."""

    step: str = "combat_damage"

    def allows(self, state) -> bool:
        from mtgz_ir.turn import STEP_ORDER, Step

        return STEP_ORDER.index(state.step) < STEP_ORDER.index(Step(self.step))

    def describe(self) -> str:
        return f"Cast this spell only before the {self.step.replace('_', ' ')} step."


def filter_cast_timing(state: "GameState", player_id: int, actions: list) -> list:
    """``legal.legal_actions``' hook: drop casts of a card whose own
    ``CastOnlyBeforeStep`` rule forbids casting it now."""
    from mtgz_ir.actions import CastSpell

    p = state.player_by_id(player_id)
    blocked = {
        c.id for zone in (p.hand, p.graveyard, p.exile) for c in zone
        if c.printing.board_rules and any(
            isinstance(r, CastOnlyBeforeStep) and not r.allows(state)
            for r in c.printing.board_rules
        )
    }
    if not blocked:
        return actions
    return [
        a for a in actions
        if not (isinstance(a, CastSpell) and a.card_id in blocked)
    ]


# ---------------------------------------------------------------------------
# State triggers (CR 603.8) and Role uniqueness (CR 704.5y)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class NoCreaturesOnBattlefield:
    """"there are no creatures on the battlefield" (Drop of Honey)."""

    def holds(self, state, source) -> bool:
        from mtgz_ir.game import is_creature_now

        return not any(is_creature_now(o) for o in state.battlefield)

    def describe(self) -> str:
        return "there are no creatures on the battlefield"


@dataclass(frozen=True)
class StateTrigger:
    """"When <condition>, <effect>." -- a state-triggered ability (CR
    603.8): it triggers whenever the condition holds and won't trigger
    again while it is on the stack (checked against the stack itself)."""

    condition: object
    ability: object

    def describe(self) -> str:
        return f"When {self.condition.describe()}, {self.ability.effect.describe()}."


def _pending(state, source, ability) -> bool:
    return any(
        item.source is source and item.ability is ability
        for item in state.stack
    )


def apply_board_state_checks(state: "GameState") -> bool:
    """``turn.state_based_actions``' hook: put each Role but the newest
    a player controls on one permanent into its owner's graveyard (CR
    704.5y), then put state triggers whose condition holds on the stack.
    Returns True when a Role was removed."""
    changed = _apply_role_uniqueness(state)
    _drop_stale_grants(state)
    if not any_rules(state):
        return changed
    from mtgz_ir.actions import push_triggered_ability

    for source, rule in list(rules_in_play(state, StateTrigger)):
        if _pending(state, source, rule.ability):
            continue
        if not rule.condition.holds(state, source):
            continue
        push_triggered_ability(
            state, source=source, ability=rule.ability, ability_index=-1,
            controller_id=source.controller_id, label="state trigger",
        )
    return changed


def _drop_stale_grants(state: "GameState") -> None:
    """A P5 grant to a spell lasts for the permanent that spell becomes
    (CR 400.7: a new object after any other zone change) -- drop it once
    its object is neither on the battlefield nor a spell on the stack
    (a countered Thief of Existence)."""
    if not state.continuous_effects:
        return
    live = {o.id for o in state.battlefield} | {
        item.source.id for item in state.stack
        if item.ability is None and item.source is not None
    }
    kept = [
        ce for ce in state.continuous_effects
        if not (
            ce.kind == "grant_triggered_ability" and len(ce.params) > 1
            and ce.params[1] == GRANT_MARK and ce.target_id not in live
        )
    ]
    if len(kept) != len(state.continuous_effects):
        state.continuous_effects = kept
        state._invalidate_layered_cache()


def _apply_role_uniqueness(state: "GameState") -> bool:
    from mtgz_ir.turn import move_off_battlefield

    roles: dict = {}
    for o in state.battlefield:
        if o.attached_to is None or "Role" not in o.printing.subtypes:
            continue
        roles.setdefault((o.attached_to, o.controller_id), []).append(o)
    changed = False
    for group in roles.values():
        if len(group) < 2:
            continue
        newest = max(group, key=lambda o: o.etb_timestamp)
        for o in group:
            if o is not newest and o in state.battlefield:
                state.log.append(
                    f"{o.printing.name} is put into the graveyard (CR 704.5y)"
                )
                move_off_battlefield(state, o, destination="graveyard")
                changed = True
    return changed


# ---------------------------------------------------------------------------
# Per-turn player facts (Veil of Summer)
# ---------------------------------------------------------------------------


def note_spell_cast(state: "GameState", player, card) -> None:
    """``actions._cast_spell``'s hook: record the cast spell's colors
    for "<player> has cast a <color> spell this turn", and spend a
    pending "the next spell you cast this turn can't be countered"."""
    from mtgz_ir.game import effective_colors

    turn, colors = player.spell_colors_cast_this_turn
    if turn != state.turn_number:
        colors = frozenset()
    player.spell_colors_cast_this_turn = (
        state.turn_number, colors | effective_colors(state, card),
    )
    if player.next_spell_uncounterable_turn == state.turn_number:
        # "The next spell you cast this turn can't be countered."
        card.cast_uncounterable = True
        player.next_spell_uncounterable_turn = -1
        state.log.append(f"{card.printing.name} can't be countered")


def spell_colors_cast_this_turn(state: "GameState", player) -> frozenset:
    turn, colors = player.spell_colors_cast_this_turn
    return colors if turn == state.turn_number else frozenset()


def spells_uncounterable_now(state: "GameState", player_id: int) -> bool:
    """"Spells you control can't be countered this turn." in force for
    ``player_id`` (``CounterTargetSpell._is_uncounterable``'s hook)."""
    return state.player_by_id(player_id).spells_uncounterable_turn == state.turn_number


def player_hexproof_from_source(
    state: "GameState", player_id: int, source, controller_id: int,
) -> bool:
    """Whether player ``player_id`` can't be targeted by
    ``controller_id``'s spell or ability from ``source`` because of
    "you gain hexproof from <quality> until end of turn" (CR 702.11d;
    only opponents' sources are refused)."""
    if source is None or player_id == controller_id:
        return False
    try:
        turn, qualities = state.player_by_id(player_id).hexproof_from_this_turn
    except KeyError:
        return False
    if turn != state.turn_number or not qualities:
        return False
    from mtgz_ir.keyword_rules import source_has_quality

    return any(source_has_quality(state, source, q) for q in qualities)


# ---------------------------------------------------------------------------
# Untap step
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DoesntUntap:
    """"<Permanents> don't untap during their controllers' untap steps."
    (Back to Basics: nonbasic lands)."""

    permanent_filter: object
    label: str = "permanents"

    def describe(self) -> str:
        return (
            f"{self.label.capitalize()} don't untap during their "
            f"controllers' untap steps."
        )


@dataclass(frozen=True)
class UntapLimit:
    """"[As long as this <permanent> is untapped, ]players can't untap
    more than <n> <permanents> during their untap steps." (Winter Orb:
    one land, while Winter Orb is untapped)."""

    permanent_filter: object
    limit: int = 1
    while_source_untapped: bool = False
    label: str = "lands"

    def describe(self) -> str:
        pre = (
            "As long as this permanent is untapped, "
            if self.while_source_untapped else ""
        )
        return (
            f"{pre}players can't untap more than {self.limit} "
            f"{self.label} during their untap steps."
        )


def untap_step_held(
    state: "GameState", player_id: int, already_held: set,
) -> set:
    """Ids of the active player's permanents that stay tapped during
    this untap step because of board rules (``turn.run_untap_step``'s
    hook). The candidates are the player's tapped permanents that would
    otherwise untap -- not stunned, exerted, held by an attached
    "doesn't untap" effect (``already_held``) or by their own "doesn't
    untap" rider. For an untap limit the active player chooses which
    ones untap (CR 502.3) through the ``choose_target`` hook over
    ``("permanent", id)`` options, one pick per allowed untap."""
    if not any_rules(state):
        return set()
    candidates = [
        o for o in state.battlefield
        if o.controller_id == player_id and o.tapped
        and o.stun_counters == 0 and not o.exerted
        and o.id not in already_held
        and not o.printing.doesnt_untap_during_untap_step
    ]
    if not candidates:
        return set()
    held: set = set()
    prior = state.resolving_controller_id
    try:
        state.resolving_controller_id = player_id
        for _src, rule in rules_in_play(state, DoesntUntap):
            for obj in candidates:
                if rule.permanent_filter.test(obj, state):
                    held.add(obj.id)
        limits = [
            rule for src, rule in rules_in_play(state, UntapLimit)
            if not (rule.while_source_untapped and src.tapped)
        ]
        agent = state.player_by_id(player_id).agent
        for rule in limits:
            limited = [
                o for o in candidates
                if o.id not in held and rule.permanent_filter.test(o, state)
            ]
            allowed: list = []
            while limited and len(allowed) < rule.limit:
                pick = limited[0]
                if agent is not None and len(limited) > 1:
                    options = [("permanent", o.id) for o in limited]
                    choice = agent.choose_target(state, player_id, options)
                    pick = next(
                        (o for o in limited if choice == ("permanent", o.id)),
                        limited[0],
                    )
                limited.remove(pick)
                allowed.append(pick)
            held.update(o.id for o in limited)
    finally:
        state.resolving_controller_id = prior
    return held


__all__ = [
    "GRANT_MARK",
    "note_spell_cast",
    "player_hexproof_from_source",
    "spell_colors_cast_this_turn",
    "spells_uncounterable_now",
    "NoCreaturesOnBattlefield",
    "StateTrigger",
    "apply_board_state_checks",
    "CastOnlyBeforeStep",
    "filter_cast_timing",
    "untargetable_by",
    "CantAttack",
    "CantBlock",
    "DoesntUntap",
    "GrantsTriggerToCreatures",
    "PowerGreaterThanControllersHandSize",
    "UntapLimit",
    "WithoutKeyword",
    "abilities_suppressed",
    "any_rules",
    "filter_attackers",
    "filter_blockers",
    "fire_becomes_blocked",
    "fire_combat_damage_dealt",
    "fire_granted_triggers",
    "granted_triggers",
    "land_enters_without_rules_text",
    "lands_without_rules_text",
    "rules_in_play",
    "untap_step_held",
]
