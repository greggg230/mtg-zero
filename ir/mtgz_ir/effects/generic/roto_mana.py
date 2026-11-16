"""Engine effects for the Vintage Roto P3 bucket: mana, costs and cast
permissions (``docs/ROTO_COVERAGE_PLAN.md`` section 4).

The parser side lives in ``cards/parser/roto_mana.py``; every effect
here is built only by that grammar. Formulas and predicates the effects
need sit next to them so the shared ``effects/formulas.py`` /
``object_predicates.py`` modules stay untouched by this package.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect, Target
from mtgz_ir.effects.formulas import Formula
from mtgz_ir.effects.generic.mana import AddManaToPool
from mtgz_ir.effects.object_predicates import ObjectPredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


# ---------------------------------------------------------------------------
# Formulas
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CardsNamedInAllGraveyards(Formula):
    """The number of cards named ``name`` in every graveyard (Rite of
    Flame's "for each card named Rite of Flame in each graveyard"). The
    resolving spell is on the stack, so it never counts itself."""

    name: str

    def value(self, state: "GameState", controller_id: int) -> int:
        return sum(
            1 for p in state.players for c in p.graveyard
            if c.printing.name == self.name
        )


@dataclass(frozen=True)
class SourcePower(Formula):
    """The resolving source's power ("where X is Vivi Ornitier's
    power")."""

    def value(self, state: "GameState", controller_id: int) -> int:
        return max(0, _source_power(state))


from mtgz_ir.effects.generic.artifact_mechanics import (  # noqa: E402
    AddManaInAnyCombination,
)


@dataclass(frozen=True)
class AddFormulaManaInAnyCombination(AddManaInAnyCombination):
    """"Add X mana in any combination of <colors>, where X is <formula>."
    (Burnt Offering: the sacrificed creature's mana value; Vivi Ornitier:
    its power). Each coin's color is asked of ``Agent.choose_mana_color``.
    An ``AddManaInAnyCombination``, so on an activated ability it is a
    mana ability (CR 605)."""

    formula: "Formula | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        n = self.formula.value(state, controller_id)
        p = state.player_by_id(controller_id)
        minted = []
        for _ in range(max(0, n)):
            chosen = self.options[0]
            if p.agent is not None:
                picked = p.agent.choose_mana_color(
                    state, controller_id, list(self.options),
                )
                if picked in self.options:
                    chosen = picked
            p.mana_pool.append(ManaCoin(color=chosen))
            minted.append(chosen)
        state.log.append(
            f"P{controller_id} adds {'+'.join(minted) or 'no mana'}"
        )

    def describe(self) -> str:
        return (
            f"add X mana in any combination of {'/'.join(self.options)} "
            f"({self.formula!r})"
        )


@dataclass(frozen=True)
class DestroyEachThenAddManaPerDestroyed(Effect):
    """"Destroy each <np>. Add {B} or {G} for each permanent destroyed
    this way." (Culling Ritual): ``destroy`` runs, then one coin per
    victim that actually left the battlefield (an indestructible or
    regenerated one wasn't destroyed), each of a color from ``options``
    chosen with ``Agent.choose_mana_color``."""

    destroy: "Effect | None" = None
    options: tuple = ()

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            victims = [
                o for o in state.battlefield
                if self.destroy.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        self.destroy.resolve(state, controller_id, targets)
        n = sum(1 for o in victims if o not in state.battlefield)
        p = state.player_by_id(controller_id)
        for _ in range(n):
            chosen = self.options[0]
            if p.agent is not None:
                picked = p.agent.choose_mana_color(
                    state, controller_id, list(self.options),
                )
                if picked in self.options:
                    chosen = picked
            p.mana_pool.append(ManaCoin(color=chosen))
        state.log.append(
            f"P{controller_id} adds {n} mana ({'/'.join(self.options)})"
        )

    def describe(self) -> str:
        return (
            f"{self.destroy.describe()}; add {'/'.join(self.options)} for "
            "each permanent destroyed this way"
        )


@dataclass(frozen=True)
class ReturnPermanentYouControlToHand(Effect):
    """"Return a[n] <permanent> you control to its owner's hand."
    (Simic Growth Chamber) -- not targeted: the permanent is chosen as the
    ability resolves (``choose_sacrifice``, the engine's "which of my
    permanents" hook); the source itself may be chosen."""

    permanent_filter: "ObjectPredicate | None" = None
    label: str = "permanent"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.targets_helpers import _bounce_to_hand

        prior = state.resolving_controller_id
        try:
            state.resolving_controller_id = controller_id
            options = [
                o for o in state.battlefield
                if o.controller_id == controller_id
                and self.permanent_filter.test(o, state)
            ]
        finally:
            state.resolving_controller_id = prior
        if not options:
            state.log.append(f"no {self.label} to return")
            return
        chosen = options[0]
        agent = state.player_by_id(controller_id).agent
        if agent is not None and len(options) > 1:
            pick = agent.choose_sacrifice(
                state, controller_id, [o.id for o in options],
            )
            chosen = next((o for o in options if o.id == pick), options[0])
        state.log.append(
            f"P{controller_id} returns {chosen.printing.name} to hand"
        )
        _bounce_to_hand(state, chosen)

    def describe(self) -> str:
        return f"return a {self.label} you control to its owner's hand"


@dataclass(frozen=True)
class LookAtTopMayPutOntoBattlefieldRestBottom(Effect):
    """"Look at the top N cards of your library. You may put a[n]
    <filter> card from among them onto the battlefield. Put the rest on
    the bottom of your library in a random order." (Kinnan, Bonder
    Prodigy: a non-Human creature card). ``excluded_subtype`` carries the
    "non-<Subtype>" qualifier the card-descriptor grammar lacks."""

    count: int = 1
    card_filter: object = None
    excluded_subtype: str = ""

    def _ok(self, state, card) -> bool:
        return self.card_filter.matches_card(state, card) and (
            not self.excluded_subtype
            or self.excluded_subtype not in card.printing.subtypes
        )

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield
        from mtgz_ir.game import Zone, reveal_to

        p = state.player_by_id(controller_id)
        n = min(self.count, len(p.library))
        looked = [p.library.pop(0) for _ in range(n)]
        for c in looked:
            reveal_to(c, controller_id)
        options = [c for c in looked if self._ok(state, c)]
        chosen = None
        if options and p.agent is not None:
            pick = p.agent.choose_card_from_library(
                state, controller_id, [c.id for c in options],
            )
            chosen = next((c for c in options if c.id == pick), None)
        rest = [c for c in looked if c is not chosen]
        state.rng.shuffle(rest)
        for c in rest:
            c.zone = Zone.LIBRARY
            p.library.append(c)
        if chosen is not None:
            state.log.append(
                f"P{controller_id} puts {chosen.printing.name} onto the "
                "battlefield"
            )
            put_into_battlefield(state, chosen, controller_id)
        state.log.append(
            f"P{controller_id} looks at {n} card(s); {len(rest)} go to the "
            "bottom"
        )

    def describe(self) -> str:
        qual = f"non-{self.excluded_subtype} " if self.excluded_subtype else ""
        return (
            f"look at the top {self.count} cards; you may put a {qual}"
            f"{self.card_filter.describe()} onto the battlefield; the rest "
            "go to the bottom in a random order"
        )


@dataclass(frozen=True)
class SacrificeSourceUnlessSacrificeTotalPower(Effect):
    """"Sacrifice it unless you sacrifice any number of creatures with
    total power N or greater." (Phyrexian Dreadnought). When the
    creatures its controller controls (the source included) reach N
    power, ``choose_yes_no`` decides whether to pay; the creatures are
    then picked one at a time (``choose_sacrifice``) until their total
    power reaches N. Otherwise the source is sacrificed."""

    total_power: int = 0

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import effective_power, is_creature_now
        from mtgz_ir.turn import sacrifice

        src = state.resolving_source
        pool = [
            o for o in state.battlefield
            if o.controller_id == controller_id and is_creature_now(o)
        ]
        agent = state.player_by_id(controller_id).agent
        can_pay = (
            sum(max(0, effective_power(state, o)) for o in pool)
            >= self.total_power
        )
        if can_pay and (agent is None or agent.choose_yes_no(
            state, controller_id,
            f"Sacrifice creatures with total power {self.total_power}?",
        )):
            paid = 0
            chosen: list = []
            while paid < self.total_power:
                options = [o for o in pool if o not in chosen]
                pick = options[0]
                if agent is not None:
                    pid = agent.choose_sacrifice(
                        state, controller_id, [o.id for o in options],
                    )
                    pick = next((o for o in options if o.id == pid), pick)
                chosen.append(pick)
                paid += max(0, effective_power(state, pick))
            for o in chosen:
                if o in state.battlefield:
                    sacrifice(state, o)
            return
        if src is not None and src in state.battlefield:
            sacrifice(state, src)

    def describe(self) -> str:
        return (
            "sacrifice it unless you sacrifice creatures with total power "
            f"{self.total_power} or greater"
        )


# ---------------------------------------------------------------------------
# Energy
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GetXEnergyPayAnyDestroyMVAtMostPaid(Effect):
    """"You get X {E}, then you may pay any amount of {E}. Destroy each
    <np> with mana value less than or equal to the amount of {E} paid
    this way." (Wrath of the Skies). X is the spell's X; the amount paid
    (any of ALL the controller's energy) is ``choose_x``."""

    destroy: "Effect | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from dataclasses import replace

        p = state.player_by_id(controller_id)
        p.energy += max(0, state.resolving_x_value)
        paid = p.energy
        if p.agent is not None:
            paid = p.agent.choose_x(
                state, controller_id, "energy to pay", p.energy,
            )
        paid = max(0, min(paid, p.energy))
        p.energy -= paid
        state.log.append(
            f"P{controller_id} pays {paid} energy (now {p.energy})"
        )
        from mtgz_ir.effects.value_predicates import HasManaValueAtMost

        replace(
            self.destroy,
            permanent_filter=self.destroy.permanent_filter
            & HasManaValueAtMost(n=paid),
        ).resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            "you get X energy, then pay any amount; "
            f"{self.destroy.describe()} with mana value up to the amount paid"
        )


@dataclass(frozen=True)
class GetEnergyThenExileUntilNonlandCastForEnergy(Effect):
    """"You get {E}{E}. Then if you cast it from your hand, exile cards
    from the top of your library until you exile a nonland card. You may
    cast that card by paying an amount of {E} equal to its mana value
    rather than paying its mana cost." (Amped Raptor). The card is cast
    during the resolution (``cast_now``, timing ignored) after the energy
    is paid; if it isn't cast it stays exiled."""

    energy: int = 2

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        p.energy += self.energy
        state.log.append(
            f"P{controller_id} gets {self.energy} energy (now {p.energy})"
        )
        src = state.resolving_source
        if src is None or not src.entered_from_hand_cast:
            return
        found = None
        while p.library:
            card = p.library.pop(0)
            card.zone = Zone.EXILE
            p.exile.append(card)
            state.log.append(f"P{controller_id} exiles {card.printing.name}")
            if not card.printing.is_land:
                found = card
                break
        if found is None:
            return
        mv = found.printing.mana_cost.total
        if p.energy < mv or not can_cast_now(
            state, controller_id, found, free=True,
        ):
            return
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            f"Pay {mv} energy to cast {found.printing.name}?",
        ):
            return
        p.energy -= mv
        state.log.append(f"P{controller_id} pays {mv} energy (now {p.energy})")
        cast_now(state, controller_id, found, free=True)

    def describe(self) -> str:
        return (
            f"you get {self.energy} energy; if cast from hand, exile until a "
            "nonland card and you may cast it paying energy equal to its "
            "mana value"
        )


