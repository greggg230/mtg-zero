"""Phrasebook: printed "target …" noun phrase → ``TargetFilter``.

Each entry in :data:`_TARGET_PHRASE_TABLE` maps a printed surface
phrase (the bit *after* the ``target `` / ``any target`` keyword) to
the structural :class:`mtgz_ir.effects.target_filter.TargetFilter`
that describes the legal-target set.

The phrasebook covers every shape encountered by the damage
recognizers in :mod:`mtgz_ir.cards.parser.effects` plus the bare
"any target" form. Cards whose phrasing is uniquely "target X an
opponent controls" / "target X you control" / "target attacking or
blocking creature" all route through here so adding a new card with
the same phrasing is one entry, not a new effect class.

Public surface:

- :func:`parse_target_phrase` — return a ``TargetFilter`` for a
  recognized phrase, or ``None`` if the phrase isn't on the table.
- :data:`_TARGET_PHRASE_TABLE` — the underlying dict, exposed for
  introspection / parser tests.
"""

from __future__ import annotations

from typing import Dict

from mtgz_ir.effects.object_predicates import (
    IsAttacking,
    IsBlocking,
    IsCreature,
    IsTapped,
)
from mtgz_ir.effects.target_filter import OneOf, TargetFilter
from mtgz_ir.effects.targets import any_target_filter


# Closed phrasebook: keys are the verbatim printed surface (after the
# ``target `` keyword and lower-cased to the parser's normalized form),
# values are the matching ``TargetFilter``. Adding a new shape is one
# entry here plus tests pinning it.
#
# Controller-relative shapes ("creature an opponent controls",
# "creature you control") are NOT in this table yet — those involve
# controller-bound predicates that don't exist in PR #3. The damage
# migration that consumes this phrasebook keeps the
# controller-relative pool selection in the engine class for now;
# PR #4 / a follow-up adds the controller-aware atoms.
_TARGET_PHRASE_TABLE: Dict[str, TargetFilter] = {
    # "any target" — CR 115.4. Expands to creature / planeswalker /
    # battle / player. Used by every "deals N damage to any target"
    # printing in the EoE pool (Nebula Dragon, Infernal Phantom's
    # source-power variant, etc.).
    "any target": any_target_filter(),
    # "target creature" — Bombard, Pinnacle Kill-Ship, the bare
    # creature-only damage pool.
    "creature": IsCreature(),
    # "target tapped creature" — Reroute Systems mode 1's tapped-only
    # burn. ``IsCreature() & IsTapped()`` matches creatures that are
    # currently tapped.
    "tapped creature": IsCreature() & IsTapped(),
    # "target attacking or blocking creature" — Focus Fire's body.
    # Combines the in-combat predicates via the bitwise-or operator
    # (which produces ``ObjectOr`` under the hood, itself a member of
    # the ``TargetFilter`` union).
    "attacking or blocking creature": (
        IsCreature() & (IsAttacking() | IsBlocking())
    ),
}


def parse_target_phrase(phrase: str) -> TargetFilter | None:
    """Return a ``TargetFilter`` for a recognized printed target
    phrase, or ``None`` if the phrase isn't on the closed table.

    The caller is responsible for stripping the ``target `` keyword
    and any surrounding whitespace before calling — the table keys
    are the post-strip noun phrase. ``"any target"`` is the one
    exception: the keyword *is* the phrase, so callers pass it
    verbatim.
    """
    return _TARGET_PHRASE_TABLE.get(phrase.strip())


__all__ = [
    "_TARGET_PHRASE_TABLE",
    "parse_target_phrase",
]
