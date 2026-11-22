"""Parser recognizers for the Vintage Roto P3 bucket: mana, costs and
cast permissions (``docs/ROTO_COVERAGE_PLAN.md`` section 4).

Every entry point is a last-resort fallback hooked into a shared
dispatcher with a single import-and-call, so no phrase an older
recognizer claims changes meaning. Like the cube buckets, the grammar is
switched off for printings that exist only in the EoE-era dumps (the
shared ``creature_rmc_grammar`` switch), so no EoE printing's parse can
change. Engine effects live in ``effects/generic/roto_mana.py``.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from mtgz_ir.effects.base import Effect


def _disabled() -> bool:
    from mtgz_ir.cards.parser.creature_rmc_grammar import _DISABLED

    return _DISABLED[0]


# ---------------------------------------------------------------------------
# "Activate only if <condition>." riders
# ---------------------------------------------------------------------------

# "you control a[n] <card type>" (Spire of Industry: "Activate only if you
# control an artifact.").
_YOU_CONTROL_A_TYPE_RE = re.compile(
    r"^you control an? (?P<type>artifact|creature|enchantment|land"
    r"|planeswalker)$"
)


def parse_activate_only_if_condition(phrase: str):
    """The ``Condition`` for an ``Activate only if <phrase>.`` rider the
    base activated grammar doesn't know, or ``None``."""
    if _disabled():
        return None
    m = _YOU_CONTROL_A_TYPE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.conditions import ControlPermanent
        from mtgz_ir.effects.object_predicates import (
            IsArtifact,
            IsCreature,
            IsEnchantment,
            IsLand,
            IsPlaneswalker,
        )

        pred = {
            "artifact": IsArtifact,
            "creature": IsCreature,
            "enchantment": IsEnchantment,
            "land": IsLand,
            "planeswalker": IsPlaneswalker,
        }[m.group("type")]()
        return ControlPermanent(pred)
    return None


# ---------------------------------------------------------------------------
# Effect phrases
# ---------------------------------------------------------------------------

# "add {C} for each card named <name> in each graveyard" (Rite of Flame).
_ADD_PER_CARD_NAMED_IN_GRAVEYARDS_RE = re.compile(
    r"^add \{(?P<c>[WUBRGC])\} for each card named (?P<name>.+) in each "
    r"graveyard$"
)
# "destroy all <permanents> that entered this turn" (Force of Despair).
_DESTROY_ALL_ENTERED_THIS_TURN_RE = re.compile(
    r"^destroy all (?P<np>.+?) that entered this turn$"
)
# "add {C} for each card in target opponent's hand" (Jeska's Will).
_ADD_PER_CARD_IN_TARGET_OPPONENTS_HAND_RE = re.compile(
    r"^add \{(?P<c>[WUBRGC])\} for each card in target opponent's hand$"
)
# "add {C}{C}. This mana can't be spent to cast <kind> spells" -- a
# negative spend restriction (The Mightstone and Weakstone: nonartifact).
_ADD_MANA_CANT_BE_SPENT_ON_RE = re.compile(
    r"^add (?P<mana>(?:\{[WUBRGC]\})+)\. This mana can't be spent to cast "
    r"(?P<kind>[a-z]+) spells$"
)
# "you may cast target <card descriptor>[ with mana value less than or
# equal to this creature's power] from your graveyard without paying its
# mana cost. If that spell would be put into your graveyard, exile it
# instead" (Dreadhorde Arcanist).
_MAY_CAST_TARGET_GRAVEYARD_FREE_RE = re.compile(
    r"^you may cast target (?P<desc>.+? card)(?P<power> with mana value less "
    r"than or equal to this creature's power)? from your graveyard without "
    r"paying its mana cost\. If that spell would be put into your "
    r"graveyard, exile it instead$"
)
# "exile the top [N] card[s] of your library" (Mystic Forge), no play
# permission attached.
_EXILE_TOP_OF_YOUR_LIBRARY_RE = re.compile(
    r"^exile the top (?:(?P<n>[a-z]+) )?cards? of your library$"
)
_COUNT_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
# "shuffle your library. Then exile the top card of your library. Until
# end of turn, you may play that card without paying its mana cost"
# (Mind's Desire) -- the same effect as Urza, Lord High Artificer's
# comma-joined wording (``ShuffleThenExileTopMayPlayFree``). A literal:
# the only card in the pools with this sentence split; broader grammar
# deferred until a second example exists.
_SHUFFLE_EXILE_TOP_PLAY_FREE_LINE = (
    "shuffle your library. Then exile the top card of your library. Until "
    "end of turn, you may play that card without paying its mana cost"
)
# "exile all the cards from your hand, then draw that many cards. Until
# the end of your next turn, you may play cards exiled this way" (Hex
# Magic). A literal: the only card in the pools with this shape; broader
# grammar deferred until a second example exists.
_EXILE_HAND_DRAW_THAT_MANY_PLAY_LINE = (
    "exile all the cards from your hand, then draw that many cards. Until "
    "the end of your next turn, you may play cards exiled this way"
)
# "remove a[n] <kind> counter from this <permanent>" (Dark Depths, its
# self-name rewritten by the land-line hook; Moonshadow's -1/-1 counter).
_REMOVE_COUNTERS_FROM_SELF_RE = re.compile(
    r"^remove (?P<n>an?|[a-z]+) (?P<kind>[a-z]+|[+-]1/[+-]1) counters? from "
    r"this (?:land|artifact|creature|enchantment|permanent)$"
)
# "return a[n] <permanent> you control to its owner's hand" -- untargeted
# (Simic Growth Chamber's enters trigger).
_RETURN_YOUR_PERMANENT_TO_HAND_RE = re.compile(
    r"^return an? (?P<np>[a-z]+) you control to its owner's hand$"
)
# "add {G}{U}" -- two or more mana of different colors at once (the
# karoos); the base grammar reads only one-color runs.
_ADD_MIXED_MANA_RE = re.compile(r"^add (?P<mana>(?:\{[WUBRGC]\}){2,})$")
# "destroy each <np>. Add {A} or {B} for each permanent destroyed this
# way" (Culling Ritual).
_DESTROY_THEN_MANA_PER_DESTROYED_RE = re.compile(
    r"^(?P<destroy>destroy (?:each|all) .+?)\. Add \{(?P<a>[WUBRG])\} or "
    r"\{(?P<b>[WUBRG])\} for each permanent destroyed this way$"
)
# "<effect with X>, where X is <amount>" -- the X-scaled mana productions
# of Burnt Offering / Vivi Ornitier.
_WHERE_X_IS_RE = re.compile(r"^(?P<base>.+), where X is (?P<x>.+)$")
# "the sacrificed creature's mana value" / "<self>'s power".
_SACRIFICED_MV_PHRASE = re.compile(
    r"^the sacrificed (?:creature|artifact|permanent)'s mana value$"
)
_SOURCE_POWER_PHRASE = re.compile(r"^(?P<self>.+)'s power$")


