"""Parser recognizers for the cube "glue" bucket.

Effect phrases (``parse_glue_effect``) are the very last fallback of
``effects._parse_effect``, so no phrase an older recognizer claims
changes meaning. Like the cube creature buckets, this grammar is
switched off for printings that exist only in the EoE-era dumps (the
shared ``creature_rmc_grammar`` switch), so no EoE printing's parse can
change. Every recognizer is parameterized over the varying parts of its
printed shape; engine effects live in ``effects/generic/cube_glue.py``.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect


def _disabled() -> bool:
    from mtgz_ir.cards.parser.creature_rmc_grammar import _DISABLED

    return _DISABLED[0]


# "return the exiled card to its owner's hand" / "return the exiled
# cards to their owners' hands" -- the consumer half of a linked exile
# pair (Tidehollow Sculler).
_RETURN_EXILED_TO_HAND_RE = re.compile(
    r"^return the exiled cards? to (?:its owner's hand|their owners' hands)$"
)


# "choose target opponent and up to one target creature they control.
# They reveal their hand. You may exile <a card descriptor> from their
# hand or the chosen creature until this creature leaves the
# battlefield" (Cloak and Dagger, Entwined; the plural self-name is
# rewritten to "this creature leaves" by the trigger normalizer).
_REVEAL_MAY_EXILE_CARD_OR_CREATURE_RE = re.compile(
    r"^choose target opponent and up to one target creature they control\. "
    r"They reveal their hand\. You may exile (?P<desc>.+?) from their hand "
    r"or the chosen creature until this (?:creature|permanent) leaves the "
    r"battlefield$"
)


# "return [up to one ]target <card descriptor> card from your graveyard to
# your hand" for descriptors the older graveyard-return shapes don't
# cover ("land card" with "up to one", "card", "instant or sorcery card").
_RETURN_GRAVEYARD_CARD_TO_HAND_RE = re.compile(
    r"^return (?P<upto>up to one )?target (?P<desc>(?:.+? )?card) from your "
    r"graveyard to your hand$"
)


# "exile the top card of your library. You may cast that card. If you
# don't, [this planeswalker ]<untargeted effect>" (Chandra, Torch of
# Defiance). The source subject of the fallback sentence is the
# planeswalker builder's mid-sentence self-name rewrite.
_EXILE_TOP_MAY_CAST_ELSE_RE = re.compile(
    r"^exile the top card of your library\. You may cast that card\. If you "
    r"don't, (?:this (?:planeswalker|creature|permanent) )?(?P<else>.+)$"
)


# "<counter target spell>. At the beginning of your next main phase, add
# an amount of {C} equal to that spell's mana value" (Mana Drain) -- the
# counter sentence parses through the unified grammar.
_COUNTER_THEN_NEXT_MAIN_MANA_RE = re.compile(
    r"^(?P<counter>counter target .+?)\. At the beginning of your next main "
    r"phase, add an amount of \{(?P<c>[WUBRGC])\} equal to that spell's mana "
    r"value$"
)
# "remove all <kind> counters from this <permanent>. Add one mana of any
# color for each <kind> counter removed this way" (Coalition Relic).
_REMOVE_ALL_COUNTERS_ADD_MANA_RE = re.compile(
    r"^remove all (?P<kind>[a-z]+) counters from this (?:artifact|creature|"
    r"permanent|land|enchantment)\. Add one mana of any color for each "
    r"(?P<kind2>[a-z]+) counter removed this way$"
)
# "choose a [nonland ]card name, then reveal the top <N> cards of your
# library. Put all cards with the chosen name from among them into your
# hand and the rest into your graveyard" (Tamiyo, Collector of Tales).
_CHOOSE_NAME_REVEAL_TOP_RE = re.compile(
    r"^choose a (?P<nonland>nonland )?card name, then reveal the top "
    r"(?P<n>\w+) cards of your library\. Put all cards with the chosen name "
    r"from among them into your hand and the rest into your graveyard$"
)


# ---------------------------------------------------------------------------
# Planeswalker lines (statics the planeswalker builder doesn't know)
# ---------------------------------------------------------------------------

# Tamiyo, Collector of Tales -- the only card in the pool with this
# protection; a literal until a second printing varies which actions it
# forbids (broader grammar deferred until a second example exists).
_OPPONENTS_CANT_MAKE_YOU_DISCARD_OR_SACRIFICE = (
    "Spells and abilities your opponents control can't cause you to "
    "discard cards or sacrifice permanents."
)


# "Whenever you tap a <Subtype> for mana, add an additional {<C>}."
# (Nissa, Who Shakes the World) -- a triggered mana ability folded into
# automatic payment (``actions.extra_mana_grants_for_tapped_lands``).
_EXTRA_MANA_ON_TAP_RE = re.compile(
    r"^Whenever you tap an? (?P<sub>[A-Z][a-z]+) for mana, add an additional "
    r"\{(?P<c>[WUBRGC])\}\.$"
)
# "During your turn, as long as <self> has one or more loyalty counters on
# <pronoun>, <pronoun>'s a P/T <Subtypes> creature[ and has <keywords>]."
# (Kaito, Bane of Nightmares).
_CREATURE_DURING_YOUR_TURN_RE = re.compile(
    r"^During your turn, as long as (?P<self>.+?) has one or more loyalty "
    r"counters on (?:him|her|it|them), (?:he|she|it|they)(?:'s| are) an? "
    r"(?P<p>\d+)/(?P<t>\d+) (?P<subs>[A-Z][a-z]+(?: [A-Z][a-z]+)*) creature"
    r"(?: and (?:has|have) (?P<kw>[a-z ,]+))?\.$"
)


def _is_self_reference(text: str, name: str) -> bool:
    short = name.split(",", 1)[0]
    return text in (name, short, "this planeswalker")


def parse_glue_walker_line(line: str, name: str) -> dict | None:
    """``CardPrinting`` field overrides for a planeswalker line the
    planeswalker builder declined, or ``None``: Tamiyo's protection,
    Nissa's extra mana, Kaito's creature-during-your-turn static and
    his Ninjutsu hand ability."""
    if _disabled():
        return None
    if line == _OPPONENTS_CANT_MAKE_YOU_DISCARD_OR_SACRIFICE:
        return {"opponents_cant_make_you_discard_or_sacrifice": True}
    m = _EXTRA_MANA_ON_TAP_RE.match(line)
    if m is not None:
        return {
            "extra_mana_when_you_tap_land_subtype": (
                (m.group("sub"), m.group("c")),
            ),
        }
    m = _CREATURE_DURING_YOUR_TURN_RE.match(line)
    if m is not None and _is_self_reference(m.group("self"), name):
        kws: frozenset = frozenset()
        if m.group("kw"):
            from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

            kws = frozenset(_parse_keyword_grant_body(m.group("kw")) or ())
            if not kws:
                return None
        return {
            "creature_during_your_turn": (
                int(m.group("p")), int(m.group("t")),
                tuple(m.group("subs").split(" ")), kws,
            ),
        }
    from mtgz_ir.cards.parser.cube_creatures_ub import _parse_ninjutsu_line

    ninjutsu = _parse_ninjutsu_line(line)
    if ninjutsu is not None:
        return {"hand_activated_abilities": tuple(ninjutsu.hand_abilities)}
    return None


# ---------------------------------------------------------------------------
# Emblems ("You get an emblem with '<body>'.")
# ---------------------------------------------------------------------------

# "<Types> cards in your graveyard have retrace." (Wrenn and Six).
_RETRACE_GRANT_RE = re.compile(
    r"^(?P<types>[A-Z][a-z]+(?: (?:and|or) [a-z]+)*) cards in your graveyard "
    r"have retrace\.$"
)
# "<Card type>s you control have <keywords>." for a noncreature card type
# (Nissa, Who Shakes the World: "Lands you control have indestructible.").
_CARD_TYPE_KEYWORD_ANTHEM_RE = re.compile(
    r"^(?P<type>Lands|Artifacts|Enchantments|Planeswalkers) you control have "
    r"(?P<kw>[a-z ,]+)\.$"
)
_CARD_TYPE_SINGULAR = {
    "Lands": "Land", "Artifacts": "Artifact", "Enchantments": "Enchantment",
    "Planeswalkers": "Planeswalker",
}


def parse_glue_emblem(body: str):
    """``CreateEmblemWithAbilities`` for an emblem body the planeswalker
    bucket's trigger path declined, or ``None``:

    * a triggered ability naming "this emblem" as its source
      ("Whenever you cast a spell, this emblem deals 5 damage to any
      target." -- Chandra, Torch of Defiance);
    * "<Types> cards in your graveyard have retrace." -> the emblem's
      ``grants_retrace_to`` printing field;
    * a subtype anthem ("Ninjas you control get +1/+1.") or a noncreature
      card-type keyword anthem ("Lands you control have
      indestructible.") -> the emblem's static abilities (emblems are
      static sources for the layered walks).
    """
    if _disabled():
        return None
    from mtgz_ir.effects.generic.cube_walkers import CreateEmblemWithAbilities

    name = f'emblem: "{body}"'
    if "this emblem" in body:
        from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line

        parsed = _parse_triggered_ability_line(body, name="this emblem")
        if parsed is None:
            return None
        arms = parsed if isinstance(parsed, tuple) else (parsed,)
        return CreateEmblemWithAbilities(name=name, triggered_abilities=arms)
    m = _RETRACE_GRANT_RE.match(body)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        desc = m.group("types").lower().replace(" and ", " or ")
        f = parse_card_descriptor(f"an {desc} card")
        if f is None or not f.any_types:
            return None
        return CreateEmblemWithAbilities(name=name, grants_retrace_to=f)
    m = _CARD_TYPE_KEYWORD_ANTHEM_RE.match(body)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body
        from mtgz_ir.effects.base import StaticAbility, StaticScope

        kws = _parse_keyword_grant_body(m.group("kw"))
        if not kws:
            return None
        return CreateEmblemWithAbilities(name=name, static_abilities=(
            StaticAbility(
                scope=StaticScope.card_type_you_control(
                    _CARD_TYPE_SINGULAR[m.group("type")],
                ),
                granted_keywords=frozenset(kws),
            ),
        ))
    from mtgz_ir.cards.parser.creature_rmc_grammar import (
        _SUBTYPE_LORD,
        parse_rmc_creature_line_post,
    )

    if _SUBTYPE_LORD.match(body):
        line = parse_rmc_creature_line_post(body, "this emblem")
        if line is not None and line.statics:
            return CreateEmblemWithAbilities(
                name=name, static_abilities=tuple(line.statics),
            )
    return None


_NUMBER_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10, "fifteen": 15,
    "twenty": 20, "thirty": 30,
}


def _number(token: str) -> int | None:
    if token.isdigit():
        return int(token)
    return _NUMBER_WORDS.get(token)


# "you may play the exiled card without paying its mana cost if a library
# has <N> or fewer cards in it" (Shelldock Isle; the Hideaway payoff).
_PLAY_EXILED_FREE_IF_LIBRARY_RE = re.compile(
    r"^you may play the exiled card without paying its mana cost if a "
    r"library has (?P<n>\w+) or fewer cards in it$"
)

# "Hideaway N" (CR 702.75a; reminder text already stripped).
_HIDEAWAY_RE = re.compile(r"^Hideaway (?P<n>\d+)$")


def parse_glue_keyword_trigger(line: str):
    """The triggered ability a keyword line stands for (``Hideaway N`` ->
    its enters trigger), or ``None``."""
    if _disabled():
        return None
    m = _HIDEAWAY_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.cube_glue import HideawayLookExileOneFaceDown
    from mtgz_ir.effects.predicates import Event, IsSelf

    return TriggeredAbility(
        event=Event.ENTERS, predicate=IsSelf(),
        effect=HideawayLookExileOneFaceDown(n=int(m.group("n"))),
    )


# "target <np> [loses all abilities and ]becomes a <color> <Subtypes>
# creature with base power and toughness P/T" -- an indefinite change
# (Oko, Thief of Crowns' +1).
_TARGET_BECOMES_CREATURE_RE = re.compile(
    r"^target (?P<np>.+?) (?P<lose>loses all abilities and )?becomes an? "
    r"(?P<color>white|blue|black|red|green) "
    r"(?P<subs>[A-Z][a-z]+(?: [A-Z][a-z]+)*) creature with base power and "
    r"toughness (?P<p>\d+)/(?P<t>\d+)$"
)
_COLOR_LETTER = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
# "exchange control of target <np> and target <np>" (Oko's -5).
_EXCHANGE_CONTROL_RE = re.compile(
    r"^exchange control of target (?P<a>.+?) and target (?P<b>.+)$"
)


# "search your library for any number of <desc> cards, exile them, then
# shuffle. Until end of turn, you may cast those cards without paying
# their mana costs" (Ugin, Eye of the Storms' -11).
_SEARCH_ANY_NUMBER_EXILE_CAST_FREE_RE = re.compile(
    r"^search your library for any number of (?P<desc>[a-z ,]+?) cards, "
    r"exile them, then shuffle\. Until end of turn, you may cast those "
    r"cards without paying their mana costs$"
)


# "put <N> <kind> counters on up to one target <np>. Untap it. It becomes
# a P/T <Subtypes> creature[ with <keywords>] that's still a <type>"
# (Nissa, Who Shakes the World's +1).
_PUT_COUNTERS_UNTAP_BECOMES_RE = re.compile(
    r"^put (?P<n>\w+) (?P<kind>\+1/\+1|[a-z]+) counters? on up to one target "
    r"(?P<np>.+?)\. Untap it\. It becomes an? (?P<p>\d+)/(?P<t>\d+) "
    r"(?P<subs>[A-Z][a-z]+(?: [A-Z][a-z]+)*) creature(?: with "
    r"(?P<kw>[a-z ,]+?))? that's still an? (?:land|artifact|enchantment)$"
)
# "search your library for any number of <desc> cards, put them onto the
# battlefield[ tapped], then shuffle" (Nissa's -8).
_SEARCH_ANY_NUMBER_ONTO_BATTLEFIELD_RE = re.compile(
    r"^search your library for any number of (?P<desc>.+?) cards, put them "
    r"onto the battlefield(?P<tapped> tapped)?, then shuffle$"
)
# "you get an emblem with "<body>" <Next sentence>" -- the emblem's closing
# quote hides the sentence boundary from the generic splitter (Nissa's -8).
_EMBLEM_THEN_SENTENCE_RE = re.compile(
    r'^(?P<emblem>you get an emblem with "[^"]+") (?P<rest>[A-Z].+)$'
)
# "<effect>. Then draw a card for each opponent who lost life this turn"
# (Kaito's 0).
_THEN_DRAW_PER_OPPONENT_LOST_LIFE_RE = re.compile(
    r"^(?P<first>.+?)\. Then draw a card for each opponent who lost life "
    r"this turn$"
)
# "tap target <np>. Put <N> stun counters on it" (Kaito's -2).
_TAP_THEN_STUN_RE = re.compile(
    r"^tap target (?P<np>.+?)\. Put (?P<n>\w+) stun counters? on it$"
)


# "<effect>. After this phase, there is an additional combat phase"
# (Fear of Missing Out).
_THEN_ADDITIONAL_COMBAT_RE = re.compile(
    r"^(?P<first>.+?)\. After this phase, there is an additional combat "
    r"phase$"
)


def _sequence(*effects):
    from mtgz_ir.effects.combinators import Sequence

    if any(e is None for e in effects):
        return None
    if sum(1 for e in effects if e.requires_target()) > 1:
        return None
    return Sequence(effects=tuple(effects))


def _lower_first(text: str) -> str:
    return text[0].lower() + text[1:] if text else text


def parse_glue_effect(phrase: str) -> Effect | None:
    """Last-resort effect recognizers for the glue bucket."""
    if _disabled():
        return None
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic import cube_glue as g

    if _RETURN_EXILED_TO_HAND_RE.match(phrase):
        return g.ReturnCardsExiledWithSourceToOwnersHand()
    m = _COUNTER_THEN_NEXT_MAIN_MANA_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        counter = _parse_effect(m.group("counter"))
        if counter is None or counter.target_count_range() != (1, 1):
            return None
        return g.CounterThenDelayedManaEqualToMV(
            counter=counter, color=m.group("c"),
        )
    m = _REMOVE_ALL_COUNTERS_ADD_MANA_RE.match(phrase)
    if m is not None and m.group("kind") == m.group("kind2"):
        return g.RemoveAllCountersAddManaPerCounter(kind=m.group("kind"))
    m = _PUT_COUNTERS_UNTAP_BECOMES_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
        from mtgz_ir.effects.targets import TargetSpec

        n = _number(m.group("n"))
        pred = parse_permanent_phrase(m.group("np"))
        kws: frozenset = frozenset()
        if m.group("kw"):
            from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

            kws = frozenset(
                _parse_keyword_grant_body(m.group("kw").replace(", and ", " and "))
                or ()
            )
            if not kws:
                return None
        if n is None or pred is None:
            return None
        return g.PutCountersUntapItBecomesCreature(
            targets=(TargetSpec(filter=pred, count_min=0, count_max=1),),
            counter_kind=m.group("kind"), counter_count=n,
            base_pt=(int(m.group("p")), int(m.group("t"))),
            subtypes=tuple(m.group("subs").split(" ")), keywords=kws,
        )
    m = _SEARCH_ANY_NUMBER_ONTO_BATTLEFIELD_RE.match(phrase)
    if m is not None:
        desc = m.group("desc")
        art = "an" if desc[0].lower() in "aeiou" else "a"
        f = parse_card_descriptor(f"{art} {desc} card")
        if f is None:
            return None
        return g.SearchAnyNumberOntoBattlefield(
            card_filter=f, tapped=bool(m.group("tapped")),
        )
    m = _EMBLEM_THEN_SENTENCE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        rest = m.group("rest")
        if rest.endswith("."):
            rest = rest[:-1]
        return _sequence(
            _parse_effect(m.group("emblem")), _parse_effect(_lower_first(rest)),
        )
    m = _THEN_ADDITIONAL_COMBAT_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        return _sequence(
            _parse_effect(m.group("first")),
            g.AdditionalCombatPhaseAfterThisPhase(),
        )
    m = _THEN_DRAW_PER_OPPONENT_LOST_LIFE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        return _sequence(
            _parse_effect(m.group("first")), g.DrawPerOpponentWhoLostLifeThisTurn(),
        )
    m = _TAP_THEN_STUN_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _target_spec

        n = _number(m.group("n"))
        spec = _target_spec(m.group("np"), up_to_one=False)
        if n is None or spec is None:
            return None
        return g.TapTargetPutStunCounters(targets=(spec,), count=n)
    m = _TARGET_BECOMES_CREATURE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _target_spec

        spec = _target_spec(m.group("np"), up_to_one=False)
        if spec is None:
            return None
        return g.TargetBecomesCreatureIndefinitely(
            targets=(spec,), lose_all_abilities=bool(m.group("lose")),
            colors=frozenset({_COLOR_LETTER[m.group("color")]}),
            creature_subtypes=tuple(m.group("subs").split(" ")),
            base_pt=(int(m.group("p")), int(m.group("t"))),
        )
    m = _EXCHANGE_CONTROL_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _target_spec

        a = _target_spec(m.group("a"), up_to_one=False)
        b = _target_spec(m.group("b"), up_to_one=False)
        if a is None or b is None:
            return None
        return g.ExchangeControlOfTargets(targets=(a, b))
    m = _SEARCH_ANY_NUMBER_EXILE_CAST_FREE_RE.match(phrase)
    if m is not None:
        desc = m.group("desc")
        colorless = desc == "colorless" or desc.startswith("colorless ")
        if colorless:
            desc = desc[len("colorless"):].strip()
        f = (
            parse_card_descriptor(f"a {desc} card") if desc
            else parse_card_descriptor("a card")
        )
        if f is None:
            return None
        return g.SearchAnyNumberExileCastFreeThisTurn(
            card_filter=f, colorless=colorless,
        )
    m = _PLAY_EXILED_FREE_IF_LIBRARY_RE.match(phrase)
    if m is not None:
        n = _number(m.group("n"))
        if n is None:
            return None
        return g.PlayExiledCardFreeIfAnyLibraryAtMost(library_at_most=n)
    m = _CHOOSE_NAME_REVEAL_TOP_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import _amount

        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        return g.ChooseNameRevealTopPutNamedIntoHand(
            count=n, nonland=bool(m.group("nonland")),
        )
    m = _EXILE_TOP_MAY_CAST_ELSE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        otherwise = _parse_effect(m.group("else"))
        if otherwise is None or otherwise.requires_target():
            return None
        return g.ExileTopCardMayCastOtherwise(otherwise=otherwise)
    m = _RETURN_GRAVEYARD_CARD_TO_HAND_RE.match(phrase)
    if m is not None:
        desc = m.group("desc")
        if not desc.startswith(("a ", "an ")):
            desc = ("an " if desc[0] in "aeiou" else "a ") + desc
        f = parse_card_descriptor(desc)
        if f is None:
            return None
        return g.ReturnTargetGraveyardCardToHand(
            card_filter=f, optional=bool(m.group("upto")),
        )
    m = _REVEAL_MAY_EXILE_CARD_OR_CREATURE_RE.match(phrase)
    if m is not None:
        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        return g.RevealHandMayExileCardOrChosenCreatureUntilSourceLeaves(
            card_filter=f,
        )
    return None


# ---------------------------------------------------------------------------
# Die-roll loyalty abilities (Comet, Stellar Pup)
# ---------------------------------------------------------------------------

# Loyalty body "Roll a <N>-sided die." / "Roll a d<N>."
_ROLL_DIE_BODY_RE = re.compile(
    r"^Roll an? (?:(?P<word>\w+)-sided die|d(?P<n>\d+))\.$"
)
# A result-table line: "<lo>[ or <hi>|-<hi>] — <row body>".
_DIE_ROW_LINE_RE = re.compile(
    r"^(?P<lo>\d+)(?:(?: or |[-–])(?P<hi>\d+))? — (?P<body>.+)$"
)
# "[+N]" / "[-N]" inside a row, leading ("[+2], then <eff>" /
# "[+1], and <eff>") or trailing ("<eff>, then [-2]").
_LOYALTY_BRACKET = r"\[(?P<sign>[+\-−])(?P<k>\d+)\]"
_ROW_LEADING_LOYALTY_RE = re.compile(
    r"^" + _LOYALTY_BRACKET + r"(?:, (?:then|and) (?P<rest>.+))?$"
)
_ROW_TRAILING_LOYALTY_RE = re.compile(
    r"^(?P<rest>.+), then " + _LOYALTY_BRACKET + r"$"
)
# "<create tokens>. They gain <keywords> until end of turn"
_CREATE_THEN_THEY_GAIN_RE = re.compile(
    r"^(?P<create>create .+?)\. They gain (?P<kw>[a-z ,]+) until end of turn$"
)
# "return a <descriptor> card from your graveyard to your hand" (untargeted)
_RETURN_A_CARD_FROM_GRAVEYARD_RE = re.compile(
    r"^return (?P<desc>an? (?:.+? )?card(?: with mana value \d+ or less)?) "
    r"from your graveyard to your hand$"
)
# "this planeswalker deals damage equal to the number of loyalty counters
# on <pronoun> to a creature or player"
_LOYALTY_DAMAGE_TO_CHOSEN_RE = re.compile(
    r"^this planeswalker deals damage equal to the number of loyalty "
    r"counters on (?:him|her|it|them|this planeswalker) to a creature or "
    r"player$"
)
# "you may activate this planeswalker's loyalty abilit(y|ies) N more
# times this turn"
_EXTRA_LOYALTY_ACTIVATIONS_RE = re.compile(
    r"^you may activate this planeswalker's loyalty abilit(?:y|ies) "
    r"(?P<n>\w+) more times? this turn$"
)


def parse_glue_die_roll_body(body: str) -> int | None:
    """The number of sides for a "Roll a six-sided die." loyalty body,
    else ``None``."""
    if _disabled():
        return None
    m = _ROLL_DIE_BODY_RE.match(body)
    if m is None:
        return None
    if m.group("n"):
        return int(m.group("n"))
    return _number(m.group("word"))


def _signed(sign: str, k: str) -> int:
    return int(k) if sign == "+" else -int(k)


def _self_rewrite(text: str, name: str) -> str:
    """The walker's own name (full or short) -> "this planeswalker"."""
    short = name.split(",", 1)[0]
    for n in (name, short):
        text = re.sub(r"\b" + re.escape(n) + r"\b", "this planeswalker", text)
    return text


def _parse_row_phrase(phrase: str):
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import cube_glue as g

    if _LOYALTY_DAMAGE_TO_CHOSEN_RE.match(phrase):
        return g.SourceDealsLoyaltyDamageToChosen()
    m = _EXTRA_LOYALTY_ACTIVATIONS_RE.match(phrase)
    if m is not None:
        n = _number(m.group("n"))
        return None if n is None else g.GrantExtraLoyaltyActivationsThisTurn(count=n)
    m = _CREATE_THEN_THEY_GAIN_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

        create = _parse_effect(m.group("create"))
        kws = _parse_keyword_grant_body(m.group("kw"))
        if create is None or not kws:
            return None
        return g.CreateThenTheyGainKeywordsUEOT(
            create=create, keywords=frozenset(kws),
        )
    m = _RETURN_A_CARD_FROM_GRAVEYARD_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        f = parse_card_descriptor(m.group("desc"))
        return None if f is None else g.ReturnChosenGraveyardCardToHand(card_filter=f)
    return _parse_effect(phrase)


def parse_glue_die_row(line: str, name: str):
    """``(lo, hi, effect)`` for one line of a die-roll result table
    ("1 or 2 — [+2], then create ..."), else ``None``. Loyalty brackets
    become ``AdjustSourceLoyalty`` steps in printed order; rows must be
    untargeted (the roll happens on resolution)."""
    if _disabled():
        return None
    m = _DIE_ROW_LINE_RE.match(line)
    if m is None:
        return None
    body = _self_rewrite(m.group("body"), name)
    if body.endswith("."):
        body = body[:-1]
    from mtgz_ir.effects.generic import cube_glue as g

    steps: list = []
    lead = _ROW_LEADING_LOYALTY_RE.match(body)
    trail = _ROW_TRAILING_LOYALTY_RE.match(body)
    if lead is not None:
        steps.append(g.AdjustSourceLoyalty(delta=_signed(lead["sign"], lead["k"])))
        rest = lead.group("rest")
        if rest:
            eff = _parse_row_phrase(_lower_first(rest))
            if eff is None:
                return None
            steps.append(eff)
    elif trail is not None:
        eff = _parse_row_phrase(_lower_first(trail.group("rest")))
        if eff is None:
            return None
        steps.append(eff)
        steps.append(g.AdjustSourceLoyalty(delta=_signed(trail["sign"], trail["k"])))
    else:
        eff = _parse_row_phrase(_lower_first(body))
        if eff is None:
            return None
        steps.append(eff)
    if any(s.target_count_range()[1] > 0 for s in steps):
        return None
    effect = steps[0] if len(steps) == 1 else _sequence(*steps)
    if effect is None:
        return None
    lo = int(m.group("lo"))
    hi = int(m.group("hi") or lo)
    return (lo, hi, effect)


def assemble_die_table(sides: int, rows: list):
    """``RollDieTable`` for a complete result table covering 1..``sides``
    exactly, else ``None``."""
    rows = sorted(rows, key=lambda r: r[0])
    expected = 1
    for lo, hi, _eff in rows:
        if lo != expected or hi < lo:
            return None
        expected = hi + 1
    if expected != sides + 1:
        return None
    from mtgz_ir.effects.generic.cube_creatures_rmc import RollDieTable

    return RollDieTable(sides=sides, table=tuple(rows))
