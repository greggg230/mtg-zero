"""Parser grammar for the Vintage Roto P5 package: board-state prison
and layers (``docs/ROTO_COVERAGE_PLAN.md`` section 3).

Three entry points, each the last fallback of a shared dispatcher, so
no phrase an older recognizer claims changes meaning:

* :func:`parse_roto_board_effect` -- effect phrases (after every other
  bucket in ``effects._parse_effect``);
* :func:`parse_roto_cost_piece` -- activation-cost atoms (after the
  cube creatures bucket in ``activated._classify_cost_piece``);
* :func:`parse_roto_board_line` -- whole oracle lines a permanent
  builder declined: printing-level statics, triggers whose condition
  the trigger tables don't know, ... (``BoardLine``).

Like the cube buckets, the grammar is switched off for printings that
exist only in the EoE-era dumps (the shared ``creature_rmc_grammar``
switch), so no EoE printing's parse can change. Every recognizer is
parameterized over the varying parts of its printed shape; engine
effects live in ``effects/generic/roto_board.py`` and the board-rule
statics in ``mtgz_ir.roto_board_statics``.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from mtgz_ir.effects.base import Effect


def _disabled() -> bool:
    from mtgz_ir.cards.parser.creature_rmc_grammar import _DISABLED

    return _DISABLED[0]


@dataclass
class BoardLine:
    """What one recognized oracle line contributes to a printing."""

    fields: dict = field(default_factory=dict)
    keywords: tuple[str, ...] = ()
    triggers: tuple = ()
    activated: tuple = ()
    statics: tuple = ()


# Printing fields several lines of one card may each contribute to
# (concatenated rather than overwritten).
_ACCUMULATED_FIELDS = frozenset({"board_rules", "enters_with_named_counters"})


def merge_board_fields(into: dict, fields: dict) -> None:
    """Fold one line's printing fields into the builder's accumulator."""
    for name, value in fields.items():
        if name in _ACCUMULATED_FIELDS:
            into[name] = tuple(into.get(name, ())) + tuple(value)
        else:
            into[name] = value


_NUMBER_WORDS = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def _n(token: str) -> int:
    return int(token) if token.isdigit() else _NUMBER_WORDS[token]


# ---------------------------------------------------------------------------
# Noun phrases
# ---------------------------------------------------------------------------

_NON_SUBTYPE = re.compile(r"\bnon-(?P<sub>[A-Z][a-z]+) ")
_PT_MODIFIER = re.compile(r"^(?P<p>\d+)/(?P<t>\d+) (?P<rest>creatures?)$")


def permanent_phrase(np: str, *, plural: bool = False):
    """The compositional permanent grammar
    (``target_phrases.parse_permanent_phrase``) plus the modifiers this
    package's cards print and the base grammar lacks: ``legendary``
    (Minamo), ``non-<Subtype>`` (Emerald Charm's "non-Aura
    enchantment"), a ``P/T`` creature (Pendelhaven's "1/1 creature")
    and the plural controller ``your opponents control`` (Cryptic
    Command). ``None`` when any word is outside the grammar."""
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic.roto_board import (
        IsLegendaryPermanent,
        LacksSubtype,
    )
    from mtgz_ir.effects.object_predicates import (
        ControlledByOpponent,
        HasPowerToughness,
    )

    np = np.strip()
    if ", " in np and " or " not in np and " and " not in np:
        # "nonland, nontoken permanents": a comma-joined modifier list
        # (conjunctive), not a list of alternatives.
        np = np.replace(", ", " ")
    quals = []
    if np.endswith(" your opponents control"):
        np = np[: -len(" your opponents control")]
        quals.append(ControlledByOpponent())
    for m in list(_NON_SUBTYPE.finditer(np)):
        quals.append(LacksSubtype(m.group("sub")))
    np = _NON_SUBTYPE.sub("", np)
    if np.startswith("legendary "):
        np = np[len("legendary "):]
        quals.append(IsLegendaryPermanent())
    m = _PT_MODIFIER.match(np)
    if m is not None:
        quals.append(HasPowerToughness(int(m.group("p")), int(m.group("t"))))
        np = m.group("rest")
    base = parse_permanent_phrase(np, plural=plural)
    if base is None:
        return None
    for q in quals:
        base = base & q
    return base


# ---------------------------------------------------------------------------
# Effect phrases
# ---------------------------------------------------------------------------

# "deals damage to each player equal to [twice] the number of <np> that
# player controls" (Price of Progress).
_DAMAGE_EACH_PLAYER_PER_CONTROLLED = re.compile(
    r"^deals damage to each player equal to (?P<twice>twice )?the number of "
    r"(?P<np>.+?) that player controls$"
)
# "tap all <plural np>" (Cryptic Command).
_TAP_ALL = re.compile(r"^tap all (?P<np>.+)$")
# "destroy each <np> with mana value equal to the number of <kind>
# counters on this <permanent>" (Engineered Explosives, The Filigree
# Sylex -- whose self-name the clause grammar rewrites to "this
# creature").
_DESTROY_EACH_MV_EQUAL_COUNTERS = re.compile(
    r"^destroy each (?P<np>.+?) with mana value equal to the number of "
    r"(?P<kind>[a-z]+) counters on this "
    r"(?:artifact|creature|enchantment|land|permanent)$"
)
# "destroy each <a>, <b>, and <c> with mana value X or less" (Pernicious
# Deed): in the "each" frame an "and" list is a union.
_DESTROY_EACH_AND_LIST = re.compile(
    r"^(?P<verb>destroy|exile) each (?P<np>[a-z]+(?:, [a-z]+)*,? and [a-z]+)"
    r"(?P<rest> with mana value X or less)?$"
)


# "that player sacrifices a <np> of their choice for each <kind> counter
# on this <permanent>" (Smokestack) / "that player taps an untapped <np>
# they control for each <kind> counter on this <permanent>" (Tangle
# Wire) -- bodies of "At the beginning of each player's upkeep".
_EVENT_PLAYER_PER_COUNTER = re.compile(
    r"^that player (?:(?P<sac>sacrifices) an? (?P<sac_np>.+?) of their choice"
    r"|(?P<tap>taps) an untapped (?P<tap_np>.+?) they control) for each "
    r"(?P<kind>[a-z]+) counter on this "
    r"(?:artifact|creature|enchantment|land|permanent)$"
)
# "destroy this creature unless you pay <cost>" (the Tabernacle's
# granted upkeep ability).
_DESTROY_SELF_UNLESS_PAY = re.compile(
    r"^destroy this creature unless you pay (?P<cost>(?:\{[^}]+\})+)$"
)


# "[un]tap [<count>] target <np>" over the extended permanent grammar
# (Emerald Charm, Deceiver Exarch, Opposition, Earthcraft, Minamo).
_TAP_UNTAP_TARGET = re.compile(r"^(?P<verb>tap|untap) (?P<rest>.*target .+)$")
# "equipped creature gets +P/+T until end of turn" (Umezawa's Jitte).
_EQUIPPED_GETS_UEOT = re.compile(
    r"^equipped creature gets (?P<p>[+-]\d+)/(?P<t>[+-]\d+) until end of "
    r"turn$"
)
# "target <np> gets +P/+T until end of turn" for noun phrases the base
# pump grammar doesn't know (Pendelhaven's "target 1/1 creature").
_TARGET_GETS_UEOT = re.compile(
    r"^target (?P<np>.+?) gets (?P<p>[+-]\d+)/(?P<t>[+-]\d+) until end of "
    r"turn$"
)


# "target <np> loses <keyword> until end of turn" (Emerald Charm).
_LOSES_KEYWORD_UEOT = re.compile(
    r"^target (?P<np>.+?) loses (?P<kw>[a-z]+(?: [a-z]+)?) until end of turn$"
)
# "<plural np> you control gain <keywords> until end of turn" for noun
# phrases the base grammar doesn't cover (Boros Charm's "permanents").
_EACH_GAINS_UEOT = re.compile(
    r"^(?P<np>[a-z]+) you control gain (?P<kws>[a-z ,]+?) until end of turn$"
)
# "deals N damage to target <player-or-permanent list>" (Boros Charm's
# "target player or planeswalker").
_DAMAGE_TO_TARGET_LIST = re.compile(
    r"^deals (?P<n>\d+) damage to target (?P<np>.+)$"
)
# "deals N damage to each of one or two targets" (Prismari Charm).
_DAMAGE_EACH_OF_TARGETS = re.compile(
    r"^deals (?P<n>\d+) damage to each of (?P<count>one or two|up to "
    r"(?:two|three)) targets$"
)
# "destroy target <np>" over the extended permanent grammar (Emerald
# Charm's "non-Aura enchantment").
_DESTROY_TARGET = re.compile(r"^destroy target (?P<np>.+)$")


# "return a <permanent> you control to its owner's hand" (Shrieking
# Drake) is P3's ``roto_mana`` recognizer (Simic Growth Chamber's shape,
# tried first); P5's identical copy was removed at integration.
# "<plural np> you control gain <keywords> and get +X/+X until end of
# turn, where X is the number of <plural np> you control" (Craterhoof).
_EACH_GAINS_AND_GETS_X = re.compile(
    r"^(?P<np>[a-z]+) you control gain (?P<kws>[a-z ,]+?) and get \+X/\+X "
    r"until end of turn, where X is the number of (?P<count>[a-z]+) you "
    r"control$"
)
# "<pump>. If you cast this spell during your main phase, that
# creature gets +P/+T until end of turn instead" (Might of Old Krosa).
_MAIN_PHASE_INSTEAD = re.compile(
    r"^(?P<base>target (?P<np>.+?) gets \+\d+/\+\d+ until end of turn)\. If "
    r"you cast this spell during your main phase, that creature gets "
    r"(?P<pt>\+\d+/\+\d+) until end of turn instead$"
)
# "you may put a creature card with mana value equal to the number of
# <kind> counters on this <permanent> from your hand onto the battlefield"
# (Aether Vial).
_PUT_MV_EQUAL_COUNTERS_FROM_HAND = re.compile(
    r"^you may put a creature card with mana value equal to the number of "
    r"(?P<kind>[a-z]+) counters on this (?:artifact|creature|enchantment|"
    r"land|permanent) from your hand onto the battlefield$"
)
# "[you may ]destroy [up to N ]target <np>. For each permanent put into
# a graveyard this way, its controller creates <token>" (Terastodon).
_DESTROY_THEN_CONTROLLER_TOKENS = re.compile(
    r"^(?P<may>you may )?destroy (?:up to (?P<n>one|two|three|four) )?target "
    r"(?P<np>.+?)\. For each permanent put into a graveyard this way, its "
    r"controller creates (?P<tok>.+)$"
)
_SELF_NOUN_LEAVES = (
    r"until this (?:enchantment|artifact|creature|permanent) leaves the "
    r"battlefield$"
)
# "exile [up to one ][other ]target <np> until this <permanent> leaves
# the battlefield" over the extended grammar (Aang's Iceberg).
_EXILE_TARGET_UNTIL_LEAVES = re.compile(
    r"^exile (?P<upto>up to one )?(?P<other>other )?target (?P<np>.+?) "
    + _SELF_NOUN_LEAVES
)
# "exile each <np> until this <permanent> leaves the battlefield"
# (Temporary Lockdown).
_EXILE_EACH_UNTIL_LEAVES = re.compile(
    r"^exile each (?P<np>.+?) " + _SELF_NOUN_LEAVES
)
# Turnabout: "choose <t1>, <t2>, or <t3>. Tap all untapped permanents of
# the chosen type target player controls, or untap all tapped permanents
# of that type that player controls".
_TAP_OR_UNTAP_CHOSEN_TYPE = re.compile(
    r"^choose (?P<types>[a-z]+, [a-z]+, or [a-z]+)\. Tap all untapped "
    r"permanents of the chosen type target player controls, or untap all "
    r"tapped permanents of that type that player controls$"
)


# "sacrifice this creature unless you discard a card[. When you discard
# a card this way, <destroy|exile> target <np> with mana value less than
# or equal to the mana value of the discarded card]" (Argentum Masticore).
_SAC_UNLESS_DISCARD = re.compile(
    r"^sacrifice this (?:creature|artifact|permanent) unless you discard a "
    r"card(?:\. When you discard a card this way, (?P<verb>destroy|exile) "
    r"target (?P<np>.+?) with mana value less than or equal to the mana "
    r"value of the discarded card)?$"
)
# "return any number of target <np> you control to their owners' hands.
# Draw a card for each card returned to your hand this way" (Paradoxical
# Outcome).
_RETURN_ANY_NUMBER_DRAW = re.compile(
    r"^return any number of target (?P<np>.+?) you control to their owners' "
    r"hands\. Draw a card for each card returned to your hand this way$"
)


_COLOR_WORDS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
# "until end of turn, <target|each> <np> becomes a <color> <Subtypes>
# with base power and toughness P/T" (Scale Up and its overload).
_BECOMES_COLORED_SUBTYPE_UEOT = re.compile(
    r"^until end of turn, (?P<who>target|each) (?P<np>.+?) becomes an? "
    r"(?P<color>white|blue|black|red|green) (?P<subs>[A-Z][a-z]+"
    r"(?: [A-Z][a-z]+)*) with base power and toughness (?P<p>\d+)/(?P<t>\d+)$"
)
# "until end of turn, target <np> gets +1/+1 for each supertype, card
# type, and subtype it has" (Embiggen).
_PUMP_PER_TYPES_UEOT = re.compile(
    r"^until end of turn, target (?P<np>.+?) gets \+(?P<p>\d+)/\+(?P<t>\d+) "
    r"for each supertype, card type, and subtype it has$"
)


# "target <np> can't be the target of spells or abilities your
# opponents control this turn[. If this spell was kicked, that creature
# gets +P/+T until end of turn]" (Vines of Vastwood).
_UNTARGETABLE_BY_OPPONENTS = re.compile(
    r"^target (?P<np>.+?) can't be the target of spells or abilities your "
    r"opponents control this turn(?:\. If this spell was kicked, that "
    r"creature gets \+(?P<p>\d+)/\+(?P<t>\d+) until end of turn)?$"
)
# "target <np> gains <keywords> and gets +X/+0 until end of turn, where
# X is its power. At the beginning of the next end step, destroy that
# creature if it attacked this turn" (Berserk).
_POWER_PUMP_DESTROY_IF_ATTACKED = re.compile(
    r"^target (?P<np>.+?) gains (?P<kws>[a-z ,]+?) and gets \+X/\+0 until "
    r"end of turn, where X is its power\. At the beginning of the next end "
    r"step, destroy that creature if it attacked this turn$"
)


# "<e1> if this is the first time this ability has resolved this turn.
# If it's the second time, <e2>. If it's the third time, <e3>" (Omnath).
_BY_RESOLUTION_COUNT = re.compile(
    r"^(?P<e1>.+?) if this is the first time this ability has resolved this "
    r"turn\. If it's the second time, (?P<e2>.+?)\. If it's the third time, "
    r"(?P<e3>.+)$"
)
# "deals N damage to each opponent and each planeswalker you don't
# control" (Omnath).
_DAMAGE_OPPONENTS_AND_WALKERS = re.compile(
    r"^(?:this creature )?deals (?P<n>\d+) damage to each opponent and each "
    r"planeswalker you don't control$"
)
# "destroy the creature with the least power. It can't be regenerated.
# If two or more creatures are tied for least power, you choose one of
# them" (Drop of Honey).
_DESTROY_LEAST_POWER = re.compile(
    r"^destroy the creature with the least power(?P<noregen>\. It can't be "
    r"regenerated)?\. If two or more creatures are tied for least power, you "
    r"choose one of them$"
)
# "the owner of target <np> shuffles it into their library. Then that
# player discovers X, where X is its mana value" (Zoyowa's Justice).
_SHUFFLE_THEN_DISCOVER = re.compile(
    r"^the owner of target (?P<np>.+?) shuffles it into their library\. Then "
    r"that player discovers X, where X is its mana value$"
)
_MV_AT_LEAST_SUFFIX = re.compile(r" with mana value (?P<n>\d+) or greater$")


# "target <np> gets +P/+T until end of turn. Create a <Name> Role token
# attached to it" (Monstrous Rage).
_PUMP_THEN_ROLE = re.compile(
    r"^target (?P<np>.+?) gets \+(?P<p>\d+)/\+(?P<t>\d+) until end of turn\. "
    r"Create an? (?P<role>[A-Z][a-z]+) Role token attached to it$"
)


# Kozilek's Command's X modes.
_TARGET_PLAYER_CREATES_X = re.compile(
    r"^target player creates X (?P<tok>.+? tokens?(?: with .+)?)$"
)
_TARGET_PLAYER_SCRIES_X_DRAWS = (
    "target player scries X, then draws a card"
)
_EXILE_TARGET_MV_AT_MOST_X = re.compile(
    r"^exile target (?P<np>.+?) with mana value X or less$"
)
_EXILE_UP_TO_X_GRAVEYARD_CARDS = (
    "exile up to X target cards from graveyards"
)


# Veil of Summer.
_DRAW_IF_OPPONENT_CAST_COLOR = re.compile(
    r"^draw (?P<n>a|two|three) cards? if an opponent has cast an? "
    r"(?P<c1>white|blue|black|red|green)(?: or (?P<c2>white|blue|black|red|"
    r"green))? spell this turn$"
)
# A literal: Veil of Summer is the only card in the pool with this
# player-scoped "this turn" shape (generalization deferred until a
# second example exists).
_YOUR_SPELLS_UNCOUNTERABLE_THIS_TURN = (
    "spells you control can't be countered this turn"
)
_YOU_AND_PERMANENTS_HEXPROOF_FROM = re.compile(
    r"^you and permanents you control gain hexproof from (?P<q1>white|blue|"
    r"black|red|green)(?: and from (?P<q2>white|blue|black|red|green))? "
    r"until end of turn$"
)


_TARGET_OPPONENT_DRAWS = re.compile(
    r"^target opponent draws (?P<n>a|two|three) cards?$"
)
# "exile up to one target <np>. If you do, this creature gains
# "<triggered ability>"" (Thief of Existence; the name was rewritten).
_EXILE_IF_YOU_DO_GAINS = re.compile(
    r'^(?P<exile>exile (?:up to one )?target .+?)\. If you do, this '
    r'(?:creature|permanent) gains "(?P<ability>.+\.)"$'
)


def _keyword_list(text: str):
    from mtgz_ir.cards.parser.keywords import _parse_keyword_line

    words = [w.strip() for w in re.split(r", and |, | and ", text) if w.strip()]
    out = set()
    for w in words:
        kws = _parse_keyword_line(w.capitalize())
        if not kws or len(kws) != 1:
            return None
        out.add(kws[0])
    return frozenset(out) if out else None


def _player_or_permanent_filter(np: str):
    """"player or planeswalker" / "opponent or creature" -- a target
    list mixing players with the permanent grammar."""
    from mtgz_ir.effects.object_predicates import ControlledByOpponent, IsPlayer

    arms = []
    has_player = False
    for item in np.split(" or "):
        item = item.strip()
        if item == "player":
            arms.append(IsPlayer())
            has_player = True
        elif item == "opponent":
            arms.append(IsPlayer() & ControlledByOpponent())
            has_player = True
        else:
            f = permanent_phrase(item)
            if f is None:
                return None
            arms.append(f)
    if not has_player:
        return None
    out = arms[0]
    for a in arms[1:]:
        out = out | a
    return out


def _target_slot(rest: str):
    from mtgz_ir.cards.parser.target_phrases import parse_target_count
    from mtgz_ir.effects.targets import TargetSpec

    counted = parse_target_count(rest)
    if counted is None:
        return None
    lo, hi, np = counted
    if hi == "X":
        return None
    f = permanent_phrase(np)
    if f is None:
        return None
    return TargetSpec(f, count_min=lo, count_max=hi)


def parse_roto_board_effect(phrase: str) -> Effect | None:
    """Effect phrases of the P5 package (normalized: lower-cased first
    letter, no trailing period, source subject stripped)."""
    if _disabled():
        return None
    from mtgz_ir.effects.generic import roto_board as rb

    m = _DAMAGE_EACH_PLAYER_PER_CONTROLLED.match(phrase)
    if m is not None:
        f = permanent_phrase(m.group("np"), plural=True)
        if f is None:
            return None
        return rb.DamageEachPlayerPerControlled(
            per_filter=f, multiplier=2 if m.group("twice") else 1,
            label=m.group("np"),
        )
    m = _EVENT_PLAYER_PER_COUNTER.match(phrase)
    if m is not None:
        action = "sacrifice" if m.group("sac") else "tap"
        np = m.group("sac_np") or m.group("tap_np")
        f = permanent_phrase(np)
        if f is None:
            return None
        return rb.EventPlayerActsPerSourceCounter(
            action=action, kind=m.group("kind"), permanent_filter=f, label=np,
        )
    m = _DESTROY_SELF_UNLESS_PAY.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import parse_mana_cost

        try:
            parse_mana_cost(m.group("cost"))
        except ValueError:
            return None
        return rb.DestroySourceUnlessPay(cost=m.group("cost"))
    m = _TAP_UNTAP_TARGET.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.tap import Tap, Untap

        slot = _target_slot(m.group("rest"))
        if slot is None:
            return None
        cls = Tap if m.group("verb") == "tap" else Untap
        return cls(targets=(slot,))
    m = _TARGET_OPPONENT_DRAWS.match(phrase)
    if m is not None:
        return rb.TargetOpponentDraws(amount=_n(m.group("n")))
    m = _EXILE_IF_YOU_DO_GAINS.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
        from mtgz_ir.effects.base import TriggeredAbility

        inner = _parse_effect(m.group("exile"))
        ability = _parse_triggered_ability_line(m.group("ability"))
        if inner is None or not isinstance(ability, TriggeredAbility):
            return None
        return rb.ExileThenSourceGainsTrigger(inner=inner, ability=ability)
    m = _DRAW_IF_OPPONENT_CAST_COLOR.match(phrase)
    if m is not None:
        colors = {_COLOR_WORDS[m.group("c1")]}
        if m.group("c2"):
            colors.add(_COLOR_WORDS[m.group("c2")])
        return rb.DrawIfOpponentCastColorThisTurn(
            colors=frozenset(colors), amount=_n(m.group("n")),
        )
    if phrase == _YOUR_SPELLS_UNCOUNTERABLE_THIS_TURN:
        return rb.YourSpellsCantBeCounteredThisTurn()
    if phrase == "the next spell you cast this turn can't be countered":
        # Mistrise Village -- a literal: the only card in the pool with
        # this shape (a "next <filter> spell" grammar is deferred until
        # a second example exists).
        return rb.NextSpellThisTurnCantBeCountered()
    m = _YOU_AND_PERMANENTS_HEXPROOF_FROM.match(phrase)
    if m is not None:
        qualities = {m.group("q1")}
        if m.group("q2"):
            qualities.add(m.group("q2"))
        return rb.YouAndPermanentsGainHexproofFromUEOT(
            qualities=frozenset(qualities),
        )
    m = _TARGET_PLAYER_CREATES_X.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.generic.tokens import CreateToken

        token = _parse_effect(f"create two {m.group('tok')}")
        if not isinstance(token, CreateToken) or token.count != 2:
            return None
        return rb.TargetPlayerCreatesXTokens(token_printing=token.token_printing)
    if phrase == _TARGET_PLAYER_SCRIES_X_DRAWS:
        return rb.TargetPlayerScriesXThenDraws()
    m = _EXILE_TARGET_MV_AT_MOST_X.match(phrase)
    if m is not None:
        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return rb.RemoveTargetWithMVAtMostX(
            action="exile", target_filter=f & rb.ManaValueAtMostX(),
        )
    if phrase == _EXILE_UP_TO_X_GRAVEYARD_CARDS:
        return rb.ExileUpToXGraveyardCards()
    m = _PUMP_THEN_ROLE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        if f is None or m.group("role") not in rb.ROLE_TOKENS:
            return None
        return rb.PumpThenCreateRoleAttached(
            role=m.group("role"), power_bonus=int(m.group("p")),
            toughness_bonus=int(m.group("t")), targets=(TargetSpec(f),),
        )
    m = _BY_RESOLUTION_COUNT.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        parts = [_parse_effect(m.group(k)) for k in ("e1", "e2", "e3")]
        if any(p is None for p in parts):
            return None
        return rb.ByResolutionCountThisTurn(by_count=tuple(parts))
    m = _DAMAGE_OPPONENTS_AND_WALKERS.match(phrase)
    if m is not None:
        return rb.DealDamageToEachOpponentAndTheirPlaneswalkers(
            amount=int(m.group("n")),
        )
    m = _DESTROY_LEAST_POWER.match(phrase)
    if m is not None:
        return rb.DestroyLeastPowerCreature(
            can_be_regenerated=m.group("noregen") is None,
        )
    m = _SHUFFLE_THEN_DISCOVER.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        np = m.group("np")
        mv = _MV_AT_LEAST_SUFFIX.search(np)
        quals = []
        if mv is not None:
            quals.append(rb.HasManaValueAtLeast(int(mv.group("n"))))
            np = np[: mv.start()]
        f = permanent_phrase(np)
        if f is None:
            return None
        for q in quals:
            f = f & q
        return rb.OwnerShufflesTargetThenDiscovers(targets=(TargetSpec(f),))
    m = _UNTARGETABLE_BY_OPPONENTS.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return rb.UntargetableByOpponentsThisTurn(
            kicked_power=int(m.group("p") or 0),
            kicked_toughness=int(m.group("t") or 0),
            targets=(TargetSpec(f),),
        )
    m = _POWER_PUMP_DESTROY_IF_ATTACKED.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        kws = _keyword_list(m.group("kws"))
        if f is None or kws is None:
            return None
        return rb.PowerPumpThenDestroyIfAttacked(
            keywords=kws, targets=(TargetSpec(f),),
        )
    m = _BECOMES_COLORED_SUBTYPE_UEOT.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        kwargs = dict(
            colors=frozenset({_COLOR_WORDS[m.group("color")]}),
            creature_subtypes=tuple(m.group("subs").split()),
            base_pt=(int(m.group("p")), int(m.group("t"))),
        )
        if m.group("who") == "each":
            return rb.BecomeColoredSubtypeBasePTUEOT(each_filter=f, **kwargs)
        return rb.BecomeColoredSubtypeBasePTUEOT(
            targets=(TargetSpec(f),), **kwargs,
        )
    m = _PUMP_PER_TYPES_UEOT.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return rb.PumpPerTypeCountUEOT(
            per_power=int(m.group("p")), per_toughness=int(m.group("t")),
            targets=(TargetSpec(f),),
        )
    m = _SAC_UNLESS_DISCARD.match(phrase)
    if m is not None:
        if m.group("verb") is None:
            return rb.SacrificeSourceUnlessDiscard()
        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return rb.SacrificeSourceUnlessDiscard(
            reflexive_action=m.group("verb"), reflexive_filter=f,
        )
    m = _RETURN_ANY_NUMBER_DRAW.match(phrase)
    if m is not None:
        from mtgz_ir.effects.object_predicates import ControlledByYou

        f = permanent_phrase(m.group("np"), plural=True)
        if f is None:
            return None
        return rb.ReturnAnyNumberDrawPerOwned(target_filter=f & ControlledByYou())
    m = _EACH_GAINS_AND_GETS_X.match(phrase)
    if m is not None:
        f = permanent_phrase(m.group("np"), plural=True)
        count = permanent_phrase(m.group("count"), plural=True)
        kws = _keyword_list(m.group("kws"))
        if f is None or count is None or kws is None:
            return None
        return rb.PumpEachYouControlByCountUEOT(
            permanent_filter=f, count_filter=count, keywords=kws,
            label=f"{m.group('np')} you control",
        )
    m = _MAIN_PHASE_INSTEAD.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        base = _parse_effect(m.group("base"))
        stronger = _parse_effect(
            f"target {m.group('np')} gets {m.group('pt')} until end of turn"
        )
        if base is None or stronger is None or type(base) is not type(stronger):
            return None
        return rb.IfResolvingInYourMainPhase(main_phase=stronger, otherwise=base)
    m = _PUT_MV_EQUAL_COUNTERS_FROM_HAND.match(phrase)
    if m is not None:
        return rb.PutCreatureWithMVEqualToCountersFromHand(kind=m.group("kind"))
    m = _DESTROY_THEN_CONTROLLER_TOKENS.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.combinators import MayDo
        from mtgz_ir.effects.generic.tokens import CreateToken

        f = permanent_phrase(m.group("np"), plural=m.group("n") is not None)
        token = _parse_effect(f"create {m.group('tok')}")
        if f is None or not isinstance(token, CreateToken):
            return None
        n = _NUMBER_WORDS[m.group("n")] if m.group("n") else 1
        eff = rb.DestroyTargetsControllersGetTokens(
            target_filter=f, token_printing=token.token_printing,
            count_min=0 if m.group("n") else 1, count_max=n,
            tokens_each=token.count,
        )
        return MayDo(inner=eff) if m.group("may") else eff
    m = _EXILE_TARGET_UNTIL_LEAVES.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.enchantment_mechanics import (
            ExileTargetUntilSourceLeaves,
        )
        from mtgz_ir.effects.object_predicates import IsSource
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        if m.group("other"):
            f = f & ~IsSource()
        return ExileTargetUntilSourceLeaves(targets=(TargetSpec(
            f, count_min=0 if m.group("upto") else 1, count_max=1,
        ),))
    m = _EXILE_EACH_UNTIL_LEAVES.match(phrase)
    if m is not None:
        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return rb.ExileEachUntilSourceLeaves(
            permanent_filter=f, label=m.group("np"),
        )
    m = _TAP_OR_UNTAP_CHOSEN_TYPE.match(phrase)
    if m is not None:
        types = tuple(
            t.capitalize() for t in re.split(r", or |, ", m.group("types"))
        )
        if not all(t in ("Artifact", "Creature", "Land", "Enchantment") for t in types):
            return None
        return rb.TapOrUntapAllOfChosenType(types=types)
    m = _LOSES_KEYWORD_UEOT.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        kws = _keyword_list(m.group("kw"))
        if f is None or kws is None or len(kws) != 1:
            return None
        return rb.LoseKeywordUEOT(keyword=next(iter(kws)), targets=(TargetSpec(f),))
    m = _EACH_GAINS_UEOT.match(phrase)
    if m is not None:
        from mtgz_ir.effects.object_predicates import ControlledByYou

        f = permanent_phrase(m.group("np"), plural=True)
        kws = _keyword_list(m.group("kws"))
        if f is None or kws is None:
            return None
        return rb.GrantKeywordsToEachUEOT(
            permanent_filter=f & ControlledByYou(), keywords=kws,
            label=f"{m.group('np')} you control",
        )
    m = _DAMAGE_EACH_OF_TARGETS.match(phrase)
    if m is not None:
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter

        count = m.group("count")
        lo, hi = (1, 2) if count == "one or two" else (
            0, _NUMBER_WORDS[count.split()[-1]],
        )
        return rb.DealDamageToEachTarget(
            amount=int(m.group("n")),
            targets=(TargetSpec(any_target_filter(), count_min=lo, count_max=hi),),
        )
    m = _DAMAGE_TO_TARGET_LIST.match(phrase)
    if m is not None and " or " in m.group("np"):
        from mtgz_ir.effects.generic.damage import DealDamage
        from mtgz_ir.effects.targets import TargetSpec

        f = _player_or_permanent_filter(m.group("np"))
        if f is None:
            return None
        return DealDamage(amount=int(m.group("n")), targets=(TargetSpec(f),))
    m = _DESTROY_TARGET.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.targeted_removal import (
            RemoveTargetPermanents,
        )

        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return RemoveTargetPermanents(action="destroy", target_filter=f)
    m = _EQUIPPED_GETS_UEOT.match(phrase)
    if m is not None:
        return rb.PumpEquippedCreatureUEOT(
            power_bonus=int(m.group("p")), toughness_bonus=int(m.group("t")),
        )
    m = _TARGET_GETS_UEOT.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.pump import Pump
        from mtgz_ir.effects.targets import TargetSpec

        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        return Pump(
            power_bonus=int(m.group("p")), toughness_bonus=int(m.group("t")),
            targets=(TargetSpec(f),),
        )
    m = _TAP_ALL.match(phrase)
    if m is not None:
        f = permanent_phrase(m.group("np"), plural=True)
        if f is None:
            return None
        return rb.TapAllMatching(permanent_filter=f, label=m.group("np"))
    m = _DESTROY_EACH_MV_EQUAL_COUNTERS.match(phrase)
    if m is not None:
        f = permanent_phrase(m.group("np"))
        if f is None:
            return None
        from mtgz_ir.effects.generic.targeted_removal import RemoveAllMatching

        return RemoveAllMatching(
            permanent_filter=f & rb.HasManaValueEqualToSourceCounters(
                m.group("kind"),
            ),
            action="destroy",
            label=(
                f"each {m.group('np')} with mana value equal to the number "
                f"of {m.group('kind')} counters on this permanent"
            ),
        )
    m = _DESTROY_EACH_AND_LIST.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.removal_phrases import parse_destroy_all

        union = m.group("np").replace(", and ", ", or ").replace(" and ", " or ")
        return parse_destroy_all(
            f"{m.group('verb')} each {union}{m.group('rest') or ''}"
        )
    return None


# ---------------------------------------------------------------------------
# Characteristic-defining P/T
# ---------------------------------------------------------------------------

_GOYF_YOUR_GRAVEYARD = re.compile(
    r"^.+?'s power is equal to the number of card types among cards in your "
    r"graveyard and its toughness is equal to that number plus (?P<k>\d+)\.$"
)


def parse_roto_dynamic_pt_line(line: str):
    """``dynamic_pt_formula`` for the P5 package's characteristic-
    defining P/T lines (Nethergoyf), or ``None``."""
    if _disabled():
        return None
    from mtgz_ir.effects.generic import roto_board as rb

    m = _GOYF_YOUR_GRAVEYARD.match(line)
    if m is not None:
        return rb.CardTypesInYourGraveyardPT(plus=int(m.group("k")))
    return None


# ---------------------------------------------------------------------------
# Trigger conditions
# ---------------------------------------------------------------------------


def parse_roto_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for the P5 package's trigger conditions:
    "equipped creature deals combat damage" (any recipient, once per
    damage step -- Umezawa's Jitte) and "this creature becomes blocked"
    (Ichorclaw Myr)."""
    if _disabled():
        return None
    from mtgz_ir.effects.predicates import Event, IsHost, IsSelf

    if phrase == "equipped creature deals combat damage":
        return (Event.DEALS_COMBAT_DAMAGE_ANY, IsHost(subject="source"))
    if phrase == "this creature becomes blocked":
        return (Event.BECOMES_BLOCKED, IsSelf(subject="attacker"))
    return None


# ---------------------------------------------------------------------------
# Cost atoms
# ---------------------------------------------------------------------------

# "Sacrifice this enchantment" (Seal of Cleansing, Pernicious Deed) is
# P1's ``roto_stack.parse_roto_cost_piece`` (tried first in
# ``activated._classify_cost_piece``); P5 had the same recognizer, removed
# when the packages were integrated.
# "Remove ten oil counters from among permanents you control" (The
# Filigree Sylex).
_REMOVE_COUNTERS_FROM_AMONG = re.compile(
    r"^Remove (?P<n>\w+) (?P<kind>[a-z]+) counters? from among permanents "
    r"you control$"
)


def parse_roto_cost_piece(piece: str):
    """Activation-cost atoms of the P5 package -> ``activated._CostPiece``
    (``None`` when the piece isn't one)."""
    if _disabled():
        return None
    from mtgz_ir.cards.parser.activated import _CostPiece
    from mtgz_ir.effects.generic import roto_board as rb
    from mtgz_ir.effects.generic.cube_creatures_rmc import SacrificePermanentsCost
    from mtgz_ir.effects.value_predicates import IsPermanent

    m = _REMOVE_COUNTERS_FROM_AMONG.match(piece)
    if m is not None and (m.group("n").isdigit() or m.group("n") in _NUMBER_WORDS):
        return _CostPiece(kind="extra", atom=rb.RemoveCountersFromAmongPermanentsCost(
            kind=m.group("kind"), count=_n(m.group("n")),
        ))
    if piece == "Put a -1/-1 counter on this creature":
        # Devoted Druid.
        return _CostPiece(kind="extra", atom=rb.PutMinusOneCounterOnSelfCost())
    if piece == "Sacrifice a permanent":
        # Claws of Gix: any permanent you control, the source included.
        return _CostPiece(kind="extra", atom=SacrificePermanentsCost(
            permanent_filter=IsPermanent(), label="a permanent",
        ))
    return None


# ---------------------------------------------------------------------------
# Whole lines
# ---------------------------------------------------------------------------


# "<costs>, <cost atom> and sacrifice <this>: <effect>" -- the last two
# cost atoms joined by "and" instead of a comma (The Filigree Sylex's
# "Remove ten oil counters from among permanents you control and
# sacrifice The Filigree Sylex").
_AND_SACRIFICE_SELF_COST = re.compile(
    r"^(?P<costs>[^:]+?) and sacrifice this (?P<noun>creature|artifact|"
    r"enchantment|land): (?P<effect>.+)$"
)


def _activated_with_and_joined_sacrifice(line: str) -> BoardLine | None:
    from mtgz_ir.cards.parser.activated import _parse_activated_ability_line

    m = _AND_SACRIFICE_SELF_COST.match(line)
    if m is None:
        return None
    rewritten = (
        f"{m.group('costs')}, Sacrifice this {m.group('noun')}: "
        f"{m.group('effect')}"
    )
    ability = _parse_activated_ability_line(rewritten)
    if ability is None or not ability.cost.sacrifice_self:
        return None
    return BoardLine(activated=(ability,))


# "Creatures [<condition>] can't attack." / "This creature can't block."
# (Moat, Ensnaring Bridge, Carrion Feeder).
_CANT_ATTACK_OR_BLOCK = re.compile(
    r"^(?P<who>Creatures|[Tt]his creature)(?: (?P<cond>.+?))? "
    r"can't (?P<verb>attack|block)\.$"
)
_WITHOUT_KEYWORD = re.compile(r"^without (?P<kw>[a-z]+(?: [a-z]+)?)$")
_POWER_GREATER_THAN_HAND = (
    "with power greater than the number of cards in your hand"
)


def _combat_condition(text: str | None):
    from mtgz_ir import roto_board_statics as rs

    if text is None:
        return None
    m = _WITHOUT_KEYWORD.match(text)
    if m is not None:
        from mtgz_ir.cards.parser.keywords import _parse_keyword_line

        kws = _parse_keyword_line(m.group("kw").capitalize())
        if not kws or len(kws) != 1:
            return False
        return rs.WithoutKeyword(kws[0])
    if text == _POWER_GREATER_THAN_HAND:
        return rs.PowerGreaterThanControllersHandSize()
    return False


def _cant_attack_or_block(line: str) -> BoardLine | None:
    from mtgz_ir import roto_board_statics as rs

    m = _CANT_ATTACK_OR_BLOCK.match(line)
    if m is None:
        return None
    self_only = m.group("who") != "Creatures"
    cond = _combat_condition(m.group("cond"))
    if cond is False or (self_only and cond is not None):
        return None
    rule_cls = rs.CantAttack if m.group("verb") == "attack" else rs.CantBlock
    return BoardLine(fields={
        "board_rules": (rule_cls(condition=cond, self_only=self_only),),
    })


_FADING = re.compile(r"^Fading (?P<n>\d+)$")


def _fading(line: str) -> BoardLine | None:
    """``Fading N`` on a non-enchantment permanent (CR 702.32a: enters
    with N fade counters; at the beginning of your upkeep, remove one,
    or sacrifice it if you can't) -- Tangle Wire. The cube enchantment
    builder has its own recognizer for enchantments."""
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.enchantment_mechanics import (
        RemoveNamedCounterFromSelfOrSacrifice,
    )
    from mtgz_ir.effects.predicates import YOU, Controlled, Event

    m = _FADING.match(line)
    if m is None:
        return None
    return BoardLine(
        fields={"enters_with_named_counters": (("fade", int(m.group("n"))),)},
        triggers=(TriggeredAbility(
            event=Event.BEGINNING_OF_UPKEEP,
            predicate=Controlled(YOU),
            effect=RemoveNamedCounterFromSelfOrSacrifice(kind="fade"),
        ),),
    )


# 'All creatures have "<triggered ability>".' (The Tabernacle at
# Pendrell Vale).
_ALL_CREATURES_HAVE_QUOTED = re.compile(
    r'^All creatures have "(?P<ability>.+\.)"$'
)


def _all_creatures_have_trigger(line: str) -> BoardLine | None:
    from mtgz_ir import roto_board_statics as rs
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    from mtgz_ir.effects.base import TriggeredAbility

    m = _ALL_CREATURES_HAVE_QUOTED.match(line)
    if m is None:
        return None
    ability = _parse_triggered_ability_line(m.group("ability"))
    if not isinstance(ability, TriggeredAbility):
        return None
    return BoardLine(fields={
        "board_rules": (rs.GrantsTriggerToCreatures(ability=ability),),
    })


# "<Permanents> don't untap during their controllers' untap steps."
# (Back to Basics).
_DONT_UNTAP = re.compile(
    r"^(?P<np>[A-Z][a-z]+(?: [a-z]+)*) don't untap during their "
    r"controllers' untap steps\.$"
)
# "[As long as this <permanent> is untapped, ]players can't untap more
# than <n> <permanents> during their untap steps." (Winter Orb).
_UNTAP_LIMIT = re.compile(
    r"^(?:As long as this (?P<self>artifact|creature|enchantment|land|"
    r"permanent) is untapped, )?[Pp]layers can't untap more than "
    r"(?P<n>one|two|three|\d+) (?P<np>[a-z]+(?: [a-z]+)*) during their "
    r"untap steps\.$"
)


def _untap_rules(line: str) -> BoardLine | None:
    from mtgz_ir import roto_board_statics as rs

    m = _DONT_UNTAP.match(line)
    if m is not None:
        np = m.group("np").lower()
        f = permanent_phrase(np, plural=True)
        if f is None:
            return None
        return BoardLine(fields={
            "board_rules": (rs.DoesntUntap(permanent_filter=f, label=np),),
        })
    m = _UNTAP_LIMIT.match(line)
    if m is not None:
        n = _n(m.group("n"))
        np = m.group("np")
        f = permanent_phrase(np, plural=n != 1)
        if f is None:
            return None
        return BoardLine(fields={"board_rules": (rs.UntapLimit(
            permanent_filter=f, limit=n,
            while_source_untapped=m.group("self") is not None,
            label=np,
        ),)})
    return None


# "Each land is a <basic land type> in addition to its other land types."
# (Urborg, Tomb of Yawgmoth; Yavimaya, Cradle of Growth) -- the engine's
# layer-4 ``grants_basic_land_type_to_all_lands`` synthesis (CR 613.7
# dependency on Blood Moon included).
_EACH_LAND_IS_TYPE = re.compile(
    r"^Each land is an? (?P<type>Plains|Island|Swamp|Mountain|Forest) in "
    r"addition to its other land types\.$"
)


def _each_land_is_type(line: str) -> BoardLine | None:
    m = _EACH_LAND_IS_TYPE.match(line)
    if m is None:
        return None
    return BoardLine(fields={
        "grants_basic_land_type_to_all_lands": m.group("type"),
    })


def _nonbasic_lands_are_mountains(line: str) -> BoardLine | None:
    """"Nonbasic lands are Mountains." on a noncreature permanent (Blood
    Moon) -- the printing flag Magus of the Moon's creature grammar
    already sets (``printing_keywords``)."""
    from mtgz_ir.cards.parser.printing_keywords import (
        _parse_nonbasic_lands_are_mountains_line,
    )

    if not _parse_nonbasic_lands_are_mountains_line(line):
        return None
    return BoardLine(fields={"nonbasic_lands_are_mountains": True})


# "Whenever equipped creature <condition>, it <body>." -- "it" is the
# equipped creature, not the Equipment (Hawkeye's Bow). These conditions
# stay out of the shared trigger tables, whose effect normalizer would
# read "it" as the trigger's source.
_EQUIPPED_IT_TRIGGER = re.compile(
    r"^Whenever equipped creature (?P<cond>becomes tapped|attacks), it "
    r"(?P<body>.+)\.$"
)
_HOST_DEALS_DAMAGE_EACH_OPPONENT = re.compile(
    r"^deals (?P<n>\d+) damage to each opponent$"
)


def _equipped_it_trigger(line: str) -> BoardLine | None:
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic import roto_board as rb
    from mtgz_ir.effects.predicates import Event, IsHost

    m = _EQUIPPED_IT_TRIGGER.match(line)
    if m is None:
        return None
    event, slot = {
        "becomes tapped": (Event.BECOMES_TAPPED, "obj"),
        "attacks": (Event.ATTACKS, "attacker"),
    }[m.group("cond")]
    body = _HOST_DEALS_DAMAGE_EACH_OPPONENT.match(m.group("body"))
    if body is None:
        return None
    return BoardLine(triggers=(TriggeredAbility(
        event=event, predicate=IsHost(subject=slot),
        effect=rb.EventSubjectDealsDamageToEachOpponent(
            amount=int(body.group("n")), subject_slot=slot,
        ),
    ),))


# "This creature gets +P/+T as long as there are N or more <type> cards
# in your graveyard." (Elvish Reclaimer) / "This creature gets +P/+T for
# each <type> card in your graveyard." (Knight of the Reliquary).
_SELF_GETS_PER_GRAVEYARD = re.compile(
    r"^[Tt]his creature gets \+(?P<p>\d+)/\+(?P<t>\d+) (?:as long as there "
    r"are (?P<n>\w+) or more (?P<desc>[a-z]+) cards in your graveyard|for "
    r"each (?P<each>[a-z]+) card in your graveyard)\.$"
)
# "Each other creature you control that's a <T1>, <T2>, or <T3> gets
# +P/+T and has <keyword>." (Kaheera, the Orphanguard).
_OTHER_SUBTYPE_LIST_ANTHEM = re.compile(
    r"^Each other creature you control that's an? (?P<subs>[A-Z][a-z]+"
    r"(?:, [A-Z][a-z]+)*,? or [A-Z][a-z]+) gets \+(?P<p>\d+)/\+(?P<t>\d+)"
    r"(?: and has (?P<kw>[a-z ]+))?\.$"
)


def _graveyard_pt_static(line: str) -> BoardLine | None:
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.base import StaticAbility, StaticScope
    from mtgz_ir.effects.generic import roto_board as rb
    from mtgz_ir.effects.modifiers import ContinuousModifier, PowerToughnessBonus

    m = _SELF_GETS_PER_GRAVEYARD.match(line)
    if m is None:
        return None
    desc = m.group("desc") or m.group("each")
    card_filter = parse_card_descriptor(f"a {desc} card")
    if card_filter is None:
        return None
    p, t = int(m.group("p")), int(m.group("t"))
    if m.group("each"):
        return BoardLine(statics=(StaticAbility(
            scope=StaticScope.SELF, power_bonus=p, toughness_bonus=t,
            bonus_per_condition_count=True,
            condition=rb.CardsInYourGraveyard(card_filter=card_filter),
        ),))
    n = m.group("n")
    if not (n.isdigit() or n in _NUMBER_WORDS):
        return None
    return BoardLine(statics=(StaticAbility(
        scope=StaticScope.SELF,
        condition=rb.CardsInYourGraveyard(
            card_filter=card_filter, at_least=_n(n),
        ),
        continuous_effect=ContinuousModifier(
            layer=7, payload=PowerToughnessBonus(power_bonus=p, toughness_bonus=t),
        ),
    ),))


def _other_subtype_list_anthem(line: str) -> BoardLine | None:
    from mtgz_ir.effects.base import StaticAbility, StaticScope
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier,
        GrantKeyword,
        PowerToughnessBonus,
    )
    from mtgz_ir.effects.object_predicates import HasSubtype

    m = _OTHER_SUBTYPE_LIST_ANTHEM.match(line)
    if m is None:
        return None
    subs = [s for s in re.split(r",? or |, ", m.group("subs")) if s]
    subject = HasSubtype(subs[0])
    for sub in subs[1:]:
        subject = subject | HasSubtype(sub)
    statics = [StaticAbility(
        scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(
                power_bonus=int(m.group("p")), toughness_bonus=int(m.group("t")),
            ),
            subject_filter=subject,
        ),
    )]
    if m.group("kw"):
        kws = _keyword_list(m.group("kw"))
        if kws is None:
            return None
        for kw in sorted(kws):
            statics.append(StaticAbility(
                scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
                continuous_effect=ContinuousModifier(
                    layer=6, payload=GrantKeyword(kw), subject_filter=subject,
                ),
            ))
    return BoardLine(statics=tuple(statics))


