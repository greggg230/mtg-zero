"""Multi-mode spell shapes -> :class:`ChooseModes`.

Recognized clause lists (after reminder-text stripping):

    Choose two —                       (exactly two different modes)
    Choose three —
    Choose one or more —               (any number, at least one)
    Choose three. You may choose the same mode more than once.
                                       (exactly three, repeats allowed;
                                        also accepted split into two
                                        clauses by the sentence splitter)
    • <mode>
    • <mode> ...

    Escalate—Discard a card.           (leading line; "Choose one or more")
    Choose one or more —
    • <mode> ...

    Spree
    + {cost} — <mode>
    + {cost} — <mode> ...

Each mode sentence goes through the unified spell-effect grammar; every
mode must parse and take zero or exactly one target, otherwise the
spell stays unclaimed.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect


_NUMBER_WORDS = {"one": 1, "two": 2, "three": 3, "four": 4}
_HEADER_RE = re.compile(
    r"^Choose (?P<n>one|two|three|four)(?P<more> or more)? —$"
)
_REPEAT_HEADER_RE = re.compile(
    r"^Choose (?P<n>two|three|four)\. You may choose the same mode more "
    r"than once\.$"
)
_REPEAT_TAIL = "You may choose the same mode more than once."
_BULLET_RE = re.compile(r"^• (?P<text>.+)$")
_ESCALATE_DISCARD_RE = re.compile(r"^Escalate—Discard a card\.?$")
_SPREE_MODE_RE = re.compile(
    r"^\+ (?P<cost>(?:\{[^}]+\})+) — (?P<text>.+)$"
)


def _parse_modes(lines: list[str], name: str, pattern) -> list | None:
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

    out = []
    for line in lines:
        m = pattern.match(line)
        if m is None:
            return None
        eff = _parse_spell_effect_phrase(m.group("text"), name)
        if eff is None:
            return None
        out.append((m, eff))
    return out


def _build(**kwargs) -> Effect | None:
    from mtgz_ir.effects.generic.multi_modal import ChooseModes

    try:
        return ChooseModes(**kwargs)
    except ValueError:
        return None


def parse_multi_modal(clauses: list[str], name: str) -> Effect | None:
    if not clauses:
        return None
    if clauses[0] == "Spree":
        parsed = _parse_modes(clauses[1:], name, _SPREE_MODE_RE)
        if not parsed or len(parsed) < 2:
            return None
        return _build(
            modes=tuple(e for _, e in parsed),
            min_modes=1,
            max_modes=len(parsed),
            mode_mana_costs=tuple(m.group("cost") for m, _ in parsed),
            header="Spree",
        )
    escalate = False
    if _ESCALATE_DISCARD_RE.match(clauses[0]):
        escalate = True
        clauses = clauses[1:]
        if not clauses:
            return None
    repeats = False
    m = _HEADER_RE.match(clauses[0])
    body = clauses[1:]
    if m is None:
        m = _REPEAT_HEADER_RE.match(clauses[0])
        if m is None and len(clauses) > 1 and clauses[1] == _REPEAT_TAIL:
            m = re.match(r"^Choose (?P<n>two|three|four)\.$", clauses[0])
            body = clauses[2:]
        if m is None:
            return None
        repeats = True
    n = _NUMBER_WORDS[m.group("n")]
    more = bool(m.groupdict().get("more"))
    if n == 1 and not more:
        return None  # "Choose one —" is ModalChooseOne's shape.
    if escalate and not (n == 1 and more):
        return None
    parsed = _parse_modes(body, name, _BULLET_RE)
    if not parsed or len(parsed) < 2:
        return None
    modes = tuple(e for _, e in parsed)
    max_modes = len(modes) if more else n
    if not repeats and max_modes > len(modes):
        return None
    header = clauses[0].rstrip(" —")
    return _build(
        modes=modes,
        min_modes=n,
        max_modes=max_modes,
        repeats=repeats,
        escalate_discard=escalate,
        header=header,
    )


__all__ = ["parse_multi_modal"]
