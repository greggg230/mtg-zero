"""Dispatch for the compositional ("precise") phrase grammar and its
interaction with the legacy per-shape grammar in ``effects.py``.

``effects._parse_effect`` consults this module twice:

1. :func:`parse_precise_override` runs *before* the legacy grammar,
   but only for the printed surfaces listed in
   :data:`_LEGACY_LOSSY_SURFACES` — shapes the legacy grammar accepts
   by dropping part of the printed text (a planeswalker target arm, an
   "unless its controller pays" tax). For those, the faithful
   compositional parse must win.
2. :func:`parse_precise_fallback` runs *after* the legacy grammar
   found nothing, so every phrase the legacy grammar already models
   faithfully keeps its existing engine class (and its Rust-bridge /
   parity identity) and the compositional grammar only claims new
   ground.

Rust-parity pin
---------------
A handful of Edge-of-Eternities-era spells were claimed through the
lossy legacy surfaces and are pinned by cross-engine parity tests to
the simplified engine classes the Rust engine mirrors today. Parsing
them faithfully would silently desynchronize the two engines, so
:func:`legacy_parity_pinned` disables this module entirely while one
of :data:`RUST_PARITY_PINNED_SPELLS` is being built. Every other card
(including the PowerLSV cube cards that share the same printed
surfaces) parses faithfully. Removing a name from the set is the
switch to flip once the Rust engine models the full printed text.
See ``docs/SEARCH_LIMITATIONS.md`` ("Rust-parity-pinned lossy legacy
spells").
"""

from __future__ import annotations

import contextlib
import contextvars
import re
from collections.abc import Callable, Iterator

from mtgz_ir.effects.base import Effect


RUST_PARITY_PINNED_SPELLS: frozenset[str] = frozenset({
    # Divert Disaster and Archenemy's Charm were pinned here until
    # feature/eoe-ir-fixes (mtg-zero docs/ir_defects.md 43, 62): they
    # parse faithfully now, and their cross-engine parity probes are
    # strict xfails (docs/SEARCH_LIMITATIONS.md, "Cross-engine parity:
    # EoE printings fixed in feature/eoe-ir-fixes").
    # "Counter target spell unless its controller pays {2}. If they
    # do, surveil 2." -> bare CounterTargetSpell().
    "Don't Make a Sound",
    # "... deals 3 damage to target creature or planeswalker. If that
    # creature or planeswalker would die this turn, exile it instead."
    # -> DealDamageToTargetCreature(3) (creature-only, rider dropped).
    "Scorching Dragonfire",
    # "Choose one — • Exile target artifact. • Exile target
    # enchantment. • Manifest dread." -> ManifestDread() via the
    # single-parseable-mode fallback (spell_builder's
    # _LEGACY_PARTIAL_MODAL_CLAIMS).
    "Break Down the Door",
})


_PINNED: contextvars.ContextVar[bool] = contextvars.ContextVar(
    "precise_grammar_pinned", default=False,
)


@contextlib.contextmanager
def legacy_parity_pinned(card_name: str) -> Iterator[None]:
    """Disable the precise grammar while parsing ``card_name`` if it is
    one of :data:`RUST_PARITY_PINNED_SPELLS`; no-op otherwise."""
    if card_name not in RUST_PARITY_PINNED_SPELLS:
        yield
        return
    token = _PINNED.set(True)
    try:
        yield
    finally:
        _PINNED.reset(token)


def precise_grammar_active() -> bool:
    return not _PINNED.get()


# Printed surfaces the legacy grammar accepts lossily. Matched against
# the whole normalized phrase (single sentence).
_LEGACY_LOSSY_SURFACES: tuple[re.Pattern[str], ...] = (
    re.compile(r"^exile target creature or planeswalker$"),
    re.compile(r"^exile target creature or Spacecraft$"),
    re.compile(r"^(?:it )?deals \d+ damage to target creature or planeswalker$"),
    re.compile(r"^counter target spell unless its controller pays \{[^}]+\}$"),
)


def _recognizers() -> tuple[Callable[[str], Effect | None], ...]:
    from mtgz_ir.cards.parser.conditional_phrases import (
        parse_an_opponent_draws,
        parse_cost_paid_conditional,
        parse_effect_with_instead_rider,
    )
    from mtgz_ir.cards.parser.counter_phrases import parse_counter
    from mtgz_ir.cards.parser.enchantment_phrases import (
        parse_enchantment_phrase,
    )
    from mtgz_ir.cards.parser.removal_phrases import (
        parse_damage_to_target,
        parse_destroy_all,
        parse_each_opponent_sacrifices,
        parse_targeted_removal,
    )
    from mtgz_ir.cards.parser.spell_misc_phrases import (
        parse_chain_copy_damage,
        parse_energy_paid_damage,
        parse_this_turn_trigger,
        parse_token_copy_of_target,
    )

    return (
        parse_targeted_removal,
        parse_destroy_all,
        parse_damage_to_target,
        parse_each_opponent_sacrifices,
        parse_counter,
        parse_cost_paid_conditional,
        parse_an_opponent_draws,
        parse_effect_with_instead_rider,
        parse_token_copy_of_target,
        parse_chain_copy_damage,
        parse_this_turn_trigger,
        parse_energy_paid_damage,
        parse_enchantment_phrase,
    )


# Ability words (CR 207.2c) carry no rules meaning of their own; where
# the condition is spelled out after the dash, the prefix is dropped.
_ABILITY_WORD_PREFIX = re.compile(r"^(?:converge) \u2014 ")


def _dispatch(phrase: str) -> Effect | None:
    phrase = _ABILITY_WORD_PREFIX.sub("", phrase)
    for recognizer in _recognizers():
        eff = recognizer(phrase)
        if eff is not None:
            return eff
    return None


def parse_precise_override(phrase: str) -> Effect | None:
    """Faithful parse for a legacy-lossy surface, else ``None``."""
    if not precise_grammar_active():
        return None
    if not any(p.match(phrase) for p in _LEGACY_LOSSY_SURFACES):
        return None
    return _dispatch(phrase)


def parse_precise_fallback(phrase: str) -> Effect | None:
    """Compositional parse for a phrase the legacy grammar rejected."""
    if not precise_grammar_active():
        return None
    return _dispatch(phrase)


__all__ = [
    "RUST_PARITY_PINNED_SPELLS",
    "legacy_parity_pinned",
    "parse_precise_fallback",
    "parse_precise_override",
    "precise_grammar_active",
]
