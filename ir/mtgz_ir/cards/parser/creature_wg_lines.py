"""Creature oracle-line recognizers for the cube W/G creature bucket.

``creature_builder._build_creature_printing`` consults
:func:`parse_wg_creature_line` only for a line the pre-existing grammar
(printing keywords + ``clause._parse_clause``) declined, so nothing here
can change how an already-claimed card parses.

Each recognizer returns a :class:`WGLine` describing what the line adds
to the printing: ``CardPrinting`` field values, keywords and triggered /
activated / static abilities. Shapes (reminder text already stripped):

* ``Evoke—Exile a <filter> card from your hand.`` / ``Evoke {cost}`` —
  :mod:`mtgz_ir.evoke` (the evoke sacrifice trigger rides along).
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field


@dataclass
class WGLine:
    """What one recognized oracle line contributes to a creature
    printing."""

    fields: dict = field(default_factory=dict)
    keywords: tuple[str, ...] = ()
    triggers: tuple = ()
    activated: tuple = ()
    statics: tuple = ()


# ---------------------------------------------------------------------
# Evoke
# ---------------------------------------------------------------------

_EVOKE_LINE_RE = re.compile(
    r"^Evoke(?: (?P<mana>(?:\{[^}]+\})+)|—Exile (?P<desc>an? .+? card) "
    r"from your hand\.)$"
)


def _parse_evoke_line(line: str, card: dict) -> WGLine | None:
    m = _EVOKE_LINE_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.evoke import EVOKE_SACRIFICE_TRIGGER

    if m.group("mana") is not None:
        from mtgz_ir.cards.base import parse_mana_cost

        fields = {
            "evoke_cost": parse_mana_cost(m.group("mana")),
            "evoke_cost_str": m.group("mana"),
        }
    else:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        fields = {"evoke_exile_from_hand": f}
    return WGLine(fields=fields, triggers=(EVOKE_SACRIFICE_TRIGGER,))


# ---------------------------------------------------------------------
# Ravenous / Mobilize
# ---------------------------------------------------------------------


def _parse_ravenous_line(line: str, card: dict) -> WGLine | None:
    """``Ravenous`` (CR 702.156): enters with X +1/+1 counters; draws a
    card on entering if X is 5 or more."""
    if line != "Ravenous":
        return None
    from mtgz_ir.effects.generic.cube_creatures_wg import ravenous_draw_trigger

    return WGLine(
        fields={"enters_with_counters_from_x": "x"},
        triggers=(ravenous_draw_trigger(),),
    )


_MOBILIZE_RE = re.compile(r"^Mobilize (?P<n>\d+)$")


def _parse_mobilize_line(line: str, card: dict) -> WGLine | None:
    """``Mobilize N`` (CR 702.181): the attack trigger making N tapped and
    attacking 1/1 red Warriors sacrificed at the next end step."""
    m = _MOBILIZE_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.cube_creatures_wg import Mobilize
    from mtgz_ir.effects.predicates import Event, IsSelf

    return WGLine(triggers=(TriggeredAbility(
        event=Event.ATTACKS, predicate=IsSelf(),
        effect=Mobilize(n=int(m.group("n"))),
    ),))


# ---------------------------------------------------------------------
# Batched enters triggers
# ---------------------------------------------------------------------

_ONE_OR_MORE_ENTER_ONCE_PER_TURN_RE = re.compile(
    r"^Whenever one or more (?P<other>other )?(?P<noun>.+?) enter, "
    r"(?P<eff>.+)\. This ability triggers only once each turn\.$"
)


def _parse_one_or_more_enter_once_per_turn(line: str, card: dict) -> WGLine | None:
    """``Whenever one or more [other] <nouns> enter, <effect>. This
    ability triggers only once each turn.`` (Enduring Innocence). With
    the once-each-turn rider the batched "one or more" trigger fires
    exactly when the per-object ENTERS trigger first fires in a turn, so
    it is modeled as that trigger with ``once_per_turn``. (Without the
    rider the batch would need its own dispatch; that form isn't
    claimed.)"""
    m = _ONE_OR_MORE_ENTER_ONCE_PER_TURN_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.creature_wg_effects import trigger_subject
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import Event, IsSelf, Subject

    subject = trigger_subject(m.group("noun"))
    effect = _parse_effect_phrase(m.group("eff"))
    if subject is None or effect is None:
        return None
    pred, controlled = subject
    predicate = Subject(None, pred)
    if controlled is not None:
        predicate = predicate & controlled
    if m.group("other"):
        predicate = predicate & ~IsSelf()
    return WGLine(triggers=(TriggeredAbility(
        event=Event.ENTERS, predicate=predicate, effect=effect,
        once_per_turn=True,
    ),))


# ---------------------------------------------------------------------
# Rules-restricting statics (``mtgz_ir.creature_statics_wg``)
# ---------------------------------------------------------------------

_STATIC_FLAG_LINES: dict[str, str] = {
    "If a nontoken creature would enter and it wasn't cast, exile it "
    "instead.": "exiles_uncast_nontoken_creatures_entering",
    "Your opponents can't cast spells during your turn.": (
        "opponents_cant_cast_spells_during_your_turn"
    ),
    "Each player can't cast more than one spell each turn.": (
        "each_player_one_spell_per_turn"
    ),
    "Nonbasic lands your opponents control enter tapped.": (
        "opponents_nonbasic_lands_enter_tapped"
    ),
    "During your turn, nonland permanent cards in your graveyard have "
    "retrace.": "grants_retrace_to_nonland_permanents_during_your_turn",
}


def _parse_static_flag_line(line: str, card: dict) -> WGLine | None:
    flag = _STATIC_FLAG_LINES.get(line)
    if flag is None:
        return None
    return WGLine(fields={flag: True})


# ---------------------------------------------------------------------
# Cost increases / combat statics / graveyard-arrival triggers
# ---------------------------------------------------------------------

_SPELLS_COST_MORE_RE = re.compile(
    r"^(?P<desc>[A-Z][a-z]+(?:, [a-z]+)*) spells cost \{(?P<n>\d+)\} more to "
    r"cast\.$"
)


def _parse_spells_cost_more_line(line: str, card: dict) -> WGLine | None:
    """``<Filter> spells cost {N} more to cast.`` (Thalia) ->
    ``spell_cost_increases`` (every player's matching spells)."""
    m = _SPELLS_COST_MORE_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.card_flow_grammar import _parse_descriptor_body

    f = _parse_descriptor_body(m.group("desc").lower())
    if f is None:
        return None
    return WGLine(fields={"spell_cost_increases": ((f, int(m.group("n"))),)})


_CANT_BE_BLOCKED_BY_RE = re.compile(
    r"^[Tt]his creature can't be blocked by (?P<noun>creatures with .+)\.$"
)


def _parse_cant_be_blocked_by_line(line: str, card: dict) -> WGLine | None:
    """``<This> can't be blocked by creatures with <qualifier>.``
    (Questing Beast) -> ``cant_be_blocked_by``."""
    m = _CANT_BE_BLOCKED_BY_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.creature_wg_effects import trigger_subject

    subject = trigger_subject(m.group("noun"))
    if subject is None or subject[1] is not None:
        return None
    return WGLine(fields={"cant_be_blocked_by": subject[0]})


_YOUR_CREATURES_DAMAGE_UNPREVENTABLE = (
    "Combat damage that would be dealt by creatures you control can't be "
    "prevented."
)


def _parse_your_creatures_damage_unpreventable_line(
    line: str, card: dict,
) -> WGLine | None:
    if line != _YOUR_CREATURES_DAMAGE_UNPREVENTABLE:
        return None
    return WGLine(fields={
        "your_creatures_combat_damage_cant_be_prevented": True,
    })


# ---------------------------------------------------------------------
# Enters with named / keyword counters
# ---------------------------------------------------------------------

_ENTERS_WITH_COUNTERS_RE = re.compile(
    r"^[Tt]his creature enters (?P<tapped>tapped )?with (?P<counters>.+?) on "
    r"(?:it|her|him)\.$"
)


def _parse_enters_with_counters_line(line: str, card: dict) -> WGLine | None:
    """``<This> enters with an indestructible counter on her.`` (Arwen,
    Leatherhead) -> ``enters_with_named_counters``."""
    m = _ENTERS_WITH_COUNTERS_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.creature_wg_effects import parse_counter_list

    counters = parse_counter_list(m.group("counters"))
    if counters is None:
        return None
    fields: dict = {"enters_with_named_counters": counters}
    if m.group("tapped"):
        fields["enters_tapped"] = True
    return WGLine(fields=fields)


# ---------------------------------------------------------------------
# "As long as ... cards exiled with this creature" (Keen-Eyed Curator)
# ---------------------------------------------------------------------

_EXILED_WITH_CARD_TYPES_STATIC_RE = re.compile(
    r"^As long as there are (?P<n>two|three|four|five|\d+) or more card "
    r"types among cards exiled with this creature, it gets "
    r"\+(?P<p>\d+)/\+(?P<t>\d+)(?: and has (?P<kw>.+))?\.$"
)
_SMALL_NUMBERS = {"two": 2, "three": 3, "four": 4, "five": 5}


def _parse_exiled_with_card_types_static(line: str, card: dict) -> WGLine | None:
    """``As long as there are N or more card types among cards exiled with
    this creature, it gets +P/+T[ and has <keywords>].`` -> a SELF static
    gated by :class:`CardTypesAmongExiledWithSourceAtLeast` (which also
    links the card's exiling abilities)."""
    m = _EXILED_WITH_CARD_TYPES_STATIC_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.effects_ext import _keywords
    from mtgz_ir.effects.base import StaticAbility, StaticScope
    from mtgz_ir.effects.generic.cube_creatures_wg import (
        CardTypesAmongExiledWithSourceAtLeast,
    )

    kws = _keywords(m.group("kw"))
    if kws is None:
        return None
    n = m.group("n")
    return WGLine(statics=(StaticAbility(
        scope=StaticScope.SELF,
        power_bonus=int(m.group("p")),
        toughness_bonus=int(m.group("t")),
        granted_keywords=kws,
        condition=CardTypesAmongExiledWithSourceAtLeast(
            n=int(n) if n.isdigit() else _SMALL_NUMBERS[n],
        ),
    ),))


# ---------------------------------------------------------------------
# Lines ending in a quoted ability
# ---------------------------------------------------------------------


def _parse_quote_final_line(line: str, card: dict) -> WGLine | None:
    """A line whose last sentence is a quoted ability ends in ``."``
    (the period inside the quote -- Mole Man's ``... with "Whenever this
    token attacks, you may mill a card."``); the clause grammar expects
    the sentence period last. Re-parse with the period moved outside."""
    if not line.endswith('."'):
        return None
    from mtgz_ir.cards.parser.clause import _parse_clause

    parsed = _parse_clause(line[:-2] + '".', name=card["name"])
    if parsed is None:
        return None
    return WGLine(
        keywords=tuple(parsed.keywords),
        triggers=tuple(parsed.triggers),
        activated=(parsed.activated,) if parsed.activated is not None else (),
        statics=tuple(parsed.statics),
    )


_ACTIVATION_LOCK_RE = re.compile(
    r"^Activated abilities of (?P<types>[a-z, ]+?)"
    r"(?P<opponents> your opponents control)? can't be activated\.$"
)
_PLURAL_TYPES = {
    "artifacts": "Artifact", "creatures": "Creature",
    "planeswalkers": "Planeswalker", "enchantments": "Enchantment",
    "lands": "Land", "battles": "Battle",
}


def _parse_activation_lock_line(line: str, card: dict) -> WGLine | None:
    """``Activated abilities of <types>[ your opponents control] can't be
    activated.`` (Clarion Conqueror, Null Rod, Stony Silence; Karn, the
    Great Creator) -> ``locks_activated_abilities_of`` /
    ``locks_opponents_activated_abilities_of`` (mana abilities included,
    as printed)."""
    m = _ACTIVATION_LOCK_RE.match(line)
    if m is None:
        return None
    words = re.split(r", and |, or |, | and | or ", m.group("types"))
    if not words or not all(w in _PLURAL_TYPES for w in words):
        return None
    field_name = (
        "locks_opponents_activated_abilities_of" if m.group("opponents")
        else "locks_activated_abilities_of"
    )
    return WGLine(fields={
        field_name: tuple(_PLURAL_TYPES[w] for w in words),
    })


# ---------------------------------------------------------------------
# Backup N (CR 702.165)
# ---------------------------------------------------------------------

_BACKUP_RE = re.compile(r"^Backup (?P<n>\d+)$")


def _parse_backup_line(line: str, card: dict) -> WGLine | None:
    """``Backup N``: the ETB trigger putting N +1/+1 counters on target
    creature; if it's another creature it gains "the following
    abilities" -- every line printed after Backup, which must all be
    keyword lines (those keywords are also the card's own)."""
    m = _BACKUP_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.keywords import _parse_keyword_line
    from mtgz_ir.cards.parser.text import _strip_reminder_text
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.cube_creatures_wg import Backup
    from mtgz_ir.effects.object_predicates import IsCreature
    from mtgz_ir.effects.predicates import Event, IsSelf
    from mtgz_ir.effects.targets import TargetSpec

    lines = [
        _strip_reminder_text(raw) for raw in card.get("oracle_text", "").split("\n")
    ]
    stripped = [ln for ln in lines if ln]
    if line not in stripped:
        return None
    following = stripped[stripped.index(line) + 1:]
    keywords: list[str] = []
    for ln in following:
        kws = _parse_keyword_line(ln)
        if kws is None:
            return None
        keywords.extend(kws)
    return WGLine(triggers=(TriggeredAbility(
        event=Event.ENTERS, predicate=IsSelf(),
        effect=Backup(
            n=int(m.group("n")), keywords=frozenset(keywords),
            targets=(TargetSpec(filter=IsCreature()),),
        ),
    ),))


_ONCE_PER_TURN_GRAVEYARD_PLAY_RE = re.compile(
    r"^Once during each of your turns, you may play a land from your "
    r"graveyard or cast a permanent spell with mana value (?P<mv>\d+) or "
    r"less from your graveyard\. If you do, it gains \"When this permanent "
    r"is put into a graveyard from the battlefield, exile it and you gain "
    r"2 life\.\"$"
)


def _parse_once_per_turn_graveyard_play(line: str, card: dict) -> WGLine | None:
    """Serra Paragon's graveyard permission (``mtgz_ir.
    graveyard_casting_wg``). The granted rider's life amount is the
    printed 2; the recognizer declines any other rider."""
    m = _ONCE_PER_TURN_GRAVEYARD_PLAY_RE.match(line)
    if m is None or int(m.group("mv")) < 1:
        return None
    return WGLine(fields={
        "grants_once_per_turn_graveyard_play_mv": int(m.group("mv")),
    })


_CHOOSE_NAME_AS_ENTERS_LINE = (
    "As this creature enters, look at an opponent's hand, then choose any "
    "card name."
)
_NAMED_SPELLS_COST_MORE_RE = re.compile(
    r"^Spells your opponents cast with the chosen name cost \{(?P<n>\d+)\} "
    r"more to cast\.$"
)
_NAMED_ABILITIES_COST_MORE_RE = re.compile(
    r"^Activated abilities of sources with the chosen name cost "
    r"\{(?P<n>\d+)\} more to activate unless they're mana abilities\.$"
)


def _parse_named_card_lines(line: str, card: dict) -> WGLine | None:
    """Anointed Peacekeeper's three lines: the as-enters name choice and
    the two named-card taxes (``creature_statics_wg``)."""
    if line == _CHOOSE_NAME_AS_ENTERS_LINE:
        return WGLine(fields={"chooses_card_name_as_enters": True})
    m = _NAMED_SPELLS_COST_MORE_RE.match(line)
    if m is not None:
        return WGLine(fields={"named_spell_cost_increase": int(m.group("n"))})
    m = _NAMED_ABILITIES_COST_MORE_RE.match(line)
    if m is not None:
        return WGLine(fields={"named_ability_cost_increase": int(m.group("n"))})
    return None


# ---------------------------------------------------------------------
# Reconfigure (CR 702.151) / Equipment-creature riders (Lion Sash)
# ---------------------------------------------------------------------

_RECONFIGURE_RE = re.compile(r"^Reconfigure (?P<cost>(?:\{[^}]+\})+)$")
_EQUIPPED_PER_OWN_COUNTER_RE = re.compile(
    r"^Equipped creature gets \+(?P<p>\d+)/\+(?P<t>\d+) for each "
    r"(?P<kind>\+1/\+1) counter on this Equipment\.$"
)


def _parse_reconfigure_line(line: str, card: dict) -> WGLine | None:
    """``Reconfigure {cost}``: "{cost}: Attach to target creature you
    control; or {cost}: Unattach this permanent. Reconfigure only as a
    sorcery." Only for Equipment creatures."""
    m = _RECONFIGURE_RE.match(line)
    if m is None or "Equipment" not in card.get("type_line", ""):
        return None
    from mtgz_ir.cards.base import parse_mana_cost
    from mtgz_ir.effects.base import ActivatedAbility, ActivationCost
    from mtgz_ir.effects.generic.cube_creatures_wg import (
        ReconfigureAttach,
        ReconfigureUnattach,
    )

    cost_str = m.group("cost")
    cost = ActivationCost(mana=parse_mana_cost(cost_str), mana_str=cost_str)
    return WGLine(
        fields={
            "reconfigure_cost": parse_mana_cost(cost_str),
            "reconfigure_cost_str": cost_str,
        },
        activated=(
            ActivatedAbility(cost=cost, effect=ReconfigureAttach(), sorcery_speed=True),
            ActivatedAbility(cost=cost, effect=ReconfigureUnattach(), sorcery_speed=True),
        ),
    )


def _parse_equipped_per_own_counter_line(line: str, card: dict) -> WGLine | None:
    m = _EQUIPPED_PER_OWN_COUNTER_RE.match(line)
    if m is None or "Equipment" not in card.get("type_line", ""):
        return None
    return WGLine(fields={"equipment_bonus_per_own_counter": (
        m.group("kind"), int(m.group("p")), int(m.group("t")),
    )})


# ---------------------------------------------------------------------
# Bestow (CR 702.103)
# ---------------------------------------------------------------------

_BESTOW_RE = re.compile(r"^Bestow (?P<cost>(?:\{[^}]+\})+)$")
_ENCHANTED_CREATURE_GETS_RE = re.compile(
    r"^Enchanted creature gets \+(?P<p>\d+)/\+(?P<t>\d+)\.$"
)


def _has_bestow(card: dict) -> bool:
    return any(
        _BESTOW_RE.match(ln.split(" (", 1)[0].strip())
        for ln in card.get("oracle_text", "").split("\n")
    )


def _parse_bestow_lines(line: str, card: dict) -> WGLine | None:
    """``Bestow {cost}`` and, on a bestow creature, ``Enchanted creature
    gets +P/+T.`` (the bonus while it's attached)."""
    m = _BESTOW_RE.match(line)
    if m is not None:
        from mtgz_ir.cards.base import parse_mana_cost

        return WGLine(fields={
            "bestow_cost": parse_mana_cost(m.group("cost")),
            "bestow_cost_str": m.group("cost"),
        })
    m = _ENCHANTED_CREATURE_GETS_RE.match(line)
    if m is not None and _has_bestow(card):
        return WGLine(fields={
            "bestow_enchanted_bonus": (int(m.group("p")), int(m.group("t"))),
        })
    return None


_EXTRA_MANA_WHEN_TAP_CREATURE_RE = re.compile(
    r"^Whenever you tap a creature for mana, add an additional "
    r"(?P<pips>(?:\{[WUBRGC]\})+)\.$"
)


def _parse_extra_mana_on_creature_tap(line: str, card: dict) -> WGLine | None:
    m = _EXTRA_MANA_WHEN_TAP_CREATURE_RE.match(line)
    if m is None:
        return None
    return WGLine(fields={
        "extra_mana_when_you_tap_creature_for_mana": tuple(
            re.findall(r"\{([WUBRGC])\}", m.group("pips")),
        ),
    })


# ---------------------------------------------------------------------
# Eternalize / Kicker (keyword lines with a cost)
# ---------------------------------------------------------------------

_ETERNALIZE_RE = re.compile(r"^Eternalize (?P<cost>(?:\{[^}]+\})+)$")
_KICKER_RE = re.compile(r"^Kicker (?P<cost>(?:\{[^}]+\})+)$")


def _parse_eternalize_line(line: str, card: dict) -> WGLine | None:
    """``Eternalize {cost}`` (CR 702.129a): "{cost}, Exile this card from
    your graveyard: Create a token that's a copy of it, except it's a 4/4
    black Zombie ... with no mana cost. Eternalize only as a sorcery." ->
    a graveyard-activated ability (the R/multi/colorless bucket's
    ``graveyard_activated_abilities`` + ``ExileSelfFromGraveyardCost``)."""
    m = _ETERNALIZE_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.base import parse_mana_cost
    from mtgz_ir.effects.base import ActivatedAbility, ActivationCost
    from mtgz_ir.effects.generic.cube_creatures_rmc import ExileSelfFromGraveyardCost
    from mtgz_ir.effects.generic.cube_creatures_wg import CreateTokenCopyOfSource

    cost_str = m.group("cost")
    return WGLine(fields={"graveyard_activated_abilities": (ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost(cost_str), mana_str=cost_str,
            extra_costs=(ExileSelfFromGraveyardCost(),),
        ),
        effect=CreateTokenCopyOfSource(eternalize=True),
        sorcery_speed=True,
    ),)})


