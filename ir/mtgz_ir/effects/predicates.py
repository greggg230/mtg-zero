"""Trigger-combinator predicate layer (top tier of the two-tier
predicate stack).

Triggered abilities use an ``(event, predicate)`` shape:

  ``TriggeredAbility(event=Event.ENTERS, predicate=Subject("obj",
      IsLand()) & Controlled(YOU))``

The event identifies an abstract MTG event (a permanent enters the
battlefield, a permanent dies, a creature attacks, …). The predicate
filters which firings of that event the ability cares about.

Three tiers cooperate:

- :mod:`mtgz_ir.effects.object_predicates` — slot-agnostic boolean
  tests over a single ``GameObject``: ``IsCreature``, ``HasKeyword``,
  ``IsTapped``, etc.
- :mod:`mtgz_ir.effects.state_predicates` — viewer-scoped boolean
  tests over the ``GameState``: ``VoidActive``, ``Delirium``, etc.
- This module — trigger-time combinators that bind those leaves to
  subject slots and to the source object: ``Subject(slot, inner)``
  pulls a named subject and dispatches to an ``ObjectPredicate``;
  ``Source(inner)`` dispatches to the firing source object;
  ``StateBound(inner)`` dispatches to a ``StatePredicate`` with
  ``viewer_id = source.controller_id``. The ``IsSelf`` / ``IsHost`` /
  ``Controlled`` trigger predicates remain at this layer because
  they're inherently source-aware.

Predicates compose via ``&`` / ``|`` / ``~``. They evaluate against a
mapping of named event subjects (each event declares its subject
slots; ``Enters`` and ``Dies`` use ``"obj"``; ``Blocks`` uses
``"blocker"`` and ``"attacker"``; ``DealsCombatDamage`` uses
``"source"`` and ``"target"``).
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum
from typing import TYPE_CHECKING, Any, Mapping

from mtgz_ir.effects.vocab import EventSubject, coerce
from mtgz_ir.effects.object_predicates import ObjectPredicate
from mtgz_ir.effects.state_predicates import StatePredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


class Event(str, Enum):
    """Abstract MTG event names. Each event has a fixed set of
    subject-slot names (see ``EVENT_SUBJECTS``). Engine dispatch
    fires every triggered ability whose ``event`` matches and whose
    predicate evaluates true against the event's subject mapping.
    """

    ENTERS = "enters"
    DIES = "dies"
    LEAVES_BATTLEFIELD = "leaves_battlefield"
    BECOMES_TAPPED = "becomes_tapped"
    ATTACKS = "attacks"
    BLOCKS = "blocks"
    DEALS_COMBAT_DAMAGE = "deals_combat_damage"
    # A permanent has just been dealt damage (combat damage from an
    # attacker/blocker hitting a creature, or non-combat damage from a
    # spell/ability). Subject is the damaged permanent. The amount is
    # threaded through ``dispatch_event``'s ``combat_damage_amount``
    # parameter so resolving effects can read
    # ``state.resolving_combat_damage_amount``. Models the
    # aura/Equipment "Whenever enchanted creature is dealt damage"
    # observer family — the dispatcher fires once per
    # source-target damage application, mirroring the per-event
    # invocation of the legacy
    # ``_fire_enchanted_dealt_damage_triggers`` helper.
    DEALT_DAMAGE = "dealt_damage"
    # A spell has just been cast. Subjects are the spell ``GameObject``
    # being cast (``"spell"``) and the casting Player (``"controller"``).
    # Models the spell-cast observer family — ``Subject("spell",
    # IsArtifact())`` for artifact-spell triggers (Pinnacle Emissary),
    # ``Controlled(YOU, subject="controller")`` to gate "you cast" vs
    # opponent. The dispatcher fires once per cast; the
    # ``second_spell_each_turn`` trigger layers
    # ``StateBound(SpellsCastThisTurn(n=2))`` on top to catch the 2nd
    # spell.
    IS_CAST = "is_cast"
    # A permanent has just been sacrificed. Subject is the sacrificed
    # ``GameObject`` (``"obj"``); the dispatcher fires AFTER the object
    # has been moved to the graveyard and includes the dearly-departed
    # via ``extra_observers`` so a self-trigger ``IsSelf()`` can match
    # though the object has left the battlefield. Predicates layer
    # ``Subject("obj", IsArtifact() | IsCreature())`` on the subject
    # and ``~IsSelf()`` to limit firing to "another" sacrifices.
    IS_SACRIFICED = "is_sacrificed"
    # A player has just played a land (the land-drop action of CR 305.1,
    # NOT effects that put a land onto the battlefield like Burgeoning).
    # Subjects are the land permanent (``"land"``) and the player who
    # played it (``"controller"``). Predicates use ``Controlled(by=
    # OPPONENT, subject="controller")`` for Burgeoning's "an opponent
    # plays a land" gate.
    PLAYS_LAND = "plays_land"
    # A player has just discarded a single card. Subject is the
    # discarding Player (``"player"``). Predicates use ``Controlled(by=
    # OPPONENT, subject="player")`` for "an opponent discards a card"
    # observers (Entropic Battlecruiser). The discarding player's id is
    # also threaded through to the resulting StackItem as a fixed target
    # ``("player", id)`` so the resolving effect can damage them.
    DISCARDS_CARD = "discards_card"
    # One or more +1/+1 counters have just been put on a creature.
    # Subject is the creature receiving the counters (``"obj"``); the
    # effective amount (post-doubling) is threaded via ``counter_amount``
    # so resolving effects can read ``state.resolving_counter_amount``
    # (Terrasymbiosis's "draw that many cards"). Predicate
    # ``Subject("obj", IsCreature()) & Controlled(YOU)`` gates "a
    # creature you control".
    COUNTERS_ADDED = "counters_added"
    # Turn-structure events. Subject is a Player (the player whose
    # phase/step is beginning), not a permanent — so the
    # ``Controlled(YOU)`` predicate matches a player subject by
    # ``player.id == source.controller_id`` rather than the
    # ``obj.controller_id == source.controller_id`` rule used for
    # permanent subjects. ``Always()`` paired with
    # ``BEGINNING_OF_END_STEP`` models the "each end step" variant
    # (Ragost) that fires regardless of whose end step it is.
    BEGINNING_OF_UPKEEP = "beginning_of_upkeep"
    BEGINNING_OF_COMBAT = "beginning_of_combat"
    BEGINNING_OF_END_STEP = "beginning_of_end_step"
    # Beginning of the active player's second (post-combat) main phase.
    # Subject is the active Player. Models the DSK Survival ability-word
    # trigger ("Survival — At the beginning of your second main phase,
    # if this creature is tapped, …") — the Player subject pairs with
    # ``Controlled(YOU)`` for the "your" qualifier exactly like the
    # other turn-structure events. The intervening-if "this creature is
    # tapped" gate is encoded via ``Source(IsTapped())`` AND-ed onto
    # the source-self ``IsSelf()`` arm.
    BEGINNING_OF_POSTCOMBAT_MAIN = "beginning_of_postcombat_main"
    # Beginning of the active player's FIRST (precombat) main phase
    # (Coalition Relic: "At the beginning of your first main phase,
    # ..."). Subject is the active Player. Dispatched by
    # ``turn.run_beginning_of_precombat_main_triggers`` from both turn
    # loops (``simulate.run_step`` and the search adapter).
    BEGINNING_OF_PRECOMBAT_MAIN = "beginning_of_precombat_main"
    # Beginning of the active player's draw step (Mana Vault's "At the
    # beginning of your draw step, if this artifact is tapped, it deals
    # 1 damage to you."). Dispatched by ``turn.run_draw_step`` right
    # after the draw turn-based action (CR 504.1-504.2). Subject is the
    # active Player, like the other turn-structure events.
    BEGINNING_OF_DRAW_STEP = "beginning_of_draw_step"
    # Blockers have been declared and at least one attacking creature is
    # unblocked. Subjects: the attacking Player (``"player"``) and the
    # defending Player (``"defender"``). Fired once per blocker
    # declaration by ``combat.declare_blockers`` (Coveted Jewel: "Whenever
    # one or more creatures an opponent controls attack you and aren't
    # blocked"). The attacking player is also passed as the fixed target
    # ``("player", id)`` so "that player …" effects can address them.
    ATTACKERS_UNBLOCKED = "attackers_unblocked"
    # A player created one or more creature tokens during the stack item
    # that just resolved (batched per resolution by
    # ``actions._flush_creature_token_creations``). Subject: that Player.
    CREATURE_TOKENS_CREATED = "creature_tokens_created"
    # A player discarded a specific card (dispatched alongside
    # DISCARDS_CARD by ``turn.fire_opponent_discards_triggers`` when the
    # card is known). Subjects: the discarding Player (``"player"``) and
    # the discarded card (``"card"``); the card is also the fixed target
    # ``("card_in_graveyard", id)``.
    CARD_DISCARDED = "card_discarded"
    # Combat damage dealt by ``source`` to the creature ``target``
    # (dispatched per pair by ``combat._fire_combat_damage_to_creature_
    # triggers``; the damaged creature is the fixed target). Separate
    # from DEALS_COMBAT_DAMAGE, whose observers assume a player target.
    DEALS_COMBAT_DAMAGE_TO_CREATURE = "deals_combat_damage_to_creature"
    # A creature has dealt combat damage in one combat damage step, to
    # any players and / or permanents -- one event per source per step
    # (CR 510.2; "Whenever equipped creature deals combat damage",
    # Umezawa's Jitte). Subject is the source (``"source"``); the total
    # rides ``combat_damage_amount``. Fired by
    # ``roto_board_statics.fire_combat_damage_dealt``.
    DEALS_COMBAT_DAMAGE_ANY = "deals_combat_damage_any"
    # An attacking creature has become blocked (CR 509.1h; once per
    # declaration however many creatures block it -- "Whenever this
    # creature becomes blocked", Ichorclaw Myr). Subject is the attacker
    # (``"attacker"``). Fired by ``roto_board_statics.fire_becomes_blocked``.
    BECOMES_BLOCKED = "becomes_blocked"
    # A Room has just had its second / final door unlocked, transitioning
    # ``unlocked_halves`` from a single-half set to the full set
    # (``len(unlocked_halves) == len(printing.room_halves)``). Subject is
    # the Room ``GameObject`` (``"room"``).
    ROOM_FULLY_UNLOCKED = "room_fully_unlocked"
    # A permanent has just become monstrous (CR 701.37b). Subject is the
    # permanent (``"obj"``). Alpha Deathclaw's "When this creature enters
    # or becomes monstrous".
    BECOMES_MONSTROUS = "becomes_monstrous"
    # A player or permanent has just become the target of a spell or
    # ability (CR 603.2). Subjects: ``"target"`` (the Player or the
    # permanent) and ``"controller"`` (the Player controlling the spell /
    # ability). Dispatched once per distinct target when a spell is cast,
    # an ability is activated or a triggered ability is put on the stack
    # with targets (Leovold, Emissary of Trest; Nadu, Winged Wisdom).
    BECOMES_TARGET = "becomes_target"
    # The active player has just declared one or more attackers in this
    # combat phase. Subject is the attacking Player (the active player —
    # ``"player"``). Distinct from ``ATTACKS`` (per-attacker, fires once
    # for each declared attacker): ``YOU_ATTACK`` fires exactly ONCE per
    # declare-attackers pass when at least one creature attacked.
    YOU_ATTACK = "you_attack"
    # ────────────────────────────────────────────────────────────────
    # Pre-fact "would-X" events for CR 614 replacement effects.
    # ────────────────────────────────────────────────────────────────
    # The post-fact events above (ENTERS, DIES, …) fire triggered
    # abilities AFTER the event resolves. The WOULD_X events below
    # fire BEFORE the event resolves so a registered
    # ``ReplacementEffect`` can transform the in-flight payload
    # (multiply the count, add instances, replace a destination,
    # prevent entirely). The :func:`mtgz_ir.effects.modifiers
    # .dispatch_replacement` walker collects matching replacements
    # and applies them in CR 616.1 order (the affected player chooses
    # among multiple). Each WOULD_X event has a natural post-fact
    # twin: COUNTERS_ADDED (post-doubling) ↔ WOULD_PLACE_COUNTER
    # (pre-doubling), DIES ↔ WOULD_GO_TO_GRAVEYARD, etc.
    #
    # Subject ``(player,)``; payload carries ``count``. Models the
    # Quantum-Riddler-shape "if you would draw a card while you have
    # no cards in hand, draw two cards instead". The drawing site
    # routes through dispatch_replacement before performing the
    # actual draw; the post-replacement count is consumed by
    # ``draw_n_cards``.
    WOULD_DRAW_CARDS = "would_draw_cards"
    # Subject ``(controller, kind)``; payload carries ``count``. The
    # "controller" subject is the Player creating the tokens; "kind"
    # is the token printing's name. Models token-doubling
    # replacements (Exalted Sunborn ×2, hypothetical Anointed
    # Procession ×3). The token-creation site routes through
    # dispatch_replacement before instantiating the token objects;
    # the post-replacement count drives the loop.
    WOULD_CREATE_TOKEN = "would_create_token"
    # Subject ``(triggering_obj, ability_source, entering_obj)``;
    # payload carries ``instances`` (defaults to 1). ``ability_source``
    # is the object whose triggered ability it is; ``entering_obj`` the
    # permanent whose entering the battlefield caused the ability to
    # trigger (absent when the trigger event is not a permanent
    # entering). Models the Strionic-Resonator-shape "your triggered
    # abilities trigger an additional time" (no subject filters) and
    # Starfield Vocalist's narrower "if a permanent entering the
    # battlefield causes a triggered ability of a permanent you control
    # to trigger" (``ability_source`` and ``entering_obj`` permanents).
    # The triggered-ability-firing path consults
    # dispatch_replacement to decide how many copies of the trigger
    # to push onto the stack.
    TRIGGERED_ABILITY_TRIGGERS = "triggered_ability_triggers"
    # Subject ``(obj, kind)``; payload carries ``count``. ``kind`` is
    # the counter name (``"+1/+1"``, ``"charge"``, …). Models the
    # Loading-Zone-shape counter doublers and the Hardened-Scales
    # +1-to-+1/+1 family. The counter-placement site routes through
    # dispatch_replacement before mutating the counter scalar.
    WOULD_PLACE_COUNTER = "would_place_counter"
    # Subject ``(card,)``; payload carries ``destination`` (default
    # "graveyard"). Models Nexus-of-Fate-shape "instead of going to
    # the graveyard, shuffle into your library". The would-go-to-
    # graveyard site routes through dispatch_replacement; if a
    # ``Replace(new_destination="library")`` transform applies, the
    # card routes there instead. ``WOULD_ENTER`` (self-replacement
    # for entry counters / clones / etc.) is a separate deferred
    # pillar — see project notes.
    WOULD_GO_TO_GRAVEYARD = "would_go_to_graveyard"
    # Subject ``(player,)``; payload starts empty. Fired by
    # ``turn.draw_card`` when a player would draw a card while their
    # library has no cards in it. Models Jace, Wielder of Mysteries'
    # "If you would draw a card while your library has no cards in it,
    # you win the game instead." -- a ``WinTheGameInstead`` transform
    # sets ``payload["win_game"]`` and the draw site declares the win
    # instead of recording the empty-library loss.
    WOULD_DRAW_FROM_EMPTY_LIBRARY = "would_draw_from_empty_library"
    # Subject ``(player,)``. Fired by ``turn.draw_card`` once per card
    # actually drawn (post-replacement). "Whenever you draw a card" --
    # Teferi, Hero of Dominaria's emblem.
    DRAWS_CARD = "draws_card"
    # Subject ``(player,)``; the amount rides ``counter_amount``. Fired by
    # ``game.gain_life`` once per life-gain event (roto P4: Professor
    # Dellian Fel's "Whenever you gain life, target opponent loses that
    # much life." emblem).
    GAINS_LIFE = "gains_life"
    # Subject ``(player,)``; payload starts empty. Fired by
    # ``turn.draw_card`` once per card about to be drawn (CR 121.6 --
    # a replacement can apply to each card individually). Models
    # Hullbreacher's "If an opponent would draw a card except the first
    # one they draw in each of their draw steps, instead you create a
    # Treasure token." -- an ``InsteadForSourceController`` transform
    # prevents the draw and queues the substitute action.
    WOULD_DRAW_A_CARD = "would_draw_a_card"
    # One or more cards were just put into a library from another zone
    # (CR 400.2; Wan Shi Tong's "Whenever one or more cards are put into
    # a library from anywhere"). Subject ``"player"`` is the library's
    # owner. Library-internal moves (look / reorder / bottom) don't fire.
    CARDS_PUT_INTO_LIBRARY = "cards_put_into_library"
    # A double-faced permanent has just transformed (CR 701.28; fired by
    # ``layouts.transform_permanent``). Subject is the permanent
    # (``"obj"``), already showing its new face -- "When this creature
    # transforms into <face>" lives on that face.
    TRANSFORMS = "transforms"
    # A permanent has just dealt damage (combat or noncombat; CR 120.1).
    # Subject is the source (``"source"``); the amount (all the damage it
    # dealt in that one event, summed over its targets) rides
    # ``combat_damage_amount``. "Whenever <this creature> deals damage"
    # (Cecil, Dark Knight). Fired by ``layouts.fire_deals_damage``.
    DEALS_DAMAGE = "deals_damage"
    # A player activated a (non-mana) ability of a permanent on the
    # battlefield (roto P1: Harsh Mentor). Subjects: the permanent
    # (``"source"``) and the activating Player (``"controller"``).
    # Dispatched by ``actions._activate_ability`` once the ability is on
    # the stack.
    ACTIVATES_ABILITY = "activates_ability"
    # "Whenever <source> deals damage to a player" (combat or not; roto
    # P6, ``effects.generic.roto_keywords.after_damage_to_player``).
    DEALS_DAMAGE_TO_PLAYER = "deals_damage_to_player"
    # A Saga's lore-counter count just went up (CR 714.2b: chapter N
    # triggers when the count becomes N or more). Subjects: the Saga
    # (``"obj"``) and the counts before / after (``"lore_before"`` /
    # ``"lore_after"``). A chapter ability is ``event=SAGA_CHAPTER,
    # predicate=ChapterReached(n)``; ``layouts.add_lore_counters`` pushes
    # the reached chapters in chapter order (not ``dispatch_event``).
    SAGA_CHAPTER = "saga_chapter"
    # A Class just became level N (CR 716.2a "When this Class becomes
    # level N, ..."). Subjects: the Class (``"obj"``) and the new level
    # (``"level"``). ``event=CLASS_LEVEL_GAINED,
    # predicate=ClassLevelReached(n)``; fired by
    # ``layouts.gain_class_level``.
    CLASS_LEVEL_GAINED = "class_level_gained"
    # A card left a graveyard (to any other zone: exile, a hand, a library,
    # the stack, the battlefield). Subjects: the graveyard's owner
    # (``"player"``) and the card as it was in the graveyard (``"card"``;
    # CR 603.10a: a trigger on a card leaving a graveyard looks back in
    # time). "Whenever an instant or sorcery card leaves your graveyard"
    # (Murktide Regent) is ``Subject("card", <instant or sorcery>) &
    # Controlled(subject="player")``. Data for the card IR (the mtg-zero
    # Rust engine): no dispatch site fires it here (SEARCH_LIMITATIONS
    # cube creatures 14).
    LEAVES_GRAVEYARD = "leaves_graveyard"
    # A card was put into a graveyard from any zone (CR 400.2: the
    # battlefield, a library, a hand, the stack, exile). Subjects: the
    # graveyard's owner (``"player"``) and the card as it is in the
    # graveyard (``"card"``): an ability that triggers when a card is put
    # into a zone "from anywhere" is never a leaves-the-battlefield ability
    # and doesn't look back in time (CR 603.6c, 603.10), so a token (not a
    # card) and a card exiled instead never fire it. "Whenever one or more
    # permanent cards are put into your graveyard from anywhere"
    # (Moonshadow) is ``Subject("card", IsPermanentCard()) &
    # Controlled(subject="player") & OneOrMoreBatch()``. Data for the card
    # IR (the mtg-zero Rust engine, ENGINE.md 220.3): no dispatch site fires
    # it here.
    CARD_PUT_INTO_GRAVEYARD = "card_put_into_graveyard"


# Subject-slot names per event. Used by the engine to validate that a
# dispatch call provides every required subject and by predicates to
# resolve the default subject when one isn't named explicitly.
EVENT_SUBJECTS: dict[Event, tuple[str, ...]] = {
    Event.ENTERS: ("obj",),
    Event.DIES: ("obj",),
    Event.LEAVES_BATTLEFIELD: ("obj",),
    Event.BECOMES_TAPPED: ("obj",),
    Event.ATTACKS: ("attacker",),
    Event.BLOCKS: ("blocker", "attacker"),
    Event.DEALS_COMBAT_DAMAGE: ("source", "target"),
    Event.DEALT_DAMAGE: ("obj",),
    Event.IS_CAST: ("spell", "controller"),
    Event.IS_SACRIFICED: ("obj",),
    Event.PLAYS_LAND: ("land", "controller"),
    Event.DISCARDS_CARD: ("player",),
    Event.COUNTERS_ADDED: ("obj",),
    Event.BEGINNING_OF_UPKEEP: ("player",),
    Event.BEGINNING_OF_COMBAT: ("player",),
    Event.BEGINNING_OF_END_STEP: ("player",),
    Event.BEGINNING_OF_POSTCOMBAT_MAIN: ("player",),
    Event.BEGINNING_OF_PRECOMBAT_MAIN: ("player",),
    Event.BEGINNING_OF_DRAW_STEP: ("player",),
    Event.ATTACKERS_UNBLOCKED: ("player", "defender"),
    Event.CREATURE_TOKENS_CREATED: ("player",),
    Event.CARD_DISCARDED: ("player", "card"),
    Event.DEALS_COMBAT_DAMAGE_TO_CREATURE: ("source", "target"),
    Event.DEALS_COMBAT_DAMAGE_ANY: ("source",),
    Event.BECOMES_BLOCKED: ("attacker",),
    Event.ROOM_FULLY_UNLOCKED: ("room",),
    Event.BECOMES_MONSTROUS: ("obj",),
    # ``target_spell`` replaces ``target`` when a spell on the stack is
    # targeted (``cube_creatures_rmc.fire_becomes_target``); ``targeting``
    # is the targeting StackItem (spell or ability).
    Event.BECOMES_TARGET: ("target", "controller", "target_spell", "targeting"),
    Event.YOU_ATTACK: ("player",),
    # WOULD_X (CR 614 replacement) events — see Event enum docs above.
    Event.WOULD_DRAW_CARDS: ("player",),
    Event.WOULD_CREATE_TOKEN: ("controller", "kind"),
    Event.TRIGGERED_ABILITY_TRIGGERS: (
        "triggering_obj", "ability_source", "entering_obj",
    ),
    Event.WOULD_PLACE_COUNTER: ("obj", "kind"),
    Event.WOULD_GO_TO_GRAVEYARD: ("card",),
    Event.WOULD_DRAW_FROM_EMPTY_LIBRARY: ("player",),
    Event.DRAWS_CARD: ("player",),
    Event.GAINS_LIFE: ("player",),
    Event.WOULD_DRAW_A_CARD: ("player",),
    Event.CARDS_PUT_INTO_LIBRARY: ("player",),
    Event.TRANSFORMS: ("obj",),
    Event.DEALS_DAMAGE: ("source",),
    Event.ACTIVATES_ABILITY: ("source", "controller"),
    Event.DEALS_DAMAGE_TO_PLAYER: ("source", "player"),
    Event.SAGA_CHAPTER: ("obj", "lore_before", "lore_after"),
    Event.CLASS_LEVEL_GAINED: ("obj", "level"),
    Event.LEAVES_GRAVEYARD: ("player", "card"),
    Event.CARD_PUT_INTO_GRAVEYARD: ("player", "card"),
}


class _Controller(Enum):
    """Symbolic reference to a player relative to the firing source.

    ``YOU`` resolves to the source ability's controller at predicate
    evaluation time. ``OPPONENT`` resolves to "any player who is NOT the
    source's controller". Future expansion: ``ANY``.
    """

    YOU = "you"
    OPPONENT = "opponent"


YOU = _Controller.YOU
OPPONENT = _Controller.OPPONENT


# ---------- TriggerPredicate base + boolean combinators ----------


class TriggerPredicate(ABC):
    """A boolean over (event subjects, game state, source object).

    Subclasses implement :meth:`matches`. Combine with ``&`` / ``|`` /
    ``~``; the operators return ``And`` / ``Or`` / ``Not`` instances
    that delegate evaluation to their operands.
    """

    @abstractmethod
    def matches(
        self,
        state: "GameState",
        subjects: Mapping[str, Any],
        source: "GameObject",
    ) -> bool:
        ...

    def __and__(self, other: "TriggerPredicate") -> "TriggerPredicate":
        return And(self, other)

    def __or__(self, other: "TriggerPredicate") -> "TriggerPredicate":
        return Or(self, other)

    def __invert__(self) -> "TriggerPredicate":
        return Not(self)


# Backwards-compatible alias. ``Predicate`` is the historical name; new
# code should prefer ``TriggerPredicate`` to keep the tier explicit, but
# the old name is kept on the surface so external imports survive the
# rename.
Predicate = TriggerPredicate


@dataclass(frozen=True)
class And(TriggerPredicate):
    left: TriggerPredicate
    right: TriggerPredicate

    def matches(self, state, subjects, source):
        return (
            self.left.matches(state, subjects, source)
            and self.right.matches(state, subjects, source)
        )


@dataclass(frozen=True)
class Or(TriggerPredicate):
    left: TriggerPredicate
    right: TriggerPredicate

    def matches(self, state, subjects, source):
        return (
            self.left.matches(state, subjects, source)
            or self.right.matches(state, subjects, source)
        )


@dataclass(frozen=True)
class Not(TriggerPredicate):
    inner: TriggerPredicate

    def matches(self, state, subjects, source):
        return not self.inner.matches(state, subjects, source)


@dataclass(frozen=True)
class Always(TriggerPredicate):
    """Tautology — fires for every event matching the trigger's
    event type. Useful as a default and in tests."""

    def matches(self, state, subjects, source):
        return True


@dataclass(frozen=True)
class Never(TriggerPredicate):
    """Contradiction — never fires. Useful for tests and as an
    explicit 'disabled' marker."""

    def matches(self, state, subjects, source):
        return False


# ---------- subject / source / state binders ----------


def _resolve_subject(
    subjects: Mapping[str, Any], slot: str | None
) -> Any:
    """Look up the named subject slot; if ``slot`` is ``None`` and
    the event has exactly one subject, use that. Returns ``None`` when
    the slot isn't present (caller then treats the binding as false)."""
    if slot is not None:
        return subjects.get(slot)
    if len(subjects) == 1:
        return next(iter(subjects.values()))
    return None


