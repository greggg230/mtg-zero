"""Generic effects cluster module — graveyard-return primitives.

Houses the unified ``ReturnFromGraveyard`` base and its constructor-sugar
subclasses (the legacy ``Return*FromGraveyard*`` family). Each subclass
preserves its public name and constructor surface for parser-dispatch /
test stability; the base does the actual zone-walk + filter + move +
optional rider work.

Sibling cluster files for adjacent surface areas:
  * ``bounce.py`` — battlefield-to-hand bounces and the legacy bounce
    sugar classes (also re-exports several ``ReturnFromGraveyard``
    subclasses for cards whose printed text starts with "return" but
    whose engine semantics live here).
  * ``zone_moves.py`` — generic zone-to-zone primitives that don't
    fit the graveyard-return family.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from mtgz_ir.effects.vocab import GraveyardReturnDestination
from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# ---------------------------------------------------------------
# Unified ``ReturnFromGraveyard`` base.
#
# Replaces the bespoke recipe shared by the legacy
# ``Return*FromGraveyard*`` family: walk the controller's graveyard,
# filter cards by a card-type predicate (creature, artifact, land,
# nonland permanent, plus optional widening for enchantment /
# Spacecraft / artifact), optionally cap by mana value, pick up to N,
# move them to ``hand`` or ``battlefield``, and (optionally) apply a
# +1/+1 counter or enforce a sum-of-mana-value budget.
#
# Each legacy class survives as a thin sugar subclass that fixes the
# config in ``__post_init__``; this preserves their public name and
# constructor surface for parser-dispatch / test stability.
# ---------------------------------------------------------------


def _gy_card_matches(
    card,  # GameObject — has .printing
    *,
    creature: bool,
    artifact: bool,
    land: bool,
    nonland_permanent: bool,
    or_enchantment: bool,
    or_spacecraft: bool,
    or_artifact: bool,
    max_mv: int,
) -> bool:
    """Return whether a graveyard ``GameObject``'s printing satisfies
    the Return-from-graveyard filter.

    Three orthogonal axes:
      * primary type (one of creature / artifact / land /
        nonland_permanent),
      * widening flags (``or_enchantment``, ``or_spacecraft``,
        ``or_artifact``) that admit additional types alongside the
        primary,
      * ``max_mv`` cap (0 = unbounded).
    """
    p = card.printing
    if creature:
        primary_ok = (
            p.is_creature
            or (or_enchantment and p.is_enchantment)
            or (or_spacecraft and p.is_spacecraft)
            or (or_artifact and p.is_artifact)
        )
    elif artifact:
        primary_ok = p.is_artifact
    elif land:
        primary_ok = p.is_land
    elif nonland_permanent:
        primary_ok = not p.is_land
    else:
        primary_ok = False
    if not primary_ok:
        return False
    if max_mv > 0 and p.mana_cost.total > max_mv:
        return False
    return True


@dataclass(frozen=True)
class ReturnFromGraveyard(Effect):
    """Unified primitive: return up to ``max_n`` cards from a
    graveyard to ``destination`` (``"hand"`` or ``"battlefield"``).

    Filter axes (all defaults are off):

      * ``creature``, ``artifact``, ``land``, ``nonland_permanent`` —
        exactly one of these is the **primary** type; the other three
        stay False.
      * ``or_enchantment``, ``or_spacecraft``, ``or_artifact`` — widening
        flags that admit those extra types alongside the primary.
      * ``max_mv`` — per-card mana-value cap (0 = unbounded).

    Count axes:

      * ``min_n`` / ``max_n`` — range for the cast pipeline's target
        picker. ``max_n=1`` is a single-target effect.
      * ``max_total_mv`` — when > 0, enforces a SUM-of-mana-value
        budget across chosen targets at resolve time (Scout for
        Survivors). Picks past the budget are skipped with a log line.

    Move axis:

      * ``destination`` — ``"hand"`` (default) or ``"battlefield"``.
        Battlefield destination routes through ``put_into_battlefield``
        so ETB triggers fire normally.

    Rider axes:

      * ``plus_one_counter`` — when True, adds a +1/+1 counter to each
        creature returned to the battlefield (Scout for Survivors).
      * ``log_suffix`` — extra trailing text appended to the move log
        line (Petrified Field's "(Petrified Field)" tag).
      * ``untargeted_all`` — when True, the effect is mass-recursion
        (no targets selected; sweep every matching card). All other
        axes still apply (filter + move).

    Target kind:

      * ``target_kind`` — the Target tuple's first slot
        (``"graveyard_creature"`` / ``"graveyard_artifact"`` /
        ``"graveyard_land"``); used both for legality emission and
        resolve-time filtering.
    """

    # Filter — primary type
    creature: bool = False
    artifact: bool = False
    land: bool = False
    nonland_permanent: bool = False
    # Filter — widening
    or_enchantment: bool = False
    or_spacecraft: bool = False
    or_artifact: bool = False
    # Filter — mana value cap (0 = unbounded)
    max_mv: int = 0
    # Count
    min_n: int = 1
    max_n: int = 1
    max_total_mv: int = 0  # 0 = no sum-budget rider
    # Move
    destination: GraveyardReturnDestination = GraveyardReturnDestination.HAND
    # Rider
    plus_one_counter: bool = False
    log_suffix: str = ""
    untargeted_all: bool = False
    # Target kind
    target_kind: str = "graveyard_creature"

    # ----- targeting protocol -----

    def __post_init__(self) -> None:
        object.__setattr__(self, "destination", GraveyardReturnDestination(self.destination))

    def requires_target(self) -> bool:
        return not self.untargeted_all

    def target_count_range(self) -> tuple[int, int]:
        return (self.min_n, self.max_n)

    def _matches(self, card) -> bool:
        return _gy_card_matches(
            card,
            creature=self.creature,
            artifact=self.artifact,
            land=self.land,
            nonland_permanent=self.nonland_permanent,
            or_enchantment=self.or_enchantment,
            or_spacecraft=self.or_spacecraft,
            or_artifact=self.or_artifact,
            max_mv=self.max_mv,
        )

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        if self.untargeted_all:
            return []
        p = state.player_by_id(controller_id)
        return [
            (self.target_kind, c.id) for c in p.graveyard if self._matches(c)
        ]

    # ----- resolve -----

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        p = state.player_by_id(controller_id)
        if self.untargeted_all:
            self._resolve_all(state, controller_id)
            return
        if not targets:
            return
        # Walk targets in pick order. For ``max_n == 1`` only the first
        # legal target is applied (preserves single-target legacy
        # behavior). For multi-target variants we dedupe by id and
        # respect the optional sum-MV budget.
        if self.max_n == 1:
            kind, target_id = targets[0]
            if kind != self.target_kind:
                return
            card = next(
                (c for c in p.graveyard if c.id == target_id), None,
            )
            if card is None:
                return
            self._move_one(state, controller_id, card)
            return
        seen_ids: set[int] = set()
        remaining = self.max_total_mv
        for kind, target_id in targets:
            if kind != self.target_kind:
                continue
            if target_id in seen_ids:
                continue
            seen_ids.add(target_id)
            card = next(
                (c for c in p.graveyard if c.id == target_id), None,
            )
            if card is None:
                continue
            if self.max_total_mv > 0:
                mv = card.printing.mana_cost.total
                if mv > remaining:
                    state.log.append(
                        f"{card.printing.name} skipped — exceeds total-MV "
                        f"budget (remaining {remaining}, needs {mv})"
                    )
                    continue
                remaining -= mv
            self._move_one(state, controller_id, card)

    def _resolve_all(
        self, state: "GameState", controller_id: int
    ) -> None:
        """Mass-recursion sweep: every matching card in the controller's
        graveyard moves to ``destination``. Iterates a snapshot to
        survive mid-loop graveyard mutation from ETB triggers."""
        p = state.player_by_id(controller_id)
        cards = [c for c in list(p.graveyard) if self._matches(c)]
        for card in cards:
            if card not in p.graveyard:
                # Defensive: an ETB earlier in the loop already moved
                # this card.
                continue
            self._move_one(state, controller_id, card)

    def _move_one(
        self, state: "GameState", controller_id: int, card,
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        p.graveyard.remove(card)
        if self.destination == "battlefield":
            from mtgz_ir.actions import put_into_battlefield

            if self.plus_one_counter:
                state.log.append(
                    f"{card.printing.name} returns to the battlefield "
                    f"under P{controller_id} with a +1/+1 counter"
                )
            else:
                state.log.append(
                    f"{card.printing.name} returns from "
                    f"P{controller_id}'s graveyard to the battlefield"
                )
            put_into_battlefield(state, card, controller_id)
            if self.plus_one_counter:
                from mtgz_ir.game import add_plus_one_counters

                add_plus_one_counters(state, card, 1)
        else:
            # destination == "hand"
            from mtgz_ir.game import reveal_to_all

            card.zone = Zone.HAND
            p.hand.append(card)
            reveal_to_all(state, card)
            suffix = f" {self.log_suffix}" if self.log_suffix else ""
            state.log.append(
                f"{card.printing.name} returns from P{controller_id}'s "
                f"graveyard to hand{suffix}"
            )

    # ----- describe (subclasses override for printed-text fidelity) -----

    def describe(self) -> str:
        if self.untargeted_all:
            return (
                f"return all creature cards from your graveyard to the "
                f"{self.destination}"
            )
        type_phrase = "card"
        if self.creature:
            extras: list[str] = []
            if self.or_artifact:
                extras.append("artifact")
            if self.or_enchantment:
                extras.append("enchantment")
            if self.or_spacecraft:
                extras.append("Spacecraft")
            if extras:
                # Match legacy phrasing: "<extra> or creature" for
                # or_artifact, "creature or <extra>" otherwise.
                if "artifact" in extras and "Spacecraft" not in extras:
                    type_phrase = "artifact or creature card"
                elif "enchantment" in extras:
                    type_phrase = "creature or enchantment card"
                elif "Spacecraft" in extras:
                    type_phrase = "creature or Spacecraft card"
            else:
                type_phrase = "creature card"
        elif self.artifact:
            type_phrase = "artifact card"
        elif self.land:
            type_phrase = "land card"
        elif self.nonland_permanent:
            type_phrase = "nonland permanent cards"
        count = "target"
        if self.max_n > 1:
            if self.max_total_mv > 0:
                count = (
                    f"up to {self.max_n} target {type_phrase}s with "
                    f"total mana value {self.max_total_mv} or less"
                )
                tail = (
                    f" from your graveyard to the {self.destination}"
                )
                if self.plus_one_counter:
                    tail += " with a +1/+1 counter on each"
                return "return " + count + tail
            count = f"up to {self.max_n} target"
            type_phrase = type_phrase if type_phrase.endswith("s") else (
                type_phrase + "s"
            )
        elif self.max_mv > 0:
            return (
                f"return target {type_phrase} with mana value "
                f"{self.max_mv} or less from your graveyard to your "
                f"{self.destination}"
            )
        return (
            f"return {count} {type_phrase} from your graveyard to "
            f"{'your hand' if self.destination == 'hand' else 'the battlefield'}"
        )


@dataclass(frozen=True)
class ReturnCreatureFromYourGraveyardToBattlefield(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for
    "return target creature card from your graveyard to the battlefield".

    Inherits all base fields verbatim. ``__post_init__`` pins
    ``creature=True`` and ``destination="battlefield"``; the legacy
    rider flags ``or_enchantment``, ``or_spacecraft``, and ``max_mv``
    are already base fields and pass through directly.

    Legacy compatibility note: the pre-collapse class typed
    ``max_mv`` as ``int | None`` with ``None`` meaning unbounded.
    The base unifies on ``int`` with ``0`` meaning unbounded;
    ``__post_init__`` translates a passed ``None`` to ``0``.
    """

    def __post_init__(self) -> None:
        object.__setattr__(self, "creature", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.BATTLEFIELD)
        if self.max_mv is None:  # legacy callers may still pass None
            object.__setattr__(self, "max_mv", 0)

    def describe(self) -> str:
        type_phrase = (
            "creature or enchantment" if self.or_enchantment else "creature"
        )
        return (
            f"return target {type_phrase} card from your graveyard to the "
            f"battlefield"
        )



@dataclass(frozen=True)
class ReturnAllCreaturesFromYourGraveyardToBattlefield(ReturnFromGraveyard):
    """``ReturnFromGraveyard`` constructor sugar for the untargeted
    mass-reanimation surface — Awakening Hall's "Return all creature
    cards from your graveyard to the battlefield" when its door is
    unlocked.

    Each creature is moved out of the graveyard and routed through
    ``put_into_battlefield`` so its ETB triggers fire normally. Order
    of resolution follows the graveyard's stored order (oldest first);
    we don't model APNAP for the iteration since the controller is
    the same player throughout.
    """

    def __post_init__(self) -> None:
        object.__setattr__(self, "creature", True)
        object.__setattr__(self, "destination", GraveyardReturnDestination.BATTLEFIELD)
        object.__setattr__(self, "untargeted_all", True)

    def describe(self) -> str:
        return (
            "return all creature cards from your graveyard to the "
            "battlefield"
        )


@dataclass(frozen=True)
class PutCardFromGraveyardOnBottomOfLibrary(Effect):
    """Put target card from a graveyard on the bottom of its owner's
    library. Matches Chrome Companion's activated ability. Any
    graveyard is a legal target source (either player's).
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import Zone

        return [
            ("card_in_graveyard", c.id)
            for p in state.players
            for c in p.graveyard
            if c.zone == Zone.GRAVEYARD
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "card_in_graveyard":
            return
        for p in state.players:
            card = next((c for c in p.graveyard if c.id == target_id), None)
            if card is None:
                continue
            p.graveyard.remove(card)
            card.zone = Zone.LIBRARY
            p.library.append(card)  # bottom = end of list
            state.log.append(
                f"{card.printing.name} put on the bottom of "
                f"P{p.id}'s library"
            )
            from mtgz_ir.turn import note_cards_put_into_library

            note_cards_put_into_library(state, p.id)
            return

    def describe(self) -> str:
        return (
            "put target card from a graveyard on the bottom of its "
            "owner's library"
        )



@dataclass(frozen=True)
class ReanimateTargetCreatureCardFromDefendersGraveyard(Effect):
    """Put target creature card from a defending player's graveyard
    onto the battlefield under the resolving controller's control,
    tapped and attacking.

    Used by Chorale of the Void's attacks trigger: the source aura's
    enchanted creature attacks, and the trigger reanimates a creature
    card from the defending player's graveyard, on the active player's
    side, already attacking.

    Target kind: ``("graveyard_creature", id)``. The legal-target pool
    is the union of every opponent's graveyard creature cards. In
    2-player mode this matches the printed "defending player" exactly
    because the resolving controller has only one opponent; in
    multiplayer / planeswalker-defender cases this is an approximation
    until the engine threads per-attacker defender identity into a
    state slot the effect can read.

    Resolve semantics:

    1. Locate the chosen card in any opponent's graveyard; remove it.
    2. ``put_into_battlefield`` brings it on under the resolving
       controller's id (CR 405.5: "under your control" — the ETB is
       the new permanent's first ETB under that controller).
    3. ``obj.tapped = True``, ``obj.attacking = True``,
       ``obj.summoning_sick = False`` so the new permanent is a
       legal attacker for the in-progress combat phase. The engine's
       combat damage path reads ``obj.attacking`` and the defending
       player id from the calling combat phase (single-defender
       semantics; the new attacker rides the same defender as the
       triggering attacker).
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("graveyard_creature", c.id)
            for p in state.players
            if p.id != controller_id
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
            card = next(
                (c for c in p.graveyard if c.id == target_id), None,
            )
            if card is None:
                continue
            p.graveyard.remove(card)
            state.log.append(
                f"{card.printing.name} returns from P{p.id}'s graveyard "
                f"under P{controller_id} tapped and attacking"
            )
            put_into_battlefield(state, card, controller_id)
            card.tapped = True
            card.attacking = True
            card.summoning_sick = False
            return

    def describe(self) -> str:
        return (
            "put target creature card from defending player's "
            "graveyard onto the battlefield under your control tapped "
            "and attacking"
        )
