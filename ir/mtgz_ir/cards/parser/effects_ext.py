"""Unified-grammar extension recognizers (cube planeswalker bucket).

Each recognizer here is a parameterized shape wired into the unified
effect grammar: :func:`mtgz_ir.cards.parser.effects._parse_effect`
calls :func:`parse_ext_effect` as its LAST fallback, after every
pre-existing single-effect recognizer and the compound splitter have
declined the phrase. Running last means these shapes can never change
how an already-claimed card parses; they only claim phrases nothing
else understood. Inner phrases recurse through ``_parse_effect`` so a
shape like "you may <effect>" composes with the whole grammar.

Shapes (normalized phrase form -- no trailing period, lowercased first
letter, source-name subject stripped):

* ``you win the game`` -> :class:`WinTheGame`
* ``if <condition>, <effect>`` -> :class:`IfCondition` (the
  condition phrasebook :data:`_CONDITION_PHRASES`, then the counted
  "you control <N> or fewer <permanents>" -> ``ControlsAtMost``)
* ``you may <effect>`` -> :class:`MayDo`
* ``destroy|exile [up to one ]target <noun phrase>`` ->
  ``Destroy`` / ``Exile`` with a composed :class:`TargetSpec`
* ``return [up to one ]target <noun phrase> to its owner's hand`` ->
  ``Bounce``
* ``put <N> +1/+1 counter(s) on [up to one ]target <noun phrase>[.
  It gains <keywords> until end of turn]`` ->
  :class:`PutPlusOneCountersOnTargetThenGrantKeywordsUEOT`
* ``put <N> +1/+1 counter(s) on each creature you control. Those
  creatures gain <keywords> until <end of turn|your next turn>`` ->
  :class:`PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords`
* ``look at the top <N> cards of your library. You may reveal a <card
  filter> card from among them and put it into your hand. Put the rest
  on the bottom of your library in a random order`` ->
  :class:`LookTopNMayRevealMatchingToHandRestBottomRandom`
* ``until your next turn, you may cast sorcery spells as though they
  had flash`` -> :class:`MayCastSorceriesAsThoughFlashUntilYourNextTurn`
* ``create <N> [tapped ]<P>/<T> <color> <Subtypes> [artifact ]creature
  token(s)[ with <keywords>]`` for a token type the registry
  (``TOKEN_PRINTINGS``) doesn't carry -> a :class:`CreateToken` whose
  printing is synthesized from the printed characteristics
* ``create <Name>, a [legendary ]<P>/<T> <color> <Subtypes> creature
  token[ with <keywords>]`` -> named-token :class:`CreateToken`
* ``sacrifice a creature. When you do, deals X damage to any target,
  where X is that creature's power. If the sacrificed creature was a
  <Subtype>, draw X cards`` ->
  :class:`SacrificeCreatureThenDamageEqualToItsPower`
* library / player shapes: ``look at the top card of target player's
  library. You may put that card on the bottom ...``, ``put N cards
  from your hand on top of your library in any order``, ``exile all
  cards from target player's library, then that player shuffles ...``,
  ``target player draws N cards, then discards M cards``, ``reveal the
  top N cards ... An opponent chooses one ... exile the other with a
  silver counter``, ``put a card you own with a silver counter on it
  from exile into your hand``
* ``gain control of target <noun>``; ``put target <noun> into its
  owner's library <Nth> from the top``; ``untap up to N <nouns>``;
  ``at the beginning of the next end step, <untargeted effect>``;
  ``target <A> becomes a copy of another target <B> until end of
  turn[, except ...]``; tokens with a quoted per-count self-anthem
* ``you get an emblem with "<trigger>"`` (the quoted trigger parses
  through the unified trigger grammar, plus the whole-line "Whenever
  you cast a spell that targets one or more permanents, gain control of
  those permanents." shape)
* trigger conditions (:func:`parse_ext_trigger_phrase`): ``you draw a
  card``, ``you cast a[n] [<filter>] spell``

:func:`legacy_grammar` switches every recognizer here off, for
printings whose claimed shape EoE parity probes pin.

The compositional target noun-phrase parser
(:func:`parse_target_noun_phrase`) covers
``[<adjectives> ]<type>[, <type>][, or|or <type>][ an opponent
controls| you control][ with <qualifier>]``, where a capitalized
``<type>`` is a subtype (``target Wolf you control``).
"""

from __future__ import annotations

import contextlib
import re

