"""Printing-level parametric keywords (Warp, Ward, Devour, Impending).

These take a numeric/mana-cost argument so they don't fit the
literal-line ``_KEYWORD_LINES`` map in ``keywords.py``. They share
the same shape: a keyword name followed by its argument on its own
line, with optional parenthesized reminder text. The argument
round-trips into the ``make_creature`` ``warp=`` / ``ward=`` /
``devour_land_multiplier=`` / ``impending=``+``impending_count=``
kwarg.

Two ``Ward`` non-mana-cost rider shapes are also recognized: the
em-dash discard-cost variants ``Ward—Discard a card.`` and
``Ward—Discard a card at random.`` (Sunset Saboteur and Alpharael
Stonechosen respectively). Each produces a sentinel return value
that the creature builder maps to the matching
``ward_discard_chosen=True`` / ``ward_discard_random=True`` kwarg
on ``make_creature``. Other riders (``Warp—{B}, Pay 2 life.``
additional-life-cost variants etc.) remain out of scope — they
intentionally don't match and cause the enclosing card to be
skipped.
"""

from __future__ import annotations

import re


_WARP_LINE = re.compile(
    r"^Warp (\{[^}]+\}(?:\{[^}]+\})*)(?: \([^)]*\))?$"
)


def _parse_warp_line(line: str) -> str | None:
    """Return the warp cost string (e.g. ``"{1}{R}"``) if ``line`` is a
    simple Warp clause, else ``None``. Lines with riders (additional
    life cost, graveyard-castable, etc.) intentionally don't match.
    """
    m = _WARP_LINE.match(line.strip())
    if m is None:
        return None
    return m.group(1)


# ``Warp—{cost}, Pay N life.`` — em-dash Warp variant with an
# additional life cost (Timeline Culler's printed surface form).
# The reminder text is stripped by ``_strip_reminder_text`` before this
# lookup runs. Only Timeline Culler carries this surface today; "broader
# grammar deferred until a second example exists" (a future card with a
# different life cost or colored warp cost would parameterize the N
# capture, but the shape is already parameterized — adding a new card
# with this surface is a zero-code change).
_WARP_WITH_LIFE_COST_LINE = re.compile(
    r"^Warp—(\{[^}]+\}(?:\{[^}]+\})*), Pay (\d+) life\.$"
)


def _parse_warp_with_life_cost_line(
    line: str,
) -> tuple[str, int] | None:
    """Return ``(cost_str, life_cost)`` if ``line`` is an em-dash Warp
    clause with an additional life cost (``Warp—{cost}, Pay N life.``),
    else ``None``. The creature builder wires ``cost_str`` to the
    ``warp`` kwarg and ``life_cost`` to ``warp_additional_life_cost``
    on ``make_creature``.
    """
    m = _WARP_WITH_LIFE_COST_LINE.match(line.strip())
    if m is None:
        return None
    return m.group(1), int(m.group(2))


# ``You may cast this card from your graveyard using its warp ability.``
# — Timeline Culler's printed graveyard-castability rider. The engine
# field ``warp_castable_from_graveyard: bool`` is consumed by the warp-
# cast pipeline; when True, the warp zone pool includes the graveyard
# as well as exile. Matched as a literal string — Timeline Culler is
# the only card in the pool with this exact surface today; deferred
# until a second example exists.
_WARP_CASTABLE_FROM_GRAVEYARD_LINE = (
    "You may cast this card from your graveyard using its warp ability."
)


def _parse_warp_castable_from_graveyard_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printed graveyard-castability
    rider for warp, else ``False``. Flips
    ``warp_castable_from_graveyard=True`` on ``make_creature``.
    """
    return line.strip() == _WARP_CASTABLE_FROM_GRAVEYARD_LINE


_WARD_LINE = re.compile(
    r"^Ward (\{[^}]+\}(?:\{[^}]+\})*)(?: \([^)]*\))?$"
)


def _parse_ward_line(line: str) -> str | None:
    """Return the ward cost string (e.g. ``"{2}"``) if ``line`` is a
    simple ``Ward {cost}`` clause, else ``None``. Lines with riders
    (em-dash + life cost, em-dash + discard) intentionally don't match.
    """
    m = _WARD_LINE.match(line.strip())
    if m is None:
        return None
    return m.group(1)


# ``Ward—Discard a card.`` / ``Ward—Discard a card at random.`` —
# em-dash discard-cost ward riders. Scryfall prints both variants
# verbatim with no surrounding reminder text. The two variants are
# distinguished only by the trailing ``at random`` qualifier; the
# function below returns a tagged string (``"chosen"`` or
# ``"random"``) so the creature builder can map it to the matching
# ``make_creature`` boolean kwarg without re-parsing.
_WARD_DISCARD_LINE = re.compile(
    r"^Ward—Discard a card(?P<random> at random)?\.$"
)


def _parse_ward_discard_line(line: str) -> str | None:
    """Return ``"chosen"`` for ``Ward—Discard a card.``, ``"random"``
    for ``Ward—Discard a card at random.``, or ``None`` if the line
    doesn't match either em-dash discard-cost ward shape.

    Two variants only — Scryfall consistently prints these exact
    surface forms; any other discard-cost ward (a future ``Ward—
    Discard two cards.`` printing, say) would land its own entry
    rather than parameterizing this one. Keeping the surface→tag
    mapping 1:1 with the engine's ``ward_discard_chosen`` /
    ``ward_discard_random`` boolean fields preserves parser/static
    parity by construction.
    """
    m = _WARD_DISCARD_LINE.match(line.strip())
    if m is None:
        return None
    return "random" if m.group("random") else "chosen"


_WARD_PAY_LIFE_LINE = re.compile(r"^Ward—Pay (\d+) life\.$")


def _parse_chooses_player_line(line: str) -> bool:
    """``As this creature enters, choose a player.``"""
    return line.strip() == "As this creature enters, choose a player."


_SELF_HAS_KEYWORDS = re.compile(r"^This creature has (?P<body>.+)\.$")


def _parse_self_has_keywords_line(line: str) -> frozenset[str] | None:
    """``This creature has <keywords>.`` — a static that just states a
    keyword the card always has (``This creature has protection from
    the chosen player.``)."""
    from mtgz_ir.cards.parser.effects import _parse_keyword_grant_body

    m = _SELF_HAS_KEYWORDS.match(line.strip())
    if m is None:
        return None
    return _parse_keyword_grant_body(m.group("body"))
_KEYWORDS_THEN_WARD = re.compile(
    r"^(?P<kws>.+), ward (?P<cost>\{[^}]+\}(?:\{[^}]+\})*)$"
)


def _parse_keyword_line_with_ward(line: str) -> tuple[tuple[str, ...], str] | None:
    """``<keyword line>, ward {cost}`` -> ``(keywords, ward_cost)``."""
    from mtgz_ir.cards.parser.keywords import _parse_keyword_line

    m = _KEYWORDS_THEN_WARD.match(line.strip())
    if m is None:
        return None
    kws = _parse_keyword_line(m.group("kws"))
    if kws is None:
        return None
    return kws, m.group("cost")


def _parse_ward_pay_life_line(line: str) -> int | None:
    """``Ward—Pay N life.`` (CR 702.21a with a life-payment cost) ->
    ``N``; consumed by ``actions._apply_ward_check`` via
    ``CardPrinting.ward_life_cost``."""
    m = _WARD_PAY_LIFE_LINE.match(line.strip())
    return int(m.group(1)) if m is not None else None


_IMPENDING_LINE = re.compile(
    # Impending N—{cost}. Scryfall uses an em-dash (U+2014). The
    # reminder text is stripped per-line by ``_strip_reminder_text``
    # before this lookup runs, so we match the bare keyword line.
    r"^Impending (\d+)—(\{[^}]+\}(?:\{[^}]+\})*)$"
)


def _parse_impending_line(line: str) -> tuple[int, str] | None:
    """Return ``(N, cost_str)`` if ``line`` is an Impending clause
    (``Impending N—{cost}``), else ``None``.

    Matches the verbatim Duskmourn Overlord cycle surface form. The
    reminder text — "(If you cast this spell for its impending cost,
    it enters with N time counters and isn't a creature until the
    last is removed. At the beginning of your end step, remove a
    time counter from it.)" — is stripped via the existing per-line
    reminder-text stripper before this lookup runs, so we only have
    to match the bare keyword line.
    """
    m = _IMPENDING_LINE.match(line.strip())
    if m is None:
        return None
    return int(m.group(1)), m.group(2)


_DEVOUR_LINE = re.compile(
    r"^Devour land (\d+)(?: \([^)]*\))?$"
)


def _parse_devour_line(line: str) -> int | None:
    """Return the devour multiplier ``N`` (an int) if ``line`` is a
    simple ``Devour land N`` clause, else ``None``. The EoE printing
    is Famished Worldsire's ``Devour land 3`` (sacrifice any number of
    lands; enter with three times that many +1/+1 counters). Lines
    with riders or non-``land`` Devour variants intentionally don't
    match — the enclosing card falls through and is skipped.
    """
    m = _DEVOUR_LINE.match(line.strip())
    if m is None:
        return None
    return int(m.group(1))


# ``This creature enters tapped.`` — printing-level static replacement
# (CR 614) flipping the existing ``enters_tapped: bool`` field on the
# card printing. The engine already consults this field at ETB time
# (``actions.put_into_battlefield`` and ``effects/combinators.py``);
# the parser side has been missing the line recognizer. Surfaces on
# Piranha Fly's two-line oracle text alongside Flying.
#
# A literal-string match is sufficient — no card today varies the
# wording. A future printing whose surface text differs (``This
# permanent enters tapped`` for a non-creature, or a numeric variant)
# would land its own entry rather than parameterizing this one,
# matching the ``Ward—Discard`` precedent above where each surface
# gets its own 1:1 recognizer.
#
# Parameterized over the self-reference noun so the artifact builder's
# ``This artifact enters tapped.`` (Time Vault, Wizard's Rockets) claims
# through the same recognizer.
_ENTERS_TAPPED_LINE = re.compile(
    r"^This (?:creature|artifact|Vehicle|Equipment) "
    r"enters tapped\.$"
)


def _parse_enters_tapped_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level ``This <noun>
    enters tapped.`` clause, else ``False``. Used by
    ``creature_builder._build_creature_printing`` and the artifact
    builder to flip the ``enters_tapped`` kwarg directly — distinct
    from the triggered-ability path because the ETB-tapped behavior is
    a static replacement effect, not an event-firing trigger.
    """
    return _ENTERS_TAPPED_LINE.match(line.strip()) is not None