_CARD_TYPE_PREDICATES = {
    "artifact": "IsArtifact", "creature": "IsCreature",
    "enchantment": "IsEnchantment", "land": "IsLand",
    "planeswalker": "IsPlaneswalker",
}


def _destroy_each_of_types(np: str):
    """``destroy each <t1>, <t2>, and <t3>`` -- a singular card-type
    list (Wrath of the Skies' "artifact, creature, and enchantment") the
    base removal grammar reads only in its plural "all" form."""
    from mtgz_ir.effects import object_predicates as op
    from mtgz_ir.effects.generic.targeted_removal import RemoveAllMatching

    words = re.split(r", and |, | and ", np)
    if len(words) < 2 or any(w not in _CARD_TYPE_PREDICATES for w in words):
        return None
    pred = None
    for w in words:
        atom = getattr(op, _CARD_TYPE_PREDICATES[w])()
        pred = atom if pred is None else pred | atom
    return RemoveAllMatching(
        permanent_filter=pred, action="destroy", label=f"each {np}",
    )


def _parse_x_formula(phrase: str):
    from mtgz_ir.effects.formulas import MVOfCostPaidObjectPlus
    from mtgz_ir.effects.generic import roto_mana as rm

    if _SACRIFICED_MV_PHRASE.match(phrase):
        return MVOfCostPaidObjectPlus(constant=0)
    m = _SOURCE_POWER_PHRASE.match(phrase)
    if m is not None and (
        m.group("self").startswith("this ") or m.group("self")[:1].isupper()
    ):
        return rm.SourcePower()
    return None


# "look at the top N cards of your library. You may put a[n] [non-<Sub> ]
# <descriptor> card from among them onto the battlefield. Put the rest on
# the bottom of your library in a random order" (Kinnan, Bonder Prodigy).
_LOOK_TOP_MAY_PUT_ONTO_BATTLEFIELD_RE = re.compile(
    r"^look at the top (?P<n>[a-z]+) cards of your library\. You may put "
    r"an? (?:non-(?P<nonsub>[A-Z][a-z]+) )?(?P<desc>.+? card) from among "
    r"them onto the battlefield\. Put the rest on the bottom of your "
    r"library in a random order$"
)
# "put a <kind> counter on it. Then for each <kind> counter on it, each
# opponent loses N life unless that player sacrifices a nonland permanent
# of their choice or discards a card" (Rottenmouth Viper).
_COUNTER_THEN_PUNISH_PER_COUNTER_RE = re.compile(
    r"^put an? (?P<kind>[a-z]+) counter on (?:it|this creature)\. Then for "
    r"each (?P=kind) counter on it, each opponent loses (?P<life>\d+) life "
    r"unless that player sacrifices a nonland permanent of their choice or "
    r"discards a card$"
)
# "sacrifice it unless you sacrifice any number of creatures with total
# power N or greater" (Phyrexian Dreadnought).
_SAC_UNLESS_TOTAL_POWER_RE = re.compile(
    r"^sacrifice (?:it|this creature) unless you sacrifice any number of "
    r"creatures with total power (?P<n>\d+) or greater$"
)
# "you get X {E}, then you may pay any amount of {E}. Destroy each <np>
# with mana value less than or equal to the amount of {E} paid this way"
# (Wrath of the Skies).
_GET_X_ENERGY_PAY_DESTROY_RE = re.compile(
    r"^you get X \{E\}, then you may pay any amount of \{E\}\. Destroy "
    r"each (?P<np>.+?) with mana value less than or equal to the amount of "
    r"\{E\} paid this way$"
)
# Amped Raptor's enters trigger. A literal: the only card in the pools
# with this energy-cast shape; broader grammar deferred until a second
# example exists.
_AMPED_RAPTOR_RE = re.compile(
    r"^you get (?P<e>(?:\{E\})+)\. Then if you cast it from your hand, "
    r"exile cards from the top of your library until you exile a nonland "
    r"card\. You may cast that card by paying an amount of \{E\} equal "
    r"to its mana value rather than paying its mana cost$"
)
# "target player mills N cards. If M or more mana was spent to cast that
# spell, that player mills K cards instead" (Exhibition Tidecaller).
_MILL_MORE_IF_MANA_SPENT_RE = re.compile(
    r"^target player mills (?P<n>[a-z]+) cards\. If (?P<m>[a-z]+) or more "
    r"mana was spent to cast that spell, that player mills (?P<k>[a-z]+) "
    r"cards instead$"
)
# "[converge — ]target player draws X cards, <this> deals X damage to any
# target, and you gain X life, where X is the number of colors of mana
# spent to cast this spell" (Together as One). A literal: the only card
# in the pools with this three-part converge body; broader grammar
# deferred until a second example exists.
_CONVERGE_DRAW_DAMAGE_LIFE_RE = re.compile(
    r"^(?:[Cc]onverge — )?[Tt]arget player draws X cards, (?P<self>.+?) deals X "
    r"damage to any target, and you gain X life, where X is the number of "
    r"colors of mana spent to cast this spell$"
)
# "until end of turn, any time you could activate a mana ability, you may
# pay 1 life. If you do, add {C}" (Channel). A literal: the only card in
# the pools with this player-level mana grant; broader grammar deferred
# until a second example exists.
_CHANNEL_LIFE_FOR_COLORLESS_LINE = (
    "until end of turn, any time you could activate a mana ability, you may "
    "pay 1 life. If you do, add {C}"
)
# "you may copy the exiled card. If you do, you may cast the copy without
# paying its mana cost" (Isochron Scepter). A literal: the only card in
# the pools with this copy-and-cast; broader grammar deferred until a
# second example exists.
_COPY_EXILED_CAST_FREE_LINE = (
    "you may copy the exiled card. If you do, you may cast the copy "
    "without paying its mana cost"
)
# "return target creature card with mana value X from your graveyard to
# the battlefield" (Chthonian Nightmare, X paid in energy). A literal: the
# only card in the pools with this X-matched reanimation; broader grammar
# deferred until a second example exists.
_RETURN_GRAVEYARD_CREATURE_MV_X_LINE = (
    "return target creature card with mana value X from your graveyard to "
    "the battlefield"
)
# "untap each <permanent> you control" -- the base grammar reads the plural
# "untap all <permanents> you control" (Grim Reaper's Sprint).
_UNTAP_EACH_YOU_CONTROL_RE = re.compile(
    r"^untap each (?P<np>creature|artifact|land|permanent) you control$"
)
# "<effect naming this creature> and it <verb phrase>" -- the pronoun is
# the source (Vivi Ornitier: "put a +1/+1 counter on this creature and it
# deals 1 damage to each opponent").
_EFFECT_ON_SELF_AND_IT_RE = re.compile(
    r"^(?P<a>.+? this creature) and it (?P<b>deals .+)$"
)
# Restriction tags of ``ManaCoin.spend_restriction`` this bucket adds
# (checked by ``effects.generic.roto_mana.roto_coin_satisfies``).
_CANT_BE_SPENT_ON_KIND_TAGS = {"nonartifact": "not_nonartifact_spell"}
# "add <mana>. This mana can't be spent to pay generic mana costs"
# (Jegantha, the Wellspring).
_ADD_MANA_NOT_FOR_GENERIC_RE = re.compile(
    r"^add (?P<mana>(?:\{[WUBRGC]\})+)\. This mana can't be spent to pay "
    r"generic mana costs$"
)
# "until end of turn, whenever a player taps an <Subtype> for mana, that
# player adds an additional {C}" (High Tide).
_UEOT_EXTRA_MANA_ON_LAND_TAP_RE = re.compile(
    r"^until end of turn, whenever a player taps an? (?P<sub>[A-Z][a-z]+) "
    r"for mana, that player adds an additional \{(?P<c>[WUBRGC])\}$"
)
# Cavern of Souls' restricted, uncounterable any-color mana. A literal:
# the only card in the pools with this restriction; broader grammar
# (other "of the chosen type" spend restrictions) deferred until a second
# example exists.
_CHOSEN_CREATURE_TYPE_MANA_LINE = (
    "add one mana of any color. Spend this mana only to cast a creature "
    "spell of the chosen type, and that spell can't be countered"
)


