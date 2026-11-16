"""Planeswalker ``CardPrinting`` builder — oracle-text driven.

A printed planeswalker line is one of:

* a *loyalty-activated* ability — a line whose printed cost is the
  ``[+−]?<digit(s)>:`` loyalty prefix (Scryfall renders the negative
  sign as the U+2212 minus character ``−``, the positive sign as ``+``,
  and bare zero as ``0:``);
* a keyword line (``Flash``);
* a planeswalker static (see ``_STATIC_LINE_RECOGNIZERS``);
* a triggered ability (the unified triggered-ability grammar, plus the
  "When ~ enters and at the beginning of <when>, <effect>." two-arm
  shape handled here).

Every recognized loyalty line becomes an :class:`ActivatedAbility`
with:

  * ``cost=ActivationCost(loyalty=N)`` — the signed loyalty cost. A
    printed ``+2`` ability stamps ``loyalty=-2`` (ADD); a printed
    ``-3`` ability stamps ``loyalty=3`` (PAY); ``0:`` stamps
    ``loyalty=0``. Mirrors Rust's sign convention exactly.
  * ``sorcery_speed=True`` — loyalty abilities are inherently
    sorcery-speed (CR 606.3 timing: only as a sorcery, your main phase,
    empty stack). The Wandering Emperor's static lifts this the turn
    she entered (``legal.activation_timing_permits``).
  * ``effect`` — the loyalty body parsed through the unified effect
    grammar (``_parse_spell_effect_phrase``), after the Tezzeret,
    Cruel Captain literal table (``_LOYALTY_ABILITY_EFFECTS``).

**No partial claims.** A loyalty line whose body the grammar can't
parse — like any other unrecognized line — makes the whole card fall
through (returns ``None``). Earlier revisions STRIPPED unrecognized
loyalty bodies, which "claimed" seven cube planeswalkers (Jace, the
Mind Sculptor; Oko; Wrenn and Six; Teferi, Hero of Dominaria; Karn,
Scion of Urza; Chandra, Torch of Defiance; Dack Fayden) with zero
loyalty abilities. That violated the parser's no-silent-no-op rule and
was removed.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CardPrinting,
    StaticAbility,
    StaticScope,
    TriggeredAbility,
    make_planeswalker,
)
from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
from mtgz_ir.cards.parser.keywords import _parse_keyword_line
from mtgz_ir.cards.parser.printing_keywords import _parse_token_doubler_line
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.effects.generic.emblem import tezzeret_cruel_captain_emblem
from mtgz_ir.effects.generic.search import SearchLibraryForCard
from mtgz_ir.effects.generic.tap import (
    UntapTargetArtifactOrCreatureConditionalCounter,
)
from mtgz_ir.effects.modifiers import (
    EventPattern,
    MultiplyCount,
    ReplacementEffect,
    WinTheGameInstead,
)
from mtgz_ir.effects.predicates import Event


# Loyalty-activation cost prefix on a printed planeswalker line.
# Scryfall renders the negative sign as U+2212 (``−``); ``+`` and the
# bare-zero ``0:`` form share the same regex via the optional sign
# group. Capture group 1 is the signed cost. The cost-then-colon-then-
# space pattern is anchored at the start of the line so a colon embedded
# mid-effect (e.g. inside a triggered-ability body) doesn't match.
_LOYALTY_ACTIVATION_PREFIX = re.compile(r"^([+\-−]?\d+): (.+)$")


def _printed_loyalty_to_signed_cost(printed: str) -> int:
    """Convert a printed loyalty cost token (``"+2"``, ``"-3"``, ``"0"``,
    or the U+2212 minus form ``"−7"``) into the signed
    ``ActivationCost.loyalty`` convention: a ``+N`` ability is
    ``loyalty=-N`` (ADD), a ``-N`` ability is ``loyalty=+N`` (PAY),
    ``0`` is ``loyalty=0``. Mirrors Rust's sign convention."""
    # Normalize the U+2212 minus to ASCII hyphen.
    token = printed.replace("−", "-")
    if token.startswith("+"):
        # +N ability → loyalty = -N (add-loyalty).
        return -int(token[1:])
    # "-N" or bare "0" / "N": int() handles the leading hyphen.
    # A "-N" ability → loyalty = +N (pay-loyalty); "0" → 0.
    return -int(token)