@dataclass(frozen=True)
class Subject(TriggerPredicate):
    """Binds an ``ObjectPredicate`` to a named subject slot.

    Pulls ``subjects[slot]`` and dispatches to ``inner.test(obj,
    state)``. ``slot=None`` means "use the event's default subject
    slot" (the single-subject case for events like ENTERS / DIES /
    ATTACKS). Missing slot → predicate evaluates false (the
    ObjectPredicate atoms are themselves robust to ``None``-ish
    subjects, but we short-circuit here for clarity).
    """

    slot: EventSubject | None
    inner: ObjectPredicate

    def __post_init__(self) -> None:
        object.__setattr__(self, "slot", coerce(EventSubject, self.slot))

    def matches(self, state, subjects, source):
        obj = _resolve_subject(subjects, self.slot)
        if obj is None:
            return False
        return self.inner.test(obj, state)


@dataclass(frozen=True)
class Source(TriggerPredicate):
    """Binds an ``ObjectPredicate`` to the firing source object.

    Useful for "is the source itself <property>?" gates that don't
    line up with any of the event's subject slots — e.g. the Survival
    intervening-if "if this creature is tapped" on a turn-structure
    event whose only subject is the active Player.
    """

    inner: ObjectPredicate

    def matches(self, state, subjects, source):
        return self.inner.test(source, state)