from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.generic.cube_walkers import (
    AtBeginningOfNextEndStep,
    CreateEmblemWithAbilities,
    IsColorless,
    PutTargetIntoOwnersLibraryNthFromTop,
    UntapUpToNPermanents,
    ExileTargetPlayersLibraryThenShuffleHandIn,
    GainControlOfTargetPermanently,
    LookAtTopCardOfTargetPlayersLibraryMayBottom,
    PutCardsFromHandOnTopOfLibrary,
    TargetPlayerDrawsThenDiscards,
    HasPrintedCardType,
    IsPermanent,
    HasManaValueAtLeast,
    HasManaValueAtMost,
    IfCondition,
    LookTopNMayRevealMatchingToHandRestBottomRandom,
    MayCastSorceriesAsThoughFlashUntilYourNextTurn,
    PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords,
    PutPlusOneCountersOnTargetThenGrantKeywordsUEOT,
    SacrificeCreatureThenDamageEqualToItsPower,
    WinTheGame,
    YourLibraryHasNoCards,
)
from mtgz_ir.effects.object_predicates import (
    ControlledByOpponent,
    ControlledByYou,
    HasKeyword,
    HasPowerAtLeast,
    HasPowerAtMost,
    HasSubtype,
    IsArtifact,
    IsAttacking,
    IsBattle,
    IsBlocking,
    IsCreature,
    IsEnchantment,
    IsLand,
    IsLegendary,
    IsNontoken,
    IsPlaneswalker,
    IsTapped,
    ObjectPredicate,
)


# ---------------------------------------------------------------------------
# Target noun phrases
# ---------------------------------------------------------------------------

# Permanent type nouns -> predicate. ``permanent`` is any battlefield
# object (``None`` = no type restriction).
_TYPE_NOUNS: dict[str, ObjectPredicate | None] = {
    "creature": IsCreature(),
    "artifact": IsArtifact(),
    "enchantment": IsEnchantment(),
    "land": IsLand(),
    "planeswalker": IsPlaneswalker(),
    "battle": IsBattle(),
    "permanent": None,
}

# Leading adjectives -> predicate (conjoined with the type union).
_ADJECTIVES: dict[str, ObjectPredicate] = {
    "tapped": IsTapped(),
    "untapped": ~IsTapped(),
    "attacking": IsAttacking(),
    "blocking": IsBlocking(),
    "nonland": ~IsLand(),
    "noncreature": ~IsCreature(),
    "nonartifact": ~IsArtifact(),
    "nontoken": IsNontoken(),
    "nonlegendary": ~IsLegendary(),
}

# Trailing controller clauses.
_CONTROLLER_SUFFIXES: tuple[tuple[str, ObjectPredicate], ...] = (
    (" an opponent controls", ControlledByOpponent()),
    # In a two-player game the defending player is the attacking
    # player's opponent (Frenzied Trapbreaker).
    (" defending player controls", ControlledByOpponent()),
    (" you don't control", ~ControlledByYou()),
    (" you control", ControlledByYou()),
)

_WITH_MANA_VALUE = re.compile(r"^mana value (\d+) or (greater|less)$")
_WITH_POWER = re.compile(r"^power (\d+) or (greater|less)$")
# A permanent subtype used as a target noun (cube W/G creature bucket:
# Bejeweled Warg's "target Wolf you control").
_SUBTYPE_NOUN = re.compile(r"^[A-Z][a-z]+$")


def _keyword_names() -> frozenset[str]:
    from mtgz_ir.cards.parser.keywords import _KEYWORD_LINES

    return frozenset(_KEYWORD_LINES.values())


def _parse_with_qualifier(text: str) -> ObjectPredicate | None:
    """``with <qualifier>`` tail: ``mana value N or greater|less``,
    ``power N or greater|less`` or a keyword disjunction (``trample or
    haste``) / single keyword."""
    m = _WITH_MANA_VALUE.match(text)
    if m is not None:
        n = int(m.group(1))
        return HasManaValueAtLeast(n) if m.group(2) == "greater" else (
            HasManaValueAtMost(n)
        )
    m = _WITH_POWER.match(text)
    if m is not None:
        n = int(m.group(1))
        return HasPowerAtLeast(n) if m.group(2) == "greater" else (
            HasPowerAtMost(n)
        )
    kws = [k.strip() for k in text.split(" or ")]
    known = _keyword_names()
    if not all(k in known for k in kws):
        return None
    pred: ObjectPredicate = HasKeyword(kws[0])
    for k in kws[1:]:
        pred = pred | HasKeyword(k)
    return pred


