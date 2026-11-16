"""Card-specific one-off effects.

Each class here encodes the unique behavior of a single EoE card (ETB
triggers, attack triggers, activated abilities, end-step reanimations,
etc.). When a mechanic becomes reusable across cards, extract it to
``generic.py`` instead.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import (
    Effect, Target,
    _apply_damage, _multicolored_permanents_count,
    _find_moonlit_replacement_source,
)

if TYPE_CHECKING:
    from mtgz_ir.cards import ManaCost
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class AstelliReclaimerETBEffect(Effect):
    """Astelli Reclaimer's ETB: "Return target noncreature, nonland
    permanent card with mana value X or less from your graveyard to
    the battlefield, where X is the amount of mana spent to cast this
    creature."

    X is computed from the resolving source's paid cost: warp cost if
    cast_via_warp is set (for warp = {2}{W} → X = 3), otherwise the
    printed mana cost's total (for {3}{W}{W} → X = 5).
    """

    def requires_target(self) -> bool:
        return True

    def _x_from_source(self, state: "GameState") -> int:
        source = state.resolving_source
        if source is None:
            return 0
        if source.cast_via_warp and source.printing.warp_cost is not None:
            return source.printing.warp_cost.total
        return source.printing.mana_cost.total

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        x = self._x_from_source(state)
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_permanent", c.id)
            for c in p.graveyard
            if c.printing.is_permanent
            and not c.printing.is_creature
            and not c.printing.is_land
            and c.printing.mana_cost.total <= x
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "graveyard_permanent":
            return
        p = state.player_by_id(controller_id)
        card = next((c for c in p.graveyard if c.id == target_id), None)
        if card is None:
            return
        p.graveyard.remove(card)
        state.log.append(
            f"{card.printing.name} returns from P{controller_id}'s "
            f"graveyard to the battlefield (Astelli Reclaimer)"
        )
        put_into_battlefield(state, card, controller_id)

    def describe(self) -> str:
        return (
            "return target noncreature, nonland permanent card with "
            "mana value X or less from your graveyard to the "
            "battlefield, where X is the mana spent to cast this"
        )



@dataclass(frozen=True)
class FamishedWorldsireETBEffect(Effect):
    """Famished Worldsire's ETB: "Look at the top X cards of your
    library, where X is this creature's power. Put any number of land
    cards from among them onto the battlefield tapped, then shuffle."

    Reads state.resolving_source to compute X from the source's
    effective power (post-Devour counters). Picks all land cards from
    the top-X (a strong simplification — the oracle allows choosing
    "any number", but greedy land-picking is strictly ramp-positive).
    Non-land cards go back into the library (bottom, random order).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, effective_power
        from mtgz_ir.turn import fire_landfall_triggers

        source = state.resolving_source
        if source is None:
            return
        x = effective_power(state, source)
        if x <= 0:
            return
        p = state.player_by_id(controller_id)
        examined = p.library[:x]
        lands = [c for c in examined if c.printing.is_land]
        rest = [c for c in examined if c not in lands]
        for land in lands:
            p.library.remove(land)
            land.zone = Zone.BATTLEFIELD
            land.tapped = True
            land.controller_id = controller_id
            land.entered_battlefield_turn = state.turn_number
            state.battlefield.append(land)
            state.log.append(
                f"P{controller_id} puts {land.printing.name} onto the "
                f"battlefield tapped (Famished Worldsire)"
            )
            # ``fire_landfall_triggers`` covers both the legacy
            # string-key path and the predicate-style ENTERS dispatch.
            fire_landfall_triggers(state, land)
        # Shuffle what's left (rest goes back where it was, then the
        # whole library is shuffled per oracle).
        from mtgz_ir.game import shuffle_library
        shuffle_library(state, controller_id)

    def describe(self) -> str:
        return (
            "look at the top X cards of your library, where X is this "
            "creature's power; put any number of land cards from among "
            "them onto the battlefield tapped, then shuffle"
        )



@dataclass(frozen=True)
class TannukEnsignLandfallEffect(Effect):
    """Tannuk, Memorial Ensign's landfall: "Tannuk deals 1 damage to
    each opponent. If this is the second time this ability has
    resolved this turn, draw a card."

    Tracks resolutions per source in state.ability_resolutions_this_turn
    (reset at cleanup). The draw fires only on the exact 2nd resolution
    this turn; 3rd, 4th, etc. get the damage but not the draw.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        # Damage each opponent 1 (log even without a source).
        from mtgz_ir.game import player_damage_prevented
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            if player_damage_prevented(state, p.id):
                continue
            p.life -= 1
            state.log.append(
                f"Tannuk deals 1 to P{p.id} (life: {p.life})"
            )
        if source is None:
            return
        key = ("tannuk_landfall", source.id)
        count = state.ability_resolutions_this_turn.get(key, 0) + 1
        state.ability_resolutions_this_turn[key] = count
        if count == 2:
            from mtgz_ir.turn import draw_n_cards

            draw_n_cards(state, controller_id, 1)
            state.log.append(
                f"Tannuk landfall second-time bonus: P{controller_id} "
                f"draws a card"
            )

    def describe(self) -> str:
        return (
            "Tannuk deals 1 damage to each opponent; if this is the "
            "second time this ability has resolved this turn, draw a "
            "card"
        )



@dataclass(frozen=True)
class SelfcraftMechanETBEffect(Effect):
    """Selfcraft Mechan's ETB: "You may sacrifice an artifact. When you
    do, put a +1/+1 counter on target creature and draw a card."

    The "may" is exposed via ``agent.choose_yes_no`` — if the controller
    declines (or has no artifact), the entire reflexive rider does
    nothing: no sacrifice, no counter, no draw. Target creature is
    selected via the agent's ``choose_target`` before resolve (via the
    standard target-selection pipeline).
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
        from mtgz_ir.game import is_artifact_now
        from mtgz_ir.turn import draw_n_cards, sacrifice as _sacrifice

        p = state.player_by_id(controller_id)
        artifacts = [
            o for o in state.battlefield
            if o.controller_id == controller_id and is_artifact_now(state, o)
        ]
        if not artifacts:
            state.log.append(
                f"Selfcraft Mechan: no artifact to sacrifice — fizzles"
            )
            return
        # Optional "may sacrifice an artifact" — controller decides.
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, "Sacrifice an artifact?"
        ):
            state.log.append(
                f"P{controller_id} declines to sacrifice an artifact "
                f"(Selfcraft Mechan)"
            )
            return
        # Agent picks which artifact to sacrifice.
        if p.agent is not None:
            pick_id = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in artifacts]
            )
            chosen = next(
                (o for o in artifacts if o.id == pick_id), artifacts[0]
            )
        else:
            chosen = artifacts[0]
        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name} "
            f"(Selfcraft Mechan)"
        )
        _sacrifice(state, chosen)
        # Apply counter to chosen target creature (if still on battlefield).
        if targets:
            kind, tid = targets[0]
            if kind == "creature":
                tgt = next(
                    (o for o in state.battlefield if o.id == tid), None
                )
                if tgt is not None:
                    from mtgz_ir.game import add_plus_one_counters
                    add_plus_one_counters(state, tgt, 1)
                    state.log.append(
                        f"+1/+1 counter on {tgt.printing.name} "
                        f"(now {tgt.plus_one_counters})"
                    )
        # Draw a card either way — the sacrifice happened, so the
        # reflexive trigger's effect resolves fully.
        draw_n_cards(state, controller_id, 1)

    def describe(self) -> str:
        return (
            "you may sacrifice an artifact; if you do, put a +1/+1 "
            "counter on target creature and draw a card"
        )



# NOTE: ``SotheraExileOpponentCreatureEffect`` was deleted in
# audit-per-opponent-choice. Sothera's DIES trigger ("Whenever a creature
# you control dies, each opponent chooses a creature they control and
# exiles it.") now decomposes into the generic
# ``EachOpponentExilesOwnCreature(link_to_source=True)`` primitive (in
# ``effects/generic.py``) via the unified parser grammar.
#
# NOTE: ``SotheraEndStepReanimateEffect`` was deleted in
# audit-sothera-foundfootage. Sothera's end-step trigger ("At the
# beginning of your end step, if a player controls no creatures,
# sacrifice Sothera, then put a creature card exiled with it onto the
# battlefield under your control with two additional +1/+1 counters on
# it.") now decomposes into the generic primitives
# ``ReanimateOneExiledWithSourceWithCounters(counter_amount=2)`` and
# ``SacrificeSelfPreservingExileLink`` (both in ``effects/generic.py``)
# composed in a ``Sequence`` (recall first, sacrifice second — see
# ``Sequence`` ordering rationale in the parser-side recognizer
# comment). The intervening-if gate "if a player controls no creatures"
# resolves through the new ``StateBound(AnyPlayerControlsNoCreatures())``
# entry in ``_INTERVENING_IF_PHRASES``.


@dataclass(frozen=True)
class TerritorialBruntarLandfallEffect(Effect):
    """Territorial Bruntar's landfall: "Exile cards from the top of
    your library until you exile a nonland card. You may cast that
    card this turn."

    Any lands revealed during the process stay exiled — the oracle
    doesn't specify returning them, just exiles until non-land. We
    mark the non-land with playable_from_exile_this_turn so
    legal_actions will surface a CastSpell for it.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        while p.library:
            top = p.library.pop(0)
            top.zone = Zone.EXILE
            p.exile.append(top)
            state.log.append(
                f"P{controller_id} exiles {top.printing.name} (Bruntar)"
            )
            if not top.printing.is_land:
                top.playable_from_exile_this_turn = True
                state.log.append(
                    f"{top.printing.name} may be cast this turn"
                )
                return

    def describe(self) -> str:
        return (
            "exile cards from the top of your library until you exile "
            "a nonland card; you may cast that card this turn"
        )




@dataclass(frozen=True)
class PainForAllETBEffect(Effect):
    """Pain for All's ETB (as an aura): "Enchanted creature deals
    damage equal to its power to any other target."

    "Any other target" (creature or player, excluding the source).
    Reads state.resolving_source for the aura; `aura.attached_to` is
    the enchanted creature; damage amount = enchanted creature's
    effective power at resolution.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        aura = state.resolving_source
        enchanted_id = aura.attached_to if aura is not None else None
        out: list[Target] = []
        for p in state.players:
            if p.has_lost:
                continue
            out.append(("player", p.id))
        for o in state.battlefield:
            if not o.printing.is_creature:
                continue
            if o.id == enchanted_id:
                continue
            out.append(("creature", o.id))
        return out

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        aura = state.resolving_source
        if aura is None or aura.attached_to is None:
            return
        enchanted = next(
            (o for o in state.battlefield if o.id == aura.attached_to),
            None,
        )
        if enchanted is None:
            return
        if not targets:
            return
        kind, target_id = targets[0]
        amount = effective_power(state, enchanted)
        if amount <= 0:
            return
        if kind == "player":
            from mtgz_ir.game import player_damage_prevented
            if player_damage_prevented(state, target_id):
                return
            victim = state.player_by_id(target_id)
            victim.life -= amount
            state.log.append(
                f"{enchanted.printing.name} deals {amount} to "
                f"P{target_id} (life: {victim.life})"
            )
        elif kind == "creature":
            creature = next(
                (o for o in state.battlefield if o.id == target_id), None
            )
            if creature is None:
                return
            creature.damage += amount
            creature.damaged_this_turn = True
            state.log.append(
                f"{enchanted.printing.name} deals {amount} to "
                f"{creature.printing.name}"
            )

    def describe(self) -> str:
        return (
            "enchanted creature deals damage equal to its power to any "
            "other target"
        )