@dataclass(frozen=True)
class StateBound(TriggerPredicate):
    """Binds a ``StatePredicate`` to the firing source's controller.

    The wrapped predicate's ``viewer_id`` is taken to be
    ``source.controller_id``. This is the natural binding for the
    intervening-if and ability-word gates that read as "from the
    source's controller's perspective" — Void, Delirium,
    OpponentControlsMoreLandsThanYou, SpellsCastThisTurn,
    LoneAttackerForYou.
    """

    inner: StatePredicate

    def matches(self, state, subjects, source):
        return self.inner.test(state, source.controller_id)


# ---------- source-aware trigger predicates ----------


@dataclass(frozen=True)
class FirstAttackThisTurn(TriggerPredicate):
    """"attacks for the first time each turn" (Fear of Missing Out): the
    source appears exactly once in its controller's
    ``Player.attacked_this_turn`` latch, which the attack declaration
    appends to before dispatching ``Event.ATTACKS`` -- so an attack in an
    additional combat phase doesn't trigger again."""

    def matches(self, state, subjects, source):
        player = state.player_by_id(source.controller_id)
        n = 0
        for obj in player.attacked_this_turn:
            if obj.id == source.id:
                n += 1
        return n == 1


@dataclass(frozen=True)
class ChapterReached(TriggerPredicate):
    """Saga chapter ``n`` (CR 714.2b): the source Saga's lore-counter
    count went from below ``n`` to ``n`` or more (``Event.SAGA_CHAPTER``
    subjects ``lore_before`` / ``lore_after``)."""

    n: int

    def matches(self, state, subjects, source):
        return (
            subjects.get("obj") is source
            and subjects.get("lore_before", 0) < self.n
            <= subjects.get("lore_after", 0)
        )