def parse_roto_mana_effect(phrase: str) -> Effect | None:
    """Effect phrases of this bucket (the last ``_parse_effect``
    fallback)."""
    if _disabled():
        return None
    from mtgz_ir.effects.generic import roto_mana as rm

    m = _ADD_PER_CARD_NAMED_IN_GRAVEYARDS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mana import AddManaOfColorPerCount

        return AddManaOfColorPerCount(
            color=m.group("c"),
            amount=rm.CardsNamedInAllGraveyards(m.group("name")),
        )
    m = _DESTROY_ALL_ENTERED_THIS_TURN_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        base = _parse_effect(f"destroy all {m.group('np')}")
        if base is None or getattr(base, "action", None) != "destroy":
            return None
        from dataclasses import replace

        return replace(
            base,
            permanent_filter=base.permanent_filter & rm.EnteredThisTurn(),
            label=f"{base.label} that entered this turn",
        )
    m = _ADD_PER_CARD_IN_TARGET_OPPONENTS_HAND_RE.match(phrase)
    if m is not None:
        return rm.AddManaPerCardInTargetOpponentsHand(color=m.group("c"))
    m = _ADD_MANA_CANT_BE_SPENT_ON_RE.match(phrase)
    if m is not None:
        tag = _CANT_BE_SPENT_ON_KIND_TAGS.get(m.group("kind"))
        if tag is None:
            return None
        from mtgz_ir.effects.generic.mana import AddManaToPool

        colors = re.findall(r"\{([WUBRGC])\}", m.group("mana"))
        return AddManaToPool(
            produces=tuple((c, 1) for c in colors), spend_restriction=tag,
        )
    if phrase == _CHOSEN_CREATURE_TYPE_MANA_LINE:
        return rm.AddAnyColorManaForChosenCreatureType()
    m = _ADD_MANA_NOT_FOR_GENERIC_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mana import AddManaToPool

        colors = re.findall(r"\{([WUBRGC])\}", m.group("mana"))
        return AddManaToPool(
            produces=tuple((c, 1) for c in colors),
            spend_restriction="colored_only",
        )
    m = _UEOT_EXTRA_MANA_ON_LAND_TAP_RE.match(phrase)
    if m is not None:
        return rm.GrantExtraManaOnLandTapUntilEndOfTurn(
            subtype=m.group("sub"), color=m.group("c"),
        )
    if phrase == _SHUFFLE_EXILE_TOP_PLAY_FREE_LINE:
        from mtgz_ir.effects.generic.cube_creatures_ub import (
            ShuffleThenExileTopMayPlayFree,
        )

        return ShuffleThenExileTopMayPlayFree()
    if phrase == _EXILE_HAND_DRAW_THAT_MANY_PLAY_LINE:
        return rm.ExileHandDrawThatManyMayPlayThem()
    m = _GET_X_ENERGY_PAY_DESTROY_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.generic.targeted_removal import RemoveAllMatching

        # The singular card-type list first: P5's general "destroy each
        # <np>" grammar also reads it, but describes the list with "or"
        # rather than the printed "and".
        base = _destroy_each_of_types(m.group("np"))
        if base is None:
            base = _parse_effect(f"destroy each {m.group('np')}")
        if not isinstance(base, RemoveAllMatching) or base.action != "destroy":
            return None
        return rm.GetXEnergyPayAnyDestroyMVAtMostPaid(destroy=base)
    m = _AMPED_RAPTOR_RE.match(phrase)
    if m is not None:
        return rm.GetEnergyThenExileUntilNonlandCastForEnergy(
            energy=m.group("e").count("{E}"),
        )
    m = _MILL_MORE_IF_MANA_SPENT_RE.match(phrase)
    if m is not None:
        n, mm, k = (_COUNT_WORDS.get(m.group(g)) for g in ("n", "m", "k"))
        if None in (n, mm, k):
            return None
        return rm.MillTargetPlayerMoreIfManaSpent(
            amount=n, amount_if_spent=k, mana_spent_at_least=mm,
        )
    m = _CONVERGE_DRAW_DAMAGE_LIFE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        damage = _parse_effect("deals 1 damage to any target")
        return rm.ConvergeDrawDamageGainLife(damage=damage)
    m = _COUNTER_THEN_PUNISH_PER_COUNTER_RE.match(phrase)
    if m is not None:
        return rm.CounterOnSourceThenPunishPerCounter(
            kind=m.group("kind"), life=int(m.group("life")),
        )
    m = _SAC_UNLESS_TOTAL_POWER_RE.match(phrase)
    if m is not None:
        return rm.SacrificeSourceUnlessSacrificeTotalPower(
            total_power=int(m.group("n")),
        )
    if phrase == "deals X damage to each of X targets":
        return rm.DealXDamageToEachOfXTargets()
    if phrase == _CHANNEL_LIFE_FOR_COLORLESS_LINE:
        return rm.GrantLifeForColorlessUntilEndOfTurn()
    if phrase == _COPY_EXILED_CAST_FREE_LINE:
        return rm.CopyImprintedCardMayCastFree()
    if phrase == _RETURN_GRAVEYARD_CREATURE_MV_X_LINE:
        return rm.ReturnTargetGraveyardCreatureWithMVX()
    m = _UNTAP_EACH_YOU_CONTROL_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        return _parse_effect(f"untap all {m.group('np')}s you control")
    m = _EFFECT_ON_SELF_AND_IT_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.base import Sequence
        from mtgz_ir.cards.parser.effects import _parse_effect

        first, second = _parse_effect(m.group("a")), _parse_effect(m.group("b"))
        if first is None or second is None:
            return None
        return Sequence(effects=(first, second))
    m = _LOOK_TOP_MAY_PUT_ONTO_BATTLEFIELD_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        n = _COUNT_WORDS.get(m.group("n"))
        desc = m.group("desc")
        card_filter = parse_card_descriptor(
            f"{'an' if desc[0] in 'aeiou' else 'a'} {desc}",
        )
        if n is None or card_filter is None:
            return None
        return rm.LookAtTopMayPutOntoBattlefieldRestBottom(
            count=n, card_filter=card_filter,
            excluded_subtype=m.group("nonsub") or "",
        )
    m = _RETURN_YOUR_PERMANENT_TO_HAND_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase

        f = parse_permanent_phrase(m.group("np"))
        if f is None:
            return None
        return rm.ReturnPermanentYouControlToHand(
            permanent_filter=f, label=m.group("np"),
        )
    m = _ADD_MIXED_MANA_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mana import AddManaToPool

        colors = re.findall(r"\{([WUBRGC])\}", m.group("mana"))
        if len(set(colors)) < 2:
            return None
        return AddManaToPool(produces=tuple((c, 1) for c in colors))
    m = _DESTROY_THEN_MANA_PER_DESTROYED_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.generic.targeted_removal import RemoveAllMatching

        base = _parse_effect(m.group("destroy"))
        if not isinstance(base, RemoveAllMatching) or base.action != "destroy":
            return None
        return rm.DestroyEachThenAddManaPerDestroyed(
            destroy=base, options=(m.group("a"), m.group("b")),
        )
    m = _WHERE_X_IS_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.generic.artifact_mechanics import (
            AddManaInAnyCombination,
        )

        base = _parse_effect(m.group("base"))
        formula = _parse_x_formula(m.group("x"))
        if (
            isinstance(base, AddManaInAnyCombination) and base.amount_is_x
            and formula is not None
        ):
            return rm.AddFormulaManaInAnyCombination(
                formula=formula, options=tuple(base.options),
            )
    m = _REMOVE_COUNTERS_FROM_SELF_RE.match(phrase)
    if m is not None:
        n = 1 if m.group("n") in ("a", "an") else _COUNT_WORDS.get(m.group("n"))
        if n is None:
            return None
        return rm.RemoveCountersFromSource(kind=m.group("kind"), amount=n)
    m = _EXILE_TOP_OF_YOUR_LIBRARY_RE.match(phrase)
    if m is not None:
        n = _COUNT_WORDS.get(m.group("n") or "one")
        if n is None:
            return None
        return rm.ExileTopCardsOfYourLibrary(count=n)
    m = _MAY_CAST_TARGET_GRAVEYARD_FREE_RE.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        desc = m.group("desc")
        article = "an" if desc[0] in "aeiou" else "a"
        card_filter = parse_card_descriptor(f"{article} {desc}")
        if card_filter is None:
            return None
        return rm.MayCastTargetGraveyardCardFree(
            card_filter=card_filter,
            mv_at_most_source_power=m.group("power") is not None,
        )
    return None