@dataclass(frozen=True)
class PainForAllDamageEchoEffect(Effect):
    """Pain for All's damage-echo: "Whenever enchanted creature is
    dealt damage, it deals that much damage to each opponent."

    Reads state.resolving_combat_damage_amount for the incoming amount.
    Simplified: each opponent loses ``amount`` life (modeling "deals
    damage" as life loss for simplicity in the non-creature-target
    case).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        amount = state.resolving_combat_damage_amount
        if amount <= 0:
            return
        from mtgz_ir.game import player_damage_prevented
        for p in state.players:
            if p.id == controller_id or p.has_lost:
                continue
            if player_damage_prevented(state, p.id):
                continue
            p.life -= amount
            state.log.append(
                f"Pain for All echoes {amount} to P{p.id} "
                f"(life: {p.life})"
            )

    def describe(self) -> str:
        return (
            "enchanted creature deals that much damage to each "
            "opponent"
        )



@dataclass(frozen=True)
class TractorBeamETBEffect(Effect):
    """Tractor Beam's ETB triggered ability: ``When this Aura enters,
    tap enchanted permanent.``

    The static "you control enchanted permanent" clause is applied
    separately in the aura-resolution path: ``_resolve_top_of_stack``
    reads the printing's ``attached_grants_control`` flag and swaps
    the target's controller BEFORE this ETB trigger goes on the stack
    — static abilities are continuous, not stack-based. The "doesn't
    untap during its controller's untap step" static is wired via
    ``attached_prevents_untap=True`` and queried at the untap step.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import tap_permanent

        aura = state.resolving_source
        if aura is None or aura.attached_to is None:
            return
        target = next(
            (o for o in state.battlefield if o.id == aura.attached_to),
            None,
        )
        if target is None:
            return
        tap_permanent(state, target)
        state.log.append(
            f"Tractor Beam taps {target.printing.name}"
        )

    def describe(self) -> str:
        return "tap enchanted permanent"



@dataclass(frozen=True)
class EnchantedCreatureFightsTargetEffect(Effect):
    """Meltstrider's Resolve's ETB: "When this Aura enters, enchanted
    creature fights up to one target creature an opponent controls."

    The fighter is the Aura's enchanted creature (read from
    ``state.resolving_source.attached_to``), not a player-chosen friendly
    creature. "Up to one" makes the target optional — with no opponent
    creatures around, the spell still resolves (and the Aura still
    attaches). When both fighters are present, each deals damage equal
    to its power to the other simultaneously, going through
    ``_apply_damage`` so any "when ~ is dealt damage" auras (Cryoshatter,
    Pain for All) on either fighter trigger correctly. State-based
    actions then handle creatures with lethal damage.
    """

    def requires_target(self) -> bool:
        return False

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id != controller_id
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        if not targets:
            return
        aura = state.resolving_source
        if aura is None or aura.attached_to is None:
            return
        enchanted = next(
            (o for o in state.battlefield if o.id == aura.attached_to),
            None,
        )
        if enchanted is None:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        opp_creature = next(
            (o for o in state.battlefield if o.id == target_id),
            None,
        )
        if opp_creature is None:
            return
        enchanted_power = effective_power(state, enchanted)
        opp_power = effective_power(state, opp_creature)
        state.log.append(
            f"{enchanted.printing.name} fights {opp_creature.printing.name}"
        )
        # Simultaneous damage: each deals damage equal to its power to
        # the other. Compute amounts before applying so a fighter that
        # dies still deals its damage (rule 701.12).
        if opp_power > 0:
            _apply_damage(state, ("creature", enchanted.id), opp_power)
        if enchanted_power > 0:
            _apply_damage(state, ("creature", opp_creature.id), enchanted_power)

    def describe(self) -> str:
        return (
            "enchanted creature fights up to one target creature an "
            "opponent controls"
        )



@dataclass(frozen=True)
class PossibilityTechnicianETBEffect(Effect):
    """Possibility Technician's ETB-and-Kavu-observer effect: "Exile
    the top card of your library. For as long as that card remains
    exiled, you may play it if you control a Kavu."

    Wired by the printing as both an ``etb`` trigger (this creature)
    and a ``kavu_you_control_enters`` observer trigger (another Kavu),
    matching the printed oracle's "this creature or another Kavu you
    control enters". Persistence is modelled with the
    ``play_from_exile_requires_kavu`` flag on the exiled card —
    legal_actions surfaces a CastSpell/PlayLand for it whenever the
    controller currently controls at least one Kavu, and the flag is
    NOT cleared at end of turn (so the cast remains available across
    turns until the card leaves exile or the Kavu count drops to 0).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        if not p.library:
            return
        top = p.library.pop(0)
        top.zone = Zone.EXILE
        top.play_from_exile_requires_kavu = True
        p.exile.append(top)
        state.log.append(
            f"Possibility Technician: P{controller_id} exiles "
            f"{top.printing.name} (playable while controller "
            f"controls a Kavu)"
        )

    def describe(self) -> str:
        return (
            "exile the top card of your library; for as long as that "
            "card remains exiled, you may play it if you control a Kavu"
        )



@dataclass(frozen=True)
class RovingActuatorETBEffect(Effect):
    """Roving Actuator's Void ETB: "Exile up to one target instant or
    sorcery card with mana value 2 or less from your graveyard. Copy
    it. You may cast the copy without paying its mana cost."

    Simplification: the "copy and cast" machinery short-circuits to
    directly resolving the exiled card's spell_effect with fresh
    targets selected by the agent. The copy doesn't go on the stack
    and can't be counterspelled — acceptable given the engine's lack
    of responses. "May" is always yes.
    """

    max_mv: int = 2

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_spell", c.id)
            for c in p.graveyard
            if (c.printing.is_instant or c.printing.is_sorcery)
            and c.printing.mana_cost.total <= self.max_mv
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "graveyard_spell":
            return
        p = state.player_by_id(controller_id)
        card = next((c for c in p.graveyard if c.id == target_id), None)
        if card is None:
            return
        # Exile the original.
        p.graveyard.remove(card)
        card.zone = Zone.EXILE
        p.exile.append(card)
        state.log.append(
            f"Roving Actuator exiles {card.printing.name} and copies it"
        )
        # Resolve a copy of its spell_effect. Fresh targets if any.
        spell_effect = card.printing.spell_effect
        if spell_effect is None:
            return
        copy_targets: tuple[Target, ...] = ()
        min_t, max_t = spell_effect.target_count_range()
        if max_t > 0:
            legal = spell_effect.legal_targets(state, controller_id)
            if not legal and min_t > 0:
                state.log.append(
                    f"Copy of {card.printing.name} fizzles "
                    f"(no legal targets)"
                )
                return
            if p.agent is not None:
                chosen_list: list[Target] = []
                remaining = list(legal)
                for _ in range(max_t):
                    if not remaining:
                        break
                    pick = p.agent.choose_target(
                        state, controller_id, remaining
                    )
                    if pick is None or pick not in remaining:
                        break
                    chosen_list.append(pick)
                    remaining.remove(pick)
                copy_targets = tuple(chosen_list)
        spell_effect.resolve(state, controller_id, copy_targets)

    def describe(self) -> str:
        return (
            f"exile up to one target instant or sorcery card with "
            f"mana value {self.max_mv} or less from your graveyard, "
            f"copy it; you may cast the copy without paying its cost"
        )



@dataclass(frozen=True)
class AtomicMicrosizerAttackEffect(Effect):
    """Atomic Microsizer's equipped-attacks trigger: "Choose up to one
    target creature. That creature can't be blocked this turn and has
    base power and toughness 1/1 until end of turn."

    Max 1 target; if no creature exists it fizzles (0 is fine per
    "up to one").
    """

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (0, 1)

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
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic import BecomeType, GrantUnblockable

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
        # Layer-6 "can't be blocked UEOT" + layer-7b "set base P/T to
        # 1/1 UEOT" — both compose the production
        # ``ApplyEffect(<primitive>, EndOfTurn())`` pattern with the
        # equipped creature as the explicit target so the registry
        # entries are anchored to the equipped creature, not to
        # ``state.resolving_source``. The unblockable side uses
        # ``GrantUnblockable``'s direct-target shape (``self.targets``
        # empty, runtime ``targets`` carries the equipped creature),
        # which writes a layer-6 ``cant_be_blocked`` registry entry.
        ApplyEffect(
            inner=GrantUnblockable(),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, targets)
        ApplyEffect(
            inner=BecomeType(base_pt=(1, 1)),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, targets)
        state.log.append(
            f"{obj.printing.name} can't be blocked this turn and has "
            f"base P/T 1/1 UEOT"
        )

    def describe(self) -> str:
        return (
            "choose up to one target creature. That creature can't be "
            "blocked this turn and has base power and toughness 1/1 "
            "until end of turn"
        )



# ChoraleReanimateDefenderGraveCreatureEffect was decomposed in
# audit-monoist-chorale: the body is now a literal multi-sentence
# recognizer in ``effects._parse_effect`` mapping to the generic
# ``ReanimateTargetCreatureCardFromDefendersGraveyard`` primitive in
# ``effects.generic``.


@dataclass(frozen=True)
class KavaronHarrierAttackEffect(Effect):
    """Kavaron Harrier's attacks trigger: "You may pay {2}. If you do,
    create a 2/2 colorless Robot artifact creature token that's tapped
    and attacking. Sacrifice that token at end of combat."

    "May" is treated as yes whenever the caster can afford {2}. Token
    is created tapped+attacking; its id is scheduled for end-of-combat
    sacrifice via state.pending_end_of_combat_sacrifices.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import auto_tap_for_cost, can_pay_cost
        from mtgz_ir.cards import TOKEN_PRINTINGS, parse_mana_cost
        from mtgz_ir.game import Zone

        cost = parse_mana_cost("{2}")
        if not can_pay_cost(state, controller_id, cost):
            state.log.append(
                f"Kavaron Harrier: P{controller_id} can't pay {{2}}; "
                f"trigger does nothing"
            )
            return
        auto_tap_for_cost(state, controller_id, cost)
        robot = TOKEN_PRINTINGS["Robot"]
        token = state.new_object(robot, controller_id, Zone.BATTLEFIELD)
        token.tapped = True
        token.attacking = True
        token.summoning_sick = False
        token.entered_battlefield_turn = state.turn_number
        state.battlefield.append(token)
        state.pending_end_of_combat_sacrifices.append(token.id)
        state.log.append(
            f"Kavaron Harrier creates tapped+attacking Robot token "
            f"(will be sacrificed at end of combat)"
        )

    def describe(self) -> str:
        return (
            "you may pay {2}; if you do, create a 2/2 Robot token "
            "that's tapped and attacking; sacrifice that token at "
            "end of combat"
        )



# MonoistCircuitFeederETBEffect was decomposed in audit-monoist-chorale:
# the body is now a literal multi-sentence recognizer in
# ``effects._parse_effect`` mapping to the generic
# ``PairedTargetPumpDebuffUEOT`` primitive in ``effects.generic`` driven
# by the ``CountControlledMatching(IsArtifact())`` formula.


