"""Effect-phrase recognizers for the cube W/G creature bucket.

Hooked into :func:`mtgz_ir.cards.parser.effects._parse_effect` as the
very last fallback (after the planeswalker extension grammar and the
card-flow grammar), so these shapes only claim phrases nothing else
understood and can never change an existing parse. Phrases arrive
normalized: no trailing period, lower-cased first letter; later
sentences keep their capitalization. Target noun phrases compose
through :func:`mtgz_ir.cards.parser.effects_ext.parse_target_noun_phrase`.

Shapes:

* ``[up to one ]target player puts all the cards from their graveyard
  on the bottom of their library in a random order`` ->
  :class:`TargetPlayerPutsGraveyardOnBottomRandom` (Endurance)
* ``the exiled card's owner creates an X/X <color> <Subtype> creature
  token, where X is the mana value of the exiled card`` ->
  :class:`ExiledCardOwnerCreatesManaValueToken` (Skyclave Apparition;
  the builder links the card's exiling ability, see
  ``creature_wg_lines.link_exile_abilities``)
* ``create a number of <P>/<T> <color> <Subtypes> creature tokens equal
  to this creature's power`` -> :class:`CreateTokensEqualToSourcePower`
  (Jacked Rabbit)
* ``you and target opponent each draw N card(s)`` ->
  :class:`YouAndTargetOpponentEachDraw` (Loran of the Third Path)
* ``return target <artifact|creature|land> card with mana value N or less
  from your graveyard to your hand`` -> ``ReturnFromGraveyard`` (Auriok
  Salvagers)
* ``put <a filter> card from your hand onto the battlefield[ tapped]`` ->
  :class:`PutCardFromHandOntoBattlefield` (Stoneforge Mystic; "you may"
  composes through the extension grammar's ``MayDo``)
* ``create a token that's a copy of it[, except it's an <type> in
  addition to its other types]`` -> :class:`CreateTokenCopyOfSource`
  (Vaultborn Tyrant)
* ``<base>. If <condition>, <alt> instead`` -> ``IfConditionElse``
  (White Plume / Undermountain Adventurer's dungeon riders)
* ``add <N> {C}`` -> ``AddManaToPool``; ``untap a <noun>`` /
  ``untap all <nouns>`` -> :class:`UntapChosenPermanents` /
  :class:`UntapAllMatching`
* ``shuffle it into its owner's library`` ->
  :class:`ShuffleSourceIntoOwnersLibrary` (Worldspine Wurm)
* ``deals that much damage to target <type> that player controls`` ->
  :class:`DealCombatDamageAmountToTarget` (Questing Beast; "that player"
  is the damaged opponent -- in two-player games, the opponent)
* ``[another ]target <noun> gains <keywords> until end of turn. Put
  <counters> on that creature and <counters> on this creature`` ->
  :class:`GrantKeywordsUEOTAndPutCounters` (Arwen)
* ``double the number of +1/+1 counters on each <noun>`` ->
  :class:`DoublePlusOneCountersOnEach` (Bristly Bill)
* ``put a +1/+1 counter on target <noun>. If this is the second time this
  ability has resolved this turn, double ... instead`` ->
  :class:`PutCounterDoublingOnSecondResolution` (Scythecat Cub)
* ``you may remove a counter from this creature|pay {E}... . When you
  do, <effect>[ that player controls]`` -> :class:`ReflexiveWhenYouDo`
  (Leatherhead, Guide of Souls)
* ``put <counters> on target <noun>. It becomes a <Subtype> in addition
  to its other types`` -> :class:`PutCountersOnTargetAndAddSubtype`
* ``target <noun> gains <keywords> until end of turn. It explores`` ->
  :class:`GrantKeywordsUEOTThenExplores` (Seasoned Dungeoneer)
* ``for each opponent, create <a token> that's tapped and attacking that
  player or a planeswalker they control`` ->
  :class:`CreateAttackingTokenForEachOpponent` (Adeline)
* ``create a P/T <color> <Subtypes> creature token named <Name> with
  "<trigger>"`` -> ``CreateToken`` carrying the trigger (Mole Man)
* ``add one mana of any color. Spend this mana only to cast a
  legendary spell, and that spell can't be countered`` -> restricted
  ``AddManaToPool`` whose coins make the spell uncounterable (Delighted
  Halfling)
* Ursine Monstrosity's mill / random-opponent attack requirement /
  per-card-type pump body -> :class:`MillChooseOpponentAttackPump`
* ``put a +1/+1 counter on target <noun>. Whenever that creature attacks
  the monarch this turn, it gains <keywords> until end of turn`` ->
  :class:`CounterThenWhenItAttacksMonarch` (Okoye)
* ``<effect>. Then if you have the city's blessing, for each token you
  control that entered this turn, create a token that's a copy of it``
  -> ``Sequence`` + :class:`CopyEachTokenYouControlEnteredThisTurn`
  (Ocelot Pride)
* ``look at target opponent's hand. You may exile a nonland card from
  it. For as long as that card remains exiled, its owner may play it. A
  spell cast this way costs {N} more to cast`` ->
  :class:`LookAtHandExileNonlandOwnerMayPlayTaxed` (Elite Spellbinder)
* ``mill N cards. You may put a land card from among them into your
  hand`` -> :class:`MillThenMayPutLandIntoHand` (Six)
* ``exile target card from a graveyard. If it was a permanent card, put a
  +1/+1 counter on this permanent`` ->
  :class:`ExileGraveyardCardCounterIfPermanent` (Lion Sash)
* ``you may pay <cost> if this permanent is attached to a creature you
  control. If you do, create a token that's a copy of that creature. If
  you didn't create a token this way, create <token>`` ->
  :class:`PayToCopyEnchantedCreatureElseToken` (Springheart Nantuko)
* ``earthbend N`` -> :class:`Earthbend` (Badgermole Cub)
* ``exile [up to one ]target <noun> until an opponent becomes the
  monarch`` -> :class:`ExileUntilOpponentBecomesMonarch` (Palace Jailer)
"""

