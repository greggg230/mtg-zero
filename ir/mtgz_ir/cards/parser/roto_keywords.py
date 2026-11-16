"""Roto package P6 grammar: keyword mechanics, face-changing & pregame.

Line recognizers for the Vintage Roto cards P6 claims
(``docs/ROTO_COVERAGE_PLAN.md`` section 6). Recognizers are
parameterized over the printed shape (the two literals say why); the
engine side lives in ``mtgz_ir.conspiracies``, ``mtgz_ir.opening_hand``
and ``mtgz_ir.effects.generic.roto_keywords`` / ``roto_damage``.

Entry points, each called from one hook:

* :func:`parse_conspiracy_rule_line` -- ``conspiracy_builder`` (CR 313):
  "You are the starting player. ..." (Power Play), "Your minimum deck
  size is reduced|increased by <N>." (Advantageous Proclamation), "At
  the beginning of each game, after mulligans, your opponent reveals
  their hand to you." (Meddling Wizard, a custom roto conspiracy).
* :func:`build_with_p6_lines` -- ``source.build_card_record`` and
  ``dfc_builder.build_face``: printing-level lines peeled off before the
  per-type builders run (like the cycling family) and attached to the
  built printing -- pregame actions (``opening_hand_action``: Chancellor
  of the Tangle's opening-hand reveal, Serum Powder's mulligan-window
  exchange), hand keyword abilities (Transmute, "Cycling—<non-mana cost>."),
  prevention / replacement statics ("Prevent all damage that would be
  dealt to <self>.", Vizier of Remedies' "-1/-1 counters minus one"),
  Plot on a permanent card, Toxic N, Split second, Echo and Start your
  engines!.
* :func:`parse_hand_keyword_line` -- also ``land_lines`` (Tolaria West).
* :func:`parse_p6_effect` -- last in ``effects._parse_effect``.
* :func:`parse_p6_trigger_phrase` -- last in
  ``triggers._parse_predicate_trigger_phrase``.
"""

from __future__ import annotations

import re
from dataclasses import replace
from typing import Callable

from mtgz_ir.conspiracies import (
    MinimumDeckSizeChange,
    OpponentRevealsHandAfterMulligans,
    YouAreTheStartingPlayer,
)
from mtgz_ir.opening_hand import ExchangeHandWhileMulliganing, RevealFromOpeningHand

_NUMBER_WORDS = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def number_word(text: str) -> int | None:
    """``"five"`` / ``"5"`` -> 5; ``None`` for anything else."""
    if text.isdigit():
        return int(text)
    return _NUMBER_WORDS.get(text.lower())


# ── Conspiracy rules (CR 313) ──────────────────────────────────────────

_STARTING_PLAYER_LINE = (
    "You are the starting player. If multiple players would be the "
    "starting player, one of those players is chosen at random."
)
_MINIMUM_DECK_SIZE_RE = re.compile(
    r"^Your minimum deck size is (?P<dir>reduced|increased) by "
    r"(?P<n>\w+)\.$"
)
# Literal: Meddling Wizard (the roto group's custom conspiracy, record in
# data/cube_roto_custom.json) is the only card with an after-mulligans
# hand reveal, and the sentence has no variable part; a broader grammar
# (who reveals, what, when) is deferred until a second example exists
# (parser CLAUDE.md single-card exception).
_REVEAL_HAND_AFTER_MULLIGANS_LINE = (
    "At the beginning of each game, after mulligans, your opponent "
    "reveals their hand to you."
)


def parse_conspiracy_rule_line(line: str):
    """The conspiracy rule a (reminder-stripped) line states, or
    ``None``. The deck-size line is parameterized over direction and
    amount. The starting-player sentence pair is a literal: Power Play
    is the only card in the pool with it and the text has no variable
    part; a broader grammar is deferred until a second example exists
    (parser CLAUDE.md single-card exception)."""
    line = line.strip()
    if line == _STARTING_PLAYER_LINE:
        return YouAreTheStartingPlayer()
    if line == _REVEAL_HAND_AFTER_MULLIGANS_LINE:
        return OpponentRevealsHandAfterMulligans()
    m = _MINIMUM_DECK_SIZE_RE.match(line)
    if m is not None:
        n = number_word(m.group("n"))
        if n is None:
            return None
        return MinimumDeckSizeChange(-n if m.group("dir") == "reduced" else n)
    return None


# ── Pregame lines (CR 103.5 mulligan window, CR 103.6) ─────────────────

_REVEAL_FROM_OPENING_HAND_RE = re.compile(
    r"^You may reveal this card from your opening hand\. If you do, at "
    r"the beginning of your first main phase of the game, (?P<effect>.+)\.$"
)
# Literal: Serum Powder is the only card in the pool with a mulligan-
# window action, and the sentence has no variable part; a broader
# grammar is deferred until a second example exists.
_EXCHANGE_HAND_WHILE_MULLIGANING_LINE = (
    "Any time you could mulligan and this card is in your hand, you may "
    "exile all the cards from your hand, then draw that many cards."
)