# Literal-line table of Tezzeret, Cruel Captain's loyalty-ability effect
# bodies → engine Effect, consulted BEFORE the unified effect grammar.
# These three bodies predate the unified-grammar route and map to
# bespoke-shaped engine classes that the cross-engine parity suite pins
# (tests/test_tezzeret_cruel_captain_parity.py); they stay literal until
# a second card with the same untap-conditional-counter / small-artifact
# tutor / emblem surface lands. The -7 maps to a CreateEmblemWithAbilities
# carrying the emblem's begin-combat trigger inline (the emblem
# substrate — see effects/generic/emblem.py + docs/SEARCH_LIMITATIONS.md;
# the emblem's becomes-0/0-Robot permanent animate is honest-stopped
# there).
_LOYALTY_ABILITY_EFFECTS: dict[str, object] = {
    # Tezzeret, Cruel Captain's 0 ability.
    "Untap target artifact or creature. If it's an artifact creature, "
    "put a +1/+1 counter on it.":
        UntapTargetArtifactOrCreatureConditionalCounter(),
    # Tezzeret, Cruel Captain's -3 ability — tutor for an artifact card
    # with mana value 1 or less.
    "Search your library for an artifact card with mana value 1 or "
    "less, reveal it, put it into your hand, then shuffle.":
        SearchLibraryForCard(
            filter_name="artifact_small", max_mv=1, may_fail=True,
        ),
    # Tezzeret, Cruel Captain's -7 ability — "You get an emblem with
    # '...'."
    'You get an emblem with "At the beginning of combat on your turn, '
    "put three +1/+1 counters on target artifact you control. If it's "
    'not a creature, it becomes a 0/0 Robot artifact creature."':
        tezzeret_cruel_captain_emblem(),
}


def _parse_loyalty_effect(body: str, name: str):
    """Loyalty-ability body → ``Effect`` (``None`` if unparseable).

    The Tezzeret literal table wins; everything else goes through the
    unified spell-effect grammar. Planeswalkers refer to themselves by
    their short name (``Chandra deals 4 damage``); the short form is
    rewritten to the full printed name so the spell normalizer strips
    it as the source subject."""
    literal = _LOYALTY_ABILITY_EFFECTS.get(body)
    if literal is not None:
        return literal
    short = name.split(",", 1)[0]
    if short != name:
        body = re.sub(
            r"\b" + re.escape(short) + r"\b(?!,)", name, body,
        )
    effect = _parse_spell_effect_phrase(body, name)
    if effect is not None:
        return effect
    # Fallback (cube glue bucket): the walker named as a subject or
    # object MID-body ("If you don't, Chandra deals 2 damage ...", "the
    # number of loyalty counters on Comet") -- only the leading subject
    # is stripped by the spell normalizer, so rewrite the remaining
    # occurrences to "this planeswalker" and retry. Tried only after
    # the established path declined, so no claimed body changes.
    rest = body[len(name):] if body.startswith(name) else body
    if name not in rest:
        return None
    head = body[: len(body) - len(rest)]
    return _parse_spell_effect_phrase(
        head + rest.replace(name, "this planeswalker"), name,
    )


# ---------------------------------------------------------------------------
# Planeswalker statics
# ---------------------------------------------------------------------------