# ``This artifact doesn't untap during your untap step.`` — printing-
# level static (Grim Monolith, Basalt Monolith, Mana Vault, Time Vault).
# Flips ``CardPrinting.doesnt_untap_during_untap_step``, honored by
# ``turn.run_untap_step``. Parameterized over the self-reference noun.
_DOESNT_UNTAP_LINE = re.compile(
    r"^This (?:artifact|creature|land|permanent) doesn't untap during "
    r"your untap step\.$"
)


def _parse_doesnt_untap_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the ``This <noun> doesn't untap
    during your untap step.`` static, else ``False``."""
    return _DOESNT_UNTAP_LINE.match(line.strip()) is not None


# ``This creature must be blocked if able.`` — printing-level static
# block-requirement (CR 509.1c). Surfaces on Fear of Being Hunted
# alongside Haste. The matching engine flag
# (``CardPrinting.must_be_blocked_if_able``) is consumed by
# ``actions._declare_blockers``: if no validated blocker is assigned
# to the attacker and at least one eligible defender exists that
# could legally block it, the engine forces one. A literal-string
# match is sufficient; future related surfaces (``This creature can't
# be blocked except by …``) would land their own recognizers.
_MUST_BE_BLOCKED_LINE = "This creature must be blocked if able."


def _parse_must_be_blocked_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level ``This creature
    must be blocked if able.`` clause, else ``False``. Used by
    ``creature_builder._build_creature_printing`` to flip the
    ``must_be_blocked_if_able`` kwarg on ``make_creature``.
    """
    return line.strip() == _MUST_BE_BLOCKED_LINE


# ``Nonbasic lands are Mountains.`` — Magus of the Moon's printed
# static effect. The engine wires the layer-4 type-replacement off
# the printing-level ``nonbasic_lands_are_mountains`` flag (rather
# than a generic ``StaticAbility`` shape, since the type replacement
# isn't a creature-relative continuous effect). Matched as a literal
# string; a future card with the same shape but different source
# (Blood Moon-style enchantment) would route through whatever
# oracle-text parser owns its card type and reuse the same flag.
_NONBASIC_LANDS_ARE_MOUNTAINS_LINE = "Nonbasic lands are Mountains."


def _parse_nonbasic_lands_are_mountains_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level
    ``Nonbasic lands are Mountains.`` clause, else ``False``. Flips the
    ``nonbasic_lands_are_mountains`` kwarg on ``make_creature`` /
    ``make_enchantment``; the engine reads the flag at land ETB and
    layer-4 type-replacement application time.
    """
    return line.strip() == _NONBASIC_LANDS_ARE_MOUNTAINS_LINE


# ``Each player can't cast more than one noncreature spell each turn.``
# — Deafening Silence's printed static. The engine wires the cast-
# pipeline gate off the printing-level
# ``restricts_noncreature_spells_to_one_per_turn`` flag (rather than a
# generic ``StaticAbility`` shape, since the cast-restriction lives
# pre-stack and isn't a creature-relative continuous effect). Matched
# as a literal string; a future card with a similar restriction would
# get its own recognizer here.
_RESTRICTS_NONCREATURE_SPELLS_LINE = (
    "Each player can't cast more than one noncreature spell each turn."
)


def _parse_restricts_noncreature_spells_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level Deafening
    Silence static, else ``False``. Flips the
    ``restricts_noncreature_spells_to_one_per_turn`` kwarg on
    ``make_enchantment``; the engine reads the flag in the cast pipeline
    to gate noncreature spell casts after the first per turn.
    """
    return line.strip() == _RESTRICTS_NONCREATURE_SPELLS_LINE


# ``You may look at face-down creatures your opponents control any
# time.`` — Found Footage's printed informational static. Flips the
# printing-level ``looks_at_opponents_face_down_creatures: bool`` flag
# on ``make_artifact``. The engine doesn't model face-down hidden
# information today (every game object is fully visible to every
# player in the simulation), so the flag carries the printed surface
# for fidelity but has no engine consequence on resolution. We
# nevertheless wire the line as a printing-keyword recognizer rather
# than dropping it, so the round-trip from oracle text to printing is
# faithful — when a future change lands face-down hidden-information
# modeling, this flag becomes the gate the new code keys off.
#
# Literal-string match — Found Footage is the only DSK/EoE card with
# this exact surface today, "broader grammar deferred until a second
# example exists" (a future Clue or Curse with the same look-grant
# would either land its own recognizer or, if a parameter varies,
# parameterize this one).
_LOOKS_AT_OPPONENTS_FACE_DOWN_LINE = (
    "You may look at face-down creatures your opponents control any time."
)


