"""Spell cost lines: printed alternative costs and one-of additional
costs on instants / sorceries.

Recognized lines (each is its own oracle line)::

    [If <gate>, ]you may <alt cost> rather than pay this spell's mana cost.
        gate     := it's your turn | it's not your turn
                  | you control a[n] <Subtype>
        alt cost := <piece>( and <piece>)*
        piece    := pay N life | pay {mana}
                  | exile a <color> card from your hand
                  | return a[n] <Subtype> you control to its owner's hand
                  | sacrifice a[n] <Subtype>

    As an additional cost to cast this spell, <arm>( or <arm>)*.
        arm := discard a card | discard N cards | pay N life
             | sacrifice a[n] <permanent np>

The lines contribute no ``Effect``; ``spell_builder`` drops them from
the effect-clause stream and threads the parsed costs onto the
``CardPrinting`` (``alternative_costs`` / ``additional_cost_options``).
An additional-cost line whose body is already handled by the legacy
per-body flag table in ``spell_builder`` is left to that table.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.spell_costs import AdditionalCostOption, AlternativeCost


_COLOR_LETTERS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
_NUMBER_WORDS = {"a": 1, "one": 1, "two": 2, "three": 3}

_ALT_COST_RE = re.compile(
    r"^(?:If (?P<gate>it's your turn|it's not your turn"
    r"|you control an? (?P<ctrl>[A-Z][a-z]+)), you may|You may) "
    r"(?P<cost>.+) rather than pay this spell's mana cost\.$"
)
_ALT_COST_CONDITION_RE = re.compile(
    r"^If (?P<cond>[^,]+), you may (?P<cost>.+) rather than pay this "
    r"spell's mana cost\.$"
)
# The gate-first word order: "If you control a Forest, rather than pay
# this spell's mana cost, you may have an opponent gain 3 life."
# (Invigorate).
_ALT_COST_GATE_FIRST_RE = re.compile(
    r"^If (?P<gate>it's your turn|it's not your turn"
    r"|you control an? (?P<ctrl>[A-Z][a-z]+)), rather than pay this "
    r"spell's mana cost, you may (?P<cost>.+)\.$"
)
_PAY_LIFE_RE = re.compile(r"^pay (\d+) life$")
_PAY_MANA_RE = re.compile(r"^pay ((?:\{[^}]+\})+)$")
_EXILE_COLOR_RE = re.compile(
    r"^exile an? (white|blue|black|red|green) card from your hand$"
)
# "return a[n] <Subtype> you control to its owner's hand" (Daze) /
# "return two <Subtype>s you control to their owner's hand" (Gush).
_RETURN_SUBTYPE_RE = re.compile(
    r"^return (?:an? (?P<one>[A-Z][a-z]+) you control to its owner's hand"
    r"|(?P<n>two|three) (?P<many>[A-Z][a-z]+)s you control to their "
    r"owner's hand)$"
)
# "sacrifice a[n] <Subtype>" (Mine Collapse) / "sacrifice two
# <Subtype>s" (Fireblast).
_SACRIFICE_SUBTYPE_RE = re.compile(
    r"^sacrifice (?:an? (?P<one>[A-Z][a-z]+)|(?P<n>two|three) "
    r"(?P<many>[A-Z][a-z]+)s)$"
)
# "have an opponent gain N life" (Invigorate).
_OPPONENT_GAINS_LIFE_RE = re.compile(r"^have an opponent gain (\d+) life$")
_ALT_COUNT_WORDS = {"two": 2, "three": 3}


def _subtype_and_count(m: re.Match) -> tuple[str, int]:
    if m.group("one") is not None:
        return m.group("one"), 1
    return m.group("many"), _ALT_COUNT_WORDS[m.group("n")]


_OVERLOAD_RE = re.compile(r"^Overload (?P<cost>(?:\{[^}]+\})+)$")


def parse_alternative_cost_line(line: str) -> AlternativeCost | None:
    line = _strip_reminder_text(line.strip())
    m = _OVERLOAD_RE.match(line)
    if m is not None:
        return AlternativeCost(mana=m.group("cost"), overload=True)
    m = _ALT_COST_RE.match(line) or _ALT_COST_GATE_FIRST_RE.match(line)
    fields: dict = {}
    if m is None:
        # Roto P1: a game-state gate ("If an opponent cast three or more
        # spells this turn, ..." -- Mindbreak Trap).
        m = _ALT_COST_CONDITION_RE.match(line)
        if m is None:
            return None
        from mtgz_ir.cards.parser.roto_stack import roto_alt_cost_condition

        condition = roto_alt_cost_condition(m.group("cond"))
        if condition is None:
            return None
        fields["condition"] = condition
    gate = m.groupdict().get("gate")
    if gate == "it's your turn":
        fields["turn"] = "yours"
    elif gate == "it's not your turn":
        fields["turn"] = "not_yours"
    elif gate is not None:
        fields["requires_control_subtype"] = m.group("ctrl")
    for piece in m.group("cost").split(" and "):
        if (pm := _PAY_LIFE_RE.match(piece)) and "pay_life" not in fields:
            fields["pay_life"] = int(pm.group(1))
        elif (pm := _PAY_MANA_RE.match(piece)) and "mana" not in fields:
            fields["mana"] = pm.group(1)
        elif (pm := _EXILE_COLOR_RE.match(piece)) and "exile_card_color" not in fields:
            fields["exile_card_color"] = _COLOR_LETTERS[pm.group(1)]
        elif (pm := _RETURN_SUBTYPE_RE.match(piece)) and "return_subtype" not in fields:
            fields["return_subtype"], n = _subtype_and_count(pm)
            if n > 1:
                fields["return_count"] = n
        elif (pm := _SACRIFICE_SUBTYPE_RE.match(piece)) and (
            "sacrifice_subtype" not in fields
        ):
            fields["sacrifice_subtype"], n = _subtype_and_count(pm)
            if n > 1:
                fields["sacrifice_count"] = n
        elif (pm := _OPPONENT_GAINS_LIFE_RE.match(piece)) and (
            "opponent_gains_life" not in fields
        ):
            fields["opponent_gains_life"] = int(pm.group(1))
        else:
            return None
    return AlternativeCost(**fields)


_ADDITIONAL_COST_RE = re.compile(
    r"^As an additional cost to cast this spell, (?P<body>.+)\.$"
)
_DISCARD_RE = re.compile(r"^discard (a|one|two|three) cards?$")
_SACRIFICE_NP_RE = re.compile(r"^sacrifice an? (?P<np>.+)$")


def _parse_arm(arm: str) -> AdditionalCostOption | None:
    if m := _DISCARD_RE.match(arm):
        return AdditionalCostOption(kind="discard", amount=_NUMBER_WORDS[m.group(1)])
    if m := _PAY_LIFE_RE.match(arm):
        return AdditionalCostOption(kind="pay_life", amount=int(m.group(1)))
    if m := _SACRIFICE_NP_RE.match(arm):
        f = parse_permanent_phrase(m.group("np"))
        if f is None:
            return None
        return AdditionalCostOption(
            kind="sacrifice", permanent_filter=f, label=m.group("np"),
        )
    return None


def parse_additional_cost_line(
    line: str,
) -> tuple[AdditionalCostOption, ...] | None:
    m = _ADDITIONAL_COST_RE.match(line.strip())
    if m is None:
        return None
    body = m.group("body")
    # A single arm ("sacrifice a creature", "pay X life") is the casting
    # substrate's (spell_builder._substrate_additional_cost_fields); only
    # the one-of shape "A or B" is modeled here.
    if _parse_arm(body) is not None:
        return None
    arms = []
    for piece in body.split(" or "):
        arm = _parse_arm(piece)
        if arm is None:
            return None
        arms.append(arm)
    return tuple(arms) if len(arms) > 1 else None


# "This spell can't be countered." (CR 101.2) -> the printing's
# ``spell_is_uncounterable`` flag, which every counter effect honors.
UNCOUNTERABLE_LINE = "This spell can't be countered."


def _is_uncounterable_line(line: str) -> bool:
    """``This spell can't be countered.`` -- reminder text stripped (Long
    Goodbye's "(This includes by the ward ability.)")."""
    return _strip_reminder_text(line.strip()).strip() == UNCOUNTERABLE_LINE


