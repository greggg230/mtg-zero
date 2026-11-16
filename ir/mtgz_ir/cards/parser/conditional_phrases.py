"""Conditional spell riders: "If <condition>, <effect>" and the
"<modified effect> instead if <condition>" riders that replace a base
clause's effect (CR 614-style "instead" text on a one-shot effect).

Shapes (normalized phrases):

- ``if the {mana} cost was paid, <effect>`` -> ``IfConditionElse(
  AlternativeCostWasPaid(), <effect>)`` (Baleful Mastery).
- ``an opponent draws a card`` / ``an opponent draws N cards``.
- Instead riders on a preceding base clause, either as a second
  sentence on the same line (Burst Lightning: "... any target. If this
  spell was kicked, it deals 4 damage instead.") or as the next oracle
  line, optionally behind an ability word (Unholy Heat: "Delirium —
  Unholy Heat deals 6 damage instead if ..."; Fatal Push: "Revolt —
  Destroy that creature if it has mana value 4 or less instead if
  ..."):

      if <condition>, [it ]<variant> instead
      [it ]<variant> instead if <condition>

  where ``<variant>`` restates the base effect with one parameter
  changed: ``deals N damage`` (damage amount) or ``destroy|exile that
  creature|permanent [if it has mana value N or less]`` (the removal's
  resolution condition). The result is ``IfConditionElse(condition,
  variant, base)`` sharing the base's single target.

Conditions: "this spell was kicked", "there are four or more card types
among cards in your graveyard" (Delirium), "a permanent left the
battlefield under your control this turn" (Revolt).
"""

from __future__ import annotations

import dataclasses
import re

from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.conditions import Condition, DeliriumActive
from mtgz_ir.effects.combinators import IfConditionElse
from mtgz_ir.effects.generic.cast_conditionals import (
    AlternativeCostWasPaid,
    AnOpponentDrawsCards,
    OptionalAdditionalCostPaid,
    PermanentLeftYourControlThisTurn,
    SpellWasKicked,
)


_CONDITIONS: dict[str, Condition] = {
    "this spell was kicked": SpellWasKicked(),
    "there are four or more card types among cards in your graveyard": (
        DeliriumActive()
    ),
    "a permanent left the battlefield under your control this turn": (
        PermanentLeftYourControlThisTurn()
    ),
    "this spell was cast using teamwork": OptionalAdditionalCostPaid(),
}

_NUMBER_WORDS = {"a": 1, "one": 1, "two": 2, "three": 3}


_COST_PAID_RE = re.compile(
    r"^if the (?:\{[^}]+\})+ cost was paid, (?P<eff>.+)$"
)
_OPPONENT_DRAWS_RE = re.compile(
    r"^an opponent draws (?:(?P<a>a) card|(?P<n>one|two|three) cards)$"
)


