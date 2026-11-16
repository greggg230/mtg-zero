"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.keyword_vocab import KeywordSpelling
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
from mtgz_ir.effects.generic.grant_keywords import GrantKeywords

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class AttachToTargetCreatureYouControl(Effect):
    """Attach the source (Equipment) to target creature the controller
    controls. Matches the common ETB trigger on EoE Equipment.

    Reads state.resolving_source for the Equipment. No-op if the source
    isn't an Equipment (is_equipment check on subtypes).

    Optional ``granted_keywords_ueot``: keywords the chosen target also
    gains until end of turn. Matches oracle riders like Squire's
    Lightblade's "...attach it to target creature you control. That
    creature gains first strike until end of turn." and Illvoi Light
    Jammer's "...gains hexproof until end of turn." Adding the keyword
    here (rather than via a separate Sequence sub-effect) keeps the
    target shared between attach and the UEOT grant. The grant is
    applied after the attach succeeds; if the attach fizzles, no
    keywords are granted.
    """

    granted_keywords_ueot: frozenset[KeywordSpelling] = frozenset()

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature and o.controller_id == controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        if "Equipment" not in source.printing.subtypes:
            return
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        target = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if target is None or not target.printing.is_creature:
            return
        if target.controller_id != controller_id:
            return
        source.attached_to = target_id
        state.log.append(
            f"{source.printing.name} is attached to {target.printing.name}"
        )
        if self.granted_keywords_ueot:
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn

            ApplyEffect(
                inner=GrantKeywords(keywords=self.granted_keywords_ueot),
                ends_at=EndOfTurn(),
            ).resolve(
                state, controller_id, (("creature", target.id),),
            )
            state.log.append(
                f"{target.printing.name} gains "
                f"{', '.join(sorted(self.granted_keywords_ueot))} "
                "until end of turn"
            )

    def describe(self) -> str:
        base = "attach this Equipment to target creature you control"
        if self.granted_keywords_ueot:
            return (
                f"{base}. That creature gains "
                f"{', '.join(sorted(self.granted_keywords_ueot))} "
                "until end of turn"
            )
        return base



@dataclass(frozen=True)
class EquipToTargetCreatureYouControl(AttachToTargetCreatureYouControl):
    """Equip activated-ability resolution effect (rule 702.6).

    Behaves identically to ``AttachToTargetCreatureYouControl`` (re-targets
    the source Equipment onto a creature the controller controls, replacing
    any prior attachment), but is given a distinct name and ``describe``
    string so logs and tooling can tell ETB-attach from a paid Equip
    activation. Pair with an ``ActivatedAbility`` whose cost is the printed
    Equip mana cost and ``sorcery_speed=True``.
    """

    def describe(self) -> str:
        return "equip: attach this Equipment to target creature you control"
