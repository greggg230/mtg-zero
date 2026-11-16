"""Target-spec grammar — per-effect target slots and back-references.

An :class:`mtgz_ir.effects.base.Effect` carries its target shape on a
``targets: tuple[TargetSlot, ...]`` field. Each ``TargetSlot`` is
either:

- :class:`TargetSpec` — a fresh target prompt: filter + count range
  + cross-slot distinctness. The caster picks one or more legal
  targets satisfying the filter at cast time; legality is re-checked
  at resolve time per CR 608.2b.
- :class:`SlotRef` — a back-reference to an *earlier* slot's resolved
  target. Models pronouns like "that creature" in the second/third
  effect of a many-effects-one-target sequence (Sparkmage's Gambit:
  "deal 4 damage to target creature. Tap that creature. It doesn't
  untap during its controller's next untap step."). A ``SlotRef`` is
  *not* a target choice — it inherits its parent slot's resolved
  object and its parent slot's legality.

Slot indices are 0-based and scoped to **one EffectSequence** —
modes of a modal effect get their own scope, so ``SlotRef(0)``
inside mode B refers to mode B's slot 0, not the outer sequence's
slot 0.

Resolution semantics (CR 608 / CR 115.7):

- Number of targets the spell has, for legality, equals the number
  of ``TargetSpec``s across the spell's effects (across selected
  modes for a modal). ``SlotRef``s do not count.
- At resolve time each ``TargetSpec`` re-checks legality against
  the current battlefield state. An effect whose ``TargetSpec`` is
  illegal skips. An effect whose ``SlotRef``'s parent slot is
  illegal also skips.
- ``distinct_from`` is enforced at *selection time*: the targeting
  pipeline filters the candidate pool to exclude already-picked
  targets from the listed prior slot indices. CR 115.6's
  "different target" rule lives here.

Modal cards layer on top via
:class:`mtgz_ir.effects.base.ModalEffect`, whose modes are each a
fresh ``EffectSequence`` with its own slot scope.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Union

from mtgz_ir.effects.target_filter import TargetFilter


@dataclass(frozen=True)
class TargetSpec:
    """A single target prompt — filter, count range, distinctness.

    Decomposition examples (full table in PR #3 design notes):

    - ``"target creature"`` → ``TargetSpec(IsCreature())``
    - ``"up to one target creature"`` →
      ``TargetSpec(IsCreature(), 0, 1)``
    - ``"two target creatures"`` → ``TargetSpec(IsCreature(), 2, 2)``
    - ``"any number of target creatures"`` →
      ``TargetSpec(IsCreature(), 0, math.inf)``
    - ``"any target"`` (damage) →
      ``TargetSpec(OneOf(IsCreature(), IsPlaneswalker(), IsBattle(),
      IsPlayer()))``
    - ``"another target"`` →
      ``TargetSpec(IsCreature(), distinct_from=(prior_idx,))``

    ``count_max`` accepts ``math.inf`` for "any number of target X"
    phrasings (Singularity Rupture's mill arm). The targeting
    pipeline collapses the range to ``0..min(count_max,
    len(legal_targets))`` at selection time.
    """

    filter: TargetFilter
    count_min: int = 1
    count_max: float = 1  # int or math.inf — Python types both as float-comparable
    distinct_from: tuple[int, ...] = ()


@dataclass(frozen=True)
class SlotRef:
    """A back-reference to an earlier slot's resolved target.

    Models pronoun anaphora across an ``EffectSequence`` — the
    second and later effects of a "deal damage to target creature.
    Tap that creature." sequence reference the first effect's
    target via ``SlotRef(0)`` rather than picking afresh. The
    targeting pipeline does NOT prompt for a ``SlotRef`` — it
    inherits the parent slot's resolved object at resolve time.

    A ``SlotRef``'s legality is its parent's legality: if the
    parent slot's target became illegal between cast and resolve,
    every effect with a ``SlotRef`` to that slot skips (CR
    608.2b).
    """

    idx: int


# Public alias for the per-effect target list element.
TargetSlot = Union[TargetSpec, SlotRef]


# Convenience constant: the "any target" target-filter expansion. CR
# 115.4 — for damage and similar effects whose printed phrase is "any
# target", the legal kinds are creature / planeswalker / battle /
# player.
def any_target_filter() -> TargetFilter:
    """Return the ``TargetFilter`` for printed "any target".

    Centralized here so every consumer ("deals N damage to any
    target", a future "exiles any target",  etc.) agrees on the
    expansion. Returns a fresh ``OneOf`` each call; instances are
    structurally equal so callers can compare against
    ``any_target_filter()`` in tests.
    """
    from mtgz_ir.effects.object_predicates import (
        IsCreature, IsPlayer,
    )
    from mtgz_ir.effects.object_predicates import (
        IsPlaneswalker, IsBattle,
    )
    from mtgz_ir.effects.target_filter import OneOf

    return OneOf(IsCreature(), IsPlaneswalker(), IsBattle(), IsPlayer())


__all__ = [
    "TargetSpec",
    "SlotRef",
    "TargetSlot",
    "any_target_filter",
]
