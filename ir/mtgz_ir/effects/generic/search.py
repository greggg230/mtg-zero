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
from mtgz_ir.effects.generic.roto_search import (
    library_search,
    searched_card_taken,
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
class SearchLibraryForCard(Effect):
    """Search your library for a card matching the named filter, reveal
    it, put it into your hand, then shuffle.

    ``filter_name`` selects the filter applied to cards in the library:
      - ``"legendary_creature"`` — creature with Legendary supertype
      - ``"basic_plains"`` / ``"basic_island"`` / ... — specific basic
      - ``"creature_small"`` — creature with mana value ≤ ``max_mv``
      - ``"artifact_small"`` — artifact card with mana value ≤ ``max_mv``
        (Tezzeret, Cruel Captain's ``-3`` tutor: ``max_mv=1``)

    If ``may_fail`` is True, the controller may decline to fetch
    (shuffle happens either way). Always shuffles at the end — matches
    MTG wording.
    """

    filter_name: LibraryCardFilter
    max_mv: int = 0
    may_fail: bool = True
    # ``filter_name="subtype"``: any card with one of these subtypes
    # (landcycling "Search your library for a Plains card", typecycling
    # "... for a Wizard card").
    subtypes: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        object.__setattr__(self, "filter_name", LibraryCardFilter(self.filter_name))

    def _matches(self, printing) -> bool:
        fn = self.filter_name
        if fn == "legendary_creature":
            return (
                printing.is_creature and "Legendary" in printing.supertypes
            )
        # Generic ``basic land card`` (any of the five basics) — Spineseeker
        # Centipede's ETB tutor. Distinct from the per-color
        # ``basic_plains`` / ``basic_island`` / ... filters which pin a
        # single basic by name.
        if fn == "basic_land":
            return printing.is_basic_land
        if fn.startswith("basic_"):
            wanted = fn[len("basic_"):].capitalize()
            return printing.is_basic_land and printing.name == wanted
        if fn == "creature_small":
            return (
                printing.is_creature
                and printing.mana_cost.total <= self.max_mv
            )
        if fn == "artifact":
            return printing.is_artifact
        # Tezzeret, Cruel Captain's -3 ability — "an artifact card with
        # mana value 1 or less". Artifact filter + an MV cap (reuses the
        # ``max_mv`` field the ``creature_small`` filter already uses).
        if fn == "artifact_small":
            return (
                printing.is_artifact
                and printing.mana_cost.total <= self.max_mv
            )
        if fn == "spacecraft":
            return printing.is_spacecraft
        if fn == "sliver":
            return "Sliver" in printing.subtypes
        # Demonic Counsel (Duskmourn) — tutor for a Demon creature
        # card. Mirrors the ``sliver`` shape (subtype-keyed filter on
        # creature printings); adding the filter here is purely
        # additive and reuses the existing tutor-via-subtype
        # convention.
        if fn == "demon":
            return "Demon" in printing.subtypes
        if fn == "subtype":
            return any(s in printing.subtypes for s in self.subtypes)
        return False

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, reveal_to_all, shuffle_library

        p = state.player_by_id(controller_id)
        options = [c for c in p.library if self._matches(c.printing)]
        if not options:
            state.log.append(
                f"P{controller_id} searches library — no "
                f"{self.filter_name} found"
            )
            shuffle_library(state, controller_id)
            return
        chosen_id = None
        if p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options]
            )
        if chosen_id is None:
            if self.may_fail:
                state.log.append(
                    f"P{controller_id} declines to fetch"
                )
                shuffle_library(state, controller_id)
                return
            chosen_id = options[0].id
        card = next((c for c in options if c.id == chosen_id), options[0])
        p.library.remove(card)
        if searched_card_taken(state, controller_id, card):
            shuffle_library(state, controller_id)
            return
        card.zone = Zone.HAND
        p.hand.append(card)
        state.log.append(
            f"P{controller_id} fetches {card.printing.name} to hand"
        )
        reveal_to_all(state, card)
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        what = (
            " or ".join(self.subtypes) if self.filter_name == "subtype"
            else self.filter_name
        )
        return (
            f"search your library for a {what} card, reveal "
            f"it, put it into your hand, then shuffle"
        )