def parse_pregame_line(line: str, card_name: str):
    """The pregame action a reminder-stripped line states, or ``None``."""
    line = line.strip()
    if line == _EXCHANGE_HAND_WHILE_MULLIGANING_LINE:
        return ExchangeHandWhileMulliganing()
    m = _REVEAL_FROM_OPENING_HAND_RE.match(line)
    if m is not None:
        from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase

        effect = _parse_spell_effect_phrase(m.group("effect") + ".", card_name)
        if effect is None or effect.requires_target():
            return None
        return RevealFromOpeningHand(effect)
    return None


# ── Effect phrases (hooked last into ``effects._parse_effect``) ─────────

_CARD_TYPE_WORDS = {
    "artifact": "Artifact", "creature": "Creature", "enchantment": "Enchantment",
    "instant": "Instant", "land": "Land", "planeswalker": "Planeswalker",
    "sorcery": "Sorcery",
}
_LOOK_MAY_REVEAL_RE = re.compile(
    r"^look at the top card of your library\. you may reveal that card\. "
    r"if an? (?P<types>[a-z]+(?: or [a-z]+)*) card is revealed this way, "
    r"(?P<then>.+)$",
    re.IGNORECASE,
)


def _card_types_phrase(text: str) -> tuple[str, ...] | None:
    """``"instant or sorcery"`` -> ``("Instant", "Sorcery")``."""
    words = text.lower().split(" or ")
    if not all(w in _CARD_TYPE_WORDS for w in words):
        return None
    return tuple(_CARD_TYPE_WORDS[w] for w in words)


_PREVENT_COMBAT_TO_AND_BY_TAIL = (
    r"prevent all combat damage that would be dealt to and dealt by "
)
_UNTAP_THEN_PREVENT_COMBAT_RE = re.compile(
    r"^untap target (?P<np>[a-z ]+?)\. " + _PREVENT_COMBAT_TO_AND_BY_TAIL
    + r"that (?:creature|permanent) this turn$",
    re.IGNORECASE,
)
_PREVENT_COMBAT_TO_AND_BY_TARGET_RE = re.compile(
    r"^" + _PREVENT_COMBAT_TO_AND_BY_TAIL + r"target (?P<np>[a-z ]+?) this turn$",
    re.IGNORECASE,
)
_REDIRECT_NEXT_DAMAGE_RE = re.compile(
    r"^the next (?P<n>\w+) damage that would be dealt to this creature this "
    r"turn is dealt to target (?P<np>[a-z ]+?) instead$",
    re.IGNORECASE,
)
# "<it|this creature> deals N damage to target <noun>" whose noun only
# ``target_noun_filter`` reads (a combat qualifier -- Tinder Wall's
# "target creature it's blocking"); the plain nouns are claimed earlier
# in ``_parse_effect`` by the base damage recognizers, so this is the
# fallback for the qualified ones.
_DEAL_N_DAMAGE_TO_TARGET_NOUN_RE = re.compile(
    r"^(?:it|this creature) deals (?P<n>\w+) damage to target "
    r"(?P<np>[a-z' ]+?)$",
    re.IGNORECASE,
)


def target_noun_filter(np: str):
    """``parse_permanent_phrase`` plus the combat qualifiers: a leading
    "attacking" ("target attacking creature") or a trailing "it's
    blocking" ("target creature it's blocking" -- Tinder Wall; the
    source's block, ``BlockedBySource``), else ``None``."""
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.object_predicates import BlockedBySource, IsAttacking

    np = np.strip().lower()
    attacking = np.startswith("attacking ")
    if attacking:
        np = np[len("attacking "):]
    blocked_by_source = np.endswith(" it's blocking")
    if blocked_by_source:
        np = np[: -len(" it's blocking")]
    base = parse_permanent_phrase(np)
    if base is None:
        return None
    if attacking:
        base = base & IsAttacking()
    return base & BlockedBySource() if blocked_by_source else base


# "Regenerate this creature." (Blight Mamba) is P2's ``roto_zones``
# recognizer / ``RegenerateSource`` (Golgari Grave-Troll), tried earlier in
# the ``_parse_effect`` chain; P6's duplicate was removed when the packages
# were integrated.
_COUNTERS_ON_EACH_CREATURE_TARGET_PLAYER_CONTROLS_RE = re.compile(
    r"^put (?P<n>a|an|\w+) \+1/\+1 counters? on each creature target player "
    r"controls$",
    re.IGNORECASE,
)


_DAMAGE_THAT_PLAYER_HAND_SIZE_RE = re.compile(
    r"^(?:this creature |it )?deals damage to that player equal to the "
    r"number of cards in their hand$",
    re.IGNORECASE,
)
_SACRIFICE_OR_DISCARD_RE = re.compile(
    r"^each opponent sacrifices an? (?P<np>[A-Za-z ]+?) of their choice\. "
    r"each opponent who can't discards a card$",
    re.IGNORECASE,
)


def _noun_or_subtype_filter(np: str):
    """"creature or Vehicle": each arm a permanent noun phrase or a
    capitalized subtype."""
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.object_predicates import HasSubtype

    result = None
    for item in np.split(" or "):
        item = item.strip()
        f = parse_permanent_phrase(item.lower()) if item[:1].islower() else None
        if f is None and item[:1].isupper() and item.isalpha():
            f = HasSubtype(item)
        if f is None:
            return None
        result = f if result is None else result | f
    return result