def _parse_kicker_line(line: str, card: dict) -> WGLine | None:
    """``Kicker {cost}`` on a creature (CR 702.33a; Sowing Mycospawn) ->
    ``kicker_cost`` (the cast pipeline's kicked-cast offer)."""
    m = _KICKER_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.base import parse_mana_cost

    return WGLine(fields={
        "kicker_cost": parse_mana_cost(m.group("cost")),
        "kicker_cost_str": m.group("cost"),
    })


_RECOGNIZERS = (
    _parse_evoke_line,
    _parse_ravenous_line,
    _parse_mobilize_line,
    _parse_static_flag_line,
    _parse_one_or_more_enter_once_per_turn,
    _parse_spells_cost_more_line,
    _parse_cant_be_blocked_by_line,
    _parse_your_creatures_damage_unpreventable_line,
    _parse_enters_with_counters_line,
    _parse_exiled_with_card_types_static,
    _parse_quote_final_line,
    _parse_activation_lock_line,
    _parse_backup_line,
    _parse_once_per_turn_graveyard_play,
    _parse_named_card_lines,
    _parse_reconfigure_line,
    _parse_equipped_per_own_counter_line,
    _parse_bestow_lines,
    _parse_extra_mana_on_creature_tap,
    _parse_eternalize_line,
    _parse_kicker_line,
)