# ---------------------------------------------------------------------------
# Land lines (``land_lines._try_build_line_composed_land`` hook)
# ---------------------------------------------------------------------------

_CHOOSE_CREATURE_TYPE_AS_ENTERS_RE = re.compile(
    r"^As this land enters, choose a creature type\.$"
)


@dataclass(frozen=True)
class RotoLine:
    """What one permanent line contributes: ``CardPrinting`` field
    updates, activated and/or triggered abilities."""

    fields: dict = field(default_factory=dict)
    activated: tuple = ()
    triggered: tuple = ()
    # Aura lines: "Enchanted creature gets +P/+T and has <keywords>."
    # -> (P, T, keywords); "Enchant creature" -> "creature".
    aura_bonus: "tuple | None" = None
    enchant_type: "str | None" = None



# Enchantment statics (``cube_enchantments`` hook).
# "You may cast spells from your hand without paying their mana costs."
# (Omniscience). A literal: the only card in the pools with this
# permission; broader grammar deferred until a second example exists.
_FREE_CASTS_FROM_HAND_LINE = (
    "You may cast spells from your hand without paying their mana costs."
)
# "Any player may cast creature spells with mana value N or less without
# paying their mana costs and as though they had flash." (Aluren).
_ANY_PLAYER_FREE_FLASH_CREATURES_RE = re.compile(
    r"^Any player may cast creature spells with mana value (?P<n>\d+) or "
    r"less without paying their mana costs and as though they had flash\.$"
)
# "Rather than pay the mana cost for a spell, its controller may discard a
# card that shares a color with that spell." (Dream Halls). A literal: the
# only card in the pools with this granted cost; broader grammar deferred
# until a second example exists.
_DISCARD_SHARING_COLOR_ALT_COST_LINE = (
    "Rather than pay the mana cost for a spell, its controller may discard "
    "a card that shares a color with that spell."
)


# "At the beginning of each of your main phases, if you haven't added mana
# with this ability this turn, you may add X mana of any one color, where
# X is the number of <Subtype>s target opponent controls." (Carpet of
# Flowers).
_EACH_MAIN_PHASE_MANA_PER_OPPONENT_SUBTYPE_RE = re.compile(
    r"^At the beginning of each of your main phases, if you haven't added "
    r"mana with this ability this turn, you may add X mana of any one "
    r"color, where X is the number of (?P<sub>[A-Z][a-z]+)s target opponent "
    r"controls\.$"
)