from __future__ import annotations

import contextlib
import re

from mtgz_ir.effects.base import Effect


_COLOR_LETTERS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}

_GRAVEYARD_ON_BOTTOM_RE = re.compile(
    r"^(?P<upto>up to one )?target player puts all the cards from their "
    r"graveyard on the bottom of their library in a random order$"
)
_EXILED_CARD_OWNER_MV_TOKEN_RE = re.compile(
    r"^the exiled card's owner creates an X/X "
    r"(?P<color>white|blue|black|red|green) (?P<sub>[A-Z][a-z]+) creature "
    r"token, where X is the mana value of the exiled card$"
)
_TOKENS_EQUAL_TO_POWER_RE = re.compile(
    r"^create a number of (?P<p>\d+)/(?P<t>\d+) "
    r"(?P<color>white|blue|black|red|green|colorless) "
    r"(?P<sub>[A-Z][a-z]+(?: [A-Z][a-z]+)*) creature tokens equal to this "
    r"creature's power$"
)
_YOU_AND_TARGET_OPPONENT_DRAW_RE = re.compile(
    r"^you and target opponent each draw (?P<n>a|two|three|\d+) cards?$"
)
_RETURN_TYPED_CARD_WITH_MV_TO_HAND_RE = re.compile(
    r"^return target (?P<type>artifact|creature|land) card with mana value "
    r"(?P<mv>\d+) or less from your graveyard to your hand$"
)
_PUT_CARD_FROM_HAND_ONTO_BATTLEFIELD_RE = re.compile(
    r"^put (?P<desc>an? .+? card) from your hand onto the battlefield"
    r"(?P<tapped> tapped)?$"
)
_TOKEN_COPY_OF_IT_RE = re.compile(
    r"^create a token that's a copy of it(?:, except it's an? "
    r"(?P<type>artifact|enchantment|creature) in addition to its other "
    r"types)?$"
)
_INSTEAD_IF_RIDER_RE = re.compile(
    r"^(?P<base>.+?)\. If (?P<cond>[^,]+), (?P<alt>.+) instead$"
)
_ADD_N_OF_ONE_COLOR_RE = re.compile(
    r"^add (?P<n>two|three|four|five|six|seven|eight|nine|ten|\d+) "
    r"\{(?P<c>[WUBRGC])\}$"
)
_UNTAP_A_RE = re.compile(r"^untap (?:a|an|one) (?P<noun>.+)$")
_UNTAP_ALL_RE = re.compile(r"^untap all (?P<noun>.+)$")
_SHUFFLE_IT_INTO_LIBRARY_RE = re.compile(
    r"^shuffle it into its owner's library$"
)
_THAT_MUCH_DAMAGE_TO_TARGET_RE = re.compile(
    r"^deals that much damage to target (?P<noun>[a-z]+) that player controls$"
)
_GAINS_UEOT_THEN_COUNTERS_RE = re.compile(
    r"^(?P<other>another )?target (?P<noun>.+?) gains (?P<kw>.+?) until end "
    r"of turn\. Put (?P<tc>.+?) on that creature and (?P<sc>.+?) on this "
    r"creature$"
)
_DOUBLE_COUNTERS_ON_EACH_RE = re.compile(
    r"^double the number of \+1/\+1 counters on each (?P<noun>.+)$"
)
_COUNTER_OR_DOUBLE_ON_SECOND_RESOLUTION_RE = re.compile(
    r"^put a \+1/\+1 counter on target (?P<noun>.+?)\. If this is the "
    r"second time this ability has resolved this turn, double the number "
    r"of \+1/\+1 counters on that creature instead$"
)
_COUNTER_PIECE_RE = re.compile(
    r"^(?P<n>a|an|one|two|three|four|five|six|seven|eight|nine|ten|\d+) "
    r"(?P<kind>\+1/\+1|[a-z]+(?: [a-z]+)?) counters?$"
)
_REFLEXIVE_WHEN_YOU_DO_RE = re.compile(
    r"^you may (?P<cost>remove a counter from (?:this creature|it|her|him)|pay "
    r"(?P<energy>(?:\{E\})+))\. When you do, (?P<eff>.+)$"
)
_COUNTERS_ON_TARGET_BECOMES_SUBTYPE_RE = re.compile(
    r"^put (?P<counters>.+?) on target (?P<noun>.+?)\. It becomes an? "
    r"(?P<sub>[A-Z][a-z]+) in addition to its other types$"
)
_GAINS_UEOT_THEN_EXPLORES_RE = re.compile(
    r"^target (?P<noun>.+?) gains (?P<kw>.+?) until end of turn\. It explores$"
)
_FOR_EACH_OPPONENT_ATTACKING_TOKEN_RE = re.compile(
    r"^for each opponent, create (?P<tok>an? .+? creature token) that's "
    r"tapped and attacking that player or a planeswalker they control$"
)
_NAMED_TOKEN_WITH_TRIGGER_RE = re.compile(
    r"^create an? (?P<p>\d+)/(?P<t>\d+) (?P<color>white|blue|black|red|green|"
    r"colorless) (?P<sub>[A-Z][a-z]+(?: [A-Z][a-z]+)*) creature token named "
    r"(?P<name>[A-Z][A-Za-z' ]*?) with \"(?P<ability>.+)\"$"
)
_ANY_COLOR_UNCOUNTERABLE_RESTRICTED_RE = re.compile(
    r"^add one mana of any color\. Spend this mana only to cast an? "
    r"(?P<kind>legendary|artifact) spell, and that spell can't be countered$"
)
_MILL_CHOOSE_OPPONENT_ATTACK_PUMP_RE = re.compile(
    r"^mill a card and choose an opponent at random\. This creature attacks "
    r"that player this combat if able\. Until end of turn, this creature "
    r"gains (?P<kw>.+?) and gets \+1/\+1 for each card type among cards in "
    r"your graveyard$"
)
_COUNTER_THEN_ATTACKS_MONARCH_RE = re.compile(
    r"^put a \+1/\+1 counter on target (?P<noun>.+?)\. Whenever that "
    r"creature attacks the monarch this turn, it gains (?P<kw>.+) until end "
    r"of turn$"
)
_THEN_IF_BLESSING_COPY_TOKENS_RE = re.compile(
    r"^(?P<first>.+?)\. Then if you have the city's blessing, for each token "
    r"you control that entered this turn, create a token that's a copy of it$"
)
_LOOK_HAND_EXILE_MAY_PLAY_TAXED_RE = re.compile(
    r"^look at target opponent's hand\. You may exile a nonland card from "
    r"it\. For as long as that card remains exiled, its owner may play it\. "
    r"A spell cast this way costs \{(?P<n>\d+)\} more to cast$"
)
_MILL_MAY_PUT_LAND_RE = re.compile(
    r"^mill (?P<n>two|three|four|five|\d+) cards\. You may put a land card "
    r"from among them into your hand$"
)
_EXILE_GRAVEYARD_CARD_COUNTER_IF_PERMANENT_RE = re.compile(
    r"^exile target card from a graveyard\. If it was a permanent card, put "
    r"a \+1/\+1 counter on this (?:permanent|creature)$"
)
_PAY_TO_COPY_ENCHANTED_ELSE_TOKEN_RE = re.compile(
    r"^you may pay (?P<cost>(?:\{[^}]+\})+) if this permanent is attached to "
    r"a creature you control\. If you do, create a token that's a copy of "
    r"that creature\. If you didn't create a token this way, create "
    r"(?P<tok>an? .+? creature token)$"
)
_EARTHBEND_RE = re.compile(r"^earthbend (?P<n>\d+)$")
_EXILE_UNTIL_OPPONENT_MONARCH_RE = re.compile(
    r"^exile (?P<upto>up to one )?target (?P<noun>.+?) until an opponent "
    r"becomes the monarch$"
)


