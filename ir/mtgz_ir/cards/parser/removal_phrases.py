"""Removal / burn phrase recognizers built on the compositional target
grammar (:mod:`mtgz_ir.cards.parser.target_phrases`).

Every function here takes one *normalized* effect phrase (lower-cased
first letter, no trailing period, source name stripped — the form
``_parse_effect`` sees) and returns an ``Effect`` or ``None``. They
are dispatched by :mod:`mtgz_ir.cards.parser.precise_grammar`, which
decides whether a recognizer runs before or after the legacy grammar.

Shapes:

- ``destroy|exile [count] target <np>[. <rider>]*`` ->
  :class:`RemoveTargetPermanents`, with riders
  ``it can't be regenerated`` / ``a creature destroyed this way can't
  be regenerated``, ``its controller gains life equal to its power``,
  ``its controller may search their library for a basic land card,
  put it|that card onto the battlefield tapped, then shuffle``,
  ``its controller creates N <Name> tokens``, and ``each player other
  than its controller creates a token that's a copy of it``.
- ``destroy all <plural np>[. they can't be regenerated]`` ->
  :class:`RemoveAllMatching` (``with mana value X or less`` binds X).
- ``deals N damage to [count] target <np>`` -> ``DealDamage`` over the
  composed filter (the any-target / creature-only surfaces keep their
  legacy sugar classes; this covers the rest, e.g. "creature or
  planeswalker").
"""

from __future__ import annotations

import re

from mtgz_ir.cards.parser.target_phrases import (
    parse_permanent_phrase,
    parse_target_count,
)
from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.value_predicates import (
    HasColor,
    HasManaValueAtMost,
    HasManaValueAtMostColorsSpent,
)
from mtgz_ir.effects.generic.targeted_removal import (
    ControllerCreatesTokens,
    ControllerGainsLifeEqualToPower,
    ControllerSearchesForBasicLand,
    IfYouControlledItDraw,
    OtherPlayersCreateTokenCopy,
    RemovalRider,
    RemoveAllMatching,
    RemoveTargetPermanents,
)


_SENTENCE_BOUNDARY = re.compile(r"(?<=\.) (?=[A-Z])")

_NUMBER_WORDS = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
}


def split_sentences(phrase: str) -> list[str]:
    """Split a normalized multi-sentence phrase into lower-cased,
    period-free sentences."""
    out: list[str] = []
    for piece in _SENTENCE_BOUNDARY.split(phrase):
        piece = piece.strip()
        if piece.endswith("."):
            piece = piece[:-1]
        if piece and piece[0].isupper():
            piece = piece[0].lower() + piece[1:]
        if piece:
            out.append(piece)
    return out


_NO_REGEN_RE = re.compile(
    r"^(?:it|they|that creature|a creature destroyed this way) "
    r"can't be regenerated$"
)
# "That creature's controller ..." is Solitude's wording of the
# Swords to Plowshares rider (cube W/G creature bucket).
_GAIN_LIFE_EQUAL_POWER_RE = re.compile(
    r"^(?:its|that creature's) controller gains life equal to its power$"
)
_MAY_SEARCH_BASIC_RE = re.compile(
    r"^its controller may search their library for a basic land card, "
    r"put (?:it|that card) onto the battlefield tapped, then shuffle$"
)
# Winds of Abandon: a two-sentence mandatory search rider, joined with
# " / " by ``_rider_sentences``.
_EACH_EXILED_SEARCHES_RE = re.compile(
    r"^for each (?:creature|permanent) (?:exiled|destroyed) this way, its "
    r"controller searches their library for a basic land card / those "
    r"players put those cards onto the battlefield tapped, then shuffle$"
)
_CREATES_TOKENS_RE = re.compile(
    r"^its controller creates (?P<n>a|an|one|two|three) "
    r"(?P<name>[A-Z][A-Za-z ]*?) tokens?$"
)
_OTHER_PLAYERS_COPY_RE = re.compile(
    r"^each player other than its controller creates a token "
    r"that's a copy of it$"
)
_IF_YOU_CONTROLLED_DRAW_RE = re.compile(
    r"^if you controlled that permanent, draw (?P<n>a card|two cards)$"
)