# Jace, Wielder of Mysteries — the only card in the pool with the
# "win instead of drawing from an empty library" replacement (Laboratory
# Maniac's templating); literal until a second printing varies it.
_DRAW_FROM_EMPTY_LIBRARY_WINS_LINE = (
    "If you would draw a card while your library has no cards in it, "
    "you win the game instead."
)
# Narset, Parter of Veils — only card in the pool with the per-turn
# draw cap on opponents; literal until a second printing varies the
# number / affected players.
_OPPONENTS_DRAW_AT_MOST_ONE_LINE = (
    "Each opponent can't draw more than one card each turn."
)
# Teferi, Time Raveler — only card in the pool with the opponents-cast-
# at-sorcery-speed restriction; literal until a second printing.
_OPPONENTS_SORCERY_SPEED_ONLY_LINE = (
    "Each opponent can cast spells only any time they could cast a "
    "sorcery."
)
# The Wandering Emperor: "As long as <name> entered this turn, you may
# activate <pronoun> loyalty abilities any time you could cast an
# instant." Group 1 must be the card's own name.
_INSTANT_SPEED_LOYALTY_IF_ENTERED_LINE = re.compile(
    r"^As long as (.+) entered this turn, you may activate "
    r"(?:his|her|its|their) loyalty abilities any time you could cast "
    r"an instant\.$"
)
# "<name> can be your commander." — a deck-construction permission
# (CR 903.3d) that only matters in the Commander variant. The engine
# plays two-player constructed/limited games with no command zone for
# commanders, so the clause has no in-game effect to model (it is not a
# dropped game rule — no game this engine plays can observe it).
_CAN_BE_YOUR_COMMANDER_LINE = re.compile(r"^(.+) can be your commander\.$")
# "When <subject> enters and at the beginning of <when>, <effect>." —
# a single printed trigger with two trigger events (Minsc & Boo). Each
# arm re-parses through the unified triggered-ability grammar.
_ENTERS_AND_AT_BEGINNING_OF_LINE = re.compile(
    r"^When (.+?) enters and at the beginning of (.+?), (.+)$"
)


def _two_arm_enters_and_upkeep_triggers(
    line: str, name: str,
) -> tuple[TriggeredAbility, ...] | None:
    m = _ENTERS_AND_AT_BEGINNING_OF_LINE.match(line)
    if m is None:
        return None
    subject, when, body = m.groups()
    arms: list[TriggeredAbility] = []
    for arm_line in (
        f"When {subject} enters, {body}",
        f"At the beginning of {when}, {body}",
    ):
        parsed = _parse_triggered_ability_line(arm_line, name=name)
        if parsed is None or isinstance(parsed, tuple):
            return None
        arms.append(parsed)
    return tuple(arms)