@dataclass(frozen=True)
class AnticausalVestigeLeavesEffect(Effect):
    """Anticausal Vestige's leaves-battlefield trigger: "Draw a card,
    then you may put a permanent card with mana value less than or
    equal to the number of lands you control from your hand onto the
    battlefield tapped."

    Simplification: "may" is always yes if there's a legal permanent
    card in hand. Agent picks which via choose_sacrifice (reusing the
    id-from-list picker — no dedicated "pick from hand" hook exists).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.turn import draw_n_cards

        draw_n_cards(state, controller_id, 1)
        p = state.player_by_id(controller_id)
        land_count = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_land
        )
        candidates = [
            c for c in p.hand
            if c.printing.is_permanent
            and c.printing.mana_cost.total <= land_count
        ]
        if not candidates or p.agent is None:
            return
        pick_id = p.agent.choose_sacrifice(
            state, controller_id, [c.id for c in candidates]
        )
        card = next((c for c in candidates if c.id == pick_id), candidates[0])
        p.hand.remove(card)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the "
            f"battlefield tapped (Anticausal Vestige)"
        )
        put_into_battlefield(state, card, controller_id)
        card.tapped = True

    def describe(self) -> str:
        return (
            "draw a card, then you may put a permanent card with mana "
            "value X or less from your hand onto the battlefield "
            "tapped, where X is the number of lands you control"
        )



@dataclass(frozen=True)
class TerrapactIntimidatorETBEffect(Effect):
    """Terrapact Intimidator's ETB: "Target opponent may have you create
    two Lander tokens. If they don't, put two +1/+1 counters on this
    creature."

    Targets a player (the chosen opponent). The opponent then picks:
    option 0 = controller creates two Landers; option 1 = controller
    puts two +1/+1 counters on the source. Reads state.resolving_source
    for the source permanent.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id)
            for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "player":
            return
        opponent = state.player_by_id(target_id)
        if opponent.has_lost or opponent.agent is None:
            return
        modes = ["have controller create two Lander tokens", "decline"]
        choice = opponent.agent.choose_mode(state, opponent.id, modes)
        if choice == 0:
            lander_printing = TOKEN_PRINTINGS["Lander"]
            for _ in range(2):
                token = state.new_object(
                    lander_printing, controller_id, Zone.BATTLEFIELD,
                )
                token.entered_battlefield_turn = state.turn_number
                state.battlefield.append(token)
            state.log.append(
                f"P{opponent.id} elected — P{controller_id} creates "
                f"two Lander tokens"
            )
        else:
            if source is not None:
                from mtgz_ir.game import add_plus_one_counters
                add_plus_one_counters(state, source, 2)
                state.log.append(
                    f"P{opponent.id} declined — {source.printing.name} "
                    f"gets two +1/+1 counters (now "
                    f"{source.plus_one_counters})"
                )

    def describe(self) -> str:
        return (
            "target opponent may have you create two Lander tokens; if "
            "they don't, put two +1/+1 counters on this creature"
        )