def _parse_rider(sentence: str) -> RemovalRider | None:
    if _GAIN_LIFE_EQUAL_POWER_RE.match(sentence):
        return ControllerGainsLifeEqualToPower()
    if _MAY_SEARCH_BASIC_RE.match(sentence):
        return ControllerSearchesForBasicLand(optional=True)
    if _EACH_EXILED_SEARCHES_RE.match(sentence):
        return ControllerSearchesForBasicLand(optional=False)
    m = _CREATES_TOKENS_RE.match(sentence)
    if m is not None:
        from mtgz_ir.cards.tokens import TOKEN_PRINTINGS

        name = m.group("name")
        if name not in TOKEN_PRINTINGS:
            return None
        return ControllerCreatesTokens(
            token_name=name, count=_NUMBER_WORDS[m.group("n")],
        )
    if _OTHER_PLAYERS_COPY_RE.match(sentence):
        return OtherPlayersCreateTokenCopy()
    m = _IF_YOU_CONTROLLED_DRAW_RE.match(sentence)
    if m is not None:
        return IfYouControlledItDraw(amount=1 if m.group("n") == "a card" else 2)
    return None


def _rider_sentences(sentences: list[str]) -> list[str]:
    """Join the two-sentence "For each ... this way, its controller
    searches ... / Those players put those cards ..." rider."""
    out: list[str] = []
    i = 0
    while i < len(sentences):
        s = sentences[i]
        if (
            s.startswith("for each ")
            and i + 1 < len(sentences)
            and sentences[i + 1].startswith("those players ")
        ):
            out.append(f"{s} / {sentences[i + 1]}")
            i += 2
            continue
        out.append(s)
        i += 1
    return out


def _parse_riders(
    sentences: list[str], verb: str,
) -> tuple[bool, RemovalRider | None] | None:
    """Parse trailing rider sentences -> ``(can_be_regenerated, rider)``
    or ``None`` if any sentence is unrecognized."""
    can_be_regenerated = True
    rider: RemovalRider | None = None
    for sentence in _rider_sentences(sentences):
        if _NO_REGEN_RE.match(sentence):
            if verb != "destroy" or not can_be_regenerated:
                return None
            can_be_regenerated = False
            continue
        parsed = _parse_rider(sentence)
        if parsed is None or rider is not None:
            return None
        rider = parsed
    return can_be_regenerated, rider


_REMOVE_TARGET_RE = re.compile(
    r"^(?:(?P<verb>destroy|exile) (?P<rest>.+?)"
    r"|return (?P<brest>.+?) to its owner's hand)"
    r"(?: if it(?:'s (?P<color>white|blue|black|red|green)"
    r"| has mana value (?P<mv>\d+) or less)"
    r"| if its mana value is (?P<converge>less than or equal to the number "
    r"of colors of mana spent to cast this spell))?$"
)
_COLOR_LETTERS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}


_OTHER_TARGET_RE = re.compile(r"\b(?:an)?other target ")


def parse_targeted_removal(phrase: str) -> Effect | None:
    """``destroy|exile [count] target <np>`` / ``return target <np> to
    its owner's hand``, plus riders."""
    sentences = split_sentences(phrase)
    if not sentences:
        return None
    m = _REMOVE_TARGET_RE.match(sentences[0])
    if m is None:
        return None
    verb = m.group("verb") or "bounce"
    rest = m.group("rest") if m.group("verb") else m.group("brest")
    # "[up to one ]other target" / "another target" (Solitude; cube W/G
    # creature bucket): the source itself isn't a legal target.
    other = _OTHER_TARGET_RE.search(rest) is not None
    if other:
        rest = _OTHER_TARGET_RE.sub("target ", rest, count=1)
    counted = parse_target_count(rest)
    if counted is None:
        return None
    count_min, count_max, np = counted
    count_from_x = count_max == "X"
    if count_from_x:
        # "up to X target ...": the maximum is fixed at cast time
        # (RemoveTargetPermanents.cast_time_shape).
        count_max = 0
    plural = count_from_x or count_max > 1
    target_filter = parse_permanent_phrase(np, plural=plural)
    if target_filter is None:
        return None
    if other:
        from mtgz_ir.effects.object_predicates import IsSource

        target_filter = target_filter & ~IsSource()
    riders = _parse_riders(sentences[1:], verb)
    if riders is None:
        return None
    can_be_regenerated, rider = riders
    color = m.group("color")
    only_if = HasColor(_COLOR_LETTERS[color]) if color else None
    if m.group("mv"):
        only_if = HasManaValueAtMost(int(m.group("mv")))
    if m.group("converge"):
        only_if = HasManaValueAtMostColorsSpent()
    return RemoveTargetPermanents(
        action=verb,
        target_filter=target_filter,
        count_min=count_min,
        count_max=count_max,
        can_be_regenerated=can_be_regenerated,
        rider=rider,
        only_if=only_if,
        count_from_x=count_from_x,
    )


_MASS_REMOVAL_RE = re.compile(
    r"^(?P<verb>destroy|exile) (?P<quant>all|each) (?P<np>.+?)$"
)
_MV_X_SUFFIX = " with mana value X or less"


