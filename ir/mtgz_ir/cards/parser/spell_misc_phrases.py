"""Phrase recognizers for token / copy modes of cube spells (engine
side: :mod:`mtgz_ir.effects.generic.spell_misc`,
:mod:`mtgz_ir.effects.generic.spell_copies`).

- ``create a token that's a copy of target <permanent np>``
- Chain Lightning's "deals N damage to any target. Then that player or
  that permanent's controller may pay {cost}. ... copy this spell"
- ``whenever <event> this turn, <effect>`` (a delayed triggered
  ability, parsed through the unified trigger grammar)
- ``choose target <np>. You get {E}.., then you may pay any amount of
  {E}. <Name> deals that much damage to that permanent``

(Discard / reveal-hand modes use the substrate's card-flow grammar.)
"""

from __future__ import annotations

import re

from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.generic.spell_misc import CreateTokenCopyOfTargetPermanent


_TOKEN_COPY_RE = re.compile(
    r"^create a token that's a copy of target (?P<np>.+)$"
)


def parse_token_copy_of_target(phrase: str) -> Effect | None:
    m = _TOKEN_COPY_RE.match(phrase)
    if m is None:
        return None
    f = parse_permanent_phrase(m.group("np"))
    if f is None:
        return None
    return CreateTokenCopyOfTargetPermanent(target_filter=f, label=m.group("np"))


_CHAIN_COPY_RE = re.compile(
    r"^deals (?P<n>\d+) damage to any target\. Then that player or that "
    r"permanent's controller may pay (?P<cost>(?:\{[^}]+\})+)\. If the player "
    r"does, they may copy this spell and may choose a new target for that "
    r"copy$"
)


def parse_chain_copy_damage(phrase: str) -> Effect | None:
    """Chain Lightning's "deals N damage to any target. Then that player
    or that permanent's controller may pay {cost}. If the player does,
    they may copy this spell and may choose a new target for that
    copy."."""
    from mtgz_ir.effects.generic.spell_copies import DealDamageThenChainCopy
    from mtgz_ir.effects.targets import TargetSpec, any_target_filter

    m = _CHAIN_COPY_RE.match(phrase)
    if m is None:
        return None
    return DealDamageThenChainCopy(
        amount=int(m.group("n")),
        targets=(TargetSpec(any_target_filter()),),
        chain_cost=m.group("cost"),
    )


_THIS_TURN_TRIGGER_RE = re.compile(
    r"^[Ww]henever (?P<event>.+?) this turn, (?P<body>.+)$"
)


def parse_this_turn_trigger(phrase: str) -> Effect | None:
    """``whenever <event> this turn, <effect>`` — the trigger condition
    and body parse through the unified triggered-ability grammar
    (``Whenever <event>, <effect>.``)."""
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    from mtgz_ir.effects.generic.spell_misc import (
        CreateThisTurnTriggeredAbility,
    )

    m = _THIS_TURN_TRIGGER_RE.match(phrase)
    if m is None:
        return None
    parsed = _parse_triggered_ability_line(
        f"Whenever {m.group('event')}, {m.group('body')}.",
    )
    if parsed is None:
        return None
    triggers = parsed if isinstance(parsed, tuple) else (parsed,)
    return CreateThisTurnTriggeredAbility(triggered_abilities=triggers)


_ENERGY_DAMAGE_RE = re.compile(
    r"^choose target (?P<np>.+?)\. You get (?P<energy>(?:\{E\})+), then you "
    r"may pay any amount of \{E\}\. .+? deals that much damage to that "
    r"(?:permanent|creature|planeswalker)$"
)


def parse_energy_paid_damage(phrase: str) -> Effect | None:
    """Galvanic Discharge: "Choose target <np>. You get {E}{E}{E}, then
    you may pay any amount of {E}. <Name> deals that much damage to that
    permanent." (reminder text already stripped)."""
    from mtgz_ir.effects.generic.spell_damage import DealDamagePaidInEnergy
    from mtgz_ir.effects.targets import TargetSpec

    m = _ENERGY_DAMAGE_RE.match(phrase)
    if m is None:
        return None
    f = parse_permanent_phrase(m.group("np"))
    if f is None:
        return None
    return DealDamagePaidInEnergy(
        targets=(TargetSpec(f),),
        energy=m.group("energy").count("{E}"),
    )


__all__ = [
    "parse_energy_paid_damage",
    "parse_this_turn_trigger",
    "parse_chain_copy_damage",
    "parse_token_copy_of_target",
]