@dataclass(frozen=True)
class DyadrineAttackEffect(Effect):
    """Dyadrine, Synthesis Amalgam's attack trigger:
    "Whenever you attack, you may remove a +1/+1 counter from each of
    two creatures you control. If you do, draw a card and create a
    2/2 colorless Robot artifact creature token."

    Simplifications:
    - Agent always opts to pay the cost when possible (consistent
      with other OptionalSac-like effects in the codebase).
    - The two creatures are picked by iteration order — the agent's
      choose_sacrifice hook is re-used to select one creature at a
      time so at least some agency is exposed.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        candidates = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and o.printing.is_creature
            and o.plus_one_counters > 0
        ]
        if len(candidates) < 2:
            state.log.append(
                "Dyadrine's attack trigger: fewer than 2 creatures "
                "with +1/+1 counters — skipped"
            )
            return
        picks: list = []
        remaining = list(candidates)
        for _ in range(2):
            chosen = remaining[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, controller_id, [o.id for o in remaining]
                )
                chosen = next(
                    (o for o in remaining if o.id == pick_id),
                    remaining[0],
                )
            chosen.plus_one_counters -= 1
            picks.append(chosen)
            remaining.remove(chosen)
            state.log.append(
                f"Dyadrine removes +1/+1 counter from "
                f"{chosen.printing.name} (now {chosen.plus_one_counters})"
            )
        # Draw a card.
        if p.library:
            card = p.library.pop(0)
            card.zone = Zone.HAND
            p.hand.append(card)
            state.log.append(
                f"Dyadrine: P{controller_id} draws {card.printing.name}"
            )
        # Create the Robot token.
        token = state.new_object(
            TOKEN_PRINTINGS["Robot"], owner_id=controller_id,
            zone=Zone.BATTLEFIELD,
        )
        put_into_battlefield(state, token, controller_id)
        state.log.append(
            f"Dyadrine: P{controller_id} creates a Robot token"
        )

    def describe(self) -> str:
        return (
            "you may remove a +1/+1 counter from each of two creatures "
            "you control; if you do, draw a card and create a 2/2 "
            "colorless Robot artifact creature token"
        )



@dataclass(frozen=True)
class CloseEncounterEffect(Effect):
    """Close Encounter `{1}{G}`:
    "As an additional cost to cast this spell, choose a creature you
    control or a warped creature card you own in exile.
    Close Encounter deals damage equal to the power of the chosen
    creature or card to target creature."

    Reads state.resolving_additional_cost_choice_id to know which
    creature/card the caster designated, then deals damage equal to
    that creature's or card's power to the chosen target creature.

    For a battlefield-creature pick, uses ``effective_power`` (the
    designation's current power, including any pumps). For an exiled
    warped creature card, uses the card's printed power — the card
    isn't on the battlefield so continuous effects don't apply.
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
        from mtgz_ir.game import effective_power

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        chosen_id = state.resolving_additional_cost_choice_id
        if chosen_id is None:
            return
        chosen = next(
            (o for o in state.battlefield if o.id == chosen_id), None
        )
        if chosen is not None:
            amount = effective_power(state, chosen)
        else:
            # Look in the caster's exile for a warped creature card —
            # that's the other branch of the additional cost.
            p = state.player_by_id(controller_id)
            chosen = next(
                (o for o in p.exile if o.id == chosen_id), None
            )
            if chosen is not None:
                amount = chosen.printing.power or 0
            else:
                # The designated creature left the battlefield before
                # this resolved (e.g., destroyed in response) — we
                # don't track a P/T snapshot at cost-pay time, so
                # fall back to 0.
                amount = 0
        _apply_damage(state, targets[0], amount)

    def describe(self) -> str:
        return (
            "deals damage equal to the designated creature's power to "
            "target creature"
        )



@dataclass(frozen=True)
class ConsultTheStarChartsEffect(Effect):
    """Consult the Star Charts `{1}{U}` (kicker `{1}{U}`):
    "Look at the top X cards of your library, where X is the number of
    lands you control. Put one of those cards into your hand. If this
    spell was kicked, put two of those cards into your hand instead.
    Put the rest on the bottom of your library in a random order."

    Reads state.resolving_kicked to choose between 1 and 2 cards to
    keep. Agent picks the kept cards via ``choose_card_from_library``.

    NOT emitted by the parser any more: the card composes from the
    generic look-at-top grammar (``IfConditionElse(SpellWasKicked(),
    LookAtTopCardsPutIntoHand(take=2, ...), LookAtTopCardsPutIntoHand(
    take=1, ...))`` over ``CountControlledMatching(IsLand())``). The class
    stays only because the Rust parity bridge still round-trips it.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        x = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id and o.printing.is_land
        )
        if x == 0 or not p.library:
            state.log.append(
                f"Consult the Star Charts: X={x}, nothing to look at"
            )
            return
        examined = p.library[:x]
        keep = 2 if state.resolving_kicked else 1
        kept_cards: list = []
        remaining = list(examined)
        for _ in range(keep):
            if not remaining:
                break
            chosen_id = None
            if p.agent is not None:
                chosen_id = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in remaining]
                )
            if chosen_id is None:
                chosen_id = remaining[0].id
            card = next(
                (c for c in remaining if c.id == chosen_id), remaining[0]
            )
            kept_cards.append(card)
            remaining.remove(card)
        from mtgz_ir.game import reset_public_knowledge
        for card in kept_cards:
            p.library.remove(card)
            card.zone = Zone.HAND
            p.hand.append(card)
            # Disturbance: opponent who previously knew this card's
            # library position can no longer track which examined card
            # went to hand vs bottom. Conservatively forget reveals.
            reset_public_knowledge(card)
            state.log.append(
                f"P{controller_id} puts {card.printing.name} into hand "
                f"from Consult the Star Charts"
            )
        # Bottom the rest in random order.
        for card in remaining:
            p.library.remove(card)
            reset_public_knowledge(card)
        state.rng.shuffle(remaining)
        p.library.extend(remaining)
        state.log.append(
            f"Consult the Star Charts: put {len(remaining)} cards on "
            f"the bottom in random order"
        )

    def describe(self) -> str:
        return (
            "look at the top X cards of your library (X = lands you "
            "control); put one into your hand (two if kicked); put "
            "the rest on the bottom in a random order"
        )



@dataclass(frozen=True)
class SeedshipImpactEffect(Effect):
    """Destroy target artifact or enchantment. If its mana value was 2
    or less, create a Lander token.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_artifact_now

        return [
            ("permanent", o.id)
            for o in state.battlefield
            if is_artifact_now(state, o)
            or "Enchantment" in o.printing.card_types
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.cards import TOKEN_PRINTINGS
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import destroy

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        if not (
            is_artifact_now(state, obj)
            or "Enchantment" in obj.printing.card_types
        ):
            return
        mv = obj.printing.mana_cost.total
        state.log.append(f"{obj.printing.name} is destroyed")
        if not destroy(state, obj):
            return  # indestructible — no Lander consolation
        if mv <= 2:
            lander = state.new_object(
                TOKEN_PRINTINGS["Lander"],
                owner_id=controller_id,
                zone=Zone.BATTLEFIELD,
            )
            state.log.append(f"P{controller_id} creates a Lander token")
            put_into_battlefield(state, lander, controller_id)

    def describe(self) -> str:
        return (
            "destroy target artifact or enchantment; if its mana value "
            "was 2 or less, create a Lander token"
        )



@dataclass(frozen=True)
class MutinousMassacreEffect(Effect):
    """Mutinous Massacre `{3}{B}{B}{R}{R}`:
    "Choose odd or even. Destroy each creature with mana value of the
    chosen quality. Then gain control of all creatures until end of
    turn. Untap them. They gain haste until end of turn. (Zero is even.)"

    The "choose odd or even" pick goes through ``choose_mode`` with
    descriptions ``["odd", "even"]``. Surviving creatures (those whose
    mana value parity differed from the chosen one) are all stolen via
    ``grant_control_change_ueot`` (layer-2 ``control_change`` registry
    entry, expires at cleanup), untapped, and given haste UEOT.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import destroy

        p = state.player_by_id(controller_id)
        pick = 0  # 0 = odd, 1 = even
        if p.agent is not None:
            pick = p.agent.choose_mode(state, controller_id, ["odd", "even"])
            pick = max(0, min(pick, 1))
        chosen_label = "odd" if pick == 0 else "even"
        state.log.append(
            f"P{controller_id} chooses {chosen_label} for Mutinous Massacre"
        )
        # Destroy phase: each creature whose mana value matches the
        # chosen parity.
        victims = []
        for o in list(state.battlefield):
            if not o.printing.is_creature:
                continue
            mv = o.printing.mana_cost.total
            if pick == 0 and mv % 2 == 1:
                victims.append(o)
            elif pick == 1 and mv % 2 == 0:
                victims.append(o)
        for v in victims:
            state.log.append(
                f"{v.printing.name} (MV {v.printing.mana_cost.total}) "
                f"destroyed by Mutinous Massacre ({chosen_label})"
            )
            destroy(state, v)
        # Gain control of all surviving creatures UEOT, untap, haste.
        src = state.resolving_source
        spell_id = src.id if src is not None else -1
        for o in state.battlefield:
            if not o.printing.is_creature:
                continue
            if o.controller_id != controller_id:
                state.grant_control_change_ueot(
                    source_id=spell_id,
                    target_id=o.id,
                    new_controller_id=controller_id,
                    description=(
                        f"Mutinous Massacre: P{controller_id} controls "
                        f"{o.printing.name} UEOT"
                    ),
                )
            o.tapped = False
            from mtgz_ir.effects.combinators import ApplyEffect
            from mtgz_ir.effects.durations import EndOfTurn
            from mtgz_ir.effects.generic import GrantKeywords

            ApplyEffect(
                inner=GrantKeywords(keywords=frozenset({"haste"})),
                ends_at=EndOfTurn(),
            ).resolve(state, controller_id, (("creature", o.id),))
            state.log.append(
                f"P{controller_id} takes control of {o.printing.name} "
                f"UEOT; untaps + gains haste"
            )

    def describe(self) -> str:
        return (
            "choose odd or even; destroy each creature with mana value "
            "of the chosen quality; then gain control of all creatures "
            "until end of turn, untap them, they gain haste until end "
            "of turn"
        )



@dataclass(frozen=True)
class SystemsOverrideEffect(Effect):
    """Systems Override `{2}{R}` — gain control of target artifact or
    creature until end of turn, untap it, give it haste UEOT. If it's
    a Spacecraft, put ten charge counters on it; if so, remove ten
    at the beginning of the next end step.

    Control reverts on cleanup via the layer-2 ``control_change``
    registry entry written by ``grant_control_change_ueot`` (the entry's
    ``expires_at="cleanup"`` is dropped by ``run_cleanup_step``'s
    ``expire_continuous_effects("cleanup")`` call, and the per-object
    controller cache is refreshed in the same pass). The ten-counter
    remove is scheduled as a delayed end-step action
    (``"remove_charge_counters"``) resolved in run_delayed_end_step_triggers.
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_artifact_now

        return [
            ("permanent", o.id)
            for o in state.battlefield
            if is_artifact_now(state, o) or o.printing.is_creature
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        # Gain control UEOT via the layer-2 control_change registry
        # entry; haste via the layer-6 GrantKeywords primitive wrapped
        # in EndOfTurn. Both expire at cleanup, with controller refresh
        # handled in ``run_cleanup_step``.
        src = state.resolving_source
        spell_id = src.id if src is not None else obj.id
        state.grant_control_change_ueot(
            source_id=spell_id,
            target_id=obj.id,
            new_controller_id=controller_id,
            description=(
                f"Systems Override: P{controller_id} controls "
                f"{obj.printing.name} UEOT"
            ),
        )
        obj.tapped = False
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic import GrantKeywords

        ApplyEffect(
            inner=GrantKeywords(keywords=frozenset({"haste"})),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", obj.id),))
        state.log.append(
            f"P{controller_id} takes control of {obj.printing.name} "
            f"UEOT; untaps + gains haste"
        )
        if obj.printing.is_spacecraft:
            from mtgz_ir.game import add_charge_counters
            add_charge_counters(state, obj, 10)
            state.log.append(
                f"{obj.printing.name} gets 10 charge counters "
                f"(now {obj.charge_counters})"
            )
            from mtgz_ir.effects import (
                RemoveChargeCountersFromSourceDelayed,
            )
            state.delayed_triggers.append({
                "fire_at_turn": state.turn_number,
                "controller_id": controller_id,
                "source_id": obj.id,
                "effect": RemoveChargeCountersFromSourceDelayed(amount=10),
            })

    def describe(self) -> str:
        return (
            "gain control of target artifact or creature until end of "
            "turn; untap it; it gains haste until end of turn; if it's "
            "a Spacecraft, put ten charge counters on it, then remove "
            "ten at the next end step"
        )



@dataclass(frozen=True)
class BiosynthicBurstEffect(Effect):
    """Put a +1/+1 counter on target creature you control. It gains reach,
    trample, and indestructible UEOT. Untap it.

    One dedicated effect because all three pieces operate on the same
    target, which doesn't compose cleanly via Sequence in our single-
    target-per-Sequence model.
    """

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
        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "creature":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None or obj.controller_id != controller_id:
            return
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, obj, 1)
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic import GrantKeywords

        ApplyEffect(
            inner=GrantKeywords(
                keywords=frozenset(
                    {"reach", "trample", "indestructible"},
                ),
            ),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", obj.id),))
        obj.tapped = False
        state.log.append(
            f"{obj.printing.name} gets a +1/+1 counter, gains reach, "
            f"trample, indestructible UEOT, and untaps"
        )

    def describe(self) -> str:
        return (
            "put a +1/+1 counter on target creature you control; it "
            "gains reach, trample, and indestructible until end of "
            "turn; untap it"
        )



@dataclass(frozen=True)
class DepressurizeEffect(Effect):
    """Target creature gets -3/-0 UEOT; then if its power is 0 or less,
    destroy it.

    Dedicated effect because the conditional destroy reads the pumped
    creature's post-modification power — an inline check after the pump
    is clearer than stitching Sequence + ConditionalDestroyIfPowerLEQ.
    """

    power_penalty: int = 3  # applied as a negative to temp_power_bonus

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
        from mtgz_ir.game import effective_power
        from mtgz_ir.turn import destroy

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
        source_id = src.id if src is not None else obj.id
        state.grant_pt_pump_ueot(
            source_id=source_id,
            target_id=obj.id,
            power=-self.power_penalty,
            toughness=0,
            description=(
                f"{obj.printing.name} gets -{self.power_penalty}/-0 UEOT"
            ),
        )
        state.log.append(
            f"{obj.printing.name} gets -{self.power_penalty}/-0 until end "
            f"of turn"
        )
        # "Then if that creature's power is 0 or less, destroy it."
        if effective_power(state, obj) <= 0:
            state.log.append(f"{obj.printing.name} is destroyed (depressurized)")
            destroy(state, obj)

    def describe(self) -> str:
        return (
            f"target creature gets -{self.power_penalty}/-0 until end of "
            f"turn; then if its power is 0 or less, destroy it"
        )



@dataclass(frozen=True)
class SingularityRuptureEffect(Effect):
    """Singularity Rupture `{3}{U}{B}{B}`:
    "Destroy all creatures, then any number of target players each mill
    half their library, rounded down."

    Multi-target: min 0, max = number of living players. After the
    wrath, each chosen player mills ``floor(library_size / 2)`` cards.
    """

    def requires_target(self) -> bool:
        # "Any number of target" — technically optional, but the cast
        # pipeline only requires a target pool to exist. legal_actions
        # won't gate casting on this since there's always at least one
        # living player.
        return True

    def target_count_range(self) -> tuple[int, int]:
        # "Any number" — min 0, max bounded by a reasonable ceiling.
        # Two-player games → max 2. We pick 8 to cover any plausible
        # multiplayer pod size; the cast pipeline clamps to actual
        # legal_targets length anyway.
        return (0, 8)

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players if not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import destroy

        # Wrath phase.
        victims = [o for o in state.battlefield if o.printing.is_creature]
        for o in victims:
            state.log.append(
                f"{o.printing.name} destroyed by Singularity Rupture"
            )
            destroy(state, o)
        # Mill phase — each targeted player mills half their library,
        # rounded down.
        for kind, tid in targets:
            if kind != "player":
                continue
            try:
                p = state.player_by_id(tid)
            except KeyError:
                continue
            if p.has_lost:
                continue
            n = len(p.library) // 2
            milled = 0
            for _ in range(n):
                if not p.library:
                    break
                card = p.library.pop(0)
                card.zone = Zone.GRAVEYARD
                p.graveyard.append(card)
                milled += 1
            state.log.append(
                f"P{tid} mills {milled} card(s) from Singularity Rupture "
                f"(library now {len(p.library)})"
            )

    def describe(self) -> str:
        return (
            "destroy all creatures; then any number of target players "
            "each mill half their library, rounded down"
        )



@dataclass(frozen=True)
class ZeroPointBalladEffect(Effect):
    """Zero Point Ballad `{X}{B}`:
    "Destroy all creatures with toughness X or less. You lose X life. If
    X is 6 or more, return a creature card put into a graveyard this way
    to the battlefield under your control."

    Reads X from ``state.resolving_x_value``. For the X≥6 clause, a
    creature card is "put into a graveyard this way" only if it was
    destroyed by the first clause AND landed in the caster's graveyard
    (i.e. the caster owned it). The agent picks one of those candidates
    via ``choose_target`` (kind=``creature_card``); declining is legal —
    the reanimation is optional.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import effective_toughness
        from mtgz_ir.turn import destroy

        x = state.resolving_x_value
        victims = [
            o for o in state.battlefield
            if o.printing.is_creature and effective_toughness(state, o) <= x
        ]
        caster_owned_victim_ids = {
            o.id for o in victims if o.owner_id == controller_id
        }
        for obj in victims:
            state.log.append(
                f"{obj.printing.name} destroyed by Zero Point Ballad "
                f"(X={x})"
            )
            destroy(state, obj)

        p = state.player_by_id(controller_id)
        p.life -= x
        state.log.append(
            f"P{controller_id} loses {x} life to Zero Point Ballad "
            f"(life: {p.life})"
        )

        if x >= 6:
            candidates = [
                c for c in p.graveyard
                if c.id in caster_owned_victim_ids
                and c.printing.is_creature
            ]
            if candidates and p.agent is not None:
                legal = [("creature_card", c.id) for c in candidates]
                chosen = p.agent.choose_target(state, controller_id, legal)
                if chosen is not None:
                    card = next(
                        (c for c in candidates if c.id == chosen[1]), None
                    )
                    if card is not None:
                        p.graveyard.remove(card)
                        put_into_battlefield(state, card, controller_id)
                        state.log.append(
                            f"Zero Point Ballad returns "
                            f"{card.printing.name} to the battlefield"
                        )

    def describe(self) -> str:
        return (
            "destroy all creatures with toughness X or less; you lose X "
            "life; if X is 6 or more, return a creature card put into a "
            "graveyard this way to the battlefield under your control"
        )



@dataclass(frozen=True)
class FocusFireEffect(Effect):
    """Deal (``base`` + your creatures and Spacecraft) damage to an
    attacking or blocking creature.

    Matches Focus Fire. Dynamic amount is recomputed at resolution.
    Legality is filtered to attackers/blockers only via
    ``self.targets[0].filter`` (``IsCreature() & (IsAttacking() |
    IsBlocking())``); that's re-checked at resolve time so a creature
    that stops attacking/blocking between cast and resolve fizzles
    the damage (CR 608.2b).

    Bespoke amount (X = base + creature/Spacecraft count) so the
    class survives PR #3's damage collapse.
    """

    base: int = 2
    targets: tuple = ()

    def __post_init__(self) -> None:
        if not self.targets:
            from mtgz_ir.effects.object_predicates import (
                IsAttacking, IsBlocking, IsCreature,
            )
            from mtgz_ir.effects.targets import TargetSpec
            object.__setattr__(
                self, "targets",
                (TargetSpec(
                    IsCreature() & (IsAttacking() | IsBlocking())
                ),),
            )

    def _amount(self, state: "GameState", controller_id: int) -> int:
        bonus = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id
            and (o.printing.is_creature or o.printing.is_spacecraft)
        )
        return self.base + bonus

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic import _slot_target_still_legal
        if not targets:
            return
        prior = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            if not _slot_target_still_legal(
                state, self.targets[0], targets[0]
            ):
                return
            _apply_damage(
                state, targets[0], self._amount(state, controller_id),
            )
        finally:
            state.resolving_controller_id = prior

    def describe(self) -> str:
        return (
            f"deals {self.base}+X damage to target attacking or blocking "
            f"creature, where X is creatures and Spacecraft you control"
        )



@dataclass(frozen=True)
class CutPropulsionEffect(Effect):
    """Target creature deals damage to itself equal to its power; doubled if
    that creature has flying.

    Matches Cut Propulsion. Self-damage uses _apply_damage, which applies
    deathtouch-style marked_lethal only when a deathtouch source is involved
    — self-damage carries no source keywords so SBA-based lethal checks
    behave normally.
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
        from mtgz_ir.game import effective_keywords, effective_power

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
        power = effective_power(state, obj)
        if power <= 0:
            state.log.append(
                f"{obj.printing.name} deals 0 damage to itself "
                f"(Cut Propulsion)"
            )
            return
        if "flying" in effective_keywords(state, obj):
            amount = power * 2
        else:
            amount = power
        state.log.append(
            f"{obj.printing.name} takes {amount} (Cut Propulsion self-damage)"
        )
        _apply_damage(state, targets[0], amount)

    def describe(self) -> str:
        return (
            "target creature deals damage to itself equal to its power; "
            "doubled if it has flying"
        )



@dataclass(frozen=True)
class DiplomaticRelationsEffect(Effect):
    """Diplomatic Relations `{2}{G}` Instant:
    "Target creature you control gets +1/+0 and gains vigilance until end
    of turn. It deals damage equal to its power to target creature an
    opponent controls."

    Two-target spell with disjoint filters. To let the existing flat
    target-pick pipeline validate each slot independently, we expose the
    two slots as distinct kinds:

    - ``("my_creature", id)`` — the pumped friendly creature (slot 0)
    - ``("creature",   id)`` — the opponent's creature taking damage (slot 1)

    ``legal_targets`` returns a flat list mixing both kinds. The agent
    picks twice; ``resolve`` then locates one entry of each kind and
    applies pump-then-damage. The damage amount is the pumped creature's
    effective power AT RESOLUTION (i.e., +1 already applied).

    If either slot has no legal candidates, ``legal_targets`` returns
    ``[]`` so the cast-time gate (``_targets_are_satisfiable``) refuses
    the spell entirely — mirroring MTG's "if you can't choose a legal
    target for each, you can't cast" rule.
    """

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def target_slot_kinds(self) -> tuple[str, ...]:
        # Slot 0: pumped friendly creature. Slot 1: damaged opp
        # creature. Per-slot kind enforcement.
        return ("my_creature", "creature")

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        mine = [
            ("my_creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id == controller_id
        ]
        theirs = [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id != controller_id
        ]
        # Both slots must be satisfiable; if either side is empty the
        # spell cannot be cast.
        if not mine or not theirs:
            return []
        return mine + theirs

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        # Locate one entry per slot. Order is agent-dependent so we
        # search by kind rather than index.
        my_target = next(
            ((k, t) for (k, t) in targets if k == "my_creature"), None
        )
        opp_target = next(
            ((k, t) for (k, t) in targets if k == "creature"), None
        )
        # Pump the friendly creature first if it's still on the battlefield
        # and still controlled by the caster (target re-validation: if it
        # left or changed control, that target is illegal and skipped per
        # rule 608.2b).
        my_obj = None
        if my_target is not None:
            my_obj = next(
                (o for o in state.battlefield if o.id == my_target[1]),
                None,
            )
            if my_obj is not None and my_obj.controller_id == controller_id:
                src = state.resolving_source
                source_id = src.id if src is not None else my_obj.id
                state.grant_pt_pump_ueot(
                    source_id=source_id,
                    target_id=my_obj.id,
                    power=1,
                    toughness=0,
                    description=f"{my_obj.printing.name} gets +1/+0 UEOT",
                )
                from mtgz_ir.effects.combinators import ApplyEffect
                from mtgz_ir.effects.durations import EndOfTurn
                from mtgz_ir.effects.generic import GrantKeywords

                ApplyEffect(
                    inner=GrantKeywords(
                        keywords=frozenset({"vigilance"}),
                    ),
                    ends_at=EndOfTurn(),
                ).resolve(
                    state, controller_id, (("creature", my_obj.id),),
                )
                state.log.append(
                    f"{my_obj.printing.name} gets +1/+0 and gains "
                    f"vigilance until end of turn"
                )
            else:
                my_obj = None
        # Damage component: requires both an opponent's-creature target
        # AND a still-living, still-controlled friendly creature whose
        # power we read.
        if opp_target is None or my_obj is None:
            return
        opp_obj = next(
            (o for o in state.battlefield if o.id == opp_target[1]),
            None,
        )
        if opp_obj is None or opp_obj.controller_id == controller_id:
            return
        amount = effective_power(state, my_obj)
        state.log.append(
            f"{my_obj.printing.name} deals {amount} damage to "
            f"{opp_obj.printing.name} (Diplomatic Relations)"
        )
        _apply_damage(state, ("creature", opp_obj.id), amount)

    def describe(self) -> str:
        return (
            "target creature you control gets +1/+0 and gains vigilance "
            "until end of turn; it deals damage equal to its power to "
            "target creature an opponent controls"
        )



@dataclass(frozen=True)
class EnduringDiesReturnAsEnchantmentEffect(Effect):
    """DSK Enduring cycle dies-trigger body: "return it to the
    battlefield under its owner's control. It's an enchantment."

    Resolves directly from the DIES dispatch path (mirrors the
    ``PerigeeBeckonerDiesReturnTappedEffect`` precedent): the dying
    Enduring creature is sitting in its owner's graveyard with
    ``state.resolving_source`` pointing at it. We move it back to
    the battlefield under its OWNER's control (per oracle — "owner's
    control", not "your control") and set
    ``enduring_returned_as_enchantment=True`` so the engine's
    ``is_creature_now`` reads False on the returned permanent (it's
    an enchantment now, not a creature). The printing's card_types
    still includes "Creature" — the suppression is per-instance,
    matching the impending_active precedent.

    Used by Enduring Innocence (W), Enduring Curiosity (U), Enduring
    Tenacity (B), Enduring Courage (R), and Enduring Vitality (G) —
    every Enduring creature in the DSK pool prints the same return-
    as-enchantment body verbatim. The "if it was a creature"
    intervening-if gate that pairs with this effect is handled by
    the parser's predicate composition (the gate is naturally
    implicit in the engine's ``_fire_dies_triggers`` path: a returned
    enchantment is no longer a creature, so a second death wouldn't
    fire the dies trigger at all — but the parser still composes the
    gate predicate to keep parser/static parity readable and to
    forward-protect against future engine changes that would loosen
    the dies-fire gate).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        source = state.resolving_source
        if source is None:
            return
        owner = state.player_by_id(source.owner_id)
        # Tokens cease to exist on death — nothing to bring back.
        # No EoE/DSK printing makes a token Enduring creature today,
        # but the guard keeps the path safe if a future card grants
        # the dies-rider to a token.
        if source.printing.is_token:
            return
        # The dying creature should be in its owner's graveyard. If
        # it's been moved elsewhere already (e.g. exiled by a
        # replacement, or the printed effect's dies-trigger fizzled
        # because the source was stolen mid-combat and graveyarded
        # under another player's pile), we don't return anything.
        if source not in owner.graveyard:
            return
        owner.graveyard.remove(source)
        # Clear UEOT-granted triggered abilities targeting this object
        # — the returned permanent is a "new object" per CR 400.7, so
        # any UEOT grants on the FORMER object are dropped before the
        # battlefield transition.
        state.continuous_effects = [
            ce for ce in state.continuous_effects
            if not (
                ce.layer == 6
                and ce.kind == "grant_triggered_ability"
                and ce.target_id == source.id
            )
        ]
        state._invalidate_layered_cache()
        put_into_battlefield(state, source, owner.id)
        # Mark the returned permanent as no-longer-a-creature. The
        # flag is sticky for the rest of this GameObject's life on
        # the battlefield; a subsequent zone change creates a fresh
        # GameObject (CR 400.7) and the printed Creature card type
        # comes back into play on the next ETB.
        source.enduring_returned_as_enchantment = True
        state.log.append(
            f"{source.printing.name} returns to P{owner.id}'s "
            f"battlefield as an enchantment (Enduring)"
        )

    def describe(self) -> str:
        return (
            "return it to the battlefield under its owner's control. "
            "it's an enchantment"
        )


@dataclass(frozen=True)
class PerigeeBeckonerDiesReturnTappedEffect(Effect):
    """Perigee Beckoner's granted dies-rider: "When this creature dies,
    return it to the battlefield tapped under its owner's control."

    Resolves directly from ``_fire_dies_triggers`` (mirrors the
    Requiem-Monolith dealt-damage rider pattern): the dying creature is
    sitting in its owner's graveyard with ``state.resolving_source`` set
    to it. We move it back onto the battlefield, tapped, under its
    owner's id (per oracle — "owner's control", not "your control").
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone

        source = state.resolving_source
        if source is None:
            return
        owner = state.player_by_id(source.owner_id)
        # Tokens cease to exist on death — nothing to bring back.
        if source.printing.is_token:
            return
        # The dying creature should be in its owner's graveyard. If it's
        # been moved elsewhere already (e.g. exiled by a replacement),
        # we don't return anything.
        if source not in owner.graveyard:
            return
        owner.graveyard.remove(source)
        # Clear UEOT-granted triggered abilities targeting this object
        # so a second death this turn (re-killed after the return)
        # doesn't re-fire the rider — rules-wise the granted UEOT
        # trigger is on the FORMER object; the returned object is a
        # "new object" with no memory of UEOT grants. Drop any
        # layer-6 registry grant entries targeting this source.
        # Cleanup wipes the registry at end of turn anyway, but
        # immediate clearing keeps the dies-trigger firing semantics
        # correct mid-turn.
        state.continuous_effects = [
            ce for ce in state.continuous_effects
            if not (
                ce.layer == 6
                and ce.kind == "grant_triggered_ability"
                and ce.target_id == source.id
            )
        ]
        state._invalidate_layered_cache()
        put_into_battlefield(state, source, owner.id)
        source.tapped = True
        state.log.append(
            f"{source.printing.name} returns to P{owner.id}'s "
            f"battlefield tapped (granted dies-rider)"
        )

    def describe(self) -> str:
        return (
            "return this creature to the battlefield tapped under its "
            "owner's control"
        )


@dataclass(frozen=True)
class AlpharaelDreamingAcolyteDiscardEffect(Effect):
    """Alpharael, Dreaming Acolyte's "discard" tail of the ETB clause:

    "...Then discard two cards unless you discard an artifact card."

    Templated as a "do X unless Y" choice — the controller may sub the
    main discard (two cards, any) for a single artifact-card discard.
    Implemented as a binary mode pick:

    - Mode 0: discard one artifact card from hand. Only legal when the
      controller has at least one artifact card in hand. The specific
      artifact picked goes through ``choose_discard`` filtered to the
      artifact subset.
    - Mode 1: discard two cards (any), the standard fallback.

    If the controller has no artifact card in hand, mode 0 is unavailable
    and the effect always falls back to discarding two cards. The draw-
    two portion of the printed clause is handled by the preceding
    ``DrawCards(2)`` in the parent ``Sequence``; this effect only models
    the "discard ... unless ..." rider.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, is_artifact_now
        from mtgz_ir.turn import fire_opponent_discards_triggers

        p = state.player_by_id(controller_id)
        if not p.hand:
            return

        artifact_ids = [c.id for c in p.hand if is_artifact_now(state, c)]
        # If no artifact in hand, the "unless" branch is unavailable —
        # fall through to the discard-two path.
        use_artifact_dodge = False
        if artifact_ids and p.agent is not None:
            pick = p.agent.choose_mode(
                state, controller_id,
                ["discard an artifact card", "discard two cards"],
            )
            pick = max(0, min(pick, 1))
            use_artifact_dodge = (pick == 0)

        if use_artifact_dodge:
            # Discard exactly one artifact card. Agent picks which via
            # choose_discard restricted to the artifact-card subset.
            chosen_id = p.agent.choose_discard(
                state, controller_id, artifact_ids,
            )
            # Defensive: if the agent returns a non-artifact id (out of
            # contract), fall back to the first artifact id rather than
            # crashing — keeps the "unless" semantics intact.
            if chosen_id not in artifact_ids:
                chosen_id = artifact_ids[0]
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(
                f"P{p.id} discards {card.printing.name} "
                f"(Alpharael artifact dodge)"
            )
            fire_opponent_discards_triggers(state, p.id, card=card)
            return

        # Fallback: discard two cards (any). Mirrors DiscardSelf(amount=2).
        for _ in range(2):
            if not p.hand:
                break
            hand_ids = [c.id for c in p.hand]
            chosen_id = p.agent.choose_discard(state, p.id, hand_ids)
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(f"P{p.id} discards {card.printing.name}")
            fire_opponent_discards_triggers(state, p.id, card=card)

    def describe(self) -> str:
        return "discard two cards unless you discard an artifact card"



# VengefulPossessionEffect was deleted in the cleanup pass: the card's
# four-sentence body now decomposes at the parser layer into
# ``Sequence(ThreatenTargetCreatureUEOT(),
#            IfYouDo(MayDo(DiscardSelf(amount=1)), DrawCards(amount=1)))``,
# composing four generic primitives instead of carrying a per-card
# bespoke class. Future Threaten-style cards reuse the threaten
# primitive without code; future may-do-X-if-you-do-Y cards reuse the
# IfYouDo + MayDo combinators without code.



@dataclass(frozen=True)
class ExorciseEffect(Effect):
    """Exorcise ``{1}{W}`` (Duskmourn): "Exile target artifact,
    enchantment, or creature with power 4 or greater."

    NOT emitted by the parser any more: Exorcise composes from the
    generic removal grammar (``RemoveTargetPermanents(action="exile",
    target_filter=IsArtifact() | IsEnchantment() | (IsCreature() &
    HasPowerAtLeast(4)))``). The class stays only because the Rust
    parity bridge (``parity/cross_engine.py`` / ``parity/trajectory.py``,
    ``Effect::ExorciseEffect``) still round-trips it.

    Three-way disjunctive target filter — the chosen target must be
    an artifact OR an enchantment OR a creature whose effective power
    is at least 4. The first two arms are unconditional (any artifact
    or enchantment is legal regardless of P/T); only the third arm
    has the power gate. A permanent that's BOTH an enchantment and
    a creature (e.g. an enduring creature, an aura'd creature) is
    legal under the enchantment arm even at low power.

    Exiles via ``move_off_battlefield(state, obj, destination=
    "exile")`` — the same path ``ExileTargetCreature`` and
    ``ExileTargetCreatureWithPowerOrToughnessAtMost`` use. Uses
    effective_power (not printed power) so static anthems and pump
    auras count toward the 4+ threshold at both targeting and
    resolution. Re-checks the condition at resolution because a
    pump between cast and resolve could change effective power
    (e.g. a -X/-Y debuff trims the attacker below 4 mid-stack).
    """

    def requires_target(self) -> bool:
        return True

    def _matches(self, state, obj) -> bool:
        # Defense-in-depth: re-evaluate effective_power against
        # current state every time we filter, so a target picked at
        # high power and debuffed mid-stack falls out of the legal
        # set on the resolve recheck.
        from mtgz_ir.game import effective_power, is_artifact_now

        if is_artifact_now(state, obj):
            return True
        if "Enchantment" in obj.printing.card_types:
            return True
        if obj.printing.is_creature and effective_power(state, obj) >= 4:
            return True
        return False

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        return [
            ("permanent", o.id)
            for o in state.battlefield
            if self._matches(state, o)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import move_off_battlefield

        if not targets:
            return
        kind, target_id = targets[0]
        if kind != "permanent":
            return
        obj = next(
            (o for o in state.battlefield if o.id == target_id), None
        )
        if obj is None:
            return
        # Re-check the disjunctive filter at resolve time — a pump
        # between cast and resolve could push the creature target
        # below the 4-power threshold (and the artifact / enchantment
        # arms could fail if the target lost its type via Duskmourn's
        # Domination's lose-all-abilities or a similar effect, though
        # that's not common).
        if not self._matches(state, obj):
            state.log.append(
                f"{obj.printing.name} no longer matches "
                f"artifact/enchantment/4+-power-creature — "
                f"Exorcise fizzles"
            )
            return
        state.log.append(f"{obj.printing.name} is exiled (Exorcise)")
        move_off_battlefield(state, obj, destination="exile")

    def describe(self) -> str:
        return (
            "exile target artifact, enchantment, or creature with "
            "power 4 or greater"
        )



@dataclass(frozen=True)
class CoordinatedClobberingEffect(Effect):
    """Coordinated Clobbering ``{G}`` (Duskmourn): "Tap one or two
    target untapped creatures you control. They each deal damage
    equal to their power to target creature an opponent controls."

    Three slots, two slot kinds:

    - ``("creature", id)`` — the opp's creature taking damage (slot 0,
      mandatory)
    - ``("my_creature", id)`` — first friendly attacker (slot 1,
      mandatory)
    - ``("my_creature", id)`` — optional second friendly attacker
      (slot 2, beyond ``min_t``)

    Why opp first: the cast-pipeline target picker iterates slots in
    order. Putting the mandatory opp's-creature slot first and the
    optional second-friendly slot LAST lets ``min_t=2`` and
    ``max_t=3`` cleanly separate "must pick" (slots 0, 1) from
    "may pick" (slot 2), since the picker honors None-decline only
    for slot indices ``>= min_t``. The printed reading order ("tap
    one or two ..." first) is just oracle-text presentation and
    doesn't constrain MTG's target-picking sequence.

    Both friendly slots filter on currently-untapped (the
    "Tap [...] untapped creatures you control" gate) — a friendly
    creature already tapped is not a legal target. The ``my_`` prefix
    on the slot kind enforces controller==caster at pick time;
    ``"creature"`` for the opp slot enforces controller!=caster via
    a separate filter.

    Resolution mechanics:
    1. Tap each friendly target (idempotent; skips ones bounced /
       retargeted / already tapped between cast and resolve).
    2. Each surviving friendly creature deals ``effective_power``
       damage to the opp's creature target (read fresh per attacker
       so a dies-trigger between attackers reads post-death state
       correctly — though the opp creature is the SAME target).

    Mirrors ``DiplomaticRelationsEffect``'s two-slot skeleton with
    the ``min_t < max_t`` extension for the optional second attacker.
    """

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        # min=2 (one friendly + one opp), max=3 (two friendlies +
        # one opp).
        return (2, 3)

    def target_slot_kinds(self) -> tuple[str, ...]:
        # Slot 0: opp's creature taking damage. Slots 1, 2: friendly
        # untapped creatures dealing damage. Mandatory (slots 0, 1)
        # before optional (slot 2) per the picker's min_t-gate.
        return ("creature", "my_creature", "my_creature")

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        mine = [
            ("my_creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id == controller_id
            and not o.tapped
        ]
        theirs = [
            ("creature", o.id)
            for o in state.battlefield
            if o.printing.is_creature
            and o.controller_id != controller_id
        ]
        # Both sides must be satisfiable at minimum ranks (one of mine
        # untapped, one opp creature) for the spell to be cast at
        # all — mirrors ``DiplomaticRelationsEffect``'s precedent.
        if not mine or not theirs:
            return []
        # Opp targets first so the slot-kind filter aligns with slot 0.
        return theirs + mine

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        # Pull one opp target and 1-2 friendly targets out of the
        # picked list. Order is agent-dependent so we search by kind.
        opp_target = next(
            ((k, t) for (k, t) in targets if k == "creature"), None
        )
        my_targets = [
            (k, t) for (k, t) in targets if k == "my_creature"
        ]
        # Tap each surviving friendly target. Re-validate per CR 608.2b:
        # if a friendly creature left the battlefield, was bounced, or
        # changed control between cast and resolve, that target is
        # illegal and the tap is skipped.
        my_objs = []
        for _, tid in my_targets:
            obj = next(
                (o for o in state.battlefield if o.id == tid), None
            )
            if obj is None:
                continue
            if obj.controller_id != controller_id:
                continue
            obj.tapped = True
            state.log.append(
                f"{obj.printing.name} taps (Coordinated Clobbering)"
            )
            my_objs.append(obj)
        # Damage component: requires both an opp's-creature target
        # AND at least one surviving friendly attacker.
        if opp_target is None or not my_objs:
            return
        opp_obj = next(
            (o for o in state.battlefield if o.id == opp_target[1]),
            None,
        )
        if opp_obj is None or opp_obj.controller_id == controller_id:
            return
        for atkr in my_objs:
            amount = effective_power(state, atkr)
            state.log.append(
                f"{atkr.printing.name} deals {amount} damage to "
                f"{opp_obj.printing.name} (Coordinated Clobbering)"
            )
            _apply_damage(state, ("creature", opp_obj.id), amount)

    def describe(self) -> str:
        return (
            "tap one or two target untapped creatures you control; "
            "each deals damage equal to its power to target creature "
            "an opponent controls"
        )



@dataclass(frozen=True)
class SawAttacksOptionalSacDrawEffect(Effect):
    """Saw's equipped-attacks trigger: "Whenever equipped creature
    attacks, you may sacrifice a permanent other than that creature or
    this Equipment. If you do, draw a card."

    Resolves on the Equipment as the trigger source. The Equipment's
    ``attached_to`` points at the equipped creature; the legal
    sacrifice pool is "permanents the controller controls, excluding
    the source Equipment AND the equipped creature." If no legal
    permanent exists the effect silently no-ops (the controller has
    nothing to sacrifice). Otherwise the controller decides via
    ``choose_yes_no``: declining is a free no-op (no draw); accepting
    fires ``choose_sacrifice`` to pick which permanent and then draws
    one card.

    The "may" is honored at the agent layer rather than baked into the
    effect because sacrificing a strictly worse permanent for a card is
    sometimes correct and sometimes not; the agent gets the final call.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards, sacrifice as _sacrifice

        source = state.resolving_source
        if source is None:
            return
        host_id = getattr(source, "attached_to", None)
        options = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and o.id != source.id
            and o.id != host_id
        ]
        if not options:
            return
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            "Sacrifice a permanent for Saw's draw?",
        ):
            state.log.append(
                f"P{controller_id} declines Saw's optional sacrifice"
            )
            return
        if p.agent is None:
            chosen = options[0]
        else:
            pick_id = p.agent.choose_sacrifice(
                state, controller_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name} (Saw)"
        )
        _sacrifice(state, chosen)
        draw_n_cards(state, controller_id, 1)

    def describe(self) -> str:
        return (
            "you may sacrifice a permanent other than that creature or "
            "this Equipment; if you do, draw a card"
        )



@dataclass(frozen=True)
class CommuneWithEvilEffect(Effect):
    """Commune with Evil ``{2}{B}`` (Duskmourn): "Look at the top four
    cards of your library. Put one of them into your hand and the rest
    into your graveyard. You gain 3 life."

    Three-step single-resolve effect:

    1. Look at the top four cards of the controller's library.
    2. Agent picks one of those cards via ``choose_card_from_library``;
       it goes to hand. The other three go to the controller's
       graveyard.
    3. Controller gains 3 life.

    Mirrors ``LookTopNPutOneInHandRestToGraveyard(n=2,
    reveal_chosen=False)`` (Codecracker Hound's ETB after the
    refined-Cycle-1 parser/engine fold; previously the bespoke
    ``LookAtTopTwoSplitHandGraveyard`` class) in shape — examine the
    top cards, split into hand/graveyard piles via the agent — but
    generalizes the 2-card split to a 4-card hand-vs-graveyard
    distribution where exactly ONE card goes to hand and the rest to
    graveyard. A bespoke class rather than reusing the parameterized
    primitive because:

    - Commune with Evil's printed text reads as a single forced choice
      ("put one of them into your hand"), best modeled by
      ``choose_card_from_library`` (a single id-pick prompt);
    - the trailing ``You gain 3 life.`` rider is bundled into the
      same resolve step rather than a Sequence wrapper because the
      static side ships the spell as one engine class.

    Library < 4 cards: examines whatever's available (1, 2, or 3
    cards). With 0 cards left, the spell simply gains 3 life and
    no-ops the look. The chosen-to-hand card is the agent's pick (or
    the top card, when no agent is seated for unit tests / when the
    agent declines / picks an out-of-pool id). The remaining
    examined cards always go to graveyard regardless of count.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, gain_life, reveal_to_all

        p = state.player_by_id(controller_id)
        examined = p.library[:4]
        if examined:
            # Agent picks one card to put into hand; default to the top
            # card when no agent is seated or the agent declines /
            # picks an out-of-pool id.
            chosen_id: int | None = None
            if p.agent is not None:
                chosen_id = p.agent.choose_card_from_library(
                    state, controller_id, [c.id for c in examined],
                )
            hand_card = next(
                (c for c in examined if c.id == chosen_id), None
            )
            if hand_card is None:
                hand_card = examined[0]
            # Move the chosen card to hand.
            p.library.remove(hand_card)
            hand_card.zone = Zone.HAND
            p.hand.append(hand_card)
            reveal_to_all(state, hand_card)
            state.log.append(
                f"P{controller_id} puts {hand_card.printing.name} into "
                f"hand (Commune with Evil)"
            )
            # Move the rest of the examined cards to graveyard.
            for c in [c for c in examined if c is not hand_card]:
                p.library.remove(c)
                c.zone = Zone.GRAVEYARD
                p.graveyard.append(c)
                state.log.append(
                    f"P{controller_id} puts {c.printing.name} into "
                    f"graveyard (Commune with Evil)"
                )
        # Trailing life-gain rider always fires, even with an empty
        # library (parallel to GainLife resolving against the same
        # state path used everywhere else in the codebase).
        gain_life(state, controller_id, 3)
        state.log.append(
            f"P{controller_id} gains 3 life "
            f"(life: {p.life}; Commune with Evil)"
        )

    def describe(self) -> str:
        return (
            "look at the top four cards of your library; put one of "
            "them into your hand and the rest into your graveyard; "
            "you gain 3 life"
        )


@dataclass(frozen=True)
class HouseCartographerSurvivalEffect(Effect):
    """House Cartographer's Survival body: "Reveal cards from the top of
    your library until you reveal a land card. Put that card into your
    hand and the rest on the bottom of your library in a random order."

    The reveal-until-land mechanic is a one-shot library walk:
    1. Pop cards from the top of the controller's library until either
       the library is empty OR a land card is revealed.
    2. The revealed land card (if any) goes to the controller's hand.
    3. All other revealed cards (the non-land "rest") go to the bottom
       of the library in a random order — implemented by shuffling the
       collected non-lands and appending them.

    If the library has no land cards at all, every revealed card is
    non-land; they all go to the bottom in random order and the library
    ends up shuffled. Matches CR 701.18a (search-and-reveal effects
    with "in a random order" tail).

    The shuffling uses ``state.rng`` so the choice is reproducible
    under a fixed-seed simulation (mirrors how every other random-order
    primitive in the codebase folds in).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        revealed: list = []
        land_card = None
        while p.library:
            top = p.library.pop(0)
            if top.printing.is_land:
                land_card = top
                break
            revealed.append(top)
        if land_card is not None:
            land_card.zone = Zone.HAND
            p.hand.append(land_card)
            state.log.append(
                f"P{controller_id} reveals {land_card.printing.name} "
                f"and puts it into hand (House Cartographer)"
            )
        else:
            state.log.append(
                f"P{controller_id} found no land in their library "
                f"(House Cartographer)"
            )
        # Non-lands go to the bottom of the library in a random order.
        state.rng.shuffle(revealed)
        for card in revealed:
            card.zone = Zone.LIBRARY
            p.library.append(card)
        if revealed:
            state.log.append(
                f"P{controller_id} puts {len(revealed)} non-land card(s) "
                f"on the bottom of their library in a random order"
            )

    def describe(self) -> str:
        return (
            "reveal cards from the top of your library until you reveal "
            "a land card; put that card into your hand and the rest on "
            "the bottom of your library in a random order"
        )



@dataclass(frozen=True)
class BoilerbilgesRipperETBEffect(Effect):
    """Boilerbilges Ripper's ETB: "You may sacrifice another creature or
    enchantment. When you do, this creature deals 2 damage to any
    target."

    The sacrifice is optional. If declined (or no legal candidate
    exists), the entire reflexive rider does nothing — no damage. If
    the controller sacrifices, the source deals 2 damage to one chosen
    "any target" (creature or player). Targets are reused from the
    parent ETB's target slot via the standard target-selection
    pipeline; ``legal_targets`` mirrors ``DealDamageAnyTarget`` so the
    cast-time pool is the standard creature ∪ player set.

    "Another creature or enchantment" excludes the source itself even
    when Boilerbilges Ripper has no other creature/enchantment subtypes
    — CR 109.5's "another" exclusion. The candidate pool checks
    ``is_creature`` OR ``is_enchantment`` on the printing layer (room
    halves and enchantment creatures both qualify since
    ``is_enchantment`` is True for any printing carrying
    ``"Enchantment"`` in ``card_types``).
    """

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_creature_now
        targets: list[Target] = []
        for o in state.battlefield:
            if is_creature_now(o):
                targets.append(("creature", o.id))
        for p in state.players:
            if not p.has_lost:
                targets.append(("player", p.id))
        return targets

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice as _sacrifice

        source = state.resolving_source
        p = state.player_by_id(controller_id)
        source_id = source.id if source is not None else None
        candidates = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and o.id != source_id
            and (o.printing.is_creature or o.printing.is_enchantment)
        ]
        if not candidates:
            state.log.append(
                "Boilerbilges Ripper: no legal sacrifice candidate — "
                "ETB does nothing"
            )
            return
        if p.agent is None or not p.agent.choose_yes_no(
            state, controller_id,
            "Sacrifice another creature or enchantment?",
        ):
            state.log.append(
                f"P{controller_id} declines to sacrifice "
                f"(Boilerbilges Ripper)"
            )
            return
        pick_id = p.agent.choose_sacrifice(
            state, controller_id, [o.id for o in candidates],
        )
        chosen = next(
            (o for o in candidates if o.id == pick_id), candidates[0],
        )
        state.log.append(
            f"P{controller_id} sacrifices {chosen.printing.name} "
            f"(Boilerbilges Ripper)"
        )
        _sacrifice(state, chosen)
        if not targets:
            return
        _apply_damage(state, targets[0], 2)

    def describe(self) -> str:
        return (
            "you may sacrifice another creature or enchantment; if you "
            "do, this creature deals 2 damage to any target"
        )



@dataclass(frozen=True)
class FanaticOfTheHarrowingETBEffect(Effect):
    """Fanatic of the Harrowing's ETB: "Each player discards a card. If
    you discarded a card this way, draw a card."

    Resolution order matches the printed clause:
    1. Each player (in turn order, starting with the controller and
       proceeding to opponents) discards one card via ``choose_discard``.
       A player with an empty hand simply skips — they had no card to
       discard.
    2. If the controller actually discarded a card in step 1 (i.e. they
       had a non-empty hand), they draw a card.

    The "if you discarded a card this way" gate is a state-based check
    on the controller specifically — not on whether anyone discarded.
    A controller with an empty hand at resolve time (rare, but possible
    after a previous discard ladder) gets nothing.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import draw_n_cards, fire_opponent_discards_triggers

        controller_discarded = False
        # Iterate in turn order starting from the controller, then each
        # other player (also in turn order). Each player's own agent
        # picks which card.
        n = len(state.players)
        order = [
            state.players[(state.players.index(state.player_by_id(controller_id)) + i) % n]
            for i in range(n)
        ]
        for p in order:
            if p.has_lost or not p.hand:
                continue
            from mtgz_ir.player_protections import opponent_effect_blocked_for

            if opponent_effect_blocked_for(state, p.id):
                continue
            hand_ids = [c.id for c in p.hand]
            chosen_id = p.agent.choose_discard(state, p.id, hand_ids)
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(
                f"P{p.id} discards {card.printing.name} "
                f"(Fanatic of the Harrowing)"
            )
            fire_opponent_discards_triggers(state, p.id, card=card)
            if p.id == controller_id:
                controller_discarded = True
        if controller_discarded:
            draw_n_cards(state, controller_id, 1)

    def describe(self) -> str:
        return (
            "each player discards a card; if you discarded a card this "
            "way, draw a card"
        )



# Splitskin Doll's ETB no longer needs a bespoke engine class — its
# printed body decomposes into the generic
#   Sequence((DrawCards(amount=1),
#             IfNot(condition=ControlsAnotherCreatureWithPowerAtMost(2),
#                   then=DiscardSelf(amount=1))))
# composition. The parser's multi-sentence literal recognizer in
# ``parser/effects._parse_effect`` returns that Sequence directly; the
# engine resolves it via the generic ``Sequence``/``IfNot``
# combinators and ``DrawCards`` / ``DiscardSelf`` primitives. See
# ``audit: decompose Splitskin Doll`` for the migration.


@dataclass(frozen=True)
class ArabellaAttackEffect(Effect):
    """Arabella, Abandoned Doll's attack trigger:

    "Whenever Arabella attacks, it deals X damage to each opponent and
    you gain X life, where X is the number of creatures you control with
    power 2 or less."

    X is computed once at resolve time as the count of creatures the
    controller controls whose ``effective_power`` is 2 or less. The
    source itself counts when its own effective power is 2 or less (the
    printed clause says "creatures you control" with no "other"
    exclusion). The damage is dealt by the source via ``_apply_damage``
    so lifelink / replacement effects on the source cohabit cleanly.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power

        x = 0
        for o in state.battlefield:
            if o.controller_id != controller_id:
                continue
            if not o.printing.is_creature:
                continue
            try:
                p_val = effective_power(state, o)
            except Exception:
                continue
            if p_val <= 2:
                x += 1
        if x <= 0:
            return
        # Damage each opponent X. ``_apply_damage`` routes through the
        # standard damage pipeline so lifelink / replacement effects fire
        # off the source as expected.
        for opp in state.players:
            if opp.id == controller_id or opp.has_lost:
                continue
            _apply_damage(state, ("player", opp.id), x)
            state.log.append(
                f"Arabella deals {x} to P{opp.id} (life: {opp.life})"
            )
        # Gain X life.
        you = state.player_by_id(controller_id)
        you.life += x
        state.log.append(
            f"P{controller_id} gains {x} life (Arabella) — life: {you.life}"
        )

    def describe(self) -> str:
        return (
            "Arabella deals X damage to each opponent and you gain X "
            "life, where X is the number of creatures you control with "
            "power 2 or less"
        )



# NOTE: ``AttackInTheBoxAttackEffect`` was deleted in
# audit-litdispatch-creatures. Attack-in-the-Box's attack-trigger body
# now decomposes into the generic combinator+primitive composition
# ``IfYouDo(MayDo(PumpSelfUntilEOT(power_bonus=4, toughness_bonus=0)),
# ScheduleSelfSacAtNextEndStep())`` via a multi-sentence literal
# recognizer in ``cards/parser/effects.py``. ``ScheduleSelfSacAtNextEndStep``
# (new in ``effects/generic.py``) schedules the delayed sac at the
# current turn's end step exactly as the deleted bespoke class did.

# NOTE: ``WickerfolkThresherAttackEffect`` was deleted in
# audit-litdispatch-creatures. Wickerfolk Thresher's Delirium attack
# body now dispatches to the canonical
# ``PeekTopMayBattlefieldIfLandElseHand`` primitive (new in
# ``effects/generic.py``) via a multi-sentence literal recognizer in
# ``cards/parser/effects.py``. The new primitive routes the
# put-onto-battlefield path through ``put_into_battlefield`` so
# landfall and other ETB triggers fire via the standard pipeline; the
# bespoke implementation manually called ``fire_landfall_triggers``
# after a direct ``state.battlefield.append`` which bypassed
# ``Event.ENTERS`` dispatch.


# NOTE: ``OsseousSticktwisterEndStepEffect`` was deleted in
# audit-per-opponent-choice. Osseous Sticktwister's Delirium-gated
# end-step body now decomposes into the generic
# ``EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage`` primitive (in
# ``effects/generic.py``) via a multi-sentence literal recognizer in
# ``cards/parser/effects.py``. The Delirium intervening-if gate parses
# via the unified ``_DELIRIUM_AT_BEGINNING_GATE_PREFIX`` and is composed
# onto the trigger predicate as ``StateBound(Delirium())``.



@dataclass(frozen=True)
class DealSelfMVDamageToEachCreature(Effect):
    """LTB-trigger effect for Terminal Velocity: when the put permanent
    leaves the battlefield, it deals damage equal to its own mana value
    to each creature.

    ``state.resolving_source`` is the departing permanent (set by the
    LEAVES_BATTLEFIELD dispatcher via ``extra_observers``), so the MV
    is read from there. Snapshot semantics: victim list is captured before
    any damage is applied to avoid mid-pass iterator invalidation.

    No-op if resolving_source is None (should not happen in practice).
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        mv = source.printing.mana_cost.total
        if mv <= 0:
            state.log.append(
                f"{source.printing.name} leaves the battlefield; "
                f"MV is 0, no damage dealt (Terminal Velocity trigger)"
            )
            return
        victims = [o for o in state.battlefield if o.printing.is_creature]
        state.log.append(
            f"{source.printing.name} leaves the battlefield; "
            f"deals {mv} damage to each creature "
            f"(Terminal Velocity trigger)"
        )
        for victim in victims:
            _apply_damage(state, ("creature", victim.id), mv)

    def describe(self) -> str:
        return (
            "when this permanent leaves the battlefield, it deals "
            "damage equal to its mana value to each creature"
        )



@dataclass(frozen=True)
class TerminalVelocityEffect(Effect):
    """Terminal Velocity ``{4}{R}{R}`` Sorcery:
    "You may put an artifact or creature card from your hand onto the
    battlefield. That permanent gains haste, \"When this permanent
    leaves the battlefield, it deals damage equal to its mana value to
    each creature,\" and \"At the beginning of your end step, sacrifice
    this permanent.\""

    Resolution sequence:
    1. Collect artifact/creature cards from the caster's hand.
    2. Ask the agent via ``choose_card_from_hand`` which one to put
       on the battlefield (returning None declines the "you may").
    3. Move the chosen card from hand to the battlefield via
       ``put_into_battlefield``.
    4. Grant the card haste until end of turn via
       ``ApplyEffect(GrantKeywords({"haste"}), EndOfTurn())``.
    5. Grant the card a LEAVES_BATTLEFIELD predicate trigger (UEOT)
       via ``ApplyEffect(GrantTriggeredAbility(...), EndOfTurn())``;
       the trigger fires ``DealSelfMVDamageToEachCreature``.
    6. Schedule a ``SacrificeSourceDelayed`` delayed trigger at the
       caster's current end step (``fire_at_turn = turn_number + 0``
       if it's the caster's turn, or ``+1`` if it's the opponent's
       turn — same end-step the sorcery resolves on, for the
       "beginning of your end step" wording).

    The UEOT LTB trigger (step 5) expires at end-of-turn cleanup, which
    happens AFTER the end-step sac fires (step 6 → sac resolves → LTB
    trigger placed on stack → LTB resolves), so the grant is still
    active when the trigger needs to fire. Cleanup wipes it afterwards.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import is_artifact_now
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic import (
            GrantTriggeredAbility, SacrificeSourceDelayed,
        )
        from mtgz_ir.effects.predicates import Event, IsSelf

        p = state.player_by_id(controller_id)
        # Collect qualifying cards: artifact or creature cards in hand.
        qualifying = [
            c for c in p.hand
            if is_artifact_now(state, c) or c.printing.is_creature
        ]
        if not qualifying:
            state.log.append(
                f"P{controller_id} has no artifact or creature card "
                f"in hand to put onto the battlefield (Terminal Velocity)"
            )
            return
        agent = p.agent
        if agent is None:
            return
        option_ids = [c.id for c in qualifying]
        chosen_id = agent.choose_card_from_hand(state, controller_id, option_ids)
        if chosen_id is None:
            state.log.append(
                f"P{controller_id} declines to put a card onto the "
                f"battlefield (Terminal Velocity)"
            )
            return
        card = next((c for c in p.hand if c.id == chosen_id), None)
        if card is None:
            return
        # Move from hand to battlefield.
        p.hand.remove(card)
        put_into_battlefield(state, card, controller_id)
        state.log.append(
            f"P{controller_id} puts {card.printing.name} onto the "
            f"battlefield via Terminal Velocity"
        )
        # Grant haste until end of turn.
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic import (
            GrantKeywords, GrantTriggeredAbility,
        )

        ApplyEffect(
            inner=GrantKeywords(keywords=frozenset({"haste"})),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", card.id),))
        # Grant LTB trigger (UEOT): when this permanent leaves, deal
        # MV damage to each creature.
        ltb_trigger = TriggeredAbility(
            event=Event.LEAVES_BATTLEFIELD,
            predicate=IsSelf(),
            effect=DealSelfMVDamageToEachCreature(),
        )
        ApplyEffect(
            inner=GrantTriggeredAbility(ability=ltb_trigger),
            ends_at=EndOfTurn(),
        ).resolve(state, controller_id, (("creature", card.id),))
        # Schedule end-step sac.
        # "At the beginning of your end step" — the end step that
        # belongs to the CASTER (controller_id). Turn offset: 0 if
        # it's currently the caster's turn (the sorcery resolves in
        # their main phase, so the end step is this same turn), 1 if
        # it's the opponent's turn.
        offset = 0 if state.active_player().id == controller_id else 1
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number + offset,
            "controller_id": controller_id,
            "source_id": card.id,
            "effect": SacrificeSourceDelayed(),
        })
        state.log.append(
            f"Terminal Velocity: {card.printing.name} will be sacrificed "
            f"at the beginning of P{controller_id}'s end step "
            f"(turn {state.turn_number + offset})"
        )

    def describe(self) -> str:
        return (
            "you may put an artifact or creature card from your hand "
            "onto the battlefield; it gains haste, a leaves-the-"
            "battlefield deal-MV-damage-to-each-creature trigger, and "
            "is sacrificed at the beginning of your end step"
        )