def _each_main_phase_triggers(effect):
    """One trigger per main phase ("each of your main phases"), gated
    by the "haven't added mana with this ability" intervening-if."""
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic import roto_mana as rm
    from mtgz_ir.effects.predicates import YOU, Controlled, Event

    gate = Controlled(YOU) & rm.HasNotAddedManaWithThisAbilityThisTurn()
    return tuple(
        TriggeredAbility(event=event, predicate=gate, effect=effect)
        for event in (
            Event.BEGINNING_OF_PRECOMBAT_MAIN,
            Event.BEGINNING_OF_POSTCOMBAT_MAIN,
        )
    )


# "Enchanted creature gets +P/+T and has <keyword>[, <keyword>...]."
_ENCHANTED_GETS_AND_HAS_RE = re.compile(
    r"^Enchanted creature gets (?P<p>[+-]\d+)/(?P<t>[+-]\d+) and has "
    r"(?P<kws>[a-z ]+(?:, [a-z ]+)*(?:,? and [a-z ]+)?)\.$"
)
_GRANTABLE_KEYWORDS = frozenset({
    "flying", "first strike", "double strike", "deathtouch", "haste",
    "hexproof", "indestructible", "lifelink", "menace", "reach", "trample",
    "vigilance",
})
# "When this Aura enters, <effect>. If it's your main phase, there is an
# additional combat phase after this phase." (Grim Reaper's Sprint).
_ETB_THEN_EXTRA_COMBAT_IF_MAIN_RE = re.compile(
    r"^When this Aura enters, (?P<first>.+?)\. If it's your main phase, "
    r"there is an additional combat phase after this phase\.$"
)


# "Enchant <land subtype>" (Utopia Sprawl: Forest).
# "Enchant <land subtype>" (Utopia Sprawl) / "Enchant land" (Tarmogoyf
# Nest): aura_enchant_type "land", with the subtype ("" for any land).
_ENCHANT_LAND_SUBTYPE_RE = re.compile(
    r"^Enchant (?P<sub>Plains|Island|Swamp|Mountain|Forest|land)$"
)
_AS_AURA_ENTERS_CHOOSE_COLOR = "As this Aura enters, choose a color."
# "Whenever enchanted <land subtype> is tapped for mana, its controller
# adds an additional one mana of the chosen color." (Utopia Sprawl).
_ENCHANTED_LAND_TAPPED_ADDS_CHOSEN_RE = re.compile(
    r"^Whenever enchanted (?:Plains|Island|Swamp|Mountain|Forest|land) is "
    r"tapped for mana, its controller adds an additional one mana of the "
    r"chosen color\.$"
)


def _aura_line(line: str) -> RotoLine | None:
    if line == "Enchant creature":
        return RotoLine(enchant_type="creature")
    m = _ENCHANT_LAND_SUBTYPE_RE.match(line)
    if m is not None:
        sub = m.group("sub")
        return RotoLine(
            enchant_type="land",
            fields={"aura_enchant_subtype": "" if sub == "land" else sub},
        )
    if line == _AS_AURA_ENTERS_CHOOSE_COLOR:
        return RotoLine(fields={"enters_choosing_color": True})
    if _ENCHANTED_LAND_TAPPED_ADDS_CHOSEN_RE.match(line):
        return RotoLine(fields={"enchanted_land_adds_chosen_color": True})
    m = _ENCHANTED_GETS_AND_HAS_RE.match(line)
    if m is not None:
        kws = frozenset(
            k.strip() for k in re.split(r",? and |, ", m.group("kws"))
        )
        if not kws <= _GRANTABLE_KEYWORDS:
            return None
        return RotoLine(
            aura_bonus=(int(m.group("p")), int(m.group("t")), kws),
        )
    m = _ETB_THEN_EXTRA_COMBAT_IF_MAIN_RE.match(line)
    if m is not None:
        from mtgz_ir.cards.base import Sequence
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic import roto_mana as rm
        from mtgz_ir.effects.predicates import Event, IsSelf

        first = _parse_effect(m.group("first"))
        if first is None:
            return None
        return RotoLine(triggered=(TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=Sequence(effects=(
                first, rm.AdditionalCombatAfterThisMainPhase(),
            )),
        ),))
    return None


def parse_roto_enchantment_line(line: str) -> RotoLine | None:
    """An enchantment line of this bucket, or ``None``."""
    if _disabled():
        return None
    aura = _aura_line(line)
    if aura is not None:
        return aura
    m = _EACH_MAIN_PHASE_MANA_PER_OPPONENT_SUBTYPE_RE.match(line)
    if m is not None:
        from mtgz_ir.effects.generic import roto_mana as rm

        return RotoLine(triggered=_each_main_phase_triggers(
            rm.MayAddManaPerTargetOpponentsSubtype(subtype=m.group("sub")),
        ))
    if line == _FREE_CASTS_FROM_HAND_LINE:
        return RotoLine(fields={"grants_free_casts_from_hand": True})
    m = _ANY_PLAYER_FREE_FLASH_CREATURES_RE.match(line)
    if m is not None:
        return RotoLine(
            fields={"free_flash_creature_casts_max_mv": int(m.group("n"))},
        )
    if line == _DISCARD_SHARING_COLOR_ALT_COST_LINE:
        return RotoLine(fields={"grants_discard_alternative_cost": True})
    return None


# "Whenever you tap this <permanent> for mana, <effect>." (Forbidden
# Orchard).
_TAPPED_FOR_MANA_TRIGGER_RE = re.compile(
    r"^Whenever you tap this (?:land|artifact|creature|permanent) for mana, "
    r"(?P<eff>.+)\.$"
)
# "Whenever you tap a nonland permanent for mana, add one mana of any type
# that permanent produced." (Kinnan, Bonder Prodigy). A literal: the only
# card in the pools with this triggered mana ability; broader grammar
# deferred until a second example exists.
_KINNAN_EXTRA_MANA_LINE = (
    "Whenever you tap a nonland permanent for mana, add one mana of any "
    "type that permanent produced."
)


