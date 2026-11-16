"""Oracle-text grammar for the cube creatures bucket: red / multicolor
/ colorless creatures (PowerLSV cube).

Four hook points, each a parameterized recognizer family that only
claims text nothing older understood:

* :func:`parse_rmc_cost_piece` — composable activation-cost atoms,
  called by ``activated._classify_cost_piece`` after every dedicated
  atom declined ("Sacrifice a Forest", "Sacrifice five Treasures",
  "Remove a +1/+1 counter from this creature", "Tap an untapped Rebel
  you control", "Exile this card from your graveyard").
* :func:`parse_rmc_effect` — the LAST fallback of
  ``effects._parse_effect`` (after the planeswalker extension and the
  card-flow grammar).
* :func:`parse_rmc_creature_line_pre` — whole-line shapes the creature
  builder must see BEFORE the generic clause dispatcher: abilities that
  function from the hand or the graveyard, Exhaust, Dash.
* :func:`parse_rmc_creature_line_post` — whole-line shapes tried only
  after the clause dispatcher declined the line (statics).

Phrases reaching :func:`parse_rmc_effect` are *normalized* (no trailing
period, lower-cased first letter; later sentences keep their case).
"""

from __future__ import annotations

import contextlib
import functools
import re
from dataclasses import dataclass, field, replace

from mtgz_ir.effects.base import (
    ActivatedAbility,
    ActivationCost,
    Effect,
    StaticAbility,
    StaticScope,
)

_CARDINALS: dict[str, int] = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4,
    "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
_N = r"(?:\d+|a|an|one|two|three|four|five|six|seven|eight|nine|ten)"


def _n(token: str) -> int:
    return int(token) if token.isdigit() else _CARDINALS[token]


# ---------------------------------------------------------------------------
# EoE guard
# ---------------------------------------------------------------------------
#
# The Edge-of-Eternities-era pools carry cross-engine parity evidence
# keyed on exactly which printings the parser claims. This grammar (and
# every cube / roto grammar that shares ``_DISABLED``) targets the cube
# and the Vintage Roto list, so it is switched off while building a
# printing that appears ONLY in the EoE-era dumps -- on neither target
# list. Cards shared with the cube (Screaming Nemesis, Fear of Missing
# Out) or with the roto list (Leyline of the Void) still use it. Without
# the guard the generic cost atoms would newly claim e.g. Intruding
# Soulrager ("Sacrifice a Room"), changing the EoE claimed set.
#
# Roto joined the target lists on 2026-10-03 (user decision; EoE's
# claimed set accepted to move 481 -> 482). That opens the guard for the
# 11 EoE-era printings on the roto list, not for the rest of the
# Duskmourn / EoE dumps; Leyline of the Void is the only one whose build
# changes (``tests/test_roto_zones_graveyard_hate.py`` pins that).

_DISABLED = [False]


@contextlib.contextmanager
def rmc_grammar_disabled():
    """Switch every recognizer in this module off for the block."""
    prior = _DISABLED[0]
    _DISABLED[0] = True
    try:
        yield
    finally:
        _DISABLED[0] = prior


@functools.lru_cache(maxsize=1)
def _eoe_only_names() -> frozenset[str]:
    from mtgz_ir.cards.pools import load_pool_cards

    eoe = {c["name"] for c in load_pool_cards("eoe")}
    targets = {
        c["name"] for pool in ("cube", "roto") for c in load_pool_cards(pool)
    }
    return frozenset(eoe - targets)


def is_eoe_only_printing(name: str) -> bool:
    """Whether ``name`` is an EoE-era printing on neither target list
    (the PowerLSV cube, the Vintage Roto list)."""
    try:
        return name in _eoe_only_names()
    except FileNotFoundError:  # pragma: no cover - dumps are committed
        return False


# ---------------------------------------------------------------------------
# Permanent descriptors for cost atoms
# ---------------------------------------------------------------------------

_TYPE_WORDS = ("artifact", "creature", "land", "enchantment", "planeswalker")


def _type_predicate(word: str):
    from mtgz_ir.effects.object_predicates import (
        IsArtifact, IsCreature, IsEnchantment, IsLand, IsPlaneswalker,
    )

    return {
        "artifact": IsArtifact, "creature": IsCreature, "land": IsLand,
        "enchantment": IsEnchantment, "planeswalker": IsPlaneswalker,
    }[word]()


def _singular(word: str) -> str:
    if word.endswith("ies"):
        return word[:-3] + "y"
    if word.endswith("ves"):
        return word[:-3] + "f"
    if word.endswith("s") and not word.endswith("ss"):
        return word[:-1]
    return word


def _permanent_noun_predicate(noun: str, plural: bool):
    """``artifact`` / ``creature or artifact`` / ``Forest`` /
    ``Treasures`` -> an ObjectPredicate over permanents (``None`` when a
    word is outside the closed vocabulary)."""
    from mtgz_ir.effects.object_predicates import HasSubtype

    pred = None
    for word in noun.split(" or "):
        word = word.strip()
        if plural:
            word = _singular(word)
        if word in _TYPE_WORDS:
            atom = _type_predicate(word)
        elif re.fullmatch(r"[A-Z][a-z]+", word):
            atom = HasSubtype(word)
        else:
            return None
        pred = atom if pred is None else pred | atom
    return pred


# ---------------------------------------------------------------------------
# Cost atoms
# ---------------------------------------------------------------------------

_SAC_PIECE = re.compile(
    rf"^Sacrifice (?:(?P<another>another)|(?P<n>{_N})) (?P<noun>.+)$"
)
_REMOVE_P1_FROM_SELF_PIECE = re.compile(
    rf"^Remove (?P<n>{_N}) \+1/\+1 counters? from this "
    r"(?:creature|artifact|permanent)$"
)
_TAP_UNTAPPED_PIECE = re.compile(
    rf"^Tap (?P<n>{_N}) untapped (?P<noun>.+) you control$"
)
_EXILE_SELF_FROM_GRAVEYARD_PIECE = "Exile this card from your graveyard"


def parse_rmc_cost_piece(piece: str):
    """A composable ``ActivationCost.extra_costs`` atom, or ``None``."""
    if _DISABLED[0]:
        return None
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        ExileSelfFromGraveyardCost,
        RemoveCountersFromSelfCost,
        SacrificePermanentsCost,
        TapUntappedPermanentsCost,
    )

    if piece == _EXILE_SELF_FROM_GRAVEYARD_PIECE:
        return ExileSelfFromGraveyardCost()
    m = _REMOVE_P1_FROM_SELF_PIECE.match(piece)
    if m is not None:
        return RemoveCountersFromSelfCost(count=_n(m.group("n")))
    m = _SAC_PIECE.match(piece)
    if m is not None:
        count = 1 if m.group("another") else _n(m.group("n"))
        pred = _permanent_noun_predicate(m.group("noun"), plural=count > 1)
        if pred is None:
            return None
        label = f"{m.group('another') or m.group('n')} {m.group('noun')}"
        return SacrificePermanentsCost(
            permanent_filter=pred, count=count,
            another=bool(m.group("another")), label=label,
        )
    m = _TAP_UNTAPPED_PIECE.match(piece)
    if m is not None:
        count = _n(m.group("n"))
        pred = _permanent_noun_predicate(m.group("noun"), plural=count > 1)
        if pred is None:
            return None
        return TapUntappedPermanentsCost(
            permanent_filter=pred, count=count,
            label=f"{m.group('n')} untapped {m.group('noun')} you control",
        )
    return None


# ---------------------------------------------------------------------------
# Effects
# ---------------------------------------------------------------------------

