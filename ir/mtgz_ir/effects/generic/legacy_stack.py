"""Stack effects of the Legacy cards outside the cube (pool ``legacy``):
the card IR of mtg-zero's Legacy Doomsday deck (its lane B: Misdirection).

* :class:`ChangeTheTargetOfTargetWithASingleTarget` -- "Change the target
  of target spell with a single target." (Misdirection; also the shape of
  Deflection, Shunt, Swerve, Ricochet Trap, and with "or ability",
  Willbender): CR 115.7a (each target can be changed only to another legal
  target; with none, it is unchanged), 115.9a (a spell "with a single
  target": one target chosen as it was put on the stack, each instance
  counted, whatever its legality now), 115.5 (a spell can't target itself),
  608.2b (the new target is chosen as this resolves, this still on the
  stack).

The Python engine doesn't model it: its spell stack items carry no
targets (``StackItem.targets`` is empty for spells), so there is nothing to
change. The class is data for the card IR, like the legacy pool's other
card-IR-only abilities (SEARCH_LIMITATIONS, "Legacy pool" 2): choosing or
resolving it raises ``NotImplementedError`` (never a silent no-op).
"""

from __future__ import annotations

from dataclasses import dataclass

from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.object_predicates import ObjectPredicate
from mtgz_ir.effects.vocab import StackObjectKind, coerce_tuple


@dataclass(frozen=True)
class ChangeTheTargetOfTargetWithASingleTarget(Effect):
    """"Change the target of target <spell / spell or ability> with a
    single target" (CR 115.7a, 115.9a).

    ``kinds``: what may be targeted (``"spell"``; with "or ability" also
    ``"activated"`` and ``"triggered"``). ``spell_filter``: the printed
    spell phrase's filter (``None`` for the bare "spell"). The target must
    have exactly one target instance (CR 115.9a). On resolution its
    controller must change that target to another target legal for it
    (checked with this spell still on the stack: it may become the new
    target); with no other legal target nothing changes (CR 115.7a)."""

    kinds: tuple[StackObjectKind, ...] = (StackObjectKind.SPELL,)
    spell_filter: "ObjectPredicate | None" = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "kinds", coerce_tuple(StackObjectKind, self.kinds))

    def requires_target(self) -> bool:
        return True

    def target_count_range(self) -> tuple[int, int]:
        return (1, 1)

    def legal_targets(self, state, controller_id):
        raise NotImplementedError(
            "ChangeTheTargetOfTargetWithASingleTarget is card-IR data for "
            "mtg-zero: the Python engine's spells carry no targets to change "
            "(SEARCH_LIMITATIONS, 'Legacy pool' 2)"
        )

    def resolve(self, state, controller_id, targets=()):
        raise NotImplementedError(
            "ChangeTheTargetOfTargetWithASingleTarget is card-IR data for "
            "mtg-zero (SEARCH_LIMITATIONS, 'Legacy pool' 2)"
        )

    def describe(self) -> str:
        what = " or ".join(
            "spell" if k == StackObjectKind.SPELL else f"{k} ability" for k in self.kinds
        )
        return f"change the target of target {what} with a single target"