def _parse_looks_at_opponents_face_down_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level Found Footage
    look-at-face-down static, else ``False``. Flips the
    ``looks_at_opponents_face_down_creatures`` kwarg on
    ``make_artifact``; the engine reads the flag for hidden-information
    handling (currently a no-op since face-down state isn't modeled).
    """
    return line.strip() == _LOOKS_AT_OPPONENTS_FACE_DOWN_LINE


# ``If one or more tokens would be created under your control, twice
# that many of those tokens are created instead.`` — Exalted Sunborn's
# printed token-doubler replacement effect. The parser builds a
# SELF-scope ``StaticAbility`` carrying a ``ReplacementEffect(
# event_pattern=EventPattern(WOULD_CREATE_TOKEN), transform=
# MultiplyCount(2))``; the dispatcher's implicit "source's controller
# is the affected player" gate handles the "under your control"
# qualifier. Matched as a literal string — Exalted Sunborn is the
# only card in the pool with this exact surface today; "broader
# grammar deferred until a second example exists" (e.g. a hypothetical
# Anointed-Procession-style enchantment with the same shape would land
# its own recognizer or, if the multiplier varies, parameterize this
# one).
_TOKEN_DOUBLER_LINE = (
    "If one or more tokens would be created under your control, "
    "twice that many of those tokens are created instead."
)


def _parse_token_doubler_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level token-doubler
    replacement clause, else ``False``. Drives a SELF-scope
    ``StaticAbility(replacement_effect=ReplacementEffect(...,
    MultiplyCount(2)))`` declaration in the calling parser; the engine
    walks ``static_abilities`` at create-token resolution time and
    doubles the count.
    """
    return line.strip() == _TOKEN_DOUBLER_LINE


# ``If a permanent entering the battlefield causes a triggered ability
# of a permanent you control to trigger, that ability triggers an
# additional time.`` — Starfield Vocalist's printed ETB-trigger
# doubler. The parser builds a SELF-scope ``StaticAbility`` carrying a
# ``ReplacementEffect(EventPattern(TRIGGERED_ABILITY_TRIGGERS,
# subject_filters={"ability_source": IsPermanent(), "entering_obj":
# IsPermanent()}), AddInstances(1))``; the dispatcher's implicit
# "source's controller is the affected player" gate handles "you
# control". Matched as a literal string — Starfield Vocalist is the
# only card in the pool with this exact surface today; "broader
# grammar deferred until a second example exists" (a hypothetical
# Panharmonicon-style printing with the same shape would either land
# its own recognizer or, if a parameter such as the trigger count
# varies, parameterize this one).
_EXTRA_ETB_TRIGGER_LINE = (
    "If a permanent entering the battlefield causes a triggered "
    "ability of a permanent you control to trigger, that ability "
    "triggers an additional time."
)


def _parse_extra_etb_trigger_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level
    Panharmonicon-style ETB-trigger doubler clause, else ``False``.
    Drives a SELF-scope ``StaticAbility(replacement_effect=
    ReplacementEffect(..., AddInstances(1)))`` declaration in the
    calling parser; the engine walks ``static_abilities`` at trigger
    enqueue time and adds the replacement's contribution.
    """
    return line.strip() == _EXTRA_ETB_TRIGGER_LINE


# ``As long as you have one or fewer cards in hand, if you would draw
# one or more cards, you draw that many cards plus one instead.`` —
# Quantum Riddler's printed draw-replacement static. The parser builds
# a SELF-scope ``StaticAbility(condition=HandSizeAtMost(1),
# replacement_effect=ReplacementEffect(WOULD_DRAW_CARDS,
# AddToCount(1)))`` declaration; the dispatcher's implicit "source's
# controller is the affected player" gate handles "you would draw"
# and the StaticAbility.condition handles "as long as you have one
# or fewer cards in hand". Matched as a literal string — Quantum
# Riddler is the only card in the pool with this exact surface
# today; "broader grammar deferred until a second example exists" (a
# future printing whose hand-size threshold differs would
# parameterize this recognizer's threshold capture).
_REPLACES_DRAW_WHEN_HAND_AT_MOST_ONE_LINE = (
    "As long as you have one or fewer cards in hand, if you would "
    "draw one or more cards, you draw that many cards plus one "
    "instead."
)


def _parse_replaces_draw_when_hand_at_most_one_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level draw-replacement
    clause for hand-size at-most-one, else ``False``. Drives a
    SELF-scope ``StaticAbility(condition=HandSizeAtMost(1),
    replacement_effect=ReplacementEffect(..., AddToCount(1)))``
    declaration in the calling parser; the engine walks
    ``static_abilities`` at draw resolution time and adds the
    replacement's contribution when the condition holds.
    """
    return line.strip() == _REPLACES_DRAW_WHEN_HAND_AT_MOST_ONE_LINE


# ``As long as an artifact entered the battlefield under your control
# this turn, this creature can attack as though it didn't have
# defender.`` — Mechan Shieldmate's printed defender-bypass static.
# The caller (creature_builder) builds a SELF-scope StaticAbility
# carrying ``ContinuousModifier(layer=6, payload=
# RemoveKeyword("defender"))`` gated on
# ``ControlledArtifactEnteredThisTurn``; ``effective_keywords`` then
# returns a set without "defender" while the gate holds and
# ``eligible_attackers``'s defender check passes naturally. Matched
# as a literal string — Mechan Shieldmate is the only card in the
# pool with this exact surface today; "broader grammar deferred
# until a second example exists" (a future printing whose triggering
# condition differs — ``a creature entered``, ``a Spacecraft
# entered``, etc. — would parameterize this recognizer's
# permanent-type capture).
_ATTACK_BYPASS_IF_ARTIFACT_ENTERED_LINE = (
    "As long as an artifact entered the battlefield under your control "
    "this turn, this creature can attack as though it didn't have "
    "defender."
)


def _parse_attack_bypass_if_artifact_entered_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level
    ``As long as an artifact entered the battlefield under your control
    this turn, this creature can attack as though it didn't have
    defender.`` clause, else ``False``. The caller builds a SELF-scope
    ``StaticAbility(condition=ControlledArtifactEnteredThisTurn(),
    continuous_effect=ContinuousModifier(layer=6,
    payload=RemoveKeyword("defender")))``; ``effective_keywords``
    drops "defender" while the gate holds and the source attacks
    naturally through ``eligible_attackers``.
    """
    return line.strip() == _ATTACK_BYPASS_IF_ARTIFACT_ENTERED_LINE


# ``This creature can't be blocked if you've cast two or more spells
# this turn.`` — Illvoi Infiltrator's printed conditional-unblockable
# static. The caller (creature_builder) builds a SELF-scope
# StaticAbility carrying ``ContinuousModifier(layer=6,
# payload=CantBeBlocked())`` gated on
# ``AtLeastNSpellsCastThisTurn(2)``. CantBeBlocked is a layer-6
# continuous gameplay-action restriction (structurally distinct from
# real keyword grants); ``combat.can_block`` /
# ``combat.declare_blockers`` reach it via ``game.is_cant_be_blocked``,
# which walks ``walk_continuous_modifiers`` for the payload. Matched
# as a literal string — Illvoi Infiltrator is the only card in the
# pool with this exact surface today; "broader grammar deferred
# until a second example exists" (a future printing whose threshold
# differs — ``three or more spells``, ``a noncreature spell``, etc.
# — would parameterize this recognizer's threshold/spell-type
# capture).
_UNBLOCKABLE_IF_TWO_SPELLS_CAST_THIS_TURN_LINE = (
    "This creature can't be blocked if you've cast two or more spells "
    "this turn."
)


def _parse_unblockable_if_two_spells_cast_this_turn_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level
    ``This creature can't be blocked if you've cast two or more spells
    this turn.`` clause, else ``False``. The caller builds a SELF-scope
    ``StaticAbility(condition=AtLeastNSpellsCastThisTurn(2),
    continuous_effect=ContinuousModifier(layer=6,
    payload=CantBeBlocked()))``; ``game.is_cant_be_blocked`` walks
    the modifier registry and short-circuits block legality while
    the gate holds.
    """
    return line.strip() == _UNBLOCKABLE_IF_TWO_SPELLS_CAST_THIS_TURN_LINE


# Dynamic P/T formula recognizer. The engine stores a ``PTFormula``
# (``dynamic_pt_formula``, ``mtgz_ir.effects.pt_formulas``) on the
# ``CardPrinting``; ``game._dynamic_base_pt`` asks it for the actual P/T
# at query time. Each distinct oracle surface maps to its formula via the
# table below; adding a new card is one entry, no new function.
#
# ``*``-based power/toughness cards (Cosmogoyf ``*/(1+*)``; Harmonious
# Grovestrider ``*/*``) get 0 / ``offset`` as their placeholder P/T in
# the grammar path — consistent with the bespoke-table printings.
#
# The Emissary Escort entry stays for backwards compatibility; it ships
# with integer P/T (1/2) so no ``*`` handling is needed for it.
def _dynamic_pt_formula_lines() -> dict:
    from mtgz_ir.effects.pt_formulas import (
        CardTypesInAllGraveyardsPT,
        CardsYouOwnInExilePT,
        LandsYouControlPT,
        PowerPlusGreatestManaValueOfOtherArtifactsPT,
    )

    return {
    # Emissary Escort {2}{W} 1/2 — printed power incremented by the
    # greatest MV among other artifacts you control. Only card today
    # with this exact surface; parameterization deferred until a second
    # example (e.g. "+X/+0 where X is the number of creatures…") lands.
    "This creature gets +X/+0, where X is the greatest mana value "
    "among other artifacts you control.": (
        PowerPlusGreatestManaValueOfOtherArtifactsPT()
    ),
    # Cosmogoyf {B}{G} */(1+*) Elemental Lhurgoyf — power = exile count,
    # toughness = exile count + 1. Only card with this formula today;
    # deferred until a second lhurgoyf-style formula lands.
    "Cosmogoyf's power is equal to the number of cards you own in "
    "exile and its toughness is equal to that number plus 1.": (
        CardsYouOwnInExilePT(toughness_plus=1)
    ),
    # Harmonious Grovestrider {3}{G}{G} */* Beast — P/T each = lands
    # you control. Only card with this formula today; deferred until a
    # second land-count P/T formula lands.
    "Harmonious Grovestrider's power and toughness are each equal to "
    "the number of lands you control.": LandsYouControlPT(),
    # Barrowgoyf {2}{B} */(1+*) Lhurgoyf -- card types among cards in
    # ALL graveyards (the classic Tarmogoyf formula; the parameterized
    # ``creature_rmc_grammar`` recognizer builds the same formula).
    "Barrowgoyf's power is equal to the number of card types among cards "
    "in all graveyards and its toughness is equal to that number plus 1.": (
        CardTypesInAllGraveyardsPT(toughness_plus=1)
    ),
    }


_DYNAMIC_PT_FORMULA_LINES: dict = _dynamic_pt_formula_lines()


def _parse_dynamic_pt_formula_line(line: str):
    """Return the ``PTFormula`` if ``line`` is a known dynamic-P/T
    oracle surface, else ``None``. The formula is forwarded as the
    ``dynamic_pt_formula`` kwarg on ``make_creature``; the engine asks
    it for the actual P/T at query time (``game._dynamic_base_pt``).

    Adding a new card is one entry in ``_DYNAMIC_PT_FORMULA_LINES``;
    parameterized shapes (any name) fall back to
    ``creature_rmc_grammar.parse_rmc_dynamic_pt_line`` and then
    ``creature_wg_lines.parse_wg_dynamic_pt_line`` (cube W/G bucket).
    """
    formula = _DYNAMIC_PT_FORMULA_LINES.get(line.strip())
    if formula is not None:
        return formula
    from mtgz_ir.cards.parser.creature_rmc_grammar import (
        parse_rmc_dynamic_pt_line,
    )

    rmc = parse_rmc_dynamic_pt_line(line.strip())
    if rmc is not None:
        return rmc
    from mtgz_ir.cards.parser.creature_wg_lines import parse_wg_dynamic_pt_line

    wg = parse_wg_dynamic_pt_line(line.strip())
    if wg is not None:
        return wg
    # Vintage Roto P5 (Nethergoyf: "card types among cards in your
    # graveyard").
    from mtgz_ir.cards.parser.roto_board import parse_roto_dynamic_pt_line

    return parse_roto_dynamic_pt_line(line.strip())


def _parse_emissary_escort_dynamic_pt_line(line: str):
    """Alias retained for backwards compatibility. Delegates to
    ``_parse_dynamic_pt_formula_line``.
    """
    return _parse_dynamic_pt_formula_line(line)


# ``This creature enters with X +1/+1 counters on it.`` — Broodguard
# Elite's printed ETB counter rider where X equals the X cost paid to
# cast it. The engine field ``enters_with_counters_from_x="x"`` is
# consumed in ``actions.put_into_battlefield``: it reads
# ``state.resolving_x_value`` and calls ``add_plus_one_counters``.
# Matched as a literal string — Broodguard Elite is the only card in
# the pool with this exact surface today; "broader grammar deferred
# until a second example exists" (a future printing with a different
# counter type or gating condition would land its own recognizer).
_ENTERS_WITH_X_COUNTERS_LINE = "This creature enters with X +1/+1 counters on it."


def _parse_enters_with_x_counters_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printing-level
    ``This creature enters with X +1/+1 counters on it.`` ETB-counter
    rider, else ``False``. Flips ``enters_with_counters_from_x="x"``
    kwarg on ``make_creature``; the engine resolves the X count from
    ``state.resolving_x_value`` at ETB time.
    """
    return line.strip() == _ENTERS_WITH_X_COUNTERS_LINE


# Basic-landcycling: ``<Land>cycling {cost}`` where ``<Land>`` is one of
# the five basic-land subtypes. CR 702.29 typecycling — while the card
# is in the controller's hand, they may pay the cycling cost and discard
# this card to search their library for a basic-land card of the named
# subtype, reveal it, put it into their hand, then shuffle. The subtype
# capitalizes per the printed line (``Plains`` / ``Island`` / ``Swamp``
# / ``Mountain`` / ``Forest``); the cost is one or more brace-wrapped
# mana symbols. The line's parenthesized reminder text is stripped by
# the per-line ``_strip_reminder_text`` step before this lookup runs,
# so the regex matches the bare keyword head only.
#
# Each of the five basic-land subtypes shares the same engine field
# layout (``CardPrinting.landcycling = (subtype, cost_str)``), so this
# single recognizer covers the whole DSK landcycling-{2} cycle
# (Shepherding Spirits, Daggermaw Megalodon, Spectral Snatcher, Bedhead
# Beastie, Slavering Branchsnapper). A future printing whose oracle
# uses non-basic typecycling (``Spiritcycling``, ``Wizardcycling``)
# would need its own recognizer — this one anchors on the five basic-
# land subtypes by name to keep the grammar's surface form 1:1 with
# the engine field's expected ``land_subtype`` values.
_LANDCYCLING_LINE = re.compile(
    r"^(?P<subtype>Plains|Island|Swamp|Mountain|Forest)cycling "
    r"(?P<cost>\{[^}]+\}(?:\{[^}]+\})*)$"
)


_ENTERS_WITH_PLUS_ONE_IF_DEATH_LINE = (
    "This creature enters with a +1/+1 counter on it if a creature died "
    "this turn."
)


def _parse_enters_with_plus_one_counter_if_creature_died_line(
    line: str,
) -> bool:
    """Return ``True`` if ``line`` is the printing-level ``This creature
    enters with a +1/+1 counter on it if a creature died this turn.``
    clause, else ``False``.

    Surfaces on Cackling Slasher's two-line oracle alongside Deathtouch.
    A literal-string match is sufficient — no card today varies the
    wording. The matching engine field
    (``CardPrinting.enters_with_plus_one_counter_if_creature_died_this_
    turn``) is consumed in ``actions.put_into_battlefield``, which
    reads ``state.creatures_died_this_turn`` (incremented in
    ``turn.move_off_battlefield`` and reset at cleanup) and adds a
    +1/+1 counter when the tracker is positive.
    """
    return line.strip() == _ENTERS_WITH_PLUS_ONE_IF_DEATH_LINE


# Spelled-cardinal -1/-1 ETB rider for cast-from-hand. Surfaces on
# Patched Plaything (DSK Toy) as
# ``This creature enters with two -1/-1 counters on it if you cast it
# from your hand.``. The cardinal is captured so a future Plaything
# variant with a different count (one, three, etc.) would map to the
# same engine field with the appropriate value. The literal-string
# fallback for ``two`` matches the only printing today; if a future
# printing prints the digit ``2`` the regex's ``\d+`` arm picks it up
# without any grammar change.
_ENTERS_WITH_MINUS_ONE_FROM_HAND_LINE = re.compile(
    r"^This creature enters with "
    r"(?P<count>\d+|one|two|three|four|five|six|seven|eight|nine|ten) "
    r"-1/-1 counters? on it if you cast it from your hand\.$"
)

_NUMBER_WORD_TO_INT: dict[str, int] = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def _parse_enters_with_minus_one_counters_from_hand_cast_line(
    line: str,
) -> int | None:
    """Return ``N`` (the count of -1/-1 counters) if ``line`` is the
    printing-level ``This creature enters with N -1/-1 counters on it
    if you cast it from your hand.`` clause, else ``None``.

    Patched Plaything is the only DSK printing carrying this surface
    (``two`` -1/-1 counters). The regex accepts both spelled-out
    cardinals (one through ten) and a digit fallback so a forward-
    looking printing with a different count or numerical surface
    lands the same field. The matching engine field
    (``CardPrinting.enters_with_minus_one_counters_from_hand_cast``)
    is consumed in ``actions.put_into_battlefield`` only when the
    GameObject's ``cast_via_hand`` flag is set — the cast-from-exile
    / library / graveyard paths clear that flag, so the rider stays
    inert on those resolutions (matching the printed "if you cast
    it from your hand" gate).
    """
    m = _ENTERS_WITH_MINUS_ONE_FROM_HAND_LINE.match(line.strip())
    if m is None:
        return None
    raw = m.group("count")
    if raw.isdigit():
        return int(raw)
    return _NUMBER_WORD_TO_INT[raw]


def _parse_landcycling_line(line: str) -> tuple[str, str] | None:
    """Return ``(land_subtype, mana_cost_str)`` if ``line`` is a
    basic-landcycling clause (``<Land>cycling {cost}``), else ``None``.

    The five basic-land subtypes are matched by name; non-basic
    typecycling (no DSK printing today) intentionally doesn't match
    and falls through. The mana-cost capture preserves the printed
    cost string verbatim — e.g. ``{2}`` for the DSK cycle, ``{1}{R}``
    for any forward-looking hybrid-cost printing — which the engine
    field stores as-is for future activation-pipeline support.

    Reminder text is stripped via the existing per-line
    ``_strip_reminder_text`` helper before this lookup runs, so the
    regex only has to match the bare keyword head.
    """
    m = _LANDCYCLING_LINE.match(line.strip())
    if m is None:
        return None
    return m.group("subtype"), m.group("cost")


# ``You may play <N> additional land(s) on each of your turns.`` — the
# printed static land-drop grant, parameterized over the count: Icetill
# Explorer / Exploration ("an additional land"), Azusa, Lost but Seeking
# ("two additional lands"). The engine field
# ``grants_additional_land_per_turn: int`` is consumed by the turn
# engine when counting legal land plays; the printing increments the
# controller's per-turn quota by this value.
_ADDITIONAL_LANDS_PER_TURN_RE = re.compile(
    r"^You may play (?P<n>an|two|three|four) additional lands? on each of "
    r"your turns\.$"
)
_ADDITIONAL_LAND_COUNTS = {"an": 1, "two": 2, "three": 3, "four": 4}


def parse_additional_lands_per_turn(line: str) -> "int | None":
    """The extra land plays a ``You may play <N> additional land(s) on
    each of your turns.`` static grants, or ``None``."""
    m = _ADDITIONAL_LANDS_PER_TURN_RE.match(line.strip())
    return None if m is None else _ADDITIONAL_LAND_COUNTS[m.group("n")]


def _parse_additional_land_per_turn_line(line: str) -> bool:
    """Return ``True`` if ``line`` is an additional-land-plays static
    (``parse_additional_lands_per_turn`` gives the count)."""
    return parse_additional_lands_per_turn(line) is not None


# ``You may play lands from your graveyard.`` — Icetill Explorer's
# printed lands-from-graveyard static. The engine field
# ``grants_play_lands_from_graveyard: bool`` is consumed by the land-
# play pipeline to allow the controller to play land cards from their
# graveyard as well as their hand. Matched as a literal string — only
# card in the pool with this surface today; deferred until a second
# example exists.
_PLAY_LANDS_FROM_GRAVEYARD_LINE = "You may play lands from your graveyard."


def _parse_play_lands_from_graveyard_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the ``You may play lands from
    your graveyard.`` static, else ``False``. Flips the
    ``grants_play_lands_from_graveyard`` kwarg on ``make_creature``.
    """
    return line.strip() == _PLAY_LANDS_FROM_GRAVEYARD_LINE


# ``This spell can't be countered.`` — printing-level static immunity to
# counter effects. The engine field ``spell_is_uncounterable: bool`` is
# checked in ``CounterTargetSpell.legal_targets`` and
# ``CounterTargetSpell.resolve``; spells with this flag don't appear in
# the legal-target list and the counter fizzles if somehow targeted
# between targeting and resolution. Matched as a literal string — Frenzied
# Baloth is the only card in the parser pool with this exact surface
# today; broader grammar deferred until a second example exists (a future
# instant/sorcery with this clause would reuse the same field without any
# grammar change needed since the surface is already parameterless).
_SPELL_CANT_BE_COUNTERED_LINE = "This spell can't be countered."


def _parse_spell_cant_be_countered_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the ``This spell can't be countered.``
    clause, else ``False``. Flips the ``spell_is_uncounterable`` kwarg on
    ``make_creature``; ``CounterTargetSpell`` checks the flag at targeting
    and resolve time.
    """
    return line.strip() == _SPELL_CANT_BE_COUNTERED_LINE


# ``Creature spells you control can't be countered.`` — static ability
# printed on a permanent that grants uncounterability to all creature
# spells the controller casts while this permanent is on the battlefield.
# The engine checks for a ``StaticAbility`` with
# ``grants_creature_spells_uncounterable=True`` on the controller's
# battlefield permanents inside ``CounterTargetSpell``. Matched as a
# literal string — Frenzied Baloth is the only card in the parser pool
# with this exact surface today; broader grammar deferred until a second
# example exists (a hypothetical "Noncreature spells you control can't be
# countered." variant would need its own field and recognizer, making this
# surface already a degenerate but specific shape).
_CREATURE_SPELLS_UNCOUNTERABLE_LINE = (
    "Creature spells you control can't be countered."
)


def _parse_creature_spells_uncounterable_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the
    ``Creature spells you control can't be countered.`` clause, else
    ``False``. The creature builder wires this as a ``StaticAbility``
    with ``grants_creature_spells_uncounterable=True`` and scope
    ``StaticScope.SELF`` (the ability doesn't buff permanents — it's
    a spell-on-stack protection; ``SELF`` is used only so the existing
    ``applies_to`` walk is a no-op and the static doesn't inadvertently
    grant keywords to the source creature).
    """
    return line.strip() == _CREATURE_SPELLS_UNCOUNTERABLE_LINE


# ``Combat damage can't be prevented.`` — printing-level static that
# causes damage events from this creature to bypass global combat-damage
# prevention effects (e.g. Darkness). The engine field
# ``combat_damage_cant_be_prevented: bool`` is checked per-source in
# ``combat._resolve_damage_step`` when ``state.prevent_combat_damage_this_turn``
# is True. Matched as a literal string — Frenzied Baloth is the only card
# in the parser pool with this exact surface today; broader grammar
# deferred until a second example exists.
_COMBAT_DAMAGE_CANT_BE_PREVENTED_LINE = "Combat damage can't be prevented."


def _parse_combat_damage_cant_be_prevented_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the
    ``Combat damage can't be prevented.`` clause, else ``False``. Flips the
    ``combat_damage_cant_be_prevented`` kwarg on ``make_creature``; the
    engine checks the flag in ``combat._source_bypasses_prevention`` to
    let this creature's damage events through even when
    ``state.prevent_combat_damage_this_turn`` is set.
    """
    return line.strip() == _COMBAT_DAMAGE_CANT_BE_PREVENTED_LINE


# ``Each creature you control with toughness greater than its power assigns
# combat damage equal to its toughness rather than its power.`` — CR 510.1b
# replacement effect printed on Tapestry Warden. Matched as a literal string
# — Tapestry Warden is the only card in the parser pool with this exact
# surface today; broader grammar (parameterizing subject/comparison/
# assignment) deferred until a second example exists.
_TOUGHNESS_AS_DAMAGE_LINE = (
    "Each creature you control with toughness greater than its power "
    "assigns combat damage equal to its toughness rather than its power."
)


def _parse_toughness_as_damage_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the toughness-as-damage clause, else
    ``False``. Flips the
    ``grants_toughness_as_damage_for_creatures_you_control`` kwarg on
    ``make_creature``; the engine checks the flag in
    ``combat._effective_combat_damage_amount`` when computing how much
    damage a creature assigns.
    """
    return line.strip() == _TOUGHNESS_AS_DAMAGE_LINE


# ``Each creature you control with toughness greater than its power stations
# permanents using its toughness rather than its power.`` — CR 502.30-adjacent
# station-power replacement effect printed on Tapestry Warden. Matched as a
# literal string — Tapestry Warden is the only card in the parser pool with
# this exact surface today; broader grammar deferred until a second example
# exists.
_TOUGHNESS_AS_STATION_POWER_LINE = (
    "Each creature you control with toughness greater than its power "
    "stations permanents using its toughness rather than its power."
)


def _parse_toughness_as_station_power_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the toughness-as-station-power clause,
    else ``False``. Flips the
    ``grants_toughness_as_station_power_for_creatures_you_control`` kwarg on
    ``make_creature``; the engine checks the flag in
    ``actions._station_spacecraft`` when computing how many charge counters
    a stationing creature contributes.
    """
    return line.strip() == _TOUGHNESS_AS_STATION_POWER_LINE


# ── Dyadrine "enters with mana-spent counters" ────────────────────────────────
#
# ``Dyadrine enters with a number of +1/+1 counters on it equal to the
# amount of mana spent to cast it.`` — Dyadrine, Synthesis Amalgam's
# printed ETB replacement effect. The engine field
# ``enters_with_counters_from_x="mana_spent"`` is consumed in
# ``actions.put_into_battlefield``: it computes the total mana paid
# (X + generic + colored pips) and calls ``add_plus_one_counters``.
# Matched as a literal string after name substitution replaces the card
# name with "this creature" — but Scryfall prints the card's own name
# here, so the recognizer matches the name-substituted form (the
# creature-builder pipeline runs ``_substitute_self_name`` on trigger
# lines but not on printing-keyword lines). The card name check in the
# builder handles name-specific surfaces; matching the whole printed
# sentence as a literal is the right approach (only Dyadrine carries
# this shape today; broader grammar deferred until a second example
# exists).
_ENTERS_WITH_MANA_SPENT_COUNTERS_LINE = (
    "Dyadrine enters with a number of +1/+1 counters on it equal to "
    "the amount of mana spent to cast it."
)


def _parse_enters_with_mana_spent_counters_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the printed ETB replacement for
    Dyadrine, Synthesis Amalgam — ``Dyadrine enters with a number of
    +1/+1 counters on it equal to the amount of mana spent to cast it.``
    — else ``False``. Flips ``enters_with_counters_from_x="mana_spent"``
    kwarg on ``make_creature``; the engine resolves the counter count
    from the total mana paid (X + generic + colored pips) at ETB time.

    Only Dyadrine in the current pool has this shape; deferred until a
    second example exists.
    """
    return line.strip() == _ENTERS_WITH_MANA_SPENT_COUNTERS_LINE


# ── Mm'menon "you may look at the top card" ──────────────────────────────────
#
# ``You may look at the top card of your library any time.`` — Mm'menon,
# the Right Hand's printed passive information-access clause. In
# simulation both players have perfect information, so this flag does
# not change engine behavior; it is recorded faithfully on the printing
# so the parser does not silently drop a printed oracle line (per
# parser/CLAUDE.md: "a no-op recognizer is the same kind of dishonest
# shortcut as a synthetic-behavior factory"). The engine field
# ``grants_look_at_top_of_library: bool`` exists on ``CardPrinting``
# as a faithful oracle-text anchor.
#
# Matched as a literal string — Mm'menon is the only card in the pool
# with this exact surface today; deferred until a second example exists.
_LOOK_AT_TOP_CARD_ANY_TIME_LINE = (
    "You may look at the top card of your library any time."
)


def _parse_look_at_top_card_any_time_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the ``You may look at the top card
    of your library any time.`` clause, else ``False``. Flips the
    ``grants_look_at_top_of_library`` kwarg on ``make_creature``; the
    field records the printed oracle clause without changing simulation
    behavior (both players have perfect information in the sim).
    """
    return line.strip() == _LOOK_AT_TOP_CARD_ANY_TIME_LINE


# ── "You may cast <kind> spells [and <kind> spells] from the top of your
# library." ────────────────────────────────────────────────────────────────
#
# The library-top cast grant, parameterized over the spell kinds:
# Mm'menon, the Right Hand ("artifact spells") and Mystic Forge
# ("artifact spells and colorless spells"). ``parse_cast_from_top_kinds``
# returns the kinds in printed order. The artifact-only grant keeps its
# ``grants_cast_artifact_from_top_of_library: bool`` field (consumed by
# ``legal_actions`` / ``actions._cast_spell``); other kind lists are the
# Vintage Roto P3 bucket's ``CardPrinting.cast_from_top_of_library_kinds``.
_CAST_FROM_TOP_RE = re.compile(
    r"^You may cast (?P<kinds>(?:artifact|colorless) spells(?: and "
    r"(?:artifact|colorless) spells)?) from the top of your library\.$"
)


def parse_cast_from_top_kinds(line: str) -> "tuple[str, ...] | None":
    """The spell kinds of a library-top cast grant line, or ``None``."""
    m = _CAST_FROM_TOP_RE.match(line.strip())
    if m is None:
        return None
    return tuple(re.findall(r"(artifact|colorless) spells", m.group("kinds")))


def _parse_cast_artifact_from_top_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the artifact-only library-top cast
    grant (``You may cast artifact spells from the top of your
    library.``), else ``False``. Flips the
    ``grants_cast_artifact_from_top_of_library`` kwarg on
    ``make_creature``; ``legal_actions`` reads the flag to offer
    top-of-library artifact casts when this permanent is on the
    battlefield.
    """
    return parse_cast_from_top_kinds(line) == ("artifact",)


# ── Mm'menon "artifacts you control have {T}: Add {U}" ───────────────────────
#
# ``Artifacts you control have "{T}: Add {U}. Spend this mana only to
# cast a spell from anywhere other than your hand."`` — Mm'menon's
# printed static-grants-activated-ability clause. The engine field
# ``grants_blue_mana_ability_to_your_artifacts: bool`` is consumed by
# ``game.effective_activated_abilities`` to append the pre-built
# ``MMNEMON_BLUE_MANA_GRANTED_ABILITY`` to artifact activations for the
# controller.
#
# Matched as a literal string — the clause text includes a Scryfall-
# printed quoted ability description and is unique to Mm'menon in the
# pool today; deferred until a second card with this surface shape
# appears (a future card granting a different mana color would
# parameterize the color capture).
_ARTIFACTS_HAVE_BLUE_MANA_ABILITY_LINE = (
    'Artifacts you control have "{T}: Add {U}. Spend this mana only '
    'to cast a spell from anywhere other than your hand."'
)


def _parse_artifacts_have_blue_mana_ability_line(line: str) -> bool:
    """Return ``True`` if ``line`` is the Mm'menon artifact blue-mana
    grant clause, else ``False``. Flips the
    ``grants_blue_mana_ability_to_your_artifacts`` kwarg on
    ``make_creature``; ``game.effective_activated_abilities`` reads the
    flag to append ``MMNEMON_BLUE_MANA_GRANTED_ABILITY`` to artifact
    activations for the controller.
    """
    return line.strip() == _ARTIFACTS_HAVE_BLUE_MANA_ABILITY_LINE


# ``Artifacts you control are Foods in addition to their other types and
# have "{2}, {T}, Sacrifice this artifact: You gain 3 life."`` — Ragost,
# Deft Gastronaut's dual static: grants the Food subtype AND the standard
# Food sac ability to every artifact under the controller's control.
# The quoted inner ability is the canonical Food activated ability text.
# Matched as a literal string — Ragost is the only card in the parser
# pool with this exact combined Food-type-plus-ability grant today;
# broader grammar deferred until a second example exists. Sets both
# ``grants_food_to_your_artifacts=True`` and
# ``grants_food_ability_to_your_artifacts=True`` on ``make_creature``
# via the sentinel return value; the creature builder maps both booleans.
_GRANTS_FOOD_AND_ABILITY_TO_YOUR_ARTIFACTS_LINE = (
    'Artifacts you control are Foods in addition to their other types '
    'and have "{2}, {T}, Sacrifice this artifact: You gain 3 life."'
)


def _parse_grants_food_and_ability_line(line: str) -> bool:
    """Return ``True`` if ``line`` is Ragost's combined Food-type-and-
    ability grant clause, else ``False``. The creature builder uses the
    True result to set both ``grants_food_to_your_artifacts=True`` and
    ``grants_food_ability_to_your_artifacts=True`` on ``make_creature``.
    """
    return line.strip() == _GRANTS_FOOD_AND_ABILITY_TO_YOUR_ARTIFACTS_LINE


# ``<Type(s)> cards in your hand have warp {cost}.`` — Tannuk, Steadfast
# Second's hand-warp grant. The subject phrase describes the type filter
# (e.g. ``"Artifact cards and red creature cards"``); the cost is a mana
# cost string. Both are parsed: the type filter into a structured
# ``HandCardFilter`` (see ``cards.base``), and the cost into a
# ``ManaCost``. The pair becomes a ``HandAbilityGrant`` on the printing's
# ``hand_ability_grants`` tuple, which ``legal.py`` walks at cast time
# to offer warp-cast actions for matching hand cards.
#
# The type filter currently parses two surface forms used in the EoE/DSK
# pool: "Artifact cards" (type-only) and "<color> creature cards"
# (type + color), joined by " and " when both apply. Future cards with
# different filter forms (e.g. "Instant cards", "Spells with mana value
# 3 or less in your hand") would extend the parser here.
_HAND_WARP_GRANT_LINE = re.compile(
    r"^(.+?) cards in your hand have warp "
    r"(\{[^}]+\}(?:\{[^}]+\})*)(?: \([^)]*\))?(?:\.)?"
    r"$"
)


_HAND_WARP_FILTER_COLORS = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}


def _parse_hand_card_filter_phrase(phrase: str):
    """Parse a hand-card-filter phrase like ``"Artifact"`` or
    ``"red creature"`` into a single ``(card_type, color | None)``
    predicate tuple. Returns ``None`` if the phrase doesn't match a
    supported shape.

    Supported forms:
    - ``"Artifact"`` → ``("Artifact", None)``
    - ``"<color> creature"`` → ``("Creature", "<COLOR>")``
    - ``"Creature"`` → ``("Creature", None)`` (color-agnostic)

    Future shapes (e.g. ``"Instant"``, ``"<color> Instant"``,
    ``"Spell"``) extend this function. Each new shape is one new
    branch — no engine changes.
    """
    parts = phrase.strip().split()
    # "<color> creature" form: e.g. "red creature"
    if len(parts) == 2 and parts[0].lower() in _HAND_WARP_FILTER_COLORS:
        color = _HAND_WARP_FILTER_COLORS[parts[0].lower()]
        if parts[1].lower() == "creature":
            return ("Creature", color)
    # Bare type form: "Artifact" / "Creature" / "Instant" / "Sorcery" /
    # "Enchantment".
    if len(parts) == 1:
        cap = parts[0].capitalize()
        if cap in {"Artifact", "Creature", "Instant", "Sorcery",
                   "Enchantment", "Land", "Planeswalker"}:
            return (cap, None)
    return None


def _parse_hand_warp_grant_line(line: str):
    """Return a ``HandAbilityGrant`` if ``line`` is a
    ``<types> cards in your hand have warp {cost}.`` clause, else
    ``None``. The creature builder stores the grant in
    ``hand_ability_grants`` on ``make_creature``.

    The type-filter phrase is split on ``" and "`` to support the
    Tannuk shape ``"Artifact cards and red creature cards"`` (a
    disjunction of two predicates). Each subphrase parses via
    ``_parse_hand_card_filter_phrase``; if any subphrase fails to
    parse, the whole line is rejected so the parser doesn't silently
    accept a partial filter.
    """
    from mtgz_ir.cards.base import (
        HandAbilityGrant, HandCardFilter, parse_mana_cost,
    )
    m = _HAND_WARP_GRANT_LINE.match(line.strip())
    if m is None:
        return None
    filter_str, cost_str = m.group(1), m.group(2)
    # Strip trailing " cards" from each subphrase (the regex captures
    # ``Artifact cards and red creature cards`` — the type word is what
    # we care about).
    subphrases = [s.strip() for s in filter_str.split(" and ")]
    predicates: list[tuple[str, "str | None"]] = []
    for sub in subphrases:
        # Each subphrase ends in "cards"; drop that suffix before parsing
        # the type/color shape.
        if sub.lower().endswith(" cards"):
            sub = sub[: -len(" cards")]
        elif sub.lower().endswith(" card"):
            sub = sub[: -len(" card")]
        pred = _parse_hand_card_filter_phrase(sub)
        if pred is None:
            return None
        predicates.append(pred)
    return HandAbilityGrant(
        card_filter=HandCardFilter(predicates=tuple(predicates)),
        granted_warp_cost=parse_mana_cost(cost_str),
        granted_warp_cost_str=cost_str,
    )


# ── Cycling family (CR 702.29) ────────────────────────────────────────
#
# ``Cycling {cost}``                 → discard this card: draw a card.
# ``<Subtype>cycling {cost}``        → discard this card: search your
#                                      library for a <Subtype> card,
#                                      reveal it, put it into your
#                                      hand, then shuffle (landcycling
#                                      ``Plainscycling`` / typecycling
#                                      ``Wizardcycling``).
# ``Basic landcycling {cost}``       → ... for a basic land card.
#
# All three are hand-activated abilities with the cost "{cost},
# Discard this card", instant speed. They land on
# ``CardPrinting.hand_activated_abilities`` (see ``parser.source`` — the hand-
# ability lines are peeled off once for every card type before the
# per-type builder runs).
_CYCLING_LINE = re.compile(
    r"^(?:(?P<basic>Basic land)|(?P<sub>[A-Z][a-z]+))?cycling "
    r"(?P<cost>\{[^}]+\}(?:\{[^}]+\})*)$"
)
_CYCLING_BARE_LINE = re.compile(
    r"^Cycling (?P<cost>\{[^}]+\}(?:\{[^}]+\})*)$"
)


def _parse_cycling_line(line: str) -> "ActivatedAbility | None":
    """Return the hand-activated ``ActivatedAbility`` for a cycling-
    family keyword line, else ``None``."""
    from mtgz_ir.cards.base import (
        ActivatedAbility, ActivationCost, DrawCards, parse_mana_cost,
    )
    from mtgz_ir.effects.generic.search import SearchLibraryForCard

    stripped = line.strip()
    bare = _CYCLING_BARE_LINE.match(stripped)
    if bare is not None:
        cost_str = bare.group("cost")
        effect = DrawCards(amount=1)
    else:
        m = _CYCLING_LINE.match(stripped)
        if m is None or not (m.group("basic") or m.group("sub")):
            return None
        cost_str = m.group("cost")
        if m.group("basic"):
            effect = SearchLibraryForCard(filter_name="basic_land")
        else:
            effect = SearchLibraryForCard(
                filter_name="subtype", subtypes=(m.group("sub"),),
            )
    return ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost(cost_str),
            mana_str=cost_str,
            discard_self=True,
        ),
        effect=effect,
    )
