"""Rules-restricting statics for the Vintage Roto P1 package (stack &
restrictions), modeled on :mod:`mtgz_ir.creature_statics_wg`.

The parser (``cards.parser.roto_stack``) puts static descriptors on
``CardPrinting.roto_statics``; this module reads them off permanents on
the battlefield and off command-zone emblems (the "this turn" player
restrictions a resolved spell leaves behind, which expire at cleanup):

* :class:`SpellTax` -- "<Spells> cost {N} more to cast" with the riders
  the existing ``spell_cost_increases`` field can't express: "except
  during its controller's turn" (Defense Grid), "with the chosen name"
  (Disruptor Flute). Added by ``creature_statics_wg.
  spell_cost_increase`` (CR 601.2f increases).
* :class:`MinimumSpellCost` -- "As long as this artifact is untapped,
  each spell that would cost less than three mana to cast costs three
  mana to cast." (Trinisphere), applied after every other cost change
  (:func:`apply_minimum_spell_cost`).
* :class:`CastProhibition` -- can't-cast statics: the chosen name
  (Meddling Mage), player-scoped "this turn" restrictions (Silence,
  Orim's Chant, Ranger-Captain of Eos).
* :class:`ActivationProhibition` -- "Activated abilities of sources with
  the chosen name can't be activated [unless they're mana abilities]."
  (Pithing Needle, Phyrexian Revoker, Disruptor Flute).

``legal.legal_actions`` drops the forbidden ``CastSpell`` /
``ActivateAbility`` proposals through :func:`filter_actions`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING
from mtgz_ir.effects.vocab import CastProhibitionScope

if TYPE_CHECKING:
    from mtgz_ir.cards.base import CardPrinting, ManaCost
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------
# Descriptors
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class SpellTax:
    """A spell-cost increase (CR 601.2f) on every spell, narrowed by:
    ``opponents_only`` -- spells the source's controller's opponents
    cast; ``except_during_casters_turn`` -- not a spell cast during its
    controller's own turn; ``from_graveyard_or_exile`` -- only spells
    cast from a graveyard or from exile; ``named`` -- only spells with
    the source's chosen card name. (Type-filtered taxes use Thalia's
    ``CardPrinting.spell_cost_increases``.)"""

    amount: int
    opponents_only: bool = False
    except_during_casters_turn: bool = False
    from_graveyard_or_exile: bool = False
    named: bool = False


@dataclass(frozen=True)
class MinimumSpellCost:
    """"each spell that would cost less than ``total`` mana to cast costs
    ``total`` mana to cast" -- only while the source is untapped when
    ``while_untapped``."""

    total: int
    while_untapped: bool = False


@dataclass(frozen=True)
class CastProhibition:
    """Spells matching ``spell_filter`` (an ``ObjectPredicate`` over the
    card; ``None`` = every spell) can't be cast by the restricted
    players: ``who`` is ``"each"`` (every player), ``"opponents"`` (the
    source's controller's opponents) or ``"player"`` (``player_id``).
    ``named``: only spells with the source's chosen card name.
    ``mv_above_casters_lands``: only spells whose mana value exceeds the
    number of lands the caster controls (X counts as chosen; see
    :func:`max_x_allowed`)."""

    who: CastProhibitionScope = CastProhibitionScope.EACH
    spell_filter: object = None
    player_id: int = -1
    named: bool = False
    # "with mana value greater than the number of lands that player
    # controls" (Lavinia, Azorius Renegade).
    mv_above_casters_lands: bool = False

    def __post_init__(self) -> None:
        if self.who not in ("each", "opponents", "player"):
            raise ValueError(f"unknown CastProhibition scope: {self.who!r}")
        object.__setattr__(self, "who", CastProhibitionScope(self.who))


@dataclass(frozen=True)
class ActivationProhibition:
    """"Activated abilities of sources with the chosen name can't be
    activated[ unless they're mana abilities]." """

    except_mana_abilities: bool = False


@dataclass(frozen=True)
class AttackProhibition:
    """"Creatures can't attack this turn." (Orim's Chant, kicked) -- on a
    cleanup-expiring emblem."""


@dataclass(frozen=True)
class ChooseCardNameAsEnters:
    """"As this <permanent> enters, choose a [nonland ]card name."
    (Pithing Needle, Phyrexian Revoker, Disruptor Flute, Meddling Mage):
    the name is stored on ``GameObject.chosen_card_name``."""

    nonland: bool = False


# ---------------------------------------------------------------------
# Static sources
# ---------------------------------------------------------------------


def _static_sources(state: "GameState"):
    """``(object, descriptor)`` for every roto static in play: permanents
    on the battlefield and command-zone emblems."""
    for o in state.battlefield:
        if o.printing.roto_statics and not o.phased_out:
            for s in o.printing.roto_statics:
                yield o, s
    for e in state.emblems:
        for s in e.printing.roto_statics:
            yield e, s


def any_roto_statics(state: "GameState") -> bool:
    return any(True for _ in _static_sources(state))


# ---------------------------------------------------------------------
# Spell taxes and the Trinisphere floor
# ---------------------------------------------------------------------


def _casting_from_graveyard_or_exile() -> bool:
    """The spell whose cost is being determined is cast from a graveyard
    or from exile (``creature_casting_ub.casting_origin``)."""
    from mtgz_ir.creature_casting_ub import (
        CASTING_FROM_EXILE,
        CASTING_FROM_GRAVEYARD,
    )

    return CASTING_FROM_GRAVEYARD.get() or CASTING_FROM_EXILE.get()


def roto_spell_tax(
    state: "GameState", printing: "CardPrinting", caster_id: "int | None",
) -> int:
    """Generic mana the roto :class:`SpellTax` statics add to the cost of
    ``caster_id`` casting ``printing``."""
    total = 0
    active_id = state.players[state.active_player_idx].id if state.players else None
    for src, s in _static_sources(state):
        if not isinstance(s, SpellTax):
            continue
        if s.opponents_only and (caster_id is None or caster_id == src.controller_id):
            continue
        if s.except_during_casters_turn and caster_id == active_id:
            continue
        if s.from_graveyard_or_exile and not _casting_from_graveyard_or_exile():
            continue
        if s.named and src.chosen_card_name != printing.name:
            continue
        total += s.amount
    return total


def minimum_spell_cost(state: "GameState") -> int:
    """The largest active "costs at least N mana" floor (0 = none)."""
    floor = 0
    for src, s in _static_sources(state):
        if not isinstance(s, MinimumSpellCost):
            continue
        if s.while_untapped and src.tapped:
            continue
        floor = max(floor, s.total)
    return floor


def apply_minimum_spell_cost(
    state: "GameState", cost: "ManaCost", already_paid: int = 0,
    *, log: bool = False,
) -> "ManaCost":
    """Raise ``cost`` with generic mana so the spell's total cost is at
    least the active floor. ``already_paid`` counts mana-value units of
    the total cost already settled another way (Phyrexian pips paid with
    life). Trinisphere applies after every other cost change (CR
    601.2f), so callers pass the final total cost."""
    floor = minimum_spell_cost(state)
    if not floor:
        return cost
    from mtgz_ir.cards.base import ManaCost

    missing = floor - (cost.total + already_paid)
    if missing <= 0:
        return cost
    if log:
        state.log.append(f"spell's total cost raised to {floor} mana")
    return ManaCost(
        generic=cost.generic + missing, colored=cost.colored,
        x_pips=cost.x_pips, hybrid=cost.hybrid, phyrexian=cost.phyrexian,
    )


# ---------------------------------------------------------------------
# Cast prohibitions
# ---------------------------------------------------------------------


def _restricts(state: "GameState", src, s: CastProhibition, player_id: int) -> bool:
    if s.who == "each":
        return True
    if s.who == "opponents":
        return player_id != src.controller_id
    return player_id == s.player_id


def _lands_controlled(state: "GameState", player_id: int) -> int:
    return sum(
        1 for o in state.battlefield
        if o.controller_id == player_id and o.printing.is_land
    )


def _applicable_prohibitions(state, player_id, card):
    for src, s in _static_sources(state):
        if not isinstance(s, CastProhibition):
            continue
        if not _restricts(state, src, s, player_id):
            continue
        if s.named and src.chosen_card_name != card.printing.name:
            continue
        if s.spell_filter is not None and not s.spell_filter.test(card, state):
            continue
        yield s


def cast_prohibited(
    state: "GameState", player_id: int, card: "GameObject",
) -> bool:
    """Whether a roto can't-cast static forbids ``player_id`` from casting
    ``card`` right now (X counted as 0 -- the least X; a larger X is
    capped by :func:`max_x_allowed`)."""
    for s in _applicable_prohibitions(state, player_id, card):
        if s.mv_above_casters_lands:
            if card.printing.mana_cost.total <= _lands_controlled(state, player_id):
                continue
        return True
    return False


def max_x_allowed(
    state: "GameState", player_id: int, card: "GameObject",
) -> "int | None":
    """The largest X ``player_id`` may choose casting ``card`` under a
    mana-value cap (Lavinia: the spell's mana value with X can't exceed
    the lands they control); ``None`` without one."""
    cost = card.printing.mana_cost
    if not cost.x_pips:
        return None
    cap = None
    for s in _applicable_prohibitions(state, player_id, card):
        if not s.mv_above_casters_lands:
            continue
        room = _lands_controlled(state, player_id) - cost.total
        x = max(0, room // cost.x_pips)
        cap = x if cap is None else min(cap, x)
    return cap


def attack_prohibited(state: "GameState", creature) -> bool:
    """Whether a "creatures can't attack this turn" effect applies."""
    return any(
        isinstance(s, AttackProhibition) for _src, s in _static_sources(state)
    )


def filter_attackers(state: "GameState", attackers: list) -> list:
    """Drop the creatures a roto attack prohibition forbids."""
    if not state.emblems:
        return attackers
    return [o for o in attackers if not attack_prohibited(state, o)]


def create_this_turn_emblem(
    state: "GameState", controller_id: int, statics: tuple, label: str,
) -> None:
    """A command-zone emblem carrying ``statics`` (``roto_statics``
    descriptors) that ends at this turn's cleanup -- "<player> can't
    ... this turn"."""
    from dataclasses import replace

    from mtgz_ir.cards.base import make_emblem
    from mtgz_ir.game import Zone

    printing = replace(make_emblem(name=f"{label} (this turn)"), roto_statics=statics)
    holder = state.new_object(printing, owner_id=controller_id, zone=Zone.COMMAND)
    holder.expires_at_cleanup = True
    holder.etb_timestamp = state.next_timestamp
    state.next_timestamp += 1
    state.emblems.append(holder)


# ---------------------------------------------------------------------
# "As this enters, choose a card name"
# ---------------------------------------------------------------------


def nameable_card_names(
    state: "GameState", controller_id: int, *, nonland: bool,
) -> list[str]:
    """The names offered for "choose a [nonland ]card name": every card
    an opponent owns, in any zone (their deck list -- public in a
    rotisserie draft), sorted. The rules allow any card name; names
    outside the opponents' cards can't affect them (see
    ``docs/SEARCH_LIMITATIONS.md``)."""
    names: set[str] = set()

    def add(card) -> None:
        pr = card.printing
        if pr.is_token or getattr(card, "is_spell_copy", False):
            return
        if nonland and pr.is_land:
            return
        names.add(pr.name)

    for p in state.players:
        if p.id == controller_id:
            continue
        for zone in (p.library, p.hand, p.graveyard, p.exile):
            for c in zone:
                add(c)
        for o in state.battlefield:
            if o.owner_id == p.id:
                add(o)
        for item in state.stack:
            if item.ability is None and item.source.owner_id == p.id:
                add(item.source)
    return sorted(names)


def choose_card_name_as_enters(
    state: "GameState", obj: "GameObject", controller_id: int,
) -> None:
    """Apply ``obj``'s :class:`ChooseCardNameAsEnters` (as it enters the
    battlefield): the controller picks one of :func:`nameable_card_names`
    through ``choose_mode`` (a search decision)."""
    spec = next(
        (s for s in obj.printing.roto_statics if isinstance(s, ChooseCardNameAsEnters)),
        None,
    )
    if spec is None:
        return
    names = nameable_card_names(state, controller_id, nonland=spec.nonland)
    obj.chosen_card_name = None
    if not names:
        state.log.append(f"{obj.printing.name}: no card name to choose")
        return
    idx = 0
    agent = state.player_by_id(controller_id).agent
    if agent is not None and len(names) > 1:
        pick = agent.choose_mode(state, controller_id, names)
        if isinstance(pick, int) and 0 <= pick < len(names):
            idx = pick
    obj.chosen_card_name = names[idx]
    state.log.append(f"{obj.printing.name}: P{controller_id} names {names[idx]}")


# ---------------------------------------------------------------------
# Activation prohibitions
# ---------------------------------------------------------------------


def activation_prohibited(
    state: "GameState", source: "GameObject", ability,
) -> bool:
    """Whether a named activation lock forbids activating ``ability`` of
    ``source`` (a permanent, or a card in a hand or graveyard)."""
    for src, s in _static_sources(state):
        if not isinstance(s, ActivationProhibition):
            continue
        if src.chosen_card_name is None:
            continue
        if src.chosen_card_name != source.printing.name:
            continue
        if s.except_mana_abilities and getattr(ability, "is_mana_ability", False):
            continue
        return True
    return False


def _find_activation_source(state: "GameState", player_id: int, source_id: int):
    """``(source, abilities)`` for an ``ActivateAbility``'s source: a
    permanent, or a card in the activator's hand or graveyard."""
    for o in state.battlefield:
        if o.id == source_id:
            from mtgz_ir.game import effective_activated_abilities

            return o, effective_activated_abilities(state, o)
    p = state.player_by_id(player_id)
    for c in p.hand:
        if c.id == source_id:
            return c, c.printing.hand_activated_abilities
    for c in p.graveyard:
        if c.id == source_id:
            return c, c.printing.graveyard_activated_abilities
    return None, ()


def _find_cast_card(state: "GameState", player_id: int, card_id: int):
    p = state.player_by_id(player_id)
    for zone in (p.hand, p.graveyard, p.library):
        for c in zone:
            if c.id == card_id:
                return c
    for pl in state.players:
        for c in pl.exile:
            if c.id == card_id:
                return c
    return None


def filter_actions(state: "GameState", player_id: int, actions: list) -> list:
    """Drop the ``CastSpell`` / ``ActivateAbility`` / ``ActivateStation``
    proposals a roto prohibition forbids (the plot special action isn't
    a cast and stays legal)."""
    if not any_roto_statics(state):
        return actions
    from mtgz_ir.actions import ActivateAbility, ActivateStation, CastSpell

    out = []
    for a in actions:
        if isinstance(a, CastSpell) and not a.use_plot:
            card = _find_cast_card(state, player_id, a.card_id)
            if card is not None and cast_prohibited(state, player_id, card):
                continue
        elif isinstance(a, ActivateAbility):
            src, abilities = _find_activation_source(state, player_id, a.source_id)
            if (
                src is not None
                and 0 <= a.ability_index < len(abilities)
                and activation_prohibited(state, src, abilities[a.ability_index])
            ):
                continue
        elif isinstance(a, ActivateStation):
            src = next(
                (o for o in state.battlefield if o.id == a.spacecraft_id), None,
            )
            if src is not None and activation_prohibited(state, src, None):
                continue
        out.append(a)
    return out


# ---------------------------------------------------------------------
# Triggers on "a permanent entering causes a triggered ability to trigger"
# ---------------------------------------------------------------------


@dataclass(frozen=True)
class CounterEnteringTriggers:
    """"Whenever a permanent entering causes a triggered ability to
    trigger, counter that ability unless its controller pays <cost>."
    (Strict Proctor) -- a triggered ability of the permanent, fired by
    :func:`fire_entering_trigger_watchers` (``push_triggered_ability``'s
    hook) rather than by event dispatch."""

    unless_pays: str = ""


# Ability index for the synthesized watcher trigger (off the printed
# abilities' once-per-turn ledger, like prowess's 2000).
_ENTERING_TRIGGER_WATCHER_INDEX = 2100


def fire_entering_trigger_watchers(state: "GameState", item_index: int) -> None:
    """The ENTERS-event triggered ability at ``state.stack[item_index]``
    was just put on the stack: each :class:`CounterEnteringTriggers`
    permanent triggers, with the ability as its fixed object."""
    watchers = [
        (o, s) for o in state.battlefield if not o.phased_out
        for s in o.printing.roto_statics
        if isinstance(s, CounterEnteringTriggers)
    ]
    if not watchers:
        return
    from mtgz_ir.actions import push_triggered_ability
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.roto_stack import CounterTriggerUnlessPays
    from mtgz_ir.effects.predicates import Event

    for obj, s in watchers:
        push_triggered_ability(
            state, source=obj,
            ability=TriggeredAbility(
                event=Event.TRIGGERED_ABILITY_TRIGGERS,
                effect=CounterTriggerUnlessPays(unless_pays=s.unless_pays),
            ),
            ability_index=_ENTERING_TRIGGER_WATCHER_INDEX,
            controller_id=obj.controller_id,
            label="triggered-ability",
            fixed_targets=(("stack", item_index),),
        )


# ---------------------------------------------------------------------
# The world rule (CR 704.5k)
# ---------------------------------------------------------------------


def apply_world_rule(state: "GameState") -> bool:
    """State-based action: with two or more world permanents on the
    battlefield, all but the one that has been a world permanent for the
    shortest time (the newest by ``etb_timestamp``) go to their owners'
    graveyards; on a tie, all of them do. Returns True when anything
    moved."""
    worlds = [o for o in state.battlefield if "World" in o.printing.supertypes]
    if len(worlds) < 2:
        return False
    from mtgz_ir.turn import move_off_battlefield

    newest = max(o.etb_timestamp for o in worlds)
    keep = [o for o in worlds if o.etb_timestamp == newest]
    survivors = keep if len(keep) == 1 else []
    for o in worlds:
        if o in survivors:
            continue
        state.log.append(f"{o.printing.name} put into graveyard (world rule)")
        move_off_battlefield(state, o, destination="graveyard")
    return True


__all__ = [
    "ActivationProhibition",
    "AttackProhibition",
    "ChooseCardNameAsEnters",
    "CounterEnteringTriggers",
    "fire_entering_trigger_watchers",
    "attack_prohibited",
    "choose_card_name_as_enters",
    "create_this_turn_emblem",
    "filter_attackers",
    "nameable_card_names",
    "CastProhibition",
    "MinimumSpellCost",
    "SpellTax",
    "activation_prohibited",
    "any_roto_statics",
    "apply_world_rule",
    "apply_minimum_spell_cost",
    "cast_prohibited",
    "filter_actions",
    "max_x_allowed",
    "minimum_spell_cost",
    "roto_spell_tax",
]