# "<this creature|this artifact|he|she> deals ..." / "... fights ..." —
# a source subject in front of an otherwise normalized effect (name
# references are already rewritten to "this creature"). A bare "it"
# subject is NOT accepted here: spell text never uses it for the spell
# itself, and the creature-trigger path strips it earlier.
_SOURCE_SUBJECT = re.compile(
    r"^(?:this creature|this artifact|he|she) (?P<rest>(?:deals|fights) .+)$"
)
_ADD_N_MANA_OF_ANY_ONE_COLOR = re.compile(
    r"^add (?P<n>\d+|two|three|four|five|six|seven|eight|nine|ten) mana of "
    r"any one color$"
)
_RETURN_THIS_CARD_FROM_GRAVEYARD = re.compile(
    r"^return this card from your graveyard to the battlefield"
    r"(?P<tapped> tapped)?$"
)
# Impulse family: "exile the top <N> card(s) of <whose> library[ face
# down]. <permission sentence>". The permission sentence is looked up in
# ``_PERMISSIONS``; Caves of Chaos Adventurer's two-branch form ("If
# <gate>, <free permission>. Otherwise, <permission>.") is its own arm.
_EXILE_TOP_WITH_PERMISSION = re.compile(
    rf"^exile the top (?:(?P<n>{_N}) )?cards? of (?P<whose>your|that player's|their) "
    r"library(?P<face_down> face down)?\. (?P<perm>.+)$"
)
_CONDITIONAL_FREE_PERMISSION = re.compile(
    r"^[Ii]f (?P<gate>[^,]+), (?P<free>you may play that card this turn "
    r"without paying its mana cost)\. Otherwise, (?P<paid>you may play that "
    r"card this turn)$"
)
_PERMISSIONS: dict[str, dict] = {
    "you may play that card this turn": {},
    "you may play them this turn": {},
    "you may play it this turn": {},
    "you may play them this turn without paying their mana costs": {"free": True},
    "you may play that card this turn without paying its mana cost": {"free": True},
    "you may look at and play that card this turn": {},
    "until end of turn, you may cast that card": {"cast_only": True},
    "you may play that card until your next end step": {
        "duration": "until_your_next_end_step",
    },
    "until the end of your next turn, you may play that card": {
        "duration": "until_end_of_your_next_turn",
    },
}
_EXILE_RANDOM_FROM_GRAVEYARD = re.compile(
    r"^exile a card at random from your graveyard\. (?P<perm>.+)$"
)
_EXILE_TARGET_FROM_GRAVEYARD_WITH_PERMISSION = re.compile(
    r"^exile target (?P<desc>.+?) card from your graveyard\. (?P<perm>.+)$"
)
_SACRIFICE_UNLESS_ESCAPED = "sacrifice it unless it escaped"
_TARGET_CREATURE_CANT_BLOCK = re.compile(
    r"^(?P<upto>up to one )?target creature can't block this turn$"
)
_CREATURES_CANT_BLOCK = re.compile(
    r"^creatures(?P<tp> target player controls)? can't block this turn$"
)
_IF_GATE_EFFECT = re.compile(r"^if (?P<gate>[^,]+), (?P<eff>.+)$")
_DAMAGE_2_PLUS_SACRIFICED_MV = re.compile(
    r"^deals damage equal to (?P<k>\d+) plus the sacrificed permanent's "
    r"mana value to any target$"
)


def _permission_kwargs(sentence: str) -> dict | None:
    """ExilePlayPermission kwargs for a printed permission sentence."""
    key = sentence[0].lower() + sentence[1:] if sentence else sentence
    m = _ATTACKED_WITH_SUBTYPE_PERMISSION.match(key)
    if m is not None:
        # "During any turn you attacked with a <Subtype>": not limited
        # to this turn -- for as long as the card stays exiled.
        return {
            "cast_only": True,
            "duration": "while_exiled",
            "requires_attacked_with_subtype": m.group("sub"),
            "any_color_mana": bool(m.group("any")),
        }
    return _PERMISSIONS.get(key)


# Robber of the Rich: "During any turn you attacked with a <Subtype>, you
# may cast that card[ and you may spend mana as though it were mana of
# any color to cast that spell]."
_ATTACKED_WITH_SUBTYPE_PERMISSION = re.compile(
    r"^during any turn you attacked with an? (?P<sub>[A-Z][a-z]+), you may "
    r"cast that card(?P<any> and you may spend mana as though it were mana "
    r"of any color to cast that spell)?$"
)


def _gate_state_predicate(gate: str):
    """The viewer-scoped ``StatePredicate`` behind an "if <gate>" phrase
    (the triggers module's intervening-if phrasebook), or ``None``."""
    from mtgz_ir.cards.parser.triggers import _parse_intervening_if_phrase
    from mtgz_ir.effects.predicates import StateBound

    pred = _parse_intervening_if_phrase(gate)
    if isinstance(pred, StateBound):
        return pred.inner
    return None


def parse_rmc_effect(phrase: str) -> Effect | None:
    """Last-resort effect recognizers for the bucket (see module doc)."""
    if _DISABLED[0]:
        return None
    from mtgz_ir.cards.parser.effects import _parse_effect

    m = _SOURCE_SUBJECT.match(phrase)
    if m is not None:
        return _parse_effect(m.group("rest"))

    m = _ADD_N_MANA_OF_ANY_ONE_COLOR.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.mana import AddNManaOfChosenColor

        return AddNManaOfChosenColor(amount=_n(m.group("n")))

    m = _RETURN_THIS_CARD_FROM_GRAVEYARD.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            ReturnThisCardFromGraveyardToBattlefield,
        )

        return ReturnThisCardFromGraveyardToBattlefield(
            tapped=bool(m.group("tapped")),
        )

    m = _EXILE_TOP_WITH_PERMISSION.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            ExileTopCardsWithPlayPermission,
        )

        base = dict(
            count=_n(m.group("n")) if m.group("n") else 1,
            whose={
                "your": "you", "that player's": "that_player",
                "their": "defending_player",
            }[m.group("whose")],
            face_down=bool(m.group("face_down")),
        )
        perm = m.group("perm")
        kwargs = _permission_kwargs(perm)
        if kwargs is not None:
            if base["face_down"] != perm.lower().startswith("you may look at"):
                return None
            return ExileTopCardsWithPlayPermission(**base, **kwargs)
        cond = _CONDITIONAL_FREE_PERMISSION.match(perm)
        if cond is not None and not base["face_down"]:
            gate = _gate_state_predicate(cond.group("gate"))
            if gate is not None:
                return ExileTopCardsWithPlayPermission(**base, free_if=gate)
        return None

    m = _EXILE_RANDOM_FROM_GRAVEYARD.match(phrase)
    if m is not None:
        kwargs = _permission_kwargs(m.group("perm"))
        if kwargs == {}:
            from mtgz_ir.effects.generic.cube_creatures_rmc import (
                ExileRandomGraveyardCardWithPlayPermission,
            )

            return ExileRandomGraveyardCardWithPlayPermission()
        return None

    m = _EXILE_TARGET_FROM_GRAVEYARD_WITH_PERMISSION.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            ExileTargetGraveyardCardWithPlayPermission,
        )

        kwargs = _permission_kwargs(m.group("perm"))
        desc = _alternation_card_filter(m.group("desc"))
        if desc is None:
            desc = parse_card_descriptor(_article(m.group("desc")) + " card")
        if kwargs is None or desc is None or set(kwargs) - {"duration"}:
            return None
        return ExileTargetGraveyardCardWithPlayPermission(
            card_filter=desc, **kwargs,
        )

    if phrase == _SACRIFICE_UNLESS_ESCAPED:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            SacrificeSelfUnlessEscaped,
        )

        return SacrificeSelfUnlessEscaped()

    m = _TARGET_CREATURE_CANT_BLOCK.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            TargetCreatureCantBlockThisTurn,
        )

        return TargetCreatureCantBlockThisTurn(optional=bool(m.group("upto")))

    m = _CREATURES_CANT_BLOCK.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            CreaturesCantBlockThisTurn,
        )

        return CreaturesCantBlockThisTurn(target_player=bool(m.group("tp")))

    m = _IF_GATE_EFFECT.match(phrase)
    if m is not None:
        gate = _gate_state_predicate(m.group("gate"))
        if gate is not None:
            inner = _parse_effect(m.group("eff"))
            if inner is not None:
                from mtgz_ir.effects.generic.cube_creatures_rmc import (
                    StateCondition,
                )
                from mtgz_ir.effects.generic.cube_walkers import IfCondition

                return IfCondition(condition=StateCondition(gate), then=inner)
            return None

    m = _DAMAGE_2_PLUS_SACRIFICED_MV.match(phrase)
    if m is not None:
        from mtgz_ir.effects.formulas import MVOfCostPaidObjectPlus
        from mtgz_ir.effects.generic.cube_creatures_rmc import DealDamageByFormula
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter

        return DealDamageByFormula(
            amount=MVOfCostPaidObjectPlus(constant=int(m.group("k"))),
            targets=(TargetSpec(any_target_filter()),),
        )
    return _parse_counts_and_hand_effect(phrase)