# Mox Diamond's entry replacement: ``If this artifact would enter, you
# may discard a land card instead. If you do, put this artifact onto
# the battlefield. If you don't, put it into its owner's graveyard.``
# Flips ``CardPrinting.enters_only_by_discarding_land``, honored at the
# top of ``actions.put_into_battlefield``. Literal: Mox Diamond is the
# only card in the pool with this replacement; a second "discard a
# <type> card instead" printing would parameterize the card type.
_DISCARD_LAND_TO_ENTER_LINE = (
    "If this artifact would enter, you may discard a land card instead. "
    "If you do, put this artifact onto the battlefield. If you don't, "
    "put it into its owner's graveyard."
)


def _parse_discard_land_to_enter_line(line: str) -> bool:
    """Return ``True`` for Mox Diamond's discard-a-land entry
    replacement line."""
    return line.strip() == _DISCARD_LAND_TO_ENTER_LINE


# ``This artifact enters with three wish counters on it.`` (Wishclaw
# Talisman) / ``This Vehicle enters with three charge counters on it.``
# (Reckoner Bankbuster) — an entry replacement placing named counters.
# Returns ``(kind, amount)`` for ``CardPrinting.enters_with_named_counters``.
# ``+1/+1`` counters on creatures keep their dedicated recognizers in
# ``triggers.py``; this one is consulted by the artifact builder only.
_ENTERS_WITH_NAMED_COUNTERS_LINE = re.compile(
    r"^This (?:artifact|Vehicle|Equipment) enters with "
    r"(a|an|one|two|three|four|five|six|seven|eight|nine|ten|\d+) "
    r"([a-z]+) counters? on it\.$"
)
_ENTERS_WITH_CARDINALS: dict[str, int] = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4,
    "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def _parse_enters_with_named_counters_line(
    line: str,
) -> tuple[str, int] | None:
    m = _ENTERS_WITH_NAMED_COUNTERS_LINE.match(line.strip())
    if m is None:
        return None
    token = m.group(1)
    amount = int(token) if token.isdigit() else _ENTERS_WITH_CARDINALS[token]
    return (m.group(2), amount)


