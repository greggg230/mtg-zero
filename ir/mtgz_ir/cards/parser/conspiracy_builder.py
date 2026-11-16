"""Conspiracy builder (CR 313) -- roto package P6.

A conspiracy card has no mana cost and is never cast: it starts the game
face up in the command zone (the parenthesized "Start the game with this
conspiracy face up in the command zone." is reminder text, stripped like
any other) and its static abilities function from there. Every remaining
line must be a conspiracy rule the engine models
(``roto_keywords.parse_conspiracy_rule_line``); hidden agenda and
draft-matters conspiracies are left unclaimed.
"""

from __future__ import annotations

from mtgz_ir.cards.base import CardPrinting, parse_mana_cost
from mtgz_ir.cards.parser.roto_keywords import parse_conspiracy_rule_line
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line


def _build_conspiracy_printing(card: dict) -> CardPrinting | None:
    supertypes, card_types, subtypes = _parse_type_line(card.get("type_line", ""))
    if card_types != ("Conspiracy",) or card.get("card_faces"):
        return None
    rules = []
    for raw in (card.get("oracle_text") or "").split("\n"):
        line = _strip_reminder_text(raw)
        if not line:
            continue
        rule = parse_conspiracy_rule_line(line)
        if rule is None:
            return None
        rules.append(rule)
    if not rules:
        return None
    return CardPrinting(
        name=card["name"],
        mana_cost_str=card.get("mana_cost", ""),
        mana_cost=parse_mana_cost(card.get("mana_cost", "")),
        type_line=card.get("type_line", ""),
        supertypes=tuple(supertypes),
        card_types=tuple(card_types),
        subtypes=tuple(subtypes),
        conspiracy_rules=tuple(rules),
    )


__all__ = ["_build_conspiracy_printing"]