# --- counts, hand shapes, back-references ---------------------------------

_DISCARD_HAND = "discard your hand"
_DISCARD_DRAW_TOKENS_PER_NONLAND = re.compile(
    rf"^discard (?P<d>{_N}) cards, then draw (?P<r>{_N}) cards\. For each "
    r"nonland card discarded this way, (?P<tok>create .+)$"
)
_DISCARD_UP_TO_DRAW_THAT_MANY = re.compile(
    rf"^(?:you may )?discard up to (?P<n>{_N}) cards"
    # "You may discard up to two cards. If you do, draw that many cards."
    # (Fable of the Mirror-Breaker's chapter II) -- the same choice: the
    # count discarded may be zero.
    r"(?:, then|\. If you do,) draw that many cards$"
)
_GET_EXPERIENCE = "you get an experience counter"
_CREATE_FOR_EACH_EXPERIENCE = re.compile(
    r"^(?P<tok>create .+?) for each experience counter you have$"
)
_TARGET_GETS_MINUS_X = re.compile(
    r"^target creature gets -X/-X until end of turn, where X is "
    r"(?P<twice>twice )?the number of (?P<noun>.+) you control$"
)
_DAMAGE_DEFENDING_PER_THEIR = re.compile(
    r"^deals damage to defending player equal to the number of "
    r"(?P<noun>.+) they control$"
)
_MAY_THEN_REFLEXIVE = re.compile(
    r"^you may (?P<a>[^.]+)\. When you do, (?P<b>.+)$"
)
_TARGET_OPPONENT_CREATES = re.compile(r"^target opponent creates (?P<tok>.+)$")
_DAMAGE_EQUAL_SPELL_MV = (
    "deals damage equal to that spell's mana value to any target"
)
_THAT_CREATURE_DAMAGE_EQUAL_POWER = (
    "that creature deals damage equal to its power to any target"
)
_DAMAGE_THAT_MUCH_OTHER = re.compile(
    r"^deals that much damage to any other target"
    r"(?P<nolife>\. If a player is dealt damage this way, they can't gain "
    r"life for the rest of the game)?$"
)
_SEARCH_ALTERNATION_TO_BATTLEFIELD = re.compile(
    r"^search your library for an? (?P<desc>.+?) card, put (?:that card|it) "
    r"onto the battlefield(?P<tapped> tapped)?, then shuffle$"
)
_THEN_IF_YOU_DO = re.compile(r"^(?P<a>[^.]+)\. If you do, (?P<b>.+)$")
_EXILE_UP_TO_ONE_GRAVEYARD_CARD = "exile up to one target card from a graveyard"


def _count_noun_predicate(noun: str):
    """``Blood tokens`` / ``artifacts`` / ``Treasures`` / ``creatures``
    (a plural permanent noun) -> ObjectPredicate, or ``None``."""
    from mtgz_ir.effects.object_predicates import HasSubtype, IsNontoken

    words = noun.split(" ")
    token_only = False
    if words[-1] == "tokens":
        token_only = True
        words = words[:-1]
        if not words:
            return None
        base = _permanent_noun_predicate(" ".join(words), plural=False)
    else:
        base = _permanent_noun_predicate(noun, plural=True)
    if base is None:
        return None
    return base & ~IsNontoken() if token_only else base