def parse_destroy_all(phrase: str) -> Effect | None:
    """``destroy|exile all <plural np>`` / ``destroy|exile each <np>``
    (the overload rewrite of "target") plus riders."""
    sentences = split_sentences(phrase)
    if not sentences:
        return None
    m = _MASS_REMOVAL_RE.match(sentences[0])
    if m is None:
        return None
    verb = m.group("verb")
    np = m.group("np")
    from_x = False
    # "with mana value X or less", optionally before the "that's one or
    # more colors" qualifier (Ugin, the Spirit Dragon's -X).
    for tail in ("", " that's one or more colors"):
        if np.endswith(_MV_X_SUFFIX + tail):
            from_x = True
            np = np[: -len(_MV_X_SUFFIX + tail)] + tail
            break
    plural = m.group("quant") == "all"
    # "artifacts and creatures" is a union in the "destroy all" frame.
    if plural and " and/or " not in np:
        np = np.replace(" and ", " and/or ")
    permanent_filter = parse_permanent_phrase(np, plural=plural)
    if permanent_filter is None:
        return None
    riders = _parse_riders(sentences[1:], verb)
    if riders is None:
        return None
    can_be_regenerated, rider = riders
    return RemoveAllMatching(
        permanent_filter=permanent_filter,
        action=verb,
        can_be_regenerated=can_be_regenerated,
        max_mana_value_from_x=from_x,
        rider=rider,
        label=f"{m.group('quant')} {m.group('np')}",
    )


_DEAL_DAMAGE_TARGET_RE = re.compile(
    r"^(?:it )?deals (?P<n>\d+|X) damage to "
    r"(?P<rest>any target|(?:up to one )?target .+)$"
)
_DIES_EXILE_RIDER_RE = re.compile(
    r"^if that (?:creature|creature or planeswalker|permanent) would die "
    r"this turn, exile it instead$"
)


def parse_damage_to_target(phrase: str) -> Effect | None:
    """``deals N|X damage to [up to one] target <permanent np>`` /
    ``deals X damage to any target``, optionally followed by "If that
    creature would die this turn, exile it instead."."""
    from mtgz_ir.effects.generic.damage import DealDamage
    from mtgz_ir.effects.generic.spell_damage import (
        DealDamageExileIfDiesThisTurn,
        DealXDamage,
    )
    from mtgz_ir.effects.targets import TargetSpec, any_target_filter

    sentences = split_sentences(phrase)
    if not sentences or len(sentences) > 2:
        return None
    m = _DEAL_DAMAGE_TARGET_RE.match(sentences[0])
    if m is None:
        return None
    dies_exile = False
    if len(sentences) == 2:
        if not _DIES_EXILE_RIDER_RE.match(sentences[1]):
            return None
        dies_exile = True
    if m.group("rest") == "any target":
        # The legacy any-target recognizer never takes the "it " subject
        # (a spell's subject is its own name); keep that boundary.
        if sentences[0].startswith("it "):
            return None
        target_filter = any_target_filter()
        count_min = 1
    else:
        # The regex admits only "target <np>" / "up to one target <np>".
        count_min, _count_max, np = parse_target_count(m.group("rest"))
        target_filter = parse_permanent_phrase(np)
        if target_filter is None:
            return None
    targets = (TargetSpec(target_filter, count_min=count_min, count_max=1),)
    n = m.group("n")
    if n == "X":
        if dies_exile:
            return None
        return DealXDamage(targets=targets)
    if dies_exile:
        return DealDamageExileIfDiesThisTurn(amount=int(n), targets=targets)
    return DealDamage(amount=int(n), targets=targets)


_EACH_OPPONENT_SACRIFICES_RE = re.compile(
    r"^each opponent sacrifices an? (?P<np>.+) of their choice$"
)


def parse_each_opponent_sacrifices(phrase: str) -> Effect | None:
    """``each opponent sacrifices a <np> of their choice``."""
    from mtgz_ir.effects.generic.targeted_removal import (
        EachOpponentSacrificesMatching,
    )

    m = _EACH_OPPONENT_SACRIFICES_RE.match(phrase)
    if m is None:
        return None
    np = m.group("np")
    permanent_filter = parse_permanent_phrase(np)
    if permanent_filter is None:
        return None
    return EachOpponentSacrificesMatching(
        permanent_filter=permanent_filter, label=np,
    )


__all__ = [
    "parse_each_opponent_sacrifices",
    "parse_damage_to_target",
    "parse_destroy_all",
    "parse_targeted_removal",
    "split_sentences",
]