def _split_type_list(text: str) -> list[str] | None:
    """``artifact, creature, or enchantment`` / ``artifact or creature``
    / ``creature`` -> list of type nouns (``None`` if malformed)."""
    text = text.replace(", or ", ", ")
    pieces: list[str] = []
    for chunk in text.split(", "):
        pieces.extend(p.strip() for p in chunk.split(" or "))
    if not pieces or any(not p for p in pieces):
        return None
    return pieces


def parse_target_noun_phrase(phrase: str) -> ObjectPredicate | None:
    """Compose a :class:`TargetFilter` for a printed target noun phrase
    (the text after ``target``). Returns ``None`` when any piece is
    outside the closed adjective / type / controller / qualifier
    vocabulary, so a caller never builds a filter that silently drops
    a restriction."""
    text = phrase.strip()
    qualifier: ObjectPredicate | None = None
    if " with " in text:
        text, _, tail = text.partition(" with ")
        qualifier = _parse_with_qualifier(tail)
        if qualifier is None:
            return None
    controller: ObjectPredicate | None = None
    for suffix, pred in _CONTROLLER_SUFFIXES:
        if text.endswith(suffix):
            text = text[: -len(suffix)]
            controller = pred
            break
    words = text.split(" ")
    adjectives: list[ObjectPredicate] = []
    while words and words[0].rstrip(",") in _ADJECTIVES and len(words) > 1:
        adjectives.append(_ADJECTIVES[words.pop(0).rstrip(",")])
    types = _split_type_list(" ".join(words))
    if types is None or not all(
        t in _TYPE_NOUNS or _SUBTYPE_NOUN.match(t) for t in types
    ):
        return None
    if qualifier is not None and len(types) > 1:
        # "artifact, enchantment, or creature with flying" binds the
        # qualifier to the LAST noun only; per-noun qualifier scoping
        # isn't modeled, so decline rather than over-apply it.
        return None
    # A capitalized noun is a subtype ("target Wolf you control",
    # "target Equipment"): any permanent with that effective subtype.
    type_preds = [
        _TYPE_NOUNS[t] if t in _TYPE_NOUNS else HasSubtype(t) for t in types
    ]
    result: ObjectPredicate
    if any(p is None for p in type_preds):
        # "permanent" anywhere in the union admits every permanent --
        # and ONLY permanents: players carry no printing, so negated
        # adjectives ("nonland") or controller clauses can't let a
        # player through.
        result = IsPermanent()
    else:
        result = type_preds[0]
        for p in type_preds[1:]:
            result = result | p
    for extra in [*adjectives, controller, qualifier]:
        if extra is not None:
            result = result & extra
    return result


def _card_type_filter(text: str) -> ObjectPredicate | None:
    """Filter over CARDS (library / graveyard) for a printed card
    descriptor such as ``noncreature, nonland``. Every comma-separated
    piece is a ``non<type>`` exclusion or a bare type; they conjoin."""
    preds: list[ObjectPredicate] = []
    for piece in (p.strip() for p in text.split(",")):
        negated = piece.startswith("non")
        noun = piece[3:] if negated else piece
        if noun not in _CARD_TYPE_WORDS:
            return None
        pred = HasPrintedCardType(_CARD_TYPE_WORDS[noun])
        preds.append(~pred if negated else pred)
    out = preds[0]
    for p in preds[1:]:
        out = out & p
    return out


_CARD_TYPE_WORDS: dict[str, str] = {
    "creature": "Creature",
    "land": "Land",
    "artifact": "Artifact",
    "enchantment": "Enchantment",
    "instant": "Instant",
    "sorcery": "Sorcery",
    "planeswalker": "Planeswalker",
}


# ---------------------------------------------------------------------------
# Effect recognizers
# ---------------------------------------------------------------------------

_COUNT = r"a|an|\d+|one|two|three|four|five|six|seven|eight|nine|ten"
_COLOR = r"white|blue|black|red|green|colorless"