# "Escape—<cost>, Exile any number of other cards from your graveyard
# with N or more card types among them." (Nethergoyf).
_ESCAPE_CARD_TYPES = re.compile(
    r"^Escape—(?P<cost>(?:\{[^}]+\})+), [Ee]xile any number of other cards "
    r"from your graveyard with (?P<n>\w+) or more card types among them\.$"
)


def _escape_card_types(line: str) -> BoardLine | None:
    from mtgz_ir.cards.base import parse_mana_cost

    m = _ESCAPE_CARD_TYPES.match(line)
    if m is None:
        return None
    n = m.group("n")
    if not (n.isdigit() or n in _NUMBER_WORDS):
        return None
    return BoardLine(fields={
        "escape_cost": parse_mana_cost(m.group("cost")),
        "escape_cost_str": m.group("cost"),
        "escape_exile_card_types": _n(n),
    })


# "When there are no creatures on the battlefield, <effect>." -- a
# state trigger (Drop of Honey).
_STATE_TRIGGER = re.compile(
    r"^When there are no creatures on the battlefield, (?P<eff>.+)\.$"
)


def _state_trigger(line: str) -> BoardLine | None:
    from mtgz_ir import roto_board_statics as rs
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.base import TriggeredAbility

    m = _STATE_TRIGGER.match(line)
    if m is None:
        return None
    body = m.group("eff")
    effect = _parse_effect(body[0].lower() + body[1:])
    if effect is None:
        return None
    return BoardLine(fields={"board_rules": (rs.StateTrigger(
        condition=rs.NoCreaturesOnBattlefield(),
        ability=TriggeredAbility(effect=effect),
    ),)})