# ---------------------------------------------------------------------------
# Mana spent to cast
# ---------------------------------------------------------------------------


def _object_by_id(state: "GameState", obj_id: int):
    """The object with ``obj_id`` wherever it is now (stack, battlefield,
    any player's zones) -- its fields keep the cast-time facts."""
    for item in state.stack:
        src = getattr(item, "source", None)
        if src is not None and src.id == obj_id:
            return src
    for o in state.battlefield:
        if o.id == obj_id:
            return o
    for p in state.players:
        for zone in (p.graveyard, p.exile, p.hand, p.library):
            for c in zone:
                if c.id == obj_id:
                    return c
    return None


@dataclass(frozen=True)
class MillTargetPlayerMoreIfManaSpent(Effect):
    """"Target player mills N cards. If M or more mana was spent to cast
    that spell, that player mills K cards instead." (Exhibition
    Tidecaller's Opus) -- "that spell" is the cast spell that triggered
    it (``event_subject_id``), read through ``mana_spent_to_cast``."""

    amount: int = 1
    amount_if_spent: int = 1
    mana_spent_at_least: int = 0

    def event_subject_id(self, subjects) -> int:
        spell = subjects.get("spell")
        return spell.id if spell is not None else -1

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return [("player", p.id) for p in state.players if not p.has_lost]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.draw_mill import MillTargetPlayer

        spell = _object_by_id(state, state.resolving_event_subject_id)
        spent = spell.mana_spent_to_cast if spell is not None else 0
        n = (
            self.amount_if_spent if spent >= self.mana_spent_at_least
            else self.amount
        )
        MillTargetPlayer(amount=n).resolve(state, controller_id, targets)

    def describe(self) -> str:
        return (
            f"target player mills {self.amount} ({self.amount_if_spent} if "
            f"{self.mana_spent_at_least}+ mana was spent on that spell)"
        )


def _colors_spent_on_resolving_spell(state: "GameState") -> int:
    spell = state.resolving_spell
    return spell.sunburst_colors_spent if spell is not None else 0


@dataclass(frozen=True)
class ConvergeDrawDamageGainLife(Effect):
    """"Converge -- Target player draws X cards, <this> deals X damage to
    any target, and you gain X life, where X is the number of colors of
    mana spent to cast this spell." (Together as One). Two target slots:
    the player (slot 0) and any target (slot 1, from ``damage``'s
    legal targets)."""

    damage: "Effect | None" = None

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (2, 2)

    def target_slot_kinds(self) -> tuple[str, ...]:
        return ("player",)

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        players = [("player", p.id) for p in state.players if not p.has_lost]
        rest = [
            t for t in self.damage.legal_targets(state, controller_id)
            if t not in players
        ]
        return players + rest

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage
        from mtgz_ir.game import gain_life
        from mtgz_ir.turn import draw_n_cards

        x = _colors_spent_on_resolving_spell(state)
        state.log.append(f"converge: X = {x}")
        if x <= 0:
            return
        if targets and targets[0][0] == "player":
            draw_n_cards(state, targets[0][1], x)
        if len(targets) > 1 and targets[1] in self.damage.legal_targets(
            state, controller_id,
        ):
            _apply_damage(state, targets[1], x)
        gain_life(state, controller_id, x)

    def describe(self) -> str:
        return (
            "target player draws X cards, X damage to any target, you gain X "
            "life (X = colors of mana spent)"
        )


@dataclass(frozen=True)
class MayAddManaPerTargetOpponentsSubtype(Effect):
    """"[If you haven't added mana with this ability this turn, ]you may
    add X mana of any one color, where X is the number of <Subtype>s
    target opponent controls." (Carpet of Flowers). The "haven't added"
    intervening-if is ``HasNotAddedManaWithThisAbilityThisTurn`` on the
    trigger and is re-checked here; adding mana (X > 0) records the turn
    on the source."""

    subtype: str = "Island"

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin, effective_subtypes

        src = state.resolving_source
        if src is not None and src.ability_mana_added_turn == state.turn_number:
            return
        if not targets or targets[0][0] != "player":
            return
        opp_id = targets[0][1]
        x = sum(
            1 for o in state.battlefield
            if o.controller_id == opp_id
            and self.subtype in effective_subtypes(state, o)
        )
        p = state.player_by_id(controller_id)
        if x <= 0 or (p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, f"Add {x} mana of one color?",
        )):
            return
        colors = ["W", "U", "B", "R", "G"]
        color = colors[0]
        if p.agent is not None:
            picked = p.agent.choose_mana_color(state, controller_id, colors)
            if picked in colors:
                color = picked
        p.mana_pool.extend(ManaCoin(color=color) for _ in range(x))
        if src is not None:
            src.ability_mana_added_turn = state.turn_number
        state.log.append(f"P{controller_id} adds {{{color}}}x{x}")

    def describe(self) -> str:
        return (
            f"you may add X mana of any one color (X = {self.subtype}s "
            "target opponent controls)"
        )


@dataclass(frozen=True)
class OnceEachTurnCost:
    """An ``ActivationCost.extra_costs`` atom for "Activate only once
    each turn." (Vivi Ornitier): payable while the source hasn't used the
    ability (``key``) this turn; paying records the turn on the source
    (``GameObject.once_each_turn_used``)."""

    key: str = ""

    def payable(self, state, player_id: int, source) -> bool:
        return (state.turn_number, self.key) not in source.once_each_turn_used

    def pay(self, state, player_id: int, source) -> list:
        source.once_each_turn_used = tuple(
            entry for entry in source.once_each_turn_used
            if entry[0] == state.turn_number
        ) + ((state.turn_number, self.key),)
        return []

    def describe(self) -> str:
        return "activate only once each turn"


@dataclass(frozen=True)
class PayXEnergyCost:
    """"Pay X {E}" where X is the mana value of the ability's targeted
    graveyard card (Chthonian Nightmare's "Return target creature card
    with mana value X"): choosing that card IS choosing X, so the energy
    is paid as the targets are locked in (``x_from_cost_atoms``) and this
    atom's own ``pay`` has nothing left to do. Payability is enforced by
    the effect's targets (cards with mana value up to your energy)."""

    def payable(self, state, player_id: int, source) -> bool:
        return True

    def pay(self, state, player_id: int, source) -> list:
        return []

    def describe(self) -> str:
        return "Pay X {E}"


def x_from_cost_atoms(
    state: "GameState", player_id: int, ability, targets, x: int,
) -> int:
    """The activation's X once targets are chosen: with a
    ``PayXEnergyCost`` atom it is the targeted graveyard card's mana
    value, and that much energy is paid now."""
    if not any(isinstance(a, PayXEnergyCost) for a in ability.cost.extra_costs):
        return x
    p = state.player_by_id(player_id)
    card = next(
        (c for c in p.graveyard if targets and c.id == targets[0][1]), None,
    )
    if card is None:
        return x
    x = card.printing.mana_cost.total
    p.energy -= x
    state.log.append(f"P{player_id} pays {x} energy (now {p.energy})")
    return x