@dataclass(frozen=True)
class ClassLevelReached(TriggerPredicate):
    """"When this Class becomes level ``n``" (CR 716.2a;
    ``Event.CLASS_LEVEL_GAINED`` subject ``level``)."""

    n: int

    def matches(self, state, subjects, source):
        return subjects.get("obj") is source and subjects.get("level") == self.n


@dataclass(frozen=True)
class IsSelf(TriggerPredicate):
    """The named subject IS the firing source object (object identity).

    Models "this creature/permanent <event>" — the event subject
    being the same permanent that hosts the triggered ability.

    On turn-structure events whose only subject is a Player rather than
    a permanent, the no-subject ``IsSelf()`` reads as "the source IS
    itself" and evaluates true: the dispatcher already iterates one
    source at a time, so the predicate is tautological for those events.
    Detection: a Player exposes ``id`` but no ``printing`` attribute.
    """

    subject: EventSubject | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "subject", coerce(EventSubject, self.subject))

    def matches(self, state, subjects, source):
        obj = _resolve_subject(subjects, self.subject)
        if (
            self.subject is None
            and obj is not None
            and not hasattr(obj, "printing")
        ):
            return True
        return obj is source


@dataclass(frozen=True)
class IsHost(TriggerPredicate):
    """The named subject IS the permanent this source is attached to.

    Models the aura/equipment relational triggers — "Whenever
    enchanted creature attacks", "Whenever enchanted permanent
    becomes tapped", "Whenever equipped creature attacks",
    "Whenever enchanted creature is dealt damage" — where the
    *source* of the trigger is the aura/Equipment and the firing
    subject is its host (the enchanted/equipped permanent it's
    attached to).

    The match keys off ``source.attached_to == subject.id``. When
    the source isn't attached to anything (``attached_to is None``)
    or the named subject isn't a permanent with an ``id``, the
    predicate evaluates false — auras that have lost their host
    via SBA cleanup never spuriously fire.
    """

    subject: EventSubject | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "subject", coerce(EventSubject, self.subject))

    def matches(self, state, subjects, source):
        obj = _resolve_subject(subjects, self.subject)
        if obj is None:
            return False
        host_id = getattr(source, "attached_to", None)
        if host_id is None:
            return False
        return getattr(obj, "id", None) == host_id