# "When/Whenever <condition>, <effect ending in a quoted ability>." --
# the line ends with the quote, which the trigger grammar's
# "<body>." shape doesn't take (Thief of Existence).
_TRIGGER_WITH_QUOTED_TAIL = re.compile(
    r'^(?:When|Whenever) (?P<cond>.+?), (?P<body>.+\.")$'
)


def _trigger_with_quoted_tail(line: str) -> BoardLine | None:
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.cards.parser.triggers import _parse_predicate_trigger_phrase
    from mtgz_ir.effects.base import TriggeredAbility

    m = _TRIGGER_WITH_QUOTED_TAIL.match(line)
    if m is None:
        return None
    cond = _parse_predicate_trigger_phrase(m.group("cond"))
    body = m.group("body")
    effect = _parse_effect(body[0].lower() + body[1:])
    if cond is None or effect is None:
        return None
    event, predicate = cond
    return BoardLine(triggers=(TriggeredAbility(
        event=event, predicate=predicate, effect=effect,
    ),))


from mtgz_ir.cards.parser.roto_layers import LAYER_LINE_RECOGNIZERS  # noqa: E402

_LINE_RECOGNIZERS = (
    _activated_with_and_joined_sacrifice,
    _trigger_with_quoted_tail,
    _state_trigger,
    _escape_card_types,
    _graveyard_pt_static,
    _other_subtype_list_anthem,
    _equipped_it_trigger,
    _each_land_is_type,
    _nonbasic_lands_are_mountains,
    _cant_attack_or_block,
    _fading,
    _all_creatures_have_trigger,
    _untap_rules,
    # Card types and colors (CR 613 layers 4 / 5): Mycosynth Lattice,
    # Unable to Scream (``cards.parser.roto_layers``).
    *LAYER_LINE_RECOGNIZERS,
)