# Time Vault's turn-skip replacement. Literal: Time Vault is the only
# card in the pool with an "if you would begin your turn" replacement;
# broader grammar deferred until a second example exists.
_SKIP_TURN_TO_UNTAP_LINE = (
    "If you would begin your turn while this artifact is tapped, you may "
    "skip that turn instead. If you do, untap this artifact."
)


def _parse_skip_turn_to_untap_line(line: str) -> bool:
    """Return ``True`` for Time Vault's turn-skip replacement line."""
    return line.strip() == _SKIP_TURN_TO_UNTAP_LINE


# ``Crew N`` (CR 702.122) — the Vehicle keyword ability. Reminder text
# is stripped before this sees the line. Returns N; the artifact
# builder turns it into the crew ``ActivatedAbility``.
_CREW_LINE = re.compile(r"^Crew (\d+)$")


def _parse_crew_line(line: str) -> int | None:
    m = _CREW_LINE.match(line.strip())
    return int(m.group(1)) if m is not None else None


# ``Sunburst`` (CR 702.44) — keyword line (reminder text stripped).
# Flips ``CardPrinting.sunburst``; the counters are placed by
# ``actions.put_into_battlefield`` from the colors recorded at cast.
def _parse_sunburst_line(line: str) -> bool:
    return line.strip() == "Sunburst"


