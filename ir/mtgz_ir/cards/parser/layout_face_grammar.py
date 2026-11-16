"""Phrase grammar for the faces of the cube layouts bucket's cards
(Jace, Vryn's Prodigy; Tamiyo, Inquisitive Student; Ajani, Nacatl
Pariah; Cecil, Dark Knight; Etali, Primal Conqueror; Outland Liberator;
...) -- effect bodies and trigger conditions those faces print that no
other grammar covers. Consulted from
:mod:`mtgz_ir.cards.parser.layout_grammar`'s fallbacks, i.e. after every
other recognizer declined the phrase.

Every recognizer is parameterized over the parts of its printed shape
that vary; inner effects recurse through the unified grammar.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect

_N = r"(?:a|an|one|two|three|four|five|six|seven|eight|nine|ten|\d+)"
_NUMBERS = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
_ORDINALS = {
    "second": 2, "third": 3, "fourth": 4, "fifth": 5, "sixth": 6,
    "seventh": 7,
}
_COLOR = r"(?:white|blue|black|red|green)"
_PRONOUN_SUBJECT = re.compile(r"^(?:he|she|they|it|this creature|this planeswalker) ")


def _n(token: str) -> int:
    return int(token) if token.isdigit() else _NUMBERS[token]


# ---------------------------------------------------------------------------
# Conditions ("if <condition>, <effect>")
# ---------------------------------------------------------------------------

_GRAVEYARD_AT_LEAST = re.compile(
    rf"^there are (?P<n>{_N}) or more cards in your graveyard$"
)
_OTHER_PERMANENT_OF_COLOR = re.compile(
    rf"^you control an? (?P<color>{_COLOR}) permanent other than "
    r"(?:this (?:creature|planeswalker|permanent)|[A-Z][\w',\- ]*)$"
)


def parse_layout_condition(text: str):
    """``Condition`` for a printed condition phrase, or ``None``."""
    from mtgz_ir.effects.generic.layout_face_effects import (
        _COLOR_WORDS,
        CardsInYourGraveyardAtLeast,
        ControlOtherPermanentOfColor,
        LifeAtMostHalfStartingLife,
    )

    m = _GRAVEYARD_AT_LEAST.match(text)
    if m is not None:
        return CardsInYourGraveyardAtLeast(n=_n(m.group("n")))
    if text == (
        "your life total is less than or equal to half your starting life "
        "total"
    ):
        return LifeAtMostHalfStartingLife()
    m = _OTHER_PERMANENT_OF_COLOR.match(text)
    if m is not None:
        return ControlOtherPermanentOfColor(color=_COLOR_WORDS[m.group("color")])
    return None


# ---------------------------------------------------------------------------
# Effects
# ---------------------------------------------------------------------------

_IF_COND = re.compile(r"^if (?P<cond>[^,]+), (?P<eff>.+)$")
# "<first>. When you do, <then>." -- a (non-optional) reflexive trigger.
_WHEN_YOU_DO = re.compile(r"^(?P<first>[^.]+)\. [Ww]hen you do, (?P<then>.+)$")
_DAMAGE_EQUAL_COUNT_YOU_CONTROL = re.compile(
    r"^deals damage equal to the number of (?P<noun>.+?) you control to "
    r"any target$"
)
_TARGET_OPPONENT_MILLS = re.compile(
    rf"^target opponent mills (?P<n>{_N}) cards?$"
)
_GETS_UNTIL_YOUR_NEXT_TURN = re.compile(
    r"^(?P<upto>up to one )?target (?P<noun>.+?) gets (?P<p>[+-]\d+)/"
    r"(?P<t>[+-]\d+) until your next turn$"
)
_CAST_FROM_GRAVEYARD_THIS_TURN = re.compile(
    r"^you may cast target (?P<desc>.+? card) from your graveyard this "
    r"turn\. [Ii]f that spell would be put into your graveyard, exile it "
    r"instead$"
)
_OTHER_ATTACKING_GAIN_UEOT = re.compile(
    r"^other attacking creatures gain (?P<kws>[a-z, ]+) until end of turn$"
)
_COUNTER_ON_EACH_YOU_CONTROL = re.compile(
    r"^put a \+1/\+1 counter on each (?P<noun>.+?) you control$"
)
_RETURN_CARD_TO_HAND = re.compile(
    r"^return target (?P<desc>(?:.+? )?card) from your graveyard to your hand"
    rf"(?:\. [Ii]f it's an? (?P<color>{_COLOR}) card, (?P<bonus>.+))?$"
)
_EMBLEM_WITH_QUOTED = re.compile(r'^you get an emblem with "(?P<body>.+)"$')
_UNTIL_YOUR_NEXT_TURN_WHENEVER = re.compile(
    r"^until your next turn, (?P<trigger>whenever .+)$"
)
_IT_GETS_UEOT = re.compile(
    r"^it gets (?P<p>[+-]\d+)/(?P<t>[+-]\d+) until end of turn$"
)
_CHOOSE_ONE_OF_EACH_SACRIFICE_REST = re.compile(
    r"^each (?P<who>opponent|player) chooses (?P<list>an? [a-z]+(?:, an? "
    r"[a-z]+)*,? and an? [a-z]+) from among the (?P<nonland>nonland )?"
    r"permanents they control, then sacrifices the rest$"
)
_TYPE_WORDS = {
    "artifact": "Artifact", "creature": "Creature",
    "enchantment": "Enchantment", "planeswalker": "Planeswalker",
    "land": "Land", "battle": "Battle",
}


def parse_face_effect(phrase: str) -> Effect | None:
    """Effect for a normalized ``phrase`` of the bucket's face shapes, or
    ``None``."""
    from mtgz_ir.cards.parser.effects import _parse_effect

    m = _IF_COND.match(phrase)
    if m is not None:
        cond = parse_layout_condition(m.group("cond"))
        if cond is None:
            return None
        inner = _parse_effect(_strip_pronoun_subject(m.group("eff")))
        if inner is None:
            return None
        from mtgz_ir.effects.generic.cube_walkers import IfCondition

        return IfCondition(condition=cond, then=inner)
    m = _WHEN_YOU_DO.match(phrase)
    if m is not None:
        first = _parse_effect(m.group("first"))
        then = _parse_effect(m.group("then"))
        if first is None or then is None or first.target_count_range()[1] > 0:
            return None
        from mtgz_ir.effects.generic.cube_creatures_rmc import DoThenReflexive

        # A reflexive triggered ability (CR 603.12) after ``first``. The
        # Python engine chooses its targets with the enclosing ability
        # (the MayDoThenReflexive precedent; see
        # docs/SEARCH_LIMITATIONS.md).
        return DoThenReflexive(first=first, then=then)
    stripped = _strip_pronoun_subject(phrase)
    m = _DAMAGE_EQUAL_COUNT_YOU_CONTROL.match(stripped)
    if m is not None:
        return _damage_equal_count(m.group("noun"))
    m = _TARGET_OPPONENT_MILLS.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.layout_face_effects import TargetOpponentMills

        return TargetOpponentMills(amount=_n(m.group("n")))
    m = _GETS_UNTIL_YOUR_NEXT_TURN.match(phrase)
    if m is not None:
        return _gets_until_next_turn(m)
    m = _CAST_FROM_GRAVEYARD_THIS_TURN.match(phrase)
    if m is not None:
        return _cast_from_graveyard_this_turn(m.group("desc"))
    m = _OTHER_ATTACKING_GAIN_UEOT.match(phrase)
    if m is not None:
        return _other_attacking_gain(m.group("kws"))
    m = _COUNTER_ON_EACH_YOU_CONTROL.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase
        from mtgz_ir.effects.generic.layout_face_effects import (
            PutPlusOneCounterOnEachMatching,
        )

        pred = parse_target_noun_phrase(m.group("noun"))
        if pred is None:
            return None
        return PutPlusOneCounterOnEachMatching(predicate=pred)
    m = _RETURN_CARD_TO_HAND.match(phrase)
    if m is not None:
        return _return_card_to_hand(m)
    if phrase == (
        "draw cards equal to half the number of cards in your library, "
        "rounded up"
    ):
        from mtgz_ir.effects.generic.layout_face_effects import (
            DrawHalfLibraryRoundedUp,
        )

        return DrawHalfLibraryRoundedUp()
    m = _EMBLEM_WITH_QUOTED.match(phrase)
    if m is not None:
        return _emblem_with_static(m.group("body"))
    if phrase == "they get that many poison counters":
        from mtgz_ir.effects.generic.layout_face_effects import (
            PoisonCountersEqualToDamage,
        )

        return PoisonCountersEqualToDamage()
    if phrase == "you lose that much life":
        from mtgz_ir.effects.generic.layout_face_effects import (
            LoseLifeEqualToEventAmount,
        )

        return LoseLifeEqualToEventAmount()
    m = _UNTIL_YOUR_NEXT_TURN_WHENEVER.match(phrase)
    if m is not None:
        return _until_your_next_turn_trigger(m.group("trigger"))
    m = _CHOOSE_ONE_OF_EACH_SACRIFICE_REST.match(phrase)
    if m is not None:
        return _choose_one_of_each(m)
    if phrase == "this creature becomes prepared":
        from mtgz_ir.effects.generic.layout_face_effects import (
            SourceBecomesPrepared,
        )

        return SourceBecomesPrepared()
    if phrase in ("he connives", "she connives", "they connive", "connives"):
        # "<this creature> connives" (CR 701.50) -- the source connives.
        from mtgz_ir.effects.generic.layout_face_effects import SourceConnives

        return SourceConnives()
    if phrase == "airbend up to one other target creature or spell":
        # Airbend (the only airbend target shape in the pool).
        from mtgz_ir.effects.generic.layout_face_effects import Airbend

        return Airbend()
    if phrase == (
        "each player exiles cards from the top of their library until they "
        "exile a nonland card. You may cast any number of spells from among "
        "the nonland cards exiled this way without paying their mana costs"
    ):
        # Etali, Primal Conqueror -- the only card with this shape in the
        # pool; generalization (other stop filters / paid casts) deferred
        # until a second printing varies it.
        from mtgz_ir.effects.generic.layout_face_effects import (
            EachPlayerExilesUntilNonlandMayCastFree,
        )

        return EachPlayerExilesUntilNonlandMayCastFree()
    return None


def _strip_pronoun_subject(text: str) -> str:
    """"he deals ..." / "it deals ..." -> "deals ..." (the source is the
    implicit subject of an ability's damage)."""
    m = _PRONOUN_SUBJECT.match(text)
    if m is not None and text[m.end():].startswith("deals "):
        return text[m.end():]
    return text


def _damage_equal_count(noun: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase
    from mtgz_ir.effects.formulas import CountControlledMatching
    from mtgz_ir.effects.generic.cube_creatures_rmc import DealDamageFromEvent
    from mtgz_ir.effects.targets import TargetSpec, any_target_filter

    if noun.endswith("s"):
        noun = noun[:-1]
    pred = parse_target_noun_phrase(noun)
    if pred is None:
        return None
    return DealDamageFromEvent(
        amount=CountControlledMatching(predicate=pred),
        targets=(TargetSpec(any_target_filter()),),
    )


def _gets_until_next_turn(m: "re.Match") -> Effect | None:
    from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase
    from mtgz_ir.effects.generic.layout_face_effects import (
        PumpTargetUntilYourNextTurn,
    )
    from mtgz_ir.effects.targets import TargetSpec

    pred = parse_target_noun_phrase(m.group("noun"))
    if pred is None:
        return None
    spec = TargetSpec(pred, count_min=0 if m.group("upto") else 1)
    return PumpTargetUntilYourNextTurn(
        power=int(m.group("p")), toughness=int(m.group("t")), targets=(spec,),
    )


def _cast_from_graveyard_this_turn(desc: str) -> Effect | None:
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic.layout_face_effects import (
        MayCastTargetGraveyardCardThisTurnExileInstead,
    )

    f = parse_card_descriptor("an " + desc if desc[0] in "aeiou" else "a " + desc)
    if f is None:
        return None
    return MayCastTargetGraveyardCardThisTurnExileInstead(card_filter=f)


def _other_attacking_gain(kws_text: str) -> Effect | None:
    from mtgz_ir.cards.parser.keywords import _parse_granted_keyword_piece
    from mtgz_ir.effects.generic.pump import PumpCreaturesYouControlUntilEOT

    kws: set[str] = set()
    for piece in kws_text.replace(" and ", ", ").split(", "):
        got = _parse_granted_keyword_piece(piece.strip())
        if not got:
            return None
        kws.update(got)
    # In a two-player game every attacking creature is the attacking
    # (active) player's -- the ability's controller's -- creature.
    return PumpCreaturesYouControlUntilEOT(
        granted_keywords=frozenset(kws), exclude_source=True,
        attacking_only=True,
    )


def _return_card_to_hand(m: "re.Match") -> Effect | None:
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic.layout_face_effects import (
        _COLOR_WORDS,
        ReturnTargetCardFromYourGraveyardToHand,
    )

    desc = m.group("desc")
    f = parse_card_descriptor(("an " if desc[0] in "aeiou" else "a ") + desc)
    if f is None:
        return None
    bonus = None
    color = None
    if m.group("bonus") is not None:
        bonus = _parse_effect(m.group("bonus"))
        if bonus is None or bonus.requires_target():
            return None
        color = _COLOR_WORDS[m.group("color")]
    return ReturnTargetCardFromYourGraveyardToHand(
        card_filter=f, bonus_color=color, bonus=bonus,
    )


def _emblem_with_static(body: str) -> Effect | None:
    from mtgz_ir.cards.parser.statics import _parse_static_ability_line
    from mtgz_ir.effects.generic.layout_face_effects import (
        CreateEmblemWithStatics,
    )

    statics = _parse_static_ability_line(body)
    if not statics:
        return None
    return CreateEmblemWithStatics(static_abilities=tuple(statics))


def _until_your_next_turn_trigger(trigger_text: str) -> Effect | None:
    from mtgz_ir.cards.base import TriggeredAbility
    from mtgz_ir.cards.parser.triggers import (
        _parse_predicate_trigger_phrase,
        _parse_triggered_ability_line,
    )
    from mtgz_ir.effects.generic.layout_face_effects import (
        CreateTriggersUntilYourNextTurn,
        EventSubjectGetsUEOT,
    )

    line = "W" + trigger_text[1:] + "."
    head = re.match(r"^Whenever (?P<cond>.+?), (?P<body>it gets .+)\.$", line)
    ability = None
    if head is not None:
        # "it" is the triggering event's object, not the ability's source.
        gets = _IT_GETS_UEOT.match(head.group("body"))
        cond = _parse_predicate_trigger_phrase(head.group("cond"))
        if gets is None or cond is None:
            return None
        event, predicate = cond
        ability = TriggeredAbility(
            event=event, predicate=predicate,
            effect=EventSubjectGetsUEOT(
                power=int(gets.group("p")), toughness=int(gets.group("t")),
            ),
        )
    else:
        parsed = _parse_triggered_ability_line(line)
        if parsed is None or isinstance(parsed, tuple):
            return None
        ability = parsed
    return CreateTriggersUntilYourNextTurn(triggered_abilities=(ability,))


def _choose_one_of_each(m: "re.Match") -> Effect | None:
    from mtgz_ir.effects.generic.layout_face_effects import (
        EachChoosesOneOfEachTypeSacrificesRest,
    )

    words = re.findall(r"an? ([a-z]+)", m.group("list"))
    types = []
    for w in words:
        t = _TYPE_WORDS.get(w)
        if t is None:
            return None
        types.append(t)
    return EachChoosesOneOfEachTypeSacrificesRest(
        card_types=tuple(types),
        opponents_only=m.group("who") == "opponent",
        nonland_only=bool(m.group("nonland")),
    )


# ---------------------------------------------------------------------------
# Trigger conditions
# ---------------------------------------------------------------------------

_DRAW_NTH_CARD = re.compile(
    r"^you draw your (?P<ord>second|third|fourth|fifth|sixth|seventh) card "
    r"(?:in|each) (?:a )?turn$"
)
_ONE_OR_MORE_OTHER_SUBTYPE_DIE = re.compile(
    r"^one or more other (?P<sub>[A-Z][a-z]+)s you control die$"
)


def parse_face_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for a trigger condition of the bucket's face
    shapes, or ``None``."""
    from mtgz_ir.effects.predicates import (
        OPPONENT,
        YOU,
        Controlled,
        Event,
        IsSelf,
        Subject,
    )

    m = _DRAW_NTH_CARD.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.layout_face_effects import (
            NthCardDrawnThisTurn,
        )

        return (
            Event.DRAWS_CARD,
            Controlled(YOU, subject="player")
            & NthCardDrawnThisTurn(n=_ORDINALS[m.group("ord")]),
        )
    m = _ONE_OR_MORE_OTHER_SUBTYPE_DIE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import OneOrMoreBatch
        from mtgz_ir.effects.object_predicates import HasSubtype, IsCreature

        return (
            Event.DIES,
            Subject(None, IsCreature() & HasSubtype(m.group("sub")))
            & Controlled(YOU) & ~IsSelf() & OneOrMoreBatch(),
        )
    if phrase == "this creature deals damage":
        return (Event.DEALS_DAMAGE, IsSelf(subject="source"))
    if phrase == "a creature attacks you or a planeswalker you control":
        from mtgz_ir.effects.object_predicates import IsCreature

        # Two-player: every attacker an opponent controls attacks you or
        # a planeswalker you control.
        return (
            Event.ATTACKS,
            Subject("attacker", IsCreature())
            & Controlled(OPPONENT, subject="attacker"),
        )
    return None


# ---------------------------------------------------------------------------
# Creature lines (hooked after the W/G creature-line grammar)
# ---------------------------------------------------------------------------

_CANT_BE_BLOCKED_LINE = re.compile(r"^[Tt]his creature can't be blocked\.$")
_MAYHEM_GRANT_LINE = re.compile(
    r"^(?:[A-Z][A-Za-z ]+ — )?Each nonland card in your graveyard has "
    r"mayhem\. The mayhem cost is equal to its mana cost\.$"
)


def parse_face_creature_line(line: str, card: dict):
    """A ``WGLine``-shaped contribution for a creature line of the
    bucket's faces, or ``None``."""
    from mtgz_ir.cards.parser.creature_wg_lines import WGLine
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    line = _substitute_self_name(line, card["name"])
    if _CANT_BE_BLOCKED_LINE.match(line):
        from mtgz_ir.cards.base import StaticAbility, StaticScope
        from mtgz_ir.effects.modifiers import CantBeBlocked, ContinuousModifier

        return WGLine(statics=(StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(layer=6, payload=CantBeBlocked()),
        ),))
    if _MAYHEM_GRANT_LINE.match(line):
        return WGLine(fields={"grants_mayhem_to_graveyard": True})
    if line in ("This creature enters prepared.", "this creature enters prepared."):
        return WGLine(fields={"enters_prepared": True})
    if line in (
        "This creature can block only creatures with flying.",
        "this creature can block only creatures with flying.",
    ):
        return WGLine(fields={"can_only_block_creatures_with_flying": True})
    return None


# ---------------------------------------------------------------------------
# Intervening-if gates
# ---------------------------------------------------------------------------

_YOU_CONTROL_N_OR_MORE = re.compile(
    rf"^you control (?P<n>{_N}) or more (?P<noun>lands|artifacts|creatures|"
    r"enchantments)$"
)


def parse_face_intervening_if(phrase: str):
    """Trigger-time gate predicate for "if you control N or more <type>s"
    (Emeritus of Abundance), or ``None``."""
    m = _YOU_CONTROL_N_OR_MORE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.effects.generic.layout_face_effects import YouControlAtLeast
    from mtgz_ir.effects.predicates import StateBound

    return StateBound(YouControlAtLeast(
        n=_n(m.group("n")), card_type=m.group("noun")[:-1].capitalize(),
    ))