# 'Enchanted <permanent> has "<cost>: Create a <Name> token."' with the
# token defined by the line's reminder text, "(It's a <mana cost>
# <subtypes> creature with "<characteristic-defining P/T sentence>")"
# (Tarmogoyf Nest -- roto integration). The only reminder-defined token
# in the pools, so the definition grammar reads just that CDA-creature
# shape; a second such token would widen ``_TOKEN_REMINDER``.
_ENCHANTED_HAS_CREATE_TOKEN = re.compile(
    r"^Enchanted (?:land|creature|artifact|permanent) has "
    r"\"(?P<cost>[^\":]+): Create an? (?P<name>[A-Z][A-Za-z']*) token\.\"$"
)
_TOKEN_REMINDER = re.compile(
    r"\(It's an? (?P<cost>(?:\{[^}]+\})+) "
    r"(?P<subs>[A-Z][a-z]+(?: [A-Z][a-z]+)*) creature with "
    r"\"(?P<cda>[^\"]+)\"\)"
)


def _reminder_defined_token(name: str, oracle_text: str):
    """The token printing a card's reminder text defines for ``name``,
    or ``None``."""
    from dataclasses import replace

    from mtgz_ir.cards.base import make_token_creature, parse_mana_cost
    from mtgz_ir.cards.parser.printing_keywords import (
        _parse_dynamic_pt_formula_line,
    )

    found = _TOKEN_REMINDER.findall(oracle_text)
    if len(found) != 1:
        return None
    cost, subs, cda = found[0]
    if not cda.startswith(f"{name}'s "):
        return None
    formula = _parse_dynamic_pt_formula_line(cda)
    if formula is None:
        return None
    token = make_token_creature(name, 0, 0, subtypes=tuple(subs.split()))
    # Its colors come from its mana cost (CR 202.2); the CDA sets P/T in
    # every zone (``game._dynamic_pt``).
    return replace(
        token, mana_cost_str=cost, mana_cost=parse_mana_cost(cost),
        dynamic_pt_formula=formula,
    )