def _parse_counts_and_hand_effect(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import cube_creatures_rmc as rmc

    if phrase == _DISCARD_HAND:
        return rmc.DiscardYourHand()
    m = _DISCARD_DRAW_TOKENS_PER_NONLAND.match(phrase)
    if m is not None:
        tok = _parse_effect(m.group("tok"))
        if tok is None or tok.target_count_range()[1] > 0:
            return None
        return rmc.DiscardThenDrawTokensPerNonland(
            discard=_n(m.group("d")), draw=_n(m.group("r")), token_effect=tok,
        )
    m = _DISCARD_UP_TO_DRAW_THAT_MANY.match(phrase)
    if m is not None:
        return rmc.DiscardUpToNThenDrawThatMany(n=_n(m.group("n")))
    if phrase == _GET_EXPERIENCE:
        return rmc.GetExperienceCounter()
    m = _CREATE_FOR_EACH_EXPERIENCE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.tokens import CreateToken

        tok = _parse_effect(m.group("tok"))
        if not isinstance(tok, CreateToken):
            return None
        return rmc.CreateTokenPerFormula(
            token_effect=tok, amount=rmc.ExperienceCounters(),
        )
    m = _TARGET_GETS_MINUS_X.match(phrase)
    if m is not None:
        from mtgz_ir.effects.formulas import CountControlledMatching, Scaled

        pred = _count_noun_predicate(m.group("noun"))
        if pred is None:
            return None
        amount = CountControlledMatching(pred)
        if m.group("twice"):
            amount = Scaled(amount, factor=2)
        return rmc.PumpTargetCreatureByFormulaUEOT(amount=amount)
    m = _DAMAGE_DEFENDING_PER_THEIR.match(phrase)
    if m is not None:
        pred = _count_noun_predicate(m.group("noun"))
        if pred is None:
            return None
        return rmc.DealDamageFromEvent(
            amount=rmc.CountPermanentsOfPlayer(pred), to_defending_player=True,
        )
    m = _MAY_THEN_REFLEXIVE.match(phrase)
    if m is not None:
        first = _parse_effect(m.group("a"))
        then = _parse_effect(m.group("b"))
        if first is None or then is None or first.target_count_range()[1] > 0:
            return None
        return rmc.MayDoThenReflexive(first=first, then=then)
    m = _TARGET_OPPONENT_CREATES.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.tokens import CreateToken

        tok = _parse_effect("create " + m.group("tok"))
        if not isinstance(tok, CreateToken):
            return None
        return rmc.TargetOpponentCreatesToken(token_effect=tok)
    if phrase == _DAMAGE_EQUAL_SPELL_MV:
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter

        return rmc.DealDamageFromEvent(
            amount=rmc.EventSubjectManaValue(),
            targets=(TargetSpec(any_target_filter()),),
            subject_slot="spell",
        )
    if phrase == _THAT_CREATURE_DAMAGE_EQUAL_POWER:
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter

        return rmc.DealDamageFromEvent(
            amount=rmc.EventSubjectPower(),
            targets=(TargetSpec(any_target_filter()),),
            subject_slot="obj",
        )
    m = _DAMAGE_THAT_MUCH_OTHER.match(phrase)
    if m is not None:
        from mtgz_ir.effects.object_predicates import IsSource, ObjectAnd
        from mtgz_ir.effects.targets import TargetSpec, any_target_filter

        return rmc.DealDamageFromEvent(
            amount=rmc.ResolvingDamageAmount(),
            targets=(TargetSpec(ObjectAnd(any_target_filter(), ~IsSource())),),
            no_lifegain_if_player_hit=bool(m.group("nolife")),
        )
    m = _SEARCH_ALTERNATION_TO_BATTLEFIELD.match(phrase)
    if m is not None:
        desc = _alternation_card_filter(m.group("desc"))
        if desc is None:
            return None
        from mtgz_ir.effects.generic.card_flow import SearchLibrary

        return SearchLibrary(
            card_filter=desc, destination="battlefield",
            tapped=bool(m.group("tapped")),
        )
    m = _THEN_IF_YOU_DO.match(phrase)
    if m is not None and not m.group("a").startswith("you may "):
        # Mandatory "<X>. If you do, <Y>." (the optional "you may"
        # form belongs to the artifact bucket's fallback). One target
        # slot at most across both halves.
        from mtgz_ir.effects.combinators import IfYouDo

        do = _parse_effect(m.group("a"))
        then = _parse_effect(m.group("b"))
        if do is None or then is None or (
            do.target_count_range()[1] > 0 and then.target_count_range()[1] > 0
        ):
            return None
        return IfYouDo(do_action=do, then_action=then)
    if phrase == _EXILE_UP_TO_ONE_GRAVEYARD_CARD:
        return rmc.ExileUpToOneTargetCardFromGraveyard()
    if phrase == _REVEAL_UNTIL_CREATURE_ATTACKING:
        return rmc.RevealUntilCreatureOntoBattlefieldAttacking()
    m = _REVEAL_TOP_ONE_PER_TYPE.match(phrase)
    if m is not None:
        return rmc.RevealTopNOnePerCardTypeToHand(count=_n(m.group("n")))
    m = _SOURCE_FIGHTS.match(phrase)
    if m is not None:
        np_filter = None
        if m.group("np") != "creature":
            from mtgz_ir.cards.parser.target_phrases import (
                parse_permanent_phrase,
            )

            np_filter = parse_permanent_phrase(m.group("np"))
            if np_filter is None:
                return None
        return rmc.SourceFightsTarget(
            optional=bool(m.group("upto")), other=bool(m.group("other")),
            np_filter=np_filter,
            exile_if_dies_this_turn=bool(m.group("exile")),
        )
    if phrase == _CHOOSE_PER_OPPONENT_DESTROY_RANDOM:
        return rmc.EachOpponentChoiceDestroyOneAtRandom()
    m = _BECOMES_WITH_BASE_PT.match(phrase)
    if m is not None:
        kws: frozenset = frozenset()
        if m.group("kw"):
            from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

            kws = _parse_keyword_grant_body(m.group("kw").replace(", and ", " and "))
            if not kws:
                return None
        return rmc.SourceBecomesIfSubtype(
            subtypes=tuple(m.group("subs").split(" ")),
            base_pt=(int(m.group("p")), int(m.group("t"))),
            keywords=kws,
            requires_subtype=m.group("req") or "",
        )
    return _parse_batch5_effect(phrase)


_CONNIVES_X_ATTACKING = (
    "target attacking creature connives X, where X is the number of "
    "attacking creatures"
)
_SACRIFICE_EFFECT = re.compile(
    r"^sacrifice (?P<another>another )?(?P<noun>(?:an? )?[a-z]+(?: or an? [a-z]+)*)$"
)
_MONSTROSITY = re.compile(rf"^monstrosity (?P<n>{_N}|\d+)$")


def _parse_batch5_effect(phrase: str) -> Effect | None:
    from mtgz_ir.effects.generic import cube_creatures_rmc as rmc

    if phrase == _CONNIVES_X_ATTACKING:
        return rmc.TargetConnives(amount=rmc.CountAttackingCreatures())
    m = _SACRIFICE_EFFECT.match(phrase)
    if m is not None:
        noun = re.sub(r"\ban? ", "", m.group("noun"))
        pred = _permanent_noun_predicate(noun, plural=False)
        if pred is None:
            return None
        return rmc.SacrificeAPermanent(
            permanent_filter=pred, another=bool(m.group("another")),
            label=f"{m.group('another') or ''}{m.group('noun')}",
        )
    m = _MONSTROSITY.match(phrase)
    if m is not None:
        return rmc.Monstrosity(count=_n(m.group("n")))
    if phrase == "its owner shuffles their graveyard into their library":
        return rmc.OwnerShufflesGraveyardIntoLibrary()
    if phrase == _REVEAL_TOP_LAND_ELSE_HAND:
        return rmc.RevealTopLandToBattlefieldElseHand()
    if phrase == _COPY_TARGET_SPELL_NEW_TARGETS:
        return rmc.CopyTargetInstantOrSorcerySpellYouControl()
    if phrase == _WELDER_SWAP:
        return rmc.SwapArtifactWithGraveyardArtifact()
    return _parse_sentence_sequence(phrase)


_REVEAL_TOP_LAND_ELSE_HAND = (
    "reveal the top card of your library. If it's a land card, put it onto "
    "the battlefield. Otherwise, put it into your hand"
)
_COPY_TARGET_SPELL_NEW_TARGETS = (
    "copy target instant or sorcery spell you control. You may choose new "
    "targets for the copy"
)
# Goblin Welder — the only card in the pool with this two-target swap;
# literal until a second printing varies the card types.
_WELDER_SWAP = (
    "choose target artifact a player controls and target artifact card in "
    "that player's graveyard. If both targets are still legal as this "
    "ability resolves, that player simultaneously sacrifices the artifact "
    "and returns the artifact card to the battlefield"
)


_REVEAL_UNTIL_CREATURE_ATTACKING = (
    "reveal cards from the top of your library until you reveal a creature "
    "card. Put that card onto the battlefield tapped and attacking and the "
    "rest on the bottom of your library in a random order"
)
_REVEAL_TOP_ONE_PER_TYPE = re.compile(
    rf"^reveal the top (?P<n>{_N}) cards of your library\. For each card "
    r"type, you may put a card of that type from among the revealed cards "
    r"into your hand\. Put the rest on the bottom of your library in a "
    r"random order$"
)
# "fights [up to one ][other ]target creature[ <qualifier>][. If that
# creature would die this turn, exile it instead]" -- the qualifier
# ("an opponent controls", "with power 3 or less", ...) goes through the
# shared permanent-phrase grammar; the exile rider is Mawloc's.
_SOURCE_FIGHTS = re.compile(
    r"^fights (?P<upto>up to one )?(?P<other>other )?target "
    r"(?P<np>creature(?: .+?)?)"
    r"(?P<exile>\. If that creature would die this turn, exile it instead)?$"
)
_CHOOSE_PER_OPPONENT_DESTROY_RANDOM = (
    "for each opponent, choose a nonland permanent that player controls. "
    "Destroy one of them chosen at random"
)
_BECOMES_WITH_BASE_PT = re.compile(
    r"^(?:if this creature is an? (?P<req>[A-Z][a-z]+), it|this creature) "
    r"becomes an? (?P<subs>[A-Z][a-z]+(?: [A-Z][a-z]+)*) with base power and "
    r"toughness (?P<p>\d+)/(?P<t>\d+)(?:(?:,| and) (?P<kw>[a-z ,]+))?$"
)


def _parse_sentence_sequence(phrase: str) -> Effect | None:
    """"<effect>. Then <effect>." — sentences resolved in order ("You get
    an experience counter. Then create ..."). The compound splitter
    declines these when an inner `` and `` belongs to one sentence
    ("tapped and attacking"). Only the explicit ``Then`` sequencing
    form is claimed (other multi-sentence phrases keep their owners);
    every sentence must parse and at most one may target."""
    from mtgz_ir.cards.parser.effects import (
        _EFFECT_SENTENCE_BOUNDARY,
        _parse_effect,
        _split_effect_sentences,
    )
    from mtgz_ir.effects.combinators import Sequence

    if not _EFFECT_SENTENCE_BOUNDARY.search(phrase) or ". Then " not in phrase:
        return None
    pieces = _split_effect_sentences(phrase)
    if len(pieces) < 2:
        return None
    parsed = []
    for piece in pieces:
        eff = _parse_effect(piece)
        if eff is None:
            return None
        parsed.append(eff)
    if sum(1 for e in parsed if e.target_count_range()[1] > 0) > 1:
        return None
    return Sequence(effects=tuple(parsed))


def _article(desc: str) -> str:
    """``artifact`` -> ``an artifact`` for ``parse_card_descriptor``."""
    return ("an " if desc[:1].lower() in "aeiou" else "a ") + desc


_CARD_TYPE_NAMES = {
    "artifact": "Artifact", "creature": "Creature", "enchantment": "Enchantment",
    "instant": "Instant", "sorcery": "Sorcery", "land": "Land",
    "planeswalker": "Planeswalker", "battle": "Battle",
}


def _alternation_card_filter(desc: str):
    """``Equipment, instant, or sorcery`` / ``artifact or Dragon`` — an
    OR-list of card types (lower-case) and subtypes (capitalized) ->
    ``CardTypeOrSubtypeFilter``; ``None`` for anything else (including
    a single type, which ``parse_card_descriptor`` owns)."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import CardTypeOrSubtypeFilter

    words = [w.strip() for w in re.split(r", or |, | or ", desc)]
    if len(words) < 2:
        return None
    types: list[str] = []
    subtypes: list[str] = []
    for w in words:
        if w in _CARD_TYPE_NAMES:
            types.append(_CARD_TYPE_NAMES[w])
        elif re.fullmatch(r"[A-Z][a-z]+", w):
            subtypes.append(w)
        else:
            return None
    return CardTypeOrSubtypeFilter(types=tuple(types), subtypes=tuple(subtypes))


# ---------------------------------------------------------------------------
# Whole-line creature recognizers
# ---------------------------------------------------------------------------


@dataclass
class RmcLine:
    """What one recognized creature line contributes to the printing.

    ``printing_fields`` are ``CardPrinting`` keyword overrides applied
    with ``dataclasses.replace`` after ``make_creature``.
    """

    keywords: tuple[str, ...] = ()
    triggers: tuple = ()
    activated: tuple = ()
    statics: tuple = ()
    hand_abilities: tuple = ()
    graveyard_abilities: tuple = ()
    printing_fields: dict = field(default_factory=dict)


_EXILE_FROM_HAND_ABILITY = re.compile(r"^Exile this card from your hand: (?P<eff>.+\.)$")
# "<cost>: It deals ..." — an activated ability whose effect names the
# source by pronoun (Walking Ballista's "Remove a +1/+1 counter from
# this creature: It deals 1 damage to any target.").
_ACTIVATED_IT_SUBJECT = re.compile(r"^(?P<cost>[^:]+): It (?P<verb>deals|fights) ")
_EXHAUST_LINE = re.compile(r"^Exhaust — (?P<body>.+)$")
_BOAST_LINE = re.compile(r"^Boast — (?P<body>.+)$")
# "When <this creature> enters, if you cast it, <effect>." — gated on the
# permanent having resolved from a spell (``GameObject.entered_via_cast``).
_IF_YOU_CAST_IT_ETB = re.compile(
    r"^When this creature enters, if you cast it, (?P<eff>.+)\.$"
)
_DASH_LINE = re.compile(r"^Dash (?P<cost>(?:\{[^}]+\})+)$")


# ---------------------------------------------------------------------------
# Trigger conditions
# ---------------------------------------------------------------------------


def parse_rmc_intervening_if(phrase: str):
    """Trigger-time gate predicates of the bucket (after the triggers
    module's own phrasebook declined):

    * ``defending player has more cards in hand than you`` (Robber of
      the Rich) -> the opponent's hand is larger than the controller's.
    """
    if _DISABLED[0]:
        return None
    if phrase == "defending player has more cards in hand than you":
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            OpponentHasMoreCardsInHand,
        )
        from mtgz_ir.effects.predicates import StateBound

        return StateBound(OpponentHasMoreCardsInHand())
    return None


def parse_rmc_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for trigger conditions of the bucket, tried
    after the predicate-trigger phrasebook and the planeswalker
    extension declined:

    * ``you cast this spell`` -> ``IS_CAST`` of the card itself
      (stacked by the cast pipeline, see ``IsThisSpell``)
    * ``this creature is dealt damage`` -> ``DEALT_DAMAGE`` of the source
    """
    if _DISABLED[0]:
        return None
    from mtgz_ir.effects.predicates import Event, IsSelf

    if phrase == "you cast this spell":
        from mtgz_ir.effects.generic.cube_creatures_rmc import IsThisSpell

        return (Event.IS_CAST, IsThisSpell())
    if phrase == "this creature is dealt damage":
        return (Event.DEALT_DAMAGE, IsSelf())
    from mtgz_ir.effects.object_predicates import HasSubtype, IsCreature
    from mtgz_ir.effects.predicates import YOU, Controlled, Subject

    if phrase == (
        "you or a permanent you control becomes the target of a spell or "
        "ability an opponent controls"
    ):
        from mtgz_ir.effects.predicates import OPPONENT

        return (
            Event.BECOMES_TARGET,
            Controlled(YOU, subject="target")
            & Controlled(OPPONENT, subject="controller"),
        )
    if phrase == "you discard one or more cards":
        from mtgz_ir.effects.generic.cube_creatures_rmc import OneOrMoreBatch

        return (
            Event.DISCARDS_CARD,
            Controlled(YOU, subject="player") & OneOrMoreBatch(),
        )
    if phrase == "this creature becomes monstrous":
        return (Event.BECOMES_MONSTROUS, IsSelf())
    if phrase == "this creature attack":
        # A plural-named source ("Whenever Raph & Mikey attack").
        return (Event.ATTACKS, IsSelf())
    m = _CAST_OR_COPY.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.effects_ext import _spell_filter
        from mtgz_ir.effects.generic.cube_creatures_rmc import CastOrCopy

        spell_pred = _spell_filter(m.group("filter"))
        if spell_pred is None:
            return None
        return (
            Event.IS_CAST,
            Subject("spell", spell_pred)
            & Controlled(YOU, subject="controller") & CastOrCopy(),
        )
    m = _SUBTYPE_BECOMES_TAPPED.match(phrase)
    if m is not None:
        return (
            Event.BECOMES_TAPPED,
            Subject(None, HasSubtype(m.group("sub"))) & Controlled(YOU),
        )
    m = _SELF_OR_ANOTHER_SUBTYPE_ENTERS.match(phrase)
    if m is not None:
        # One ability whose predicate covers both arms, so the trigger's
        # "that creature" is the entering object either way.
        return (
            Event.ENTERS,
            IsSelf() | (
                Subject(None, IsCreature() & HasSubtype(m.group("sub")))
                & Controlled(YOU)
            ),
        )
    return None


_STATIC_ABILITY_WORD = re.compile(
    r"^[A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)* — (?=As long as )"
)
_CAST_OR_COPY = re.compile(r"^you cast or copy an? (?P<filter>.+) spell$")
_SUBTYPE_BECOMES_TAPPED = re.compile(
    r"^an? (?P<sub>[A-Z][a-z]+) you control becomes tapped$"
)
_SELF_OR_ANOTHER_SUBTYPE_ENTERS = re.compile(
    r"^this creature or another (?P<sub>[A-Z][a-z]+) creature you control enters$"
)


def _functions_from_graveyard(ability: ActivatedAbility) -> bool:
    """An activated ability whose cost or effect only makes sense with
    the source in its owner's graveyard."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        ExileSelfFromGraveyardCost,
    )

    if any(isinstance(a, ExileSelfFromGraveyardCost) for a in ability.cost.extra_costs):
        return True
    return bool(getattr(ability.effect, "functions_from_graveyard", False))


def parse_rmc_creature_line_pre(line: str, name: str) -> RmcLine | None:
    """Shapes the creature builder must route before the generic clause
    dispatcher (which would otherwise file them as battlefield
    abilities)."""
    if _DISABLED[0]:
        return None
    from mtgz_ir.cards.parser.activated import (
        _is_mana_ability_effect,
        _normalize_activated_effect_phrase,
        _parse_activated_ability_line,
    )
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    m = _EXILE_FROM_HAND_ABILITY.match(line)
    if m is not None:
        effect = _parse_effect(_normalize_activated_effect_phrase(m.group("eff")))
        if effect is None:
            return None
        return RmcLine(hand_abilities=(ActivatedAbility(
            cost=ActivationCost(exile_self=True),
            effect=effect,
            is_mana_ability=_is_mana_ability_effect(effect),
        ),))

    m = _EXHAUST_LINE.match(line)
    if m is not None:
        ability = _parse_activated_ability_line(
            _substitute_self_name(m.group("body"), name)
        )
        if ability is None or _functions_from_graveyard(ability):
            return None
        return RmcLine(activated=(replace(ability, exhaust=True),))

    m = _BOAST_LINE.match(line)
    if m is not None:
        ability = _parse_activated_ability_line(
            _substitute_self_name(m.group("body"), name)
        )
        if ability is None or _functions_from_graveyard(ability):
            return None
        return RmcLine(activated=(replace(ability, boast=True),))

    m = _DASH_LINE.match(line)
    if m is not None:
        from mtgz_ir.cards.base import parse_mana_cost

        cost = m.group("cost")
        return RmcLine(printing_fields={
            "dash_cost": parse_mana_cost(cost), "dash_cost_str": cost,
        })

    if line.startswith("Escape—"):
        from mtgz_ir.cards.parser.spell_builder import _casting_keyword_fields

        fields = _casting_keyword_fields(line)
        if fields is not None and "escape_cost" in fields:
            return RmcLine(printing_fields=fields)
        return None

    m = _IF_YOU_CAST_IT_ETB.match(_substitute_self_name(line, name))
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect_phrase
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic.cube_creatures_rmc import EnteredViaCast
        from mtgz_ir.effects.predicates import Event, IsSelf

        effect = _parse_effect_phrase(m.group("eff"))
        if effect is None:
            return None
        return RmcLine(triggers=(TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf() & EnteredViaCast(),
            effect=effect,
        ),))

    if line == "Cascade":
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            Cascade,
            IsThisSpell,
        )
        from mtgz_ir.effects.predicates import Event

        return RmcLine(
            keywords=("cascade",),
            triggers=(TriggeredAbility(
                event=Event.IS_CAST, predicate=IsThisSpell(), effect=Cascade(),
            ),),
        )

    if ": " in line and "graveyard" in line:
        ability = _parse_activated_ability_line(_substitute_self_name(line, name))
        if ability is not None and _functions_from_graveyard(ability):
            return RmcLine(graveyard_abilities=(ability,))
    return None


_SPELLS_UNCOUNTERABLE = "Spells you control can't be countered."
_GRANT_WARD_PAY_LIFE = re.compile(
    r'^(?P<who>Other creatures|Creatures) you control have '
    r'"Ward—Pay (?P<n>\d+) life\."$'
)


def parse_rmc_creature_line_post(line: str, name: str) -> RmcLine | None:
    """Static lines tried after the clause dispatcher declined."""
    if _DISABLED[0]:
        return None
    # Ability words carry no rules meaning (CR 207.2c): "Delirium — As
    # long as ...".
    line = _STATIC_ABILITY_WORD.sub("", line, count=1)
    m = _ACTIVATED_IT_SUBJECT.match(line)
    if m is not None:
        from mtgz_ir.cards.parser.activated import _parse_activated_ability_line

        ability = _parse_activated_ability_line(
            line[: m.start("verb") - 3] + "This creature " + line[m.start("verb"):]
        )
        if ability is not None:
            return RmcLine(activated=(ability,))
        return None
    if line == _SPELLS_UNCOUNTERABLE:
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.SELF, grants_spells_uncounterable=True,
        ),))
    m = _GRANT_WARD_PAY_LIFE.match(line)
    if m is not None:
        scope = (
            StaticScope.OTHER_CREATURES_YOU_CONTROL
            if m.group("who") == "Other creatures"
            else StaticScope.CREATURES_YOU_CONTROL
        )
        return RmcLine(statics=(StaticAbility(
            scope=scope, grants_ward_life_cost=int(m.group("n")),
        ),))
    kw_line = _parse_extended_keyword_line(line)
    if kw_line is not None:
        return kw_line
    companion = parse_companion_line(line)
    if companion is not None:
        # Companion (CR 702.139): data for the card IR; this engine
        # doesn't play companions (SEARCH_LIMITATIONS, cube creatures
        # bucket).
        return RmcLine(printing_fields={"companion": companion})
    if _OUTSIDE_THE_GAME_LINE.match(line):
        # A companion clause with a condition not modeled yet (see
        # ``parse_companion_line``) / "Choose a Background" (commander
        # deck construction) / Conspiracy draft abilities (CR 905): they
        # work only outside a game — in a deck this engine plays they do
        # nothing (see SEARCH_LIMITATIONS, cube creatures bucket).
        return RmcLine()
    m = _GRAVEYARD_ARRIVAL_TRIGGER.match(
        _substitute_name_for_line(line, name)
    )
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            PutIntoGraveyardFromAnywhere,
        )
        from mtgz_ir.effects.predicates import Event

        effect = _parse_effect(m.group("eff"))
        if effect is None:
            return None
        return RmcLine(
            triggers=(TriggeredAbility(
                event=Event.DIES, predicate=PutIntoGraveyardFromAnywhere(),
                effect=effect,
            ),),
            printing_fields={"watched_in_graveyard": True},
        )
    m = _WHEN_THIS_A_OR_B.match(_substitute_name_for_line(line, name))
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect_phrase
        from mtgz_ir.cards.parser.triggers import _parse_predicate_trigger_phrase
        from mtgz_ir.effects.base import TriggeredAbility

        effect = _parse_effect_phrase(m.group("eff"))
        if effect is None:
            return None
        arms = []
        for arm in (m.group("a"), m.group("b")):
            parsed = _parse_predicate_trigger_phrase(f"this creature {arm}")
            if parsed is None:
                return None
            arms.append(TriggeredAbility(
                event=parsed[0], predicate=parsed[1], effect=effect,
            ))
        return RmcLine(triggers=tuple(arms))
    from mtgz_ir.cards.parser.planeswalker_builder import (
        _OPPONENTS_DRAW_AT_MOST_ONE_LINE,
    )

    if line == _OPPONENTS_DRAW_AT_MOST_ONE_LINE:
        return RmcLine(printing_fields={
            "opponents_cant_draw_more_than_one_card_each_turn": True,
        })
    m = _GRANT_BECOMES_TARGET_TRIGGER.match(line)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        effect = _parse_effect(m.group("eff"))
        if effect is None or effect.target_count_range()[1] > 0:
            return None
        limit = {"once": 1, "twice": 2, None: 0}.get(m.group("n"))
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            granted_becomes_target_effect=effect,
            granted_trigger_limit_per_turn=limit,
        ),))
    if line == _ZIRDA_ACTIVATION_REDUCTION:
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.SELF, activation_cost_reduction=2,
        ),))
    m = _LURRUS_GRAVEYARD_CAST.match(line)
    if m is not None:
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.SELF,
            graveyard_permanent_cast_mv_max=int(m.group("mv")),
        ),))
    m = _SUBTYPE_LORD.match(line)
    if m is not None:
        subtype = _singular(m.group("subs"))
        if not re.fullmatch(r"[A-Z][a-z]+", subtype):
            return None
        kws = frozenset()
        if m.group("kw"):
            from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

            kws = _parse_keyword_grant_body(m.group("kw"))
            if not kws:
                return None
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.subtype_you_control(
                subtype, other=bool(m.group("other")),
            ),
            power_bonus=int(m.group("p")), toughness_bonus=int(m.group("t")),
            granted_keywords=kws,
        ),))
    m = _AS_LONG_AS_SELF_GETS.match(line)
    if m is not None:
        cond = _STATIC_CONDITIONS.get(m.group("cond"))
        if cond is None:
            return None
        kws: frozenset = frozenset()
        if m.group("kw"):
            from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

            kws = _parse_keyword_grant_body(m.group("kw"))
            if not kws:
                return None
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.SELF, condition=cond(),
            power_bonus=int(m.group("p")), toughness_bonus=int(m.group("t")),
            granted_keywords=kws,
            must_attack_each_combat=bool(m.group("attack")),
        ),))
    m = _OPPONENTS_ENTER_TAPPED.match(line)
    if m is not None:
        types = []
        for word in re.split(r", and | and |, ", m.group("types")):
            t = _CARD_TYPE_NAMES.get(_singular(word.strip().lower()))
            if t is None:
                return None
            types.append(t)
        return RmcLine(statics=(StaticAbility(
            scope=StaticScope.SELF, opponents_enter_tapped_types=tuple(types),
        ),))
    if line == _CHOOSE_A_COLOR_AS_ENTERS:
        return RmcLine(printing_fields={"_chooses_color": True})
    from mtgz_ir.cards.parser.roto_layers import parse_all_objects_color_line

    color_fields = parse_all_objects_color_line(line)
    if color_fields is not None:
        # Painter's Servant ("... are the chosen color in addition to
        # their other colors."), one shape with Mycosynth Lattice's
        # "... are colorless."
        return RmcLine(printing_fields=color_fields)
    m = _HEAL_REPLACEMENT.match(line.replace(name, "this creature"))
    if m is None:
        from mtgz_ir.cards.parser.triggers import _substitute_self_name as _sub

        m = _HEAL_REPLACEMENT.match(_sub(line, name))
    if m is not None:
        return RmcLine(printing_fields={
            "heals_other_damage_when_dealt_damage": True,
        })
    if "would be put into a graveyard from anywhere" in line:
        from mtgz_ir.cards.parser.spell_builder import (
            _detect_shuffle_self_statics,
        )

        statics = _detect_shuffle_self_statics(line, name)
        if statics:
            return RmcLine(
                statics=statics,
                printing_fields={"watched_in_graveyard": True},
            )
        return None
    batch7 = _parse_batch7_line(line, name)
    if batch7 is not None:
        return batch7
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    m = _ENTERS_AND_WHENEVER.match(_substitute_self_name(line, name))
    if m is not None:
        from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line

        second = m.group("second")
        if second.startswith("it "):
            second = "this creature " + second[len("it "):]
        arms = []
        for text in (
            f"When {m.group('first')}, {m.group('eff')}.",
            f"Whenever {second}, {m.group('eff')}.",
        ):
            parsed = _parse_triggered_ability_line(text)
            if parsed is None or isinstance(parsed, tuple):
                return None
            arms.append(parsed)
        return RmcLine(triggers=tuple(arms))
    return None


# "[Other ]<Subtype>s you control get +N/+M[ and have <keywords>]."
_SUBTYPE_LORD = re.compile(
    r"^(?P<other>Other )?(?P<subs>[A-Z][a-z]+) you control get "
    r"\+(?P<p>\d+)/\+(?P<t>\d+)(?: and have (?P<kw>.+))?\.$"
)
# "As long as <condition>, this creature gets +N/+M[ and has <kws>]."
_AS_LONG_AS_SELF_GETS = re.compile(
    r"^As long as (?P<cond>.+?), this creature gets \+(?P<p>\d+)/\+(?P<t>\d+)"
    r"(?:(?: and|,) has (?P<kw>[a-z ]+?))?"
    r"(?P<attack>,? and attacks each combat if able)?\.$"
)


def _hand_at_most_one():
    from mtgz_ir.effects.conditions import HandSizeAtMost

    return HandSizeAtMost(1)


def _delirium():
    from mtgz_ir.effects.conditions import DeliriumActive

    return DeliriumActive()


_STATIC_CONDITIONS = {
    "you have one or fewer cards in hand": _hand_at_most_one,
    "there are four or more card types among cards in your graveyard": _delirium,
}
# "Companion — <condition>" (CR 702.139a): the deck conditions modeled
# (``effects.deck_conditions``). "Each <noun> card in your starting deck
# <requirement>."
_COMPANION_EACH = re.compile(
    r"^Companion — Each (?P<noun>permanent|nonland|creature) card in your "
    r"starting deck (?P<req>.+)\.$"
)
_COMPANION_MV_AT_MOST = re.compile(r"^has mana value (?P<n>\d+) or less$")


def _companion_noun(noun: str):
    from mtgz_ir.effects.object_predicates import IsCreature, IsLand
    from mtgz_ir.effects.value_predicates import IsPermanentCard

    if noun == "permanent":
        return IsPermanentCard()
    if noun == "nonland":
        return ~IsLand()
    return IsCreature()


def parse_companion_line(line: str):
    """The :class:`~mtgz_ir.effects.deck_conditions.Companion` a
    "Companion — <condition>" line spells (CR 702.139a), or ``None`` for
    another line or a condition not modeled yet: "Each <permanent /
    nonland / creature> card in your starting deck has mana value N or
    less" (Lurrus), "has an activated ability" (Zirda), "has a different
    name" (Lutri)."""
    from mtgz_ir.effects.deck_conditions import (
        Companion, EachCardInStartingDeck, StartingDeckDifferentNames,
    )
    from mtgz_ir.effects.value_predicates import (
        HasActivatedAbility, HasManaValueAtMost,
    )

    m = _COMPANION_EACH.match(line)
    if m is None:
        return None
    cards = _companion_noun(m.group("noun"))
    req = m.group("req")
    if req == "has a different name":
        return Companion(StartingDeckDifferentNames(cards=cards))
    if req == "has an activated ability":
        return Companion(EachCardInStartingDeck(cards=cards, must=HasActivatedAbility()))
    mv = _COMPANION_MV_AT_MOST.match(req)
    if mv is not None:
        return Companion(EachCardInStartingDeck(
            cards=cards, must=HasManaValueAtMost(int(mv.group("n"))),
        ))
    return None


_OUTSIDE_THE_GAME_LINE = re.compile(
    r"^(?:Companion — .+|Choose a Background|Draft this card face up\.|"
    r"As you draft a card, you may draft an additional card from that "
    r"booster pack\. If you do, put this card into that booster pack\.)$"
)
_GRAVEYARD_ARRIVAL_TRIGGER = re.compile(
    r"^When this creature is put into a graveyard from anywhere, (?P<eff>.+)\.$"
)
_WHEN_THIS_A_OR_B = re.compile(
    r"^When this creature (?P<a>enters|dies|attacks) or "
    r"(?P<b>becomes monstrous), (?P<eff>.+)\.$"
)
_GRANT_BECOMES_TARGET_TRIGGER = re.compile(
    r'^Creatures you control have "Whenever this creature becomes the target '
    r'of a spell or ability, (?P<eff>.+?)\.(?: This ability triggers only '
    r'(?P<n>once|twice) each turn\.)?"$'
)
_ZIRDA_ACTIVATION_REDUCTION = (
    "Abilities you activate that aren't mana abilities cost {2} less to "
    "activate. This effect can't reduce the mana in that cost to less than "
    "one mana."
)
_LURRUS_GRAVEYARD_CAST = re.compile(
    r"^Once during each of your turns, you may cast a permanent spell with "
    r"mana value (?P<mv>\d+) or less from your graveyard\.$"
)


# --- Multikicker (Batroc the Leaper) --------------------------------------
_MULTIKICKER_LINE = re.compile(r"^Multikicker (?P<cost>(?:\{[^}]+\})+)$")
_PRONOUN = r"(?:it|he|she|they)"
_POSSESSIVE = r"(?:its|his|her|their)"
_ENTERS_WITH_COUNTER_PER_KICK = re.compile(
    r"^this creature enters with a \+1/\+1 counter on (?:it|him|her|them) "
    rf"for each time {_PRONOUN} (?:was|were) kicked\.$"
)
_ETB_POWER_DAMAGE_PER_KICK = re.compile(
    rf"^When this creature enters, {_PRONOUN} deals? damage equal to "
    rf"{_POSSESSIVE} power to each of up to X targets, where X is the number "
    rf"of times {_PRONOUN} (?:was|were) kicked\.$"
)
# --- "Name Sticker" Goblin's die-roll ETB --------------------------------
_ENTERS_NOT_FROM_ROLL_DIE = re.compile(
    r"^When this creature enters from anywhere other than a graveyard or "
    r"exile, if it's on the battlefield and you control (?P<n>\w+) or fewer "
    r"creatures named (?P<named>.+), roll a (?P<sides>\d+)-sided die\.$"
)
_DIE_TABLE_ROW = re.compile(r"^(?P<lo>\d+)(?:-(?P<hi>\d+))? \| (?P<eff>.+)\.$")
# --- Laelia's library / graveyard exile watcher ---------------------------
_CARDS_EXILED_FROM_LIBRARY_OR_GRAVEYARD = re.compile(
    r"^Whenever one or more cards are put into exile from your library "
    r"and/or your graveyard, (?P<eff>.+)\.$"
)
_NUMBER_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def _self_referenced(line: str, name: str) -> str:
    """``_substitute_self_name`` plus the bare first-word short name
    ("Batroc" for "Batroc the Leaper"), lower-casing a sentence-initial
    "This creature"."""
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    out = _substitute_self_name(line, name) if name else line
    short = name.split(",")[0].split(" ")[0] if name else ""
    if short and len(short) > 2:
        out = re.sub(rf"\b{re.escape(short)}\b", "this creature", out)
    if out.startswith("This creature "):
        out = "this creature " + out[len("This creature "):]
    return out


def _parse_batch7_line(line: str, name: str) -> RmcLine | None:
    """Multikicker + per-kick lines, die-roll tables and the
    library/graveyard exile watcher."""
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic import cube_creatures_rmc as rmc
    from mtgz_ir.effects.predicates import Event, IsSelf

    m = _MULTIKICKER_LINE.match(line)
    if m is not None:
        from mtgz_ir.cards.base import parse_mana_cost

        return RmcLine(printing_fields={
            "kicker_cost": parse_mana_cost(m.group("cost")),
            "kicker_cost_str": m.group("cost"),
            "multikicker": True,
        })
    own = _self_referenced(line, name)
    if _ENTERS_WITH_COUNTER_PER_KICK.match(own):
        return RmcLine(printing_fields={"enters_with_counter_per_kick": True})
    if _ETB_POWER_DAMAGE_PER_KICK.match(own):
        return RmcLine(triggers=(TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=rmc.DealPowerDamageToEachOfUpToKickedTargets(),
        ),))
    m = _ENTERS_NOT_FROM_ROLL_DIE.match(_substitute_name_for_line(line, name))
    if m is not None:
        n = _NUMBER_WORDS.get(m.group("n"), None)
        if n is None and m.group("n").isdigit():
            n = int(m.group("n"))
        if n is None:
            return None
        named = m.group("named").strip()
        # The printing's own name is swapped for "this creature" by the
        # self-name substitution; the gate names the card itself.
        if named == "this creature":
            named = name
        return RmcLine(printing_fields={"_die_roll": (
            int(m.group("sides")), n, named,
        )})
    m = _DIE_TABLE_ROW.match(line)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        eff = m.group("eff")
        effect = _parse_effect(eff[:1].lower() + eff[1:])
        if effect is None or effect.target_count_range()[1] > 0:
            return None
        lo = int(m.group("lo"))
        hi = int(m.group("hi") or lo)
        return RmcLine(printing_fields={f"_die_row_{lo:03d}": (lo, hi, effect)})
    m = _CARDS_EXILED_FROM_LIBRARY_OR_GRAVEYARD.match(own)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_effect

        effect = _parse_effect(m.group("eff"))
        if effect is None:
            return None
        return RmcLine(
            triggers=(TriggeredAbility(
                event=Event.DIES,
                predicate=rmc.CardsExiledFromLibraryOrGraveyard(),
                effect=effect,
            ),),
            printing_fields={"watches_library_graveyard_exile": True},
        )
    return None


def assemble_die_roll_trigger(fields: dict):
    """Builder epilogue: pop the ``_die_roll`` header and its
    ``_die_row_NNN`` rows out of ``fields`` and return the assembled
    ``TriggeredAbility`` — ``None`` when there is no die roll, the string
    ``"invalid"`` when the header / rows don't form a complete table
    covering 1..N exactly."""
    rows = sorted(
        fields.pop(k) for k in [k for k in fields if k.startswith("_die_row_")]
    )
    header = fields.pop("_die_roll", None)
    if header is None and not rows:
        return None
    if header is None or not rows:
        return "invalid"
    sides, at_most, named = header
    expected = 1
    for lo, hi, _eff in rows:
        if lo != expected or hi < lo:
            return "invalid"
        expected = hi + 1
    if expected != sides + 1:
        return "invalid"
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic import cube_creatures_rmc as rmc
    from mtgz_ir.effects.predicates import Event, IsSelf

    return TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & rmc.EnteredNotFromZones()
        & rmc.ControlsAtMostNamed(name=named, at_most=at_most),
        effect=rmc.RollDieTable(
            sides=sides, table=tuple(rows),
            gate=rmc.NameStickerGate(name=named, at_most=at_most),
        ),
    )


