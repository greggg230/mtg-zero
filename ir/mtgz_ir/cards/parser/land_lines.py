"""Line-composed land grammar.

The historical land builders in :mod:`land_builder` each match one
whole-card oracle shape (exactly N lines, in a fixed order). This module
instead walks a land's oracle text line by line, sends every line
through a small set of parameterized per-line recognizers, and composes
the resulting pieces into one :func:`make_utility_land` printing. A land
is claimed only when EVERY line is recognized; any unrecognized line
returns ``None`` (the card stays unclaimed — no silent drops).

It runs last in ``_build_land_printing`` so every pre-existing claim
keeps its bespoke builder; only lands no whole-card shape claimed reach
it.

Per-line recognizers:

- **Intrinsic-mana reminder** — ``({T}: Add {W} or {U}.)`` /
  ``({T}: Add {W}, {B}, or {G}.)`` on a land with basic land types. The
  mana ability is intrinsic to the types (CR 305.6), so the line is
  reminder text; it is accepted only when its colors equal the colors
  of the type line's basic land types.
- ``This land enters tapped.``
- ``This land enters tapped unless <condition>.`` —
  ``you control <N> or fewer other lands`` (fast lands),
  ``you control a basic land``, ``you control a/an <basic land type>``
  (check lands). Compiles to ``CardPrinting.enters_tapped_unless``.
  ... or ``it's your first, second, or third turn of the game``
  (Starting Town).
- ``As this land enters, choose a basic land type. Then you may pay N
  life. If you don't, it enters tapped.`` + ``This land is the chosen
  type.`` (Multiversal Passage; both lines required).
- ``This land enters with <N> <kind> counters on it.`` for a counter
  kind without a dedicated engine field (Gemstone Mine).
- ``Cycling <cost>`` — a hand-activated ``<cost>, Discard this card:
  Draw a card.`` ability (CR 702.29a).
- ``[<Ability word> — ]<cost incl. "Discard this card">: <effect>.`` —
  other hand-activated abilities (Channel), via the activated grammar's
  ``from_hand=True`` mode. Ability-word prefixes are stripped (they
  have no rules meaning) before every activated / triggered parse.
- Triggered-ability lines through the unified trigger grammar
  (``When this land enters, surveil 1.``).
- Activated-ability lines through the unified activated grammar. Mana
  abilities are folded into ``produces`` when the engine's automatic
  payment models them exactly:

  * ``{T}: Add {X}.`` / ``{T}: Add {X} or {Y}.`` — free single-mana
    production.
  * ``{T}, Pay N life: Add {X} or {Y}.`` when it is the land's only mana
    ability — ``produces`` + ``produces_life_cost=N``.
  * ``{T}: Add {C}{C}.``-style multi-mana of one kind is kept as an
    explicit mana ability (manual activation yields the full amount) and
    its kind is also listed in ``produces`` (automatic payment may tap
    it for one of that mana — the rest would simply empty).

  Every other mana ability (extra costs, spend restrictions, scaling
  amounts, filter costs) stays an explicit activated mana ability, and
  every printing built here sets ``mana_only_from_produces=True`` so the
  engine's legacy "any land pays one generic" fallback never lets such a
  land (or a fetch land with no mana ability) pay phantom mana.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
    AddNManaOfChosenColor,
    CardPrinting,
    DrawCards,
    make_utility_land,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import (
    _DEDICATED_COUNTER_KINDS,
    _parse_activated_ability_line,
)
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.effects.conditions import (
    Condition,
    ControlPermanent,
    ControlsAtMost,
    YourTurnOfGameAtMost,
)
from mtgz_ir.effects.object_predicates import HasSubtype, IsBasic, IsLand


_BASIC_TYPE_COLORS: dict[str, str] = {
    "Plains": "W",
    "Island": "U",
    "Swamp": "B",
    "Mountain": "R",
    "Forest": "G",
}

_NUMBER_WORDS: dict[str, int] = {
    "zero": 0, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7,
}

# ``({T}: Add {W}.)`` / ``({T}: Add {W} or {U}.)`` /
# ``({T}: Add {W}, {B}, or {G}.)`` — the reminder of a land's intrinsic
# basic-land-type mana abilities.
_INTRINSIC_MANA_REMINDER_RE = re.compile(
    r"^\(\{T\}: Add (?P<body>\{[WUBRG]\}(?:,? (?:or )?\{[WUBRG]\})*)\.\)$"
)
_MANA_SYMBOL_RE = re.compile(r"\{([WUBRGC])\}")

_ENTERS_TAPPED_LINE = "This land enters tapped."
_ENTERS_TAPPED_UNLESS_RE = re.compile(
    r"^This land enters tapped unless (?P<cond>.+)\.$"
)
_CONTROL_N_OR_FEWER_OTHER_LANDS_RE = re.compile(
    r"^you control (?P<n>[a-z]+) or fewer other lands$"
)
_CONTROL_A_BASIC_TYPE_RE = re.compile(
    r"^you control an? (?P<t>Plains|Island|Swamp|Mountain|Forest)"
    r"(?: or an? (?P<t2>Plains|Island|Swamp|Mountain|Forest))?$"
)
# "As this land enters, choose a basic land type. Then you may pay N
# life. If you don't, it enters tapped." + "This land is the chosen
# type." (Multiversal Passage). The choose clause is only meaningful
# with the static line, so the builder requires both.
_CHOOSE_TYPE_THEN_PAY_LIFE_RE = re.compile(
    r"^As this land enters, choose a basic land type\. Then you may pay "
    r"(?P<life>\d+) life\. If you don't, it enters tapped\.$"
)
_IS_CHOSEN_TYPE_LINE = "This land is the chosen type."
# "This land enters with <N> <kind> counters on it." for a
# ``named_counters`` kind (Gemstone Mine: "three mining counters").
_ENTERS_WITH_NAMED_COUNTERS_RE = re.compile(
    r"^This land enters with (?P<n>[a-z]+|\d+) (?P<kind>[a-z]+) counters? "
    r"on it\.$"
)
_CYCLING_RE = re.compile(r"^Cycling (?P<cost>(?:\{[^}]+\})+)$")
# ``<Ability word> — <ability>`` (Channel — ...). Ability words have no
# rules meaning (CR 207.2c); the ability after the dash is parsed on its
# own merits.
_ABILITY_WORD_PREFIX_RE = re.compile(r"^[A-Z][a-z]+ — (?P<rest>.+)$")


_ORDINALS: tuple[str, ...] = (
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh",
)
_YOUR_NTH_TURN_RE = re.compile(
    r"^it's your (?P<list>[a-z]+(?:, [a-z]+)*,? or [a-z]+|[a-z]+) turn of "
    r"the game$"
)


def _parse_your_early_turn(phrase: str) -> Condition | None:
    """``it's your first, second, or third turn of the game`` ->
    ``YourTurnOfGameAtMost(3)``. The ordinals must be exactly
    first..Nth in order."""
    m = _YOUR_NTH_TURN_RE.match(phrase)
    if m is None:
        return None
    words = re.split(r",? or |, ", m.group("list"))
    if tuple(words) != _ORDINALS[: len(words)]:
        return None
    return YourTurnOfGameAtMost(len(words))


def _parse_enters_tapped_condition(phrase: str) -> Condition | None:
    """Map the ``unless <phrase>`` tail of an enters-tapped line to the
    Condition under which the land enters UNTAPPED, or ``None``."""
    m = _CONTROL_N_OR_FEWER_OTHER_LANDS_RE.match(phrase)
    if m is not None:
        n = _NUMBER_WORDS.get(m.group("n"))
        if n is None:
            return None
        return ControlsAtMost(IsLand(), n)
    if phrase == "you control a basic land":
        return ControlPermanent(IsLand() & IsBasic())
    early = _parse_your_early_turn(phrase)
    if early is not None:
        return early
    m = _CONTROL_A_BASIC_TYPE_RE.match(phrase)
    if m is not None:
        # "a Mountain or a Forest" (Mistrise Village): either type.
        pred = HasSubtype(m.group("t"))
        if m.group("t2"):
            pred = pred | HasSubtype(m.group("t2"))
        return ControlPermanent(pred)
    return None


def _intrinsic_colors(subtypes: tuple[str, ...]) -> frozenset[str]:
    return frozenset(
        _BASIC_TYPE_COLORS[s] for s in subtypes if s in _BASIC_TYPE_COLORS
    )


def _simple_mana_colors(effect) -> frozenset[str] | None:
    """Colors of a one-mana, unrestricted production (``Add {X}`` or
    ``Add {X} or {Y}``), else ``None``."""
    if isinstance(effect, AddManaToPool):
        if (
            effect.spend_restriction is not None
            or effect.grants_haste_to_creature_spell
        ):
            return None
        if len(effect.produces) != 1:
            return None
        color, count = effect.produces[0]
        if count != 1 or color == "any":
            return None
        return frozenset({color})
    if isinstance(effect, AddNManaOfChosenColor):
        if effect.amount != 1:
            return None
        return frozenset(effect.options)
    return None


def _multi_mana_kind(effect) -> str | None:
    """The single mana kind of an unrestricted ``Add {X}{X}...`` (two or
    more of one kind), else ``None``."""
    if not isinstance(effect, AddManaToPool):
        return None
    if (
        effect.spend_restriction is not None
        or effect.grants_haste_to_creature_spell
        or len(effect.produces) != 1
    ):
        return None
    color, count = effect.produces[0]
    if count < 2 or color == "any":
        return None
    return color


def _mixed_mana_colors(effect) -> frozenset[str]:
    """The colors of an unrestricted ``Add {X}{Y}...`` minting two or more
    distinct colors (``{T}: Add {G}{U}.``), else an empty set."""
    if not isinstance(effect, AddManaToPool) or (
        effect.spend_restriction is not None
        or effect.grants_haste_to_creature_spell
    ):
        return frozenset()
    colors = frozenset(c for c, n in effect.produces if n > 0)
    if len(colors) < 2 or "any" in colors:
        return frozenset()
    return colors


_TAP_ONLY = ActivationCost(tap_self=True)
_WUBRG = frozenset("WUBRG")
# "{T}: Add {C}. If <this land> has a <kind> counter on it, instead add
# one mana of any color." (Gemstone Caverns) -- a counter-gated mana
# ability: ``<base>`` without the counter, any color with it.
_COUNTER_GATED_MANA_RE = re.compile(
    r"^\{T\}: Add \{(?P<base>[WUBRGC])\}\. If (?P<self>.+?) has an? "
    r"(?P<kind>[a-z]+) counter on it, instead add one mana of any color\.$"
)


def _counter_gated_mana_ability(kind: str, base: str) -> ActivatedAbility:
    from mtgz_ir.effects.combinators import IfConditionElse
    from mtgz_ir.effects.conditions import SourceHasCounter

    return ActivatedAbility(
        cost=_TAP_ONLY,
        effect=IfConditionElse(
            condition=SourceHasCounter(kind),
            then_effect=AddManaToPool(produces=(("any", 1),)),
            else_effect=AddManaToPool(produces=((base, 1),)),
        ),
        is_mana_ability=True,
    )


def _try_build_line_composed_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    card_types: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Compose a land printing from per-line recognizers; ``None`` if
    the card isn't a plain (optionally Legendary) Land or any line is
    unrecognized."""
    from mtgz_ir.cards.parser.roto_mana import (
        extra_land_card_types,
        extra_land_supertypes,
        merge_roto_fields,
        parse_roto_land_line,
    )

    # Artifact lands and Snow (Vintage Roto P3: Seat of the Synod, Dark
    # Depths) ride the printing's card types / supertypes.
    if tuple(card_types) != ("Land",) and (
        tuple(card_types) not in extra_land_card_types()
    ):
        return None
    if set(supertypes) - {"Legendary"} - extra_land_supertypes():
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if not raw_lines:
        return None
    intrinsic = _intrinsic_colors(subtypes)
    enters_tapped = False
    enters_tapped_unless: Condition | None = None
    triggered: list = []
    activated: list[ActivatedAbility] = []
    hand_abilities: list[ActivatedAbility] = []
    free_colors: set[str] = set()
    life_mana: list[tuple[int, frozenset[str], ActivatedAbility]] = []
    chooses_type = False
    is_chosen_type = False
    shock_life = 0
    named_counters: list[tuple[str, int]] = []
    opening_hand_action = None
    gated: tuple[str, str] | None = None  # (counter kind, base color)
    roto_fields: dict = {}
    self_refs = ("this land", card["name"])
    roto_mana_fields: dict = {}  # Vintage Roto P3 lines (``roto_mana``)
    for raw in raw_lines:
        m = _INTRINSIC_MANA_REMINDER_RE.match(raw)
        if m is not None:
            colors = frozenset(_MANA_SYMBOL_RE.findall(m.group("body")))
            if not intrinsic or colors != intrinsic:
                return None
            continue
        line = _strip_reminder_text(raw)
        if not line:
            return None
        from mtgz_ir.cards.parser.opening_hand_lines import (
            parse_opening_hand_line,
        )

        action = parse_opening_hand_line(line, card["name"])
        if action is not None:
            if opening_hand_action is not None:
                return None
            opening_hand_action = action
            continue
        m = _COUNTER_GATED_MANA_RE.match(line)
        if m is not None:
            if gated is not None or m.group("self") not in self_refs:
                return None
            gated = (m.group("kind"), m.group("base"))
            activated.append(
                _counter_gated_mana_ability(gated[0], gated[1])
            )
            continue
        if line == _ENTERS_TAPPED_LINE:
            if enters_tapped or enters_tapped_unless is not None:
                return None
            enters_tapped = True
            continue
        m = _ENTERS_TAPPED_UNLESS_RE.match(line)
        if m is not None:
            if enters_tapped or enters_tapped_unless is not None:
                return None
            cond = _parse_enters_tapped_condition(m.group("cond"))
            if cond is None:
                return None
            enters_tapped_unless = cond
            continue
        m = _ENTERS_WITH_NAMED_COUNTERS_RE.match(line)
        if m is not None:
            n_word = m.group("n")
            n = int(n_word) if n_word.isdigit() else _NUMBER_WORDS.get(
                "one" if n_word == "a" else n_word,
            )
            if n is None or m.group("kind") in _DEDICATED_COUNTER_KINDS:
                return None
            named_counters.append((m.group("kind"), n))
            continue
        m = _CHOOSE_TYPE_THEN_PAY_LIFE_RE.match(line)
        if m is not None:
            if chooses_type:
                return None
            chooses_type = True
            shock_life = int(m.group("life"))
            continue
        if line == _IS_CHOSEN_TYPE_LINE:
            if is_chosen_type:
                return None
            is_chosen_type = True
            continue
        from mtgz_ir.cards.parser.roto_keywords import parse_hand_keyword_line

        p6_hand_ability = parse_hand_keyword_line(line, card)  # roto P6
        if p6_hand_ability is not None:
            hand_abilities.append(p6_hand_ability)
            continue
        m = _CYCLING_RE.match(line)
        if m is not None:
            cost_str = m.group("cost")
            hand_abilities.append(
                ActivatedAbility(
                    cost=ActivationCost(
                        mana=parse_mana_cost(cost_str),
                        mana_str=cost_str,
                        discard_self=True,
                    ),
                    effect=DrawCards(amount=1),
                )
            )
            continue
        from mtgz_ir.cards.parser.cube_glue_grammar import (
            parse_glue_keyword_trigger,
        )

        keyword_trigger = parse_glue_keyword_trigger(line)
        if keyword_trigger is not None:
            # Hideaway N (Shelldock Isle; cube glue bucket).
            triggered.append(keyword_trigger)
            continue
        roto = parse_roto_land_line(line, card["name"])
        if roto is not None:
            # Vintage Roto P3 lines (Cavern of Souls, Dark Depths).
            merge_roto_fields(roto_mana_fields, roto.fields)
            activated.extend(roto.activated)
            continue
        m = _ABILITY_WORD_PREFIX_RE.match(line)
        if m is not None:
            line = m.group("rest")
        hand_ability = _parse_activated_ability_line(line, from_hand=True)
        if hand_ability is not None:
            hand_abilities.append(hand_ability)
            continue
        if line.startswith(("When ", "Whenever ", "At ")):
            trig = _parse_triggered_ability_line(line)
            if trig is None:
                return None
            triggered.append(trig)
            continue
        ability = _parse_activated_ability_line(line)
        if ability is None:
            # Vintage Roto P5 lines (board rules, land-type grants, ...).
            from mtgz_ir.cards.parser.roto_board import absorb_land_line

            if absorb_land_line(line, card, roto_fields, triggered, activated):
                continue
            return None
        if ability.is_mana_ability:
            colors = _simple_mana_colors(ability.effect)
            if colors is not None and ability.cost == _TAP_ONLY:
                free_colors |= colors
                continue
            if (
                colors is not None
                and ability.cost.pay_life
                and ability.cost == ActivationCost(
                    tap_self=True, pay_life=ability.cost.pay_life,
                )
            ):
                life_mana.append((ability.cost.pay_life, colors, ability))
                continue
            kind = _multi_mana_kind(ability.effect)
            if kind is not None and ability.cost == _TAP_ONLY:
                free_colors.add(kind)
            elif ability.cost == _TAP_ONLY:
                # ``{T}: Add {G}{U}.`` (the karoos; Vintage Roto P3):
                # like ``{C}{C}``, automatic payment may tap it for one of
                # its colors; the explicit ability yields both.
                free_colors |= _mixed_mana_colors(ability.effect)
        activated.append(ability)
    if chooses_type != is_chosen_type:
        return None
    produces = set(intrinsic) | free_colors
    produces_with_counter = None
    if gated is not None:
        # Automatic payment: the base mana without the counter; with it
        # the gated ability makes any color instead (other free mana
        # abilities keep producing their own colors either way).
        produces_with_counter = (gated[0], frozenset(produces) | _WUBRG)
        produces.add(gated[1])
    produces_life_cost = 0
    if life_mana:
        life_amounts = {amount for amount, _, _ in life_mana}
        if not produces and len(life_amounts) == 1:
            # Every mana ability of this land costs the same life: fold
            # them into ``produces`` + a per-tap life charge.
            produces_life_cost = life_amounts.pop()
            for _, colors, _ in life_mana:
                produces |= colors
        else:
            # Mixed free + life-costed production (Starting Town): the
            # life-costed ability stays explicit so automatic payment
            # never charges life for the free mana.
            activated.extend(ability for _, _, ability in life_mana)
    printing = make_utility_land(
        card["name"],
        produces=frozenset(produces),
        subtypes_str=" ".join(subtypes),
        legendary="Legendary" in supertypes,
        triggered_abilities=tuple(triggered),
        activated_abilities=tuple(activated),
        enters_tapped=enters_tapped,
        enters_tapped_unless=enters_tapped_unless,
        mana_only_from_produces=True,
        produces_life_cost=produces_life_cost,
        hand_activated_abilities=tuple(hand_abilities),
        shock_land_life_cost=shock_life,
        enters_choosing_basic_land_type=chooses_type,
        is_chosen_basic_land_type=is_chosen_type,
        enters_with_named_counters=tuple(named_counters),
        produces_with_counter=produces_with_counter,
        opening_hand_action=opening_hand_action,
    )
    if "Snow" in supertypes or tuple(card_types) != ("Land",):
        roto_mana_fields["supertypes"] = tuple(supertypes)
        roto_mana_fields["card_types"] = tuple(card_types)
        roto_mana_fields["type_line"] = card["type_line"]
    if roto_mana_fields:
        from dataclasses import replace

        printing = replace(printing, **roto_mana_fields)
    if roto_fields:
        from mtgz_ir.cards.parser.roto_board import apply_board_fields

        printing = apply_board_fields(printing, roto_fields)
    return printing