def _target_spec(noun: str, up_to_one: bool, other: bool = False):
    """A ``TargetSpec`` for a composed noun phrase (``None`` when the
    noun phrase isn't in the closed vocabulary)."""
    from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase
    from mtgz_ir.effects.object_predicates import IsSource
    from mtgz_ir.effects.targets import TargetSpec

    pred = parse_target_noun_phrase(noun)
    if pred is None:
        return None
    if other:
        pred = pred & ~IsSource()
    return TargetSpec(filter=pred, count_min=0 if up_to_one else 1)


# Edge of Eternities-era printings the W/G shapes would newly claim.
# Their parse is pinned to the pre-bucket grammar so EoE behavior (and
# the EoE parity campaign's claimed set) doesn't move.
EOE_PINNED_NAMES: frozenset[str] = frozenset({
    # "... you may put a permanent card from your hand onto the
    # battlefield." (Survival) -- the put-from-hand shape.
    "Kona, Rescue Beastie",
    # "Whenever another creature you control enters, it gets +2/+0 and
    # gains haste until end of turn." -- the generic enters trigger would
    # claim it, but "it" (the entering creature) would bind to the
    # source through the creature-effect normalizer.
    "Enduring Courage",
})

_DISABLED = [False]


@contextlib.contextmanager
def wg_grammar_pinned_off(name: str):
    """Disable every W/G recognizer while building ``name`` when it is
    one of :data:`EOE_PINNED_NAMES` (no-op otherwise)."""
    prior = _DISABLED[0]
    _DISABLED[0] = prior or name in EOE_PINNED_NAMES
    try:
        yield
    finally:
        _DISABLED[0] = prior