def _enchanted_has_create_token(line: str, card: dict) -> BoardLine | None:
    from mtgz_ir.cards.base import ActivatedAbility
    from mtgz_ir.cards.parser.activated import _parse_activation_cost
    from mtgz_ir.effects.generic.tokens import CreateToken

    m = _ENCHANTED_HAS_CREATE_TOKEN.match(line)
    if m is None:
        return None
    token = _reminder_defined_token(m.group("name"), card.get("oracle_text", ""))
    cost = _parse_activation_cost(m.group("cost"))
    if token is None or cost is None:
        return None
    ability = ActivatedAbility(cost=cost, effect=CreateToken(token_printing=token))
    return BoardLine(fields={"attached_grants_activated_abilities": (ability,)})


def parse_roto_board_line(line: str, card: dict) -> BoardLine | None:
    """A whole oracle line of a P5 permanent the builder's grammar
    declined (reminder text stripped)."""
    if _disabled():
        return None
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    line = _substitute_self_name(line.strip(), card.get("name", ""))
    granted = _enchanted_has_create_token(line, card)
    if granted is not None:
        return granted
    for recognizer in _LINE_RECOGNIZERS:
        parsed = recognizer(line)
        if parsed is not None:
            return parsed
    return None


def parse_roto_board_line_into(
    line: str, card: dict, fields: dict,
) -> BoardLine | None:
    """:func:`parse_roto_board_line`, folding the line's printing fields
    into the builder's ``fields`` accumulator (so several lines may each
    add board rules); the returned line carries no fields of its own."""
    board = parse_roto_board_line(line, card)
    if board is None:
        return None
    merge_board_fields(fields, board.fields)
    return BoardLine(
        keywords=board.keywords, triggers=board.triggers,
        activated=board.activated, statics=board.statics,
    )


