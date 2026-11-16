"""Dynamic power/toughness formulas -- the values of
``CardPrinting.dynamic_pt_formula``.

Every formula is a frozen dataclass deriving from :class:`PTFormula` and
answers ``base_pt(state, obj) -> (power, toughness)``, the creature's
base P/T before counters and other layer-7 modifications. The engine
reads it in ``game._dynamic_base_pt``; ``None`` on the printing means
"use the printed P/T".

They cover characteristic-defining abilities (CR 604.3, layer 7a:
Lhurgoyfs, Cosmogoyf, Harmonious Grovestrider, Territorial Kavu,
Adeline), leveler P/T (CR 711.2b, ``LevelerPT``) and Emissary Escort's
"+X/+0" rider, which the engine folds into the base P/T.

Formula classes that live next to their cards' other primitives also
derive from :class:`PTFormula`: ``cube_creatures_wg.PowerEqualsCount``,
``layout_effects.LevelerPT`` and ``roto_board.CardTypesInYourGraveyardPT``.
The card IR (``mtgz_ir.cards.ir``) emits them like any other class.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover
    from mtgz_ir.game import GameObject, GameState


class PTFormula:
    """Base of every ``dynamic_pt_formula`` value."""

    def base_pt(self, state: "GameState", obj: "GameObject") -> tuple[int, int]:
        raise NotImplementedError  # pragma: no cover - abstract


@dataclass(frozen=True)
class CardsYouOwnInExilePT(PTFormula):
    """"<This>'s power is equal to the number of cards you own in exile
    and its toughness is equal to that number plus K." (Cosmogoyf).
    "You" is the creature's controller; ``Player.exile`` holds the
    cards that player owns."""

    toughness_plus: int = 1

    def base_pt(self, state, obj):
        n = len(state.player_by_id(obj.controller_id).exile)
        return n, n + self.toughness_plus


@dataclass(frozen=True)
class LandsYouControlPT(PTFormula):
    """"<This>'s power and toughness are each equal to the number of
    lands you control." (Harmonious Grovestrider)."""

    def base_pt(self, state, obj):
        n = sum(
            1 for o in state.battlefield
            if o.controller_id == obj.controller_id and o.printing.is_land
        )
        return n, n


@dataclass(frozen=True)
class CardTypesInAllGraveyardsPT(PTFormula):
    """"<This>'s power is equal to the number of card types among cards
    in all graveyards and its toughness is equal to that number plus K."
    (Tarmogoyf, Barrowgoyf, Pyrogoyf). Card types are counted by the
    shared ``effects.card_types`` helper (CR 205.2a, split / double-faced
    cards outside the battlefield)."""

    toughness_plus: int = 1

    def base_pt(self, state, obj):
        from mtgz_ir.effects.card_types import count_card_types

        n = count_card_types(c for pl in state.players for c in pl.graveyard)
        return n, n + self.toughness_plus


_BASIC_LAND_TYPES = ("Plains", "Island", "Swamp", "Mountain", "Forest")


@dataclass(frozen=True)
class DomainPT(PTFormula):
    """"Domain -- <This>'s power and toughness are each equal to the
    number of basic land types among lands you control." (Territorial
    Kavu). Reads effective subtypes, so a land given a basic land type
    by a layer-4 effect counts."""

    def base_pt(self, state, obj):
        from mtgz_ir.game import effective_subtypes

        seen: set[str] = set()
        for o in state.battlefield:
            if o.controller_id != obj.controller_id or not o.printing.is_land:
                continue
            seen.update(
                s for s in effective_subtypes(state, o) if s in _BASIC_LAND_TYPES
            )
        n = len(seen)
        return n, n


@dataclass(frozen=True)
class PowerPlusGreatestManaValueOfOtherArtifactsPT(PTFormula):
    """"This creature gets +X/+0, where X is the greatest mana value
    among other artifacts you control." (Emissary Escort). Folded into
    the base: printed power + X, printed toughness."""

    def base_pt(self, state, obj):
        from mtgz_ir.game import is_artifact_now

        mvs = [
            o2.printing.mana_cost.total
            for o2 in state.battlefield
            if o2.controller_id == obj.controller_id
            and is_artifact_now(state, o2)
            and o2.id != obj.id
        ]
        x = max(mvs) if mvs else 0
        return (obj.printing.power or 0) + x, obj.printing.toughness or 0


__all__ = [
    "CardTypesInAllGraveyardsPT",
    "CardsYouOwnInExilePT",
    "DomainPT",
    "LandsYouControlPT",
    "PTFormula",
    "PowerPlusGreatestManaValueOfOtherArtifactsPT",
]
