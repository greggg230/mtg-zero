"""Evergreen keyword line grammar.

A keyword line is one or more evergreen keywords joined by ``", "``.
Each piece must be a recognized standalone keyword and contributes
its lowercase engine spelling to the parsed clause's ``keywords``
tuple. Adding a new evergreen keyword is a one-line addition to
``_KEYWORD_LINES``.
"""

from __future__ import annotations

import re

from mtgz_ir.keyword_rules import PROTECTION_QUALITIES, protection_keyword


_KEYWORD_LINES: dict[str, str] = {
    "Flying": "flying",
    "Trample": "trample",
    "Vigilance": "vigilance",
    "Haste": "haste",
    "Reach": "reach",
    "Deathtouch": "deathtouch",
    "Lifelink": "lifelink",
    "Defender": "defender",
    "Flash": "flash",
    "Menace": "menace",
    "Double strike": "double strike",
    "First strike": "first strike",
    "Prowess": "prowess",
    "Hexproof": "hexproof",
    "Indestructible": "indestructible",
    "Shroud": "shroud",
    "Exalted": "exalted",
    "Infect": "infect",
    "Shadow": "shadow",
    "Battle cry": "battle cry",
    "Devoid": "devoid",
    "Persist": "persist",
    # Ascend (CR 702.131) on a permanent: ``creature_statics_wg.
    # apply_ascend`` grants the city's blessing (cube W/G bucket).
    "Ascend": "ascend",
    # Landwalk (CR 702.14; roto P6): block legality in
    # ``effects.generic.roto_keywords.blocked_by_landwalk``.
    "Plainswalk": "plainswalk",
    "Islandwalk": "islandwalk",
    "Swampwalk": "swampwalk",
    "Mountainwalk": "mountainwalk",
    "Forestwalk": "forestwalk",
}


# ``Protection from <quality>`` / ``Protection from <q1> and from <q2>``
# (CR 702.16). Each named quality becomes its own engine keyword
# ``"protection from <quality>"`` (see ``mtgz_ir.keyword_rules``). A
# quality outside ``PROTECTION_QUALITIES`` (``the chosen player``,
# ``spells that are one or more colors``) doesn't match, so the card
# stays unclaimed rather than carrying a keyword nothing enforces.
_PROTECTION_PIECE = re.compile(
    r"^[Pp]rotection from "
    r"(?P<first>the chosen player|each of your opponents|[a-z]+)"
    r"(?P<rest>(?: and from [a-z]+)*)$"
)


def _parse_protection_piece(piece: str) -> tuple[str, ...] | None:
    """Engine keywords for one ``protection from ...`` piece, or None."""
    m = _PROTECTION_PIECE.match(piece)
    if m is None:
        return None
    qualities = [m.group("first")] + re.findall(
        r" and from ([a-z]+)", m.group("rest"),
    )
    if not all(q in PROTECTION_QUALITIES for q in qualities):
        return None
    return tuple(protection_keyword(q) for q in qualities)


def _parse_keyword_piece(piece: str) -> tuple[str, ...] | None:
    """Engine keywords for one comma-separated keyword-line piece
    (capitalized per ``_parse_keyword_line``'s normalization)."""
    if piece in _KEYWORD_LINES:
        return (_KEYWORD_LINES[piece],)
    return _parse_protection_piece(piece)


def _parse_granted_keyword_piece(piece: str) -> tuple[str, ...] | None:
    """Engine keywords for a lowercase granted-keyword piece as it
    appears mid-sentence (``gains hexproof``, ``has protection from
    red``). Accepts every engine spelling in ``_KEYWORD_LINES`` plus
    the protection family."""
    if piece in _KEYWORD_LINES.values():
        return (piece,)
    return _parse_protection_piece(piece)


def _parse_keyword_line(line: str) -> tuple[str, ...] | None:
    """Parse a (possibly comma-separated) keyword-only line.

    A single keyword like ``"Flying"`` returns ``("flying",)``. A
    multi-keyword line like ``"Flash, Flying"`` returns
    ``("flash", "flying")``. Returns ``None`` if any piece is not a
    recognized keyword — the caller may then try other clause
    patterns (e.g. triggered abilities).

    Scryfall's printing convention for multi-keyword lines is
    sentence-case: only the FIRST piece is capitalized, every
    subsequent piece starts lowercase (``"Flying, haste"``,
    ``"Trample, haste"``, ``"Vigilance, menace"`` etc.). We
    normalize the first letter of every piece AFTER the first to
    upper-case before the ``_KEYWORD_LINES`` lookup so the printed
    sentence-case form parses. The first piece is matched as-is
    (case-sensitive) — a lowercase single keyword like
    ``"hexproof"`` doesn't match because lowercase keyword names
    only appear mid-sentence, not as a printed keyword line.
    """
    pieces = [p.strip() for p in line.split(", ")]
    if not pieces:  # pragma: no cover - defensive; ``str.split`` always returns ≥1 piece
        return None
    normalized = [pieces[0]] + [
        p[:1].upper() + p[1:] if p else p for p in pieces[1:]
    ]
    out: list[str] = []
    for p in normalized:
        parsed = _parse_keyword_piece(p)
        if parsed is None:
            return None
        out.extend(parsed)
    return tuple(out)