_TARGET_OPPONENT_GETS_POISON_RE = re.compile(
    r"^target opponent gets (?P<n>a|an|\w+) poison counters?$", re.IGNORECASE,
)


_LOSES_THE_GAME_RE = re.compile(
    r"^(?:they lose|that player loses) the game$", re.IGNORECASE,
)


_PLAYER_RULE_PHRASES = {
    "you can't lose the game this turn": "cant_lose_the_game",
    "your opponents can't win the game this turn": "opponents_cant_win",
    (
        "until end of turn, damage that would reduce your life total to "
        "less than 1 reduces it to 1 instead"
    ): "damage_life_floor_one",
}


def _parse_player_rule_phrase(phrase: str):
    """Angel's Grace's player rules -- each clause, and "<clause> and
    <clause>" joining two of them."""
    from mtgz_ir.effects.combinators import Sequence
    from mtgz_ir.effects.generic.roto_keywords import PlayerRuleUntilEndOfTurn

    p = phrase.strip().lower()
    if p in _PLAYER_RULE_PHRASES:
        return PlayerRuleUntilEndOfTurn(kind=_PLAYER_RULE_PHRASES[p])
    left, sep, right = p.partition(" and ")
    if sep and left in _PLAYER_RULE_PHRASES and right in _PLAYER_RULE_PHRASES:
        return Sequence(effects=(
            PlayerRuleUntilEndOfTurn(kind=_PLAYER_RULE_PHRASES[left]),
            PlayerRuleUntilEndOfTurn(kind=_PLAYER_RULE_PHRASES[right]),
        ))
    return None


def _parse_keyword_action_phrase(phrase: str):
    from mtgz_ir.effects.generic.roto_keywords import (
        PutPlusOneCountersOnEachCreatureTargetPlayerControls,
        TargetOpponentGetsPoisonCounters,
    )
    from mtgz_ir.effects.object_predicates import ControlledByOpponent, IsPlayer
    from mtgz_ir.effects.targets import TargetSpec

    m = _SACRIFICE_OR_DISCARD_RE.match(phrase.strip())
    if m is not None:
        from mtgz_ir.effects.generic.roto_keywords import (
            EachOpponentSacrificesOrDiscards,
        )

        f = _noun_or_subtype_filter(m.group("np"))
        if f is None:
            return None
        return EachOpponentSacrificesOrDiscards(permanent_filter=f, label=m.group("np"))
    if phrase.strip().lower() == "you gain life equal to your speed":
        from mtgz_ir.effects.generic.roto_keywords import GainLifeEqualToYourSpeed

        return GainLifeEqualToYourSpeed()
    if phrase.strip().lower() == "exile all graveyards":
        from mtgz_ir.effects.generic.exile import ExileAllGraveyards

        return ExileAllGraveyards()
    if _LOSES_THE_GAME_RE.match(phrase):
        # "they" / "that player": the triggering event's player.
        from mtgz_ir.effects.generic.cube_creatures_ub import BindEventPlayer
        from mtgz_ir.effects.generic.roto_keywords import TargetPlayerLosesTheGame

        return BindEventPlayer(TargetPlayerLosesTheGame())
    if _DAMAGE_THAT_PLAYER_HAND_SIZE_RE.match(phrase):
        # "that player" is the triggering event's player.
        from mtgz_ir.effects.generic.cube_creatures_ub import BindEventPlayer
        from mtgz_ir.effects.generic.roto_keywords import (
            DealDamageToPlayerEqualToTheirHandSize,
        )

        return BindEventPlayer(DealDamageToPlayerEqualToTheirHandSize())
    m = _TARGET_OPPONENT_GETS_POISON_RE.match(phrase)
    if m is not None:
        n = number_word(m.group("n"))
        if n is None:
            return None
        return TargetOpponentGetsPoisonCounters(
            amount=n, targets=(TargetSpec(IsPlayer() & ControlledByOpponent()),),
        )
    m = _COUNTERS_ON_EACH_CREATURE_TARGET_PLAYER_CONTROLS_RE.match(phrase)
    if m is not None:
        n = number_word(m.group("n"))
        if n is None:
            return None
        return PutPlusOneCountersOnEachCreatureTargetPlayerControls(
            amount=n, targets=(TargetSpec(IsPlayer()),),
        )
    return None


_CANT_BE_PREVENTED_THEN_DAMAGE_RE = re.compile(
    r"^damage can't be prevented this turn\. (?P<src>[^.]+?) deals "
    r"(?P<n>\d+) damage to (?P<count>up to one target|target) "
    r"(?P<np>[a-z ]+?)$",
    re.IGNORECASE,
)


def _parse_cant_be_prevented_phrase(phrase: str):
    """"Damage can't be prevented this turn[. <source> deals N damage to
    [up to one] target <np>]" (Impractical Joke). The two-sentence form
    is recognized whole because the damage sentence alone is claimed
    first by the base grammar, which reads "creature or planeswalker"
    as creature-only (``cards/parser/effects.py``, a documented
    EoE-parity reading); here the target noun phrase is composed
    faithfully by ``target_noun_filter``."""
    from mtgz_ir.effects.combinators import Sequence
    from mtgz_ir.effects.generic.damage import DealDamage
    from mtgz_ir.effects.generic.roto_damage import DamageCantBePreventedThisTurn
    from mtgz_ir.effects.targets import TargetSpec

    if phrase.strip().lower() == "damage can't be prevented this turn":
        return DamageCantBePreventedThisTurn()
    m = _CANT_BE_PREVENTED_THEN_DAMAGE_RE.match(phrase.strip())
    if m is None:
        return None
    f = target_noun_filter(m.group("np"))
    if f is None:
        return None
    count_min = 0 if m.group("count").lower().startswith("up to") else 1
    return Sequence(effects=(
        DamageCantBePreventedThisTurn(),
        DealDamage(
            amount=int(m.group("n")),
            targets=(TargetSpec(f, count_min, 1),),
        ),
    ))