_WIN_THE_GAME = re.compile(r"^(?:you )?win the game$")
_IF_CONDITION = re.compile(r"^if (?P<cond>[^,]+), (?P<eff>.+)$")
_YOU_MAY = re.compile(r"^you may (?P<eff>.+)$")
_DESTROY_OR_EXILE = re.compile(
    r"^(?P<verb>destroy|exile) (?P<upto>up to one )?(?P<another>another )?"
    r"target (?P<noun>.+)$"
)
_BOUNCE = re.compile(
    r"^return (?P<upto>up to one )?target (?P<noun>.+) to its owner's hand$"
)
_COUNTERS_ON_TARGET = re.compile(
    rf"^put (?P<n>{_COUNT}) \+1/\+1 counters? on (?P<upto>up to one )?"
    r"target (?P<noun>.+?)"
    r"(?:\. [Ii]t gains (?P<kw>.+) until end of turn)?$"
)
_COUNTERS_ON_EACH_THEN_GAIN = re.compile(
    rf"^put (?P<n>{_COUNT}) \+1/\+1 counters? on each creature you "
    r"control\. [Tt]hose creatures gain (?P<kw>.+) until "
    r"(?P<dur>end of turn|your next turn)$"
)
_LOOK_TOP_MAY_REVEAL_REST_BOTTOM = re.compile(
    rf"^look at the top (?P<n>{_COUNT}) cards of your library\. "
    r"[Yy]ou may reveal an? (?P<filter>.+?) card from among them and put "
    r"it into your hand\. [Pp]ut the rest on the bottom of your library "
    r"in a random order$"
)
_SORCERIES_AS_FLASH = re.compile(
    r"^until your next turn, you may cast sorcery spells as though they "
    r"had flash$"
)
_CREATE_STAT_TOKEN = re.compile(
    rf"^create (?P<count>{_COUNT}) (?P<tapped>tapped )?(?P<p>\d+)/(?P<t>\d+) "
    rf"(?P<color>{_COLOR}) (?P<sub>[A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)*) "
    r"(?P<artifact>artifact )?creature tokens?(?: with (?P<kw>.+))?$"
)
_CREATE_NAMED_TOKEN = re.compile(
    r"^create (?P<name>[A-Z][A-Za-z' ]*), an? (?P<legendary>legendary )?"
    rf"(?P<p>\d+)/(?P<t>\d+) (?P<color>{_COLOR}) "
    r"(?P<sub>[A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)*) "
    r"(?P<artifact>artifact )?creature token(?: with (?P<kw>.+))?$"
)
_SAC_CREATURE_DAMAGE_BY_POWER = re.compile(
    r"^sacrifice a creature\. When you do, deals X damage to any target, "
    r"where X is that creature's power\. If the sacrificed creature was "
    r"an? (?P<sub>[A-Z][A-Za-z]*), draw X cards$"
)

# Jace, the Mind Sculptor's +2 -- only card in the pool with the
# look-at-any-player's-top-card-may-bottom shape; literal until a second
# printing varies the card count ("top N cards") or the destination.
_LOOK_TOP_OF_TARGET_PLAYER_MAY_BOTTOM = re.compile(
    r"^look at the top card of target player's library\. [Yy]ou may put "
    r"that card on the bottom of that player's library$"
)
_PUT_CARDS_FROM_HAND_ON_TOP = re.compile(
    rf"^put (?P<n>{_COUNT}) cards? from your hand on top of your library "
    r"in any order$"
)
# Jace, the Mind Sculptor's -12 -- only card in the pool with this
# library-exile + hand-shuffle shape; literal until a second printing.
_EXILE_TARGET_LIBRARY_SHUFFLE_HAND_IN = re.compile(
    r"^exile all cards from target player's library, then that player "
    r"shuffles their hand into their library$"
)
_TARGET_PLAYER_DRAWS_THEN_DISCARDS = re.compile(
    rf"^target player draws (?P<d>{_COUNT}) cards?, then discards "
    rf"(?P<x>{_COUNT}) cards?$"
)
_GAIN_CONTROL_OF_TARGET = re.compile(r"^gain control of target (?P<noun>.+)$")
_AT_BEGINNING_OF_NEXT_END_STEP = re.compile(
    r"^at the beginning of the next end step, (?P<eff>.+)$"
)
_UNTAP_UP_TO_N = re.compile(rf"^untap up to (?P<n>{_COUNT}) (?P<noun>\w+)s$")
_ORDINALS = {"second": 2, "third": 3, "fourth": 4, "fifth": 5}
_PUT_TARGET_INTO_LIBRARY_NTH_FROM_TOP = re.compile(
    r"^put target (?P<noun>.+) into its owner's library "
    r"(?P<nth>second|third|fourth|fifth) from the top$"
)
_YOU_GET_AN_EMBLEM_WITH = re.compile(r'^you get an emblem with "(?P<body>.+)"$')
# Karn, Scion of Urza's +1 -- only card in the pool with the
# opponent-chooses-one-of-the-revealed-cards shape; the revealed count
# and the counter kind are captured, the rest stays literal until a
# second printing varies it.
_REVEAL_TOP_OPPONENT_CHOOSES_ONE = re.compile(
    rf"^reveal the top (?P<n>{_COUNT}) cards of your library\. [Aa]n "
    r"opponent chooses one of them\. [Pp]ut that card into your hand and "
    r"exile the other with an? (?P<counter>silver) counter on it$"
)
_PUT_CARD_WITH_COUNTER_FROM_EXILE = re.compile(
    r"^put a card you own with an? (?P<counter>silver) counter on it from "
    r"exile into your hand$"
)
_TARGET_BECOMES_COPY_OF_ANOTHER_TARGET_UEOT = re.compile(
    r"^target (?P<a>.+?) becomes a copy of another target (?P<b>.+?) "
    r"until end of turn(?:, except it's an? "
    r"(?P<extra>artifact|creature|enchantment) in addition to its other "
    r"types)?$"
)
# Stat token whose only rider is a quoted self-anthem scaling with a
# count: 'with "This token gets +N/+M for each <noun> you control."'
_CREATE_TOKEN_WITH_PER_COUNT_ANTHEM = re.compile(
    rf"^create (?P<count>{_COUNT}) (?P<p>\d+)/(?P<t>\d+) (?P<color>{_COLOR}) "
    r"(?P<sub>[A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)*) "
    r"(?P<artifact>artifact )?creature tokens? with \"This token gets "
    r"\+(?P<pb>\d+)/\+(?P<tb>\d+) for each (?P<each>[a-z]+) you control"
    # The closing ``."`` is absent when the enclosing trigger-line regex
    # already consumed it (Urza, Lord High Artificer's ETB).
    r"(?:\.\")?$"
)

