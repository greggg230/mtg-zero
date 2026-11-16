"""Effect grammar for card-flow spell shapes: targeted draw, tutors,
hand disruption, library look-and-select, wheels, mass zone moves and
a few mana / library primitives.

Hooked into ``effects._parse_effect`` as the LAST dispatch step (after
every pre-existing recognizer), so no phrase an older recognizer
already claims changes meaning; these recognizers only claim phrases
nothing else did. Everything here is parameterized — the varying parts
of a printed shape (amounts, card descriptors, destinations, the
"may" rider) are capture groups — and multi-sentence shapes recurse
into ``_parse_effect`` for their trailing sentences, so a new card with
the same shape costs zero code.

Every phrase reaching this module is *normalized* (see
``effects._normalize_spell_effect_phrase``): no trailing period and a
lower-cased first letter. Later sentences inside a multi-sentence
phrase keep their original capitalization, hence the ``[Yy]ou`` style
alternations.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.generic.card_flow import (
    ANY_CARD,
    CardFilter,
    EachPlayerDiscardsHand,
    EachPlayerShufflesZonesIntoLibrary,
    ExileThisSpell,
    ShuffleThisSpellIntoOwnersLibrary,
    LookAtTargetPlayersHand,
    LookAtTopCardsPutIntoHand,
    ReorderTopCards,
    ReturnAllPermanentsToOwnersHands,
    ReturnTargetSpellToOwnersHand,
    SearchLibrary,
    ShuffleYourLibrary,
    TargetPlayerDiscardsAtRandom,
    TargetPlayerDraws,
    TargetPlayerRevealsHandYouChooseDiscard,
    TargetPlayerTakesExtraTurn,
    UntapUpToNLands,
)


_CARDINALS: dict[str, int] = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4,
    "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
_AMT = r"(?:\d+|a|an|one|two|three|four|five|six|seven|eight|nine|ten|X)"


def _amount(token: str):
    """Digit / spelled cardinal -> int; ``X`` -> ``ResolvingX()``."""
    if token == "X":
        from mtgz_ir.effects.formulas import ResolvingX

        return ResolvingX()
    if token.isdigit():
        return int(token)
    return _CARDINALS[token]


# ---------------------------------------------------------------------
# Card-descriptor grammar ("a green creature card", "a noncreature,
# nonland card", "an instant or sorcery card", ...)
# ---------------------------------------------------------------------

_TYPE_WORDS: dict[str, str] = {
    "artifact": "Artifact", "creature": "Creature",
    "enchantment": "Enchantment", "instant": "Instant",
    "sorcery": "Sorcery", "land": "Land", "planeswalker": "Planeswalker",
    "battle": "Battle", "permanent": "Permanent",
}
_COLOR_WORDS: dict[str, str] = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
_SUPERTYPE_WORDS: dict[str, str] = {"legendary": "Legendary", "basic": "Basic"}

_DESCRIPTOR_RE = re.compile(
    r"^(?:a|an) (?:(?P<body>.+?) )?card"
    r"(?: with mana value (?P<mv>\d+) or less"
    # "with mana cost {0} or {1}" (Urza's Saga): the printed mana cost.
    r"| with mana cost (?P<costs>\{[^}]+\}(?: or \{[^}]+\})*))?$"
)
_PERMANENT_DESCRIPTOR_RE = re.compile(r"^(?:a|an) (?P<body>.+)$")
_SUBTYPE_WORD_RE = re.compile(r"^[A-Z][a-z]+$")


def _parse_descriptor_body(body: str) -> CardFilter | None:
    """Parse the adjective / type words of a card descriptor.

    Comma-separated parts are conjunctive ("noncreature, nonland"); a
    part may hold an ``or`` alternation of types ("instant or
    sorcery"). Recognized words: ``non<type>`` exclusions, colors,
    ``legendary``, and card types (``permanent`` is a pseudo-type).
    Anything else rejects the descriptor.
    """
    any_types: list[str] = []
    excluded: list[str] = []
    color = None
    supertype = None
    subtype = None
    for part in body.split(", "):
        alternatives = part.split(" or ")
        if len(alternatives) > 1:
            for alt in alternatives:
                t = _TYPE_WORDS.get(alt)
                if t is None:
                    return None
                any_types.append(t)
            continue
        for word in part.split(" "):
            if word.startswith("non") and word[3:] in _TYPE_WORDS:
                excluded.append(_TYPE_WORDS[word[3:]])
            elif word in _COLOR_WORDS and color is None:
                color = _COLOR_WORDS[word]
            elif word in _SUPERTYPE_WORDS and supertype is None:
                supertype = _SUPERTYPE_WORDS[word]
            elif word in _TYPE_WORDS and not any_types:
                any_types.append(_TYPE_WORDS[word])
            elif _SUBTYPE_WORD_RE.match(word) and subtype is None:
                # "an Equipment card", "a Wolf" (cube W/G creatures).
                subtype = word
            else:
                return None
    return CardFilter(
        any_types=tuple(any_types),
        excluded_types=tuple(excluded),
        color=color,
        supertype=supertype,
        subtype=subtype,
    )


def parse_card_descriptor(text: str) -> CardFilter | None:
    """``a card`` / ``an artifact card`` / ``a nonland card with mana
    value 3 or less`` ... -> ``CardFilter`` (``None`` if unrecognized)."""
    m = _DESCRIPTOR_RE.match(text)
    if m is None:
        return None
    body = m.group("body")
    f = ANY_CARD if body is None else _parse_descriptor_body(body)
    if f is None:
        return None
    if m.group("mv") is not None:
        f = CardFilter(
            any_types=f.any_types, excluded_types=f.excluded_types,
            max_mv=int(m.group("mv")), color=f.color, supertype=f.supertype,
            subtype=f.subtype,
        )
    if m.group("costs") is not None:
        import dataclasses

        f = dataclasses.replace(
            f, mana_costs=tuple(m.group("costs").split(" or ")),
        )
    return f


def parse_permanent_descriptor(text: str) -> CardFilter | None:
    """``a land`` / ``an artifact`` / ``a green creature`` (permanent
    descriptors in sacrifice costs) -> ``CardFilter``."""
    m = _PERMANENT_DESCRIPTOR_RE.match(text)
    if m is None or m.group("body").endswith(" card"):
        return None
    f = _parse_descriptor_body(m.group("body"))
    if f is None or not (f.any_types or f.excluded_types):
        return None
    return f


# ---------------------------------------------------------------------
# Recognizers
# ---------------------------------------------------------------------

_TARGET_PLAYER_DRAWS_RE = re.compile(
    rf"^target player draws (?P<n>{_AMT}) cards?$"
)
_TARGET_PLAYER_EXTRA_TURN_RE = re.compile(
    r"^target player takes an extra turn after this one$"
)
# "Target player discards N cards[. That player may copy this spell and
# may choose a new target for that copy]." (Chain of Smog.)
_TARGET_PLAYER_DISCARDS_RE = re.compile(
    rf"^target player discards (?P<n>{_AMT}) cards?"
    r"(?P<copy>\. [Tt]hat player may copy this spell and may choose a new "
    r"target for that copy)?$"
)
_TARGET_PLAYER_DISCARDS_AT_RANDOM_RE = re.compile(
    rf"^target player discards (?P<n>{_AMT}) cards? at random$"
)
# Thoughtseize / Inquisition / Duress — three printed sentences, with an
# optional trailing sentence (Thoughtseize's "You lose 2 life.") parsed
# recursively.
_REVEAL_HAND_YOU_CHOOSE_DISCARD_RE = re.compile(
    r"^target (?P<who>player|opponent) reveals their hand\. "
    r"[Yy]ou (?P<may>may )?choose (?P<desc>.+?) from it"
    r"(?P<mv> with mana value \d+ or less)?\. "
    # "You may choose ... If you do, that player discards that card, then
    # draws a card." (Revealing Eye) -- the optional arm.
    r"(?(may)[Ii]f you do, that player|[Tt]hat player) discards that card"
    rf"(?(may)(?:, then draws (?P<draws>{_AMT}) cards?)?|)"
    r"(?:\. (?P<rest>.+))?$"
)
# "Target opponent reveals their hand and you choose a nonland card from
# it. Exile that card." (Tidehollow Sculler) -- the one-sentence reveal +
# choose with an exile instead of a discard.
_REVEAL_HAND_YOU_CHOOSE_EXILE_RE = re.compile(
    r"^target (?P<who>player|opponent) reveals their hand and you choose "
    r"(?P<desc>.+?) from it\. [Ee]xile that card$"
)
_SEARCH_TO_ZONE_RE = re.compile(
    r"^search your library for (?P<desc>.+? card(?: with mana (?:value "
    r"\d+ or less|cost \{[^}]+\}(?: or \{[^}]+\})*))?)"
    r"(?P<reveal>, reveal (?:it|that card))?, "
    r"put (?:that card|it) (?P<dest>into your hand|into your graveyard|"
    r"onto the battlefield)(?P<tapped> tapped)?, then shuffle$"
)
_SEARCH_TO_TOP_RE = re.compile(
    r"^search your library for (?P<desc>.+? card)(?P<reveal>, reveal it)?, "
    r"then shuffle and put (?:that|the) card on top$"
)
# Look at / reveal the top N, then a selection sentence (and optionally a
# "put the rest ..." sentence), then optional trailing sentences.
_LOOK_TOP_N_HEAD_RE = re.compile(
    rf"^(?P<verb>look at|reveal) the top (?P<n>{_AMT}) cards of your library"
    r"(?:, where X is (?P<xdef>.+?))?\. (?P<sel>.+)$"
)
_SEL_PUT_K_REST_RE = re.compile(
    rf"^[Pp]ut (?P<k>{_AMT}) of them into your hand and the rest "
    r"(?P<rest>on the bottom of your library in (?:any|a random) order|"
    r"into your graveyard)(?:\. (?P<tail>.+))?$"
)
_SEL_MAY_TAKE_RE = re.compile(
    r"^[Yy]ou may (?:reveal (?P<desc_r>.+? card) from among them and put it "
    r"into your hand|put (?P<desc_p>.+? card) from among them into your hand)"
    r"\. [Pp]ut the rest (?P<rest>on the bottom of your library in "
    r"(?:any|a random) order|into your graveyard)(?:\. (?P<tail>.+))?$"
)
# "Put any number of <type> and/or <type> cards from among them into
# your hand and the rest ..." (Torsten, Founder of Benalia; cube W/G
# creature bucket).
_SEL_ANY_NUMBER_RE = re.compile(
    r"^[Pp]ut any number of (?P<types>[a-z]+(?: (?:and/or|or) [a-z]+)*) "
    r"cards from among them into your hand and the rest "
    r"(?P<rest>on the bottom of your library in (?:any|a random) order|"
    r"into your graveyard)(?:\. (?P<tail>.+))?$"
)
# "..., put K of those cards into your hand. If this spell was kicked,
# put K' of those cards into your hand instead. Put the rest on the
# bottom ..." -- the kicker "instead" rider on the pick count (Consult
# the Star Charts).
_SEL_PUT_K_KICKED_INSTEAD_RE = re.compile(
    rf"^[Pp]ut (?P<k>{_AMT}) of those cards into your hand\. If this spell "
    rf"was kicked, put (?P<kk>{_AMT}) of those cards into your hand instead\. "
    r"[Pp]ut the rest (?P<rest>on the bottom of your library in "
    r"(?:any|a random) order)$"
)
_X_DEFINITION_TWICE_LANDS = "twice the number of lands you control"
_X_DEFINITION_LANDS = "the number of lands you control"
_REORDER_TOP_N_RE = re.compile(
    rf"^look at the top (?P<n>{_AMT}) cards of your library, then put them "
    r"back in any order$"
)
_SHUFFLE_RE = re.compile(r"^(?P<may>you may )?shuffle$")
_EACH_PLAYER_THEN_DRAWS_RE = re.compile(
    r"^each player (?P<act>discards their hand|shuffles their hand and "
    r"graveyard into their library), then draws "
    rf"(?P<n>{_AMT}) cards$"
)
_UNTAP_UP_TO_N_LANDS_RE = re.compile(
    rf"^(?:you )?untap up to (?P<n>{_AMT}) lands$"
)
_RETURN_ALL_PERMANENTS_RE = re.compile(
    r"^return all permanents to their owners' hands$"
)
_RETURN_TARGET_SPELL_TO_HAND_RE = re.compile(
    r"^return target spell to its owner's hand$"
)
_SCRY_N_RE = re.compile(r"^scry (?P<n>\d+)$")
_EXILE_THIS_SPELL_RE = re.compile(r"^exile this spell$")
_SHUFFLE_THIS_SPELL_RE = re.compile(
    r"^shuffle this spell into its owner's library$"
)
_LOOK_AT_TARGET_PLAYERS_HAND_RE = re.compile(r"^look at target player's hand$")
# ``add {B}{B}{B}`` / ``add {U}{R}`` — any run of three or more colored
# (or colorless) pips. One- and two-pip forms are claimed earlier by the
# pre-existing mana recognizers.
_ADD_MANA_PIPS_RE = re.compile(r"^add (?P<pips>(?:\{[WUBRGC]\}){3,})$")
_ADD_N_MANA_ANY_COMBINATION_RE = re.compile(
    rf"^add (?P<n>{_AMT}) mana in any combination of colors$"
)


# Reanimation: base sentence + optional anaphoric rider sentences.
_REANIMATE_RE = re.compile(
    r"^(?:put target (?P<any_desc>.+? card) from a graveyard onto the "
    r"battlefield under your control(?: with an? (?P<counter>[a-z]+) counter "
    r"on it)?|return (?P<upto>up to one )?target (?P<your_desc>.+? card"
    r"(?: with mana value \d+ or less)?) "
    r"from your graveyard to the battlefield(?: with an? "
    r"(?P<your_counter>[a-z]+) counter on it)?|return the top "
    r"(?P<top_desc>.+? card) of your graveyard to the battlefield)"
    r"(?:\. (?P<riders>.+))?$"
)
_REANIMATE_RIDERS: dict[str, tuple[str, object]] = {
    "that creature gains haste until end of turn": ("haste", "until_end_of_turn"),
    "that creature gains haste": ("haste", "permanent"),
    "exile it at the beginning of the next end step": (
        "exile_at_next_end_step", True,
    ),
    "you lose life equal to that card's mana value": (
        "lose_life_equal_mv", True,
    ),
    # Death (Life // Death).
    "you lose life equal to its mana value": ("lose_life_equal_mv", True),
    "if that creature would leave the battlefield, exile it instead of "
    "putting it anywhere else": ("exile_if_it_would_leave", True),
}
_EACH_PLAYER_REANIMATES_RE = re.compile(
    r"^each player puts (?P<desc>an? .+? card) from their graveyard onto "
    r"the battlefield$"
)
_PUT_INTO_LIBRARY_RE = re.compile(
    r"^put target (?P<desc>.+?) into its owner's library "
    r"(?P<pos>second from the top|just beneath the top X cards of that "
    r"library)(?:\. [Ii]ts controller gains (?P<life>\d+) life)?$"
)
_ALL_CREATURES_GET_UEOT_RE = re.compile(
    r"^(?P<who>all|other) creatures get (?P<p>[+-](?:\d+|X))/(?P<t>[+-](?:\d+|X)) "
    r"until end of turn$"
)


def _pt_part(token: str) -> tuple[int, int]:
    """``+2`` -> (2, 0); ``-X`` -> (0, -1)."""
    sign = -1 if token[0] == "-" else 1
    body = token[1:]
    if body == "X":
        return 0, sign
    return sign * int(body), 0


def _parse_reanimate(m: re.Match) -> Effect | None:
    from mtgz_ir.effects.generic.reanimation import (
        ReturnCardFromGraveyardToBattlefield,
    )

    if m.group("any_desc") is not None:
        source, desc = "target_any_graveyard", m.group("any_desc")
    elif m.group("your_desc") is not None:
        source, desc = "target_your_graveyard", m.group("your_desc")
    else:
        source, desc = "top_of_your_graveyard", m.group("top_desc")
    # "target creature card" / "the top creature card" carry no article.
    f = parse_card_descriptor("a " + desc)
    if f is None:
        return None
    kwargs: dict = {}
    if m.group("counter") is not None:
        kwargs["with_counter"] = m.group("counter")
    if m.group("your_counter") is not None:
        from mtgz_ir.game import KEYWORD_COUNTER_KINDS

        # Only counters with engine semantics (keyword counters, CR
        # 122.1b); e.g. a finality counter would be a silent no-op.
        if m.group("your_counter") not in KEYWORD_COUNTER_KINDS:
            return None
        kwargs["with_counter"] = m.group("your_counter")
    if m.group("upto") is not None:
        kwargs["up_to_one"] = True
    extra: list = []
    riders = m.group("riders")
    if riders is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        for sentence in riders.split(". "):
            key = sentence[0].lower() + sentence[1:]
            rider = _REANIMATE_RIDERS.get(key)
            if rider is not None:
                if rider[0] in kwargs:
                    return None
                kwargs[rider[0]] = rider[1]
                continue
            # A sentence that isn't about the returned card ("You take
            # the initiative.") is an ordinary effect of the spell.
            other = _parse_effect(key)
            if other is None:
                return None
            extra.append(other)
    base = ReturnCardFromGraveyardToBattlefield(
        card_filter=f, source=source, **kwargs,
    )
    if not extra:
        return base
    from mtgz_ir.cards.base import Sequence

    return Sequence(effects=(base, *extra))


# Expressive Iteration's distribution sentence: one card per pile.
_SEL_DISTRIBUTE_PART_PILES: dict[str, str] = {
    "put one of them into your hand": "hand",
    "put one of them on the bottom of your library": "bottom",
    "exile one of them": "exile_playable",
}
_SEL_DISTRIBUTE_RE = re.compile(
    r"^(?P<parts>[Pp]ut one of them .+?, and exile one of them)\. "
    r"[Yy]ou may play the exiled card this turn$"
)
# Demonic Consultation (only card with this shape; the exile count is
# the parameter).
_CHOOSE_NAME_REVEAL_UNTIL_RE = re.compile(
    rf"^choose a card name\. [Ee]xile the top (?P<n>{_AMT}) cards of your "
    r"library, then reveal cards from the top of your library until you "
    r"reveal a card with the chosen name\. [Pp]ut that card into your hand "
    r"and exile all other cards revealed this way$"
)
# Lim-Dul's Vault (only card with this shape; window size and life
# payment are the parameters).
_REPEAT_LOOK_PAY_LIFE_RE = re.compile(
    rf"^look at the top (?P<n>{_AMT}) cards of your library\. [Aa]s many "
    r"times as you choose, you may pay (?P<life>\d+) life, put those cards on "
    r"the bottom of your library in any order, then look at the top "
    rf"(?P=n) cards of your library\. [Tt]hen shuffle and put the last "
    r"cards you looked at this way on top in any order$"
)
# Occult Epiphany.
_DRAW_DISCARD_TOKENS_PER_TYPE_RE = re.compile(
    rf"^draw (?P<n>{_AMT}) cards, then discard (?P=n) cards\. [Cc]reate "
    r"(?P<token>an? .+? creature token(?: with .+?)?) for each card type "
    r"among cards discarded this way$"
)
# Balance (only card with this shape).
_BALANCE_RE = re.compile(
    r"^each player chooses a number of lands they control equal to the "
    r"number of lands controlled by the player who controls the fewest, "
    r"then sacrifices the rest\. [Pp]layers discard cards and sacrifice "
    r"creatures the same way$"
)
# Council's Judgment (only Will-of-the-council card in the pool).
_VOTE_EXILE_RE = re.compile(
    r"^will of the council — [Ss]tarting with you, each player votes for a "
    r"nonland permanent you don't control\. [Ee]xile each permanent with "
    r"the most votes or tied for most votes$"
)
# Flash — "sacrifice it unless you pay its mana cost reduced by {N}".
_PUT_CREATURE_SAC_UNLESS_PAY_RE = re.compile(
    r"^you may put a creature card from your hand onto the battlefield\. "
    r"If you do, sacrifice it unless you pay its mana cost reduced by "
    r"\{(?P<n>\d+)\}$"
)
# Transmute Artifact (only card with this shape).
_TRANSMUTE_ARTIFACT_RE = re.compile(
    r"^sacrifice an artifact\. If you do, search your library for an "
    r"artifact card\. If that card's mana value is less than or equal to "
    r"the sacrificed artifact's mana value, put it onto the battlefield\. "
    r"If it's greater, you may pay \{X\}, where X is the difference\. If you "
    r"do, put it onto the battlefield\. If you don't, put it into its "
    r"owner's graveyard\. Then shuffle$"
)
# Rain of Filth — a quoted activated ability granted until end of turn.
_GRANT_ABILITY_TO_YOURS_UEOT_RE = re.compile(
    r"^until end of turn, (?P<what>lands|creatures|artifacts) you control "
    r'gain "(?P<ability>[^"]+)"$'
)
_PLURAL_PERMANENT_TYPES = {
    "lands": "Land", "creatures": "Creature", "artifacts": "Artifact",
}


def _parse_distribute(m: re.Match, count) -> Effect | None:
    from mtgz_ir.effects.generic.card_flow import LookAtTopCardsDistribute

    parts = re.split(r", and |, ", m.group("parts"))
    piles = []
    for part in parts:
        pile = _SEL_DISTRIBUTE_PART_PILES.get(part[0].lower() + part[1:])
        if pile is None or pile in piles:
            return None
        piles.append(pile)
    return LookAtTopCardsDistribute(count=count, piles=tuple(piles))


# Divided damage (Pyrokinesis, Fire Covenant, Shatterskull Smashing —
# whose "If X is N or more, <Name> deals twice X damage divided as you
# choose among them instead" rider is the optional tail).
_DIVIDED_DAMAGE_RE = re.compile(
    r"^deals (?P<n>\d+|X) damage divided as you choose among "
    r"(?:any number of|up to (?P<k>\w+)) target "
    r"(?P<kinds>creatures|creatures and/or planeswalkers)"
    r"(?:\. If X is (?P<dbl>\d+) or more, .+? deals twice X damage divided "
    r"as you choose among them instead)?$"
)
# Sink into Stupor.
_RETURN_TARGET_SPELL_OR_PERMANENT_RE = re.compile(
    r"^return target spell or nonland permanent an opponent controls to "
    r"its owner's hand$"
)

# Invert Polarity (only coin-flip spell-steal in the pool).
_FLIP_STEAL_OR_COUNTER_RE = re.compile(
    r"^choose target spell, then flip a coin\. If you win the flip, gain "
    r"control of that spell and you may choose new targets for it\. If you "
    r"lose the flip, counter that spell$"
)

# Through the Breach ("Sacrifice that creature"), Sneak Attack's
# activation ("Sacrifice the creature").
_PUT_CREATURE_HASTE_SAC_AT_END_RE = re.compile(
    r"^you may put a creature card from your hand onto the battlefield\. "
    r"That creature gains haste\. Sacrifice (?:that|the) creature at the "
    r"beginning of the next end step$"
)

# Sunfall.
_EXILE_ALL_CREATURES_INCUBATE_RE = re.compile(
    r"^exile all creatures\. [Ii]ncubate X, where X is the number of "
    r"creatures exiled this way$"
)

# The Flashback card.
_GRANT_FLASHBACK_RE = re.compile(
    r"^target (?P<desc>.+? card) in your graveyard gains flashback until "
    r"end of turn\. [Tt]he flashback cost is equal to its mana cost$"
)
# Yawgmoth's Will (two separate oracle lines).
_MAY_PLAY_FROM_GRAVEYARD_RE = re.compile(
    r"^until end of turn, you may play lands and cast spells from your "
    r"graveyard$"
)
_EXILE_INSTEAD_OF_GRAVEYARD_RE = re.compile(
    r"^if a card would be put into your graveyard from anywhere this turn, "
    r"exile that card instead$"
)


def _rest_fields(rest: str) -> tuple[str, bool]:
    if rest == "into your graveyard":
        return "graveyard", False
    return "bottom", rest.endswith("a random order")


def _with_tail(effect: Effect, tail: str | None) -> Effect | None:
    """Sequence ``effect`` with a recursively parsed trailing phrase."""
    if tail is None:
        return effect
    from mtgz_ir.cards.base import Sequence
    from mtgz_ir.cards.parser.effects import _parse_effect

    tail = tail[0].lower() + tail[1:]
    tail_effect = _parse_effect(tail)
    if tail_effect is None:
        return None
    return Sequence(effects=(effect, tail_effect))


def _parse_look_top_n(m: re.Match) -> Effect | None:
    count = _amount(m.group("n"))
    xdef = m.group("xdef")
    if xdef is not None:
        from mtgz_ir.effects.formulas import CountControlledMatching, Scaled
        from mtgz_ir.effects.object_predicates import IsLand

        if m.group("n") != "X":
            return None
        if xdef == _X_DEFINITION_TWICE_LANDS:
            count = Scaled(inner=CountControlledMatching(IsLand()), factor=2)
        elif xdef == _X_DEFINITION_LANDS:
            count = CountControlledMatching(IsLand())
        else:
            return None
    elif not isinstance(count, int):
        return None
    reveal = m.group("verb") == "reveal"
    sel = m.group("sel")
    sm = _SEL_PUT_K_KICKED_INSTEAD_RE.match(sel)
    if sm is not None:
        k, kk = _amount(sm.group("k")), _amount(sm.group("kk"))
        if not isinstance(k, int) or not isinstance(kk, int):
            return None
        from mtgz_ir.effects.combinators import IfConditionElse
        from mtgz_ir.effects.generic.cast_conditionals import SpellWasKicked

        rest_zone, rest_random = _rest_fields(sm.group("rest"))
        base, kicked = (
            LookAtTopCardsPutIntoHand(
                count=count, take=take, reveal=reveal,
                rest_zone=rest_zone, rest_random=rest_random,
            )
            for take in (k, kk)
        )
        return IfConditionElse(
            condition=SpellWasKicked(), then_effect=kicked, else_effect=base,
        )
    sm = _SEL_PUT_K_REST_RE.match(sel)
    if sm is not None:
        k = _amount(sm.group("k"))
        if not isinstance(k, int):
            return None
        rest_zone, rest_random = _rest_fields(sm.group("rest"))
        eff = LookAtTopCardsPutIntoHand(
            count=count, take=k, reveal=reveal,
            rest_zone=rest_zone, rest_random=rest_random,
        )
        return _with_tail(eff, sm.group("tail"))
    sm = _SEL_DISTRIBUTE_RE.match(sel)
    if sm is not None:
        if not isinstance(count, int):
            return None
        return _parse_distribute(sm, count)
    sm = _SEL_ANY_NUMBER_RE.match(sel)
    if sm is not None and isinstance(count, int):
        words = re.split(r" (?:and/or|or) ", sm.group("types"))
        if not all(w in _TYPE_WORDS for w in words):
            return None
        rest_zone, rest_random = _rest_fields(sm.group("rest"))
        eff = LookAtTopCardsPutIntoHand(
            count=count, take=count,
            take_filter=CardFilter(any_types=tuple(_TYPE_WORDS[w] for w in words)),
            optional=True, reveal=reveal, reveal_taken=reveal,
            rest_zone=rest_zone, rest_random=rest_random,
        )
        return _with_tail(eff, sm.group("tail"))
    sm = _SEL_MAY_TAKE_RE.match(sel)
    if sm is not None:
        desc = sm.group("desc_r") or sm.group("desc_p")
        f = parse_card_descriptor(desc)
        if f is None:
            return None
        rest_zone, rest_random = _rest_fields(sm.group("rest"))
        eff = LookAtTopCardsPutIntoHand(
            count=count, take=1, take_filter=f, optional=True,
            reveal=reveal, reveal_taken=sm.group("desc_r") is not None,
            rest_zone=rest_zone, rest_random=rest_random,
        )
        return _with_tail(eff, sm.group("tail"))
    return None


def parse_card_flow_effect(phrase: str) -> Effect | None:
    """Dispatch ``phrase`` (normalized) over the card-flow recognizers."""
    m = _TARGET_PLAYER_DRAWS_RE.match(phrase)
    if m is not None:
        return TargetPlayerDraws(amount=_amount(m.group("n")))
    if _TARGET_PLAYER_EXTRA_TURN_RE.match(phrase) is not None:
        return TargetPlayerTakesExtraTurn()
    m = _TARGET_PLAYER_DISCARDS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.card_flow import TargetPlayerDiscards

        eff: Effect = TargetPlayerDiscards(amount=_amount(m.group("n")))
        if m.group("copy") is not None:
            from mtgz_ir.cards.base import Sequence
            from mtgz_ir.effects.generic.spell_copies import (
                TargetPlayerMayCopyThisSpell,
            )

            eff = Sequence(effects=(eff, TargetPlayerMayCopyThisSpell()))
        return eff
    m = _TARGET_PLAYER_DISCARDS_AT_RANDOM_RE.match(phrase)
    if m is not None:
        return TargetPlayerDiscardsAtRandom(amount=_amount(m.group("n")))
    m = _REVEAL_HAND_YOU_CHOOSE_DISCARD_RE.match(phrase)
    if m is not None:
        desc = m.group("desc")
        if m.group("mv") is not None:
            desc += m.group("mv")
        f = parse_card_descriptor(desc)
        if f is None:
            return None
        draws = m.group("draws")
        eff = TargetPlayerRevealsHandYouChooseDiscard(
            card_filter=f, opponent_only=m.group("who") == "opponent",
            optional=m.group("may") is not None,
            then_draws=_amount(draws) if draws is not None else 0,
        )
        return _with_tail(eff, m.group("rest"))
    m = _REVEAL_HAND_YOU_CHOOSE_EXILE_RE.match(phrase)
    if m is not None:
        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        return TargetPlayerRevealsHandYouChooseDiscard(
            card_filter=f, opponent_only=m.group("who") == "opponent",
            destination="exile",
        )
    m = _SEARCH_TO_ZONE_RE.match(phrase)
    if m is not None:
        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        dest = {
            "into your hand": "hand",
            "into your graveyard": "graveyard",
            "onto the battlefield": "battlefield",
        }[m.group("dest")]
        if m.group("tapped") is not None and dest != "battlefield":
            return None
        return SearchLibrary(
            card_filter=f, destination=dest, reveal=m.group("reveal") is not None,
            tapped=m.group("tapped") is not None,
        )
    m = _SEARCH_TO_TOP_RE.match(phrase)
    if m is not None:
        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        return SearchLibrary(
            card_filter=f, destination="top", reveal=m.group("reveal") is not None,
        )
    m = _REORDER_TOP_N_RE.match(phrase)
    if m is not None:
        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        return ReorderTopCards(count=n)
    m = _REPEAT_LOOK_PAY_LIFE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.card_flow import (
            LookAtTopRepeatPayLifeThenStackTop,
        )

        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        return LookAtTopRepeatPayLifeThenStackTop(
            count=n, life_per_repeat=int(m.group("life")),
        )
    m = _LOOK_TOP_N_HEAD_RE.match(phrase)
    if m is not None:
        return _parse_look_top_n(m)
    m = _SHUFFLE_RE.match(phrase)
    if m is not None:
        return ShuffleYourLibrary(optional=m.group("may") is not None)
    m = _EACH_PLAYER_THEN_DRAWS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import EachPlayerDraws, Sequence

        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        first: Effect = (
            EachPlayerDiscardsHand()
            if m.group("act") == "discards their hand"
            else EachPlayerShufflesZonesIntoLibrary(zones=("hand", "graveyard"))
        )
        return Sequence(effects=(first, EachPlayerDraws(amount=n)))
    m = _UNTAP_UP_TO_N_LANDS_RE.match(phrase)
    if m is not None:
        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        return UntapUpToNLands(amount=n)
    if _RETURN_ALL_PERMANENTS_RE.match(phrase) is not None:
        return ReturnAllPermanentsToOwnersHands()
    if _RETURN_TARGET_SPELL_TO_HAND_RE.match(phrase) is not None:
        return ReturnTargetSpellToOwnersHand()
    m = _SCRY_N_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.library_lookat import Scry

        return Scry(amount=int(m.group("n")), bottom_choice=True)
    if _EXILE_THIS_SPELL_RE.match(phrase) is not None:
        return ExileThisSpell()
    if _SHUFFLE_THIS_SPELL_RE.match(phrase) is not None:
        return ShuffleThisSpellIntoOwnersLibrary()
    if _LOOK_AT_TARGET_PLAYERS_HAND_RE.match(phrase) is not None:
        return LookAtTargetPlayersHand()
    m = _ADD_MANA_PIPS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import AddManaToPool

        counts: dict[str, int] = {}
        for c in re.findall(r"\{([WUBRGC])\}", m.group("pips")):
            counts[c] = counts.get(c, 0) + 1
        return AddManaToPool(produces=tuple(counts.items()))
    m = _REANIMATE_RE.match(phrase)
    if m is not None:
        return _parse_reanimate(m)
    m = _EACH_PLAYER_REANIMATES_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.reanimation import (
            EachPlayerPutsCardFromGraveyardOntoBattlefield,
        )

        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        return EachPlayerPutsCardFromGraveyardOntoBattlefield(card_filter=f)
    m = _PUT_INTO_LIBRARY_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.reanimation import (
            PutTargetPermanentIntoOwnersLibrary,
        )

        f = parse_permanent_descriptor("a " + m.group("desc"))
        if f is None:
            return None
        return PutTargetPermanentIntoOwnersLibrary(
            card_filter=f,
            position=(
                "second_from_top" if m.group("pos") == "second from the top"
                else "beneath_top_x"
            ),
            controller_gains_life=int(m.group("life") or 0),
        )
    m = _ALL_CREATURES_GET_UEOT_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mass_effects import PumpAllCreaturesUEOT

        p, xp = _pt_part(m.group("p"))
        t, xt = _pt_part(m.group("t"))
        return PumpAllCreaturesUEOT(
            power=p, toughness=t, x_power=xp, x_toughness=xt,
            exclude_source=m.group("who") == "other",
        )
    m = _CHOOSE_NAME_REVEAL_UNTIL_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.card_flow import (
            ChooseNameExileThenRevealUntilNamed,
        )

        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        return ChooseNameExileThenRevealUntilNamed(exile_first=n)
    m = _DRAW_DISCARD_TOKENS_PER_TYPE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import CreateToken
        from mtgz_ir.cards.parser.effects import _parse_create_token_effect
        from mtgz_ir.effects.generic.card_flow import (
            DrawDiscardThenTokensPerDiscardedType,
        )

        token = _parse_create_token_effect("create " + m.group("token"))
        if not isinstance(token, CreateToken) or token.count != 1:
            return None
        return DrawDiscardThenTokensPerDiscardedType(
            amount=_amount(m.group("n")), token_printing=token.token_printing,
        )
    if _BALANCE_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.mass_effects import BalanceEffect

        return BalanceEffect()
    if _VOTE_EXILE_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.mass_effects import VoteExileNonlandPermanent

        return VoteExileNonlandPermanent()
    m = _PUT_CREATURE_SAC_UNLESS_PAY_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cheat_into_play import (
            PutCreatureFromHandSacrificeUnlessPay,
        )

        return PutCreatureFromHandSacrificeUnlessPay(reduction=int(m.group("n")))
    if _TRANSMUTE_ARTIFACT_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.cheat_into_play import (
            SacrificeArtifactTutorArtifactPayDifference,
        )

        return SacrificeArtifactTutorArtifactPayDifference()
    m = _GRANT_ABILITY_TO_YOURS_UEOT_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
        from mtgz_ir.effects.generic.mass_effects import (
            GrantAbilityToPermanentsYouControlUEOT,
        )

        ability = _parse_activated_ability_line(m.group("ability"))
        if ability is None:
            return None
        return GrantAbilityToPermanentsYouControlUEOT(
            card_filter=CardFilter(
                any_types=(_PLURAL_PERMANENT_TYPES[m.group("what")],),
            ),
            ability=ability,
        )
    m = _DIVIDED_DAMAGE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.divided_damage import (
            DealDamageDividedAmongTargets,
        )

        max_targets = None
        if m.group("k") is not None:
            k = _CARDINALS.get(m.group("k"))
            if k is None:
                return None
            max_targets = k
        if m.group("dbl") is not None and m.group("n") != "X":
            return None
        return DealDamageDividedAmongTargets(
            amount=_amount(m.group("n")), max_targets=max_targets,
            include_planeswalkers=m.group("kinds") != "creatures",
            double_if_x_at_least=int(m.group("dbl") or 0),
        )
    if _RETURN_TARGET_SPELL_OR_PERMANENT_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.card_flow import (
            ReturnTargetSpellOrNonlandPermanentToHand,
        )

        return ReturnTargetSpellOrNonlandPermanentToHand(opponent_only=True)
    if _PUT_CREATURE_HASTE_SAC_AT_END_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.cheat_into_play import (
            PutCreatureFromHandHasteSacrificeAtNextEndStep,
        )

        return PutCreatureFromHandHasteSacrificeAtNextEndStep()
    if _EXILE_ALL_CREATURES_INCUBATE_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.incubate import ExileAllCreaturesThenIncubate

        return ExileAllCreaturesThenIncubate()
    if _FLIP_STEAL_OR_COUNTER_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.spell_copies import (
            FlipCoinStealOrCounterTargetSpell,
        )

        return FlipCoinStealOrCounterTargetSpell()
    m = _GRANT_FLASHBACK_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.graveyard_casting import (
            GrantFlashbackToTargetCardUEOT,
        )

        f = parse_card_descriptor("a " + m.group("desc"))
        if f is None:
            return None
        return GrantFlashbackToTargetCardUEOT(card_filter=f)
    if _MAY_PLAY_FROM_GRAVEYARD_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.graveyard_casting import (
            MayPlayFromGraveyardThisTurn,
        )

        return MayPlayFromGraveyardThisTurn()
    if _EXILE_INSTEAD_OF_GRAVEYARD_RE.match(phrase) is not None:
        from mtgz_ir.effects.generic.graveyard_casting import (
            ExileInsteadOfGraveyardThisTurn,
        )

        return ExileInsteadOfGraveyardThisTurn()
    m = _ADD_N_MANA_ANY_COMBINATION_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import AddManaToPool

        n = _amount(m.group("n"))
        if not isinstance(n, int):
            return None
        return AddManaToPool(produces=(("any", n),))
    return None