# "<cost>: Choose one —" followed by "• <mode>" lines -- a modal
# activated ability (Umezawa's Jitte).
_MODAL_ACTIVATION_HEADER = re.compile(r"^(?P<cost>[^:]+): Choose one —$")
# "Equipped creature gets +P/+T and has <keywords> and ward {N}." -- the
# ward rider of a granted-keyword list (Lavaspur Boots).
_EQUIPPED_WARD_RIDER = re.compile(
    r"^(?P<head>Equipped creature (?:gets [+-]\d+/[+-]\d+ and )?has .+?),? "
    r"and ward (?P<cost>(?:\{[^}]+\})+)\.$"
)


def _modal_activation(header: str, bullets: list[str], name: str):
    from mtgz_ir.cards.parser.activated import _parse_activation_cost
    from mtgz_ir.cards.parser.creature_builder import _parse_modal_mode_phrase
    from mtgz_ir.cards.parser.triggers import _substitute_self_name
    from mtgz_ir.effects.base import ActivatedAbility
    from mtgz_ir.effects.combinators import ModalChooseOne

    m = _MODAL_ACTIVATION_HEADER.match(_substitute_self_name(header, name))
    if m is None or len(bullets) < 2:
        return None
    cost = _parse_activation_cost(m.group("cost"))
    if cost is None:
        return None
    modes = []
    for bullet in bullets:
        eff = _parse_modal_mode_phrase(
            _substitute_self_name(bullet[len("• "):], name),
        )
        if eff is None:
            return None
        modes.append(eff)
    return ActivatedAbility(cost=cost, effect=ModalChooseOne(modes=tuple(modes)))