@dataclass(frozen=True)
class SurveyMechanActivationEffect(Effect):
    """Survey Mechan's activated ability body:
    "It deals 3 damage to any target. Target player draws three cards
    and gains 3 life."

    Two REQUIRED targets (CR 601.2c — both slots must be filled to
    activate):

    - Slot 0: ``("any_damage_target", id)`` — the target for 3 damage.
      Legal kinds at cast time: creature, planeswalker, battle, player.
    - Slot 1: ``("player", id)`` — the player who draws 3 cards and
      gains 3 life.

    The two slots are deliberately distinct tag kinds so the targeting
    pipeline can present both pools separately. If either pool is
    empty, ``legal_targets`` returns ``[]`` so the ability can't be
    legally activated (no legal-target set for the mandatory slot).

    At resolve time each sub-effect re-checks independently per
    CR 608.2b: if the damage target is gone, damage fizzles; if the
    player target is gone (has already lost), the draw+life fizzles —
    the other sub-effect still applies.
    """

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def target_slot_kinds(self) -> tuple[str, ...]:
        # Slot 0: any damage target. Slot 1: target player.
        return ("any_damage_target", "player")

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        from mtgz_ir.game import is_creature_now

        # Slot 0: any target (creature, planeswalker, battle, player) —
        # tagged "any_damage_target" so the agent pipeline can pick one
        # independently of the "player" slot below.
        any_pool: list[Target] = []
        for o in state.battlefield:
            if is_creature_now(o):
                any_pool.append(("any_damage_target", o.id))
            elif "Planeswalker" in o.printing.card_types:
                any_pool.append(("any_damage_target", o.id))
            elif "Battle" in o.printing.card_types:
                any_pool.append(("any_damage_target", o.id))
        for p in state.players:
            if not p.has_lost:
                any_pool.append(("any_damage_target", p.id))

        # Slot 1: any living player — tagged "player" so the agent
        # picks the draw+gain recipient separately from the damage slot.
        player_pool: list[Target] = [
            ("player", p.id)
            for p in state.players
            if not p.has_lost
        ]
        # Both slots must be satisfiable for the ability to be legally
        # activated. If either pool is empty, return [] so legal_actions
        # sees no valid activation.
        if not any_pool or not player_pool:
            return []
        return any_pool + player_pool

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import draw_n_cards
        from mtgz_ir.game import gain_life

        # Find each slot in the committed target tuple by kind.
        damage_target = next(
            (t for t in targets if t[0] == "any_damage_target"), None
        )
        player_target = next(
            (t for t in targets if t[0] == "player"), None
        )

        # Sub-effect 1: deal 3 damage to any target.
        if damage_target is not None:
            # The damage slot uses the "any_damage_target" tag, which
            # needs remapping to the canonical ("creature"|"player",
            # id) tuple that _apply_damage expects.
            _kind, _id = damage_target
            # Check if the target is a battlefield permanent or a player.
            bf_obj = next(
                (o for o in state.battlefield if o.id == _id), None
            )
            if bf_obj is not None:
                _apply_damage(state, ("creature", _id), 3)
            else:
                # Must be a player target inside the any_damage_target
                # pool (legal_targets includes players there too).
                try:
                    _p = state.player_by_id(_id)
                    from mtgz_ir.game import player_damage_prevented
                    if not player_damage_prevented(state, _id):
                        _p.life -= 3
                        state.log.append(
                            f"P{_id} takes 3 damage from Survey Mechan "
                            f"(life: {_p.life})"
                        )
                except KeyError:
                    state.log.append(
                        f"Survey Mechan damage target P{_id} not found"
                    )

        # Sub-effect 2: target player draws 3 and gains 3 life.
        if player_target is not None:
            _pkind, _pid = player_target
            try:
                _p = state.player_by_id(_pid)
                if not _p.has_lost:
                    drawn = draw_n_cards(state, _pid, 3)
                    state.log.append(
                        f"P{_pid} draws {drawn} card(s) "
                        f"(Survey Mechan activation)"
                    )
                    gain_life(state, _pid, 3)
                    state.log.append(
                        f"P{_pid} gains 3 life "
                        f"(Survey Mechan activation)"
                    )
            except KeyError:
                state.log.append(
                    f"Survey Mechan draw/life target P{_pid} not found"
                )

    def describe(self) -> str:
        return (
            "it deals 3 damage to any target; "
            "target player draws three cards and gains 3 life"
        )