# Closed condition phrasebook for the ``if <condition>, <effect>`` shape.
_CONDITION_PHRASES = {
    "your library has no cards in it": YourLibraryHasNoCards(),
}

# "you control <N> or fewer <permanents>" (Edge of Autumn: "If you control
# four or fewer lands, ..."), checked as the effect resolves; the noun is
# a plural permanent noun of the composable cost grammar ("lands",
# "creatures", "Forests", "artifacts or creatures").
_CONTROL_N_OR_FEWER = re.compile(
    rf"^you control (?P<n>{_COUNT}) or fewer (?P<noun>.+)$"
)


def _condition_phrase(text: str):
    """The ``Condition`` an ``if <condition>, <effect>`` phrase names, or
    ``None``."""
    cond = _CONDITION_PHRASES.get(text)
    if cond is not None:
        return cond
    m = _CONTROL_N_OR_FEWER.match(text)
    if m is None:
        return None
    from mtgz_ir.cards.parser.creature_rmc_grammar import (
        _permanent_noun_predicate,
    )
    from mtgz_ir.effects.conditions import ControlsAtMost

    pred = _permanent_noun_predicate(m.group("noun"), plural=True)
    if pred is None:
        return None
    return ControlsAtMost(pred, _count(m.group("n")))


def _count(token: str) -> int:
    from mtgz_ir.cards.parser.effects import _parse_token_count

    return _parse_token_count(token)


def _keywords(body: str | None) -> frozenset[str] | None:
    """Parse ``<kw>[ and <kw>]`` into engine keyword spellings (``None``
    when any piece is unknown)."""
    if body is None:
        return frozenset()
    from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

    return _parse_keyword_grant_body(body)


def _target_spec(noun: str, up_to_one: bool):
    from mtgz_ir.effects.targets import TargetSpec

    pred = parse_target_noun_phrase(noun)
    if pred is None:
        return None
    return TargetSpec(filter=pred, count_min=0 if up_to_one else 1)


# A token's colour word -> its colour indicator ("colorless": none).
_COLOR_LETTERS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}


def _token_printing(
    name: str, power: str, toughness: str, subtypes: str,
    artifact: bool, keywords: frozenset[str], legendary: bool = False,
    static_abilities: tuple = (), color: str = "colorless",
):
    from mtgz_ir.cards.base import make_token_creature

    letter = _COLOR_LETTERS.get(color)
    return make_token_creature(
        name=name,
        power=int(power),
        toughness=int(toughness),
        subtypes=tuple(subtypes.split(" ")),
        keywords=keywords,
        artifact=artifact,
        legendary=legendary,
        static_abilities=static_abilities,
        colors=(letter,) if letter else (),
    )


_LEGACY_GRAMMAR = [False]