def parse_cost_paid_conditional(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects import _parse_effect

    m = _COST_PAID_RE.match(phrase)
    if m is None:
        return None
    inner = _parse_effect(m.group("eff"))
    if inner is None or inner.requires_target():
        return None
    return IfConditionElse(
        condition=AlternativeCostWasPaid(), then_effect=inner,
    )


def parse_an_opponent_draws(phrase: str) -> Effect | None:
    m = _OPPONENT_DRAWS_RE.match(phrase)
    if m is None:
        return None
    n = 1 if m.group("a") else _NUMBER_WORDS[m.group("n")]
    return AnOpponentDrawsCards(amount=n)


# ---------- instead riders ----------

_ABILITY_WORD_PREFIX = re.compile(r"^[A-Z][a-z]+ — ")
_IF_THEN_INSTEAD_RE = re.compile(
    r"^if (?P<cond>.+?), (?:it )?(?P<eff>.+) instead$"
)
_INSTEAD_IF_RE = re.compile(r"^(?:it )?(?P<eff>.+) instead if (?P<cond>.+)$")
_DAMAGE_VARIANT_RE = re.compile(r"^deals (?P<n>\d+) damage$")
_COUNTER_TAX_VARIANT_RE = re.compile(
    r"^counter that spell unless its controller pays (?P<cost>(?:\{[^}]+\})+)$"
)
_REMOVAL_VARIANT_RE = re.compile(
    r"^(?P<verb>destroy|exile) that (?:creature|permanent)"
    r"(?: if it has mana value (?P<mv>\d+) or less)?$"
)


def _normalize_rider(text: str, name: str) -> str:
    text = text.strip()
    text = _ABILITY_WORD_PREFIX.sub("", text)
    if text.endswith("."):
        text = text[:-1]
    if text.startswith(name + " "):
        text = text[len(name) + 1:]
    if text and text[0].isupper():
        text = text[0].lower() + text[1:]
    return text


def _variant(eff_text: str, base: Effect) -> Effect | None:
    from mtgz_ir.effects.generic.damage import DealDamage
    from mtgz_ir.effects.generic.targeted_removal import RemoveTargetPermanents
    from mtgz_ir.effects.value_predicates import HasManaValueAtMost

    from mtgz_ir.effects.generic.spell_damage import DealXDamage

    m = _DAMAGE_VARIANT_RE.match(eff_text)
    if (
        m is not None
        and isinstance(base, DealDamage)
        and not isinstance(base, DealXDamage)
    ):
        return dataclasses.replace(base, amount=int(m.group("n")))
    from mtgz_ir.effects.generic.counterspells import CounterSpell

    m = _COUNTER_TAX_VARIANT_RE.match(eff_text)
    if m is not None and isinstance(base, CounterSpell) and base.unless_pays:
        return dataclasses.replace(base, unless_pays=m.group("cost"))
    # Roto P1: "counter that spell instead" drops the base's tax
    # (Stubborn Denial's Ferocious line).
    from mtgz_ir.cards.parser.roto_stack import roto_counter_variant

    variant = roto_counter_variant(eff_text, base)
    if variant is not None:
        return variant
    m = _REMOVAL_VARIANT_RE.match(eff_text)
    if (
        m is not None
        and isinstance(base, RemoveTargetPermanents)
        and base.action == m.group("verb")
    ):
        mv = m.group("mv")
        only_if = HasManaValueAtMost(int(mv)) if mv else None
        return dataclasses.replace(base, only_if=only_if)
    # A restated removal with a different target ("If this spell was
    # kicked, exile target nonland permanent instead." — Tear Asunder).
    # Only valid for cast-time conditions, where the target shape is
    # known when targets are chosen (``IfConditionElse.cast_time_shape``).
    # The restatement must be the same kind of targeted removal (same
    # engine class, and the same action for RemoveTargetPermanents).
    if base.requires_target() and eff_text.split(" ", 1)[0] in (
        "destroy", "exile",
    ):
        from mtgz_ir.cards.parser.effects import _parse_effect

        other = _parse_effect(eff_text)
        if type(other) is type(base) and getattr(
            other, "action", None,
        ) == getattr(base, "action", None):
            return other
    return None


def parse_instead_rider(rider: str, base: Effect, name: str = "") -> Effect | None:
    """Combine ``base`` with an "instead" rider sentence, or ``None``."""
    text = _normalize_rider(rider, name) if name else rider
    m = _IF_THEN_INSTEAD_RE.match(text)
    if m is None:
        m = _INSTEAD_IF_RE.match(text)
    if m is None:
        return None
    condition = _CONDITIONS.get(m.group("cond"))
    if condition is None:
        # Roto P1 conditions ("you control a creature with power 4 or
        # greater").
        from mtgz_ir.cards.parser.roto_stack import roto_rider_condition

        condition = roto_rider_condition(m.group("cond"))
    if condition is None:
        return None
    variant = _variant(m.group("eff"), base)
    if variant is None:
        return None
    if variant.targets != base.targets and not isinstance(
        condition, SpellWasKicked,
    ):
        # A different target set only works when the branch is known at
        # cast time.
        return None
    return IfConditionElse(
        condition=condition, then_effect=variant, else_effect=base,
    )


def parse_effect_with_instead_rider(phrase: str) -> Effect | None:
    """Single-line form: ``<base sentence>. <instead rider>``."""
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.cards.parser.removal_phrases import split_sentences

    sentences = split_sentences(phrase)
    if len(sentences) != 2:
        return None
    base = _parse_effect(sentences[0])
    if base is None:
        return None
    return parse_instead_rider(sentences[1], base)


def combine_instead_rider(clauses: list[str], name: str) -> Effect | None:
    """Two-line form (spell builder): a base clause followed by an
    instead-rider line."""
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    if len(clauses) != 2:
        return None
    base = _parse_spell_effect_phrase(clauses[0], name)
    if base is None:
        return None
    return parse_instead_rider(clauses[1], base, name)


__all__ = [
    "combine_instead_rider",
    "parse_an_opponent_draws",
    "parse_cost_paid_conditional",
    "parse_effect_with_instead_rider",
    "parse_instead_rider",
]
