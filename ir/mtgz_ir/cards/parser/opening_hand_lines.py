"""Opening-hand action lines (CR 103.6) -> ``BeginGameOnBattlefield``.

One parameterized recognizer for the printed shape::

    If this card is in your opening hand[ and you're not the starting
    player], you may begin the game with <it | <card name>> on the
    battlefield[ with a/an <kind> counter on it].[ If you do, exile
    a/one/two card(s) from your hand.]

Gemstone Caverns uses every optional part; the Leyline cycle's "If this
card is in your opening hand, you may begin the game with it on the
battlefield." is the bare form (Leyline of Mutation, EoE-era pool, still
goes through the enchantment builder's honest-no-op list -- see
``docs/SEARCH_LIMITATIONS.md``; routing it here is a one-line change
whenever that pool's parses may move).
"""

from __future__ import annotations

import re

from mtgz_ir.opening_hand import BeginGameOnBattlefield

_OPENING_HAND_RE = re.compile(
    r"^If this card is in your opening hand"
    r"(?P<not_starting> and you're not the starting player)?, "
    r"you may begin the game with (?P<self>.+?) on the battlefield"
    r"(?: with an? (?P<kind>[a-z]+) counter on it)?\."
    r"(?: If you do, exile (?P<n>a|one|two|three) cards? from your hand\.)?$"
)
_NUMBER_WORDS = {"a": 1, "one": 1, "two": 2, "three": 3}


def parse_opening_hand_line(
    line: str, card_name: str,
) -> BeginGameOnBattlefield | None:
    """``BeginGameOnBattlefield`` for an opening-hand line naming this
    card ("it" or its own name), else ``None``."""
    m = _OPENING_HAND_RE.match(line.strip())
    if m is None or m.group("self") not in ("it", card_name):
        return None
    kind = m.group("kind")
    n = m.group("n")
    return BeginGameOnBattlefield(
        only_if_not_starting_player=m.group("not_starting") is not None,
        counters=((kind, 1),) if kind else (),
        exile_from_hand=_NUMBER_WORDS[n] if n else 0,
    )


__all__ = ["parse_opening_hand_line"]