@contextlib.contextmanager
def legacy_grammar():
    """Disable every recognizer in this module for the duration of the
    block -- the pre-extension grammar. Used for printings whose claimed
    shape is pinned by EoE cross-engine parity probes (see
    ``spell_builder._LEGACY_PARTIAL_MODAL_CLAIMS``)."""
    prior = _LEGACY_GRAMMAR[0]
    _LEGACY_GRAMMAR[0] = True
    try:
        yield
    finally:
        _LEGACY_GRAMMAR[0] = prior


def parse_ext_effect(phrase: str) -> Effect | None:
    """Last-resort recognizers for the unified effect grammar. See the
    module docstring for the shape list."""
    if _LEGACY_GRAMMAR[0]:
        return None
    from mtgz_ir.cards.parser.effects import _parse_effect

    if _WIN_THE_GAME.match(phrase):
        return WinTheGame()

    m = _IF_CONDITION.match(phrase)
    if m is not None:
        cond = _condition_phrase(m.group("cond"))
        if cond is not None:
            inner = _parse_effect(m.group("eff"))
            if inner is not None:
                return IfCondition(condition=cond, then=inner)

    if _SORCERIES_AS_FLASH.match(phrase):
        return MayCastSorceriesAsThoughFlashUntilYourNextTurn()

    m = _YOU_MAY.match(phrase)
    if m is not None and ". " not in m.group("eff"):
        # Single-sentence body only: in "You may X. Y." the optional
        # does not scope over the second sentence.
        inner = _parse_effect(m.group("eff"))
        if inner is not None:
            from mtgz_ir.effects.combinators import MayDo

            return MayDo(inner=inner)

    m = _DESTROY_OR_EXILE.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), bool(m.group("upto")))
        if spec is not None and m.group("another"):
            from dataclasses import replace as _replace

            from mtgz_ir.effects.object_predicates import IsSource

            spec = _replace(spec, filter=spec.filter & ~IsSource())
        if spec is not None:
            if m.group("verb") == "destroy":
                from mtgz_ir.effects.generic.destroy import Destroy

                return Destroy(targets=(spec,))
            from mtgz_ir.effects.generic.exile import Exile

            return Exile(targets=(spec,))

    m = _BOUNCE.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), bool(m.group("upto")))
        if spec is not None:
            from mtgz_ir.effects.generic.bounce import Bounce

            return Bounce(targets=(spec,))

    m = _COUNTERS_ON_TARGET.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), bool(m.group("upto")))
        kws = _keywords(m.group("kw"))
        if spec is not None and kws is not None:
            return PutPlusOneCountersOnTargetThenGrantKeywordsUEOT(
                amount=_count(m.group("n")),
                targets=(spec,),
                granted_keywords=kws,
            )

    m = _COUNTERS_ON_EACH_THEN_GAIN.match(phrase)
    if m is not None:
        kws = _keywords(m.group("kw"))
        if kws:
            return PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords(
                amount=_count(m.group("n")),
                granted_keywords=kws,
                until_your_next_turn=m.group("dur") == "your next turn",
            )

    m = _LOOK_TOP_MAY_REVEAL_REST_BOTTOM.match(phrase)
    if m is not None:
        card_filter = _card_type_filter(m.group("filter"))
        if card_filter is not None:
            return LookTopNMayRevealMatchingToHandRestBottomRandom(
                n=_count(m.group("n")), card_filter=card_filter,
            )

    m = _CREATE_STAT_TOKEN.match(phrase)
    if m is not None:
        from mtgz_ir.cards import TOKEN_PRINTINGS

        kws = _keywords(m.group("kw"))
        # Registered token types were already claimed (or deliberately
        # declined) by the registry-backed recognizer in effects.py.
        if m.group("sub") not in TOKEN_PRINTINGS and kws is not None:
            from mtgz_ir.effects.generic.tokens import CreateToken

            return CreateToken(
                token_printing=_token_printing(
                    m.group("sub"), m.group("p"), m.group("t"),
                    m.group("sub"), bool(m.group("artifact")), kws,
                    color=m.group("color"),
                ),
                count=_count(m.group("count")),
                tapped=bool(m.group("tapped")),
            )

    m = _CREATE_NAMED_TOKEN.match(phrase)
    if m is not None:
        kws = _keywords(m.group("kw"))
        if kws is not None:
            from mtgz_ir.effects.generic.tokens import CreateToken

            return CreateToken(
                token_printing=_token_printing(
                    m.group("name"), m.group("p"), m.group("t"),
                    m.group("sub"), bool(m.group("artifact")), kws,
                    legendary=bool(m.group("legendary")),
                    color=m.group("color"),
                ),
            )

    m = _SAC_CREATURE_DAMAGE_BY_POWER.match(phrase)
    if m is not None:
        return SacrificeCreatureThenDamageEqualToItsPower(
            draw_if_subtype=m.group("sub"),
        )

    if _LOOK_TOP_OF_TARGET_PLAYER_MAY_BOTTOM.match(phrase):
        return LookAtTopCardOfTargetPlayersLibraryMayBottom()

    m = _PUT_CARDS_FROM_HAND_ON_TOP.match(phrase)
    if m is not None:
        return PutCardsFromHandOnTopOfLibrary(amount=_count(m.group("n")))

    if _EXILE_TARGET_LIBRARY_SHUFFLE_HAND_IN.match(phrase):
        return ExileTargetPlayersLibraryThenShuffleHandIn()

    m = _TARGET_PLAYER_DRAWS_THEN_DISCARDS.match(phrase)
    if m is not None:
        return TargetPlayerDrawsThenDiscards(
            draw=_count(m.group("d")), discard=_count(m.group("x")),
        )

    m = _GAIN_CONTROL_OF_TARGET.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), up_to_one=False)
        if spec is not None:
            return GainControlOfTargetPermanently(targets=(spec,))

    m = _AT_BEGINNING_OF_NEXT_END_STEP.match(phrase)
    if m is not None:
        inner = _parse_effect(m.group("eff"))
        # A delayed trigger's own targets would be chosen when it
        # triggers; the scheduling pipeline can't do that, so only
        # untargeted delayed effects are claimed.
        if inner is not None and inner.target_count_range()[1] == 0:
            return AtBeginningOfNextEndStep(inner=inner)

    m = _UNTAP_UP_TO_N.match(phrase)
    if m is not None:
        pred = parse_target_noun_phrase(m.group("noun"))
        if pred is not None:
            return UntapUpToNPermanents(
                n=_count(m.group("n")), permanent_filter=pred,
            )

    m = _PUT_TARGET_INTO_LIBRARY_NTH_FROM_TOP.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), up_to_one=False)
        if spec is not None:
            return PutTargetIntoOwnersLibraryNthFromTop(
                position=_ORDINALS[m.group("nth")], targets=(spec,),
            )

    m = _REVEAL_TOP_OPPONENT_CHOOSES_ONE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_walkers import (
            RevealTopNOpponentChoosesOneRestExiledWithCounter,
        )

        return RevealTopNOpponentChoosesOneRestExiledWithCounter(
            n=_count(m.group("n")), counter=m.group("counter"),
        )

    m = _PUT_CARD_WITH_COUNTER_FROM_EXILE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_walkers import (
            PutCardYouOwnWithCounterFromExileIntoHand,
        )

        return PutCardYouOwnWithCounterFromExileIntoHand(
            counter=m.group("counter"),
        )

    m = _CREATE_TOKEN_WITH_PER_COUNT_ANTHEM.match(phrase)
    if m is not None:
        each = parse_target_noun_phrase(m.group("each"))
        if each is not None:
            from mtgz_ir.cards.base import StaticAbility, StaticScope
            from mtgz_ir.effects.conditions import PerControlled
            from mtgz_ir.effects.generic.tokens import CreateToken

            anthem = StaticAbility(
                scope=StaticScope.SELF,
                power_bonus=int(m.group("pb")),
                toughness_bonus=int(m.group("tb")),
                condition=PerControlled(each),
                bonus_per_condition_count=True,
            )
            return CreateToken(
                token_printing=_token_printing(
                    m.group("sub"), m.group("p"), m.group("t"),
                    m.group("sub"), bool(m.group("artifact")), frozenset(),
                    static_abilities=(anthem,), color=m.group("color"),
                ),
                count=_count(m.group("count")),
            )

    m = _TARGET_BECOMES_COPY_OF_ANOTHER_TARGET_UEOT.match(phrase)
    if m is not None:
        a = _target_spec(m.group("a"), up_to_one=False)
        b = _target_spec(m.group("b"), up_to_one=False)
        if a is not None and b is not None:
            from dataclasses import replace as _replace

            from mtgz_ir.effects.generic.cube_walkers import (
                TargetBecomesCopyOfAnotherTargetUEOT,
            )

            extra = m.group("extra")
            return TargetBecomesCopyOfAnotherTargetUEOT(
                targets=(a, _replace(b, distinct_from=(0,))),
                add_card_types=(
                    (_CARD_TYPE_WORDS[extra],) if extra else ()
                ),
            )

    m = _YOU_GET_AN_EMBLEM_WITH.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.triggers import (
            _parse_triggered_ability_line,
        )

        body = m.group("body")
        parsed = _parse_spell_targets_gain_control_trigger(body)
        if parsed is None:
            parsed = _parse_triggered_ability_line(body)
        if parsed is not None:
            arms = parsed if isinstance(parsed, tuple) else (parsed,)
            return CreateEmblemWithAbilities(
                name=f'emblem: "{body}"', triggered_abilities=arms,
            )
        # Cube glue bucket: emblem statics / printing-level grants and
        # triggers naming "this emblem" as their source.
        from mtgz_ir.cards.parser.cube_glue_grammar import parse_glue_emblem

        return parse_glue_emblem(body)
    return None