# ---------------------------------------------------------------------
# Linked exile abilities (CR 607.2a)
# ---------------------------------------------------------------------


def _is_exiling_effect(effect) -> bool:
    from mtgz_ir.effects.generic.exile import Exile, ExileTargetCardFromGraveyard

    return isinstance(effect, (Exile, ExileTargetCardFromGraveyard)) or bool(
        getattr(effect, "links_exiled_cards", False)
    )


def _consumes_exile_link(ability) -> bool:
    effect = getattr(ability, "effect", None)
    condition = getattr(ability, "condition", None)
    return bool(
        getattr(effect, "consumes_exile_link", False)
        or getattr(condition, "consumes_exile_link", False)
    )


def link_exile_abilities(
    triggers: list, activated: list, statics: list,
) -> None:
    """When a card prints an ability that refers to "the exiled card" /
    "cards exiled with this creature", its exiling ability is the other
    half of a linked pair: wrap every exiling triggered / activated
    ability in ``LinkExiledCardsToSource`` (in place). A card without a
    link consumer is left untouched."""
    from dataclasses import replace

    from mtgz_ir.effects.generic.cube_creatures_wg import (
        LinkExiledCardsToSource,
    )

    if not any(
        _consumes_exile_link(a) for a in (*triggers, *activated, *statics)
    ):
        return
    for abilities in (triggers, activated):
        for i, ability in enumerate(abilities):
            if _is_exiling_effect(ability.effect):
                abilities[i] = replace(
                    ability, effect=LinkExiledCardsToSource(ability.effect),
                )