def _parse_damage_phrase(phrase: str):
    from mtgz_ir.effects.generic.roto_damage import (
        PreventCombatDamageToAndByTarget,
        RedirectNextDamageToSourceToTarget,
    )
    from mtgz_ir.effects.targets import TargetSpec

    for regex, untap in (
        (_UNTAP_THEN_PREVENT_COMBAT_RE, True),
        (_PREVENT_COMBAT_TO_AND_BY_TARGET_RE, False),
    ):
        m = regex.match(phrase)
        if m is not None:
            f = target_noun_filter(m.group("np"))
            if f is None:
                return None
            return PreventCombatDamageToAndByTarget(
                targets=(TargetSpec(f),), untap=untap,
            )
    m = _REDIRECT_NEXT_DAMAGE_RE.match(phrase)
    if m is not None:
        n = number_word(m.group("n"))
        f = target_noun_filter(m.group("np"))
        if n is None or f is None:
            return None
        return RedirectNextDamageToSourceToTarget(amount=n, targets=(TargetSpec(f),))
    m = _DEAL_N_DAMAGE_TO_TARGET_NOUN_RE.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.damage import DealDamage

        n = number_word(m.group("n"))
        f = target_noun_filter(m.group("np"))
        if n is None or f is None:
            return None
        return DealDamage(amount=n, targets=(TargetSpec(f),))
    return None


_UNTAP_THEN_GRANT_RE = re.compile(
    r'^untap target (?P<np>[a-z ]+?)\. until end of turn, it gains '
    r'(?P<kws>[a-z]+(?:[a-z ,]*[a-z])?)(?:,? and "(?P<quoted>[^"]+)")?\.?$',
    re.IGNORECASE,
)
_TAPPED_BITE_TRIGGER_RE = re.compile(
    r"^Whenever this creature becomes tapped, it deals damage equal to its "
    r"power to (?P<count>up to one target|target) (?P<np>[a-z ]+?)\.?$"
)


def _keyword_list(text: str) -> frozenset | None:
    """``"reach, hexproof"`` / ``"reach and hexproof"`` -> engine
    keywords, every piece a known evergreen keyword."""
    from mtgz_ir.cards.parser.keywords import _KEYWORD_LINES

    known = {v for v in _KEYWORD_LINES.values()}
    pieces = [p.strip() for p in re.split(r",\s*(?:and\s+)?|\s+and\s+", text) if p.strip()]
    if not pieces or any(p.lower() not in known for p in pieces):
        return None
    return frozenset(p.lower() for p in pieces)


def _parse_tapped_bite_trigger(text: str):
    """The quoted "Whenever this creature becomes tapped, it deals damage
    equal to its power to up to one target <np>." -- "it" is the
    creature itself (the trigger's subject)."""
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.roto_keywords import (
        SourceDealsDamageEqualToItsPowerToTarget,
    )
    from mtgz_ir.effects.predicates import Event, IsSelf
    from mtgz_ir.effects.targets import TargetSpec

    m = _TAPPED_BITE_TRIGGER_RE.match(text.strip())
    if m is None:
        return None
    f = target_noun_filter(m.group("np"))
    if f is None:
        return None
    count_min = 0 if m.group("count") == "up to one target" else 1
    return TriggeredAbility(
        event=Event.BECOMES_TAPPED, predicate=IsSelf(),
        effect=SourceDealsDamageEqualToItsPowerToTarget(
            targets=(TargetSpec(f, count_min, 1),),
        ),
    )


def _parse_untap_then_grant(phrase: str):
    from mtgz_ir.effects.generic.roto_keywords import UntapTargetThenGrantUntilEndOfTurn
    from mtgz_ir.effects.targets import TargetSpec

    m = _UNTAP_THEN_GRANT_RE.match(phrase)
    if m is None:
        return None
    f = target_noun_filter(m.group("np"))
    kws = _keyword_list(m.group("kws"))
    if f is None or kws is None:
        return None
    trigger = None
    if m.group("quoted") is not None:
        trigger = _parse_tapped_bite_trigger(m.group("quoted"))
        if trigger is None:
            return None
    return UntapTargetThenGrantUntilEndOfTurn(
        targets=(TargetSpec(f),), keywords=kws, granted_trigger=trigger,
    )


def _parse_flip_and_counter_phrase(phrase: str):
    """"flip this creature" -- only while ``flip_builder`` builds a flip
    card's top half (it knows the flipped half); "counter that spell" --
    the triggering cast's spell."""
    from mtgz_ir.effects.generic.roto_keywords import CounterThatSpell, FlipSource

    p = phrase.strip().lower()
    if p in ("flip this creature", "flip this permanent"):
        from mtgz_ir.cards.parser.flip_builder import current_flipped_face

        flipped = current_flipped_face()
        return None if flipped is None else FlipSource(flipped=flipped)
    if p == "counter that spell":
        return CounterThatSpell()
    return None