# "Whenever you cast a spell that targets one or more <nouns>, gain
# control of those <nouns>." -- a whole trigger line because the effect's
# "those <nouns>" is anaphoric to the condition (the triggering spell's
# targets); neither half is meaningful alone. Dack Fayden's emblem.
_SPELL_TARGETS_GAIN_CONTROL_TRIGGER = re.compile(
    r"^Whenever you cast a spell that targets one or more "
    r"(?P<noun>permanents|creatures|artifacts), gain control of those "
    r"(?P=noun)\.$"
)


def _parse_spell_targets_gain_control_trigger(line: str):
    m = _SPELL_TARGETS_GAIN_CONTROL_TRIGGER.match(line)
    if m is None:
        return None
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.cube_walkers import (
        GainControlOfTriggeringSpellTargets,
        SpellTargetsMatching,
    )
    from mtgz_ir.effects.predicates import YOU, Controlled, Event, Subject

    noun_filter = parse_target_noun_phrase(m.group("noun")[:-1])
    return TriggeredAbility(
        event=Event.IS_CAST,
        predicate=(
            Subject("spell", SpellTargetsMatching(noun_filter))
            & Controlled(YOU, subject="controller")
        ),
        effect=GainControlOfTriggeringSpellTargets(noun_filter),
    )