@dataclass(frozen=True)
class IsFormerHost(TriggerPredicate):
    """The named subject is the permanent this Equipment/Aura was
    attached to when that permanent left the battlefield — "Whenever
    equipped creature dies" (Skullclamp).

    ``move_off_battlefield`` unattaches Equipment from a leaving host
    BEFORE dispatching DIES; it records the host id on the Equipment's
    transient ``last_attached_to`` for the duration of that dispatch,
    which this predicate reads.
    """

    subject: EventSubject | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "subject", coerce(EventSubject, self.subject))

    def matches(self, state, subjects, source):
        obj = _resolve_subject(subjects, self.subject)
        if obj is None:
            return False
        host_id = getattr(source, "last_attached_to", None)
        if host_id is None:
            return False
        return getattr(obj, "id", None) == host_id


@dataclass(frozen=True)
class HostAttacksWithAnother(TriggerPredicate):
    """"Whenever equipped creature and at least one other creature
    attack" (Glimmer Lens): on the once-per-declaration YOU_ATTACK
    event, the source's host is attacking and at least one other
    creature its controller controls is attacking too."""

    def matches(self, state, subjects, source):
        host_id = getattr(source, "attached_to", None)
        if host_id is None:
            return False
        attackers = [
            o for o in state.battlefield
            if o.attacking and o.controller_id == source.controller_id
        ]
        return (
            any(o.id == host_id for o in attackers) and len(attackers) >= 2
        )