def parse_p6_effect(phrase: str):
    """A normalized effect phrase (lowercased first letter, no final
    period) the P6 grammar models, else ``None``."""
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic.card_flow import CardFilter
    from mtgz_ir.effects.generic.roto_keywords import LookAtTopMayRevealIfMatches

    m = _LOOK_MAY_REVEAL_RE.match(phrase)
    if m is not None:
        types = _card_types_phrase(m.group("types"))
        then = _parse_effect(m.group("then"))
        if types is None or then is None or then.requires_target():
            return None
        return LookAtTopMayRevealIfMatches(
            card_filter=CardFilter(any_types=types), then=then,
        )
    return (
        _parse_damage_phrase(phrase)
        or _parse_cant_be_prevented_phrase(phrase)
        or _parse_keyword_action_phrase(phrase)
        or _parse_untap_then_grant(phrase)
        or _parse_flip_and_counter_phrase(phrase)
        or _parse_player_rule_phrase(phrase)
    )


# ── Trigger conditions (hooked last into ``triggers``) ─────────────────


_ORDINALS = {
    "first": 1, "second": 2, "third": 3, "fourth": 4, "fifth": 5,
    "sixth": 6, "seventh": 7, "eighth": 8, "ninth": 9, "tenth": 10,
}
_NTH_SPELL_OF_A_TURN_RE = re.compile(
    r"^the (?P<nth>[a-z]+) spell of a turn is cast$"
)
_CASTS_NTH_SPELL_EACH_TURN_RE = re.compile(
    r"^(?P<who>an opponent|a player) casts their (?P<nth>[a-z]+) spell "
    r"each turn$"
)
_BECOMES_TARGET_RE = re.compile(
    r"^(?P<who>this creature|a player or permanent|a creature you control) "
    r"becomes the target of (?P<what>a spell or ability|a spell|an ability)"
    r"(?: (?P<ctl>you control|an opponent controls))?$"
)




