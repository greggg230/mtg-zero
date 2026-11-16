"""Object-tier predicates: pure boolean tests over a single
``GameObject``.

This module is the bottom layer of the two-tier predicate stack. An
``ObjectPredicate`` is slot-agnostic — it doesn't know about subject
slots, source objects, or trigger dispatch. It just answers
"does this object satisfy the property?" given the object and the
current ``GameState`` (the state is needed for runtime-keyword and
runtime-power lookups that consult layers/grants).

The trigger-combinator layer in :mod:`mtgz_ir.effects.predicates`
wraps these atoms with ``Subject(slot, inner)`` and ``Source(inner)``
to bind them to a specific subject slot or to the source object.

Composition: ``&`` / ``|`` / ``~`` build object-level And/Or/Not
combinators. The resulting trees are themselves ``ObjectPredicate``s
and can be wrapped by ``Subject`` / ``Source`` exactly like the atoms.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.keyword_vocab import KeywordSpelling

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


class ObjectPredicate(ABC):
    """A boolean over a single ``GameObject`` plus the current
    ``GameState``.

    Subclasses implement :meth:`test`. Combine with ``&`` / ``|`` /
    ``~``; the operators return ``ObjectAnd`` / ``ObjectOr`` /
    ``ObjectNot`` instances that recursively delegate to their operands.
    """

    @abstractmethod
    def test(self, obj: "GameObject", state: "GameState") -> bool:
        ...

    def __and__(self, other: "ObjectPredicate") -> "ObjectPredicate":
        return ObjectAnd(self, other)

    def __or__(self, other: "ObjectPredicate") -> "ObjectPredicate":
        return ObjectOr(self, other)

    def __invert__(self) -> "ObjectPredicate":
        return ObjectNot(self)


@dataclass(frozen=True)
class ObjectAnd(ObjectPredicate):
    # Either side may be a ``target_filter.OneOf`` ("creature or
    # planeswalker" ANDed with a controller filter).
    left: "ObjectPredicate | OneOf"
    right: "ObjectPredicate | OneOf"

    def test(self, obj, state):
        return self.left.test(obj, state) and self.right.test(obj, state)


@dataclass(frozen=True)
class ObjectOr(ObjectPredicate):
    left: ObjectPredicate
    right: ObjectPredicate

    def test(self, obj, state):
        return self.left.test(obj, state) or self.right.test(obj, state)


@dataclass(frozen=True)
class ObjectNot(ObjectPredicate):
    inner: ObjectPredicate

    def test(self, obj, state):
        return not self.inner.test(obj, state)


# ---------- type predicates ----------


@dataclass(frozen=True)
class IsLand(ObjectPredicate):
    """The object is a land (``"Land"`` in its effective card types)."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, "Land")


@dataclass(frozen=True)
class IsBasic(ObjectPredicate):
    """The object's printing carries the ``"Basic"`` supertype.

    Composes with :class:`IsLand` for "nonbasic land" filters
    (``IsLand() & ~IsBasic()``) — Dust Bowl's "destroy target nonbasic
    land" decomposes cleanly through this. Players have no
    ``printing`` and evaluate False.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return "Basic" in obj.printing.supertypes


@dataclass(frozen=True)
class IsCreature(ObjectPredicate):
    """The object is a creature, in the rules-correct sense.

    Reads through :func:`mtgz_ir.game.is_creature_now` so type-changing
    runtime state is honoured: an Impending-suppressed Overlord whose
    printed type line still reads "Creature" evaluates False until the
    last time counter is removed (CR 702.176); a face-down permanent
    is True regardless of the printed type (CR 701.34); a Spacecraft
    above its tier-threshold is True; a manlanded land is True for
    the activation duration. Mirrors the
    :class:`HasSubtype` precedent — predicates should reflect the
    object's effective state, not just its printing.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import is_creature_now

        return is_creature_now(obj)