def claim_equipment_lines(card: dict):
    """The equipment builder's pre-pass: claim the multi-line / compound
    shapes its line loop can't see whole. Returns ``(remaining oracle
    text, BoardLine)`` -- modal activated abilities in
    ``BoardLine.activated``, a ward rider as the
    ``attached_grants_ward_cost`` field with the rest of its line handed
    back -- or ``None`` when a claimed block fails to parse."""
    from mtgz_ir.cards.base import parse_mana_cost
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    text = card.get("oracle_text", "")
    if _disabled():
        return text, BoardLine()
    lines = [_strip_reminder_text(raw).strip() for raw in text.split("\n")]
    kept: list[str] = []
    out = BoardLine()
    activated: list = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if _MODAL_ACTIVATION_HEADER.match(line):
            bullets = []
            j = i + 1
            while j < len(lines) and lines[j].startswith("• "):
                bullets.append(lines[j])
                j += 1
            ability = _modal_activation(line, bullets, card.get("name", ""))
            if ability is None:
                return None
            activated.append(ability)
            i = j
            continue
        m = _EQUIPPED_WARD_RIDER.match(line)
        if m is not None:
            out.fields["attached_grants_ward_cost"] = parse_mana_cost(
                m.group("cost"),
            )
            out.fields["attached_grants_ward_str"] = m.group("cost")
            kept.append(f"{m.group('head')}.")
            i += 1
            continue
        kept.append(line)
        i += 1
    out.activated = tuple(activated)
    return "\n".join(kept), out


