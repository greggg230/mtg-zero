"""Parser recognizers for the Vintage Roto P4 package: planeswalkers,
copies, control changes and turn structure.

Every hook here is a *last* fallback of the shared grammar it extends
(``effects._parse_effect``, ``statics._parse_static_ability_line``, the
planeswalker builder), so no phrase an older recognizer claims changes
meaning. Like the cube buckets, the grammar is switched off for
printings that exist only in the EoE-era dumps (the shared
``creature_rmc_grammar`` switch), so no EoE printing's parse can
change. Engine effects live in ``effects/generic/roto_walkers.py`` and
the turn-structure rules in ``mtgz_ir.roto_turns``.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from mtgz_ir.effects.base import StaticAbility, StaticScope


def _disabled() -> bool:
    from mtgz_ir.cards.parser.creature_rmc_grammar import _DISABLED

    return _DISABLED[0]


# ---------------------------------------------------------------------------
# Statics
# ---------------------------------------------------------------------------

# "Skip your draw step." (Necropotence, Yawgmoth's Bargain; CR 614.10).
_SKIP_DRAW_STEP_LINE = "Skip your draw step."


def parse_roto_static_line(line: str) -> tuple[StaticAbility, ...]:
    """``StaticAbility`` tuple for a static line the shared static
    grammar declined, or ``()``."""
    if _disabled():
        return ()
    if line == _SKIP_DRAW_STEP_LINE:
        return (StaticAbility(
            scope=StaticScope.SELF, skips_your_draw_step=True,
        ),)
    return ()




# ---------------------------------------------------------------------------
# Effect phrases (last fallback of ``effects._parse_effect``)
# ---------------------------------------------------------------------------

_CARDINALS = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10, "twenty": 20,
}
_N = (
    r"(?:\d+|a|an|one|two|three|four|five|six|seven|eight|nine|ten|twenty)"
)


def _n(token: str) -> int:
    return int(token) if token.isdigit() else _CARDINALS[token]


def _effect(phrase: str):
    from mtgz_ir.cards.parser.effects import _parse_effect

    return _parse_effect(phrase)


def _untargeted(phrase: str):
    eff = _effect(phrase)
    if eff is None or eff.requires_target():
        return None
    return eff


# "exile the top [<N>] card(s) of your library[ face down]. Put that card
# / them into your hand at the beginning of your next end step"
# (Necropotence).
_EXILE_TOP_TO_HAND_AT_NEXT_END_STEP_RE = re.compile(
    rf"^exile the top (?:(?P<n>{_N}) )?cards? of your library"
    r"(?P<down> face down)?\. Put (?:that card|them|it) into your hand at "
    r"the beginning of your next end step$"
)


def _exile_top_to_hand(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.ExileTopCardsToHandAtYourNextEndStep(
        count=_n(m.group("n")) if m.group("n") else 1,
        face_down=bool(m.group("down")),
    )


# "if it's your turn, <effect>" (Day's Undoing).
_IF_YOUR_TURN_RE = re.compile(r"^if it's your turn, (?P<eff>.+)$")


def _if_your_turn(m):
    from mtgz_ir.effects.combinators import IfConditionElse
    from mtgz_ir.effects.conditions import DuringYourTurn

    inner = _untargeted(m.group("eff"))
    if inner is None:
        return None
    return IfConditionElse(condition=DuringYourTurn(), then_effect=inner)


# "take an extra turn after this one. At the beginning of that turn's end
# step, <effect>" (Final Fortune).
_EXTRA_TURN_THEN_END_STEP_RE = re.compile(
    r"^take an extra turn after this one\. At the beginning of that turn's "
    r"end step, (?P<eff>.+)$"
)


def _extra_turn_then_end_step(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    inner = _untargeted(m.group("eff"))
    if inner is None:
        return None
    return rw.TakeExtraTurnWithEndStepTrigger(inner=inner)


# "you may reveal <a card descriptor> you own from outside the game and
# put it into your hand" (Burning Wish; the wishes).
_WISH_RE = re.compile(
    r"^you may reveal (?P<desc>an? .+? card) you own from outside the game "
    r"and put it into your hand$"
)


def _wish(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(m.group("desc"))
    if card_filter is None:
        return None
    return rw.PutCardFromOutsideTheGameIntoHand(card_filter=card_filter)


# "each player discards a card" (Liliana of the Veil's +1) is P2's
# ``roto_zones`` "each player ..." attrition grammar (Smallpox's shape,
# ``EachPlayerSequence``), tried earlier in the ``_parse_effect`` chain;
# P4's same-behaviour ``EachPlayerDiscards`` was removed when the
# packages were integrated.


# "put up to <N> <descriptor> cards from your hand onto the battlefield"
# (Ugin, the Spirit Dragon).
_PUT_UP_TO_N_FROM_HAND_RE = re.compile(
    rf"^put up to (?P<n>{_N}) (?P<desc>.+?) cards from your hand onto the "
    r"battlefield$"
)


def _put_up_to_n_from_hand(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
    if card_filter is None:
        return None
    return rw.PutUpToNCardsFromHandOntoBattlefield(
        count=_n(m.group("n")), card_filter=card_filter,
    )


# "untap target <permanent noun phrase>" (Kiora, Behemoth Beckoner's -1)
# is P5's ``roto_board`` tap / untap grammar, tried earlier in the
# ``_parse_effect`` chain; P4's narrower copy was removed when the
# packages were integrated.


# "until your next turn, <effect>" -- the leading form of the "<effect>
# until your next turn" duration (Jace, the Perfected Mind's +1).
_UNTIL_YOUR_NEXT_TURN_LEADING_RE = re.compile(
    r"^until your next turn, (?P<eff>.+)$"
)


def _until_your_next_turn_leading(m):
    return _effect(f"{m.group('eff')} until your next turn")


# "target player mills [<K> times ]X cards" (Jace, the Perfected Mind's
# -X; X is the loyalty X).
_MILL_TIMES_X_RE = re.compile(
    rf"^target player mills (?:(?P<k>{_N}) times )?X cards$"
)


def _mill_times_x(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.MillTargetPlayerTimesX(
        per_x=_n(m.group("k")) if m.group("k") else 1,
    )


# "<effect>. Then if <condition>, <effect>. Otherwise, <effect>" (Jace,
# the Perfected Mind's -2). Conditions: ``_THEN_IF_CONDITIONS``.
_THEN_IF_OTHERWISE_RE = re.compile(
    r"^(?P<first>[^.]+)\. Then if (?P<cond>[^,]+), (?P<then>[^.]+)\. "
    r"Otherwise, (?P<else>[^.]+)$"
)
_GRAVEYARD_HAS_AT_LEAST_RE = re.compile(
    rf"^a graveyard has (?P<n>{_N}) or more cards in it$"
)


def _then_if_otherwise(m):
    from mtgz_ir.effects.combinators import IfConditionElse, Sequence
    from mtgz_ir.effects.generic import roto_walkers as rw

    c = _GRAVEYARD_HAS_AT_LEAST_RE.match(m.group("cond"))
    if c is None:
        return None
    first = _effect(m.group("first"))
    then = _untargeted(_lower_first(m.group("then")))
    other = _untargeted(_lower_first(m.group("else")))
    if first is None or then is None or other is None:
        return None
    return Sequence(effects=(first, IfConditionElse(
        condition=rw.AnyGraveyardHasAtLeast(n=_n(c.group("n"))),
        then_effect=then, else_effect=other,
    )))


def _lower_first(text: str) -> str:
    return text[:1].lower() + text[1:]


# "separate all permanents target player controls into two piles. That
# player sacrifices all permanents in the pile of their choice" (Liliana
# of the Veil's -6).
_PILES_RE = re.compile(
    r"^separate all permanents target player controls into two piles\. That "
    r"player sacrifices all permanents in the pile of their choice$"
)


def _piles(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.SeparatePermanentsIntoPilesSacrificeChosen()


# "exchange control of this <permanent> and [up to one ]target <np>. If
# you don't or can't make an exchange, sacrifice this <permanent>[. This
# ability still resolves if its target becomes illegal]" (Gilded Drake).
_EXCHANGE_SELF_OR_SACRIFICE_RE = re.compile(
    r"^exchange control of this (?P<self>creature|artifact|permanent) and "
    r"(?P<upto>up to one )?target (?P<np>.+?)\. If you don't or can't make an "
    r"exchange, sacrifice this (?P=self)(?P<still>\. This ability still "
    r"resolves if its target becomes illegal)?$"
)


def _exchange_self_or_sacrifice(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"))
    if target_filter is None:
        return None
    spec = TargetSpec(
        filter=target_filter, count_min=0 if m.group("upto") else 1,
    )
    return rw.ExchangeControlOfSourceAndTarget(
        targets=(spec,),
        still_resolves_if_targets_illegal=bool(m.group("still")),
    )


# "gain control of target <np> until end of turn. Untap that <noun> / it.
# It gains haste until end of turn" (Zealous Conscripts).
_THREATEN_RE = re.compile(
    r"^gain control of target (?P<np>.+?) until end of turn\. Untap (?:that "
    r"(?:creature|permanent|artifact|land)|it)\. It gains haste until end of "
    r"turn$"
)


def _threaten(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"))
    if target_filter is None:
        return None
    return rw.GainControlUntapHasteUEOT(targets=(TargetSpec(filter=target_filter),))


# "[you may ]exile [another ]target <np>, then return it / that card to the
# battlefield under its owner's control" (Ephemerate, Felidar Guardian).
_FLICKER_RE = re.compile(
    r"^(?P<may>you may )?exile (?P<another>another )?target (?P<np>.+?), then "
    r"return (?:it|that card) to the battlefield under its owner's control$"
)


def _flicker(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.object_predicates import IsSource
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"))
    if target_filter is None:
        return None
    if m.group("another"):
        target_filter = target_filter & ~IsSource()
    return rw.ExileTargetThenReturnUnderOwnersControl(
        targets=(TargetSpec(filter=target_filter),),
        optional=bool(m.group("may")),
    )


# "target opponent exiles cards from the top of their library until they
# exile a nonland card. Until end of turn, you may cast that card without
# paying its mana cost" (Nicol Bolas, God-Pharaoh).
_EXILE_UNTIL_NONLAND_CAST_FREE_RE = re.compile(
    r"^target opponent exiles cards from the top of their library until they "
    r"exile a nonland card\. Until end of turn, you may cast that card "
    r"without paying its mana cost$"
)


def _exile_until_nonland_cast_free(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.TargetOpponentExilesUntilNonlandCastFree()


# "each opponent exiles <N> cards from their hand" (Nicol Bolas).
_EACH_OPPONENT_EXILES_FROM_HAND_RE = re.compile(
    rf"^each opponent exiles (?P<n>{_N}) cards? from their hand$"
)


def _each_opponent_exiles_from_hand(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.EachOpponentExilesCardsFromHand(count=_n(m.group("n")))


# "deals <N> damage to target <item>, <item>, or <item>" -- a target list
# whose items carry their own controller ("target opponent, creature an
# opponent controls, or planeswalker an opponent controls" -- Nicol Bolas).
_DAMAGE_TO_TARGET_LIST_RE = re.compile(
    r"^deals (?P<n>\d+) damage to target (?P<items>.+, or .+)$"
)
_PLAYER_ITEMS = {"player": None, "opponent": "opponent"}


def _damage_to_target_list(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic.damage import DealDamage
    from mtgz_ir.effects.object_predicates import ControlledByOpponent, IsPlayer
    from mtgz_ir.effects.target_filter import OneOf
    from mtgz_ir.effects.targets import TargetSpec

    items = re.split(r", or |, ", m.group("items"))
    arms = []
    for item in items:
        if item in _PLAYER_ITEMS:
            arm = IsPlayer()
            if _PLAYER_ITEMS[item] == "opponent":
                arm = arm & ControlledByOpponent()
            arms.append(arm)
            continue
        f = parse_permanent_phrase(item)
        if f is None:
            return None
        arms.append(f)
    return DealDamage(
        amount=int(m.group("n")),
        targets=(TargetSpec(filter=OneOf(*arms)),),
    )


# "... your opponents control ..." reads as "... an opponent controls ..."
# (two players): retried through the whole grammar.
_YOUR_OPPONENTS_CONTROL = "your opponents control"


_LITERAL_PHRASES = {
    # CR 723: "End the turn." (Sundial of the Infinite, Day's Undoing).
    "end the turn": "EndTheTurn",
    # CR 104.3d (Final Fortune's delayed trigger).
    "you lose the game": "LoseTheGame",
    # "that much" is the triggering event's amount (Dellian Fel's emblem).
    "target opponent loses that much life": "TargetOpponentLosesThatMuchLife",
    # Enter the Infinite (a "no maximum hand size" emblem until your turn).
    "you have no maximum hand size until your next turn": (
        "NoMaximumHandSizeUntilYourNextTurn"
    ),
}
_PHRASE_RECOGNIZERS = (
    (_IF_YOUR_TURN_RE, _if_your_turn),
    (_EXTRA_TURN_THEN_END_STEP_RE, _extra_turn_then_end_step),
    (_WISH_RE, _wish),
    (_EXILE_TOP_TO_HAND_AT_NEXT_END_STEP_RE, _exile_top_to_hand),
    (_PUT_UP_TO_N_FROM_HAND_RE, _put_up_to_n_from_hand),
    (_UNTIL_YOUR_NEXT_TURN_LEADING_RE, _until_your_next_turn_leading),
    (_MILL_TIMES_X_RE, _mill_times_x),
    (_THEN_IF_OTHERWISE_RE, _then_if_otherwise),
    (_PILES_RE, _piles),
    (_EXCHANGE_SELF_OR_SACRIFICE_RE, _exchange_self_or_sacrifice),
    (_THREATEN_RE, _threaten),
    (_FLICKER_RE, _flicker),
    (_EXILE_UNTIL_NONLAND_CAST_FREE_RE, _exile_until_nonland_cast_free),
    (_EACH_OPPONENT_EXILES_FROM_HAND_RE, _each_opponent_exiles_from_hand),
    (_DAMAGE_TO_TARGET_LIST_RE, _damage_to_target_list),
)


def parse_roto_effect(phrase: str):
    """``Effect`` for a normalized phrase every older grammar declined,
    or ``None``."""
    if _disabled():
        return None
    literal = _LITERAL_PHRASES.get(phrase)
    if literal is not None:
        from mtgz_ir.effects.generic import roto_walkers as rw

        return getattr(rw, literal)()
    for pattern, build in (
        *_PHRASE_RECOGNIZERS, *_PHRASE_RECOGNIZERS_2, *_PHRASE_RECOGNIZERS_3,
    ):
        m = pattern.match(phrase)
        if m is not None:
            return build(m)
    if _YOUR_OPPONENTS_CONTROL in phrase:
        return _effect(
            phrase.replace(_YOUR_OPPONENTS_CONTROL, "an opponent controls"),
        )
    return None


# ---------------------------------------------------------------------------
# Planeswalker lines (hooks in ``planeswalker_builder``)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RotoWalkerLine:
    """What a planeswalker line contributes: activated abilities and/or
    keywords."""

    activated: tuple = ()
    keywords: frozenset = frozenset()


# "−X: <effect>" -- a loyalty ability whose cost is X loyalty.
_LOYALTY_X_RE = re.compile(r"^[−-]X: (?P<body>.+)$")
# Sentence starts that refer back to an earlier sentence: such a body
# can't be split into independent effects.
_ANAPHORIC_START_RE = re.compile(
    r"^(?:It|Its|That|Those|They|Them|Their|This|These|Otherwise|If|When|"
    r"Each of them|Then (?:if|that|those|it))\b"
)


def parse_roto_loyalty_body(body: str, name: str):
    """A loyalty-ability body the unified grammar declined, or ``None``:
    a body of independent sentences ("Scry 1. Saheeli Rai deals 1
    damage to each opponent.") becomes their ``Sequence`` -- the spell
    builder's sentence composition -- unless a later sentence refers
    back to an earlier one."""
    if _disabled():
        return None
    from mtgz_ir.cards.parser.planeswalker_builder import _parse_loyalty_effect
    from mtgz_ir.cards.parser.spell_builder import _split_sentences
    from mtgz_ir.effects.combinators import Sequence

    sentences = _split_sentences(body)
    if len(sentences) < 2:
        return None
    if any(_ANAPHORIC_START_RE.match(s) for s in sentences[1:]):
        return None
    effects = []
    for sentence in sentences:
        eff = _parse_loyalty_effect(sentence, name)
        if eff is None:
            return None
        effects.append(eff)
    if sum(e.requires_target() for e in effects) > 1:
        # One target choice per ability (``Sequence``'s contract).
        return None
    return Sequence(effects=tuple(effects))


def parse_roto_walker_line(line: str, name: str) -> RotoWalkerLine | None:
    """A planeswalker line the builder declined: a "−X" loyalty ability
    or the Compleated keyword (CR 702.150)."""
    if _disabled():
        return None
    from mtgz_ir.cards.base import ActivatedAbility, ActivationCost

    if line == "Compleated":
        return RotoWalkerLine(keywords=frozenset({"compleated"}))
    m = _LOYALTY_X_RE.match(line)
    if m is not None:
        from mtgz_ir.cards.parser.planeswalker_builder import (
            _parse_loyalty_effect,
        )

        body = m.group("body")
        effect = _parse_loyalty_effect(body, name)
        if effect is None:
            effect = parse_roto_loyalty_body(body, name)
        if effect is None:
            return None
        return RotoWalkerLine(activated=(ActivatedAbility(
            cost=ActivationCost(loyalty_x=True),
            effect=effect,
            sorcery_speed=True,
        ),))
    return None


# ---------------------------------------------------------------------------
# Trigger conditions (last fallback of ``triggers._parse_predicate_trigger_
# phrase``)
# ---------------------------------------------------------------------------


def parse_roto_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for a trigger condition the older grammars
    declined, or ``None``:

    * "this creature becomes the target of a spell or ability" -- the
      source as the targeted object (Phantasmal Image's granted
      trigger), dispatched by ``cube_creatures_rmc.fire_becomes_target``;
    * "you gain life" -- ``Event.GAINS_LIFE`` from ``game.gain_life``
      (Professor Dellian Fel's emblem).
    """
    if _disabled():
        return None
    from mtgz_ir.effects.predicates import Event, IsSelf

    if phrase == "this creature becomes the target of a spell or ability":
        return (Event.BECOMES_TARGET, IsSelf(subject="target"))
    if phrase == "you gain life":
        from mtgz_ir.effects.predicates import YOU, Controlled

        return (Event.GAINS_LIFE, Controlled(YOU, subject="player"))
    return None


# ---------------------------------------------------------------------------
# Clone "except" riders (hook in ``cube_creatures_ub._parse_enter_as_copy_
# line``)
# ---------------------------------------------------------------------------

# "it's an? <Subtype> in addition to its other types[ and it has
# <keywords> | "<ability>"]" (Phantasmal Image, Mockingbird).
_COPY_EXCEPTION_RE = re.compile(
    r"^it's an? (?P<sub>[A-Z][a-z]+) in addition to its other types"
    r"(?: and it has (?P<has>.+))?$"
)


def parse_copy_exception(exc: str) -> dict | None:
    """``CardPrinting`` clone fields for an "except ..." rider the clone
    recognizer's own shapes declined, or ``None``."""
    if _disabled():
        return None
    m = _COPY_EXCEPTION_RE.match(exc)
    if m is None:
        return None
    fields: dict = {"enters_as_copy_add_subtypes": (m.group("sub"),)}
    has = m.group("has")
    if has is None:
        return fields
    if has.startswith('"') and has.endswith('"'):
        trigger = _quoted_self_trigger(has[1:-1])
        if trigger is None:
            return None
        fields["enters_as_copy_add_triggers"] = (trigger,)
        return fields
    from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

    kws = _parse_keyword_grant_body(has)
    if not kws:
        return None
    fields["enters_as_copy_add_keywords"] = frozenset(kws)
    return fields


def _quoted_self_trigger(text: str):
    """A granted triggered ability whose condition names "this creature";
    a closing "it" is that creature ("..., sacrifice it.")."""
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line

    if not text.startswith(("When this creature ", "Whenever this creature ")):
        return None
    if not text.endswith("."):
        text += "."
    if text.endswith(" it."):
        text = text[: -len(" it.")] + " this creature."
    parsed = _parse_triggered_ability_line(text)
    if parsed is None or isinstance(parsed, tuple):
        return None
    return parsed


# ---------------------------------------------------------------------------
# Casting keywords (hook in ``spell_builder._casting_keyword_fields``)
# ---------------------------------------------------------------------------


# "You may exile <N> <desc> cards from your hand rather than pay this
# spell's mana cost." -- the pitch cost with a count above one
# (Commandeer); the single-card form is ``spell_builder``'s
# ``_PITCH_ALT_COST_LINE_RE``.
_PITCH_N_CARDS_RE = re.compile(
    rf"^You may exile (?P<n>{_N}) (?P<desc>.+?) cards from your hand rather "
    r"than pay this spell's mana cost\.$"
)


def parse_roto_casting_keyword(clause: str) -> dict | None:
    """``CardPrinting`` fields for a casting-keyword line the spell
    builder's own table declined: ``Rebound`` (CR 702.88); a pitch cost
    exiling several cards."""
    if _disabled():
        return None
    if clause == "Rebound":
        return {"rebound": True}
    m = _PITCH_N_CARDS_RE.match(clause)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
        if card_filter is None:
            return None
        return {
            "alt_cost_exile_from_hand": card_filter,
            "alt_cost_exile_from_hand_count": _n(m.group("n")),
        }
    return None


# ---------------------------------------------------------------------------
# Counts, hand size, token sizes, mass control (second batch)
# ---------------------------------------------------------------------------

_ZONES = r"(?P<zone>library|graveyard|hand)"


def _zone_count(what: str, zone: str):
    """``CardsInYourZone`` for "the number of <cards> in your <zone>"."""
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    if what == "cards":
        return rw.CardsInYourZone(zone=zone)
    if not what.endswith(" cards"):
        return None
    card_filter = parse_card_descriptor(f"a {what[: -len(' cards')]} card")
    if card_filter is None:
        return None
    return rw.CardsInYourZone(zone=zone, card_filter=card_filter)


# "draw cards equal to the number of <cards> in your <zone>" (Enter the
# Infinite).
_DRAW_EQUAL_TO_ZONE_COUNT_RE = re.compile(
    rf"^draw cards equal to the number of (?P<what>.+?) in your {_ZONES}$"
)


def _draw_equal_to_zone_count(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    count = _zone_count(m.group("what"), m.group("zone"))
    return None if count is None else rw.DrawCardsEqualTo(count=count)


# "each opponent loses life equal to the number of <cards> in your <zone>"
# (Grist, the Hunger Tide's -5).
_OPPONENTS_LOSE_LIFE_EQUAL_TO_ZONE_COUNT_RE = re.compile(
    rf"^each opponent loses life equal to the number of (?P<what>.+?) in "
    rf"your {_ZONES}$"
)


def _opponents_lose_life_equal_to_zone_count(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    count = _zone_count(m.group("what"), m.group("zone"))
    return None if count is None else rw.EachOpponentLosesLifeEqualTo(count=count)


# "put a card from your hand on top of your library" -- the single-card
# form of Brainstorm's put-back, printed without "in any order" (Enter the
# Infinite); ``effects_ext._PUT_CARDS_FROM_HAND_ON_TOP`` requires the
# ordering clause and is left as is so the EoE-era pools' parses can't
# move.
_PUT_A_CARD_FROM_HAND_ON_TOP_RE = re.compile(
    r"^put (?:a|one) card from your hand on top of your library$"
)


def _put_a_card_from_hand_on_top(m):
    from mtgz_ir.effects.generic.cube_walkers import (
        PutCardsFromHandOnTopOfLibrary,
    )

    return PutCardsFromHandOnTopOfLibrary(amount=1)


# "create an X/X <token>, where X is this planeswalker's loyalty" (Nissa,
# Ascended Animist; the planeswalker builder rewrites the walker's name).
_CREATE_X_X_TOKEN_LOYALTY_RE = re.compile(
    r"^create an X/X (?P<tok>.+? creature token), where X is this "
    r"(?:planeswalker|permanent)'s loyalty$"
)


def _create_x_x_token_loyalty(m):
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.generic.tokens import CreateToken

    base = _effect(f"create a 0/0 {m.group('tok')}")
    if not isinstance(base, CreateToken) or base.count != 1:
        return None
    return rw.CreateTokenWithPTEqualTo(
        token_printing=base.token_printing, count=rw.SourceLoyalty(),
    )


# "until end of turn, <effect>" -- the leading form of "<effect> until
# end of turn" (Nissa, Ascended Animist's -7).
_UNTIL_EOT_LEADING_RE = re.compile(r"^until end of turn, (?P<eff>.+)$")


def _until_eot_leading(m):
    return _effect(f"{m.group('eff')} until end of turn")


def _keywords(text: str) -> frozenset | None:
    """"vigilance, lifelink, and indestructible" -> keyword set."""
    from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

    out: set = set()
    for word in re.split(r", and |, | and ", text):
        kws = _parse_keyword_grant_body(word)
        if not kws:
            return None
        out.update(kws)
    return frozenset(out)


# "creatures you control get +P/+T for each <Subtype | noun> you control[
# and gain <keywords>] until end of turn" (Nissa, Ascended Animist).
_PUMP_PER_CONTROLLED_RE = re.compile(
    r"^creatures you control get \+(?P<p>\d+)/\+(?P<t>\d+) for each "
    r"(?P<what>.+?) you control(?: and gain (?P<kws>.+?))? until end of turn$"
)


def _controlled_noun(what: str):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.object_predicates import HasSubtype

    if re.fullmatch(r"[A-Z][a-z]+", what):
        return HasSubtype(what)
    return parse_permanent_phrase(what)


def _pump_per_controlled(m):
    from mtgz_ir.effects.formulas import CountControlledMatching
    from mtgz_ir.effects.generic import roto_walkers as rw

    pred = _controlled_noun(m.group("what"))
    if pred is None:
        return None
    kws: frozenset = frozenset()
    if m.group("kws"):
        kws = _keywords(m.group("kws"))
        if kws is None:
            return None
    return rw.PumpCreaturesYouControlPerCount(
        power=int(m.group("p")), toughness=int(m.group("t")),
        count=CountControlledMatching(predicate=pred), keywords=kws,
    )


# "[up to one ]target <np> gains <keywords> until your next turn" (Dihada).
_GAINS_UNTIL_YOUR_NEXT_TURN_RE = re.compile(
    r"^(?P<upto>up to one )?target (?P<np>.+?) gains (?P<kws>.+?) until your "
    r"next turn$"
)


def _target_noun(np: str):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.object_predicates import IsLegendary

    if np.startswith("legendary "):
        rest = parse_permanent_phrase(np[len("legendary "):])
        return None if rest is None else rest & IsLegendary()
    return parse_permanent_phrase(np)


def _gains_until_your_next_turn(m):
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = _target_noun(m.group("np"))
    kws = _keywords(m.group("kws"))
    if target_filter is None or kws is None:
        return None
    spec = TargetSpec(
        filter=target_filter, count_min=0 if m.group("upto") else 1,
    )
    return rw.GrantKeywordsUntilYourNextTurn(targets=(spec,), keywords=kws)


# "gain control of all <np> until end of turn. Untap them. They gain haste
# until end of turn" (Dihada, Binder of Wills' -11).
_GAIN_CONTROL_OF_ALL_RE = re.compile(
    r"^gain control of all (?P<np>.+?) until end of turn\. Untap them\. They "
    r"gain haste until end of turn$"
)


def _gain_control_of_all(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw

    pred = parse_permanent_phrase(m.group("np"), plural=True)
    if pred is None:
        return None
    return rw.GainControlOfAllUntapHasteUEOT(permanent_filter=pred)


# "reveal the top <N> cards of your library. Put any number of <desc>
# cards from among them into your hand and the rest into your graveyard[.
# Create a Treasure token for each card put into your graveyard this
# way]" (Dihada, Binder of Wills' -3).
_REVEAL_TOP_ANY_NUMBER_RE = re.compile(
    rf"^reveal the top (?P<n>{_N}) cards of your library\. Put any number of "
    r"(?P<desc>.+?) cards from among them into your hand and the rest into "
    r"your graveyard(?P<treasure>\. Create a Treasure token for each card put "
    r"into your graveyard this way)?$"
)


def _reveal_top_any_number(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
    if card_filter is None:
        return None
    return rw.RevealTopPutMatchingInHandRestInGraveyard(
        count=_n(m.group("n")), card_filter=card_filter,
        treasure_per_graveyard_card=bool(m.group("treasure")),
    )


# "each opponent may scry <N>" (Kaya, Intangible Slayer).
_EACH_OPPONENT_MAY_SCRY_RE = re.compile(rf"^each opponent may scry (?P<n>{_N})$")


def _each_opponent_may_scry(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.EachOpponentMayScry(amount=_n(m.group("n")))


_PHRASE_RECOGNIZERS_2 = (
    (_DRAW_EQUAL_TO_ZONE_COUNT_RE, _draw_equal_to_zone_count),
    (_OPPONENTS_LOSE_LIFE_EQUAL_TO_ZONE_COUNT_RE,
     _opponents_lose_life_equal_to_zone_count),
    (_PUT_A_CARD_FROM_HAND_ON_TOP_RE, _put_a_card_from_hand_on_top),
    (_CREATE_X_X_TOKEN_LOYALTY_RE, _create_x_x_token_loyalty),
    (_UNTIL_EOT_LEADING_RE, _until_eot_leading),
    (_PUMP_PER_CONTROLLED_RE, _pump_per_controlled),
    (_GAINS_UNTIL_YOUR_NEXT_TURN_RE, _gains_until_your_next_turn),
    (_GAIN_CONTROL_OF_ALL_RE, _gain_control_of_all),
    (_REVEAL_TOP_ANY_NUMBER_RE, _reveal_top_any_number),
    (_EACH_OPPONENT_MAY_SCRY_RE, _each_opponent_may_scry),
)


# ---------------------------------------------------------------------------
# Token copies with "except" riders; searches (third batch)
# ---------------------------------------------------------------------------

_COLOR_WORDS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
_CARD_TYPE_WORDS = {
    "artifact": "Artifact", "creature": "Creature",
    "enchantment": "Enchantment", "land": "Land",
}
# "it's an? [<P>/<T> ][<color> ]<types>[ with <keywords>] in addition to
# its other types"
_TOKEN_COPY_EXCEPTION_RE = re.compile(
    r"^it's an? (?:(?P<p>\d+)/(?P<t>\d+) )?(?:(?P<color>white|blue|black|red|"
    r"green) )?(?P<types>[A-Za-z ]+?)(?: with (?P<kws>[a-z, ]+?))? in "
    r"addition to its other types$"
)


def _token_copy_exceptions(text: str):
    """``CopyExceptions`` for a token copy's "except ..." rider."""
    from mtgz_ir.effects.generic import roto_walkers as rw

    m = _TOKEN_COPY_EXCEPTION_RE.match(text)
    if m is None:
        return None
    add_types: list = []
    add_subtypes: list = []
    for word in m.group("types").split(" "):
        if word in _CARD_TYPE_WORDS:
            add_types.append(_CARD_TYPE_WORDS[word])
        elif re.fullmatch(r"[A-Z][a-z]+", word):
            add_subtypes.append(word)
        else:
            return None
    kws: frozenset = frozenset()
    if m.group("kws"):
        kws = _keywords(m.group("kws"))
        if kws is None:
            return None
    return rw.CopyExceptions(
        add_types=tuple(add_types),
        add_subtypes=tuple(add_subtypes),
        base_pt=(int(m.group("p")), int(m.group("t"))) if m.group("p") else None,
        colors=(
            frozenset({_COLOR_WORDS[m.group("color")]}) if m.group("color")
            else None
        ),
        add_keywords=kws,
    )


# "create a token that's a copy of target <np>, except <rider>[. That
# token gains haste][. Exile it at the beginning of the next end step]"
# (Saheeli Rai's -2).
_TOKEN_COPY_OF_TARGET_EXCEPT_RE = re.compile(
    r"^create a token that's a copy of target (?P<np>.+?), except "
    r"(?P<exc>.+?)(?P<haste>\. That token gains haste)?(?P<exile>\. Exile "
    r"it at the beginning of the next end step)?$"
)


def _token_copy_of_target_except(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"))
    exc = _token_copy_exceptions(m.group("exc"))
    if target_filter is None or exc is None:
        return None
    return rw.CreateTokenCopyOfTargetWithExceptions(
        targets=(TargetSpec(filter=target_filter),),
        exceptions=exc,
        gains_haste=bool(m.group("haste")),
        exile_at_next_end_step=bool(m.group("exile")),
    )


# "exile target <np>. If it wasn't an Aura, create a token that's a copy
# of it, except <rider>" (Kaya, Intangible Slayer's -3).
_EXILE_THEN_COPY_UNLESS_AURA_RE = re.compile(
    r"^exile target (?P<np>.+?)\. If it wasn't an Aura, create a token that's "
    r"a copy of it, except (?P<exc>.+)$"
)


def _exile_then_copy_unless_aura(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"))
    exc = _token_copy_exceptions(m.group("exc"))
    if target_filter is None or exc is None:
        return None
    return rw.ExileTargetThenTokenCopyUnlessAura(
        targets=(TargetSpec(filter=target_filter),), exceptions=exc,
    )


# "search your library for up to <N> <desc> cards with different names,
# put them onto the battlefield, then shuffle" (Saheeli Rai's -7).
_SEARCH_DIFFERENT_NAMES_ONTO_BATTLEFIELD_RE = re.compile(
    rf"^search your library for up to (?P<n>{_N}) (?P<desc>.+?) cards with "
    r"different names, put them onto the battlefield, then shuffle$"
)


def _search_different_names_onto_battlefield(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
    if card_filter is None:
        return None
    return rw.SearchUpToNDifferentNamesOntoBattlefield(
        count=_n(m.group("n")), card_filter=card_filter,
    )


# "you gain control of target opponent during that player's next turn[.
# After that turn, that player takes an extra turn]" (Emrakul, the
# Promised End's cast trigger).
_CONTROL_OPPONENT_NEXT_TURN_RE = re.compile(
    r"^you gain control of target opponent during that player's next turn"
    r"(?P<extra>\. After that turn, that player takes an extra turn)?$"
)


def _control_opponent_next_turn(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.ControlTargetOpponentDuringNextTurn(
        extra_turn_after=bool(m.group("extra")),
    )


# "return target <np> to its owner's hand. Then that permanent's
# controller may sacrifice a land of their choice. If the player does,
# they may copy this spell and may choose a new target for that copy"
# (Chain of Vapor).
_BOUNCE_CHAIN_COPY_RE = re.compile(
    r"^return target (?P<np>.+?) to its owner's hand\. Then that "
    r"permanent's controller may sacrifice a land of their choice\. If the "
    r"player does, they may copy this spell and may choose a new target for "
    r"that copy$"
)


def _bounce_chain_copy(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"))
    if target_filter is None:
        return None
    return rw.BounceThenChainCopyBySacrificingLand(
        targets=(TargetSpec(filter=target_filter),),
    )


# "exile a <desc> card from your graveyard at random, then create a tapped
# token that's a copy of that card. If the exiled card is a land card,
# repeat this process" (Sin, Spira's Punishment).
_RANDOM_GRAVEYARD_TOKEN_COPY_RE = re.compile(
    r"^exile an? (?P<desc>.+?) card from your graveyard at random, then "
    r"create a tapped token that's a copy of that card\. If the exiled card "
    r"is a land card, repeat this process$"
)


def _random_graveyard_token_copy(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
    if card_filter is None:
        return None
    return rw.ExileRandomGraveyardCardTokenCopyRepeatIfLand(
        card_filter=card_filter,
    )


# "copy it / that spell. You may choose new targets for the copy" -- the
# body of a "whenever you cast <a spell>" trigger (Chandra, Hope's
# Beacon); "it" is the cast spell (the trigger's event subject).
_COPY_CAST_SPELL_RE = re.compile(
    r"^copy (?:it|that spell)\. You may choose new targets for the copy$"
)


def _copy_cast_spell(m):
    from mtgz_ir.effects.generic import roto_walkers as rw

    return rw.CopyTriggeringSpell()


# "exile the top <N> cards of your library. Until the end of your next
# turn, you may cast a <desc> spell from among those exiled cards"
# (Chandra, Hope's Beacon's +1).
_EXILE_TOP_CAST_ONE_RE = re.compile(
    rf"^exile the top (?P<n>{_N}) cards of your library\. Until the end of "
    r"your next turn, you may cast an? (?P<desc>.+?) spell from among those "
    r"exiled cards$"
)


def _exile_top_cast_one(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
    if card_filter is None:
        return None
    return rw.ExileTopMayCastOneFromAmongUntilEndOfNextTurn(
        count=_n(m.group("n")), card_filter=card_filter,
    )


# "deals X damage to each of up to <N> targets" (Chandra, Hope's
# Beacon's -X; X is the loyalty X).
_X_DAMAGE_EACH_OF_UP_TO_N_RE = re.compile(
    rf"^deals X damage to each of up to (?P<n>{_N}) targets$"
)


def _x_damage_each_of_up_to_n(m):
    from mtgz_ir.effects.formulas import ResolvingX
    from mtgz_ir.effects.generic.cube_creatures_rmc import DealDamageByFormula
    from mtgz_ir.effects.targets import TargetSpec, any_target_filter

    slot = TargetSpec(any_target_filter(), count_min=0, count_max=1)
    return DealDamageByFormula(
        amount=ResolvingX(), targets=(slot,) * _n(m.group("n")),
    )


# "exchange control of two target <np> that share a card type" (Bilbo,
# Luckwearer // Burglar's Plot).
_EXCHANGE_TWO_SHARING_TYPE_RE = re.compile(
    r"^exchange control of two target (?P<np>.+?) that share a card type$"
)


def _exchange_two_sharing_type(m):
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_walkers as rw
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(m.group("np"), plural=True)
    if target_filter is None:
        return None
    spec = TargetSpec(filter=target_filter)
    return rw.ExchangeControlOfTwoTargetsSharingCardType(targets=(spec, spec))


# "gain control of target <desc> spell. You may choose new targets for
# it" (Commandeer).
_GAIN_CONTROL_OF_SPELL_RE = re.compile(
    r"^gain control of target (?P<desc>.+?) spell\. You may choose new "
    r"targets for it$"
)


def _gain_control_of_spell(m):
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import roto_walkers as rw

    card_filter = parse_card_descriptor(f"a {m.group('desc')} card")
    if card_filter is None:
        return None
    return rw.GainControlOfTargetSpell(card_filter=card_filter)


_PHRASE_RECOGNIZERS_3 = (
    (_GAIN_CONTROL_OF_SPELL_RE, _gain_control_of_spell),
    (_EXCHANGE_TWO_SHARING_TYPE_RE, _exchange_two_sharing_type),
    (_COPY_CAST_SPELL_RE, _copy_cast_spell),
    (_EXILE_TOP_CAST_ONE_RE, _exile_top_cast_one),
    (_X_DAMAGE_EACH_OF_UP_TO_N_RE, _x_damage_each_of_up_to_n),
    (_RANDOM_GRAVEYARD_TOKEN_COPY_RE, _random_graveyard_token_copy),
    (_BOUNCE_CHAIN_COPY_RE, _bounce_chain_copy),
    (_CONTROL_OPPONENT_NEXT_TURN_RE, _control_opponent_next_turn),
    (_TOKEN_COPY_OF_TARGET_EXCEPT_RE, _token_copy_of_target_except),
    (_EXILE_THEN_COPY_UNLESS_AURA_RE, _exile_then_copy_unless_aura),
    (_SEARCH_DIFFERENT_NAMES_ONTO_BATTLEFIELD_RE,
     _search_different_names_onto_battlefield),
)