def _substitute_name_for_line(line: str, name: str) -> str:
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    return _substitute_self_name(line, name) if name else line


def _parse_extended_keyword_line(line: str) -> RmcLine | None:
    """A comma-separated keyword line using pieces the base keyword
    grammar lacks: ``annihilator N`` (an attack trigger, CR 702.86) and
    ``protection from spells that are one or more colors``. Every piece
    must be recognized."""
    from mtgz_ir.cards.parser.keywords import _parse_keyword_piece
    from mtgz_ir.keyword_rules import protection_keyword

    pieces = [p.strip() for p in line.rstrip(".").split(", ")]
    if not any(
        p.lower().startswith(("annihilator ", "protection from spells"))
        for p in pieces
    ):
        return None
    keywords: list[str] = []
    triggers: list = []
    for piece in pieces:
        low = piece.lower()
        m = re.fullmatch(r"annihilator (\d+)", low)
        if m is not None:
            from mtgz_ir.effects.base import TriggeredAbility
            from mtgz_ir.effects.generic.cube_creatures_rmc import (
                DefendingPlayerSacrificesPermanents,
            )
            from mtgz_ir.effects.predicates import Event, IsSelf

            n = int(m.group(1))
            keywords.append(f"annihilator {n}")
            triggers.append(TriggeredAbility(
                event=Event.ATTACKS, predicate=IsSelf(),
                effect=DefendingPlayerSacrificesPermanents(count=n),
            ))
            continue
        if low == "protection from spells that are one or more colors":
            keywords.append(protection_keyword("spells that are one or more colors"))
            continue
        parsed = _parse_keyword_piece(piece[:1].upper() + piece[1:])
        if parsed is None:
            return None
        keywords.extend(parsed)
    return RmcLine(keywords=tuple(keywords), triggers=tuple(triggers))