@dataclass(frozen=True)
class IsArtifact(ObjectPredicate):
    """The object is an artifact -- its effective card types (CR 613
    layer 4: Mycosynth Lattice, Liquimetal Coating, ...) through
    :func:`mtgz_ir.game.has_card_type`."""

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, "Artifact")


@dataclass(frozen=True)
class IsLegendary(ObjectPredicate):
    """The object's printing carries the ``"Legendary"`` supertype.

    Composes with the type atoms for "legendary creature" filters
    (Karakas's "Return target legendary creature to its owner's
    hand."). Players have no ``printing`` and evaluate False.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return "Legendary" in obj.printing.supertypes


@dataclass(frozen=True)
class IsEnchantment(ObjectPredicate):
    """The object is an enchantment.

    Models the "an enchantment you control enters" qualifier on the
    Duskmourn "Eerie" ability-word trigger family. Mirrors the
    ``IsArtifact`` / ``IsCreature`` siblings — checks the printing's
    ``card_types`` for the ``"Enchantment"`` type. Rooms (which print
    as ``Enchantment — Room``) carry ``"Enchantment"`` in their
    ``card_types`` so a Room entering ALSO satisfies this predicate;
    that's the printed Eerie semantics.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, "Enchantment")


@dataclass(frozen=True)
class IsPlayer(ObjectPredicate):
    """The object is a Player (not a permanent).

    Distinguishes the player vs. permanent target slot in multi-subject
    events like ``DEALS_COMBAT_DAMAGE`` whose ``"target"`` slot can be
    either a creature or a player. Detection mirrors the
    permanent-vs-player branch elsewhere: Players expose ``id`` but
    lack ``printing``, while permanents expose both.
    """

    def test(self, obj, state):
        # Players have ``id`` but no ``printing``; permanents have both.
        return obj is not None and not hasattr(obj, "printing")


@dataclass(frozen=True)
class IsPlaneswalker(ObjectPredicate):
    """The object is a planeswalker (``"Planeswalker"`` in its
    card_types).

    Models the planeswalker arm of the "any target" expansion (CR 115:
    a planeswalker is one of the legal kinds for damage-targeting
    effects whose printed phrasing is "any target"). Players evaluate
    false.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, "Planeswalker")


@dataclass(frozen=True)
class IsBattle(ObjectPredicate):
    """The object is a Battle (``"Battle"`` in its card_types).

    Models the battle arm of the "any target" expansion. The current
    EoE pool ships no Battles, so this atom never matches today, but
    it's part of the structural target-filter vocabulary so a future
    printing wired to "any target" picks Battles up automatically.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, "Battle")


@dataclass(frozen=True)
class IsNontoken(ObjectPredicate):
    """The object is NOT a token.

    Token-ness lives on the printing (``CardPrinting.is_token`` checks
    for the ``"Token"`` supertype). Models the "nontoken" qualifier in
    triggers like ``"Whenever a nontoken artifact you control enters"``
    — cards spawned by token-creator effects must be excluded from the
    firing pool.
    """

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        return not obj.printing.is_token


@dataclass(frozen=True)
class HasSubtype(ObjectPredicate):
    """The object's *effective* subtypes include the given string.

    Models the type-line subtype check used by triggers and Affinity-
    style cost reductions that gate on a creature subtype (e.g.
    ``"Whenever a Kavu you control enters"``,
    ``PerControlled(HasSubtype("Sliver"))``). Reads through
    :func:`mtgz_ir.game.effective_subtypes` so a creature granted a
    subtype by a layer-3 / layer-4 continuous effect (Conspiracy
    granting "Sliver", Ragost granting "Food", Magus / Dryad layering
    on lands, …) counts toward the predicate. This is the rules-correct
    CR 613 reading: subtype membership is determined after all type-
    changing layers apply, not from the printed type line alone.
    """

    name: str

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_subtypes

        return self.name in effective_subtypes(state, obj)