@dataclass(frozen=True)
class SearchLibraryForBasicLandTapped(Effect):
    """Search the controller's library for up to ``count`` basic land
    cards, put them onto the battlefield tapped, then shuffle.

    Picks the FIRST ``count`` basic lands in library order (a
    deterministic stand-in for the player's free choice of which basic
    to fetch — the chosen card is hidden information, so picking the
    topmost match is strength-neutral). If fewer basic lands exist,
    puts what it finds.

    **Round-54 post-search shuffle restoration (SEARCH_LIMITATIONS
    #15 closure).** CR 701.19 / 701.22 require the library to be
    shuffled after a search, whether or not a card was found. Every
    sibling library-search Effect in this module
    (``SearchLibraryForCard`` / ``SearchLibraryForEquipmentToBattlefield``
    / ``SearchLibraryForGreenCreatureWithMVAtMostX``) already calls
    ``shuffle_library(state, controller_id)`` at the end of its
    ``resolve``; this Effect's prior omission was a Stage-3
    simplification (the now-removed "no extra shuffle" note). The Rust
    ``Effect::SearchLibraryForBasicLandTapped`` resolve arm has always
    shuffled post-search (SEARCH_LIMITATIONS #16). ``shuffle_library``
    calls ``state.rng.shuffle(p.library)``; when the cross-engine
    parity harness installs the SAME ``random.Random`` instance on
    ``state.rng`` (Python) and via ``GameState.set_external_rng`` (Rust
    — which Fisher-Yates over the identical ``_randbelow`` draw
    sequence per #16), the two engines' post-search library orders are
    byte-identical. In production (no shared rng installed) the shuffle
    is the CR-correct behavior with ``state.rng``'s own stream.
    """

    count: int = 1

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, shuffle_library
        from mtgz_ir.turn import fire_landfall_triggers

        p = state.player_by_id(controller_id)
        for _ in range(self.count):
            land = next(
                (c for c in p.library if c.printing.is_basic_land), None,
            )
            if land is None:
                state.log.append(
                    f"P{controller_id} searches but finds no more basic lands"
                )
                # CR 701.19 / 701.22: shuffle even when the search
                # finds nothing (matches the sibling search Effects'
                # no-match early-return shuffle, and the Rust arm which
                # always shuffles post-search). Mirrors #16.
                shuffle_library(state, controller_id)
                return
            p.library.remove(land)
            if searched_card_taken(state, controller_id, land):
                continue
            land.zone = Zone.BATTLEFIELD
            land.tapped = True
            # CR 302.1 — summoning sickness applies only to creatures;
            # a land entering via a search effect has no summoning
            # sickness. A library GameObject defaults
            # ``summoning_sick=True`` (game.py), so it must be cleared
            # explicitly here — the prior code left it stale, making
            # the searched land enter summoning-sick on Python while
            # the Rust ``Effect::SearchLibraryForBasicLandTapped`` arm
            # explicitly sets ``summoning_sick = false``. Round-54
            # cross-engine byte-equality fix (same resolve path as the
            # SEARCH_LIMITATIONS #15 shuffle restoration).
            land.summoning_sick = False
            land.controller_id = controller_id
            land.entered_battlefield_turn = state.turn_number
            state.battlefield.append(land)
            state.log.append(
                f"P{controller_id} puts {land.printing.name} onto the "
                f"battlefield tapped"
            )
            # ``fire_landfall_triggers`` covers both the legacy
            # string-key path and the predicate-style ENTERS dispatch.
            fire_landfall_triggers(state, land)
        # CR 701.19 / 701.22 post-search shuffle (SEARCH_LIMITATIONS
        # #15 closure, round 54). Byte-equal to the Rust arm's
        # post-search Fisher-Yates when a shared rng is threaded (#16).
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        if self.count == 1:
            return (
                "search your library for a basic land card, put it onto "
                "the battlefield tapped, then shuffle"
            )
        return (
            f"search your library for up to {self.count} basic land "
            f"cards, put them onto the battlefield tapped, then shuffle"
        )



