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
from mtgz_ir.effects.generic.grant_keywords import GrantKeywords, GrantTriggeredAbility

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.effects.formulas import Formula
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class Pump(Effect):
    """Unified pump primitive — target gets ``+power_bonus/+toughness_bonus``
    and gains ``granted_keywords`` until end of turn.

    Sibling of :class:`Destroy` / :class:`Exile` / :class:`Tap` for the
    pump family. The unified resolve walks each slot, re-verifies
    legality at resolution per CR 608.2b, and stamps the bonus +
    keywords onto each surviving target's ``temp_*`` fields (cleared
    at cleanup). The pump amount and granted keywords are uniform
    across slots — multi-slot pumps with per-slot bonuses would need
    a different shape.

    Variants with bespoke amount (Spacecraft-power-X, draw-rider) or
    extra side effects (granted UEOT triggered ability, warp-cost
    rider, draw-if-counter rider) override ``resolve``; the bare
    ``Pump.resolve`` covers the simplest "target X gets +N/+M and
    gains KW UEOT" oracle shape.
    """

    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    targets: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        from mtgz_ir.effects.targets import SlotRef, TargetSpec

        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            for slot, chosen in zip(self.targets, targets):
                if isinstance(slot, SlotRef):
                    continue
                if not isinstance(slot, TargetSpec):
                    continue
                if not _slot_target_still_legal(state, slot, chosen):
                    state.log.append(
                        _fizzle_message(state, chosen, action="pump")
                    )
                    continue
                _, target_id = chosen
                obj = next(
                    (o for o in state.battlefield if o.id == target_id),
                    None,
                )
                if obj is None:
                    continue
                obj.temp_power_bonus += self.power_bonus
                obj.temp_toughness_bonus += self.toughness_bonus
                if self.granted_keywords:
                    from mtgz_ir.effects.combinators import ApplyEffect
                    from mtgz_ir.effects.durations import EndOfTurn

                    ApplyEffect(
                        inner=GrantKeywords(
                            keywords=self.granted_keywords,
                        ),
                        ends_at=EndOfTurn(),
                    ).resolve(state, controller_id, (chosen,))
                kw_note = (
                    f" and gains "
                    f"{', '.join(sorted(self.granted_keywords))}"
                    if self.granted_keywords else ""
                )
                state.log.append(
                    f"{obj.printing.name} gets "
                    f"+{self.power_bonus}/+{self.toughness_bonus}"
                    f"{kw_note} until end of turn"
                )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        parts: list[str] = []
        if self.power_bonus or self.toughness_bonus:
            parts.append(
                f"{_describe_targets(self.targets)} gets "
                f"+{self.power_bonus}/+{self.toughness_bonus}"
            )
        if self.granted_keywords:
            kw = ", ".join(sorted(self.granted_keywords))
            if parts:
                parts.append(f"gains {kw}")
            else:
                parts.append(
                    f"{_describe_targets(self.targets)} gains {kw}"
                )
        return " and ".join(parts) + " until end of turn."


@dataclass(frozen=True)
class PumpTargetOtherCreatureByXOverXWhereXIsSourcePower(Effect):
    """Target another creature you control gets +X/+X until end of turn,
    where X is the effect's source's effective power. Reads
    ``state.resolving_source``; target must be under the same
    controller and not be the source itself.

    Matches Hemosymbic Mite's becomes-tapped trigger.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        source = state.resolving_source
        source_id = source.id if source is not None else -1
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id == controller_id
            and o.id != source_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        source = state.resolving_source
        if source is None:
            return
        x = effective_power(state, source)
        if x <= 0:
            return
        if not targets:
            return
        kind, tid = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == tid), None
        )
        if obj is None:
            return
        state.grant_pt_pump_ueot(
            source_id=source.id, target_id=obj.id,
            power=x, toughness=x,
            description=(
                f"{source.printing.name} pumps {obj.printing.name} "
                f"+{x}/+{x} UEOT (source power)"
            ),
        )
        state.log.append(
            f"{obj.printing.name} gets +{x}/+{x} UEOT (source power)"
        )

    def describe(self) -> str:
        return (
            "another target creature you control gets +X/+X until end "
            "of turn, where X is this creature's power"
        )



@dataclass(frozen=True)
class DoubleTargetCreaturesPowerUEOT(Effect):
    """Target creature you control has its effective power doubled
    until end of turn. Implemented by adding the target's current
    effective power as a temp_power_bonus.

    Matches Mightform Harmonizer's landfall trigger. Simplification:
    the bonus is a one-time snapshot at resolution — further counter
    gains or other pumps don't re-double within the same turn.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id == controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        if not targets:
            return
        kind, tid = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == tid), None
        )
        if obj is None:
            return
        snapshot = effective_power(state, obj)
        if snapshot <= 0:
            return
        src = state.resolving_source
        state.grant_pt_pump_ueot(
            source_id=src.id if src is not None else obj.id,
            target_id=obj.id,
            power=snapshot, toughness=0,
            description=(
                f"{obj.printing.name}'s power doubled UEOT "
                f"(+{snapshot})"
            ),
        )
        state.log.append(
            f"{obj.printing.name}'s power is doubled UEOT "
            f"(+{snapshot} temp power)"
        )

    def describe(self) -> str:
        return "double the power of target creature you control until end of turn"