def wg_grammar_disabled() -> bool:
    return _DISABLED[0]


_WORD_NUMBERS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def _controlled(noun: str):
    """Object filter for a (possibly plural) permanent noun phrase with
    its controller clause folded in as an object predicate (``creatures
    you control``) -- for untargeted "a/all <noun>" effects, which
    resolve with ``state.resolving_controller_id`` pinned."""
    from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase

    words = noun.split(" ")
    for i, w in enumerate(words):
        if w.endswith("s") and w[:-1] in (
            "creature", "artifact", "land", "enchantment", "permanent",
        ):
            words[i] = w[:-1]
            break
    return parse_target_noun_phrase(" ".join(words))


def parse_counter_list(text: str):
    """``a +1/+1 counter and a lifelink counter`` / ``two +1/+1 counters
    and a flying counter`` -> ``(("+1/+1", 1), ("lifelink", 1))``.
    Kinds are ``+1/+1`` or a keyword-counter / named-counter word;
    ``None`` when any piece doesn't parse."""
    from mtgz_ir.game import _DEDICATED_COUNTER_FIELDS, KEYWORD_COUNTER_KINDS

    out = []
    for piece in text.split(" and "):
        m = _COUNTER_PIECE_RE.match(piece.strip())
        if m is None:
            return None
        kind = m.group("kind")
        if kind not in KEYWORD_COUNTER_KINDS and kind not in _DEDICATED_COUNTER_FIELDS:
            return None
        n = m.group("n")
        count = 1 if n in ("a", "an") else (int(n) if n.isdigit() else _WORD_NUMBERS[n])
        out.append((kind, count))
    return tuple(out)


def _named_token_with_trigger(m):
    """``create a P/T <color> <Subtypes> creature token named <Name> with
    "<triggered ability>"`` (Mole Man's Moloid) -> ``CreateToken`` whose
    printing carries the quoted trigger ("this token" reads as the
    token itself)."""
    from mtgz_ir.cards.base import make_token_creature
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    from mtgz_ir.effects.generic.tokens import CreateToken

    body = m.group("ability").replace("this token", "this creature")
    if not body.endswith("."):
        body += "."
    parsed = _parse_triggered_ability_line(body)
    if parsed is None:
        return None
    arms = parsed if isinstance(parsed, tuple) else (parsed,)
    color = _COLOR_LETTERS.get(m.group("color"))
    printing = make_token_creature(
        name=m.group("name"), power=int(m.group("p")),
        toughness=int(m.group("t")),
        subtypes=tuple(m.group("sub").split(" ")),
        colors=(color,) if color else (),
    )
    from dataclasses import replace

    return CreateToken(token_printing=replace(printing, triggered_abilities=arms))