_REPLICATE_RE = re.compile(r"^Replicate (?P<cost>(?:\{[^}]+\})+)$")


def parse_replicate_line(line: str) -> str | None:
    """``Replicate {cost}`` (reminder text stripped) -> the cost."""
    m = _REPLICATE_RE.match(_strip_reminder_text(line.strip()))
    return m.group("cost") if m else None


_TEAMWORK_RE = re.compile(r"^Teamwork (?P<n>\d+)$")


def parse_teamwork_line(line: str) -> int | None:
    """``Teamwork N`` (reminder text stripped) -> N."""
    m = _TEAMWORK_RE.match(_strip_reminder_text(line.strip()))
    return int(m.group("n")) if m else None


def is_spell_cost_line(line: str) -> bool:
    """Whether ``line`` is a spell-printing line (alternative cost incl.
    Overload, additional cost, or "This spell can't be countered.")
    that contributes no ``Effect``."""
    return (
        _is_uncounterable_line(line)
        or parse_replicate_line(line) is not None
        or parse_teamwork_line(line) is not None
        or parse_alternative_cost_line(line) is not None
        or parse_additional_cost_line(line) is not None
    )


def detect_spell_costs(
    text: str, *, legacy_line=lambda line: False,
) -> tuple[tuple[AlternativeCost, ...], tuple[AdditionalCostOption, ...]]:
    """Every alternative cost and the additional-cost arms printed in
    ``text`` (one oracle line each). Lines for which ``legacy_line``
    is true are owned by ``spell_builder``'s legacy flag table and
    skipped."""
    alts: list[AlternativeCost] = []
    arms: tuple[AdditionalCostOption, ...] = ()
    for raw in text.split("\n"):
        line = raw.strip()
        if legacy_line(line):
            continue
        alt = parse_alternative_cost_line(line)
        if alt is not None:
            alts.append(alt)
            continue
        parsed = parse_additional_cost_line(line)
        if parsed is not None:
            arms = arms + parsed
    return tuple(alts), arms


