"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import (
    Effect, Target, TriggeredAbility,
    _apply_damage, _multicolored_permanents_count,
    _find_moonlit_replacement_source,
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
class MagicalHackTextChangeEffect(Effect):
    """Magical Hack: 'Change the text of target spell or permanent
    by replacing all instances of one basic land type with another.'

    Implementation scope: targets a basic land on the battlefield.
    Picks the new basic land type via the controller's
    ``choose_mana_color`` callback, mapped W/U/B/R/G to
    Plains/Island/Swamp/Mountain/Forest. The OLD basic land type is
    derived automatically from the target's effective subtypes
    (basic lands typically have exactly one). Writes a layer-3
    ``"replace_basic_land_type"`` registry entry sourced by the
    resolving spell with ``params=(old, new)`` and
    ``expires_at=None`` — text changes persist indefinitely (CR
    612). The sorcery itself goes to graveyard, so the entry is
    never auto-dropped.

    Limitations: only handles basic-land-type substitution on basic
    lands. Real Magical Hack also rewrites rules text on non-basic
    permanents that mention basic land types (e.g., a creature with
    "Forestwalk"); this engine doesn't yet model landwalk so the
    extension is a no-op there.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if o.printing.is_basic_land
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import (
            _BASIC_LAND_TYPE_TO_COLOR, effective_subtypes,
        )

        if not targets:
            return
        _, target_id = targets[0]
        target = next(
            (o for o in state.battlefield if o.id == target_id), None,
        )
        if target is None:
            return
        # Pick the OLD basic land type from the target's current
        # effective subtypes. Basic lands typically have exactly one
        # basic-land-type subtype.
        current_subs = effective_subtypes(state, target)
        old = next(
            (s for s in current_subs if s in _BASIC_LAND_TYPE_TO_COLOR),
            None,
        )
        if old is None:
            state.log.append(
                f"{target.printing.name} has no basic land type — "
                f"Magical Hack fizzles"
            )
            return
        # Pick the NEW basic land type via choose_mana_color, excluding
        # the OLD color (per Magical Hack: "ONE basic land type with
        # ANOTHER" implies they must differ).
        color_to_type = {
            v: k for k, v in _BASIC_LAND_TYPE_TO_COLOR.items()
        }
        old_color = _BASIC_LAND_TYPE_TO_COLOR[old]
        options = [c for c in ["W", "U", "B", "R", "G"] if c != old_color]
        chosen = state.players[controller_id].agent.choose_mana_color(
            state, controller_id, options,
        )
        new = color_to_type.get(chosen)
        if new is None or new == old:
            state.log.append(
                f"Magical Hack: invalid new basic land type, fizzling"
            )
            return
        src = state.resolving_source
        source_id = src.id if src is not None else target.id
        state.add_continuous_effect(
            source_id=source_id,
            layer=3,
            kind="replace_basic_land_type",
            target_id=target.id,
            params=(old, new),
            description=(
                f"Magical Hack rewrites {target.printing.name}: "
                f"{old} -> {new}"
            ),
        )
        state.log.append(
            f"Magical Hack rewrites {target.printing.name}: "
            f"{old} -> {new}"
        )

    def describe(self) -> str:
        return (
            "change the text of target spell or permanent by replacing "
            "all instances of one basic land type with another"
        )



@dataclass(frozen=True)
class SleightOfMindTextChangeEffect(Effect):
    """Sleight of Mind: 'Change the text of target spell or
    permanent by replacing all instances of one color word with
    another.' Layer 3 text-changing.

    Implementation scope: rewrites color words in
    ``protection_from_color`` on the target's printing. The OLD
    color is auto-derived from the target's current effective
    protection (the only color-word reference our engine
    currently models on permanents); the controller picks the NEW
    color via ``choose_mana_color``. Writes a layer-3
    ``"replace_color_word"`` registry entry sourced by the
    resolving spell with ``params=(old, new)`` and
    ``expires_at=None`` (text changes persist per CR 612).

    Limitations: doesn't rewrite color words inside other rules
    text (e.g., "Whenever a black creature ETBs, ..."). The engine
    doesn't have such color-keyed triggers in scope, so the
    extension is a no-op there.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # Permits any permanent on the battlefield. In practice
        # only the protection-from-color holders get observable
        # results; targeting elsewhere just writes a no-op entry.
        return [
            ("permanent", o.id) for o in state.battlefield
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_protection_from_colors

        if not targets:
            return
        _, target_id = targets[0]
        target = next(
            (o for o in state.battlefield if o.id == target_id), None,
        )
        if target is None:
            return
        # Derive the OLD color from the target's current effective
        # protection set. If protection is empty or has multiple
        # colors, we can't unambiguously rewrite — fizzle.
        prot = effective_protection_from_colors(state, target)
        if len(prot) != 1:
            state.log.append(
                f"Sleight of Mind fizzles: {target.printing.name} "
                f"has no single color word to rewrite"
            )
            return
        (old,) = prot
        options = [c for c in ["W", "U", "B", "R", "G"] if c != old]
        chosen = state.players[controller_id].agent.choose_mana_color(
            state, controller_id, options,
        )
        if chosen not in options:
            state.log.append(
                f"Sleight of Mind fizzles: invalid new color choice"
            )
            return
        src = state.resolving_source
        source_id = src.id if src is not None else target.id
        state.add_continuous_effect(
            source_id=source_id,
            layer=3,
            kind="replace_color_word",
            target_id=target.id,
            params=(old, chosen),
            description=(
                f"Sleight of Mind rewrites {target.printing.name}: "
                f"{old} -> {chosen}"
            ),
        )
        state.log.append(
            f"Sleight of Mind rewrites {target.printing.name}: "
            f"{old} -> {chosen}"
        )

    def describe(self) -> str:
        return (
            "change the text of target spell or permanent by replacing "
            "all instances of one color word with another"
        )