# Bolas's Citadel's play-from-top grant with its life-payment
# alternative cost. Literal (only card in the pool with this grant);
# flips ``CardPrinting.grants_play_from_top_paying_life``.
_PLAY_FROM_TOP_PAYING_LIFE_LINE = (
    "You may play lands and cast spells from the top of your library. If "
    "you cast a spell this way, pay life equal to its mana value rather "
    "than pay its mana cost."
)


def _parse_play_from_top_paying_life_line(line: str) -> bool:
    return line.strip() == _PLAY_FROM_TOP_PAYING_LIFE_LINE


# ── Deck-construction statements (CR 100.2a / 100.4) ───────────────────────
#
# Rules text that only constrains how a DECK is built -- never anything a
# game in progress can observe: "This card is banned." (Gleemox, whose
# printed text makes it illegal in every sanctioned format) and the
# "A deck can have any number of / up to N cards named <this card>." family
# (the deck-size exceptions of CR 100.2a). The engine plays already-built
# decks, so there is no in-game behavior to model: the recognizer is the
# exact class of deckbuilding statements (see the "glue" section of
# docs/SEARCH_LIMITATIONS.md), not a generic unrecognized-line escape. A
# "cards named X" statement must name the card itself.
_DECK_CONSTRUCTION_STATEMENT_LINE = re.compile(
    r"^(?:This card is banned|A deck can have (?:any number of|up to "
    r"(?:one|two|three|four|five|six|seven|eight|nine|ten|\d+)) cards named "
    r"(?P<name>.+))\.$"
)


def _parse_deck_construction_statement_line(line: str, name: str) -> bool:
    """``True`` if ``line`` is a deck-construction statement about the
    card ``name`` (see ``_DECK_CONSTRUCTION_STATEMENT_LINE``)."""
    m = _DECK_CONSTRUCTION_STATEMENT_LINE.match(line.strip())
    if m is None:
        return False
    named = m.group("name")
    return named is None or named == name
