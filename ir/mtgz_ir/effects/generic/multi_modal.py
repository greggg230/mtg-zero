"""Multi-mode spells: "Choose two —", "Choose three. You may choose the
same mode more than once.", "Choose one or more —" with Escalate, and
Spree (CR 700.2, 702.120 Escalate, 702.172 Spree).

:class:`ChooseModes` is picked up by the cast pipeline
(``actions._choose_modes_at_cast`` / ``_select_spell_targets`` /
``_cast_spell``):

1. Modes are chosen at cast time (CR 601.2b) via the agent's
   ``choose_modes`` hook, then sanitized: unavailable modes (a targeted
   mode with no legal target) are dropped, duplicates are dropped
   unless ``repeats``, the count is capped at ``max_modes`` and — for
   Spree / Escalate — trimmed until the extra costs are payable, then
   padded up to ``min_modes`` (cycling through the agent's own picks,
   then the available modes).
2. Targets are chosen per chosen mode instance (CR 601.2c), each
   instance independently (a repeated mode may pick the same target
   again). Every targeted mode here takes exactly one target, so the
   flat target tuple splits back into per-mode targets at resolution.
3. Spree adds each chosen mode's mana cost to the total cost; Escalate
   charges its cost (discard a card) once per mode beyond the first.
4. Resolution runs the chosen modes in order (CR 700.2d) from
   ``state.resolving_modes``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from mtgz_ir.alt_casting import cost_plus
from mtgz_ir.effects.base import Effect, Target

if TYPE_CHECKING:
    from mtgz_ir.cards.base import ManaCost
    from mtgz_ir.game import GameObject, GameState


def _mode_target_count(mode: Effect) -> int:
    return mode.target_count_range()[1]


def _x_dependent(mode: Effect) -> bool:
    """A mode whose targets depend on the spell's X: "up to X target ..."
    (``count_from_x``) or a target filter that reads X (``targets_use_x``,
    "target creature with mana value X or less")."""
    return bool(
        getattr(mode, "count_from_x", False)
        or getattr(mode, "targets_use_x", False)
    )


# An X no spell can pay: an X-dependent mode is available to choose (CR
# 700.2a) when some X would give it a legal target; X itself is
# announced after the modes (CR 601.2b).
_ANY_X = 10 ** 6


@dataclass(frozen=True)
class ChooseModes(Effect):
    modes: tuple[Effect, ...]
    min_modes: int = 1
    max_modes: int = 1
    repeats: bool = False
    # Spree: per-mode additional mana cost strings, parallel to modes.
    mode_mana_costs: tuple[str, ...] = ()
    # Escalate: "discard a card" paid per mode beyond the first.
    escalate_discard: bool = False
    header: str = ""

    def __post_init__(self) -> None:
        for mode in self.modes:
            lo, hi = mode.target_count_range()
            if (lo, hi) in ((0, 0), (1, 1)):
                continue
            if lo == 0 and getattr(mode, "count_from_x", False):
                continue  # "up to X target ..." (Kozilek's Command)
            raise ValueError(
                "ChooseModes modes must take zero or exactly one target"
                " (or up to X)"
            )
        if self.mode_mana_costs and len(self.mode_mana_costs) != len(self.modes):
            raise ValueError("mode_mana_costs must parallel modes")

    @property
    def count_from_x(self) -> bool:
        """X is announced before targets when a mode's targets depend on
        it (``actions._targets_depend_on_x``)."""
        return any(_x_dependent(m) for m in self.modes)

    # ---- targeting surface (the cast pipeline selects per mode) ----

    def requires_target(self) -> bool:
        return False

    def target_count_range(self) -> tuple[int, int]:
        return (0, 0)

    def legal_targets(
        self, state: "GameState", controller_id: int,
    ) -> list[Target]:
        out: list[Target] = []
        for mode in self.modes:
            for t in mode.legal_targets(state, controller_id):
                if t not in out:
                    out.append(t)
        return out

    # ---- cast-time mode choice ----

    def mode_available(self, state: "GameState", controller_id: int, idx: int) -> bool:
        mode = self.modes[idx]
        if _mode_target_count(mode) == 0:
            return True
        if mode.target_count_range()[0] == 0:
            return True  # "up to" targets: always choosable
        if _x_dependent(mode):
            prior = state.resolving_x_value
            state.resolving_x_value = _ANY_X
            try:
                return bool(mode.legal_targets(state, controller_id))
            finally:
                state.resolving_x_value = prior
        return bool(mode.legal_targets(state, controller_id))

    def _mode_cost(self, picks) -> "ManaCost":
        from mtgz_ir.cards.base import ManaCost, parse_mana_cost

        total = ManaCost()
        for i in picks:
            total = cost_plus(total, parse_mana_cost(self.mode_mana_costs[i]))
        return total

    def mode_mana_cost(self, picks: tuple[int, ...]) -> "ManaCost":
        """Spree: extra mana for the chosen modes (zero otherwise)."""
        from mtgz_ir.cards.base import ManaCost

        if not self.mode_mana_costs:
            return ManaCost()
        return self._mode_cost(picks)

    def single_mode_mana_costs(self) -> list["ManaCost"]:
        """Spree: each mode's own extra cost (a cast picks at least one,
        so legal actions check base + each single mode)."""
        from mtgz_ir.cards.base import ManaCost, parse_mana_cost

        if not self.mode_mana_costs:
            return [ManaCost()]
        return [parse_mana_cost(c) for c in self.mode_mana_costs]

    def choose_at_cast(
        self, state: "GameState", player_id: int, card: "GameObject",
        base_cost: "ManaCost",
    ) -> tuple[int, ...]:
        from mtgz_ir.actions import can_pay_cost

        available = [
            i for i in range(len(self.modes))
            if self.mode_available(state, player_id, i)
        ]
        if not available:
            return ()
        p = state.player_by_id(player_id)
        raw: list = []
        if p.agent is not None:
            raw = list(p.agent.choose_modes(
                state, player_id, [m.describe() for m in self.modes],
            ))
        picks: list[int] = []
        for i in raw:
            if i not in available:
                continue
            if not self.repeats and i in picks:
                continue
            picks.append(i)
        picks = picks[: self.max_modes]

        def affordable(sel: list[int]) -> bool:
            if self.escalate_discard:
                others = [c for c in p.hand if c is not card]
                if len(sel) - 1 > len(others):
                    return False
            if self.mode_mana_costs:
                return can_pay_cost(
                    state, player_id,
                    cost_plus(base_cost, self._mode_cost(sel)),
                    sac_for_mana=True,
                )
            return True

        while len(picks) > 1 and not affordable(picks):
            picks.pop()
        # Pad up to the minimum: cycle the agent's own picks when modes
        # may repeat, else take the next unused available modes.
        pool = picks[:] if (self.repeats and picks) else available
        k = 0
        while len(picks) < self.min_modes and pool:
            cand = pool[k % len(pool)]
            k += 1
            if not self.repeats and cand in picks:
                if k > len(pool):
                    break
                continue
            picks.append(cand)
        if self.mode_mana_costs and picks and not affordable(picks):
            # Fall back to the cheapest single affordable mode.
            singles = [i for i in available if affordable([i])]
            picks = singles[:1]
        return tuple(picks)

    def select_targets(
        self, state: "GameState", player_id: int, picks: tuple[int, ...],
        *, x_value: "int | None" = None, card: "GameObject | None" = None,
    ) -> tuple[Target, ...]:
        """Targets per chosen mode instance, in mode order. With the
        spell's X known (``x_value``), X-dependent modes are shaped by it
        ("up to X target ...") and their filters read it ("mana value X
        or less"). How many targets each instance took is recorded on
        the spell (``card.mode_target_counts``) so resolution can split
        them back when a mode took fewer than its maximum."""
        from mtgz_ir.actions import cast_time_effect

        p = state.player_by_id(player_id)
        chosen: list[Target] = []
        counts: list[int] = []
        prior_x = state.resolving_x_value
        if x_value is not None:
            state.resolving_x_value = x_value
        try:
            for idx in picks:
                mode = cast_time_effect(self.modes[idx], x_value=x_value)
                lo, hi = mode.target_count_range()
                if hi == 0:
                    counts.append(0)
                    continue
                pool = list(mode.legal_targets(state, player_id))
                taken: list[Target] = []
                while pool and len(taken) < hi:
                    pick = (
                        p.agent.choose_target(state, player_id, pool)
                        if p.agent else None
                    )
                    if pick is None or pick not in pool:
                        if len(taken) >= lo:
                            break
                        pick = pool[0]
                    taken.append(pick)
                    pool.remove(pick)
                chosen.extend(taken)
                counts.append(len(taken))
        finally:
            state.resolving_x_value = prior_x
        if card is not None:
            card.mode_target_counts = tuple(counts)
        return tuple(chosen)

    def pay_escalate(
        self, state: "GameState", player_id: int, picks: tuple[int, ...],
    ) -> None:
        from mtgz_ir.effects.generic.discard import DiscardSelf

        if not self.escalate_discard or len(picks) <= 1:
            return
        state.log.append(
            f"escalate: P{player_id} discards {len(picks) - 1} card(s)"
        )
        DiscardSelf(amount=len(picks) - 1).resolve(state, player_id)

    # ---- resolution ----

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        picks = state.resolving_modes or (0,)
        spell = state.resolving_spell
        counts = getattr(spell, "mode_target_counts", ()) if spell else ()
        if len(counts) != len(picks):
            counts = tuple(_mode_target_count(self.modes[i]) for i in picks)
        pos = 0
        for idx, n in zip(picks, counts):
            mode = self.modes[idx]
            mode_targets = tuple(targets[pos:pos + n])
            pos += n
            lo = mode.target_count_range()[0]
            if lo and len(mode_targets) < lo:
                state.log.append(f"mode {idx} has no target — skipped")
                continue
            state.log.append(f"mode {idx}: {mode.describe()}")
            mode.resolve(state, controller_id, mode_targets)

    def describe(self) -> str:
        return f"{self.header}: " + " | ".join(m.describe() for m in self.modes)


__all__ = ["ChooseModes", "merge_mana_costs"]
