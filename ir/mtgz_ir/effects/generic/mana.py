"""Generic effects cluster module. Behavior and API unchanged from
the pre-split mtgz_ir.effects.generic monolith; this is a pure file move."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import SpendRestriction, coerce, plain
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
class AddManaToPool(Effect):
    """Add mana to the controller's mana pool. Paired with an
    ActivatedAbility flagged is_mana_ability=True so activation
    bypasses the stack.

    ``produces`` is a parsed list of (color, count) tuples. ``color``
    is one of "W", "U", "B", "R", "G", "C" (colorless), or "any"
    (wildcard satisfying any colored requirement; used by All-Fates
    Scroll's "Add one mana of any color").

    ``spend_restriction`` tags each generated coin for cost-payment
    filtering (e.g., "artifact_spell" for Steelswarm Operator's
    "Spend this mana only to cast an artifact spell";
    "artifact_activation" for Steelswarm's second mana ability
    "Spend this mana only to activate abilities of artifact
    sources").
    """

    produces: tuple[tuple[str, int], ...]
    spend_restriction: "SpendRestriction | None" = None
    # "If any of that mana is spent on a creature spell, it gains haste
    # until end of turn." — tags each minted coin
    # (``ManaCoin.grants_haste_to_creature_spell``).
    grants_haste_to_creature_spell: bool = False
    # "... and that spell can't be countered" (Delighted Halfling).
    makes_spell_uncounterable: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "spend_restriction", coerce(SpendRestriction, self.spend_restriction))

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        p = state.player_by_id(controller_id)
        minted: list[str] = []
        for color, count in self.produces:
            if color == "chosen":
                # "Add one mana of the chosen color" (Sunken Citadel): the
                # color chosen as the source entered (CR 614.12).
                src = state.resolving_source
                color = getattr(src, "chosen_color", None) or "C"
            for _ in range(count):
                p.mana_pool.append(
                    ManaCoin(
                        color=color,
                        spend_restriction=plain(self.spend_restriction),
                        grants_haste_to_creature_spell=(
                            self.grants_haste_to_creature_spell
                        ),
                        makes_spell_uncounterable=(
                            self.makes_spell_uncounterable
                        ),
                    )
                )
                minted.append(color)
        restriction = (
            f" ({self.spend_restriction})" if self.spend_restriction else ""
        )
        state.log.append(
            f"P{controller_id} adds {'+'.join(minted)}{restriction} to "
            f"mana pool"
        )

    def describe(self) -> str:
        parts = [
            (f"{{{c}}}" * n if c != "any" else "{any}") for c, n in self.produces
        ]
        base = "Add " + "".join(parts)
        if self.spend_restriction:
            base += f" (spend only to {self.spend_restriction})"
        return base



@dataclass(frozen=True)
class AddOneOfManaCombinations(Effect):
    """Add ONE of several fixed mana combinations, chosen by the
    controller at resolution — the filter-land shape "{U/B}, {T}: Add
    {U}{U}, {U}{B}, or {B}{B}." (Sunken Ruins).

    ``options`` is a tuple of combinations, each a ``produces``-style
    tuple of ``(color, count)`` pairs. The pick goes through
    ``Agent.choose_mode`` over one description per combination
    (``"{U}{U}"`` ...); an agent-less controller or an out-of-range
    answer takes the first combination.
    """

    options: tuple[tuple[tuple[str, int], ...], ...]

    @staticmethod
    def _label(combo: tuple[tuple[str, int], ...]) -> str:
        return "".join(f"{{{c}}}" * n for c, n in combo)

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        p = state.player_by_id(controller_id)
        idx = 0
        if p.agent is not None and len(self.options) > 1:
            picked = p.agent.choose_mode(
                state, controller_id,
                [self._label(c) for c in self.options],
            )
            if isinstance(picked, int) and 0 <= picked < len(self.options):
                idx = picked
        combo = self.options[idx]
        for color, count in combo:
            for _ in range(count):
                p.mana_pool.append(ManaCoin(color=color))
        state.log.append(
            f"P{controller_id} adds {self._label(combo)} to mana pool"
        )

    def describe(self) -> str:
        labels = [self._label(c) for c in self.options]
        return "add " + ", ".join(labels[:-1]) + ", or " + labels[-1]


@dataclass(frozen=True)
class AddManaOfColorPerCount(Effect):
    """Add ``amount.value(state, controller)`` mana of one fixed
    ``color`` — "{T}: Add {G} for each creature you control." (Gaea's
    Cradle) / "{T}: Add {U} for each artifact you control." (Tolarian
    Academy). The count is read at resolution (the mana ability
    resolves immediately, CR 605.3a), so zero matching permanents adds
    no mana.
    """

    color: str
    amount: "Formula"

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        n = self.amount.value(state, controller_id)
        p = state.player_by_id(controller_id)
        for _ in range(n):
            p.mana_pool.append(ManaCoin(color=self.color))
        state.log.append(
            f"P{controller_id} adds {self.color}x{n} to mana pool"
        )

    def describe(self) -> str:
        return f"add {{{self.color}}} for each ({self.amount!r})"


@dataclass(frozen=True)
class AddNManaOfChosenColor(Effect):
    """Add ``amount`` mana of a single color chosen by the controller
    at resolution time.

    Models oracle text like Lotus Field's "Add three mana of any one
    color" — the player picks ONE of W/U/B/R/G and gets ``amount``
    coins of that one color (not ``amount`` independent picks). This
    is strictly less flexible than minting "any"-coloured wildcards,
    so it is faithful to the printed clause.

    The choice is delegated to ``Agent.choose_mana_color``. If the
    controller has no agent seated (e.g. some unit-test setups), the
    first color in ``options`` is used as a deterministic fallback.

    Designed to be reusable for other "controller picks one color"
    mana abilities (Reflecting Pool, Meteor Crater, Sunken Citadel),
    though wiring those is left for a follow-up.
    """

    amount: int
    options: tuple[str, ...] = ("W", "U", "B", "R", "G")

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        p = state.player_by_id(controller_id)
        opts = list(self.options)
        chosen = opts[0]
        if p.agent is not None:
            picked = p.agent.choose_mana_color(state, controller_id, opts)
            if picked in opts:
                chosen = picked
        for _ in range(self.amount):
            p.mana_pool.append(ManaCoin(color=chosen))
        state.log.append(
            f"P{controller_id} adds {chosen}x{self.amount} to mana pool "
            f"(chosen color)"
        )

    def describe(self) -> str:
        return f"add {self.amount} mana of any one color"



@dataclass(frozen=True)
class AddChargeCounterManaOfChosenColor(Effect):
    """Add X mana of any one color, where X is the number of charge
    counters on the source permanent.

    Models The Eternity Elevator's tier-gated activated ability:
    ``20+ | {T}: Add X mana of any one color, where X is the number of
    charge counters on The Eternity Elevator.``

    On resolution, reads ``state.resolving_source.charge_counters`` for
    X; if the source object is unavailable (e.g. the ability was somehow
    triggered off-battlefield), produces 0 mana. Delegates to
    ``AddNManaOfChosenColor`` at resolve time so the controller's agent
    picks the color via ``choose_mana_color``.

    Literal-line recognizer: only The Eternity Elevator in the EoE pool
    prints this "X mana of any one color where X is the charge counter
    count" shape today; a second Spacecraft with the same shape would
    call for a parameterized grammar arm in the spacecraft builder.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        source = state.resolving_source
        if source is None:
            return
        amount = source.charge_counters
        if amount <= 0:
            return
        AddNManaOfChosenColor(amount=amount).resolve(
            state, controller_id, targets,
        )

    def describe(self) -> str:
        return "add X mana of any one color, where X is the number of charge counters on this"



@dataclass(frozen=True)
class AddOneManaFromControlledLandsProductions(Effect):
    """Reflecting Pool: "{T}: Add one mana of any type that a land you
    control could produce."

    On resolution, walk the controller's battlefield and union the
    ``produces`` color set of every land they control. Ask the
    controller's agent to pick one color from that union via
    ``choose_mana_color``, then mint one ``ManaCoin`` of that color.

    Defensive fallback: if the controller has no lands at all, or no
    land in play has any non-empty ``produces`` set (e.g., a colorless
    artifact-land cycle), mint a single ``"C"`` colorless coin so the
    ability still produces *something*. In normal play Reflecting Pool
    itself is on the battlefield, so the union is always non-empty for
    any deck containing a colored basic.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        p = state.player_by_id(controller_id)
        options: set[str] = set()
        for o in state.battlefield:
            if o.controller_id != controller_id:
                continue
            if not o.printing.is_land:
                continue
            options |= set(o.printing.produces)
        if not options:
            color = "C"
        else:
            sorted_options = sorted(options)
            if p.agent is not None:
                color = p.agent.choose_mana_color(
                    state, controller_id, sorted_options,
                )
            else:
                color = sorted_options[0]
            if color not in options:
                color = sorted_options[0]
        p.mana_pool.append(ManaCoin(color=color))
        state.log.append(
            f"P{controller_id} adds {color} to mana pool"
        )

    def describe(self) -> str:
        return "add one mana of any type a land you control could produce"



@dataclass(frozen=True)
class AddOneManaOfChosenPermanentColor(Effect):
    """Meteor Crater: "{T}: Choose a color of a permanent you control.
    Add one mana of that color."

    On resolution, walk the controller's battlefield and union the
    colors of every permanent they control (per ``mana_cost.colors``).
    Lands and other colorless permanents contribute nothing — colors
    come from colored mana symbols in the cost (rule 105.2). Ask the
    agent to pick one color from the union via ``choose_mana_color``,
    then mint one ``ManaCoin`` of that color.

    Defensive fallback: if the controller has no colored permanents
    (e.g., only lands and colorless artifacts), mint a single ``"C"``
    colorless coin so the ability still produces *something*.
    """

    def resolve(
        self, state: "GameState", controller_id: int,
        targets: tuple[Target, ...] = (),
    ) -> None:
        from mtgz_ir.game import ManaCoin

        p = state.player_by_id(controller_id)
        from mtgz_ir.game import effective_colors

        options: set[str] = set()
        for o in state.battlefield:
            if o.controller_id != controller_id:
                continue
            options |= set(effective_colors(state, o))
        if not options:
            color = "C"
        else:
            sorted_options = sorted(options)
            if p.agent is not None:
                color = p.agent.choose_mana_color(
                    state, controller_id, sorted_options,
                )
            else:
                color = sorted_options[0]
            if color not in options:
                color = sorted_options[0]
        p.mana_pool.append(ManaCoin(color=color))
        state.log.append(
            f"P{controller_id} adds {color} to mana pool"
        )

    def describe(self) -> str:
        return (
            "add one mana of a color of a permanent you control"
        )