# ---------------------------------------------------------------------------
# Trigger-condition phrases (fallback for triggers._parse_predicate_
# trigger_phrase)
# ---------------------------------------------------------------------------

_YOU_CAST_A_SPELL = re.compile(r"^you cast an? (?:(?P<filter>.+) )?spell$")


def _spell_filter(text: str) -> ObjectPredicate | None:
    """Printed spell descriptor -> predicate over the spell object:
    ``noncreature``, ``colorless``, ``creature``, ``instant or
    sorcery`` ..."""
    if text == "colorless":
        return IsColorless()
    pieces = text.split(" or ")
    preds: list[ObjectPredicate] = []
    for piece in pieces:
        negated = piece.startswith("non")
        noun = piece[3:] if negated else piece
        if noun not in _CARD_TYPE_WORDS:
            return None
        pred = HasPrintedCardType(_CARD_TYPE_WORDS[noun])
        preds.append(~pred if negated else pred)
    out = preds[0]
    for p in preds[1:]:
        out = out | p
    return out


def parse_ext_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for trigger conditions outside the
    literal predicate-trigger phrasebook, or ``None``:

    * ``you draw a card`` -> ``DRAWS_CARD`` for the source's controller
    * ``you cast a spell`` / ``you cast a[n] <filter> spell`` ->
      ``IS_CAST`` for spells the source's controller casts, filtered by
      :func:`_spell_filter`
    """
    from mtgz_ir.effects.predicates import YOU, Controlled, Event, Subject

    if phrase == "you draw a card":
        return (Event.DRAWS_CARD, Controlled(YOU))
    m = _YOU_CAST_A_SPELL.match(phrase)
    if m is not None:
        yours = Controlled(YOU, subject="controller")
        if m.group("filter") is None:
            return (Event.IS_CAST, yours)
        pred = _spell_filter(m.group("filter"))
        if pred is not None:
            return (Event.IS_CAST, Subject("spell", pred) & yours)
    # Cube W/G creature bucket trigger conditions.
    from mtgz_ir.cards.parser.creature_wg_effects import parse_wg_trigger_phrase

    return parse_wg_trigger_phrase(phrase)


__all__ = ["parse_ext_effect", "parse_target_noun_phrase"]