@dataclass(frozen=True)
class ReturnTargetGraveyardCreatureWithMVX(Effect):
    """"Return target creature card with mana value X from your graveyard
    to the battlefield." -- X is the ability's X (``resolving_x_value``,
    the energy paid). While choosing, cards with mana value up to the
    controller's energy are legal; on resolution the card must still be
    there with mana value exactly X."""

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        p = state.player_by_id(controller_id)
        return [
            ("graveyard_card", c.id) for c in p.graveyard
            if c.printing.is_creature
            and c.printing.mana_cost.total <= p.energy
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.actions import put_into_battlefield

        if not targets or targets[0][0] != "graveyard_card":
            return
        p = state.player_by_id(controller_id)
        card = next((c for c in p.graveyard if c.id == targets[0][1]), None)
        if (
            card is None or not card.printing.is_creature
            or card.printing.mana_cost.total != state.resolving_x_value
        ):
            state.log.append("reanimation target is no longer legal")
            return
        p.graveyard.remove(card)
        state.log.append(
            f"P{controller_id} returns {card.printing.name} to the battlefield"
        )
        put_into_battlefield(state, card, controller_id)

    def describe(self) -> str:
        return (
            "return target creature card with mana value X from your "
            "graveyard to the battlefield"
        )


# ---------------------------------------------------------------------------
# Turn structure: an additional combat after a main phase
# ---------------------------------------------------------------------------

# Registry kinds (layer 0, cleanup-expiring -- cloned and hashed with the
# rest of ``state.continuous_effects``): a pending combat after the
# postcombat main phase, and "no main phase follows this combat".
EXTRA_COMBAT_AFTER_MAIN2_KIND = "roto_extra_combat_after_postcombat_main"
NO_MAIN_AFTER_COMBAT_KIND = "roto_no_main_after_this_combat"


@dataclass(frozen=True)
class AdditionalCombatAfterThisMainPhase(Effect):
    """"If it's your main phase, there is an additional combat phase after
    this phase." (Grim Reaper's Sprint). In the precombat main phase the
    engine's pending extra combat (``extra_combat_phases``) follows the
    turn's regular combat, so the turn still has two combats before the
    postcombat main phase. In the postcombat main phase a combat phase is
    scheduled right after it, followed by the end step (no extra main
    phase is printed). Outside your main phases nothing happens."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import Step

        if state.active_player().id != controller_id:
            return
        if state.step == Step.PRECOMBAT_MAIN:
            state.extra_combat_phases += 1
        elif state.step == Step.POSTCOMBAT_MAIN:
            src = state.resolving_spell or state.resolving_source
            state.add_continuous_effect(
                source_id=src.id if src is not None else -1, layer=0,
                kind=EXTRA_COMBAT_AFTER_MAIN2_KIND, expires_at="cleanup",
                description="an additional combat after this main phase",
            )
        else:
            return
        state.log.append("an additional combat phase will follow this phase")

    def describe(self) -> str:
        return (
            "if it's your main phase, there is an additional combat phase "
            "after this phase"
        )


def roto_next_step(state: "GameState", current) -> object:
    """``turn.next_step``'s override for this bucket's turn structure,
    or ``None``: the postcombat main phase goes to an additional combat
    phase, and that combat ends in the end step."""
    from mtgz_ir.turn import Step

    if not state.continuous_effects:
        return None
    if current == Step.POSTCOMBAT_MAIN:
        pending = next(
            (ce for ce in state.continuous_effects
             if ce.kind == EXTRA_COMBAT_AFTER_MAIN2_KIND), None,
        )
        if pending is None:
            return None
        state.continuous_effects = [
            ce for ce in state.continuous_effects if ce is not pending
        ]
        state.add_continuous_effect(
            source_id=pending.source_id, layer=0,
            kind=NO_MAIN_AFTER_COMBAT_KIND, expires_at="cleanup",
            description="this combat is followed by the end step",
        )
        state.log.append("an additional combat phase begins")
        return Step.BEGINNING_OF_COMBAT
    if current == Step.END_OF_COMBAT and state.extra_combat_phases == 0:
        marker = next(
            (ce for ce in state.continuous_effects
             if ce.kind == NO_MAIN_AFTER_COMBAT_KIND), None,
        )
        if marker is None:
            return None
        state.continuous_effects = [
            ce for ce in state.continuous_effects if ce is not marker
        ]
        state._invalidate_layered_cache()
        return Step.END
    return None


from mtgz_ir.effects.conditions import Condition  # noqa: E402


@dataclass(frozen=True)
class CreatureDiedThisTurn(Condition):
    """Cost-reduction condition: a creature died this turn (Morbid;
    ``GameState.creatures_died_this_turn``)."""

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return state.creatures_died_this_turn > 0


# ---------------------------------------------------------------------------
# Auras on lands (Utopia Sprawl)
# ---------------------------------------------------------------------------


def enchantable_land(state: "GameState", aura_printing, obj) -> bool:
    """Can an Aura with ``aura_enchant_type="land"`` enchant ``obj``: a
    land, with the printed land subtype if any ("Enchant Forest")."""
    from mtgz_ir.game import effective_subtypes

    if not obj.printing.is_land:
        return False
    sub = aura_printing.aura_enchant_subtype
    return not sub or sub in effective_subtypes(state, obj)


def choose_color_as_enters(state: "GameState", obj, controller_id: int) -> None:
    """"As this <permanent> enters, choose a color." --
    ``Agent.choose_mana_color`` over WUBRG into ``chosen_color``."""
    colors = ["W", "U", "B", "R", "G"]
    color = colors[0]
    agent = state.player_by_id(controller_id).agent
    if agent is not None:
        picked = agent.choose_mana_color(state, controller_id, colors)
        if picked in colors:
            color = picked
    obj.chosen_color = color
    state.log.append(f"{obj.printing.name} enters; chose {color}")


def enchanted_land_mana_grants(state: "GameState", player_id: int) -> list:
    """``(land id, color)`` grants for ``player_id``'s lands enchanted by
    an "its controller adds an additional one mana of the chosen color"
    Aura (read by automatic payment through
    ``actions.extra_mana_grants_for_tapped_lands``)."""
    out: list = []
    for aura in state.battlefield:
        if (
            not aura.printing.enchanted_land_adds_chosen_color
            or aura.attached_to is None or aura.chosen_color is None
        ):
            continue
        land = next(
            (o for o in state.battlefield if o.id == aura.attached_to), None,
        )
        if land is not None and land.controller_id == player_id:
            out.append((land.id, aura.chosen_color))
    return out


@dataclass(frozen=True)
class CopyImprintedCardMayCastFree(Effect):
    """"You may copy the exiled card. If you do, you may cast the copy
    without paying its mana cost." (Isochron Scepter). The copy of the
    card imprinted on the source (CR 707.12) is created in exile and cast
    during the resolution (``cast_now``, timing ignored); a copy of a
    card that isn't cast -- declined, or no legal targets -- ceases to
    exist, and a cast one ceases to exist when it leaves the stack
    (``GameObject.is_spell_copy``). The two "may"s are one choice: a
    copy that isn't cast does nothing."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.effects.generic.artifact_mechanics import imprinted_cards
        from mtgz_ir.game import Zone

        src = state.resolving_source
        if src is None:
            return
        cards = imprinted_cards(state, src)
        if not cards:
            state.log.append("no card is exiled with it")
            return
        p = state.player_by_id(controller_id)
        copy = state.new_object(cards[0].printing, controller_id, Zone.EXILE)
        copy.is_spell_copy = True
        p.exile.append(copy)
        if can_cast_now(state, controller_id, copy, free=True) and (
            p.agent is None or p.agent.choose_yes_no(
                state, controller_id,
                f"Cast a copy of {copy.printing.name} for free?",
            )
        ):
            state.log.append(
                f"P{controller_id} casts a copy of {copy.printing.name}"
            )
            cast_now(state, controller_id, copy, free=True)
            return
        p.exile.remove(copy)
        copy.exiled_face_down = False  # left exile (CR 400.7)
        state.log.append(f"the copy of {copy.printing.name} ceases to exist")

    def describe(self) -> str:
        return (
            "you may copy the exiled card and cast the copy without paying "
            "its mana cost"
        )


# ---------------------------------------------------------------------------
# Channel: life for colorless mana
# ---------------------------------------------------------------------------

LIFE_FOR_COLORLESS_KIND = "roto_pay_life_add_colorless"


@dataclass(frozen=True)
class GrantLifeForColorlessUntilEndOfTurn(Effect):
    """"Until end of turn, any time you could activate a mana ability,
    you may pay 1 life. If you do, add {C}." (Channel): a
    cleanup-expiring layer-0 registry entry for the controller, used by
    automatic payment (``life_mana_units_needed`` / ``pay_life_for_mana``)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_spell or state.resolving_source
        state.add_continuous_effect(
            source_id=src.id if src is not None else -1, layer=0,
            kind=LIFE_FOR_COLORLESS_KIND, params=(controller_id,),
            expires_at="cleanup",
            description=f"P{controller_id} may pay 1 life for {{C}} this turn",
        )
        state.log.append(f"P{controller_id} may pay 1 life for {{C}} this turn")

    def describe(self) -> str:
        return (
            "until end of turn, any time you could activate a mana ability, "
            "you may pay 1 life to add {C}"
        )


def _life_mana_budget(state: "GameState", player_id: int) -> int:
    """How many {C} the player may make by paying life: 0 without the
    grant, else life above 1 (automatic payment never pays down to 0
    -- the cube-lands life-cost convention)."""
    if not any(
        ce.kind == LIFE_FOR_COLORLESS_KIND and ce.params == (player_id,)
        for ce in state.continuous_effects
    ):
        return 0
    return max(0, state.player_by_id(player_id).life - 1)


def life_mana_units_needed(
    state: "GameState", player_id: int, cost, usable_pool, lands,
) -> "int | None":
    """The fewest life-paid {C} that make ``cost`` payable from
    ``usable_pool`` and ``lands``; ``None`` when no amount the player can
    pay does (or the grant isn't active)."""
    from mtgz_ir.actions import _can_pay_from
    from mtgz_ir.game import ManaCoin

    budget = _life_mana_budget(state, player_id)
    if budget <= 0:
        return None
    need = cost.generic + len(cost.hybrid)
    from mtgz_ir.actions import mana_spendable_as_any_color

    if mana_spendable_as_any_color(state):
        # The {C} may pay colored pips too (Mycosynth Lattice).
        need += sum(n for c, n in cost.colored if c != "S")
    for k in range(1, min(budget, need) + 1):
        pool = list(usable_pool) + [ManaCoin(color="C")] * k
        if _can_pay_from(state, cost, pool, lands):
            return k
    return None


def pay_life_for_mana(
    state: "GameState", player_id: int, cost, spell_kind,
) -> None:
    """During automatic payment: pay life for the fewest {C} that make
    ``cost`` payable (nothing if the pool and lands already cover it)."""
    from mtgz_ir.actions import (
        _can_pay_from,
        _coin_satisfies_context,
        _untapped_lands,
    )
    from mtgz_ir.game import ManaCoin

    if _life_mana_budget(state, player_id) <= 0:
        return
    p = state.player_by_id(player_id)
    usable = [c for c in p.mana_pool if _coin_satisfies_context(c, spell_kind)]
    lands = _untapped_lands(state, player_id)
    if _can_pay_from(state, cost, usable, lands):
        return
    k = life_mana_units_needed(state, player_id, cost, usable, lands)
    if k is None:
        return
    p.life -= k
    p.mana_pool.extend(ManaCoin(color="C") for _ in range(k))
    state.log.append(
        f"P{player_id} pays {k} life for {{C}}x{k} (life: {p.life})"
    )


# ---------------------------------------------------------------------------
# "discard X cards" additional cost; "X damage to each of X targets"
# ---------------------------------------------------------------------------


def _any_target_filter():
    from mtgz_ir.effects.object_predicates import (
        IsBattle, IsCreature, IsPlaneswalker, IsPlayer,
    )
    from mtgz_ir.effects.target_filter import OneOf

    return OneOf(IsCreature(), IsPlaneswalker(), IsBattle(), IsPlayer())


@dataclass(frozen=True)
class DealXDamageToEachOfXTargets(Effect):
    """"<This> deals X damage to each of X targets." (Firestorm). The
    target count is the X announced while casting (``cast_time_shape``);
    each target still legal on resolution is dealt X damage."""

    count: int = -1  # set by ``cast_time_shape``; -1 before X is known

    def requires_target(self) -> bool:
        return self.count > 0

    def target_count_range(self) -> tuple[int, int]:
        n = max(self.count, 0)
        return (n, n)

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return self._enumerate_for_filter(
            state, controller_id, _any_target_filter(),
        )

    def cast_time_shape(self, *, kicked=False, alt_cost_paid=False, x_value=None):
        from dataclasses import replace

        if x_value is None:
            return self
        return replace(self, count=max(0, x_value))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.base import _apply_damage

        x = max(0, state.resolving_x_value)
        legal = set(self.legal_targets(state, controller_id))
        for t in targets:
            if t in legal and x > 0:
                _apply_damage(state, t, x)

    def describe(self) -> str:
        return "deals X damage to each of X targets"


def choose_additional_cost_x(
    state: "GameState", player_id: int, card,
) -> "int | None":
    """X for "As an additional cost to cast this spell, discard X cards."
    (``None`` for other spells), announced before targets (CR 601.2b):
    at most the other cards in hand and, for "each of X targets", the
    number of legal targets."""
    if not card.printing.additional_cost_discard_x:
        return None
    p = state.player_by_id(player_id)
    upper = len([c for c in p.hand if c is not card])
    eff = card.printing.spell_effect
    if isinstance(eff, DealXDamageToEachOfXTargets):
        upper = min(upper, len(eff.legal_targets(state, player_id)))
    x = upper
    if p.agent is not None:
        x = p.agent.choose_x(state, player_id, card.printing.name, upper)
    x = max(0, min(upper, x))
    state.log.append(f"P{player_id} chooses X={x} for {card.printing.name}")
    return x


def pay_discard_x(state: "GameState", player_id: int, card, x: int) -> None:
    """Discard ``x`` cards (``choose_discard`` each) as the cost."""
    from mtgz_ir.game import Zone
    from mtgz_ir.turn import fire_opponent_discards_triggers

    p = state.player_by_id(player_id)
    for _ in range(x):
        options = [c for c in p.hand if c is not card]
        if not options:  # pragma: no cover - X is bounded by the hand
            return
        chosen = options[0]
        if p.agent is not None:
            pick = p.agent.choose_discard(state, player_id, [c.id for c in options])
            chosen = next((c for c in options if c.id == pick), options[0])
        p.hand.remove(chosen)
        chosen.zone = Zone.GRAVEYARD
        p.graveyard.append(chosen)
        state.log.append(
            f"additional cost: P{player_id} discards {chosen.printing.name}"
        )
        fire_opponent_discards_triggers(state, player_id, card=chosen)


# ---------------------------------------------------------------------------
# Rottenmouth Viper: sacrifice any number for a cost reduction; blight
# ---------------------------------------------------------------------------

import contextlib  # noqa: E402
from contextvars import ContextVar  # noqa: E402

from mtgz_ir.effects.conditions import Condition as _Condition  # noqa: E402

# How many permanents the spell being cast had sacrificed for its "costs
# {1} less for each permanent sacrificed this way" (None outside a cast:
# affordability checks then count every permanent that could be).
_SACRIFICED_FOR_COST: ContextVar = ContextVar(
    "roto_sacrificed_for_cost", default=None,
)


@dataclass(frozen=True)
class PerPermanentSacrificedForCost(_Condition):
    """Cost-reduction condition "for each permanent sacrificed this way":
    the number the cast sacrificed, or -- in ``legal_actions``'
    affordability probe -- every permanent the caster could sacrifice
    except its sac-for-mana sources (a Treasure the affordability check
    already counts as a mana: sacrificing it here instead saves at most
    the one generic it would have paid, so leaving it out is exact)."""

    permanent_filter: object = None

    def matches(self, state, viewer_id, chosen_targets=None) -> bool:
        return True

    def multiplier(self, state, viewer_id, chosen_targets=None) -> int:
        from mtgz_ir.actions import _sac_for_mana_sources

        n = _SACRIFICED_FOR_COST.get()
        if n is not None:
            return n
        mana = {src.id for src, _, _ in _sac_for_mana_sources(state, viewer_id)}
        return sum(
            1 for o in _sacrifice_candidates(state, viewer_id, self.permanent_filter)
            if o.id not in mana
        )


def _sacrifice_candidates(state: "GameState", player_id: int, pred) -> list:
    prior = state.resolving_controller_id
    try:
        state.resolving_controller_id = player_id
        return [
            o for o in state.battlefield
            if o.controller_id == player_id and pred.test(o, state)
        ]
    finally:
        state.resolving_controller_id = prior


@contextlib.contextmanager
def optional_sacrifice_for_cost(
    state: "GameState", player_id: int, card, base_cost, targets=None,
):
    """Wrap the cast's cost determination (``actions._cast_spell``):
    choose how many permanents to sacrifice (``choose_x``, at least as
    many as the cost needs to stay payable), sacrifice them one at a time
    (``choose_sacrifice``, never one whose loss would make the reduced
    cost unpayable -- a Treasure the payment needs), and expose the count
    to ``PerPermanentSacrificedForCost`` for the block."""
    pred = card.printing.optional_sacrifice_any_number
    if pred is None:
        yield
        return
    from mtgz_ir.actions import apply_cost_reductions, can_pay_cost
    from mtgz_ir.turn import sacrifice

    def _payable(n: int) -> bool:
        # A probe: the reduction's log line belongs to the real cast.
        token = _SACRIFICED_FOR_COST.set(n)
        logging, state.log.enabled = state.log.enabled, False
        try:
            cost = apply_cost_reductions(
                card.printing, state, player_id, base_cost,
                chosen_targets=targets, card=card,
            )
        finally:
            state.log.enabled = logging
            _SACRIFICED_FOR_COST.reset(token)
        return can_pay_cost(state, player_id, cost, sac_for_mana=True)

    candidates = _sacrifice_candidates(state, player_id, pred)
    min_n = next(
        (n for n in range(len(candidates) + 1) if _payable(n)), 0,
    )
    p = state.player_by_id(player_id)
    n = min_n
    if p.agent is not None and candidates:
        n = p.agent.choose_x(
            state, player_id, f"{card.printing.name}: permanents to sacrifice",
            len(candidates),
        )
    n = max(min_n, min(n, len(candidates)))
    paid = 0
    for _ in range(n):
        options = []
        for o in _sacrifice_candidates(state, player_id, pred):
            idx = state.battlefield.index(o)
            state.battlefield.remove(o)
            try:
                if _payable(n):
                    options.append(o)
            finally:
                state.battlefield.insert(idx, o)
        if not options:
            break
        chosen = options[0]
        if p.agent is not None:
            pick = p.agent.choose_sacrifice(
                state, player_id, [o.id for o in options],
            )
            chosen = next((o for o in options if o.id == pick), options[0])
        state.log.append(
            f"additional cost: P{player_id} sacrifices {chosen.printing.name}"
        )
        sacrifice(state, chosen)
        paid += 1
    token = _SACRIFICED_FOR_COST.set(paid)
    try:
        yield
    finally:
        _SACRIFICED_FOR_COST.reset(token)


@dataclass(frozen=True)
class CounterOnSourceThenPunishPerCounter(Effect):
    """"Put a <kind> counter on it. Then for each <kind> counter on it,
    each opponent loses N life unless that player sacrifices a nonland
    permanent of their choice or discards a card." (Rottenmouth Viper).
    Per counter, each opponent chooses (``choose_mode``) among the options
    they can perform; the sacrificed permanent / discarded card is their
    choice (``choose_sacrifice`` / ``choose_discard``)."""

    kind: str = "blight"
    life: int = 4

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone, add_counters, counter_count
        from mtgz_ir.player_protections import opponent_effect_blocked_for
        from mtgz_ir.turn import fire_opponent_discards_triggers, sacrifice

        src = state.resolving_source
        if src is None:
            return
        if src in state.battlefield:
            add_counters(state, src, self.kind, 1)
        n = counter_count(src, self.kind)
        for _ in range(n):
            for opp in state.players:
                if opp.id == controller_id or opp.has_lost:
                    continue
                sacs = [
                    o for o in state.battlefield
                    if o.controller_id == opp.id and not o.printing.is_land
                ]
                modes, tags = [], []
                if sacs:
                    modes.append("sacrifice a nonland permanent")
                    tags.append("sac")
                if opp.hand:
                    modes.append("discard a card")
                    tags.append("discard")
                modes.append(f"lose {self.life} life")
                tags.append("life")
                pick = 0 if opp.agent is None else opp.agent.choose_mode(
                    state, opp.id, modes,
                )
                tag = tags[max(0, min(pick, len(tags) - 1))]
                if tag == "sac":
                    if opponent_effect_blocked_for(state, opp.id):
                        continue
                    chosen = sacs[0]
                    if opp.agent is not None:
                        sid = opp.agent.choose_sacrifice(
                            state, opp.id, [o.id for o in sacs],
                        )
                        chosen = next((o for o in sacs if o.id == sid), sacs[0])
                    state.log.append(f"P{opp.id} sacrifices {chosen.printing.name}")
                    sacrifice(state, chosen)
                elif tag == "discard":
                    if opponent_effect_blocked_for(state, opp.id):
                        continue
                    ids = [c.id for c in opp.hand]
                    did = ids[0] if opp.agent is None else opp.agent.choose_discard(
                        state, opp.id, ids,
                    )
                    card = next((c for c in opp.hand if c.id == did), opp.hand[0])
                    opp.hand.remove(card)
                    card.zone = Zone.GRAVEYARD
                    opp.graveyard.append(card)
                    state.log.append(f"P{opp.id} discards {card.printing.name}")
                    fire_opponent_discards_triggers(state, opp.id, card=card)
                else:
                    opp.life -= self.life
                    state.log.append(
                        f"P{opp.id} loses {self.life} life (life: {opp.life})"
                    )

    def describe(self) -> str:
        return (
            f"put a {self.kind} counter on it; for each, each opponent loses "
            f"{self.life} life unless they sacrifice a nonland permanent or "
            "discard a card"
        )


# ---------------------------------------------------------------------------
# Predicates
# ---------------------------------------------------------------------------


from mtgz_ir.effects.predicates import TriggerPredicate  # noqa: E402


@dataclass(frozen=True)
class HasNotAddedManaWithThisAbilityThisTurn(TriggerPredicate):
    """Trigger gate: "if you haven't added mana with this ability this
    turn" (Carpet of Flowers; ``GameObject.ability_mana_added_turn``)."""

    def matches(self, state, subjects, source) -> bool:
        return source.ability_mana_added_turn != state.turn_number


# ---------------------------------------------------------------------------
# "if {C}{C} was spent to cast it" (Wistfulness)
# ---------------------------------------------------------------------------


def _colors_assignable(sources: tuple, demands) -> bool:
    """Can distinct spent units be assigned colors so each ``(color,
    n)`` demand gets ``n`` units of that color? (bipartite matching of
    demand slots to units)."""
    slots = [c for c, n in demands for _ in range(n)]
    match: dict[int, int] = {}

    def _augment(slot: int, seen: set) -> bool:
        for unit, colors in enumerate(sources):
            if slots[slot] not in colors or unit in seen:
                continue
            seen.add(unit)
            if unit not in match or _augment(match[unit], seen):
                match[unit] = slot
                return True
        return False

    return all(_augment(i, set()) for i in range(len(slots)))


def _mana_spent_demands(printing) -> tuple:
    """The ``(color, n)`` of every "if {C}{C} was spent" condition on
    ``printing``'s triggered abilities."""
    out: list = []

    def _walk(pred) -> None:
        if isinstance(pred, ManaSpentToCastSource):
            out.append((pred.color, pred.count))
        for attr in ("left", "right", "inner"):
            child = getattr(pred, attr, None)
            if child is not None:
                _walk(child)

    for trig in printing.triggered_abilities:
        _walk(trig.predicate)
    return tuple(dict.fromkeys(out))


def spent_mana_choice(state: "GameState", obj) -> frozenset:
    """The "{C}{C} was spent" conditions ``obj``'s controller settled the
    spent mana's colors to satisfy: the most conditions that hold
    together (each flexible unit -- a dual land, an "any" coin -- is one
    color of its controller's choice); between several such sets the
    controller picks with ``choose_mode``. Settled once per cast and
    cached on the object."""
    from itertools import combinations

    if obj.spent_color_choice is not None:
        return obj.spent_color_choice
    demands = _mana_spent_demands(obj.printing)
    sources = obj.mana_spent_color_sources
    best: list = [frozenset()]
    for size in range(len(demands), 0, -1):
        feasible = [
            frozenset(combo) for combo in combinations(demands, size)
            if _colors_assignable(sources, combo)
        ]
        if feasible:
            best = feasible
            break
    choice = best[0]
    agent = state.player_by_id(obj.controller_id).agent
    if len(best) > 1 and agent is not None:
        labels = [
            " and ".join("{" + c + "}" * n for c, n in sorted(s)) for s in best
        ]
        idx = agent.choose_mode(state, obj.controller_id, labels)
        if isinstance(idx, int) and 0 <= idx < len(best):
            choice = best[idx]
    obj.spent_color_choice = choice
    return choice


@dataclass(frozen=True)
class ManaSpentToCastSource(TriggerPredicate):
    """Intervening-if "if {C}...{C} (``count`` of ``color``) was spent to
    cast it" on the source's enters trigger: the source entered by
    resolving as the spell that was cast, and the settled color
    assignment (``spent_mana_choice``) covers this condition."""

    color: str = "G"
    count: int = 2

    def matches(self, state, subjects, source) -> bool:
        if getattr(source, "entered_from_zone", "") != "stack":
            return False
        return (self.color, self.count) in spent_mana_choice(state, source)


@dataclass(frozen=True)
class EnteredThisTurn(ObjectPredicate):
    """A permanent that entered the battlefield this turn ("creatures
    that entered this turn", Force of Despair). Reads the
    ``GameObject.entered_battlefield_turn`` stamp
    ``actions.put_into_battlefield`` writes for every permanent."""

    def test(self, obj, state) -> bool:
        turn = getattr(obj, "entered_battlefield_turn", None)
        return turn is not None and turn == state.turn_number


# ---------------------------------------------------------------------------
# Spend restrictions (``ManaCoin.spend_restriction`` tags of this bucket)
# ---------------------------------------------------------------------------

# Payment-context tags (``actions._coin_satisfies_context``) that mark a
# payment as something other than casting a spell: activating an
# ability (``legal`` / ``actions._activate_ability`` add
# "activated_ability") or turning a face-down creature up.
_NON_SPELL_CONTEXTS = frozenset({"activated_ability"})


def roto_coin_satisfies(coin, kinds: "frozenset[str]") -> bool:
    """Whether a coin carrying one of this bucket's restriction tags may
    pay in the payment context ``kinds``.

    - ``"not_nonartifact_spell"``: "This mana can't be spent to cast
      nonartifact spells." (The Mightstone and Weakstone) -- it pays for
      artifact spells and for anything that isn't casting a spell.
    - ``"creature_type:<T>"``: "Spend this mana only to cast a creature
      spell of the chosen type" (Cavern of Souls) -- matched against
      the ``creature_type:<T>`` tags ``roto_spell_kinds`` adds for a
      creature spell (``creature_type:*`` for a changeling).
    """
    tag = coin.spend_restriction or ""
    if tag == "colored_only":
        # "This mana can't be spent to pay generic mana costs." --
        # spendable in any context; the generic-payment loops skip it.
        return True
    if tag == "not_nonartifact_spell":
        return "artifact_spell" in kinds or bool(kinds & _NON_SPELL_CONTEXTS)
    if tag.startswith("creature_type:"):
        return tag in kinds or (
            tag != "creature_type:" and "creature_type:*" in kinds
        )
    return False


def roto_spell_kinds(card) -> "frozenset[str]":
    """Payment-context tags this bucket adds when ``card`` is cast (the
    casting contexts in ``legal._spell_kinds_for_card`` and
    ``actions._cast_spell``): one ``creature_type:<T>`` per creature type
    of a creature spell, ``creature_type:*`` for a changeling (every
    creature type, CR 702.73a)."""
    pr = card.printing
    if not pr.is_creature:
        return frozenset()
    if "changeling" in pr.keywords:
        return frozenset({"creature_type:*"})
    return frozenset(f"creature_type:{s}" for s in pr.subtypes)


# ---------------------------------------------------------------------------
# As-enters choices
# ---------------------------------------------------------------------------


def creature_types_owned_by(state: "GameState", player_id: int) -> list[str]:
    """Creature types among creature cards ``player_id`` owns in any zone
    (library, hand, graveyard, exile, battlefield, stack), sorted -- the
    offered set of "choose a creature type" (a type no card of yours has
    can't matter to an own-spell restriction)."""
    p = state.player_by_id(player_id)
    zones = list(p.library) + list(p.hand) + list(p.graveyard) + list(p.exile)
    zones += [o for o in state.battlefield if o.owner_id == player_id]
    zones += [
        item.source for item in state.stack
        if getattr(item, "source", None) is not None
        and item.source.owner_id == player_id
    ]
    types: set[str] = set()
    for c in zones:
        if c.printing.is_creature and not c.printing.is_token:
            types.update(c.printing.subtypes)
    return sorted(types)


def apply_roto_land_as_enters(
    state: "GameState", player_id: int, card,
) -> None:
    """This bucket's "As this land enters, ..." choices (called from
    ``actions.apply_land_etb_replacements``)."""
    if card.printing.enters_choosing_creature_type:
        choose_creature_type_as_enters(state, player_id, card)


def choose_creature_type_as_enters(
    state: "GameState", player_id: int, card,
) -> None:
    """"As this land enters, choose a creature type." (Cavern of Souls):
    ``choose_mode`` over ``creature_types_owned_by``; with no creature
    card owned the choice is moot and recorded as ``None``."""
    options = creature_types_owned_by(state, player_id)
    card.chosen_creature_type = None
    if not options:
        state.log.append(f"{card.printing.name}: no creature type to choose")
        return
    idx = 0
    agent = state.player_by_id(player_id).agent
    if agent is not None and len(options) > 1:
        picked = agent.choose_mode(state, player_id, options)
        if isinstance(picked, int) and 0 <= picked < len(options):
            idx = picked
    card.chosen_creature_type = options[idx]
    state.log.append(
        f"{card.printing.name}: P{player_id} chooses {options[idx]}"
    )


# ---------------------------------------------------------------------------
# Mana effects
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class AddManaPerCardInTargetOpponentsHand(Effect):
    """"Add {C} for each card in target opponent's hand." (Jeska's Will).
    A spell mode, not a mana ability: it targets and uses the stack. The
    hand is counted as the spell resolves."""

    color: str = "R"

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return [
            ("player", p.id) for p in state.players
            if p.id != controller_id and not p.has_lost
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        if not targets or targets[0][0] != "player":
            return
        opp = next(
            (p for p in state.players if p.id == targets[0][1]), None,
        )
        if opp is None or opp.id == controller_id:
            return
        n = len(opp.hand)
        pool = state.player_by_id(controller_id).mana_pool
        pool.extend(ManaCoin(color=self.color) for _ in range(n))
        state.log.append(
            f"P{controller_id} adds {{{self.color}}}x{n} (cards in "
            f"P{opp.id}'s hand)"
        )

    def describe(self) -> str:
        return f"add {{{self.color}}} for each card in target opponent's hand"


@dataclass(frozen=True)
class AddAnyColorManaForChosenCreatureType(AddManaToPool):
    """"Add one mana of any color. Spend this mana only to cast a creature
    spell of the chosen type, and that spell can't be countered."
    (Cavern of Souls). An ``AddManaToPool`` (so it is a mana ability and
    is never folded into automatic payment) whose coin's restriction is
    read from the source's ``chosen_creature_type`` as it resolves."""

    produces: tuple = (("any", 1),)
    makes_spell_uncounterable: bool = True

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        src = state.resolving_source
        chosen = getattr(src, "chosen_creature_type", None) or ""
        state.player_by_id(controller_id).mana_pool.append(
            ManaCoin(
                color="any",
                spend_restriction=f"creature_type:{chosen}",
                makes_spell_uncounterable=True,
            )
        )
        state.log.append(
            f"P{controller_id} adds {{any}} (only for a {chosen or 'no'} "
            f"creature spell; it can't be countered)"
        )

    def describe(self) -> str:
        return (
            "add one mana of any color (only to cast a creature spell of "
            "the chosen type; that spell can't be countered)"
        )


# ---------------------------------------------------------------------------
# Cast permissions
# ---------------------------------------------------------------------------


def _source_power(state: "GameState") -> int:
    """The resolving source's power (last known information once it has
    left the battlefield: the object keeps its counters and the
    registry entries keyed to it until cleanup)."""
    from mtgz_ir.game import effective_power

    src = state.resolving_source
    return effective_power(state, src) if src is not None else 0


@dataclass(frozen=True)
class MayCastTargetGraveyardCardFree(Effect):
    """"You may cast target <filter> card [with mana value less than or
    equal to this creature's power] from your graveyard without paying
    its mana cost. If that spell would be put into your graveyard, exile
    it instead." (Dreadhorde Arcanist). The target is a card in the
    controller's graveyard, re-checked (filter, mana value bound) on
    resolution; the spell is cast during the resolution (CR 608.2g,
    timing ignored) through ``creature_casting_ub.cast_now``."""

    card_filter: object = None
    mv_at_most_source_power: bool = False

    def _ok(self, state: "GameState", card) -> bool:
        if not self.card_filter.matches_card(state, card):
            return False
        if self.mv_at_most_source_power:
            return card.printing.mana_cost.total <= _source_power(state)
        return True

    def requires_target(self) -> bool:
        return True

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        return [
            ("graveyard_card", c.id)
            for c in state.player_by_id(controller_id).graveyard
            if self._ok(state, c)
        ]

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now

        if not targets or targets[0][0] != "graveyard_card":
            return
        p = state.player_by_id(controller_id)
        card = next((c for c in p.graveyard if c.id == targets[0][1]), None)
        if card is None or not self._ok(state, card):
            state.log.append("graveyard cast target is no longer legal")
            return
        if not can_cast_now(state, controller_id, card, free=True):
            return
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id, f"Cast {card.printing.name} for free?",
        ):
            return
        from mtgz_ir.game import _CowList

        if isinstance(p.graveyard, _CowList):
            # The flags below mutate a graveyard card in place: isolate
            # this state's graveyard from search clones first.
            p.graveyard.fork()
        card.exile_if_put_into_graveyard_from_stack = True
        cast_now(state, controller_id, card, free=True)

    def describe(self) -> str:
        bound = (
            " with mana value up to this creature's power"
            if self.mv_at_most_source_power else ""
        )
        return (
            f"you may cast target {self.card_filter.describe()}{bound} from "
            "your graveyard free; exile it instead of putting it into your "
            "graveyard"
        )


def _top_card_kind_matches(state: "GameState", card, kinds) -> bool:
    from mtgz_ir.game import effective_colors, is_artifact_now

    if card.printing.is_land:
        return False
    if "artifact" in kinds and is_artifact_now(state, card):
        return True
    return "colorless" in kinds and not effective_colors(state, card)


def top_of_library_cast_allowed(
    state: "GameState", player_id: int, card,
) -> bool:
    """"You may cast <kind> spells from the top of your library." (Mystic
    Forge's ``cast_from_top_of_library_kinds``): ``card`` is the top of
    ``player_id``'s library and a permanent they control grants its
    kind."""
    p = state.player_by_id(player_id)
    if not p.library or p.library[0] is not card:
        return False
    return any(
        o.controller_id == player_id
        and o.printing.cast_from_top_of_library_kinds
        and _top_card_kind_matches(
            state, card, o.printing.cast_from_top_of_library_kinds,
        )
        for o in state.battlefield
    )


def _castable_now(state: "GameState", player_id: int, card, *, free: bool) -> bool:
    """Targets, additional costs and (unless free) the mana cost of
    casting ``card`` from outside the hand are all satisfiable."""
    from mtgz_ir.actions import apply_cost_reductions
    from mtgz_ir.creature_casting_ub import casting_origin
    from mtgz_ir.game import Zone
    from mtgz_ir.legal import (
        _additional_costs_payable, _can_cast, _targets_are_satisfiable,
    )

    if not (
        _targets_are_satisfiable(state, player_id, card)
        and _additional_costs_payable(state, player_id, card)
    ):
        return False
    if free:
        return True
    with casting_origin(
        card.zone != Zone.HAND, from_graveyard=card.zone == Zone.GRAVEYARD,
    ):
        cost = apply_cost_reductions(
            card.printing, state, player_id, card.printing.mana_cost,
            card=card,
        )
    return _can_cast(state, player_id, card, cost)


def _instant_speed(card) -> bool:
    return card.printing.is_instant or "flash" in card.printing.keywords


def _omniscience(state: "GameState", player_id: int) -> bool:
    return any(
        o.controller_id == player_id and o.printing.grants_free_casts_from_hand
        for o in state.battlefield
    )


def _aluren_max_mv(state: "GameState") -> "int | None":
    """The largest mana value an Aluren on the battlefield (any
    controller: "any player may") lets creature spells be cast for free
    with flash, or ``None``."""
    caps = [
        o.printing.free_flash_creature_casts_max_mv for o in state.battlefield
        if o.printing.free_flash_creature_casts_max_mv is not None
    ]
    return max(caps) if caps else None


def _aluren_allows(state: "GameState", card) -> bool:
    cap = _aluren_max_mv(state)
    return (
        cap is not None and card.printing.is_creature
        and card.printing.mana_cost.total <= cap
    )


def static_free_cast_permission(
    state: "GameState", player_id: int, card,
) -> bool:
    """May ``player_id`` cast ``card`` (from their hand) without paying
    its mana cost under a static permission: Omniscience ("spells from
    your hand") or Aluren (creature spells with mana value N or less, any
    player). Consulted by ``alt_casting.free_cast_permission``."""
    from mtgz_ir.game import Zone

    if card.zone != Zone.HAND or card.printing.is_land:
        return False
    return _omniscience(state, player_id) or _aluren_allows(state, card)


def _free_cast_tax_payable(state: "GameState", player_id: int, card) -> bool:
    """Cost increases (Thalia) still apply to free casts (CR 601.2f)."""
    from mtgz_ir.actions import can_pay_cost
    from mtgz_ir.cards.base import ManaCost
    from mtgz_ir.creature_statics_wg import spell_cost_increase
    from mtgz_ir.legal import _spell_kinds_for_card

    tax = spell_cost_increase(state, card.printing, player_id, card=card)
    return tax == 0 or can_pay_cost(
        state, player_id, ManaCost(generic=tax),
        spell_kind=_spell_kinds_for_card(state, card), sac_for_mana=True,
    )


def roto_legal_actions(
    state: "GameState", player_id: int, existing: list, *,
    sorcery_timing: bool,
) -> list:
    """This bucket's extra legal actions (the single
    ``legal._legal_actions_core`` hook): permission casts and suspend
    (``permission_cast_actions``) plus activations of abilities that
    work from exile (``exile_ability_actions``)."""
    return permission_cast_actions(
        state, player_id, existing, sorcery_timing=sorcery_timing,
    ) + exile_ability_actions(state, player_id)


def exile_ability_actions(state: "GameState", player_id: int) -> list:
    """``ActivateAbility`` for each payable exile-zone ability of a card
    ``player_id`` owns in exile (Greater Gargadon: only while suspended).
    No timing restriction is printed, so they're offered at any
    priority."""
    from mtgz_ir.actions import ActivateAbility, can_pay_cost
    from mtgz_ir.effects.generic.cube_creatures_rmc import extra_costs_payable

    out: list = []
    for card in state.player_by_id(player_id).exile:
        for idx, ab in enumerate(card.printing.exile_activated_abilities):
            if ab.activate_only_if_suspended and not is_suspended(card):
                continue
            if ab.cost.mana is not None and not can_pay_cost(
                state, player_id, ab.cost.mana,
            ):
                continue
            if not extra_costs_payable(state, player_id, card, ab.cost):
                continue
            out.append(ActivateAbility(source_id=card.id, ability_index=idx))
    return out


def activate_exile_ability(
    state: "GameState", player_id: int, card_id: int, ability_index: int,
) -> None:
    """Activate ``exile_activated_abilities[ability_index]`` of a card in
    ``player_id``'s exile: pay its costs and put it on the stack with the
    card as its source (the graveyard-ability path's shape)."""
    from mtgz_ir.actions import auto_tap_for_cost
    from mtgz_ir.effects.generic.cube_creatures_rmc import pay_extra_costs
    from mtgz_ir.game import StackItem

    card = next(
        (c for c in state.player_by_id(player_id).exile if c.id == card_id),
        None,
    )
    if card is None:
        return
    abilities = card.printing.exile_activated_abilities
    if not 0 <= ability_index < len(abilities):
        return
    ability = abilities[ability_index].ability
    if ability.cost.mana is not None:
        auto_tap_for_cost(state, player_id, ability.cost.mana)
    paid = pay_extra_costs(state, player_id, card, ability.cost)
    state.stack.append(StackItem(
        source=card, controller_id=player_id, ability=ability,
        targets=(), cost_paid_objects=tuple(paid),
    ))
    state.log.append(
        f"P{player_id} activates {card.printing.name}'s ability from exile"
    )


@dataclass(frozen=True)
class ExileZoneAbility:
    """An activated ability that functions while its card is in exile.
    ``activate_only_if_suspended``: "Activate only if this card is
    suspended." (Greater Gargadon)."""

    ability: object
    activate_only_if_suspended: bool = False

    @property
    def cost(self):
        return self.ability.cost


def permission_cast_actions(
    state: "GameState", player_id: int, existing: list, *,
    sorcery_timing: bool,
) -> list:
    """``CastSpell`` actions granted by this bucket's cast permissions
    (the single ``legal._legal_actions_core`` hook, placed before the
    instant-speed early return so every later restriction filter sees
    them). At sorcery timing every permitted cast is offered; otherwise
    only instants and flash cards. Actions already in ``existing`` are
    not repeated."""
    from mtgz_ir.actions import CastSpell

    out: list = []
    seen = set(existing)

    def _emit(action) -> None:
        if action not in seen:
            seen.add(action)
            out.append(action)

    p = state.player_by_id(player_id)
    # Omniscience / Aluren: free casts from hand (Aluren's "as though
    # they had flash" lets its creatures in at any priority).
    seen_free: set[str] = set()
    omniscience = _omniscience(state, player_id)
    aluren = _aluren_max_mv(state) is not None
    for card in (p.hand if (omniscience or aluren) else ()):
        if card.printing.is_land or card.printing.name in seen_free:
            continue
        timing_ok = sorcery_timing or _instant_speed(card) or (
            _aluren_allows(state, card)
        )
        if not (
            timing_ok
            and static_free_cast_permission(state, player_id, card)
            and _castable_now(state, player_id, card, free=True)
            and _free_cast_tax_payable(state, player_id, card)
        ):
            continue
        seen_free.add(card.printing.name)
        _emit(CastSpell(card.id, cast_for_free=True))
    # Suspend (CR 702.62a): the special action from hand.
    seen_suspend: set[str] = set()
    for card in p.hand:
        if card.printing.name in seen_suspend:
            continue
        if suspend_castable(
            state, player_id, card, sorcery_timing=sorcery_timing,
        ):
            seen_suspend.add(card.printing.name)
            _emit(CastSpell(card.id, use_suspend=True))
    # Mystic Forge: the top card of the library.
    if p.library:
        top = p.library[0]
        if (
            (sorcery_timing or _instant_speed(top))
            and top_of_library_cast_allowed(state, player_id, top)
            and _castable_now(state, player_id, top, free=False)
        ):
            _emit(CastSpell(top.id))
    return out


# ---------------------------------------------------------------------------
# Library effects
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ExileTopCardsOfYourLibrary(Effect):
    """"Exile the top [N] card[s] of your library." (Mystic Forge's
    ``{T}, Pay 1 life`` ability) -- face up, no play permission."""

    count: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import Zone

        p = state.player_by_id(controller_id)
        for _ in range(min(self.count, len(p.library))):
            card = p.library.pop(0)
            card.zone = Zone.EXILE
            p.exile.append(card)
            state.log.append(
                f"P{controller_id} exiles {card.printing.name} from the top "
                "of their library"
            )

    def describe(self) -> str:
        return f"exile the top {self.count} card(s) of your library"


@dataclass(frozen=True)
class ExileHandDrawThatManyMayPlayThem(Effect):
    """"Exile all the cards from your hand, then draw that many cards.
    Until the end of your next turn, you may play cards exiled this way."
    (Hex Magic): each exiled card carries an exile play permission
    (``UNTIL_END_OF_YOUR_NEXT_TURN``, normal costs, lands playable with a
    land drop)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            UNTIL_END_OF_YOUR_NEXT_TURN,
            grant_exile_play,
        )
        from mtgz_ir.game import Zone
        from mtgz_ir.turn import draw_card

        p = state.player_by_id(controller_id)
        exiled = list(p.hand)
        for card in exiled:
            p.hand.remove(card)
            card.zone = Zone.EXILE
            p.exile.append(card)
            grant_exile_play(
                state, card, controller_id,
                duration=UNTIL_END_OF_YOUR_NEXT_TURN,
            )
        state.log.append(
            f"P{controller_id} exiles their hand ({len(exiled)} cards); may "
            "play them until the end of their next turn"
        )
        for _ in range(len(exiled)):
            draw_card(state, controller_id)

    def describe(self) -> str:
        return (
            "exile your hand, then draw that many; until the end of your "
            "next turn, you may play the exiled cards"
        )


# ---------------------------------------------------------------------------
# Suspend (CR 702.62)
# ---------------------------------------------------------------------------


def is_suspended(card) -> bool:
    """CR 702.62b: a card is suspended if it's in exile, has suspend and
    has a time counter on it."""
    from mtgz_ir.game import Zone

    return (
        card.zone == Zone.EXILE
        and card.printing.suspend_count > 0
        and card.time_counters > 0
    )


def suspend_castable(state: "GameState", player_id: int, card, *,
                     sorcery_timing: bool) -> bool:
    """Could ``player_id`` suspend ``card`` from their hand now: "If you
    could begin to cast this card by putting it onto the stack from your
    hand" (its normal timing permission) and the suspend cost is
    payable."""
    pr = card.printing
    if pr.suspend_count <= 0 or pr.suspend_cost is None:
        return False
    if not (sorcery_timing or _instant_speed(card)):
        return False
    from mtgz_ir.actions import can_pay_cost

    return can_pay_cost(state, player_id, pr.suspend_cost, sac_for_mana=True)


def suspend_card(state: "GameState", player_id: int, card_id: int) -> None:
    """The suspend special action (CR 702.62a, CR 116.2f): pay the suspend
    cost and exile the card from hand face up with N time counters. It
    doesn't use the stack and isn't a cast."""
    from mtgz_ir.actions import (
        _activate_sac_for_mana_for_cost,
        auto_tap_for_cost,
    )
    from mtgz_ir.game import Zone, reveal_to_all

    p = state.player_by_id(player_id)
    card = next((c for c in p.hand if c.id == card_id), None)
    if card is None or card.printing.suspend_count <= 0:  # pragma: no cover - legal_actions gates
        raise ValueError(f"card {card_id} can't be suspended by P{player_id}")
    cost = _activate_sac_for_mana_for_cost(
        state, player_id, card.printing.suspend_cost, frozenset({"spell"}),
    )
    auto_tap_for_cost(state, player_id, cost, spell_kind=frozenset({"spell"}))
    p.hand.remove(card)
    card.zone = Zone.EXILE
    card.time_counters = card.printing.suspend_count
    p.exile.append(card)
    reveal_to_all(state, card)
    state.log.append(
        f"P{player_id} suspends {card.printing.name} with "
        f"{card.time_counters} time counters ({card.printing.suspend_cost_str})"
    )


@dataclass(frozen=True)
class RemoveTimeCounterFromSuspendedSource(Effect):
    """Suspend's upkeep trigger: "At the beginning of your upkeep, if this
    card is suspended, remove a time counter from it." (the intervening
    "if" is re-checked on resolution). Removing the last counter triggers
    ``CastSuspendedSourceFree``. Also the effect of Greater Gargadon's
    "Remove a time counter from this card."."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        card = state.resolving_source
        if card is None or not is_suspended(card):
            return
        remove_time_counter(state, card)

    def describe(self) -> str:
        return "remove a time counter from this suspended card"


def _fork_exile_of(state: "GameState", card) -> None:
    from mtgz_ir.game import _CowList

    exile = state.player_by_id(card.owner_id).exile
    if isinstance(exile, _CowList):
        exile.fork()


def remove_time_counter(state: "GameState", card) -> None:
    """Remove one time counter from a suspended ``card``; when it was the
    last, its "When the last time counter is removed" trigger goes on the
    stack (CR 702.62a)."""
    from mtgz_ir.actions import push_triggered_ability
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import Always, Event

    _fork_exile_of(state, card)
    card.time_counters -= 1
    state.log.append(
        f"{card.printing.name}: a time counter is removed "
        f"({card.time_counters} left)"
    )
    if card.time_counters == 0:
        push_triggered_ability(
            state,
            source=card,
            ability=TriggeredAbility(
                event=Event.BEGINNING_OF_UPKEEP, predicate=Always(),
                effect=CastSuspendedSourceFree(),
            ),
            ability_index=-1,
            controller_id=card.owner_id,
            label="suspend (last time counter)",
        )


@dataclass(frozen=True)
class CastSuspendedSourceFree(Effect):
    """"When the last time counter is removed from this card, if it's
    exiled, you may cast it without paying its mana cost if able. If you
    don't, it remains exiled. If you cast a creature spell this way, it
    gains haste until you lose control of the spell or the permanent it
    becomes." (CR 702.62a). Cast during the resolution through
    ``creature_casting_ub.cast_now`` (timing ignored, CR 608.2g)."""

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.creature_casting_ub import can_cast_now, cast_now
        from mtgz_ir.game import Zone

        card = state.resolving_source
        if card is None or card.zone != Zone.EXILE:
            return
        if card not in state.player_by_id(card.owner_id).exile:
            return
        if not can_cast_now(state, controller_id, card, free=True):
            state.log.append(f"{card.printing.name} can't be cast; stays exiled")
            return
        p = state.player_by_id(controller_id)
        if p.agent is not None and not p.agent.choose_yes_no(
            state, controller_id,
            f"Cast suspended {card.printing.name} for free?",
        ):
            return
        if card.printing.is_creature:
            state.add_continuous_effect(
                source_id=card.id, layer=6, kind="grant_keyword",
                target_id=card.id, params=("haste",),
                description=f"{card.printing.name} has haste (suspend)",
            )
        cast_now(state, controller_id, card, free=True)

    def describe(self) -> str:
        return "you may cast this suspended card without paying its mana cost"


def push_suspend_upkeep_triggers(state: "GameState") -> None:
    """At the beginning of the active player's upkeep, each card they own
    that is suspended triggers (the single ``turn`` upkeep hook)."""
    from mtgz_ir.actions import push_triggered_ability
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import Always, Event

    ap = state.active_player()
    for card in list(ap.exile):
        if card.owner_id != ap.id or not is_suspended(card):
            continue
        push_triggered_ability(
            state,
            source=card,
            ability=TriggeredAbility(
                event=Event.BEGINNING_OF_UPKEEP, predicate=Always(),
                effect=RemoveTimeCounterFromSuspendedSource(),
            ),
            ability_index=-1,
            controller_id=ap.id,
            label="suspend upkeep",
        )


# ---------------------------------------------------------------------------
# Counters and state triggers
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RemoveCountersFromSource(Effect):
    """"Remove a[n] <kind> counter from this <permanent>." (Dark Depths'
    ``{3}`` ability) -- from the resolving source, if it's still on the
    battlefield."""

    kind: str = ""
    amount: int = 1

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import remove_counters

        src = state.resolving_source
        if src is None or src not in state.battlefield:
            return
        n = remove_counters(src, self.kind, self.amount)
        state.log.append(
            f"{src.printing.name}: {n} {self.kind} counter(s) removed"
        )

    def describe(self) -> str:
        return f"remove {self.amount} {self.kind} counter(s) from this"


@dataclass(frozen=True)
class HasNoCountersOfKind:
    """State-trigger condition: the source has no ``kind`` counters."""

    kind: str

    def holds(self, state: "GameState", obj) -> bool:
        from mtgz_ir.game import counter_count

        return counter_count(obj, self.kind) == 0

    def describe(self) -> str:
        return f"this permanent has no {self.kind} counters on it"


@dataclass(frozen=True)
class SacrificeSourceThen(Effect):
    """"Sacrifice it. If you do, <effect>." -- the resolving source is
    sacrificed if its controller still controls it on the battlefield;
    only then does ``then_effect`` happen (Dark Depths' Marit Lage)."""

    then_effect: "Effect | None" = None

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.turn import sacrifice

        src = state.resolving_source
        if (
            src is None or src not in state.battlefield
            or src.controller_id != controller_id
        ):
            state.log.append("nothing to sacrifice")
            return
        sacrifice(state, src)
        if src in state.battlefield:  # pragma: no cover - sacrifice can't fail
            return
        self.then_effect.resolve(state, controller_id, targets)

    def describe(self) -> str:
        return f"sacrifice it; if you do, {self.then_effect.describe()}"


# ---------------------------------------------------------------------------
# Mana abilities: what they produced
# ---------------------------------------------------------------------------


def after_mana_ability(
    state: "GameState", player_id: int, source, pool_before: int,
    *, tapped: bool = False,
) -> None:
    """Called by ``actions._activate_ability`` right after a mana ability
    of ``source`` resolved; the coins from index ``pool_before`` on are
    what it produced. ``tapped``: the ability's cost tapped the source
    ("tap it for mana").

    - Mana from a snow source is snow mana (CR 107.4h, paying {S}).
    - Kinnan, Bonder Prodigy: tapping a nonland permanent for mana adds
      one more mana of a type it produced (a triggered mana ability).
    - "Whenever you tap this <permanent> for mana" triggers (Forbidden
      Orchard) go on the stack."""
    from dataclasses import replace

    pool = state.player_by_id(player_id).mana_pool
    if "Snow" in source.printing.supertypes:
        for i in range(pool_before, len(pool)):
            pool[i] = replace(pool[i], snow=True)
    if not tapped:
        return
    produced = pool[pool_before:]
    if produced and not source.printing.is_land:
        _kinnan_extra_mana(state, player_id, produced)
    if source.printing.tapped_for_mana_triggers:
        push_tapped_for_mana_triggers(state, player_id, source)


def _kinnan_extra_mana(state: "GameState", player_id: int, produced) -> None:
    from mtgz_ir.game import ManaCoin, _source_layer6_wiped

    types = sorted({c.color for c in produced})
    p = state.player_by_id(player_id)
    for o in state.battlefield:
        if (
            o.controller_id != player_id
            or not o.printing.extra_mana_when_you_tap_nonland_for_mana
        ):
            continue
        if state.continuous_effects and _source_layer6_wiped(state, o):
            continue
        color = types[0]
        if p.agent is not None and len(types) > 1:
            picked = p.agent.choose_mana_color(state, player_id, types)
            if picked in types:
                color = picked
        p.mana_pool.append(ManaCoin(color=color))
        state.log.append(f"{o.printing.name} adds an additional {{{color}}}")


def push_tapped_for_mana_triggers(
    state: "GameState", player_id: int, source,
) -> None:
    """"Whenever you tap this <permanent> for mana, <effect>." -- stack
    ``source``'s triggers (targets picked as they go on the stack)."""
    from mtgz_ir.actions import push_triggered_ability

    for trig in source.printing.tapped_for_mana_triggers:
        push_triggered_ability(
            state, source=source, ability=trig, ability_index=-1,
            controller_id=player_id, label="tapped for mana",
        )


# "Until end of turn, whenever a player taps an <Subtype> for mana, that
# player adds an additional {C}." -- a registry entry (layer 0: no layer
# effect, only the cleanup expiry) read by automatic payment through
# ``actions.extra_mana_grants_for_tapped_lands``.
UEOT_LAND_TAP_MANA_KIND = "roto_extra_mana_on_land_tap"


def ueot_land_tap_mana_grants(state: "GameState") -> list:
    return [
        tuple(ce.params) for ce in state.continuous_effects
        if ce.kind == UEOT_LAND_TAP_MANA_KIND
    ]


@dataclass(frozen=True)
class GrantExtraManaOnLandTapUntilEndOfTurn(Effect):
    """High Tide: "Until end of turn, whenever a player taps an Island
    for mana, that player adds an additional {U}." (every player's lands;
    expires at cleanup)."""

    subtype: str = "Island"
    color: str = "U"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        src = state.resolving_spell or state.resolving_source
        state.add_continuous_effect(
            source_id=src.id if src is not None else -1, layer=0,
            kind=UEOT_LAND_TAP_MANA_KIND, params=(self.subtype, self.color),
            expires_at="cleanup",
            description=(
                f"{self.subtype}s tapped for mana add an additional "
                f"{{{self.color}}} this turn"
            ),
        )

    def describe(self) -> str:
        return (
            f"until end of turn, whenever a player taps an {self.subtype} "
            f"for mana, that player adds an additional {{{self.color}}}"
        )


__all__ = [
    "after_mana_ability",
    "CounterOnSourceThenPunishPerCounter",
    "optional_sacrifice_for_cost",
    "PerPermanentSacrificedForCost",
    "choose_additional_cost_x",
    "DealXDamageToEachOfXTargets",
    "pay_discard_x",
    "GrantLifeForColorlessUntilEndOfTurn",
    "life_mana_units_needed",
    "pay_life_for_mana",
    "ManaSpentToCastSource",
    "spent_mana_choice",
    "CopyImprintedCardMayCastFree",
    "choose_color_as_enters",
    "enchantable_land",
    "enchanted_land_mana_grants",
    "AdditionalCombatAfterThisMainPhase",
    "CreatureDiedThisTurn",
    "roto_next_step",
    "PayXEnergyCost",
    "ReturnTargetGraveyardCreatureWithMVX",
    "x_from_cost_atoms",
    "OnceEachTurnCost",
    "HasNotAddedManaWithThisAbilityThisTurn",
    "MayAddManaPerTargetOpponentsSubtype",
    "activate_exile_ability",
    "ExileZoneAbility",
    "roto_legal_actions",
    "ConvergeDrawDamageGainLife",
    "MillTargetPlayerMoreIfManaSpent",
    "GetEnergyThenExileUntilNonlandCastForEnergy",
    "GetXEnergyPayAnyDestroyMVAtMostPaid",
    "SacrificeSourceUnlessSacrificeTotalPower",
    "LookAtTopMayPutOntoBattlefieldRestBottom",
    "GrantExtraManaOnLandTapUntilEndOfTurn",
    "push_tapped_for_mana_triggers",
    "ueot_land_tap_mana_grants",
    "ReturnPermanentYouControlToHand",
    "DestroyEachThenAddManaPerDestroyed",
    "AddFormulaManaInAnyCombination",
    "SourcePower",
    "AddAnyColorManaForChosenCreatureType",
    "HasNoCountersOfKind",
    "RemoveCountersFromSource",
    "SacrificeSourceThen",
    "CastSuspendedSourceFree",
    "RemoveTimeCounterFromSuspendedSource",
    "ExileHandDrawThatManyMayPlayThem",
    "ExileTopCardsOfYourLibrary",
    "MayCastTargetGraveyardCardFree",
    "AddManaPerCardInTargetOpponentsHand",
    "CardsNamedInAllGraveyards",
    "EnteredThisTurn",
]