# "[<Ability word> — ]<Types> your opponents control enter tapped."
_OPPONENTS_ENTER_TAPPED = re.compile(
    r"^(?:[A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)* — )?(?P<types>[A-Z][a-z]+(?:(?:,| and|, and) "
    r"[a-z]+)*) your opponents control enter tapped\.$"
)
_CHOOSE_A_COLOR_AS_ENTERS = "As this creature enters, choose a color."
# (The all-objects color line -- Painter's Servant -- is
# ``roto_layers.parse_all_objects_color_line``.)
_HEAL_REPLACEMENT = re.compile(
    r"^If damage would be dealt to this creature, instead that damage is "
    r"dealt, but all other damage already dealt to (?:him|her|it|them) is "
    r"healed\.$"
)
# "When <this creature enters> and whenever <it does X>, <effect>."
_ENTERS_AND_WHENEVER = re.compile(
    r"^When (?P<first>this creature enters) and whenever (?P<second>[^,]+), "
    r"(?P<eff>.+)\.$"
)


# ---------------------------------------------------------------------------
# Dynamic P/T lines
# ---------------------------------------------------------------------------

_LHURGOYF_PT = re.compile(
    r"^.+?'s power is equal to the number of card types among cards in all "
    r"graveyards and its toughness is equal to that number plus (?P<k>\d+)\.$"
)
_DOMAIN_PT = re.compile(
    r"^(?:Domain — )?.+?'s power and toughness are each equal to the number "
    r"of basic land types among lands you control\.$"
)


def parse_rmc_dynamic_pt_line(line: str):
    """``dynamic_pt_formula`` (a ``PTFormula``) for a parameterized
    characteristic-defining P/T line (any card name), or ``None``."""
    if _DISABLED[0]:
        return None
    from mtgz_ir.effects.pt_formulas import CardTypesInAllGraveyardsPT, DomainPT

    m = _LHURGOYF_PT.match(line)
    if m is not None:
        return CardTypesInAllGraveyardsPT(toughness_plus=int(m.group("k")))
    if _DOMAIN_PT.match(line):
        return DomainPT()
    return None


__all__ = [
    "RmcLine",
    "parse_rmc_cost_piece",
    "parse_rmc_creature_line_post",
    "parse_rmc_creature_line_pre",
    "parse_rmc_effect",
]