_THAT_PLAYER_CONTROLS = " that player controls"


def _reflexive(m):
    """``you may <cost>. When you do, <effect>`` -> ``ReflexiveWhenYouDo``;
    a trailing "that player controls" on the effect's target binds it to
    the player the triggering event fixed (the damaged player)."""
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import cube_creatures_wg as wg

    if m.group("energy"):
        cost = wg.PayEnergy(amount=m.group("energy").count("{E}"))
    else:
        cost = wg.RemoveACounterFromSelf()
    body = m.group("eff")
    event_player = body.endswith(_THAT_PLAYER_CONTROLS)
    if event_player:
        body = body[: -len(_THAT_PLAYER_CONTROLS)]
    effect = _parse_effect(body)
    if effect is None:
        return None
    return wg.ReflexiveWhenYouDo(
        cost=cost, effect=effect, event_player_controls=event_player,
    )


def _instead_if_rider(m):
    """``<base>. If <condition>, <alt> instead`` -> ``IfConditionElse``
    (condition from the trigger intervening-if phrasebook, evaluated for
    the controller at resolution)."""
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.cards.parser.triggers import _parse_intervening_if_phrase
    from mtgz_ir.effects.combinators import IfConditionElse
    from mtgz_ir.effects.predicates import StateBound

    gate = _parse_intervening_if_phrase(m.group("cond"))
    if not isinstance(gate, StateBound):
        return None
    base = _parse_effect(m.group("base"))
    alt = _parse_effect(m.group("alt"))
    if base is None or alt is None:
        return None
    return IfConditionElse(condition=gate.inner, then_effect=alt, else_effect=base)


