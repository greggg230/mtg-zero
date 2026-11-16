"""State-tier predicates: subject-independent boolean tests over the
``GameState``, viewed from a particular player's perspective.

This module is the second leaf-layer of the two-tier predicate stack
alongside :mod:`mtgz_ir.effects.object_predicates`. A
``StatePredicate`` doesn't know about subject slots or source objects;
it just answers "does the game state satisfy this property right now,
viewed from ``viewer_id``'s perspective?"

The trigger-combinator layer in :mod:`mtgz_ir.effects.predicates`
binds these atoms via ``StateBound(inner)`` (wraps a ``StatePredicate``
into a trigger predicate, evaluating with
``viewer_id = source.controller_id``).

Composition: ``&`` / ``|`` / ``~`` build state-level And/Or/Not
combinators. The resulting trees are themselves ``StatePredicate``s
and can be wrapped by ``StateBound`` exactly like the atoms.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.card_types import DELIRIUM_THRESHOLD

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# Player ids are plain ints engine-side; alias for clarity at call
# sites and in documentation.
PlayerId = int


class StatePredicate(ABC):
    """A boolean over the ``GameState`` plus a viewer player id.

    Subclasses implement :meth:`test`. Combine with ``&`` / ``|`` /
    ``~``; the operators return ``StateAnd`` / ``StateOr`` /
    ``StateNot`` instances that recursively delegate.
    """

    @abstractmethod
    def test(self, state: "GameState", viewer_id: PlayerId) -> bool:
        ...

    def __and__(self, other: "StatePredicate") -> "StatePredicate":
        return StateAnd(self, other)

    def __or__(self, other: "StatePredicate") -> "StatePredicate":
        return StateOr(self, other)

    def __invert__(self) -> "StatePredicate":
        return StateNot(self)


@dataclass(frozen=True)
class StateAnd(StatePredicate):
    left: StatePredicate
    right: StatePredicate

    def test(self, state, viewer_id):
        return (
            self.left.test(state, viewer_id)
            and self.right.test(state, viewer_id)
        )


@dataclass(frozen=True)
class StateOr(StatePredicate):
    left: StatePredicate
    right: StatePredicate

    def test(self, state, viewer_id):
        return (
            self.left.test(state, viewer_id)
            or self.right.test(state, viewer_id)
        )


@dataclass(frozen=True)
class StateNot(StatePredicate):
    inner: StatePredicate

    def test(self, state, viewer_id):
        return not self.inner.test(state, viewer_id)


# ---------- atoms ----------


@dataclass(frozen=True)
class VoidActive(StatePredicate):
    """Void is active right now — i.e. a nonland permanent left the
    battlefield this turn or a spell was warped this turn. Viewer-
    independent (Void state is global, not per-player).

    Models the EoE "Void —" intervening-if gate that appears on
    triggered-ability lines like ``Void — Whenever <X>, if a nonland
    permanent left the battlefield this turn or a spell was warped this
    turn, <effect>.`` (Alpharael Stonechosen, Roving Actuator).
    """

    def test(self, state, viewer_id):
        from mtgz_ir.game import is_void_active

        return is_void_active(state)


@dataclass(frozen=True)
class OpponentControlsMoreLandsThanYou(StatePredicate):
    """At least one opponent of ``viewer_id`` controls strictly more
    lands than ``viewer_id`` does.

    Models the "if an opponent controls more lands than you"
    intervening-if gate on Sunstar Expansionist's ETB. The static-side
    equivalent is ``ConditionalIfOpponentHasMoreLands`` (an effect
    combinator that gates resolution).
    """

    def test(self, state, viewer_id):
        my_lands = sum(
            1 for o in state.battlefield
            if o.controller_id == viewer_id and o.printing.is_land
        )
        for p in state.players:
            if p.id == viewer_id or getattr(p, "has_lost", False):
                continue
            count = sum(
                1 for o in state.battlefield
                if o.controller_id == p.id and o.printing.is_land
            )
            if count > my_lands:
                return True
        return False


@dataclass(frozen=True)
class Delirium(StatePredicate):
    """There are ``min_card_types`` (printed: four) or more card types
    among cards in the viewer's graveyard -- the Delirium ability word's
    condition at the trigger layer (``StateBound(Delirium())`` for the
    "Delirium -- Whenever ..., if there are four or more card types
    among cards in your graveyard, ..." intervening-if gates).

    The count itself is :func:`mtgz_ir.effects.card_types.delirium_active`
    -- the ONE shared definition (CR 205.2a types incl. kindred, split
    halves combined, DFCs by their front face, tokens excluded) that the
    Condition-tier twin :class:`mtgz_ir.effects.conditions.DeliriumActive`
    (statics, spell riders, activation restrictions) also delegates to.
    """

    min_card_types: int = DELIRIUM_THRESHOLD

    def test(self, state, viewer_id):
        from mtgz_ir.effects.card_types import delirium_active

        return delirium_active(state, viewer_id, self.min_card_types)


@dataclass(frozen=True)
class NoncreatureSpellsCastThisTurn(StatePredicate):
    """The viewer has cast exactly ``n`` noncreature spells this turn
    (``Player.noncreature_spells_cast_this_turn``, read post-increment
    like ``SpellsCastThisTurn``). The Fantasticar's "Whenever you cast
    your fourth noncreature spell each turn"."""

    n: int

    def test(self, state, viewer_id):
        try:
            player = state.player_by_id(viewer_id)
        except (KeyError, AttributeError):
            return False
        return getattr(player, "noncreature_spells_cast_this_turn", 0) == self.n


@dataclass(frozen=True)
class SpellsCastThisTurn(StatePredicate):
    """The viewer has cast exactly ``n`` spells this turn (counted via
    ``Player.spells_cast_this_turn``).

    Models the "second spell each turn" trigger family (Illvoi
    Operative, Sunstar Lightsmith, Cosmogrand Zenith, Station Monitor):
    paired with ``Subject("controller", IsControllerYou)`` (or the
    pre-existing ``Controlled(YOU, "controller")`` trigger gate at the
    trigger-combinator layer), this latches the moment the source's
    own controller's running total flips to ``n``. The predicate reads
    the count post-increment of ``spells_cast_this_turn`` since the
    dispatcher fires AFTER ``actions._cast_spell`` has bumped the
    counter.
    """

    n: int

    def test(self, state, viewer_id):
        try:
            player = state.player_by_id(viewer_id)
        except (KeyError, AttributeError):
            return False
        return getattr(player, "spells_cast_this_turn", 0) == self.n


@dataclass(frozen=True)
class LoneAttackerForYou(StatePredicate):
    """Exactly one creature is currently attacking AND that lone
    attacker is controlled by ``viewer_id``.

    Models the "Whenever a creature you control attacks alone, ..."
    trigger qualifier (Widow's Walk on Derelict Attic // Widow's Walk).
    Pairs with the once-per-pass ``Event.YOU_ATTACK`` event so the
    predicate is evaluated AFTER every attacker for the active
    declare-attackers pass has flipped to ``attacking=True`` —
    distinct from the per-attacker ``Event.ATTACKS`` event whose
    dispatch fires inside the per-attacker loop.
    """

    def test(self, state, viewer_id):
        attackers = [o for o in state.battlefield if o.attacking]
        if len(attackers) != 1:
            return False
        return attackers[0].controller_id == viewer_id


@dataclass(frozen=True)
class AnyPlayerControlsNoCreatures(StatePredicate):
    """At least one (non-eliminated) player controls zero creatures on
    the battlefield.

    Viewer-independent: the predicate walks every active player and
    returns True the moment one of them owns no battlefield creatures.
    Any creature on the battlefield (printed creature, animated land,
    stationed Spacecraft) counts via ``printing.is_creature``; the
    legacy bespoke effect used the same surface check, so we mirror
    that scope here.

    Models the intervening-if gate "if a player controls no creatures"
    on Sothera, the Supervoid's end-step trigger. The trigger fires only
    when, at the beginning of the controller's end step, at least one
    player has been wiped of creatures — the gate is the printed
    "give up your last creature, get one back" pivot.
    """

    def test(self, state, viewer_id):
        for p in state.players:
            if getattr(p, "has_lost", False):
                continue
            has_creature = any(
                o.controller_id == p.id and o.printing.is_creature
                for o in state.battlefield
            )
            if not has_creature:
                return True
        return False


@dataclass(frozen=True)
class IsMonarch(StatePredicate):
    """The viewer is the monarch (CR 724) — ``if you're the monarch``."""

    def test(self, state, viewer_id):
        return state.player_by_id(viewer_id).is_monarch


@dataclass(frozen=True)
class HasInitiative(StatePredicate):
    """The viewer has the initiative (CR 725) — ``if you have the
    initiative``."""

    def test(self, state, viewer_id):
        return state.player_by_id(viewer_id).has_initiative


@dataclass(frozen=True)
class GainedLifeThisTurn(StatePredicate):
    """The viewer gained life this turn -- ``if you gained life this
    turn`` (Ocelot Pride)."""

    def test(self, state, viewer_id):
        return state.player_by_id(viewer_id).life_gained_this_turn > 0


@dataclass(frozen=True)
class HasCitysBlessing(StatePredicate):
    """The viewer has the city's blessing (Ascend, CR 702.131) -- ``if
    you have the city's blessing``."""

    def test(self, state, viewer_id):
        return state.player_by_id(viewer_id).has_citys_blessing


@dataclass(frozen=True)
class CompletedADungeon(StatePredicate):
    """The viewer has completed at least one dungeon (CR 309.7) —
    ``if you've completed a dungeon``."""

    def test(self, state, viewer_id):
        return state.player_by_id(viewer_id).dungeons_completed > 0


@dataclass(frozen=True)
class CardsInGraveyardAtLeast(StatePredicate):
    """The viewer has at least ``n`` cards in their graveyard —
    Threshold's ``if there are seven or more cards in your graveyard``
    (``n=7``)."""

    n: int = 7

    def test(self, state, viewer_id):
        return len(state.player_by_id(viewer_id).graveyard) >= self.n


@dataclass(frozen=True)
class LandsPlayedThisTurnAtLeast(StatePredicate):
    """The viewer has played at least ``n`` lands this turn — Fastbond's
    ``if it wasn't the first land you played this turn`` (``n=2``,
    checked after the land play that triggered it is counted)."""

    n: int = 2

    def test(self, state, viewer_id):
        return state.player_by_id(viewer_id).lands_played_this_turn >= self.n