# "Sacrifice a[n] <type list>: Remove a time counter from this card.
# Activate only if this card is suspended." (Greater Gargadon).
_SUSPENDED_SAC_REMOVE_TIME_COUNTER_RE = re.compile(
    r"^Sacrifice an? (?P<noun>[a-z]+(?:, [a-z]+)*,? or [a-z]+|[a-z]+): Remove "
    r"a time counter from this card\. Activate only if this card is "
    r"suspended\.$"
)


def _type_union_predicate(noun: str):
    """``artifact, creature, or land`` -> IsArtifact | IsCreature | IsLand
    (``None`` outside the card-type vocabulary)."""
    from mtgz_ir.effects import object_predicates as op

    words = [w for w in re.split(r",? or |, ", noun) if w]
    pred = None
    for w in words:
        if w not in _CARD_TYPE_PREDICATES:
            return None
        atom = getattr(op, _CARD_TYPE_PREDICATES[w])()
        pred = atom if pred is None else pred | atom
    return pred


def _suspended_time_counter_ability(noun: str):
    from mtgz_ir.cards.base import ActivatedAbility, ActivationCost
    from mtgz_ir.effects.generic import roto_mana as rm
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        SacrificePermanentsCost,
    )

    pred = _type_union_predicate(noun)
    if pred is None:
        return None
    ability = ActivatedAbility(
        cost=ActivationCost(extra_costs=(SacrificePermanentsCost(
            permanent_filter=pred, count=1, another=False,
            label=f"a {noun}",
        ),)),
        effect=rm.RemoveTimeCounterFromSuspendedSource(),
    )
    return rm.ExileZoneAbility(ability=ability, activate_only_if_suspended=True)


# "<cost>: <effect>. Activate only during your turn and only once each
# turn." (Vivi Ornitier) -- the combined rider the base activated grammar
# reads only as separate sentences.
_ACTIVATE_DURING_YOUR_TURN_ONCE_EACH_TURN_RE = re.compile(
    r"^(?P<body>.+\.) Activate only during your turn and only once each "
    r"turn\.$"
)


# "As an additional cost to cast this spell, you may sacrifice any number
# of <permanents>. This spell costs {N} less to cast for each permanent
# sacrificed this way." (Rottenmouth Viper).
_OPTIONAL_SACRIFICE_ANY_NUMBER_RE = re.compile(
    r"^As an additional cost to cast this spell, you may sacrifice any "
    r"number of (?P<np>.+?)\. This spell costs \{(?P<n>\d+)\} less to cast "
    r"for each permanent sacrificed this way\.$"
)


def _optional_sacrifice_any_number(m):
    from mtgz_ir.cards.base import CostReduction
    from mtgz_ir.cards.parser.creature_wg_lines import WGLine
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic import roto_mana as rm

    pred = parse_permanent_phrase(m.group("np"), plural=True)
    if pred is None:
        return None
    reduction = CostReduction(
        amount=int(m.group("n")),
        condition=rm.PerPermanentSacrificedForCost(permanent_filter=pred),
    )
    return WGLine(fields={
        "optional_sacrifice_any_number": pred,
        "cost_reductions": (reduction,),
    })


# "When this creature enters, if {C}{C} was spent to cast it, <effect>."
# (Wistfulness).
_ETB_IF_MANA_SPENT_RE = re.compile(
    r"^When this creature enters, if (?P<mana>(?:\{[WUBRG]\})+) was spent "
    r"to cast it, (?P<eff>.+)\.$"
)


def _etb_if_mana_spent(m):
    from mtgz_ir.cards.parser.creature_wg_lines import WGLine
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic import roto_mana as rm
    from mtgz_ir.effects.predicates import Event, IsSelf

    colors = re.findall(r"\{([WUBRG])\}", m.group("mana"))
    if len(set(colors)) != 1:
        return None
    eff = _parse_effect(m.group("eff"))
    if eff is None:
        return None
    return WGLine(triggers=(TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & rm.ManaSpentToCastSource(
            color=colors[0], count=len(colors),
        ),
        effect=eff,
    ),))


def _once_each_turn_ability(body: str, name: str):
    from dataclasses import replace

    from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
    from mtgz_ir.effects.generic import roto_mana as rm

    ability = _parse_activated_ability_line(
        body.replace(f"where X is {name}'s", "where X is this creature's"),
    )
    if ability is None:
        return None
    cost = replace(
        ability.cost,
        extra_costs=tuple(ability.cost.extra_costs)
        + (rm.OnceEachTurnCost(key=body),),
    )
    return replace(
        ability, cost=cost, activate_only_during_your_turn=True,
    )


# "Return this <permanent> to its owner's hand" as a cost (Chthonian
# Nightmare) is P2's ``roto_zones.parse_roto_zones_cost_piece`` (also
# Recurring Nightmare), the next cost-piece fallback; P3's identical atom
# was removed when the packages were integrated.


# "[<Ability word> — ]This spell costs {N} less to cast if <condition>."
# for this bucket's conditions (Morbid: "a creature died this turn").
_ROTO_COST_REDUCTION_RE = re.compile(
    r"^(?:[A-Z][a-z]+ — )?This spell costs \{(?P<n>\d+)\} less to cast if "
    r"(?P<cond>.+)\.$"
)


def parse_roto_cost_reduction_line(line: str):
    """A ``CostReduction`` for this bucket's conditions (the first check
    of ``cost_reduction._parse_cost_reduction_line``), or ``None``."""
    if _disabled():
        return None
    m = _ROTO_COST_REDUCTION_RE.match(line)
    if m is None or m.group("cond") != "a creature died this turn":
        return None
    from mtgz_ir.cards.base import CostReduction
    from mtgz_ir.effects.generic import roto_mana as rm

    return CostReduction(
        amount=int(m.group("n")), condition=rm.CreatureDiedThisTurn(),
    )


def roto_additional_cost_fields(body: str) -> dict | None:
    """``CardPrinting`` fields for an "As an additional cost to cast this
    spell, <body>." the casting substrate doesn't know, or ``None``."""
    if _disabled():
        return None
    if body == "discard X cards":
        return {"additional_cost_discard_x": True}
    return None


def parse_roto_cost_piece(piece: str):
    """A composable ``ActivationCost.extra_costs`` atom of this bucket
    (the activated grammar's last cost-piece fallback), or ``None``."""
    if _disabled():
        return None
    from mtgz_ir.effects.generic import roto_mana as rm

    if piece == "Pay X {E}":
        return rm.PayXEnergyCost()
    return None