def parse_wg_effect(phrase: str) -> Effect | None:
    """Last-resort W/G bucket recognizers (see the module docstring)."""
    if _DISABLED[0]:
        return None
    from mtgz_ir.effects.generic import cube_creatures_wg as wg

    if phrase == "copy this spell":
        return wg.CopyThisSpell()
    m = _GRAVEYARD_ON_BOTTOM_RE.match(phrase)
    if m is not None:
        return wg.TargetPlayerPutsGraveyardOnBottomRandom(
            up_to_one=bool(m.group("upto")),
        )
    m = _EXILED_CARD_OWNER_MV_TOKEN_RE.match(phrase)
    if m is not None:
        return wg.ExiledCardOwnerCreatesManaValueToken(
            color=_COLOR_LETTERS[m.group("color")], subtype=m.group("sub"),
        )
    m = _TOKENS_EQUAL_TO_POWER_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import make_token_creature

        color = _COLOR_LETTERS.get(m.group("color"))
        subtypes = tuple(m.group("sub").split(" "))
        return wg.CreateTokensEqualToSourcePower(
            token_printing=make_token_creature(
                name=m.group("sub"), power=int(m.group("p")),
                toughness=int(m.group("t")), subtypes=subtypes,
                colors=(color,) if color else (),
            ),
        )
    m = _YOU_AND_TARGET_OPPONENT_DRAW_RE.match(phrase)
    if m is not None:
        n = m.group("n")
        return wg.YouAndTargetOpponentEachDraw(
            amount={"a": 1, "two": 2, "three": 3}.get(n) or int(n),
        )
    m = _RETURN_TYPED_CARD_WITH_MV_TO_HAND_RE.match(phrase)
    if m is not None and int(m.group("mv")) > 0:
        from mtgz_ir.effects.generic.graveyard import ReturnFromGraveyard

        kind = m.group("type")
        return ReturnFromGraveyard(**{
            kind: True, "max_mv": int(m.group("mv")),
            "destination": "hand", "target_kind": f"graveyard_{kind}",
        })
    m = _PUT_CARD_FROM_HAND_ONTO_BATTLEFIELD_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        f = parse_card_descriptor(m.group("desc"))
        if f is not None:
            return wg.PutCardFromHandOntoBattlefield(
                card_filter=f, optional=False, tapped=bool(m.group("tapped")),
            )
    m = _REFLEXIVE_WHEN_YOU_DO_RE.match(phrase)
    if m is not None:
        reflexive = _reflexive(m)
        if reflexive is not None:
            return reflexive
    m = _COUNTERS_ON_TARGET_BECOMES_SUBTYPE_RE.match(phrase)
    if m is not None:
        counters = parse_counter_list(m.group("counters"))
        spec = _target_spec(m.group("noun"), False)
        if counters is not None and spec is not None:
            return wg.PutCountersOnTargetAndAddSubtype(
                targets=(spec,), counters=counters, subtype=m.group("sub"),
            )
    m = _EARTHBEND_RE.match(phrase)
    if m is not None:
        return wg.Earthbend(n=int(m.group("n")))
    m = _PAY_TO_COPY_ENCHANTED_ELSE_TOKEN_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import parse_mana_cost
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.generic.tokens import CreateToken

        base = _parse_effect("create " + m.group("tok"))
        if isinstance(base, CreateToken) and base.count == 1:
            return wg.PayToCopyEnchantedCreatureElseToken(
                cost=parse_mana_cost(m.group("cost")),
                cost_str=m.group("cost"),
                token_printing=base.token_printing,
            )
    if _EXILE_GRAVEYARD_CARD_COUNTER_IF_PERMANENT_RE.match(phrase):
        return wg.ExileGraveyardCardCounterIfPermanent()
    m = _MILL_MAY_PUT_LAND_RE.match(phrase)
    if m is not None:
        n = m.group("n")
        return wg.MillThenMayPutLandIntoHand(
            n=int(n) if n.isdigit() else _WORD_NUMBERS[n],
        )
    m = _LOOK_HAND_EXILE_MAY_PLAY_TAXED_RE.match(phrase)
    if m is not None and int(m.group("n")) >= 1:
        return wg.LookAtHandExileNonlandOwnerMayPlayTaxed(
            surcharge=int(m.group("n")),
        )
    m = _THEN_IF_BLESSING_COPY_TOKENS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.combinators import IfConditionElse, Sequence
        from mtgz_ir.effects.state_predicates import HasCitysBlessing

        first = _parse_effect(m.group("first"))
        if first is not None:
            return Sequence(effects=(
                first,
                IfConditionElse(
                    condition=HasCitysBlessing(),
                    then_effect=wg.CopyEachTokenYouControlEnteredThisTurn(),
                ),
            ))
    m = _MILL_CHOOSE_OPPONENT_ATTACK_PUMP_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _keywords

        kws = _keywords(m.group("kw"))
        if kws:
            return wg.MillChooseOpponentAttackPump(mill=1, keywords=kws)
    m = _COUNTER_THEN_ATTACKS_MONARCH_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _keywords

        spec = _target_spec(m.group("noun"), False)
        kws = _keywords(m.group("kw"))
        if spec is not None and kws:
            return wg.CounterThenWhenItAttacksMonarch(targets=(spec,), keywords=kws)
    m = _ANY_COLOR_UNCOUNTERABLE_RESTRICTED_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mana import AddManaToPool

        return AddManaToPool(
            produces=(("any", 1),),
            spend_restriction=f"{m.group('kind')}_spell",
            makes_spell_uncounterable=True,
        )
    m = _NAMED_TOKEN_WITH_TRIGGER_RE.match(phrase)
    if m is not None:
        token = _named_token_with_trigger(m)
        if token is not None:
            return token
    m = _FOR_EACH_OPPONENT_ATTACKING_TOKEN_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.generic.tokens import CreateToken

        base = _parse_effect("create " + m.group("tok"))
        if isinstance(base, CreateToken) and base.count == 1:
            return wg.CreateAttackingTokenForEachOpponent(
                token_printing=base.token_printing,
            )
    m = _GAINS_UEOT_THEN_EXPLORES_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _keywords

        spec = _target_spec(m.group("noun"), False)
        kws = _keywords(m.group("kw"))
        if spec is not None and kws:
            return wg.GrantKeywordsUEOTThenExplores(targets=(spec,), keywords=kws)
    m = _GAINS_UEOT_THEN_COUNTERS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _keywords

        spec = _target_spec(m.group("noun"), False, bool(m.group("other")))
        kws = _keywords(m.group("kw"))
        tc = parse_counter_list(m.group("tc"))
        sc = parse_counter_list(m.group("sc"))
        if None not in (spec, kws, tc, sc):
            return wg.GrantKeywordsUEOTAndPutCounters(
                targets=(spec,), keywords=kws,
                target_counters=tc, self_counters=sc,
            )
    m = _DOUBLE_COUNTERS_ON_EACH_RE.match(phrase)
    if m is not None:
        f = _controlled(m.group("noun"))
        if f is not None:
            return wg.DoublePlusOneCountersOnEach(permanent_filter=f)
    m = _COUNTER_OR_DOUBLE_ON_SECOND_RESOLUTION_RE.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), False)
        if spec is not None:
            return wg.PutCounterDoublingOnSecondResolution(targets=(spec,))
    m = _INSTEAD_IF_RIDER_RE.match(phrase)
    if m is not None:
        rider = _instead_if_rider(m)
        if rider is not None:
            return rider
    m = _ADD_N_OF_ONE_COLOR_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mana import AddManaToPool

        n = m.group("n")
        count = int(n) if n.isdigit() else _WORD_NUMBERS[n]
        return AddManaToPool(produces=((m.group("c"), count),))
    m = _UNTAP_A_RE.match(phrase)
    if m is not None:
        f = _controlled(m.group("noun"))
        if f is not None:
            return wg.UntapChosenPermanents(n=1, permanent_filter=f)
    m = _UNTAP_ALL_RE.match(phrase)
    if m is not None:
        f = _controlled(m.group("noun"))
        if f is not None:
            return wg.UntapAllMatching(permanent_filter=f)
    if _SHUFFLE_IT_INTO_LIBRARY_RE.match(phrase):
        return wg.ShuffleSourceIntoOwnersLibrary()
    m = _THAT_MUCH_DAMAGE_TO_TARGET_RE.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun") + " an opponent controls", False)
        if spec is not None:
            return wg.DealCombatDamageAmountToTarget(targets=(spec,))
    m = _TOKEN_COPY_OF_IT_RE.match(phrase)
    if m is not None:
        t = m.group("type")
        return wg.CreateTokenCopyOfSource(
            extra_card_types=(t.capitalize(),) if t else (),
        )
    m = _EXILE_UNTIL_OPPONENT_MONARCH_RE.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), bool(m.group("upto")))
        if spec is not None:
            return wg.ExileUntilOpponentBecomesMonarch(targets=(spec,))
    return None