@dataclass(frozen=True)
class IsTapped(ObjectPredicate):
    """The object is currently tapped.

    Plain object-property check — ``bool(getattr(obj, "tapped",
    False))``. Players have no ``tapped`` attribute and evaluate false.
    The Survival idiom "if this creature is tapped" composes this
    atom with ``Source(...)`` at the trigger layer to pin the check to
    the source permanent rather than the (player) event subject.
    """

    def test(self, obj, state):
        return bool(getattr(obj, "tapped", False))


@dataclass(frozen=True)
class HasPowerToughness(ObjectPredicate):
    """The object's effective power and toughness are exactly
    ``power``/``toughness`` ("a 1/1 creature" — Sword of the Meek)."""

    power: int
    toughness: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_power, effective_toughness

        return (
            effective_power(state, obj) == self.power
            and effective_toughness(state, obj) == self.toughness
        )


@dataclass(frozen=True)
class IsHarnessed(ObjectPredicate):
    """The object is harnessed (CR 701.64 — The Mind Stone's
    ``Harness``). Gates the Infinity (``∞ —``) abilities of Infinity
    Stones: composed as ``Source(IsHarnessed())`` on their triggers."""

    def test(self, obj, state):
        return bool(getattr(obj, "harnessed", False))


@dataclass(frozen=True)
class IsSource(ObjectPredicate):
    """The object IS the currently-resolving source.

    Reads ``state.resolving_source`` — pinned by the trigger-firing /
    spell-resolution path (``actions._fire_triggered_ability`` and
    peers) for the duration of target enumeration and resolution.
    Composes with ``~`` for "another / other target" qualifiers
    ("destroy another target creature" → ``IsCreature() & ~IsSource()``).

    Players have no ``id`` matching a permanent's source id, but
    defensively ``ControlledByOpponent``-style: returns ``False`` if
    the source isn't pinned, rather than over-accepting.
    """

    def test(self, obj, state):
        source = getattr(state, "resolving_source", None)
        if source is None:
            return False
        if not hasattr(obj, "printing"):
            return False
        return obj.id == source.id


@dataclass(frozen=True)
class ControlledByOpponent(ObjectPredicate):
    """The object's controller is NOT the current target-selecting
    controller.

    Models the "an opponent controls" qualifier on filters like
    "target creature an opponent controls". The "current target-
    selecting controller" is read from
    ``state.resolving_controller_id`` — a transient field the
    targeting pipeline pins before invoking ``TargetFilter.test``.
    Players evaluate against ``id != resolving_controller_id``;
    permanents against ``controller_id != resolving_controller_id``.

    A ``None`` ``resolving_controller_id`` means the field hasn't
    been pinned for this enumeration — defensively returns ``False``
    rather than over-accepting.
    """

    def test(self, obj, state):
        viewer_id = getattr(state, "resolving_controller_id", None)
        if viewer_id is None:
            return False
        if hasattr(obj, "printing"):
            return obj.controller_id != viewer_id
        # Players: their ``id`` is the controller identity.
        if obj is None:
            return False
        return getattr(obj, "id", None) != viewer_id


@dataclass(frozen=True)
class ControlledByYou(ObjectPredicate):
    """The object's controller IS the current target-selecting
    controller.

    Mirror of :class:`ControlledByOpponent` for "you control"
    qualifiers ("target creature you control"). Same
    ``resolving_controller_id`` source; same defensive ``False``
    when the field is unset.
    """

    def test(self, obj, state):
        viewer_id = getattr(state, "resolving_controller_id", None)
        if viewer_id is None:
            return False
        if hasattr(obj, "printing"):
            return obj.controller_id == viewer_id
        if obj is None:
            return False
        return getattr(obj, "id", None) == viewer_id


@dataclass(frozen=True)
class IsAttacking(ObjectPredicate):
    """The object is currently attacking — ``obj.attacking`` is True.

    Models the "attacking creature" qualifier on filters like Focus
    Fire's "target attacking or blocking creature" (composed as
    ``IsCreature() & (IsAttacking() | IsBlocking())``). Players and
    lands have no ``attacking`` attribute and evaluate false.
    """

    def test(self, obj, state):
        return bool(getattr(obj, "attacking", False))