def _build_planeswalker_printing(card: dict) -> CardPrinting | None:
    """Parse a Scryfall planeswalker record into a ``CardPrinting`` via
    oracle-text recognition.

    Returns ``None`` if the card isn't a Planeswalker, has more than
    one printed subtype (the engine takes a single-subtype string),
    has no printed loyalty, or contains any unparseable line.
    """
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Planeswalker" not in card_types:
        return None
    if len(subtypes) > 1:
        return None
    loyalty_str = card.get("loyalty")
    if loyalty_str is None:
        return None
    try:
        loyalty = int(loyalty_str)
    except (TypeError, ValueError):
        return None
    name = card["name"]

    triggers: list[TriggeredAbility] = []
    activated: list[ActivatedAbility] = []
    statics: list[StaticAbility] = []
    keywords: set[str] = set()
    flags: dict[str, bool] = {}
    # Extra ``CardPrinting`` fields from the cube glue bucket's walker
    # lines, stamped on after ``make_planeswalker``.
    printing_fields: dict = {}
    # Die-roll loyalty ability: (index into ``activated``, sides) and its
    # result-table rows.
    die_ability: tuple | None = None
    die_rows: list = []
    for raw_line in card.get("oracle_text", "").split("\n"):
        line = _strip_reminder_text(raw_line)
        if not line:
            continue
        loyalty_match = _LOYALTY_ACTIVATION_PREFIX.match(line)
        if loyalty_match:
            cost_token, effect_body = loyalty_match.groups()
            effect = _parse_loyalty_effect(effect_body, name)
            if effect is None:
                # Roto P4: bodies of independent sentences.
                from mtgz_ir.cards.parser.roto_walkers import (
                    parse_roto_loyalty_body,
                )

                effect = parse_roto_loyalty_body(effect_body, name)
            if effect is None:
                # "Roll a six-sided die." followed by result-table lines
                # (Comet, Stellar Pup; cube glue bucket): the table is
                # assembled after the loop.
                from mtgz_ir.cards.parser.cube_glue_grammar import (
                    parse_glue_die_roll_body,
                )

                sides = parse_glue_die_roll_body(effect_body)
                if sides is None or die_ability is not None:
                    return None
                die_ability = (len(activated), sides)
                effect = effect_body  # placeholder, replaced below
            activated.append(
                ActivatedAbility(
                    cost=ActivationCost(
                        loyalty=_printed_loyalty_to_signed_cost(cost_token)
                    ),
                    effect=effect,  # type: ignore[arg-type]
                    sorcery_speed=True,
                )
            )
            continue
        from mtgz_ir.cards.parser.cube_glue_grammar import parse_glue_die_row

        die_row = parse_glue_die_row(line, name)
        if die_row is not None:
            die_rows.append(die_row)
            continue
        kw = _parse_keyword_line(line)
        if kw is not None:
            keywords.update(kw)
            continue
        if _parse_token_doubler_line(line):
            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                replacement_effect=ReplacementEffect(
                    event_pattern=EventPattern(
                        event=Event.WOULD_CREATE_TOKEN,
                    ),
                    transform=MultiplyCount(factor=2),
                ),
            ))
            continue
        if line == _DRAW_FROM_EMPTY_LIBRARY_WINS_LINE:
            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                replacement_effect=ReplacementEffect(
                    event_pattern=EventPattern(
                        event=Event.WOULD_DRAW_FROM_EMPTY_LIBRARY,
                    ),
                    transform=WinTheGameInstead(),
                ),
            ))
            continue
        if line == _OPPONENTS_DRAW_AT_MOST_ONE_LINE:
            flags["opponents_cant_draw_more_than_one_card_each_turn"] = True
            continue
        if line == _OPPONENTS_SORCERY_SPEED_ONLY_LINE:
            flags["opponents_cast_spells_only_at_sorcery_speed"] = True
            continue
        m = _INSTANT_SPEED_LOYALTY_IF_ENTERED_LINE.match(line)
        if m is not None:
            if m.group(1) != name:
                return None
            flags[
                "loyalty_abilities_at_instant_speed_if_entered_this_turn"
            ] = True
            continue
        m = _CAN_BE_YOUR_COMMANDER_LINE.match(line)
        if m is not None:
            if m.group(1) != name:
                return None
            continue
        # "Activated abilities of <types>[ your opponents control] can't
        # be activated." (Karn, the Great Creator) -- the activation-lock
        # recognizer the creature / artifact builders share.
        from mtgz_ir.cards.parser.creature_wg_lines import (
            _parse_activation_lock_line,
        )

        lock = _parse_activation_lock_line(line, card)
        if lock is not None:
            printing_fields.update(lock.fields)
            continue
        two_arm = _two_arm_enters_and_upkeep_triggers(line, name)
        if two_arm is not None:
            triggers.extend(two_arm)
            continue
        from mtgz_ir.cards.parser.cube_glue_grammar import (
            parse_glue_walker_line,
        )

        glue_fields = parse_glue_walker_line(line, name)
        if glue_fields is not None:
            printing_fields.update(glue_fields)
            continue
        # Roto P4: "−X" loyalty abilities and Compleated.
        from mtgz_ir.cards.parser.roto_walkers import parse_roto_walker_line

        roto = parse_roto_walker_line(line, name)
        if roto is not None:
            activated.extend(roto.activated)
            keywords.update(roto.keywords)
            continue
        parsed = _parse_triggered_ability_line(line, name=name)
        if parsed is None:
            return None
        if isinstance(parsed, tuple):
            triggers.extend(parsed)
        else:
            triggers.append(parsed)

    if die_ability is not None or die_rows:
        from mtgz_ir.cards.parser.cube_glue_grammar import assemble_die_table

        if die_ability is None:
            return None
        idx, sides = die_ability
        table = assemble_die_table(sides, die_rows)
        if table is None:
            return None
        import dataclasses as _dc

        activated[idx] = _dc.replace(activated[idx], effect=table)

    printing = make_planeswalker(
        name=name,
        cost=card.get("mana_cost", ""),
        loyalty=loyalty,
        subtype=subtypes[0] if subtypes else None,
        legendary="Legendary" in supertypes,
        triggered_abilities=tuple(triggers),
        activated_abilities=tuple(activated),
        static_abilities=tuple(statics),
        keywords=frozenset(keywords),
        **flags,
    )
    if printing_fields:
        from dataclasses import replace

        printing = replace(printing, **printing_fields)
    return printing