# ---------------------------------------------------------------------
# Trigger conditions (fallback of ``effects_ext.parse_ext_trigger_phrase``)
# ---------------------------------------------------------------------

_PERMANENT_PUT_INTO_GRAVEYARD_RE = re.compile(
    r"^an? (?P<noun>[a-z]+) you control is put into a graveyard from the "
    r"battlefield$"
)
_YOU_SACRIFICE_RE = re.compile(r"^you sacrifice an? (?P<noun>[a-z]+)$")
_NOUN_ENTERS_RE = re.compile(r"^(?:(?P<another>another)|an?) (?P<noun>.+) enters$")
_DEALS_COMBAT_DAMAGE_TO_OPPONENT = (
    "this creature deals combat damage to an opponent"
)
_SELF_OR_ANOTHER_ENTERS_RE = re.compile(
    r"^this creature or another (?P<noun>.+) enters$"
)


def trigger_subject(noun: str):
    """``(ObjectPredicate, TriggerPredicate | None)`` for a trigger's
    subject noun phrase (``creature you control with power 4 or
    greater``): the object filter plus the ``Controlled`` binder for a
    controller clause (``None`` when the phrase has none). ``None`` when
    the noun phrase isn't in the closed vocabulary. Plural heads
    (``creatures``) are accepted."""
    from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase
    from mtgz_ir.effects.predicates import OPPONENT, YOU, Controlled

    head, sep, qualifier = noun.partition(" with ")
    controlled = None
    for suffix, who in ((" you control", YOU), (" an opponent controls", OPPONENT)):
        if head.endswith(suffix):
            head = head[: -len(suffix)]
            controlled = Controlled(who)
            break
    words = head.split(" ")
    if words[-1].endswith("s") and words[-1][:-1] in (
        "creature", "artifact", "land", "enchantment", "permanent",
    ):
        words[-1] = words[-1][:-1]
    pred = parse_target_noun_phrase(" ".join(words) + sep + qualifier)
    if pred is None:
        return None
    return pred, controlled


_CREATURE_OR_CREATURE_SPELL_BECOMES_TARGET = (
    "a creature you control or a creature spell you control becomes the "
    "target of a spell or ability an opponent controls"
)