@dataclass(frozen=True)
class Controlled(TriggerPredicate):
    """The named subject is controlled by the indicated player.

    ``by`` of ``YOU`` resolves to the source ability's controller at
    evaluation time; ``OPPONENT`` resolves to "any non-controller"
    (which in two-player games is exactly "the other player").

    Two subject shapes are supported:

    - **Permanent subject** (``GameObject``): match by
      ``obj.controller_id == source.controller_id`` — the standard
      "you control this permanent" check used by ENTERS / DIES /
      ATTACKS / etc.
    - **Player subject** (``Player``): match by
      ``player.id == source.controller_id``. This is the path used by
      turn-structure events and the discarding-player /
      land-playing player subjects.

    The branch is selected by the presence of ``controller_id`` on the
    subject.
    """

    by: _Controller = YOU
    subject: EventSubject | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "subject", coerce(EventSubject, self.subject))

    def matches(self, state, subjects, source):
        obj = _resolve_subject(subjects, self.subject)
        if obj is None:
            return False
        if hasattr(obj, "controller_id"):
            controller = obj.controller_id
        else:
            controller = getattr(obj, "id", None)
        if controller is None:
            return False
        if self.by == YOU:
            return controller == source.controller_id
        if self.by == OPPONENT:
            return controller != source.controller_id
        raise NotImplementedError(f"unsupported controller ref: {self.by!r}")