def detect_spell_line_fields(
    text: str, *, legacy_line=lambda line: False,
) -> dict:
    """``CardPrinting`` field updates implied by the spell-printing
    lines in ``text`` (empty dict when there are none)."""
    alts, arms = detect_spell_costs(text, legacy_line=legacy_line)
    fields: dict = {}
    if alts:
        fields["alternative_costs"] = alts
    if arms:
        fields["additional_cost_options"] = arms
    if any(_is_uncounterable_line(raw) for raw in text.split("\n")):
        fields["spell_is_uncounterable"] = True
    for raw in text.split("\n"):
        rep = parse_replicate_line(raw)
        if rep is not None:
            fields["replicate_cost"] = rep
        teamwork = parse_teamwork_line(raw)
        if teamwork is not None:
            fields["teamwork_power"] = teamwork
    return fields


_TARGET_WORD_RE = re.compile(r"\btarget\b")


def apply_overload(effect, text: str, name: str, parse_text):
    """If ``text`` prints an Overload cost, wrap ``effect`` (the
    "target" version) as ``IfConditionElse(AlternativeCostWasPaid(),
    <each-version>, effect)``, parsing the each-version with
    ``parse_text`` from the effect lines with "target" rewritten to
    "each" (CR 702.96b). Returns ``None`` if the rewritten text doesn't
    parse; ``effect`` unchanged when there is no Overload line."""
    from mtgz_ir.effects.combinators import IfConditionElse
    from mtgz_ir.effects.generic.cast_conditionals import (
        AlternativeCostWasPaid,
    )

    lines = text.split("\n")
    overload = [
        alt for alt in (parse_alternative_cost_line(line) for line in lines)
        if alt is not None and alt.overload
    ]
    if not overload:
        return effect
    effect_lines = [line for line in lines if not is_spell_cost_line(line)]
    rewritten = _TARGET_WORD_RE.sub("each", "\n".join(effect_lines))
    overloaded = parse_text(rewritten, name)
    if overloaded is None or overloaded.requires_target():
        return None
    return IfConditionElse(
        condition=AlternativeCostWasPaid(),
        then_effect=overloaded,
        else_effect=effect,
    )


__all__ = [
    "UNCOUNTERABLE_LINE",
    "parse_replicate_line",
    "parse_teamwork_line",
    "apply_overload",
    "detect_spell_costs",
    "detect_spell_line_fields",
    "is_spell_cost_line",
    "parse_additional_cost_line",
    "parse_alternative_cost_line",
]