def parse_wg_intervening_if(phrase: str):
    """Intervening-if gates of "When you cast this spell" triggers:

    * ``it was kicked`` -> :class:`ThisSpellWasKicked` (Sowing Mycospawn)
    * ``you've cast another spell this turn`` ->
      :class:`CastAnotherSpellThisTurn` (Sage of the Skies)
    """
    if _DISABLED[0]:
        return None
    from mtgz_ir.effects.generic import cube_creatures_wg as wg

    if phrase == "it was kicked":
        return wg.ThisSpellWasKicked()
    if phrase == "you've cast another spell this turn":
        return wg.CastAnotherSpellThisTurn()
    return None


def parse_wg_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for W/G bucket trigger conditions:

    * ``a[n] <type> you control is put into a graveyard from the
      battlefield`` -> ``DIES`` (the engine's battlefield-to-graveyard
      event, dispatched for creatures, artifacts and lands) filtered to
      that type under your control (Titania, Protector of Argoth).
    * ``this creature or another <noun> enters`` -> ``ENTERS`` for the
      source or another matching permanent (Vaultborn Tyrant).
    * ``you sacrifice a[n] <type>`` -> ``IS_SACRIFICED`` (dispatched
      for creatures, artifacts and lands; Baloth Prime).
    * ``a[n]|another <noun> enters`` -> ``ENTERS`` filtered by the
      composed noun phrase (Guide of Souls).
    * ``this creature deals combat damage to an opponent`` (Questing
      Beast).
    * ``a creature you control or a creature spell you control becomes
      the target of a spell or ability an opponent controls`` ->
      ``BECOMES_TARGET`` (a targeted spell is the event's
      ``target_spell`` subject; Surrak, Elusive Hunter).
    """
    if _DISABLED[0]:
        return None
    from mtgz_ir.cards.parser.effects_ext import parse_target_noun_phrase
    from mtgz_ir.effects.predicates import YOU, Controlled, Event, Subject

    m = _PERMANENT_PUT_INTO_GRAVEYARD_RE.match(phrase)
    if m is not None:
        pred = parse_target_noun_phrase(m.group("noun"))
        if pred is not None:
            return (Event.DIES, Subject(None, pred) & Controlled(YOU))
    if phrase == _DEALS_COMBAT_DAMAGE_TO_OPPONENT:
        from mtgz_ir.effects.object_predicates import IsPlayer
        from mtgz_ir.effects.predicates import OPPONENT, IsSelf

        return (
            Event.DEALS_COMBAT_DAMAGE,
            IsSelf(subject="source") & Subject("target", IsPlayer())
            & Controlled(OPPONENT, subject="target"),
        )
    if phrase == _CREATURE_OR_CREATURE_SPELL_BECOMES_TARGET:
        from mtgz_ir.effects.object_predicates import IsCreature
        from mtgz_ir.effects.predicates import OPPONENT

        return (
            Event.BECOMES_TARGET,
            (
                (Subject("target", IsCreature()) & Controlled(YOU, subject="target"))
                | (
                    Subject("target_spell", IsCreature())
                    & Controlled(YOU, subject="target_spell")
                )
            )
            & Controlled(OPPONENT, subject="controller"),
        )
    m = _YOU_SACRIFICE_RE.match(phrase)
    if m is not None:
        pred = parse_target_noun_phrase(m.group("noun"))
        if pred is not None:
            return (Event.IS_SACRIFICED, Subject(None, pred) & Controlled(YOU))
    m = _NOUN_ENTERS_RE.match(phrase)
    if m is not None and not m.group("noun").startswith("or "):
        subject = trigger_subject(m.group("noun"))
        if subject is not None:
            from mtgz_ir.effects.predicates import IsSelf

            pred, controlled = subject
            predicate = Subject(None, pred)
            if controlled is not None:
                predicate = predicate & controlled
            if m.group("another"):
                predicate = predicate & ~IsSelf()
            return (Event.ENTERS, predicate)
    m = _SELF_OR_ANOTHER_ENTERS_RE.match(phrase)
    if m is not None:
        subject = trigger_subject(m.group("noun"))
        if subject is not None:
            from mtgz_ir.effects.predicates import IsSelf

            pred, controlled = subject
            other = Subject(None, pred) & ~IsSelf()
            if controlled is not None:
                other = other & controlled
            return (Event.ENTERS, IsSelf() | other)
    return None


__all__ = ["parse_wg_effect", "parse_wg_intervening_if", "parse_wg_trigger_phrase"]