def parse_p6_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for a trigger condition (the text between
    "Whenever" and the comma) the P6 grammar models, else ``None``.

    "<who> becomes the target of <a spell or ability | a spell | an
    ability>[ you control | an opponent controls]" -> ``BECOMES_TARGET``:
    who = this creature (Cephalid Illusionist) / a player or permanent
    (Loki, God of Mischief) / a creature you control (Venerated
    Rotpriest); the spell-vs-ability and controller qualifiers read the
    event's ``targeting`` / ``controller`` subjects.

    "an opponent attacks you" -> the opponent's ``YOU_ATTACK``
    (Emberwilde Captain). "the <nth> spell of a turn is cast" and "<an
    opponent | a player> casts their <nth> spell each turn" -> ``IS_CAST``
    with a turn-wide / per-caster count (Erayo).
    """
    from mtgz_ir.effects.generic.roto_keywords import (
        TargetIsPlayerOrPermanent,
        TargetingIs,
    )
    from mtgz_ir.effects.object_predicates import IsCreature
    from mtgz_ir.effects.predicates import (
        OPPONENT, YOU, Controlled, Event, IsSelf, Subject,
    )

    m = _NTH_SPELL_OF_A_TURN_RE.match(phrase)
    if m is not None and m.group("nth") in _ORDINALS:
        # Erayo, Soratami Ascendant: counts every player's spells.
        from mtgz_ir.effects.generic.roto_keywords import NthSpellOfTurn

        return (Event.IS_CAST, NthSpellOfTurn(_ORDINALS[m.group("nth")]))
    m = _CASTS_NTH_SPELL_EACH_TURN_RE.match(phrase)
    if m is not None and m.group("nth") in _ORDINALS:
        # Erayo's Essence: the caster's own count.
        from mtgz_ir.effects.generic.roto_keywords import CastersNthSpellThisTurn

        pred = CastersNthSpellThisTurn(_ORDINALS[m.group("nth")])
        if m.group("who") == "an opponent":
            pred = Controlled(OPPONENT, subject="controller") & pred
        return (Event.IS_CAST, pred)
    if phrase == "this creature deals damage to a player":
        return (Event.DEALS_DAMAGE_TO_PLAYER, IsSelf(subject="source"))
    if phrase == "an opponent attacks you":
        # The engine's attackers always attack the defending player
        # (no planeswalker / battle attacks), so an opponent's attack is
        # an attack on you: their ``YOU_ATTACK`` event. ("while you're
        # the monarch" is a ``triggers`` while-gate.)
        return (Event.YOU_ATTACK, Controlled(OPPONENT, subject="player"))
    m = _BECOMES_TARGET_RE.match(phrase)
    if m is None:
        return None
    who = m.group("who")
    if who == "this creature":
        pred = IsSelf(subject="target")
    elif who == "a player or permanent":
        pred = TargetIsPlayerOrPermanent()
    else:
        pred = Subject("target", IsCreature()) & Controlled(YOU, subject="target")
    what = m.group("what")
    if what == "a spell":
        pred = pred & TargetingIs("spell")
    elif what == "an ability":
        pred = pred & TargetingIs("ability")
    ctl = m.group("ctl")
    if ctl == "you control":
        pred = pred & Controlled(YOU, subject="controller")
    elif ctl == "an opponent controls":
        pred = pred & Controlled(OPPONENT, subject="controller")
    return (Event.BECOMES_TARGET, pred)


_DEALT_DAMAGE_TO_THAT_PLAYER_RE = re.compile(
    r"^it has dealt (?P<n>\d+) or more damage to that player this turn$"
)


def parse_p6_intervening_if(phrase: str):
    """An intervening-"if" gate (CR 603.4) the P6 grammar models, else
    ``None``: "it has dealt N or more damage to that player this turn"
    (Vessel of the All-Consuming)."""
    from mtgz_ir.effects.generic.roto_keywords import (
        DealtDamageToThatPlayerThisTurnAtLeast,
    )

    m = _DEALT_DAMAGE_TO_THAT_PLAYER_RE.match(phrase)
    if m is not None:
        return DealtDamageToThatPlayerThisTurnAtLeast(int(m.group("n")))
    return None


# ── Hand keyword lines (abilities that work from the hand) ─────────────

_TRANSMUTE_RE = re.compile(r"^Transmute (?P<cost>(?:\{[^}]+\})+)$")
# Cycling with a non-mana cost (CR 702.29a: "Cycling [cost]" means "[Cost],
# Discard this card: Draw a card."), written with an em dash: "Cycling—Pay
# N life." (Street Wraith), "Cycling—Sacrifice a land." (Edge of Autumn).
# The cost is a comma-separated list of pieces, each an
# ``ActivationCost.extra_costs`` atom (the atoms the hand-activation path
# pays): "Pay N life" -> ``PayLifeCost``; every other piece goes through
# the composable cost-atom grammar (``creature_rmc_grammar.
# parse_rmc_cost_piece``: "Sacrifice a land" -> ``SacrificePermanentsCost``).
_CYCLING_NONMANA_COST_RE = re.compile(r"^Cycling—(?P<cost>[^.{}]+)\.?$")
_PAY_LIFE_PIECE_RE = re.compile(r"^Pay (?P<n>\w+) life$")


def _nonmana_cycling_cost_atoms(cost: str) -> tuple | None:
    """The ``extra_costs`` atoms of a non-mana cycling cost, or ``None``
    when a piece is outside the grammar."""
    from mtgz_ir.cards.parser.creature_rmc_grammar import parse_rmc_cost_piece
    from mtgz_ir.effects.generic.roto_keywords import PayLifeCost

    atoms = []
    for piece in (p.strip() for p in cost.split(",")):
        m = _PAY_LIFE_PIECE_RE.match(piece)
        if m is not None:
            n = number_word(m.group("n"))
            if n is None:
                return None
            atoms.append(PayLifeCost(n))
            continue
        atom = parse_rmc_cost_piece(piece)
        if atom is None:
            return None
        atoms.append(atom)
    return tuple(atoms) or None


def parse_hand_keyword_line(line: str, card: dict):
    """The hand-activated ``ActivatedAbility`` a keyword line states, or
    ``None``.

    Cycling—<cost>. (a non-mana cost: Street Wraith's "Pay 2 life", Edge
    of Autumn's "Sacrifice a land"; mana-cost cycling is parsed by
    ``printing_keywords._parse_cycling_line``.)

    Transmute {cost} (CR 702.53): "{cost}, Discard this card: Search
    your library for a card with the same mana value as the discarded
    card, reveal it, put it into your hand, then shuffle. Transmute only
    as a sorcery." The discarded card is this card, so its mana value is
    the printed one."""
    from mtgz_ir.cards.base import ActivatedAbility, ActivationCost, parse_mana_cost
    from mtgz_ir.effects.generic.card_flow import SearchLibrary
    from mtgz_ir.effects.generic.roto_keywords import ManaValueCardFilter

    m = _CYCLING_NONMANA_COST_RE.match(line.strip())
    if m is not None:
        # Cycling with a non-mana cost (CR 702.29a): "<cost>, Discard this
        # card: Draw a card."
        from mtgz_ir.effects.generic.draw_mill import DrawCards

        atoms = _nonmana_cycling_cost_atoms(m.group("cost"))
        if atoms is None:
            return None
        return ActivatedAbility(
            cost=ActivationCost(discard_self=True, extra_costs=atoms),
            effect=DrawCards(amount=1),
        )
    m = _TRANSMUTE_RE.match(line.strip())
    if m is not None:
        cost_str = m.group("cost")
        mana_value = parse_mana_cost(card.get("mana_cost") or "").total
        return ActivatedAbility(
            cost=ActivationCost(
                mana=parse_mana_cost(cost_str),
                mana_str=cost_str,
                discard_self=True,
            ),
            effect=SearchLibrary(
                card_filter=ManaValueCardFilter(mana_value=mana_value),
                destination="hand",
                reveal=True,
            ),
            sorcery_speed=True,
        )
    return None


# ── Printed replacement / prevention statics ───────────────────────────

_PREVENT_ALL_DAMAGE_TO_SELF_RE = re.compile(
    r"^Prevent all damage that would be dealt to (?P<self>.+)\.$"
)
_FEWER_MINUS_ONE_COUNTERS_RE = re.compile(
    r"^If one or more -1/-1 counters would be put on a creature you "
    r"control, that many -1/-1 counters minus (?P<n>\w+) are put on it "
    r"instead\.$"
)


def is_self_reference(text: str, card_name: str) -> bool:
    """``text`` names the card itself: "this creature", its full name or
    a legendary short name ("Black Panther" for "Black Panther, Hope
    Enduring")."""
    return text in (
        "this creature", "this permanent", card_name,
        card_name.split(", ")[0],
    )


def parse_replacement_static_line(line: str, card_name: str):
    """A printed prevention / replacement static, or ``None``."""
    from mtgz_ir.effects.generic.roto_damage import PreventAllDamageToSelf
    from mtgz_ir.effects.generic.roto_keywords import (
        FewerMinusOneCountersOnYourCreatures,
    )

    m = _PREVENT_ALL_DAMAGE_TO_SELF_RE.match(line.strip())
    if m is not None and is_self_reference(m.group("self"), card_name):
        return PreventAllDamageToSelf()
    m = _FEWER_MINUS_ONE_COUNTERS_RE.match(line.strip())
    if m is not None:
        n = number_word(m.group("n"))
        return None if n is None else FewerMinusOneCountersOnYourCreatures(n)
    return None


# ── The printing-line pre-pass ─────────────────────────────────────────

def _attach_opening_hand_action(printing, action):
    if printing.opening_hand_action is not None:
        return None  # one opening-hand slot: two such lines aren't claimed
    return replace(printing, opening_hand_action=action)


def _attach_hand_ability(printing, ability):
    return replace(
        printing,
        hand_activated_abilities=(
            tuple(printing.hand_activated_abilities) + (ability,)
        ),
    )


_PLOT_RE = re.compile(r"^Plot (?P<cost>(?:\{[^}]+\})+)$")
_SELF_TRIGGER_COUNTERS_ON_IT_RE = re.compile(
    r"^Whenever (?P<cond>this creature [^,]+), put (?P<n>a|an|\w+) \+1/\+1 "
    r"counters? on it\.$"
)


def _self_trigger_counters_on_it(line: str):
    """"Whenever this creature <condition>, put N +1/+1 counter(s) on
    it." -- "it" is the condition's subject, this creature (Vessel of the
    All-Consuming). Recognized whole: "on it" alone has no referent."""
    from mtgz_ir.cards.parser.triggers import _parse_predicate_trigger_phrase
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.counters import PutPlusOneCounterOnSelf

    m = _SELF_TRIGGER_COUNTERS_ON_IT_RE.match(line.strip())
    if m is None:
        return None
    n = number_word(m.group("n"))
    cond = _parse_predicate_trigger_phrase(m.group("cond"))
    if n is None or cond is None:
        return None
    return TriggeredAbility(
        event=cond[0], predicate=cond[1], effect=PutPlusOneCounterOnSelf(amount=n),
    )
_TOXIC_RE = re.compile(r"^Toxic (?P<n>\d+)$")
_SPLIT_SECOND_LINE = "Split second"
_START_YOUR_ENGINES_LINE = "Start your engines!"
_ECHO_RE = re.compile(r"^Echo (?P<cost>(?:\{[^}]+\})+)$")


def _attach_trigger(printing, trigger):
    return replace(
        printing,
        triggered_abilities=tuple(printing.triggered_abilities) + (trigger,),
    )


def _echo_trigger(cost: str):
    """Echo {cost} (CR 702.30a): "At the beginning of your upkeep, if this
    permanent came under your control since the beginning of your last
    upkeep, sacrifice it unless you pay {cost}." """
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.roto_keywords import EchoDue, EchoUpkeep
    from mtgz_ir.effects.predicates import YOU, Controlled, Event

    return TriggeredAbility(
        event=Event.BEGINNING_OF_UPKEEP,
        predicate=Controlled(YOU) & EchoDue(),
        effect=EchoUpkeep(cost_str=cost),
    )


def _attach_keyword(printing, keyword: str):
    return replace(printing, keywords=frozenset(printing.keywords) | {keyword})


def _attach_plot(printing, cost: str):
    from mtgz_ir.cards.base import parse_mana_cost

    if printing.plot_cost is not None:
        return None
    return replace(printing, plot_cost=parse_mana_cost(cost), plot_cost_str=cost)


def _attach_replacement_static(printing, rule):
    return replace(
        printing,
        replacement_statics=tuple(printing.replacement_statics) + (rule,),
    )


def _line_patch(line: str, card: dict):
    """A ``CardPrinting -> CardPrinting | None`` patch for a P6
    printing-level line, or ``None`` when the line isn't one."""
    action = parse_pregame_line(line, card["name"])
    if action is not None:
        return lambda p: _attach_opening_hand_action(p, action)
    ability = parse_hand_keyword_line(line, card)
    if ability is not None:
        return lambda p: _attach_hand_ability(p, ability)
    rule = parse_replacement_static_line(line, card["name"])
    if rule is not None:
        return lambda p: _attach_replacement_static(p, rule)
    trigger = _self_trigger_counters_on_it(line)
    if trigger is not None:
        return lambda p: _attach_trigger(p, trigger)
    m = _PLOT_RE.match(line.strip())
    type_line = card.get("type_line", "")
    if m is not None and "Instant" not in type_line and "Sorcery" not in type_line:
        # Plot (CR 702.170) on a permanent card; the spell builder owns
        # instants' and sorceries' Plot lines.
        cost = m.group("cost")
        return lambda p: _attach_plot(p, cost)
    m = _ECHO_RE.match(line.strip())
    if m is not None and "Instant" not in type_line and "Sorcery" not in type_line:
        trigger = _echo_trigger(m.group("cost"))
        return lambda p: _attach_trigger(p, trigger)
    if line.strip() == _START_YOUR_ENGINES_LINE:
        # CR 702.179: ``roto_keywords.update_speed``.
        return lambda p: _attach_keyword(p, "start your engines!")
    if line.strip() == _SPLIT_SECOND_LINE:
        # Split second (CR 702.61): ``roto_keywords.filter_split_second``.
        return lambda p: _attach_keyword(p, "split second")
    m = _TOXIC_RE.match(line.strip())
    if m is not None:
        # Toxic N (CR 702.164): ``effects.generic.roto_keywords.apply_toxic``.
        keyword = f"toxic {int(m.group('n'))}"
        return lambda p: _attach_keyword(p, keyword)
    return None


def _split_p6_lines(card: dict) -> tuple[dict, list]:
    """``(card without its P6 printing-level lines, [patches])``. Lands
    (``land_lines`` parses their keyword lines itself) and multi-face
    records are left untouched."""
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    text = card.get("oracle_text")
    if (
        not text or card.get("card_faces")
        or "Land" in card.get("type_line", "")
    ):
        return card, []
    kept: list[str] = []
    patches = []
    for raw in text.split("\n"):
        patch = _line_patch(_strip_reminder_text(raw), card)
        if patch is None:
            kept.append(raw)
        else:
            patches.append(patch)
    if not patches:
        return card, []
    stripped = dict(card)
    stripped["oracle_text"] = "\n".join(kept)
    return stripped, patches


_CLEAVE_RE = re.compile(r"^Cleave (?P<cost>(?:\{[^}]+\})+)$")
_BRACKETED_RE = re.compile(r"\s*\[[^\]]*\]")


def _split_cleave(card: dict):
    """``(cost, printed_card, cleaved_card)`` for an instant / sorcery
    printing "Cleave {cost}" (CR 702.148): the printed text keeps the
    bracketed words (brackets dropped); the cleaved text removes them.
    ``None`` for any other card."""
    from mtgz_ir.cards.parser.text import _strip_reminder_text

    type_line = card.get("type_line", "")
    text = card.get("oracle_text") or ""
    if card.get("card_faces") or not (
        "Instant" in type_line or "Sorcery" in type_line
    ):
        return None
    lines = text.split("\n")
    costs = [
        m.group("cost") for m in (
            _CLEAVE_RE.match(_strip_reminder_text(raw)) for raw in lines
        ) if m is not None
    ]
    if len(costs) != 1:
        return None
    body = "\n".join(
        raw for raw in lines if _CLEAVE_RE.match(_strip_reminder_text(raw)) is None
    )
    if "[" not in body:
        return None
    printed = body.replace("[", "").replace("]", "")
    cleaved = _BRACKETED_RE.sub("", body)
    return costs[0], dict(card, oracle_text=printed), dict(card, oracle_text=cleaved)


def _build_cleave(card: dict, build: Callable[[dict], object]):
    """Cleave: an alternative cost (CR 702.148a) whose payment changes
    the spell's text -- the overload pattern:
    ``IfConditionElse(AlternativeCostWasPaid(), <cleaved>, <printed>)``."""
    from mtgz_ir.cards.spell_costs import AlternativeCost
    from mtgz_ir.effects.combinators import IfConditionElse
    from mtgz_ir.effects.generic.cast_conditionals import AlternativeCostWasPaid

    cost, printed_card, cleaved_card = _split_cleave(card)
    printed = build_with_p6_lines(printed_card, build)
    cleaved = build_with_p6_lines(cleaved_card, build)
    if (
        printed is None or cleaved is None
        or printed.spell_effect is None or cleaved.spell_effect is None
    ):
        return None
    return replace(
        printed,
        alternative_costs=tuple(printed.alternative_costs) + (AlternativeCost(mana=cost),),
        spell_effect=IfConditionElse(
            condition=AlternativeCostWasPaid(),
            then_effect=cleaved.spell_effect,
            else_effect=printed.spell_effect,
        ),
    )


def build_with_p6_lines(card: dict, build: Callable[[dict], object]):
    """Build ``card`` with ``build`` after peeling off its P6 printing-
    level lines (pregame actions, hand keyword abilities, prevention /
    replacement statics), then apply
    each line's patch. Any patch refusing (``None``) leaves the card
    unclaimed."""
    if _split_cleave(card) is not None:
        return _build_cleave(card, build)
    body, patches = _split_p6_lines(card)
    built = build(body)
    for patch in patches:
        if built is None:
            return None
        built = patch(built)
    return built


__all__ = [
    "build_with_p6_lines",
    "number_word",
    "parse_conspiracy_rule_line",
    "parse_hand_keyword_line",
    "parse_pregame_line",
]