@dataclass(frozen=True)
class IsBlocking(ObjectPredicate):
    """The object is currently blocking — ``obj.blocking is not None``.

    Models the "blocking creature" qualifier on cost-reduction gates
    like Dark Endurance ("This spell costs {1} less to cast if it
    targets a blocking creature."). Players and lands have no
    ``blocking`` attribute and evaluate false; declared-blocker state
    on a creature ``GameObject`` is the integer id of the attacker it
    was assigned to (or ``None`` when not blocking).
    """

    def test(self, obj, state):
        return getattr(obj, "blocking", None) is not None


@dataclass(frozen=True)
class BlockedBySource(ObjectPredicate):
    """The object is a creature the resolving source is blocking --
    Tinder Wall's "target creature it's blocking".

    Reads ``state.resolving_source`` (pinned during target enumeration
    and resolution, like ``IsSource``). While the source is on the
    battlefield its live block decides (``source.blocking``; a blocker
    removed from combat blocks nothing). Once it has left -- Tinder Wall
    is sacrificed as part of the cost -- the ability uses the source's
    last known information (CR 608.2h): the attacker it was blocking
    still lists it among its blockers (``blocked_by``; an attacker stays
    blocked when its blocker leaves, CR 509.1h)."""

    def test(self, obj, state):
        source = getattr(state, "resolving_source", None)
        if source is None or not hasattr(obj, "printing"):
            return False
        if any(o is source for o in state.battlefield):
            return getattr(source, "blocking", None) == obj.id
        return source.id in getattr(obj, "blocked_by", ())


@dataclass(frozen=True)
class HasKeyword(ObjectPredicate):
    """The object currently has the given keyword.

    Reads through ``effective_keywords(state, obj)`` rather than the
    raw ``obj.printing.keywords`` set — granted/temporary keywords are
    honored. Players have no keywords and evaluate false.
    """

    name: KeywordSpelling

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            # Players don't have keywords.
            return False
        from mtgz_ir.game import effective_keywords

        return self.name in effective_keywords(state, obj)


# Map a counter "kind" string to the ``GameObject`` attribute that
# holds its current count. Engine stores counters as separate scalar
# fields on ``GameObject`` (not a single dict) — the predicate maps
# the canonical printed names (``"+1/+1"``, ``"charge"``, ``"stun"``,
# ``"loyalty"``) onto those fields. New counter kinds must extend
# this table when they get added engine-side.
_COUNTER_ATTRS: dict[str, str] = {
    "+1/+1": "plus_one_counters",
    "charge": "charge_counters",
    "stun": "stun_counters",
    "silver": "silver_counters",
    "loyalty": "loyalty_counters",
}


@dataclass(frozen=True)
class HasCounter(ObjectPredicate):
    """The object has at least one counter of ``kind`` on it.

    ``kind`` is the printed counter name (``"+1/+1"``, ``"charge"``,
    ``"stun"``, ``"loyalty"``) — the predicate looks up the
    corresponding scalar field on ``GameObject`` via ``_COUNTER_ATTRS``
    and tests for ``> 0``. Unknown counter kinds raise ``KeyError`` at
    evaluation time so a typo surfaces immediately rather than
    silently never matching.
    """

    kind: str

    def test(self, obj, state):
        attr = _COUNTER_ATTRS[self.kind]
        return getattr(obj, attr, 0) > 0


@dataclass(frozen=True)
class HasPowerAtMost(ObjectPredicate):
    """The object's effective power is less than or equal to ``n``.

    Models the "with power N or less" qualifier in triggers like
    Vicious Clown's ``Whenever another creature you control with power
    2 or less enters, …``. Reads through ``effective_power(state, obj)``
    so a creature whose base power is reduced or boosted by counters /
    pump / anthems is measured by its current power.

    Non-creature subjects (lands, players) and subjects without a
    measurable power evaluate false — the qualifier only makes sense
    on a creature subject.
    """

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_power

        try:
            power = effective_power(state, obj)
        except Exception:
            return False
        return power <= self.n