@dataclass(frozen=True)
class SearchLibraryForLandCardToBattlefield(Effect):
    """Search your library for a land card of a named kind, put it onto
    the battlefield (``tapped`` if printed so), then shuffle.

    ``land_types`` — "a Mountain or Plains card" (the fetch lands):
    any card whose printed subtypes include one of the named land
    types. ``basic_only`` — "a basic land card" (Prismatic Vista). Both
    may combine; with neither set any land card qualifies.

    The controller's agent picks via ``choose_card_from_library``;
    ``None`` declines (a search for a card with a stated quality may
    fail to find, CR 701.19b). The found land enters through
    ``actions.apply_land_etb_replacements`` so its OWN printed entry
    rules apply (a fetched surveil land or triome enters tapped, a
    fetched shock land offers its life payment, a fetched fast land
    checks its land count), then its ENTERS triggers and landfall
    fire. The library is shuffled whether or not a card was found.
    """

    land_types: tuple[str, ...] = ()
    basic_only: bool = False
    tapped: bool = False

    def _matches(self, printing) -> bool:
        if not printing.is_land:
            return False
        if self.basic_only and not printing.is_basic_land:
            return False
        if self.land_types:
            return any(t in printing.subtypes for t in self.land_types)
        return True

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import (
            _trigger_etb_abilities, apply_land_etb_replacements,
        )
        from mtgz_ir.game import Zone, shuffle_library

        p = state.player_by_id(controller_id)
        options = [c for c in p.library if self._matches(c.printing)]
        chosen_id = None
        if options and p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
        card = next((c for c in options if c.id == chosen_id), None)
        if card is None:
            state.log.append(
                f"P{controller_id} searches library — finds no "
                f"{self._describe_kind()}"
            )
            shuffle_library(state, controller_id)
            return
        p.library.remove(card)
        if searched_card_taken(state, controller_id, card):
            shuffle_library(state, controller_id)
            return
        card.zone = Zone.BATTLEFIELD
        card.controller_id = controller_id
        card.summoning_sick = False
        apply_land_etb_replacements(
            state, controller_id, card, force_tapped=self.tapped,
        )
        card.entered_battlefield_turn = state.turn_number
        state.battlefield.append(card)
        p.lands_entered_this_turn += 1
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the "
            f"battlefield{' tapped' if card.tapped else ''}"
        )
        _trigger_etb_abilities(state, card)
        shuffle_library(state, controller_id)

    def _describe_kind(self) -> str:
        if self.land_types:
            kind = " or ".join(self.land_types)
        else:
            kind = "land"
        if self.basic_only:
            kind = "basic " + kind
        return kind + " card"

    def describe(self) -> str:
        tapped = " tapped" if self.tapped else ""
        return (
            f"search your library for a {self._describe_kind()}, put it "
            f"onto the battlefield{tapped}, then shuffle"
        )


@dataclass(frozen=True)
class SearchLibraryForEquipmentToBattlefield(Effect):
    """Search the controller's library for an Equipment card, put it
    onto the battlefield (untapped, under the controller's control),
    then shuffle.

    Models Honored Knight-Captain's activated tutor: ``{4}{W}{W},
    Sacrifice this creature: Search your library for an Equipment card,
    put it onto the battlefield, then shuffle.``

    The agent picks which Equipment via ``choose_card_from_library``.
    If no Equipment exists in library, the search fails and a shuffle
    still happens. The card enters via ``put_into_battlefield`` so any
    ETB triggers (e.g., the auto-attach riders on EoE Equipment) fire
    normally.
    """

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        p = state.player_by_id(controller_id)
        options = [
            c for c in p.library
            if "Equipment" in c.printing.subtypes
        ]
        if not options:
            state.log.append(
                f"P{controller_id} searches but finds no Equipment"
            )
            shuffle_library(state, controller_id)
            return
        chosen_id = None
        if p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options]
            )
        card = next(
            (c for c in options if c.id == chosen_id), options[0],
        )
        p.library.remove(card)
        if searched_card_taken(state, controller_id, card):
            shuffle_library(state, controller_id)
            return
        state.log.append(
            f"P{controller_id} fetches {card.printing.name} to "
            f"battlefield (Honored Knight-Captain)"
        )
        put_into_battlefield(state, card, controller_id)
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            "search your library for an Equipment card, put it onto "
            "the battlefield, then shuffle"
        )



@dataclass(frozen=True)
class SearchLibraryForGreenCreatureWithMVAtMostX(Effect):
    """Matches Green Sun's Zenith: "Search your library for a green
    creature card with mana value X or less, put it onto the battlefield,
    then shuffle." X is read from ``state.resolving_x_value``.

    Green creature: printing has "G" in ``mana_cost.colors``. Mana value
    uses ``mana_cost.total`` (includes the green pip). Declining to
    fetch is legal (matches the "search ... may fail" convention).
    """

    @library_search
    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_colors

        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import shuffle_library

        x = state.resolving_x_value
        p = state.player_by_id(controller_id)
        options = [
            c for c in p.library
            if c.printing.is_creature
            and "G" in effective_colors(state, c)
            and c.printing.mana_cost.total <= x
        ]
        if not options:
            state.log.append(
                f"P{controller_id} searches for a green creature with "
                f"MV ≤ {x} — none found"
            )
            shuffle_library(state, controller_id)
            return
        chosen_id = None
        if p.agent is not None:
            chosen_id = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options]
            )
        if chosen_id is None:
            state.log.append(
                f"P{controller_id} declines to fetch from Green Sun's Zenith"
            )
            shuffle_library(state, controller_id)
            return
        card = next((c for c in options if c.id == chosen_id), options[0])
        p.library.remove(card)
        if searched_card_taken(state, controller_id, card):
            shuffle_library(state, controller_id)
            return
        state.log.append(
            f"P{controller_id} fetches {card.printing.name} onto the battlefield"
        )
        put_into_battlefield(state, card, controller_id)
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            "search your library for a green creature card with mana "
            "value X or less, put it onto the battlefield, then shuffle"
        )