def _tapped_for_mana_trigger(line: str):
    m = _TAPPED_FOR_MANA_TRIGGER_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import Always, Event

    eff = _parse_effect(m.group("eff"))
    if eff is None:
        return None
    # The event is bookkeeping only: these triggers are stacked by the
    # mana-ability / automatic-payment hooks, never by event dispatch.
    return TriggeredAbility(
        event=Event.BECOMES_TAPPED, predicate=Always(), effect=eff,
    )


def parse_roto_creature_line(line: str, card: dict):
    """A creature line of this bucket (the creature builder's last
    fallback, after every other bucket declined it) as a ``WGLine``, or
    ``None``."""
    if _disabled():
        return None
    from mtgz_ir.cards.parser.creature_wg_lines import WGLine

    m = _ETB_IF_MANA_SPENT_RE.match(line)
    if m is not None:
        return _etb_if_mana_spent(m)
    m = _OPTIONAL_SACRIFICE_ANY_NUMBER_RE.match(line)
    if m is not None:
        return _optional_sacrifice_any_number(m)
    m = _ACTIVATE_DURING_YOUR_TURN_ONCE_EACH_TURN_RE.match(line)
    if m is not None:
        ability = _once_each_turn_ability(m.group("body"), card["name"])
        if ability is None:
            return None
        return WGLine(activated=(ability,))
    m = _SUSPENDED_SAC_REMOVE_TIME_COUNTER_RE.match(line)
    if m is not None:
        ability = _suspended_time_counter_ability(m.group("noun"))
        if ability is None:
            return None
        return WGLine(fields={"exile_activated_abilities": (ability,)})
    if line == _KINNAN_EXTRA_MANA_LINE:
        return WGLine(fields={"extra_mana_when_you_tap_nonland_for_mana": True})
    trig = _tapped_for_mana_trigger(line)
    if trig is not None:
        return WGLine(fields={"tapped_for_mana_triggers": (trig,)})
    return None


def extra_land_card_types() -> frozenset:
    """Card-type tuples beyond a plain Land the line-composed land builder
    accepts (artifact lands: Seat of the Synod, Darksteel Citadel)."""
    return frozenset() if _disabled() else frozenset({("Artifact", "Land")})


def merge_roto_fields(acc: dict, fields: dict) -> None:
    """Fold one line's printing fields into ``acc`` (keyword sets are
    unioned, everything else set)."""
    for key, value in fields.items():
        if key == "keywords":
            acc[key] = frozenset(acc.get(key, frozenset())) | frozenset(value)
        else:
            acc[key] = value


def extra_land_supertypes() -> frozenset[str]:
    """Supertypes beyond Legendary the line-composed land builder accepts
    (Snow: Dark Depths)."""
    return frozenset() if _disabled() else frozenset({"Snow"})


# "<Self> enters with <N> <kind> counters on it." in the old self-named
# wording (Dark Depths); "This land enters with ..." is the land
# grammar's own line.
_SELF_ENTERS_WITH_COUNTERS_RE = re.compile(
    r"^(?P<self>.+?) enters with (?P<n>[a-z]+|\d+) (?P<kind>[a-z]+) "
    r"counters? on it\.$"
)
# "When <self> has no <kind> counters on it, <effect>." -- a state
# trigger (CR 603.8; Dark Depths).
_STATE_TRIGGER_NO_COUNTERS_RE = re.compile(
    r"^When (?P<self>.+?) has no (?P<kind>[a-z]+) counters on it, "
    r"(?P<eff>.+)\.$"
)


def _keyword_only_line(line: str, name: str) -> frozenset:
    """The keywords of a keyword-only line ("Indestructible"), else an
    empty set."""
    from mtgz_ir.cards.parser.clause import _parse_clause

    clause = _parse_clause(line, name=name or None)
    if clause is None or not clause.keywords:
        return frozenset()
    if clause.triggers or clause.activated is not None or clause.statics:
        return frozenset()
    return frozenset(clause.keywords)


def _self_ref(text: str, name: str) -> bool:
    return text in (name, "this land", "this permanent")


def parse_roto_land_line(line: str, name: str = "") -> RotoLine | None:
    """A land line of this bucket, or ``None`` (the line-composed land
    grammar then tries its own)."""
    if _disabled():
        return None
    if _CHOOSE_CREATURE_TYPE_AS_ENTERS_RE.match(line):
        return RotoLine(fields={"enters_choosing_creature_type": True})
    keywords = _keyword_only_line(line, name)
    if keywords:
        # "Indestructible" (Darksteel Citadel).
        return RotoLine(fields={"keywords": keywords})
    trig = _tapped_for_mana_trigger(line)
    if trig is not None:
        return RotoLine(fields={"tapped_for_mana_triggers": (trig,)})
    m = _SELF_ENTERS_WITH_COUNTERS_RE.match(line)
    if m is not None and _self_ref(m.group("self"), name):
        n = _COUNT_WORDS.get(m.group("n"))
        if m.group("n").isdigit():
            n = int(m.group("n"))
        if n is None:
            return None
        return RotoLine(
            fields={"enters_with_named_counters": ((m.group("kind"), n),)},
        )
    m = _STATE_TRIGGER_NO_COUNTERS_RE.match(line)
    if m is not None and _self_ref(m.group("self"), name):
        trig = _parse_state_trigger_effect(m.group("eff"))
        if trig is None:
            return None
        # One state-trigger mechanism (CR 603.8): P5's board-rule
        # ``StateTrigger``, read through the "loses all abilities" /
        # Blood Moon suppression (``roto_board_statics.rules_in_play``).
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic.roto_mana import HasNoCountersOfKind
        from mtgz_ir.roto_board_statics import StateTrigger

        return RotoLine(fields={"board_rules": (StateTrigger(
            condition=HasNoCountersOfKind(m.group("kind")),
            ability=TriggeredAbility(effect=trig),
        ),)})
    if name and name in line:
        # An activated ability naming the land itself ("{3}: Remove an
        # ice counter from Dark Depths.").
        from mtgz_ir.cards.parser.activated import (
            _parse_activated_ability_line,
        )

        ability = _parse_activated_ability_line(line.replace(name, "this land"))
        if ability is not None:
            return RotoLine(activated=(ability,))
    return None


# "sacrifice it. If you do, <effect>" -- the state trigger's body.
_SACRIFICE_IT_IF_YOU_DO_RE = re.compile(r"^sacrifice it\. If you do, (?P<then>.+)$")


def _parse_state_trigger_effect(phrase: str):
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import roto_mana as rm

    m = _SACRIFICE_IT_IF_YOU_DO_RE.match(phrase)
    if m is not None:
        then = _parse_effect(m.group("then"))
        return None if then is None else rm.SacrificeSourceThen(then_effect=then)
    return _parse_effect(phrase)


