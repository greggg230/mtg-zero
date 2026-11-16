"""Oracle-text preprocessing helpers.

Right now this module exposes a single helper, ``_strip_reminder_text``,
which removes parenthesized reminder-text clauses from a single oracle
line so the downstream clause grammars (keyword / triggered / activated
/ static / Warp / Ward / Devour) don't have to know about reminder
wording.

Reminder text is the parenthesized flavor printed alongside keywords
and uncommon mechanics — e.g. ``"Flying (This creature can't be
blocked except by creatures with flying or reach.)"`` or
``"surveil 1. (Look at the top card of your library. ...)"``.
It carries no rules consequence and is safe to drop before clause
parsing.

The land builder intentionally parses basic-land reminder text
(``"({T}: Add {W}.)"``) as the land's actual oracle text — the
helper is not used there. It's only called from the creature and
spell builders.
"""

from __future__ import annotations

import re


# Match an optional leading whitespace run + a parenthesized substring
# that contains no inner parens. Reminder text never nests parens, so
# a paren-balanced regex is unnecessary; the simple ``[^()]*`` body is
# enough and stays linear in line length.
#
# ``{`` and ``}`` are not matched — mana-cost notation like ``{1}{R}``
# uses braces, not parens, and is preserved verbatim. The Scryfall
# basic-land oracle text ``"({T}: Add {W}.)"`` IS a parenthesized
# clause, but the land builder doesn't call this helper, so it stays
# intact for the basic-land grammar.
_REMINDER_CLAUSE_RE = re.compile(r"\s*\([^()]*\)")


def _strip_reminder_text(line: str) -> str:
    """Remove every parenthesized reminder-text clause from ``line``.

    Each match is the longest run of non-paren characters wrapped in
    a single ``(...)`` pair, optionally preceded by whitespace. The
    helper applies the regex repeatedly until the line is stable, so
    a line with multiple reminder clauses is fully cleaned in one
    call.

    Returns the resulting line with surrounding whitespace stripped.
    Lines that contain no parentheses round-trip unchanged (modulo
    leading/trailing whitespace).
    """
    out = line
    while True:
        new = _REMINDER_CLAUSE_RE.sub("", out)
        if new == out:
            break
        out = new
    return out.strip()
