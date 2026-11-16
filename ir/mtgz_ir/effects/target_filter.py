"""Target-filter grammar — what counts as a legal target for a
``TargetSpec``.

A ``TargetFilter`` answers "is this object a legal target for this
slot?" given the object plus the current ``GameState`` plus the
controller of the resolving spell/ability. The filter vocabulary is:

- ``ObjectPredicate`` — every predicate atom from
  :mod:`mtgz_ir.effects.object_predicates` is itself a ``TargetFilter``,
  so ``IsCreature()`` / ``IsPlayer()`` / ``IsCreature() & IsTapped()``
  / ``HasSubtype("Sliver")`` all describe a target shape.
- :class:`OneOf` — a structural disjunction over filters. The "any
  target" target type expands to ``OneOf(IsCreature(),
  IsPlaneswalker(), IsBattle(), IsPlayer())``; a future "target
  artifact or creature" filter would be ``OneOf(IsArtifact(),
  IsCreature())``.

Composition rationale: ``ObjectPredicate``'s own ``|`` operator
returns ``ObjectOr``, which works fine for predicates that all run
on objects with a ``printing`` (a creature *or* an artifact, both
permanents). ``OneOf`` is a separate type because the "any target"
expansion mixes object-level predicates (``IsCreature``, …) with the
``IsPlayer`` predicate whose semantics is "this is a Player, not a
permanent" — it's still an ``ObjectPredicate`` and composable via
``|``, but flagging the structural disjunction explicitly with
``OneOf`` makes the target-grammar tree readable as a tree instead
of a chain of ``ObjectOr`` nodes, and gives tests / parsers a
concrete type to dispatch on.

A ``TargetFilter`` is *purely* a per-object predicate. Cross-target
constraints ("another target" / "different target") live on
:class:`mtgz_ir.effects.targets.TargetSpec` as ``distinct_from``;
controller-relative constraints ("a creature an opponent controls",
"a creature you control") will compose at the filter level when the
parser introduces controller-aware atoms in a later PR — for now the
existing engine machinery handles those via per-effect overrides.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Union

from mtgz_ir.effects.object_predicates import ObjectPredicate

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


@dataclass(frozen=True)
class OneOf:
    """Match any of the wrapped sub-filters.

    Models structural disjunction in the target-filter grammar — most
    immediately, the "any target" expansion::

        OneOf(IsCreature(), IsPlaneswalker(), IsBattle(), IsPlayer())

    Members may be ``ObjectPredicate`` atoms or other ``OneOf``
    instances, mirroring the recursive shape of the broader
    ``TargetFilter`` union. Equality is structural (frozen dataclass +
    tuple of filters), so two ``OneOf(IsCreature(), IsPlayer())`` values
    constructed independently compare equal — useful for parser /
    decomposition tests that pin the produced filter shape.
    """

    filters: tuple["TargetFilter", ...]

    def __init__(self, *filters: "TargetFilter") -> None:
        # Frozen dataclass: bypass setattr by writing through __dict__.
        object.__setattr__(self, "filters", tuple(filters))

    def test(self, obj: "GameObject", state: "GameState") -> bool:
        return any(f.test(obj, state) for f in self.filters)


# Public alias for the target-filter union. Membership: every
# ``ObjectPredicate`` (atoms and ``ObjectAnd`` / ``ObjectOr`` /
# ``ObjectNot`` combinators) plus ``OneOf``. Both arms expose a
# ``.test(obj, state) -> bool`` method, which is the only contract the
# target-selection pipeline relies on.
TargetFilter = Union[ObjectPredicate, OneOf]