# ---------------------------------------------------------------------------
# Modal triggered abilities on noncreature permanents
# ---------------------------------------------------------------------------


def extract_modal_trigger_blocks(text: str, name: str):
    """Pull every ``When/Whenever <event>, choose one —`` header and its
    ``• <mode>`` lines out of a noncreature permanent's oracle ``text``
    (The Mightstone and Weakstone), parsing each block with the creature
    builder's modal-trigger grammar.

    Returns ``(remaining_text, triggers)``; ``None`` when a block is
    present but doesn't parse (the card stays unclaimed). Text without a
    block comes back unchanged with no triggers."""
    if _disabled():
        return text, ()
    from mtgz_ir.cards.parser.creature_builder import (
        _MODAL_CHOOSE_ONE_SUFFIX,
        _MODAL_MODE_PREFIX,
        _parse_modal_trigger_block,
    )
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    lines = text.split("\n")
    kept: list[str] = []
    triggers = []
    i = 0
    while i < len(lines):
        line = _strip_reminder_text(lines[i])
        if line.endswith(_MODAL_CHOOSE_ONE_SUFFIX) and line.startswith(
            ("When ", "Whenever "),
        ):
            modes = []
            j = i + 1
            while j < len(lines) and _strip_reminder_text(lines[j]).startswith(
                _MODAL_MODE_PREFIX,
            ):
                modes.append(_strip_reminder_text(lines[j]))
                j += 1
            if modes:
                trig = _parse_modal_trigger_block(line, modes, name=name)
                if trig is None:
                    return None
                triggers.append(trig)
                i = j
                continue
        kept.append(lines[i])
        i += 1
    return "\n".join(kept), tuple(triggers)


# ---------------------------------------------------------------------------
# Spell clause rewrites (run before the spell builder splits sentences)
# ---------------------------------------------------------------------------

# "Choose one. If you control a commander as you cast this spell, you may
# choose both instead." (Jeska's Will). The engine plays two-player games
# with no commander (no player ever controls one, CR 903.3), so the
# upgrade's condition is false in every game it simulates and the header
# is exactly "Choose one —" (documented in SEARCH_LIMITATIONS).
_COMMANDER_CHOOSE_BOTH_HEADER_RE = re.compile(
    r"^Choose one\. If you control a commander as you cast this spell, you "
    r"may choose both instead\.$"
)


def rewrite_spell_clauses(clauses: list[str]) -> list[str]:
    """``clauses`` (a spell's oracle lines) with this bucket's header
    shapes rewritten to the base grammar's equivalent."""
    if _disabled():
        return clauses
    return [
        "Choose one —" if _COMMANDER_CHOOSE_BOTH_HEADER_RE.match(c) else c
        for c in clauses
    ]


# ---------------------------------------------------------------------------
# Card-level lines (``source.build_card_record`` hook)
# ---------------------------------------------------------------------------

# "Suspend N—<cost>" (CR 702.62), reminder text stripped.
_SUSPEND_RE = re.compile(r"^Suspend (?P<n>\d+)—(?P<cost>(?:\{[^}]+\})+)$")


def split_roto_card_lines(card: dict) -> tuple[dict, dict]:
    """Peel this bucket's card-level lines off a single-faced record
    before the per-type builders run: ``Suspend N—<cost>`` (a keyword
    that works from the hand and from exile, whatever the card's type).
    Also flags a nonland card printed with no mana cost (Lotus Bloom),
    which can't be cast by paying its mana cost (CR 202.1b).

    Returns ``(record, CardPrinting field updates)``."""
    if _disabled() or card.get("card_faces"):
        return card, {}
    from mtgz_ir.cards.base import parse_mana_cost
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    fields: dict = {}
    if not card.get("mana_cost") and "Land" not in card.get("type_line", ""):
        fields["has_no_mana_cost"] = True
    from mtgz_ir.cards.base import _parse_type_line

    supertypes = _parse_type_line(card.get("type_line", ""))[0]
    if "Snow" in supertypes:
        # The per-type builders rebuild the type line from their own
        # flags (Legendary only): keep the printed supertypes so a snow
        # permanent is a snow source (CR 205.4g, 107.4h).
        fields["supertypes"] = tuple(supertypes)
        fields["type_line"] = card["type_line"]
    text = card.get("oracle_text") or ""
    kept: list[str] = []
    for raw in text.split("\n"):
        m = _SUSPEND_RE.match(_strip_reminder_text(raw).strip())
        if m is None:
            kept.append(raw)
            continue
        fields["suspend_count"] = int(m.group("n"))
        fields["suspend_cost"] = parse_mana_cost(m.group("cost"))
        fields["suspend_cost_str"] = m.group("cost")
    if "suspend_count" not in fields:
        return card, fields
    stripped = dict(card)
    stripped["oracle_text"] = "\n".join(kept)
    return stripped, fields


def _parse_roto_artifact_line(line: str) -> dict | None:
    """``CardPrinting`` field updates for one noncreature-artifact line
    of this bucket, or ``None``."""
    from mtgz_ir.cards.parser.printing_keywords import (
        parse_cast_from_top_kinds,
    )

    kinds = parse_cast_from_top_kinds(line)
    if kinds is not None:
        if kinds == ("artifact",):
            return {"grants_cast_artifact_from_top_of_library": True}
        return {"cast_from_top_of_library_kinds": kinds}
    return None


def extract_roto_artifact_lines(text: str, name: str):
    """The artifact builder's hook: lift this bucket's lines out of a
    noncreature artifact's oracle ``text``. Returns ``(remaining_text,
    triggers, printing_fields)``, or ``None`` when a recognized block
    doesn't parse (the card stays unclaimed)."""
    extracted = extract_modal_trigger_blocks(text, name)
    if extracted is None:
        return None
    text, triggers = extracted
    if _disabled():
        return text, triggers, {}
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    kept: list[str] = []
    fields: dict = {}
    for raw in text.split("\n"):
        parsed = _parse_roto_artifact_line(_strip_reminder_text(raw))
        if parsed is None:
            kept.append(raw)
        else:
            fields.update(parsed)
    return "\n".join(kept), triggers, fields


__all__ = [
    "extract_roto_artifact_lines",
    "extract_modal_trigger_blocks",
    "parse_roto_land_line",
    "rewrite_spell_clauses",
    "parse_activate_only_if_condition",
    "parse_roto_mana_effect",
]