@dataclass(frozen=True)
class PumpTargetCreatureUntilEOT(Effect):
    """Target creature gets +power/+toughness and optionally gains keywords
    until end of turn.

    Matches oracle text like "Target creature gets +3/+0 and gains first
    strike and reach until end of turn." Bonuses stack additively and are
    cleared at cleanup. Negative bonuses model "target creature gets
    -N/-0" debuffs.

    Set ``optional=True`` for "up to one target creature" oracle text:
    the trigger / activation resolves as a no-op when no target is
    chosen / available (e.g. Dubious Delicacy's ETB).

    ``granted_triggered_ability`` (optional) composes
    ``ApplyEffect(GrantTriggeredAbility(ability=...), EndOfTurn())``
    against the chosen target so the rider lives on the layer-6
    registry with ``expires_at="cleanup"``. Use for cards whose pump
    rider also grants "Whenever ~ does X, Y" until EOT.
    """

    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    you_control_only: bool = False  # restrict to "target creature you control"
    opponents_only: bool = False  # restrict to "an opponent controls"
    # Extra keywords granted UEOT only when the target was cast for its
    # warp cost (oracle: "If that creature was cast for its warp cost, it
    # also gains X"). Generic — any future card with the same rider uses it.
    granted_keywords_if_warp_cast: frozenset[KeywordSpelling] = frozenset()
    # Conditional draw rider: "If [target] has a +1/+1 counter on it,
    # draw a card." Matches Dual-Sun Technique. The controller of the
    # spell does the drawing. Inspected at resolution time.
    draw_if_target_has_plus_one_counter: int = 0
    # Target subtype filter: when set, only creatures whose effective
    # subtypes (printed plus any layer-3/4 registry-granted entries via
    # ``effective_subtypes``) include this value are legal targets.
    # Matches oracle text like "Target Blinkmoth creature gets +1/+1
    # until end of turn" (Blinkmoth Nexus — the animated Nexus carries
    # the Blinkmoth subtype as a registry entry written by the
    # ``ApplyEffect(BecomeType, EndOfTurn())`` manland animation).
    target_subtype: str | None = None
    # Granted triggered ability composed via
    # ``ApplyEffect(GrantTriggeredAbility(...), EndOfTurn())`` so the
    # rider lives on the layer-6 registry with
    # ``expires_at="cleanup"``. Matches oracle text like Requiem
    # Monolith's "Until end of turn, target creature gains 'Whenever
    # this creature is dealt damage, you draw that many cards and lose
    # that much life.'"
    granted_triggered_ability: "TriggeredAbility | None" = None
    # "Up to one target" semantics — the trigger / activation resolves
    # as a no-op when no target is chosen or available (e.g. Dubious
    # Delicacy's ETB "up to one target creature gets -3/-3 UEOT").
    optional: bool = False
    # "Another target creature ..." — exclude state.resolving_source from
    # legal targets and from resolve. Matches oracle wording on cards
    # like Perigee Beckoner's ETB ("another target creature you control
    # gets +2/+0 and gains [dies-rider]").
    exclude_self: bool = False
    # "Target attacking creature ..." — restrict the legal-target pool to
    # creatures currently flagged ``attacking=True``. Matches the DSK
    # "Whenever you attack" rider on Most Valuable Slayer
    # ("...target attacking creature gets +1/+0 and gains first strike
    # until end of turn"). Independent of the controller filters
    # (``you_control_only`` / ``opponents_only``); attackers are always
    # the active player's so in practice the source's controller is
    # also the attackers' controller, but keeping the flag orthogonal
    # mirrors the engine class's other narrowing-flag conventions.
    attacking_only: bool = False
    # "Target legendary creature ..." — restrict the legal-target pool to
    # creatures whose printing carries the Legendary supertype. Matches
    # Plaza of Heroes' "{3},{T},Exile this land: Target legendary creature
    # gains hexproof and indestructible until end of turn." Oracle text
    # is explicit: only creatures with the Legendary supertype are valid
    # targets. Independent of all other narrowing flags.
    legendary_only: bool = False
    # "Target creature gets +X/+0 until end of turn" where X is a
    # cost-payment-determined value read from ``state.resolving_x_value``
    # (set by the X-cost activation pipeline in ``_activate_ability`` —
    # the same channel ``{X}`` mana costs and the new "tap X untapped
    # artifacts you control" cost atom feed). When True, the effective
    # power bonus at resolve is ``state.resolving_x_value`` (clamped to
    # >= 0) rather than the static ``power_bonus``. Wires Secluded
    # Starforge's "{2}, {T}, Tap X untapped artifacts you control:
    # Target creature gets +X/+0 until end of turn." Mirrors the
    # ``PumpSelfXPowerUEOT`` X-channel convention on a TARGET creature.
    power_bonus_from_x: bool = False

    def requires_target(self) -> bool:
        return not self.optional

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1) if self.optional else (1, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import effective_subtypes

        source = state.resolving_source if self.exclude_self else None
        from mtgz_ir.game import is_creature_now

        source_id = source.id if source is not None else -1
        return [
            ("creature", o.id)
            for o in state.battlefield
            if is_creature_now(o)
            and (not self.you_control_only or o.controller_id == controller_id)
            and (not self.opponents_only or o.controller_id != controller_id)
            and (not self.attacking_only or o.attacking)
            and (not self.legendary_only or o.printing.is_legendary)
            and (
                self.target_subtype is None
                or self.target_subtype in effective_subtypes(state, o)
            )
            and o.id != source_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        src = state.resolving_source
        # When power_bonus_from_x is set, the power bonus is the
        # cost-determined X (clamped to >= 0), read from the same
        # resolving_x_value channel {X} mana costs use. The toughness
        # bonus stays static (Secluded Starforge is +X/+0).
        effective_power = self.power_bonus
        if self.power_bonus_from_x:
            effective_power = max(0, state.resolving_x_value)
        if effective_power or self.toughness_bonus:
            state.grant_pt_pump_ueot(
                source_id=src.id if src is not None else obj.id,
                target_id=obj.id,
                power=effective_power,
                toughness=self.toughness_bonus,
                description=(
                    f"{src.printing.name if src else obj.printing.name} "
                    f"pumps {obj.printing.name} "
                    f"+{effective_power}/+{self.toughness_bonus} UEOT"
                ),
            )
        granted = self.granted_keywords
        if self.granted_keywords_if_warp_cast and obj.cast_via_warp:
            granted = granted | self.granted_keywords_if_warp_cast
        if granted:
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn

            ApplyEffect(
                inner=GrantKeywords(keywords=granted),
                ends_at=EndOfTurn(),
            ).resolve(
                state, controller_id, (("creature", obj.id),),
            )
        if self.granted_triggered_ability is not None:
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn

            ApplyEffect(
                inner=GrantTriggeredAbility(
                    ability=self.granted_triggered_ability,
                ),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, (("creature", obj.id),))
        state.log.append(
            f"{obj.printing.name} gets "
            f"+{effective_power}/+{self.toughness_bonus} until end of turn"
            + (f" and gains {', '.join(sorted(granted))}"
               if granted else "")
            + (" and gains a granted triggered ability"
               if self.granted_triggered_ability is not None else "")
        )
        if (
            self.draw_if_target_has_plus_one_counter > 0
            and obj.plus_one_counters > 0
        ):
            from mtgz_ir.turn import draw_n_cards
            draw_n_cards(
                state, controller_id,
                self.draw_if_target_has_plus_one_counter,
            )
            state.log.append(
                f"P{controller_id} draws "
                f"{self.draw_if_target_has_plus_one_counter} "
                f"(target had +1/+1 counter)"
            )

    def describe(self) -> str:
        parts: list[str] = []
        if self.power_bonus_from_x:
            parts.append(
                f"target creature gets +X/+{self.toughness_bonus}"
            )
        elif self.power_bonus or self.toughness_bonus:
            parts.append(
                f"target creature gets +{self.power_bonus}/+{self.toughness_bonus}"
            )
        if self.granted_keywords:
            parts.append(
                f"gains {', '.join(sorted(self.granted_keywords))}"
            )
        if self.granted_triggered_ability is not None:
            parts.append(
                f"gains \"{self.granted_triggered_ability.describe()}\""
            )
        return " and ".join(parts) + " until end of turn."


@dataclass(frozen=True)
class SetBasePTUntilEOT(Effect):
    """Target creature has base power and toughness N/N until end of turn.

    CR layer 7b: sets the base P/T of the target creature to ``power``
    / ``toughness``, overriding its printed (or otherwise established)
    base. The effect is recorded as a layer-7 sublayer-1 registry entry
    with ``expires_at="cleanup"`` via
    ``GameState.grant_set_base_pt_ueot``. Any layer-7c additive bonuses
    (anthems, pump spells, +1/+1 counters) still stack on top.

    Parameterized over ``power`` and ``toughness`` so a single class
    covers every "has base power and toughness N/N until end of turn"
    surface: Genemorph Imago's 3/3 / 6/6 arms, and any future printing
    with a different static value.
    """

    power: int
    toughness: int
    # "up to one" — declining a target is legal (count_min 0).
    optional: bool = False
    # "other" — the resolving source can't be the target (Restless
    # Vinestalk's attack trigger: "up to one other target creature").
    exclude_self: bool = False

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1) if self.optional else (1, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list["Target"]:
        from mtgz_ir.game import is_creature_now

        src = state.resolving_source if self.exclude_self else None
        return [
            ("creature", o.id)
            for o in state.battlefield
            if is_creature_now(o) and (src is None or o.id != src.id)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple["Target", ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        src = state.resolving_source
        state.grant_set_base_pt_ueot(
            source_id=src.id if src is not None else obj.id,
            target_id=obj.id,
            power=self.power,
            toughness=self.toughness,
            description=(
                f"{src.printing.name if src else '?'} sets "
                f"{obj.printing.name} base P/T to "
                f"{self.power}/{self.toughness} UEOT"
            ),
        )
        state.log.append(
            f"{obj.printing.name} has base power and toughness "
            f"{self.power}/{self.toughness} until end of turn"
        )

    def describe(self) -> str:
        return (
            f"target creature has base power and toughness "
            f"{self.power}/{self.toughness} until end of turn"
        )


@dataclass(frozen=True)
class SetBasePTConditionalUntilEOT(Effect):
    """Target creature has base power and toughness N/N until end of turn;
    if you control ``land_threshold`` or more lands, M/M instead.

    Models Genemorph Imago's landfall trigger:
      "target creature has base power and toughness 3/3 until end of
      turn. If you control six or more lands, that creature has base
      power and toughness 6/6 until end of turn instead."

    At resolve time counts the lands the effect's controller controls.
    If the count meets or exceeds ``land_threshold``, applies
    ``upgrade_power``/``upgrade_toughness``; otherwise applies
    ``default_power``/``default_toughness``. Only ONE registry entry is
    written (the winning branch) so the most-recent-timestamp 7b rule
    sees a single clean entry.

    Parameterized over all four stat values and the threshold so a
    future printing with different numbers claims via the same class.
    """

    default_power: int
    default_toughness: int
    land_threshold: int
    upgrade_power: int
    upgrade_toughness: int

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (1, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list["Target"]:
        from mtgz_ir.game import is_creature_now

        return [
            ("creature", o.id)
            for o in state.battlefield
            if is_creature_now(o)
        ]

    def _land_count(self, state: "GameState", controller_id: int) -> int:
        return sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_land
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple["Target", ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        lands = self._land_count(state, controller_id)
        if lands >= self.land_threshold:
            p, t = self.upgrade_power, self.upgrade_toughness
        else:
            p, t = self.default_power, self.default_toughness
        src = state.resolving_source
        state.grant_set_base_pt_ueot(
            source_id=src.id if src is not None else obj.id,
            target_id=obj.id,
            power=p,
            toughness=t,
            description=(
                f"{src.printing.name if src else '?'} sets "
                f"{obj.printing.name} base P/T to {p}/{t} UEOT "
                f"(lands={lands})"
            ),
        )
        state.log.append(
            f"{obj.printing.name} has base power and toughness "
            f"{p}/{t} until end of turn "
            f"(you control {lands} land(s))"
        )

    def describe(self) -> str:
        return (
            f"target creature has base power and toughness "
            f"{self.default_power}/{self.default_toughness} until end of turn; "
            f"if you control {self.land_threshold} or more lands, "
            f"{self.upgrade_power}/{self.upgrade_toughness} instead"
        )


@dataclass(frozen=True)
class PumpSelfUntilEOT(Effect):
    """The ability's source gets +power/+toughness (and optional keywords)
    until end of turn. Reads state.resolving_source."""

    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: frozenset[KeywordSpelling] = frozenset()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        if self.power_bonus or self.toughness_bonus:
            state.grant_pt_pump_ueot(
                source_id=source.id, target_id=source.id,
                power=self.power_bonus, toughness=self.toughness_bonus,
                description=(
                    f"{source.printing.name} pumps itself "
                    f"+{self.power_bonus}/+{self.toughness_bonus} UEOT"
                ),
            )
        if self.granted_keywords:
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn

            ApplyEffect(
                inner=GrantKeywords(keywords=self.granted_keywords),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, ())
        state.log.append(
            f"{source.printing.name} gets "
            f"+{self.power_bonus}/+{self.toughness_bonus} until end of turn"
        )

    def describe(self) -> str:
        return (
            f"this gets +{self.power_bonus}/+{self.toughness_bonus} "
            f"until end of turn"
        )



@dataclass(frozen=True)
class PumpCreaturesYouControlUntilEOT(Effect):
    """Each creature the controller controls gets +power/+toughness until
    end of turn (applied at resolution; creatures entering later don't get
    the bonus — Stage-3 simplification vs. a continuous effect).

    ``exclude_source=True`` models "other creatures you control" — reads
    ``state.resolving_source`` to skip the triggering permanent.

    ``attacking_only=True`` narrows the scope to attacking creatures
    regardless of controller — "Attacking creatures get +1/+0 until
    end of turn" (Contested War Zone). Attackers are always the
    active player's, so in practice this picks up your own attackers
    during your turn.

    ``also_untap_if_not_your_turn=True`` models the Zealous Display
    rider — "If it's not your turn, untap those creatures." When the
    spell resolves on an opponent's turn, every creature that received
    the pump is also untapped, letting them block (or, with vigilance/
    haste, attack again next turn fresh).
    """

    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    exclude_source: bool = False
    attacking_only: bool = False
    also_untap_if_not_your_turn: bool = False
    # "Creature tokens you control ..." (Ravenous Robots) — only tokens.
    tokens_only: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source if self.exclude_source else None
        untap_rider_active = (
            self.also_untap_if_not_your_turn
            and state.players[state.active_player_idx].id != controller_id
        )
        count = 0
        untapped_count = 0
        for obj in state.battlefield:
            if self.attacking_only:
                if not obj.attacking:
                    continue
            else:
                if obj.controller_id != controller_id:
                    continue
                if not obj.printing.is_creature:
                    continue
            if source is not None and obj is source:
                continue
            if self.tokens_only and "Token" not in obj.printing.supertypes:
                continue
            if self.power_bonus or self.toughness_bonus:
                src = state.resolving_source
                state.grant_pt_pump_ueot(
                    source_id=src.id if src is not None else obj.id,
                    target_id=obj.id,
                    power=self.power_bonus, toughness=self.toughness_bonus,
                    description=(
                        f"{src.printing.name if src else obj.printing.name} "
                        f"pumps {obj.printing.name} "
                        f"+{self.power_bonus}/+{self.toughness_bonus} UEOT"
                    ),
                )
            if self.granted_keywords:
                from mtgz_ir.effects.combinators import ApplyEffect
                from mtgz_ir.effects.durations import EndOfTurn

                ApplyEffect(
                    inner=GrantKeywords(keywords=self.granted_keywords),
                    ends_at=EndOfTurn(),
                ).resolve(
                    state, controller_id, (("creature", obj.id),),
                )
            if untap_rider_active and obj.tapped:
                obj.tapped = False
                untapped_count += 1
            count += 1
        if self.attacking_only:
            scope = "attacking creatures"
        elif self.exclude_source:
            scope = "other creatures"
        else:
            scope = "creatures"
        state.log.append(
            f"P{controller_id}'s {scope} ({count}) get "
            f"+{self.power_bonus}/+{self.toughness_bonus} until end of turn"
        )
        if untap_rider_active and untapped_count:
            state.log.append(
                f"P{controller_id}'s {scope} ({untapped_count}) untap "
                f"(not P{controller_id}'s turn)"
            )

    def describe(self) -> str:
        if self.attacking_only:
            scope = "attacking creatures"
        elif self.exclude_source:
            scope = "other creatures you control"
        else:
            scope = "creatures you control"
        return (
            f"{scope} get "
            f"+{self.power_bonus}/+{self.toughness_bonus} until end of turn"
        )



@dataclass(frozen=True)
class PumpCreaturesOpponentsControlUntilEOT(Effect):
    """Each creature an opponent of the controller controls gets
    +power/+toughness until end of turn (applied at resolution;
    creatures entering later don't get the bonus — Stage-3 snapshot vs.
    a true continuous effect).

    Negative bonuses model debuffs like "creatures your opponents
    control get -3/-0 until end of turn" (one-time snapshot).

    Set ``until_caster_next_turn=True`` for the "until your next turn"
    variant (Mouth of the Storm). The P/T bonus is written via
    ``grant_pt_pump_until_player_next_turn`` (layer-7 sublayer-2
    ``pump_pt`` registry entry, ``expires_at="controller_untap"``,
    ``expires_at_player_id`` = caster). The keyword grant rides
    ``ApplyEffect(GrantKeywords(...), EndOfYourNextTurn())`` and writes
    a layer-6 ``grant_keyword`` registry entry under the same lifetime.
    Both expire at the start (``run_untap_step``) of the caster's NEXT
    turn — the opponent's full subsequent turn keeps the debuff applied.
    """

    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: frozenset[str] = frozenset()
    until_caster_next_turn: bool = False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        count = 0
        for obj in state.battlefield:
            if obj.controller_id == controller_id:
                continue
            if not obj.printing.is_creature:
                continue
            if self.until_caster_next_turn:
                src = state.resolving_source
                if self.power_bonus or self.toughness_bonus:
                    state.grant_pt_pump_until_player_next_turn(
                        source_id=src.id if src is not None else obj.id,
                        target_id=obj.id,
                        power=self.power_bonus,
                        toughness=self.toughness_bonus,
                        until_player_id=controller_id,
                        description=(
                            f"opp creature {obj.printing.name} pumped "
                            f"{self.power_bonus:+d}/{self.toughness_bonus:+d} "
                            f"until P{controller_id}'s next turn"
                        ),
                    )
                if self.granted_keywords:
                    from mtgz_ir.effects.combinators import ApplyEffect
                    from mtgz_ir.effects.durations import EndOfYourNextTurn

                    ApplyEffect(
                        inner=GrantKeywords(
                            keywords=self.granted_keywords,
                        ),
                        ends_at=EndOfYourNextTurn(),
                    ).resolve(
                        state, controller_id, (("creature", obj.id),),
                    )
            else:
                if self.power_bonus or self.toughness_bonus:
                    src = state.resolving_source
                    state.grant_pt_pump_ueot(
                        source_id=src.id if src is not None else obj.id,
                        target_id=obj.id,
                        power=self.power_bonus,
                        toughness=self.toughness_bonus,
                        description=(
                            f"opp creature {obj.printing.name} pumped "
                            f"{self.power_bonus:+d}/{self.toughness_bonus:+d} UEOT"
                        ),
                    )
                if self.granted_keywords:
                    from mtgz_ir.effects.combinators import ApplyEffect
                    from mtgz_ir.effects.durations import EndOfTurn

                    ApplyEffect(
                        inner=GrantKeywords(
                            keywords=self.granted_keywords,
                        ),
                        ends_at=EndOfTurn(),
                    ).resolve(
                        state, controller_id, (("creature", obj.id),),
                    )
            count += 1
        # Format signed bonuses ("+3"/"-3" rather than "+-3").
        p_str = f"{self.power_bonus:+d}"
        t_str = f"{self.toughness_bonus:+d}"
        scope = (
            f"until P{controller_id}'s next turn"
            if self.until_caster_next_turn
            else "until end of turn"
        )
        state.log.append(
            f"P{controller_id}'s opponents' creatures ({count}) get "
            f"{p_str}/{t_str} {scope}"
        )

    def describe(self) -> str:
        p_str = f"{self.power_bonus:+d}"
        t_str = f"{self.toughness_bonus:+d}"
        scope = (
            "until your next turn"
            if self.until_caster_next_turn
            else "until end of turn"
        )
        return (
            f"creatures your opponents control get {p_str}/{t_str} "
            f"{scope}"
        )



@dataclass(frozen=True)
class PumpSelfXPowerUEOT(Effect):
    """Source gets +X/+0 until end of turn, where X comes from
    ``state.resolving_x_value`` (set by the X-cost activation pipeline
    in ``_activate_ability``).

    Wires Lavaclaw Reaches' granted "{X}: This creature gets +X/+0 UEOT"
    while animated.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        x = state.resolving_x_value
        if x <= 0:
            return
        source.temp_power_bonus += x
        state.log.append(
            f"{source.printing.name} gets +{x}/+0 until end of turn"
        )

    def describe(self) -> str:
        return "this creature gets +X/+0 until end of turn"


@dataclass(frozen=True)
class SwitchSourcePowerToughnessUEOT(Effect):
    """Switch the source's power and toughness until end of turn.

    Implementation: writes a layer-7 sublayer-4 ``"switch_pt"``
    registry entry sourced by the source object with
    ``expires_at="cleanup"``. The entry carries no params — each
    such entry contributes a single swap, walked in timestamp order
    by ``_compute_effective_pt``. Two switches cancel; three
    switches is one swap.

    Per CR 613.4 layer 7e, switch operates on the post-7b/c/d
    effective P/T — so a Lignified manland (0/4 from layer 7b) with
    a +1/+1 counter (7d) becomes 1/5 before any switch, and 5/1
    after one switch. That matches MTG's "switch power and
    toughness" semantics on a creature whose P/T has already been
    modified by other effects.

    Wires Wandering Fumarole's granted "{0}: Switch this creature's
    power and toughness until end of turn" while animated. The {0}
    cost means the ability is freely re-activatable; each activation
    appends another switch entry, toggling the displayed P/T.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        state.add_continuous_effect(
            source_id=source.id,
            layer=7, sublayer=4, kind="switch_pt",
            target_id=source.id,
            params=(),
            expires_at="cleanup",
            description=(
                f"{source.printing.name} P/T switched UEOT"
            ),
        )
        state.log.append(
            f"{source.printing.name}'s power and toughness are switched "
            f"until end of turn"
        )

    def describe(self) -> str:
        return (
            "switch this creature's power and toughness until end of turn"
        )



@dataclass(frozen=True)
class PumpTargetArtifactOrCreatureUntilEOT(Pump):
    """``Pump`` constructor sugar for "target artifact or creature gets
    +N/+M and gains KW UEOT" (Reroute Systems mode 1, Mental
    Modulation's pump arm, …).

    Filter is ``IsArtifact() | IsCreature()``. The unified
    ``Pump.resolve`` re-verifies the slot's filter at resolution per
    CR 608.2b — a target that lost both relevant card types between
    cast and resolve fails the recheck and the pump is skipped.
    """

    def __post_init__(self) -> None:
        from mtgz_ir.effects.object_predicates import IsArtifact, IsCreature
        from mtgz_ir.effects.targets import TargetSpec
        object.__setattr__(
            self, "targets",
            (TargetSpec(IsArtifact() | IsCreature()),),
        )

    def describe(self) -> str:
        parts: list[str] = []
        if self.power_bonus or self.toughness_bonus:
            parts.append(
                f"target artifact or creature gets "
                f"+{self.power_bonus}/+{self.toughness_bonus}"
            )
        if self.granted_keywords:
            kw = ", ".join(sorted(self.granted_keywords))
            if parts:
                parts.append(f"gains {kw}")
            else:
                parts.append(
                    f"target artifact or creature gains {kw}"
                )
        return " and ".join(parts) + " until end of turn."



@dataclass(frozen=True)
class ThreatenTargetCreatureUEOT(Effect):
    """Generic Threaten core: "Gain control of target creature until end
    of turn. Untap it. It gains haste until end of turn."

    Decomposes the three anaphoric sentences into one targeted Effect
    so multi-sentence parser literals can compose this with riders via
    ``Sequence`` (e.g., Vengeful Possession adds an
    ``IfYouDo(MayDo(DiscardSelf(1)), DrawCards(1))`` looter rider after
    this core resolves).

    Control reverts on cleanup via the layer-2 control_change registry
    (``state.grant_control_change_ueot``); haste rides
    ``ApplyEffect(GrantKeywords({"haste"}), EndOfTurn())`` for the
    layer-6 grant. Source-name in the log lines is pulled from
    ``state.resolving_source.printing.name`` so the same class fires
    correctly for any spell whose printed text matches the Threaten
    core (currently Vengeful Possession; future cards with the same
    surface get this for free).

    Target slot: ``("creature", id)`` — for the wider
    artifact-or-creature variant (Systems Override), use a separate
    threaten primitive or pass an alternate target filter.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        src = state.resolving_source
        spell_id = src.id if src is not None else obj.id
        source_name = (
            src.printing.name if src is not None else "Threaten effect"
        )
        state.grant_control_change_ueot(
            source_id=spell_id,
            target_id=obj.id,
            new_controller_id=controller_id,
            description=(
                f"{source_name}: P{controller_id} controls "
                f"{obj.printing.name} UEOT"
            ),
        )
        obj.tapped = False
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn

        ApplyEffect(
            inner=GrantKeywords(keywords=frozenset({"haste"})),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", obj.id),))
        state.log.append(
            f"P{controller_id} takes control of {obj.printing.name} "
            f"UEOT; untaps + gains haste"
        )

    def describe(self) -> str:
        return (
            "gain control of target creature until end of turn; "
            "untap it; it gains haste until end of turn"
        )



@dataclass(frozen=True)
class PairedTargetPumpDebuffUEOT(Effect):
    """Two required creature targets, opposite-polarity UEOT deltas
    keyed off a shared ``Formula``.

    Slot 0 ("positive"): a creature matching ``positive_kind`` —
    typically ``"my_creature"`` for "target creature you control".
    Slot 1 ("negative"): a creature matching ``negative_kind`` —
    typically ``"creature"`` for "target creature an opponent controls".
    Per-slot kind enforcement (via ``target_slot_kinds``) prevents an
    agent from filling both slots from the same pool.

    At resolve time, X is computed via ``formula.value(state,
    controller_id)``. The positive slot gets ``+X*positive_power /
    +X*positive_toughness``; the negative slot gets ``-X*-negative_power
    / -X*-negative_toughness`` (signed multiplication preserves the
    "+X/+0 vs -0/-X" Monoist shape — pass ``positive_power=1,
    positive_toughness=0, negative_power=0, negative_toughness=-1``).

    Both slots are REQUIRED (per CR 601.2c the trigger doesn't go on
    the stack unless both can be filled). ``legal_targets`` returns
    ``[]`` if either side has no candidate; the firing path's
    no-legal-targets gate then fizzles before the trigger lands on the
    stack — no half-effect leak.

    Re-validates each chosen target at resolve time per CR 608.2b: a
    target that's left the battlefield or changed control is illegal
    and skipped, and the OTHER half still resolves.
    """

    formula: "Formula | None" = None
    # Sign multipliers for each slot — apply X * sign to the per-slot
    # delta. Default to +X/+0 (positive) and -0/-X (negative), the
    # Monoist Circuit-Feeder shape; cards with different polarities
    # parameterize via the constructor.
    positive_power: int = 1
    positive_toughness: int = 0
    negative_power: int = 0
    negative_toughness: int = -1
    # Per-slot target kinds. Defaults match Monoist
    # ("target creature you control" + "target creature an opponent
    # controls").  A future card with different kind constraints (e.g.
    # "target attacking creature" + "target creature defending player
    # controls") swaps these without touching the resolve path.
    positive_kind: str = "my_creature"
    negative_kind: str = "creature"

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def target_slot_kinds(self) -> tuple[str, ...]:
        return (self.positive_kind, self.negative_kind)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        positive = [
            (self.positive_kind, o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id == controller_id
        ]
        negative = [
            (self.negative_kind, o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id != controller_id
        ]
        # Both slots required: empty pool on either side fizzles the
        # trigger before it can be put on the stack.
        if not positive or not negative:
            return []
        return positive + negative

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if self.formula is None:
            return
        x = self.formula.value(state, controller_id)
        src = state.resolving_source
        source_id = src.id if src is not None else -1
        # Locate one entry per slot. Order is agent-dependent, so
        # search by kind rather than index.
        pos_target = next(
            ((k, t) for (k, t) in targets if k == self.positive_kind),
            None,
        )
        neg_target = next(
            ((k, t) for (k, t) in targets if k == self.negative_kind),
            None,
        )
        if pos_target is not None:
            obj = next(
                (o for o in state.battlefield if o.id == pos_target[1]),
                None,
            )
            # Re-validate per CR 608.2b: still a creature, still under
            # the resolving controller's control.
            if (
                obj is not None
                and obj.controller_id == controller_id
                and obj.printing.is_creature
            ):
                p_delta = self.positive_power * x
                t_delta = self.positive_toughness * x
                if p_delta or t_delta:
                    state.grant_pt_pump_ueot(
                        source_id=source_id,
                        target_id=obj.id,
                        power=p_delta,
                        toughness=t_delta,
                        description=(
                            f"{obj.printing.name} gets "
                            f"{_signed(p_delta)}/{_signed(t_delta)} UEOT"
                        ),
                    )
                    state.log.append(
                        f"{obj.printing.name} gets "
                        f"{_signed(p_delta)}/{_signed(t_delta)} UEOT"
                    )
        if neg_target is not None:
            obj = next(
                (o for o in state.battlefield if o.id == neg_target[1]),
                None,
            )
            if (
                obj is not None
                and obj.controller_id != controller_id
                and obj.printing.is_creature
            ):
                p_delta = self.negative_power * x
                t_delta = self.negative_toughness * x
                if p_delta or t_delta:
                    state.grant_pt_pump_ueot(
                        source_id=source_id,
                        target_id=obj.id,
                        power=p_delta,
                        toughness=t_delta,
                        description=(
                            f"{obj.printing.name} gets "
                            f"{_signed(p_delta)}/{_signed(t_delta)} UEOT"
                        ),
                    )
                    state.log.append(
                        f"{obj.printing.name} gets "
                        f"{_signed(p_delta)}/{_signed(t_delta)} UEOT"
                    )

    def describe(self) -> str:
        return (
            f"until end of turn, target creature you control gets "
            f"{_signed(self.positive_power)}X/"
            f"{_signed(self.positive_toughness)}X and target creature "
            f"an opponent controls gets "
            f"{_signed(self.negative_power)}X/"
            f"{_signed(self.negative_toughness)}X"
        )


@dataclass(frozen=True)
class PumpYoursAndDebuffOpponentByArtifactCountUntilEOT(Effect):
    """**Cross-engine bridge sibling of**
    :class:`PairedTargetPumpDebuffUEOT` **for Monoist Circuit-Feeder's
    ETB** — DETERMINISTIC first-creature pick, no agent target choice.

    Monoist Circuit-Feeder (real EoE card, 14,727 17lands decks):
    "When this creature enters, until end of turn, target creature you
    control gets +X/+0 and target creature an opponent controls gets
    -0/-X, where X is the number of artifacts you control."

    This class is the EXACT analogue of
    :class:`mtgz_ir.effects.generic.damage.FightTargetCreatureWith
    TargetCreature` (the deterministic-pick Rust-unit-variant bridge
    sibling) vs the agent-driven
    :class:`mtgz_ir.effects.bespoke...EnchantedCreatureFightsTarget
    Effect`. The real Monoist Circuit-Feeder printing parses (via
    ``cards/parser/effects.py``) to the AGENT-DRIVEN, two-REQUIRED-
    target :class:`PairedTargetPumpDebuffUEOT` (the controller freely
    picks ANY legal own creature + ANY legal opponent creature, CR
    601.2c). That class stays the real-card target. THIS class exists
    purely as the cross-engine bridge target for Rust's unit-variant
    ``Effect::TwoSubjectPumpAndDebuffByArtifactCount`` (which picks
    BOTH targets deterministically at apply time with no decision
    callback — the established campaign no-decision-callback
    convention, ``rust/src/state_ops_resolve.rs``).

    **Why NOT loader-admitted (§2).** Routing the real card's
    agent-driven targeting through this deterministic bridge in the
    production-deck loader would silently degrade free agent target
    choice to a fixed first-creature pick in multi-creature board
    states. Per the Fight precedent the deterministic sibling is NOT
    production-deck-loader-admitted; the genuine real-EoE probe drives
    the actual ``PairedTargetPumpDebuffUEOT`` constrained by a picker
    shim so the agent's two-target pick is forced to the FIRST own /
    FIRST opponent creature, byte-equal with this class's scan. See
    ``docs/SEARCH_LIMITATIONS.md``.

    **Mechanism (mirrors the Rust resolve arm BIT-FOR-BIT).**
      * ``x = sum(1 for o in state.battlefield if o.controller_id ==
        controller_id and o.printing.is_artifact)`` — the SAME
        artifact-count read :class:`DealDamageToOpponentCreatureBy
        ArtifactCount` uses (damage.py:445-448) and the SAME
        denominator the parser's ``CountControlledMatching(
        IsArtifact())`` formula computes.
      * ``x = max(0, x)``; if ``x <= 0`` → silent no-op (matches the
        Rust ``if x <= 0`` short-circuit and the round-32..49
        X-from-count zero convention).
      * Source-side target = FIRST creature ``o`` in
        ``state.battlefield`` insertion order with
        ``o.controller_id == controller_id`` (smallest list index —
        same scan :class:`FightTargetCreatureWithTargetCreature` and
        Rust's resolve use). If present → ``grant_pt_pump_ueot(power=
        x, toughness=0)``.
      * Opponent-side target = FIRST creature with
        ``o.controller_id != controller_id``. If present →
        ``grant_pt_pump_ueot(power=0, toughness=-x)``.
      * Per-side INDEPENDENT (CR 608.2c — two independent "target"
        clauses, NOT "up to one"): a missing side is a silent no-op
        and the other side still applies. Matches the Rust arm's
        per-side independence.

    End-of-turn cleanup is the standard layer-7 ``"pump_pt"``
    ``expires_at="cleanup"`` registry expiry (same path
    :class:`PumpTargetCreatureUntilEOT` / :class:`PairedTargetPump
    DebuffUEOT` use).

    Unit-shape ``@dataclass(frozen=True)`` with no constructor args
    (X is dynamic — the dynamic-X is the whole point of the class;
    mirrors the round-32..49 unit-shape convention and Rust's unit
    variant).
    """

    def requires_target(self) -> bool:
        # Deterministic-pick shape: no explicit target choice. Mirrors
        # Rust's first-slice no-decision-callback convention (same as
        # FightTargetCreatureWithTargetCreature).
        return False

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now

        x = sum(
            1
            for o in state.battlefield
            if o.controller_id == controller_id and is_artifact_now(state, o)
        )
        x = max(0, x)
        if x <= 0:
            return
        src = state.resolving_source
        source_id = src.id if src is not None else -1
        # Source-side target: FIRST creature the controller controls
        # (battlefield insertion order — smallest index). Mirrors the
        # Rust resolve scan + FightTargetCreatureWithTargetCreature.
        own = next(
            (
                o
                for o in state.battlefield
                if o.controller_id == controller_id
                and o.printing.is_creature
            ),
            None,
        )
        # Opponent-side target: FIRST creature an opponent controls.
        opp = next(
            (
                o
                for o in state.battlefield
                if o.controller_id != controller_id
                and o.printing.is_creature
            ),
            None,
        )
        # Per-side INDEPENDENT apply (CR 608.2c). A missing side is a
        # silent no-op; the other side still applies.
        if own is not None:
            state.grant_pt_pump_ueot(
                source_id=source_id,
                target_id=own.id,
                power=x,
                toughness=0,
                description=(
                    f"{own.printing.name} gets +{x}/+0 UEOT"
                ),
            )
            state.log.append(f"{own.printing.name} gets +{x}/+0 UEOT")
        if opp is not None:
            state.grant_pt_pump_ueot(
                source_id=source_id,
                target_id=opp.id,
                power=0,
                toughness=-x,
                description=(
                    f"{opp.printing.name} gets +0/-{x} UEOT"
                ),
            )
            state.log.append(f"{opp.printing.name} gets +0/-{x} UEOT")

    def describe(self) -> str:
        return (
            "until end of turn, target creature you control gets "
            "+X/+0 and target creature an opponent controls gets "
            "-0/-X, where X is the number of artifacts you control"
        )


def _signed(n: int) -> str:
    """Format a signed integer with an explicit ``+`` for non-negative
    values. ``+0`` reads more naturally than ``0`` in pump/debuff
    descriptions ("gets +X/+0" vs "gets X/0")."""
    return f"+{n}" if n >= 0 else str(n)
