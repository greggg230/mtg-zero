"""Oracle-line recognizers for the Vintage Roto P1 package (stack &
restrictions): spell taxes, cast and activation prohibitions, counter-
spell variants and cast triggers.

Two hook points, each consulted only after every older grammar
declined, so no claimed card's parse changes:

* :func:`parse_roto_stack_line` -- whole permanent oracle lines (the
  creature / artifact / cube-enchantment builders' last fallback),
  returning a :class:`~mtgz_ir.cards.parser.creature_wg_lines.WGLine`
  (printing fields, keywords, triggered / activated / static
  abilities). Tuple-valued fields from several lines are merged by
  :func:`merge_fields`.
* :func:`parse_roto_stack_effect` -- the very last fallback of
  ``effects._parse_effect`` (normalized phrases: no trailing period,
  lower-cased first letter).

Smaller hooks feed shared grammars: :func:`parse_roto_cost_piece`
(``activated._classify_cost_piece``), :func:`roto_rider_condition` /
:func:`roto_counter_variant` (``conditional_phrases``'s "instead"
riders), :func:`roto_alt_cost_condition` (``spell_cost_lines``) and
:func:`roto_cost_reduction_condition` (``cost_reduction``).

Like the cube buckets, the grammar is switched off for printings that
exist only in the EoE-era dumps (``creature_rmc_grammar``'s switch).
Engine statics live in :mod:`mtgz_ir.roto_restrictions`; effects in
:mod:`mtgz_ir.effects.generic.roto_stack`.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.parser.creature_wg_lines import WGLine
from mtgz_ir.effects.base import Effect


def _disabled() -> bool:
    from mtgz_ir.cards.parser.creature_rmc_grammar import _DISABLED

    return _DISABLED[0]


# Printing fields whose values are tuples that several oracle lines may
# each contribute to.
_TUPLE_FIELDS = frozenset({
    "roto_statics", "spell_cost_increases", "locks_activated_abilities_of",
})


def merge_fields(into: dict, fields: dict) -> None:
    """Fold one line's printing ``fields`` into ``into`` (tuple-valued
    fields concatenate; everything else is set)."""
    for key, value in fields.items():
        if key in _TUPLE_FIELDS and key in into:
            into[key] = tuple(into[key]) + tuple(value)
        else:
            into[key] = value


# ---------------------------------------------------------------------
# Spell taxes and the cost floor
# ---------------------------------------------------------------------

_SPELLS_COST_MORE_RE = re.compile(
    r"^(?:(?P<desc>[A-Z][a-z]+(?:, [a-z]+)*) spells|Spells) cost "
    r"\{(?P<n>\d+)\} more to cast\.$"
)


def _spells_cost_more(line: str, card: dict) -> WGLine | None:
    """``[<Descriptor>] spells cost {N} more to cast.`` (Sphere of
    Resistance, Thorn of Amethyst) -> ``spell_cost_increases`` (every
    player's matching spells; Thalia's field)."""
    m = _SPELLS_COST_MORE_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.card_flow_grammar import _parse_descriptor_body
    from mtgz_ir.effects.generic.card_flow import CardFilter

    desc = m.group("desc")
    f = CardFilter() if desc is None else _parse_descriptor_body(desc.lower())
    if f is None:
        return None
    return WGLine(fields={"spell_cost_increases": ((f, int(m.group("n"))),)})


_EACH_SPELL_COSTS_MORE_EXCEPT_RE = re.compile(
    r"^Each spell costs \{(?P<n>\d+)\} more to cast except during its "
    r"controller's turn\.$"
)


def _each_spell_costs_more_except_controllers_turn(
    line: str, card: dict,
) -> WGLine | None:
    """``Each spell costs {N} more to cast except during its controller's
    turn.`` (Defense Grid)."""
    m = _EACH_SPELL_COSTS_MORE_EXCEPT_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.roto_restrictions import SpellTax

    return WGLine(fields={"roto_statics": (
        SpellTax(amount=int(m.group("n")), except_during_casters_turn=True),
    )})


_NUMBER_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
}
_MINIMUM_COST_RE = re.compile(
    r"^(?P<untapped>As long as this (?:artifact|creature|permanent) is "
    r"untapped, )?(?:e|E)ach spell that would cost less than (?P<n>\w+) "
    r"mana to cast costs (?P<n2>\w+) mana to cast\.$"
)


def _minimum_spell_cost(line: str, card: dict) -> WGLine | None:
    """``[As long as this artifact is untapped, ]each spell that would
    cost less than N mana to cast costs N mana to cast.`` (Trinisphere)."""
    m = _MINIMUM_COST_RE.match(line)
    if m is None or m.group("n") != m.group("n2"):
        return None
    n = _NUMBER_WORDS.get(m.group("n"))
    if n is None:
        return None
    from mtgz_ir.roto_restrictions import MinimumSpellCost

    return WGLine(fields={"roto_statics": (
        MinimumSpellCost(total=n, while_untapped=bool(m.group("untapped"))),
    )})


# ---------------------------------------------------------------------
# Prohibitions shared with the cube W/G statics
# ---------------------------------------------------------------------


def _activation_lock(line: str, card: dict) -> WGLine | None:
    """``Activated abilities of <types> can't be activated.`` (Null Rod,
    Stony Silence) -- Clarion Conqueror's ``locks_activated_abilities_of``
    (mana abilities included, as printed)."""
    from mtgz_ir.cards.parser.creature_wg_lines import (
        _parse_activation_lock_line,
    )

    return _parse_activation_lock_line(line, card)


def _static_flag(line: str, card: dict) -> WGLine | None:
    """The W/G bucket's rules-restricting flag lines on a noncreature
    permanent ("Each player can't cast more than one spell each turn." --
    High Noon)."""
    from mtgz_ir.cards.parser.creature_wg_lines import _parse_static_flag_line

    return _parse_static_flag_line(line, card)


_ENTERS_WITH_MINUS_COUNTERS_RE = re.compile(
    r"^This (?:creature|artifact|permanent) enters with "
    r"(?P<n>a|an|one|two|three|four|five|\d+) -1/-1 counters? on it\.$"
)


def _enters_with_minus_one_counters(line: str, card: dict) -> WGLine | None:
    """``This creature enters with a -1/-1 counter on it.`` (Glen Elendra
    Guardian) -> ``enters_with_named_counters``."""
    m = _ENTERS_WITH_MINUS_COUNTERS_RE.match(line)
    if m is None:
        return None
    n = m.group("n")
    count = 1 if n in ("a", "an") else (int(n) if n.isdigit() else _NUMBER_WORDS[n])
    return WGLine(fields={"enters_with_named_counters": (("-1/-1", count),)})


_ENTERS_WITH_X_COUNTERS_RE = re.compile(
    r"^This (?:artifact|creature|permanent|enchantment) enters with X "
    r"(?P<kind>charge) counters on it\.$"
)


def _enters_with_x_counters(line: str, card: dict) -> WGLine | None:
    """``This artifact enters with X charge counters on it.`` (Chalice of
    the Void) -> ``enters_with_counters_from_x="x_charge"`` (X is the
    value paid when it was cast; 0 otherwise)."""
    m = _ENTERS_WITH_X_COUNTERS_RE.match(line)
    if m is None:
        return None
    return WGLine(fields={"enters_with_counters_from_x": "x_charge"})


_CUMULATIVE_UPKEEP_RE = re.compile(
    r"^Cumulative upkeep (?P<cost>(?:\{[0-9WUBRGC]\})+)$"
)


def _cumulative_upkeep(line: str, card: dict) -> WGLine | None:
    """``Cumulative upkeep {cost}`` (CR 702.24; Mystic Remora) -- a mana
    cost only (life / other costs aren't modeled)."""
    m = _CUMULATIVE_UPKEEP_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.roto_stack import CumulativeUpkeep
    from mtgz_ir.effects.predicates import YOU, Controlled, Event

    return WGLine(triggers=(TriggeredAbility(
        event=Event.BEGINNING_OF_UPKEEP, predicate=Controlled(YOU),
        effect=CumulativeUpkeep(cost=m.group("cost")),
    ),))


# ---------------------------------------------------------------------
# Cast triggers
# ---------------------------------------------------------------------

_CAST_TRIGGER_RE = re.compile(
    r"^Whenever (?P<who>a player|an opponent|you) casts? (?P<what>[^,]+), "
    r"(?P<eff>.+)\.$"
)
_FIRST_EACH_TURN_RE = re.compile(
    r"^their first (?P<np>(?:noncreature )?spell) each turn$"
)
_MV_AT_MOST_SUFFIX_RE = re.compile(
    r"^(?P<np>.+?) with mana value (?P<n>\d+) or less$"
)
_MV_EQUALS_COUNTERS_SUFFIX_RE = re.compile(
    r"^(?P<np>.+?) with mana value equal to the number of (?P<kind>[a-z]+) "
    r"counters on this (?:artifact|creature|permanent|enchantment)$"
)


def _cast_subject_predicate(what: str):
    """The cast event's ``spell`` condition for ``a <spell np>`` (``None``
    when the phrase is outside the grammar; ``Always()`` for "a spell")."""
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.roto_stack import (
        CastersNoncreatureSpellsThisTurnIs,
        SpellManaValueAtMost,
        SpellManaValueEqualsSourceCounters,
    )
    from mtgz_ir.effects.predicates import Always, Subject

    m = _FIRST_EACH_TURN_RE.match(what)
    if m is not None:
        if m.group("np") != "noncreature spell":
            return None
        from mtgz_ir.effects.object_predicates import IsCreature

        return Subject("spell", ~IsCreature()) & CastersNoncreatureSpellsThisTurnIs(1)
    for article in ("a ", "an "):
        if what.startswith(article):
            np = what[len(article):]
            break
    else:
        return None
    extra = None
    m = _MV_EQUALS_COUNTERS_SUFFIX_RE.match(np)
    if m is not None:
        np = m.group("np")
        extra = SpellManaValueEqualsSourceCounters(kind=m.group("kind"))
    else:
        m = _MV_AT_MOST_SUFFIX_RE.match(np)
        if m is not None:
            np = m.group("np")
            extra = Subject("spell", SpellManaValueAtMost(int(m.group("n"))))
    ok, f = parse_spell_phrase(np)
    if not ok:
        return None
    pred = Always() if f is None else Subject("spell", f)
    if extra is not None:
        pred = extra if f is None else pred & extra
    return pred


_UNLESS_PAYS_RE = re.compile(
    r"^counter (?:it|that spell) unless that player pays "
    r"(?P<cost>(?:\{[0-9WUBRGC]\})+)$"
)
_DEALS_DAMAGE_TO_THAT_PLAYER_RE = re.compile(
    r"^this (?:creature|artifact|enchantment|permanent) deals (?P<n>\d+) "
    r"damage to that player$"
)
_DRAW_UNLESS_PAYS_RE = re.compile(
    r"^(?P<may>you may )?draw a card unless that player pays "
    r"(?P<cost>\{X\}, where X is this creature's power|"
    r"(?:\{[0-9WUBRGC]\})+)$"
)


def _cast_trigger_effect(eff: str):
    """``(effect, extra spell condition)`` for a cast trigger's effect
    text, or ``None``."""
    from mtgz_ir.effects.generic.roto_stack import (
        CounterEventSpell,
        DrawUnlessCasterPays,
        GainLifePerSpellYouCastThisTurn,
        NoManaSpentToCast,
        SourceDealsDamageToCaster,
    )
    from mtgz_ir.effects.predicates import Subject

    extra = None
    if eff.startswith("if no mana was spent to cast it, "):
        # Intervening if (CR 603.4): the mana spent never changes once
        # the spell is cast, so the trigger-time check is also the
        # resolution-time check.
        eff = eff[len("if no mana was spent to cast it, "):]
        extra = Subject("spell", NoManaSpentToCast())
    if eff == "counter that spell":
        return CounterEventSpell(), extra
    m = _UNLESS_PAYS_RE.match(eff)
    if m is not None:
        return CounterEventSpell(unless_pays=m.group("cost")), extra
    if extra is not None:
        return None
    m = _DEALS_DAMAGE_TO_THAT_PLAYER_RE.match(eff)
    if m is not None:
        return SourceDealsDamageToCaster(amount=int(m.group("n"))), None
    m = _DRAW_UNLESS_PAYS_RE.match(eff)
    if m is not None:
        cost = m.group("cost")
        if cost.startswith("{X}"):
            eff_obj = DrawUnlessCasterPays(
                cost_is_source_power=True, optional=bool(m.group("may")),
            )
        else:
            eff_obj = DrawUnlessCasterPays(cost=cost, optional=bool(m.group("may")))
        return eff_obj, None
    # Literals: Aetherflux Reservoir / The Queen of Dale are the only
    # cards in the pool with these effects; broader grammar deferred
    # until a second example exists.
    if eff == "you gain 1 life for each spell you've cast this turn":
        return GainLifePerSpellYouCastThisTurn(), None
    if eff == "you recruit":
        from mtgz_ir.effects.generic.roto_stack import Recruit

        return Recruit(), None
    return None


def _cast_trigger(line: str, card: dict) -> WGLine | None:
    """``Whenever <a player | an opponent | you> cast(s) <spell>, <effect>.``
    -- the cast triggers whose effect refers back to the spell or its
    caster (Chalice of the Void, Eidolon of the Great Revel, Esper
    Sentinel, Mystic Remora, Vexing Bauble, Nether Void, Aetherflux
    Reservoir)."""
    m = _CAST_TRIGGER_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.predicates import OPPONENT, YOU, Controlled, Event

    who = m.group("who")
    subject = _cast_subject_predicate(m.group("what"))
    if subject is None:
        return None
    if m.group("what").startswith("their ") and who != "an opponent":
        return None
    parsed = _cast_trigger_effect(m.group("eff"))
    if parsed is None:
        return None
    effect, extra = parsed
    from mtgz_ir.effects.predicates import Always

    parts = [] if isinstance(subject, Always) else [subject]
    if who == "an opponent":
        parts.append(Controlled(OPPONENT, subject="controller"))
    elif who == "you":
        parts.append(Controlled(YOU, subject="controller"))
    if extra is not None:
        parts.append(extra)
    pred = parts[0] if parts else subject
    for part in parts[1:]:
        pred = pred & part
    return WGLine(triggers=(TriggeredAbility(
        event=Event.IS_CAST, predicate=pred, effect=effect,
    ),))


# ---------------------------------------------------------------------
# "Choose a card name" and the named restrictions
# ---------------------------------------------------------------------

_CHOOSE_NAME_RE = re.compile(
    r"^As this (?:creature|artifact|enchantment|permanent) enters, choose a "
    r"(?P<nonland>nonland )?card name\.$"
)
_NAMED_ACTIVATION_LOCK_RE = re.compile(
    r"^Activated abilities of sources with the chosen name can't be "
    r"activated(?P<mana> unless they're mana abilities)?\.$"
)
# Literal: Meddling Mage is the only card in the pool with this line;
# broader grammar deferred until a second example exists.
_NAMED_CANT_CAST = "Spells with the chosen name can't be cast."
_NAMED_TAX_RE = re.compile(
    r"^Spells with the chosen name cost \{(?P<n>\d+)\} more to cast\.$"
)


def _named_lines(line: str, card: dict) -> WGLine | None:
    """Pithing Needle / Phyrexian Revoker / Disruptor Flute / Meddling
    Mage: the as-enters name choice and the restrictions keyed on it."""
    from mtgz_ir.roto_restrictions import (
        ActivationProhibition,
        CastProhibition,
        ChooseCardNameAsEnters,
        SpellTax,
    )

    m = _CHOOSE_NAME_RE.match(line)
    if m is not None:
        static = ChooseCardNameAsEnters(nonland=bool(m.group("nonland")))
        return WGLine(fields={"roto_statics": (static,)})
    m = _NAMED_ACTIVATION_LOCK_RE.match(line)
    if m is not None:
        static = ActivationProhibition(except_mana_abilities=bool(m.group("mana")))
        return WGLine(fields={"roto_statics": (static,)})
    if line == _NAMED_CANT_CAST:
        return WGLine(fields={"roto_statics": (
            CastProhibition(who="each", named=True),
        )})
    m = _NAMED_TAX_RE.match(line)
    if m is not None:
        return WGLine(fields={"roto_statics": (
            SpellTax(amount=int(m.group("n")), named=True),
        )})
    return None


_MV_ABOVE_LANDS_RE = re.compile(
    r"^Each opponent can't cast (?P<np>(?:[a-z]+ )?spells) with mana value "
    r"greater than the number of lands that player controls\.$"
)


def _mv_above_lands(line: str, card: dict) -> WGLine | None:
    """``Each opponent can't cast <type> spells with mana value greater
    than the number of lands that player controls.`` (Lavinia, Azorius
    Renegade)."""
    m = _MV_ABOVE_LANDS_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.roto_restrictions import CastProhibition

    ok, f = parse_spell_phrase(m.group("np")[:-1])
    if not ok:
        return None
    return WGLine(fields={"roto_statics": (CastProhibition(
        who="opponents", spell_filter=f, mv_above_casters_lands=True,
    ),)})


# ---------------------------------------------------------------------
# Activation triggers, entering-trigger watchers, opening-hand reveals
# ---------------------------------------------------------------------

_ACTIVATION_TRIGGER_RE = re.compile(
    r"^Whenever an opponent activates an ability of an? (?P<types>[a-z, ]+?) "
    r"on the battlefield, if it isn't a mana ability, this "
    r"(?:creature|artifact|enchantment|permanent) deals (?P<n>\d+) damage "
    r"to that player\.$"
)
def _type_word_predicate(word: str):
    from mtgz_ir.effects.object_predicates import (
        IsArtifact, IsCreature, IsEnchantment, IsLand, IsPlaneswalker,
    )

    return {
        "artifact": IsArtifact, "creature": IsCreature, "land": IsLand,
        "enchantment": IsEnchantment, "planeswalker": IsPlaneswalker,
    }.get(word, lambda: None)()


def _activation_trigger(line: str, card: dict) -> WGLine | None:
    """``Whenever an opponent activates an ability of an artifact,
    creature, or land on the battlefield, if it isn't a mana ability,
    this creature deals N damage to that player.`` (Harsh Mentor). The
    activation event is only dispatched for non-mana abilities."""
    m = _ACTIVATION_TRIGGER_RE.match(line)
    if m is None:
        return None
    atoms = [_type_word_predicate(w) for w in re.split(r", or |, | or ", m.group("types"))]
    if any(a is None for a in atoms):
        return None
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.roto_stack import SourceDealsDamageToCaster
    from mtgz_ir.effects.predicates import OPPONENT, Controlled, Event, Subject

    kinds = atoms[0]
    for a in atoms[1:]:
        kinds = kinds | a
    pred = Subject("source", kinds) & Controlled(OPPONENT, subject="controller")
    return WGLine(triggers=(TriggeredAbility(
        event=Event.ACTIVATES_ABILITY, predicate=pred,
        effect=SourceDealsDamageToCaster(amount=int(m.group("n"))),
    ),))


_COUNTER_ENTERING_TRIGGERS_RE = re.compile(
    r"^Whenever a permanent entering causes a triggered ability to trigger, "
    r"counter that ability unless its controller pays "
    r"(?P<cost>(?:\{[0-9WUBRGC]\})+)\.$"
)


def _counter_entering_triggers(line: str, card: dict) -> WGLine | None:
    """``Whenever a permanent entering causes a triggered ability to
    trigger, counter that ability unless its controller pays {N}.``
    (Strict Proctor)."""
    m = _COUNTER_ENTERING_TRIGGERS_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.roto_restrictions import CounterEnteringTriggers

    return WGLine(fields={"roto_statics": (
        CounterEnteringTriggers(unless_pays=m.group("cost")),
    )})


_OPENING_HAND_REVEAL_RE = re.compile(
    r"^You may reveal this card from your opening hand\. If you do, when "
    r"each opponent casts their first spell of the game, counter that spell "
    r"unless that player pays (?P<cost>(?:\{[0-9WUBRGC]\})+)\.$"
)


def _opening_hand_reveal(line: str, card: dict) -> WGLine | None:
    """Chancellor of the Annex's opening-hand reveal (CR 103.6) ->
    ``opening_hand_action``."""
    m = _OPENING_HAND_REVEAL_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.effects.generic.roto_stack import RevealForFirstSpellTax

    return WGLine(fields={
        "opening_hand_action": RevealForFirstSpellTax(unless_pays=m.group("cost")),
    })


_GRAVEYARD_EXILE_TAX_RE = re.compile(
    r"^Spells your opponents cast from graveyards or from exile cost "
    r"\{(?P<n>\d+)\} more to cast\.$"
)


def _graveyard_exile_tax(line: str, card: dict) -> WGLine | None:
    """``Spells your opponents cast from graveyards or from exile cost {N}
    more to cast.`` (Aven Interrupter)."""
    m = _GRAVEYARD_EXILE_TAX_RE.match(line)
    if m is None:
        return None
    from mtgz_ir.roto_restrictions import SpellTax

    return WGLine(fields={"roto_statics": (SpellTax(
        amount=int(m.group("n")), opponents_only=True,
        from_graveyard_or_exile=True,
    ),)})


# ---------------------------------------------------------------------
# "<pump creatures you control>. Untap those creatures" (Jeskai Ascendancy)
# ---------------------------------------------------------------------

_PUMP_THEN_UNTAP_RE = re.compile(
    r"^(?P<pump>creatures you control get [+-]\d+/[+-]\d+ until end of "
    r"turn)\. Untap those creatures$"
)


def _pump_then_untap(phrase: str) -> Effect | None:
    """``creatures you control get +N/+M until end of turn. Untap those
    creatures`` -- "those creatures" are the ones the first sentence
    affected: the creatures you control as it resolves."""
    m = _PUMP_THEN_UNTAP_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.combinators import Sequence

    pump = _parse_effect(m.group("pump"))
    untap = _parse_effect("untap all creatures you control")
    if pump is None or untap is None:
        return None
    return Sequence(effects=(pump, untap))


# ---------------------------------------------------------------------
# Effect phrases: "can't cast / can't attack this turn", "it deals"
# ---------------------------------------------------------------------

_CANT_CAST_THIS_TURN_RE = re.compile(
    r"^(?P<who>your opponents|target player) can't cast "
    r"(?P<np>(?:[a-z]+ )?spells) this turn$"
)


def _cant_cast_this_turn(phrase: str) -> Effect | None:
    """``your opponents | target player can't cast [<type>] spells this
    turn`` (Silence, Orim's Chant, Ranger-Captain of Eos)."""
    m = _CANT_CAST_THIS_TURN_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.roto_stack import CantCastSpellsThisTurn

    ok, f = parse_spell_phrase(m.group("np")[:-1])  # "spells" -> "spell"
    if not ok:
        return None
    who = "opponents" if m.group("who") == "your opponents" else "target_player"
    return CantCastSpellsThisTurn(who=who, spell_filter=f)


def _creatures_cant_attack_this_turn(phrase: str) -> Effect | None:
    """``creatures can't attack this turn`` (Orim's Chant's kicker).
    Literal: only card in the pool with this shape; broader grammar
    deferred until a second example exists."""
    if phrase != "creatures can't attack this turn":
        return None
    from mtgz_ir.effects.generic.roto_stack import CreaturesCantAttackThisTurn

    return CreaturesCantAttackThisTurn()


_IF_KICKED_RE = re.compile(r"^if this spell was kicked, (?P<eff>.+)$")


def _if_kicked(phrase: str) -> Effect | None:
    """``if this spell was kicked, <untargeted effect>`` -- an additional
    kicked-only effect (Orim's Chant's "creatures can't attack this
    turn"); the "<...> instead" form is ``conditional_phrases``'s."""
    m = _IF_KICKED_RE.match(phrase)
    if m is None or m.group("eff").endswith(" instead"):
        return None
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.combinators import IfConditionElse
    from mtgz_ir.effects.generic.cast_conditionals import SpellWasKicked

    inner = _parse_effect(m.group("eff"))
    if inner is None or inner.requires_target():
        return None
    return IfConditionElse(condition=SpellWasKicked(), then_effect=inner)


# ---------------------------------------------------------------------
# Cost pieces
# ---------------------------------------------------------------------

_REMOVE_ANY_COUNTER_PIECE = re.compile(
    r"^Remove a counter from this (?:creature|artifact|permanent)$"
)


def parse_roto_cost_piece(piece: str):
    """``(cost-piece kind, atom)`` for a cost piece the base grammar
    declined (the last fallback of ``activated._classify_cost_piece``):
    ``("extra", <atom>)`` for a composable ``ActivationCost.extra_costs``
    atom, ``("sacrifice_self", None)`` for "Sacrifice this enchantment"
    (High Noon -- the enchantment wording of the base's sacrifice-self
    pieces). ``None`` otherwise."""
    if _disabled():
        return None
    if _REMOVE_ANY_COUNTER_PIECE.match(piece):
        from mtgz_ir.effects.generic.roto_stack import (
            RemoveAnyCounterFromSelfCost,
        )

        return ("extra", RemoveAnyCounterFromSelfCost())
    if piece == "Sacrifice this enchantment":
        return ("sacrifice_self", None)
    return None


# ---------------------------------------------------------------------
# Effect phrases: counterspell variants
# ---------------------------------------------------------------------

# Plural subtypes whose singular isn't the regular English one.
_IRREGULAR_PLURAL_SUBTYPES = {
    "Faeries": "Faerie", "Zombies": "Zombie", "Pixies": "Pixie",
}


def _plural_permanent_noun(noun: str):
    """``Faeries`` / ``artifacts`` / ``Elves`` -> an ObjectPredicate."""
    from mtgz_ir.cards.parser.creature_rmc_grammar import (
        _permanent_noun_predicate,
    )
    from mtgz_ir.effects.object_predicates import HasSubtype

    if noun in _IRREGULAR_PLURAL_SUBTYPES:
        return HasSubtype(_IRREGULAR_PLURAL_SUBTYPES[noun])
    return _permanent_noun_predicate(noun, plural=True)


_COUNTER_WITH_MV_RE = re.compile(
    r"^counter target (?P<np>.+?) with mana value (?P<n>\d+)$"
)


def _counter_with_mana_value(phrase: str) -> Effect | None:
    """``counter target <spell np> with mana value N`` (Mental Misstep,
    Spell Snare) -- X counts on the stack."""
    m = _COUNTER_WITH_MV_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    from mtgz_ir.effects.generic.roto_stack import SpellManaValueIs

    ok, f = parse_spell_phrase(m.group("np"))
    if not ok:
        return None
    mv = SpellManaValueIs(int(m.group("n")))
    return CounterSpell(
        spell_filter=mv if f is None else f & mv,
        label=f"{m.group('np')} with mana value {m.group('n')}",
    )


_COUNTER_MV_AT_MOST_COUNT_RE = re.compile(
    r"^counter target (?P<np>.+?) with mana value X or less, where X is the "
    r"number of (?P<noun>.+) you control$"
)


def _counter_mv_at_most_count(phrase: str) -> Effect | None:
    """``counter target <spell np> with mana value X or less, where X is
    the number of <permanents> you control`` (Spellstutter Sprite)."""
    m = _COUNTER_MV_AT_MOST_COUNT_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.roto_stack import CounterSpellMVAtMostCount

    ok, f = parse_spell_phrase(m.group("np"))
    pred = _plural_permanent_noun(m.group("noun"))
    if not ok or pred is None:
        return None
    return CounterSpellMVAtMostCount(
        spell_filter=f, count_predicate=pred,
        label=f"{m.group('np')} with mana value X or less",
    )


_COUNTER_THEN_CONTROLLER_RE = re.compile(
    r"^(?P<counter>counter target [^.]+)\. Its controller "
    r"(?P<verb>creates) (?P<rest>.+)$"
)


def _counter_then_controller(phrase: str) -> Effect | None:
    """``<counter target ...>. Its controller creates <token>`` (Swan
    Song, Strix Serenade). The "Its controller draws a card" sibling
    (Glen Elendra Guardian) is the U/B bucket's ``ThenItsControllerDraws``."""
    m = _COUNTER_THEN_CONTROLLER_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.counter_phrases import parse_counter
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    from mtgz_ir.effects.generic.roto_stack import (
        CounterSpellThenControllerEffect,
    )

    counter = parse_counter(m.group("counter"))
    if not isinstance(counter, CounterSpell):
        return None
    rider = _parse_effect(f"create {m.group('rest')}")
    if rider is None or rider.requires_target():
        return None
    return CounterSpellThenControllerEffect(counter=counter, rider=rider)


def _return_spell_or_permanent(phrase: str) -> Effect | None:
    """``return target spell or permanent to its owner's hand`` (Venser,
    Shaper Savant). Literal: only card in the pool with this shape (the
    nonland / opponent-only sibling is ``card_flow_grammar``'s); broader
    grammar deferred until a second example exists."""
    if phrase != "return target spell or permanent to its owner's hand":
        return None
    from mtgz_ir.effects.generic.roto_stack import (
        ReturnTargetSpellOrPermanentToHand,
    )

    return ReturnTargetSpellOrPermanentToHand()


def _exile_spell_plotted(phrase: str) -> Effect | None:
    """``exile target spell. It becomes plotted`` (Aven Interrupter).
    Literal: only card in the pool with this shape; broader grammar
    deferred until a second example exists."""
    if phrase != "exile target spell. It becomes plotted":
        return None
    from mtgz_ir.effects.generic.roto_stack import ExileTargetSpellPlotted

    return ExileTargetSpellPlotted()


def _exile_any_number_of_spells(phrase: str) -> Effect | None:
    """``exile any number of target spells`` (Mindbreak Trap). Literal:
    only card in the pool with this shape; broader grammar deferred
    until a second example exists."""
    if phrase != "exile any number of target spells":
        return None
    from mtgz_ir.effects.generic.roto_stack import ExileTargetSpells

    return ExileTargetSpells()


# ---------------------------------------------------------------------
# "Instead" riders (``conditional_phrases.parse_instead_rider`` hooks)
# ---------------------------------------------------------------------

_CONTROL_CREATURE_POWER_RE = re.compile(
    r"^you control a creature with power (?P<n>\d+) or greater$"
)


def roto_rider_condition(text: str):
    """``you control a creature with power N or greater`` (Ferocious) ->
    a ``Condition`` checked on resolution, or ``None``."""
    if _disabled():
        return None
    m = _CONTROL_CREATURE_POWER_RE.match(text)
    if m is None:
        return None
    from mtgz_ir.effects.conditions import ControlPermanent
    from mtgz_ir.effects.object_predicates import HasPowerAtLeast, IsCreature

    return ControlPermanent(IsCreature() & HasPowerAtLeast(int(m.group("n"))))


def roto_counter_variant(eff_text: str, base):
    """``counter that spell`` restating a taxed counterspell without its
    tax (Stubborn Denial), or ``None``."""
    if _disabled() or eff_text != "counter that spell":
        return None
    from mtgz_ir.effects.generic.counterspells import CounterSpell

    if not isinstance(base, CounterSpell) or not base.unless_pays:
        return None
    import dataclasses

    return dataclasses.replace(base, unless_pays="")


# ---------------------------------------------------------------------
# Alternative-cost gates (``spell_cost_lines`` hook)
# ---------------------------------------------------------------------

_OPPONENT_CAST_N_SPELLS_RE = re.compile(
    r"^an opponent cast (?P<n>\w+) or more spells this turn$"
)


def roto_alt_cost_condition(text: str):
    """``an opponent cast N or more spells this turn`` (Mindbreak Trap) ->
    :class:`~mtgz_ir.effects.generic.roto_stack.OpponentCastSpellsThisTurn`."""
    if _disabled():
        return None
    m = _OPPONENT_CAST_N_SPELLS_RE.match(text)
    if m is None:
        return None
    n = m.group("n")
    n = int(n) if n.isdigit() else _NUMBER_WORDS.get(n)
    if n is None:
        return None
    from mtgz_ir.effects.generic.roto_stack import OpponentCastSpellsThisTurn

    return OpponentCastSpellsThisTurn(n)


# ---------------------------------------------------------------------
# Cost-reduction conditions (``cost_reduction`` hook)
# ---------------------------------------------------------------------

_TARGETS_SPELL_RE = re.compile(r"^it targets an? (?P<np>.+ spell)$")


def roto_cost_reduction_condition(text: str):
    """``it targets a <spell np>`` (Mystical Dispute: "a blue spell") ->
    :class:`~mtgz_ir.effects.generic.roto_stack.TargetsSpellMatching`."""
    if _disabled():
        return None
    m = _TARGETS_SPELL_RE.match(text)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.roto_stack import TargetsSpellMatching

    ok, f = parse_spell_phrase(m.group("np"))
    if not ok or f is None:
        return None
    return TargetsSpellMatching(f)


_COUNTER_ABILITY_OR_SPELL_RE = re.compile(r"^counter target (?P<items>.+)$")
_ABILITY_KINDS = {
    "activated ability": ("activated",),
    "triggered ability": ("triggered",),
}


def _counter_ability_or_spell(phrase: str) -> Effect | None:
    """``counter target triggered ability or colorless spell`` (Consign
    to Memory): stack-object kinds where the spell kind carries a filter
    (``colorless`` included)."""
    m = _COUNTER_ABILITY_OR_SPELL_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.roto_stack import (
        CounterTargetStackObjectFiltered,
        IsColorless,
    )

    kinds: list[str] = []
    spell_filter = None
    has_spell = False
    for item in re.split(r", or |, | or ", m.group("items")):
        if item in _ABILITY_KINDS:
            kinds.extend(_ABILITY_KINDS[item])
            continue
        if has_spell or not item.endswith("spell"):
            return None
        has_spell = True
        colorless = item.startswith("colorless ")
        if colorless:
            item = item[len("colorless "):]
        ok, f = parse_spell_phrase(item)
        if not ok:
            return None
        if colorless:
            f = IsColorless() if f is None else f & IsColorless()
        spell_filter = f
        kinds.append("spell")
    if not has_spell or len(kinds) < 2 or spell_filter is None:
        # Plain spell counters and pure ability counters belong to the
        # base / U/B grammars.
        return None
    return CounterTargetStackObjectFiltered(
        kinds=tuple(kinds), spell_filter=spell_filter,
    )


# ---------------------------------------------------------------------
# Entry points
# ---------------------------------------------------------------------

_LINE_RECOGNIZERS = (
    _spells_cost_more,
    _each_spell_costs_more_except_controllers_turn,
    _minimum_spell_cost,
    _activation_lock,
    _static_flag,
    _enters_with_minus_one_counters,
    _enters_with_x_counters,
    _cumulative_upkeep,
    _cast_trigger,
    _named_lines,
    _mv_above_lands,
    _activation_trigger,
    _counter_entering_triggers,
    _opening_hand_reveal,
    _graveyard_exile_tax,
)


def parse_roto_stack_line(line: str, card: dict) -> WGLine | None:
    """The :class:`WGLine` for one reminder-stripped permanent oracle
    ``line`` of ``card``, or ``None`` when no P1 recognizer claims it."""
    if _disabled():
        return None
    from mtgz_ir.cards.parser.triggers import _substitute_self_name

    line = _substitute_self_name(line.strip(), card.get("name", ""))
    for recognizer in _LINE_RECOGNIZERS:
        out = recognizer(line, card)
        if out is not None:
            return out
    return None


_EFFECT_RECOGNIZERS: tuple = (
    _counter_with_mana_value,
    _counter_mv_at_most_count,
    _counter_then_controller,
    _return_spell_or_permanent,
    _exile_any_number_of_spells,
    _exile_spell_plotted,
    _counter_ability_or_spell,
    _cant_cast_this_turn,
    _creatures_cant_attack_this_turn,
    _if_kicked,
    _pump_then_untap,
)


def parse_roto_stack_effect(phrase: str) -> Effect | None:
    """A P1 effect phrase (normalized), or ``None``."""
    if _disabled():
        return None
    for recognizer in _EFFECT_RECOGNIZERS:
        eff = recognizer(phrase)
        if eff is not None:
            return eff
    return None


__all__ = [
    "merge_fields",
    "parse_roto_cost_piece",
    "parse_roto_stack_effect",
    "parse_roto_stack_line",
    "roto_alt_cost_condition",
    "roto_cost_reduction_condition",
    "roto_counter_variant",
    "roto_rider_condition",
]