# "Cast this spell only before the <step> step." (Berserk).
_CAST_ONLY_BEFORE_STEP = re.compile(
    r"^Cast this spell only before the (?P<step>combat damage|declare "
    r"blockers|declare attackers|end) step\.$"
)


def claim_spell_lines(text: str) -> tuple[str, dict]:
    """The spell builder's pre-pass: lines that are rules of the card
    itself become printing fields (``CastOnlyBeforeStep``); returns the
    remaining oracle text and the fields."""
    from mtgz_ir import roto_board_statics as rs

    if _disabled():
        return text, {}
    kept, fields = [], {}
    for raw in text.split("\n"):
        m = _CAST_ONLY_BEFORE_STEP.match(raw.strip())
        if m is not None:
            merge_board_fields(fields, {"board_rules": (rs.CastOnlyBeforeStep(
                step=m.group("step").replace(" ", "_"),
            ),)})
            continue
        kept.append(raw)
    return "\n".join(kept), fields


def apply_board_fields(printing, fields: dict):
    """``dataclasses.replace`` for a builder's accumulated P5 fields:
    accumulated fields extend what the printing already carries."""
    from dataclasses import replace

    if not fields:
        return printing
    updates = {}
    for name, value in fields.items():
        if name in _ACCUMULATED_FIELDS:
            updates[name] = tuple(getattr(printing, name)) + tuple(value)
        else:
            updates[name] = value
    return replace(printing, **updates)


def absorb_land_line(line: str, card: dict, fields: dict,
                     triggered: list, activated: list) -> bool:
    """The line-composed land builder's fallback for a line its grammar
    declined. Lands take triggered / activated abilities and printing
    fields; a line contributing keywords or anthem statics is declined."""
    board = parse_roto_board_line(line, card)
    if board is None or board.keywords or board.statics:
        return False
    triggered.extend(board.triggers)
    activated.extend(board.activated)
    merge_board_fields(fields, board.fields)
    return True


def absorb_enchantment_line(line: str, card: dict, acc) -> bool:
    """The cube enchantment builder's fallback for a line its grammar
    declined: fold a recognized P5 line into its ``_Accumulator``."""
    board = parse_roto_board_line(line, card)
    if board is None:
        return False
    acc.keywords.update(board.keywords)
    acc.triggers.extend(board.triggers)
    acc.activated.extend(board.activated)
    acc.statics.extend(board.statics)
    merge_board_fields(acc.printing_fields, board.fields)
    return True


__all__ = [
    "BoardLine",
    "claim_spell_lines",
    "parse_roto_dynamic_pt_line",
    "claim_equipment_lines",
    "parse_roto_trigger_phrase",
    "parse_roto_board_effect",
    "parse_roto_board_line",
    "parse_roto_cost_piece",
    "permanent_phrase",
]