# ---------------------------------------------------------------------
# Dynamic P/T (``printing_keywords._parse_dynamic_pt_formula_line``)
# ---------------------------------------------------------------------

_POWER_EQUALS_COUNT_RE = re.compile(
    r"^(?:This creature|[A-Z][^.]*?)'s power is equal to the number of "
    r"(?P<noun>[a-z][a-z ]*)\.$"
)


def parse_wg_dynamic_pt_line(line: str):
    """``<This>'s power is equal to the number of <permanents you
    control>.`` (Adeline) -> a structured ``dynamic_pt_formula``
    (:class:`PowerEqualsCount`); ``None`` otherwise."""
    from mtgz_ir.cards.parser.creature_wg_effects import (
        _controlled,
        wg_grammar_disabled,
    )

    if wg_grammar_disabled():
        return None
    m = _POWER_EQUALS_COUNT_RE.match(line)
    if m is None:
        return None
    f = _controlled(m.group("noun"))
    if f is None:
        return None
    from mtgz_ir.effects.generic.cube_creatures_wg import PowerEqualsCount

    return PowerEqualsCount(count_filter=f)


def parse_wg_creature_line(line: str, card: dict) -> WGLine | None:
    """The :class:`WGLine` for ``line`` (one reminder-stripped oracle line
    of ``card``), or ``None`` when no W/G bucket recognizer claims it."""
    from mtgz_ir.cards.parser.creature_wg_effects import wg_grammar_disabled

    if wg_grammar_disabled():
        return None
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    line = _substitute_self_name(line, card["name"])
    for recognizer in _RECOGNIZERS:
        out = recognizer(line, card)
        if out is not None:
            return out
    return None