@dataclass(frozen=True)
class HasPowerAtLeast(ObjectPredicate):
    """The object's effective power is at least ``n`` ("with power N or
    greater" -- Vaultborn Tyrant, the ferocious gate). Same measuring
    rules as :class:`HasPowerAtMost`."""

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_power

        try:
            power = effective_power(state, obj)
        except Exception:
            return False
        return power >= self.n


@dataclass(frozen=True)
class HasToughnessAtMost(ObjectPredicate):
    """The object's effective toughness is less than or equal to ``n``.

    Mirror of :class:`HasPowerAtMost` for the "with toughness N or
    less" qualifier. Composes with ``HasPowerAtMost`` via ``|`` to
    express "with power or toughness N or less" (Warping Wail's
    Scryfall-updated mode 1).
    """

    n: int

    def test(self, obj, state):
        if not hasattr(obj, "printing"):
            return False
        from mtgz_ir.game import effective_toughness

        try:
            toughness = effective_toughness(state, obj)
        except Exception:
            return False
        return toughness <= self.n


@dataclass(frozen=True)
class HasCardType(ObjectPredicate):
    """The object's card types include ``card_type`` — the zone-agnostic
    check the spell-cast trigger family needs (``Whenever you cast a
    noncreature spell`` / ``an instant or sorcery spell``), where the
    subject is a card on the stack. Reads :func:`mtgz_ir.game.
    has_card_type` (a permanent's layer-4 types; a card elsewhere has its
    printed ones)."""

    card_type: str

    def test(self, obj, state):
        if getattr(obj, "printing", None) is None:
            return False
        from mtgz_ir.game import has_card_type

        return has_card_type(state, obj, self.card_type)


# ---------- kicker ----------


@dataclass(frozen=True)
class ManaValueAtMostSourceX(ObjectPredicate):
    """"with mana value X or less" where X is the resolving source's X
    (``GameObject.entered_x_value``, Thieving Skydiver's kicker X)."""

    def test(self, obj, state):
        src = getattr(state, "resolving_source", None)
        printing = getattr(obj, "printing", None)
        if src is None or printing is None:
            return False
        return printing.mana_cost.total <= src.entered_x_value


# ---------- optional additional costs (Squad / Offspring) ----------


@dataclass(frozen=True)
class SquadCostPaid(ObjectPredicate):
    """"if its squad cost was paid" (CR 702.157a)."""

    def test(self, obj, state):
        return getattr(obj, "squad_paid_count", 0) > 0


@dataclass(frozen=True)
class OffspringCostPaid(ObjectPredicate):
    """"if its offspring cost was paid" (CR 702.175a)."""

    def test(self, obj, state):
        return bool(getattr(obj, "offspring_paid", False))


# ---------- drawing-player predicates (DRAWS_CARD / WOULD_DRAW_A_CARD) ----------


@dataclass(frozen=True)
class DrawIsFirstInOwnDrawStep(ObjectPredicate):
    """The subject Player's in-flight draw is the first card they draw
    in their own draw step (CR 504.1's turn-based draw). Negated, it is
    the "except the first one they draw in each of their draw steps"
    rider (Orcish Bowmasters, Hullbreacher). ``turn.draw_card`` raises
    ``Player.drawing_first_in_draw_step`` while that draw's replacement
    and trigger dispatch run."""

    def test(self, obj, state):
        return bool(getattr(obj, "drawing_first_in_draw_step", False))


@dataclass(frozen=True)
class HasDrawnNCardsThisTurn(ObjectPredicate):
    """The subject Player has drawn exactly ``n`` cards this turn —
    evaluated right after a draw, so ``n=2`` reads "draws their second
    card each turn" (Faerie Mastermind)."""

    n: int

    def test(self, obj, state):
        return getattr(obj, "cards_drawn_this_turn", None) == self.n
