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
class CopyTargetInstantOrSorceryYouControl(Effect):
    """Mirrorpool mode 1: "Copy target instant or sorcery spell you
    control. You may choose new targets for the copy."

    Simplification: rather than pushing a real stack copy (which would
    require mid-resolution target selection — not supported by the
    engine's cast-time target pipeline), we short-circuit by re-
    resolving the target spell's ``spell_effect`` with freshly-chosen
    targets. Functionally equivalent for anything our engine models.
    Close enough for flavor; counterspell-proofing isn't modeled.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        # Target key is the spell source's object id, NOT the stack
        # index — the cost-payment pipeline filters targets whose id
        # matches the ability source (``sacrifice_self``), so stack
        # indices (0, 1, …) would collide with low object ids.
        out: list[Target] = []
        for item in state.stack:
            if item.controller_id != controller_id:
                continue
            src = item.source
            if src is None:
                continue
            if not (src.printing.is_instant or src.printing.is_sorcery):
                continue
            out.append(("stack_spell", src.id))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, src_id = targets[0]
        if kind != "stack_spell":
            return
        item = next(
            (
                i for i in state.stack
                if i.source is not None and i.source.id == src_id
            ),
            None,
        )
        if item is None:
            return
        src = item.source
        if src is None:
            return
        spell_effect = src.printing.spell_effect
        if spell_effect is None:
            return
        # Choose fresh targets for the copy.
        copy_targets: tuple[Target, ...] = ()
        if spell_effect.requires_target():
            legal = spell_effect.legal_targets(state, controller_id)
            if legal:
                p = state.player_by_id(controller_id)
                if p.agent is not None:
                    chosen = p.agent.choose_target(
                        state, controller_id, legal
                    )
                    if chosen is not None:
                        copy_targets = (chosen,)
                if not copy_targets:
                    copy_targets = (legal[0],)
        state.log.append(
            f"Mirrorpool copies {src.printing.name}"
        )
        # Short-circuit: resolve the copy by re-running the spell
        # effect with the freshly-chosen target.
        prior = state.resolving_source
        state.resolving_source = src
        try:
            spell_effect.resolve(state, controller_id, copy_targets)
        finally:
            state.resolving_source = prior

    def describe(self) -> str:
        return (
            "copy target instant or sorcery spell you control; you may "
            "choose new targets for the copy"
        )



@dataclass(frozen=True)
class CounterTargetSpell(Effect):
    """Counter target spell on the stack.

    ``filter_types`` optionally restricts what card types can be
    countered (e.g., frozenset({"Artifact", "Enchantment"}) for Annul).
    Empty set means "any spell".

    Targets a stack item by its index via the new ``"stack"`` target kind.
    Triggered and activated abilities on the stack can't be countered by
    this effect (only spells — items where ``ability`` is None).

    Uncounterable spells are excluded from legal targets:
    - A spell whose printing has ``spell_is_uncounterable=True`` (e.g.
      Frenzied Baloth's "This spell can't be countered.").
    - A creature spell whose controller controls a permanent with a
      static ability carrying ``grants_creature_spells_uncounterable=True``
      (e.g. Frenzied Baloth on the battlefield granting the same
      protection to your other creature spells).

    CR 101.2: such spells can't be countered by spells or abilities.
    """

    filter_types: frozenset[str] = frozenset()

    def requires_target(self) -> bool:
        return True

    @staticmethod
    def _is_uncounterable(state: "GameState", item: "object") -> bool:
        """Return True if this stack item is immune to counter effects.

        Checks two sources of uncounterability:
        1. The spell's own printing carries ``spell_is_uncounterable``.
        2. The spell is a creature spell and the controller has a permanent
           on the battlefield with a static ability carrying
           ``grants_creature_spells_uncounterable``.
        """
        src = item.source.printing  # type: ignore[attr-defined]
        # Self-flag: "This spell can't be countered."
        if src.spell_is_uncounterable:
            return True
        # Battlefield static grant: "Spells you control can't be
        # countered." (Hexing Squelcher) -- every spell.
        spell_controller = item.controller_id  # type: ignore[attr-defined]
        for obj in state.battlefield:
            if obj.controller_id != spell_controller:
                continue
            for sa in obj.printing.static_abilities:
                if sa.grants_spells_uncounterable:
                    return True
        # Paid with "that spell can't be countered" mana (Delighted
        # Halfling).
        if getattr(item.source, "cast_uncounterable", False):
            return True
        # "Spells you control can't be countered this turn." (Veil of
        # Summer; Vintage Roto P5).
        from mtgz_ir.roto_board_statics import spells_uncounterable_now

        if spells_uncounterable_now(state, spell_controller):
            return True
        # Battlefield static grant: "Creature spells you control can't be
        # countered." Only applies to creature spells.
        if "Creature" in src.card_types:
            spell_controller = item.controller_id  # type: ignore[attr-defined]
            for obj in state.battlefield:
                if obj.controller_id != spell_controller:
                    continue
                for sa in obj.printing.static_abilities:
                    if sa.grants_creature_spells_uncounterable:
                        return True
        return False

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        out: list[Target] = []
        for i, item in enumerate(state.stack):
            if item.ability is not None:
                continue  # not a spell
            if item.countered:
                continue
            if self._is_uncounterable(state, item):
                continue  # CR 101.2: can't be countered
            src = item.source.printing
            if self.filter_types:
                if not any(t in src.card_types for t in self.filter_types):
                    continue
            out.append(("stack", i))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, idx = targets[0]
        if kind != "stack":
            return
        if not (0 <= idx < len(state.stack)):
            return
        item = state.stack[idx]
        if item.ability is not None:
            return
        # Re-check uncounterability at resolve time (the spell may have
        # gained protection between targeting and resolution, e.g. a
        # Frenzied Baloth entering the battlefield in response).
        if self._is_uncounterable(state, item):
            state.log.append(
                f"counterspell fizzles: "
                f"{item.source.printing.name} can't be countered"
            )
            return
        item.countered = True
        state.log.append(
            f"counterspell: {item.source.printing.name} is countered"
        )

    def describe(self) -> str:
        if not self.filter_types:
            return "counter target spell"
        types = ", ".join(sorted(t.lower() for t in self.filter_types))
        return f"counter target {types} spell"



@dataclass(frozen=True)
class TakeExtraTurnAfterThisOne(Effect):
    """"Take an extra turn after this one."

    Appends the controller to ``state.extra_turn_queue``; the sim loop
    in simulate.simulate_game consumes the queue between turns. Matches
    Nexus of Fate's extra-turn clause.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.extra_turn_queue.append(controller_id)
        state.log.append(
            f"P{controller_id} is scheduled to take an extra turn"
        )

    def describe(self) -> str:
        return "take an extra turn after this one"



@dataclass(frozen=True)
class PreventAllCombatDamageThisTurn(Effect):
    """"Prevent all combat damage that would be dealt this turn."

    Matches Darkness ({B} instant). Sets state-level flag consumed by
    ``combat._resolve_damage_step``; the flag is cleared at end-of-turn
    cleanup. Non-combat damage (direct damage, pings) is unaffected.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        state.prevent_combat_damage_this_turn = True
        state.log.append("all combat damage prevented this turn")

    def describe(self) -> str:
        return "prevent all combat damage that would be dealt this turn"
