"""Mass effects for the cube spells-B bucket (Toxic Deluge's "All
creatures get -X/-X until end of turn", Balance)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.effects.base import Effect

if TYPE_CHECKING:
    from mtgz_ir.game import GameState


@dataclass(frozen=True)
class PumpAllCreaturesUEOT(Effect):
    """"All creatures get +P/+T until end of turn", with optional
    X-scaled parts: the applied delta is ``power + x_power * X`` /
    ``toughness + x_toughness * X`` where X is
    ``state.resolving_x_value`` (Toxic Deluge: ``x_power=x_toughness=-1``
    with X paid as life). Every creature on the battlefield — both
    players' — is affected, through the layer-7 UEOT registry
    (``GameState.grant_pt_pump_ueot``)."""

    power: int = 0
    toughness: int = 0
    x_power: int = 0
    x_toughness: int = 0
    # "Other creatures get ..." (Harvester of Misery): every creature
    # except the resolving ability's source.
    exclude_source: bool = False

    def useful_x_cap(self, state: "GameState", controller_id: int) -> "int | None":
        """Largest X that still changes the outcome: once -X/-X kills the
        toughest creature, a bigger X does nothing more. ``None`` when X
        doesn't shrink toughness (no natural cap)."""
        if self.x_toughness >= 0:
            return None
        from mtgz_ir.game import effective_toughness, is_creature_now

        toughest = 0
        for o in state.battlefield:
            if is_creature_now(o):
                toughest = max(toughest, effective_toughness(state, o) + self.toughness)
        per_x = -self.x_toughness
        return -(-toughest // per_x)  # ceil

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import is_creature_now

        x = max(0, state.resolving_x_value)
        dp = self.power + self.x_power * x
        dt = self.toughness + self.x_toughness * x
        source = state.resolving_source if self.exclude_source else None
        affected = [
            o for o in state.battlefield
            if is_creature_now(o)
            and (source is None or o.id != source.id)
        ]
        who = "other" if self.exclude_source else "all"
        for o in affected:
            state.grant_pt_pump_ueot(
                source_id=o.id, target_id=o.id, power=dp, toughness=dt,
                description=f"{who} creatures get {dp:+d}/{dt:+d}",
            )
        state.log.append(
            f"{who} creatures get {dp:+d}/{dt:+d} until end of turn "
            f"({len(affected)} affected)"
        )

    def describe(self) -> str:
        def part(base: int, xcoef: int) -> str:
            if xcoef == 0:
                return f"{base:+d}"
            sign = "-" if xcoef < 0 else "+"
            return f"{sign}X" if base == 0 else f"{base:+d}{sign}X"

        who = "other" if self.exclude_source else "all"
        return (
            f"{who} creatures get {part(self.power, self.x_power)}/"
            f"{part(self.toughness, self.x_toughness)} until end of turn"
        )


def _apnap(state: "GameState", starting_id: int) -> list:
    """Living players in turn order starting with ``starting_id``."""
    n = len(state.players)
    start = state.index_of(starting_id)
    out = []
    for i in range(n):
        p = state.players[(start + i) % n]
        if not p.has_lost:
            out.append(p)
    return out


@dataclass(frozen=True)
class BalanceEffect(Effect):
    """Balance: "Each player chooses a number of lands they control equal
    to the number of lands controlled by the player who controls the
    fewest, then sacrifices the rest. Players discard cards and
    sacrifice creatures the same way."

    Lands, then hands, then creatures. Each surplus permanent is picked
    by its controller through ``choose_sacrifice`` (the "which of mine
    goes" decision); surplus hand cards through ``choose_discard``.
    """

    def _balance_permanents(self, state, controller_id, is_kind) -> None:
        from mtgz_ir.turn import sacrifice

        players = _apnap(state, controller_id)
        counts = {
            p.id: [o for o in state.battlefield
                   if o.controller_id == p.id and is_kind(o)]
            for p in players
        }
        fewest = min((len(v) for v in counts.values()), default=0)
        for p in players:
            owned = counts[p.id]
            doomed: list = []
            pool = list(owned)
            for _ in range(len(owned) - fewest):
                pick = pool[0]
                if p.agent is not None:
                    pid = p.agent.choose_sacrifice(state, p.id, [o.id for o in pool])
                    pick = next((o for o in pool if o.id == pid), pool[0])
                pool.remove(pick)
                doomed.append(pick)
            for o in doomed:
                if o in state.battlefield:
                    sacrifice(state, o)

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.effects.generic.card_flow import discard_card
        from mtgz_ir.game import is_creature_now

        self._balance_permanents(
            state, controller_id, lambda o: o.printing.is_land,
        )
        players = _apnap(state, controller_id)
        fewest = min((len(p.hand) for p in players), default=0)
        for p in players:
            for _ in range(len(p.hand) - fewest):
                chosen = p.hand[0]
                if p.agent is not None:
                    cid = p.agent.choose_discard(state, p.id, [c.id for c in p.hand])
                    chosen = next((c for c in p.hand if c.id == cid), p.hand[0])
                discard_card(state, p, chosen)
        self._balance_permanents(state, controller_id, is_creature_now)
        state.log.append("Balance resolves")

    def describe(self) -> str:
        return (
            "each player keeps as many lands, cards in hand and creatures "
            "as the player with the fewest; the rest are sacrificed / "
            "discarded"
        )


@dataclass(frozen=True)
class VoteExileNonlandPermanent(Effect):
    """Will of the council (Council's Judgment): "Starting with you, each
    player votes for a nonland permanent you don't control. Exile each
    permanent with the most votes or tied for most votes."

    Voting is not targeting (hexproof / protection don't stop it). The
    caster votes through ``choose_target`` over the eligible permanents;
    every other voter through ``choose_sacrifice`` (the "pick one of
    these permanents" decision — in two-player games the options are
    the voter's own permanents).
    """

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.game import is_creature_now
        from mtgz_ir.turn import move_off_battlefield

        options = [
            o for o in state.battlefield
            if not o.printing.is_land and o.controller_id != controller_id
            and not o.phased_out
        ]
        if not options:
            state.log.append("no permanent to vote for")
            return
        votes: dict[int, int] = {}
        for p in _apnap(state, controller_id):
            pick = options[0]
            if p.agent is not None:
                if p.id == controller_id:
                    tagged = [
                        ("creature" if is_creature_now(o) else "permanent", o.id)
                        for o in options
                    ]
                    choice = p.agent.choose_target(state, p.id, tagged)
                    pid = choice[1] if choice is not None else None
                else:
                    pid = p.agent.choose_sacrifice(state, p.id, [o.id for o in options])
                pick = next((o for o in options if o.id == pid), options[0])
            votes[pick.id] = votes.get(pick.id, 0) + 1
            state.log.append(f"P{p.id} votes for {pick.printing.name}")
        top = max(votes.values())
        for o in options:
            if votes.get(o.id) == top and o in state.battlefield:
                move_off_battlefield(state, o, destination="exile")
                state.log.append(f"{o.printing.name} is exiled (most votes)")

    def describe(self) -> str:
        return (
            "each player votes for a nonland permanent you don't control; "
            "exile each permanent with the most votes or tied"
        )


@dataclass(frozen=True)
class GrantAbilityToPermanentsYouControlUEOT(Effect):
    """"Until end of turn, <permanents> you control gain "<ability>"."
    (Rain of Filth: lands gain "Sacrifice this land: Add {B}."). Grants
    ``ability`` to every matching permanent the controller controls at
    resolution, through layer-6 ``grant_activated_ability`` entries that
    expire at cleanup. Granted mana abilities are picked up by the
    cast pipeline's sac-for-mana sources (``effective_activated_abilities``).
    """

    card_filter: "object" = None
    ability: "object" = None

    def resolve(self, state, controller_id, targets=()):
        from mtgz_ir.alt_casting import permanent_matches_filter
        from mtgz_ir.effects.combinators import ApplyEffect
        from mtgz_ir.effects.durations import EndOfTurn
        from mtgz_ir.effects.generic.grant_keywords import GrantActivatedAbility

        if self.ability is None:
            return
        matching = [
            o for o in state.battlefield
            if o.controller_id == controller_id
            and (self.card_filter is None
                 or permanent_matches_filter(state, o, self.card_filter))
        ]
        grant = ApplyEffect(
            inner=GrantActivatedAbility(ability=self.ability),
            ends_at=EndOfTurn(),
        )
        for o in matching:
            grant.resolve(state, controller_id, (("permanent", o.id),))
        state.log.append(
            f"{len(matching)} permanent(s) gain an ability until end of turn"
        )

    def describe(self) -> str:
        what = self.card_filter.describe() if self.card_filter else "permanent"
        return f"until end of turn, each {what} you control gains an ability"
