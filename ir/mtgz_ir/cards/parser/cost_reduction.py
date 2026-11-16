"""Spell cost-reduction rider parser.

Recognizes a single oracle-text line of the shape::

    This spell costs {N} less to cast if <condition>.

and produces a ``CostReduction(amount=N, condition=<Condition>)`` that
callers attach to a ``CardPrinting`` via the ``cost_reductions`` kwarg
on ``make_creature`` / ``make_instant`` / ``make_sorcery``.

The same rider appears on both creatures (Gigastorm Titan, Lashwhip
Predator) and noncreature spells (Dark Endurance, Temporal
Intervention), so the recognizer is factored out here and reused by
both the creature builder and the spell builder.

Variable-amount riders of the shape::

    This spell costs {X} less to cast, where X is <land-group-phrase>.

are also recognized via the ``PerDifferentlyNamedLand`` condition
(Fungal Colossus). The reduction is computed at cast time by the
engine's ``apply_cost_reductions`` — the unit ``amount=1`` is
multiplied by ``condition.multiplier`` (the count of distinctly named
lands the controller controls).

``Affinity for <type>`` keyword lines are recognized by the
``_parse_affinity_line`` function at the bottom of this module.
Reminder text is stripped before that function is called, so the
bare ``Affinity for Slivers`` form (not the parenthesized reminder)
reaches the recognizer. Produces ``CostReduction(amount=1,
condition=PerControlled(HasSubtype("<type>")))`` per CR 702.41.

Out of scope this slice:

* Rider-only conditions used by activated abilities (Thaumaton
  Torpedo's ``This ability costs {3} less to activate if …``) —
  those live on ``ActivatedAbility.cost_reductions``, parsed
  elsewhere.

Adding a new condition: extend ``_COST_REDUCTION_CONDITIONS`` with the
verbatim oracle phrase mapped to a structural ``Condition`` object.
The ``Condition`` is consumed directly by
``actions._evaluate_cost_reduction`` via the single-dispatch path —
no engine-side entry needs to be added in lockstep.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import CostReduction, ExternalCostReduction
from mtgz_ir.effects.conditions import (
    SpellCastFromGraveyard,
    SpellCastFromOutsideHand,
    AnotherSpellCastThisTurn,
    Condition,
    DuringYourTurn,
    NthSpellThisTurn,
    OpponentsControlAtLeast,
    PerControlled,
    PerBasicLandTypeAmongLandsYouControl,
    PerDifferentlyNamedLand,
    TargetMatches,
    VoidActiveCondition,
)
from mtgz_ir.effects.object_predicates import (
    HasSubtype,
    IsArtifact,
    IsBlocking,
    IsCreature,
    IsTapped,
)


# Surface form: ``This spell costs {N} less to cast if <condition>.``
# (with the ``if`` clause) and ``This spell costs {N} less to cast
# <connector-phrase>.`` (without ``if``, e.g. ``...less to cast during
# your turn.``). Two regexes rather than one regex with optional ``if``
# because a missing ``if`` puts the connector right after ``cast`` and
# the recognized connector list is a closed set — keeping them
# separate makes the lookup against the closed set explicit.
# ``N`` is restricted to one or more digits; the ``{X}``/per-land-group
# variants don't match and fall through to the static side.
#
# A leading ``Void — `` prefix on the rider (Temporal Intervention) is
# stripped via a third regex variant; the body's ``if`` clause body is
# the canonical Void-state phrase, which the recognizer maps to the
# ``VoidActiveCondition`` ``Condition``. The Void-prefix variant is its
# own regex (rather than letting the ``if`` clause body match a free-
# form Void description) because the prefix and the engine condition
# are tightly coupled — a future Void-prefixed cost-reduction with a
# different body would still want the ``VoidActiveCondition`` mapping,
# since every Void rider gates on the same state.
_COST_REDUCTION_LINE = re.compile(
    r"^This spell costs \{(\d+)\} less to cast if (?P<condition>.+)\.$"
)
_COST_REDUCTION_LINE_NO_IF = re.compile(
    r"^This spell costs \{(\d+)\} less to cast (?P<condition>.+)\.$"
)
_COST_REDUCTION_LINE_VOID_PREFIX = re.compile(
    r"^Void — This spell costs \{(\d+)\} less to cast "
    r"if a nonland permanent left the battlefield this turn "
    r"or a spell was warped this turn\.$"
)
# Variable-amount X-cost reduction. Fungal Colossus is the only EoE
# printing today carrying this shape; the verbatim line is::
#
#   ``This spell costs {X} less to cast, where X is the number of
#     differently named lands you control.``
#
# Maps to ``CostReduction(amount=1, condition=PerDifferentlyNamedLand
# ())`` — the engine's ``apply_cost_reductions`` multiplies the unit
# amount by the condition's ``multiplier`` (the count of distinctly
# named lands the controller controls at cast time). Literal-line
# recognizer with no capture groups — the entire surface is the
# parameter; broader grammar (parameterizing the land-group counter on
# a different distinctness criterion, or pointing at a different
# battlefield set such as creatures or artifacts) is deferred until a
# second printing with a varying piece exists.
_COST_REDUCTION_LINE_X_PER_DIFFERENTLY_NAMED_LAND = (
    "This spell costs {X} less to cast, where X is the number of "
    "differently named lands you control."
)


_COST_REDUCTION_LINE_FOR_EACH = re.compile(
    r"^(?:[A-Z][a-z]+ — )?This spell costs \{(\d+)\} less to cast "
    r"for each (?P<what>.+)\.$"
)
# Counted things for the ``for each <what>`` shape -> variable-amount
# ``Condition`` (``multiplier`` = the count).
_COST_REDUCTION_PER_EACH: dict[str, Condition] = {
    "basic land type among lands you control": (
        PerBasicLandTypeAmongLandsYouControl()
    ),
}


def _per_each_condition(what: str):
    """The counted thing's condition, or ``None``. Roto P4 adds "card
    type among cards in your graveyard" (Emrakul, the Promised End)."""
    condition = _COST_REDUCTION_PER_EACH.get(what)
    if condition is not None:
        return condition
    if what == "card type among cards in your graveyard":
        from mtgz_ir.effects.generic.roto_walkers import (
            PerCardTypeInYourGraveyard,
        )

        return PerCardTypeInYourGraveyard()
    return None


# Conditions recognized as cost-reduction triggers. The mapping is from
# the verbatim oracle-text phrase (after the ``if`` and before the
# trailing period) to a structural ``Condition`` object. Each
# ``Condition`` is consumed directly by
# ``actions._evaluate_cost_reduction``; no engine-side string-key
# entry is required.
_COST_REDUCTION_CONDITIONS: dict[str, Condition] = {
    "you've cast another spell this turn": AnotherSpellCastThisTurn(),
    "your opponents control three or more creatures": (
        OpponentsControlAtLeast(IsCreature(), 3)
    ),
    "it targets a blocking creature": (
        TargetMatches(IsCreature() & IsBlocking())
    ),
    # Seized from Slumber (Duskmourn) — parallel to ``it targets a
    # blocking creature`` shape but for the tapped state.
    "it targets a tapped creature": (
        TargetMatches(IsCreature() & IsTapped())
    ),
}


# Conditions on the no-``if`` shape — same condition→engine-key
# mapping, but the surface text reads naturally without the ``if``
# (e.g. ``"during your turn"`` rather than ``"if it's your turn"``).
# Keys are the verbatim text between ``cast `` and the trailing
# period. Each value is a structural ``Condition`` consumed directly
# by ``actions._evaluate_cost_reduction``.
_COST_REDUCTION_CONDITIONS_NO_IF: dict[str, Condition] = {
    "during your turn": DuringYourTurn(),
}


# External cost-reduction lines — a permanent-on-the-battlefield grants
# a flat reduction to OTHER spells the controller casts that match the
# stated condition. Distinct from the per-spell ``CostReduction`` rider
# above (which lives on the spell being cast); the external variant
# lives on the granting permanent's printing and feeds
# ``apply_cost_reductions``'s scan over the caster's battlefield.
#
# Currently recognized surfaces:
#
# * ``The second spell you cast each turn costs {N} less to cast.`` —
#   Uthros Psionicist's static. Maps to
#   ``ExternalCostReduction(amount=N, condition=NthSpellThisTurn(2))``.
#
# Adding a new surface: extend ``_EXTERNAL_COST_REDUCTION_LINES`` with
# the verbatim oracle phrase mapped to a structural ``Condition``.
_EXTERNAL_COST_REDUCTION_LINES: dict[str, Condition] = {
    # Uthros Psionicist — fires when the controller is casting their
    # second spell of the turn.
    "The second spell you cast each turn costs {N} less to cast.": (
        NthSpellThisTurn(2)
    ),
    # Bilbo, Thief in the Night.
    "Spells you cast from anywhere other than your hand cost {N} less to "
    "cast.": SpellCastFromOutsideHand(),
    # Green Goblin.
    "Spells you cast from your graveyard cost {N} less to cast.": (
        SpellCastFromGraveyard()
    ),
}


# ``<Subject>. (<reminder>)`` printing-flag-style external cost-
# reductions whose surface form is just a static-keyword sentence
# (no embedded ``{N}`` template). The amount is implied by the
# keyword's reminder text and is wired here to a structural
# ``Condition`` directly. Reminder text is stripped before this lookup
# runs, so the trailing parenthetical (``(They cost {1} less to cast
# for each artifact you control.)``) is gone by the time we see the
# line.
_EXTERNAL_COST_REDUCTION_KEYWORD_LINES: dict[str, tuple[int, Condition]] = {
    # Sami, Wildcat Captain — affinity-for-artifacts on every spell
    # the controller casts. The ``PerControlled(IsArtifact())``
    # condition's ``multiplier`` returns the artifact count; the
    # engine's ``apply_cost_reductions`` scan multiplies ``amount``
    # (1) by it.
    "Spells you cast have affinity for artifacts.": (
        1, PerControlled(IsArtifact()),
    ),
}


# Compiled lookup keyed on the integer-template form. We keep the
# template as the dict key (with the literal ``{N}`` placeholder) so
# the recognizer's surface→Condition mapping reads next to the
# template; the regex is built off the keys so adding a new surface
# is one entry rather than two.
_EXTERNAL_COST_REDUCTION_RE_TO_CONDITION: dict[
    "re.Pattern[str]", Condition
] = {
    re.compile(
        "^" + re.escape(template).replace(r"\{N\}", r"\{(\d+)\}") + "$"
    ): condition
    for template, condition in _EXTERNAL_COST_REDUCTION_LINES.items()
}


def _parse_external_cost_reduction_line(
    line: str,
) -> ExternalCostReduction | None:
    """Return an ``ExternalCostReduction`` for a recognized printed
    static-cost-reduction line, or ``None`` if the line doesn't match
    any registered surface form. The caller (creature_builder) routes
    the result through the ``external_cost_reductions`` kwarg on
    ``make_creature``.
    """
    stripped = line.strip()
    for pattern, condition in (
        _EXTERNAL_COST_REDUCTION_RE_TO_CONDITION.items()
    ):
        m = pattern.match(stripped)
        if m is not None:
            return ExternalCostReduction(
                amount=int(m.group(1)), condition=condition,
            )
    keyword_match = _EXTERNAL_COST_REDUCTION_KEYWORD_LINES.get(stripped)
    if keyword_match is not None:
        amount, condition = keyword_match
        return ExternalCostReduction(amount=amount, condition=condition)
    return None


def _parse_cost_reduction_line(line: str) -> CostReduction | None:
    """Return a ``CostReduction`` for a single oracle-text line of the
    shape ``This spell costs {N} less to cast if <condition>.``,
    ``This spell costs {N} less to cast <connector>.`` (Mental
    Modulation's ``...less to cast during your turn.``),
    ``Void — This spell costs {N} less to cast if a nonland
    permanent left the battlefield this turn or a spell was warped
    this turn.`` (Temporal Intervention), or
    ``This spell costs {X} less to cast, where X is the number of
    differently named lands you control.`` (Fungal Colossus), or
    ``None`` if the line shape doesn't match or the condition phrase
    isn't on the recognized whitelist.

    The caller is responsible for stripping reminder text and trimming
    surrounding whitespace before calling this helper, matching the
    convention used elsewhere in the parser.
    """
    stripped = line.strip()
    # Vintage Roto P3 conditions ("Morbid — ... if a creature died this
    # turn.").
    from mtgz_ir.cards.parser.roto_mana import parse_roto_cost_reduction_line

    roto = parse_roto_cost_reduction_line(stripped)
    if roto is not None:
        return roto
    # ``[<Ability word> — ]This spell costs {N} less to cast for each
    # <counted thing>.`` (Leyline Binding's ``Domain —`` reduction). The
    # ability word has no rules meaning (CR 207.2c); the counted thing
    # resolves through ``_COST_REDUCTION_PER_EACH`` to a variable-amount
    # condition whose ``multiplier`` scales the unit amount.
    m = _COST_REDUCTION_LINE_FOR_EACH.match(stripped)
    if m is not None:
        condition = _per_each_condition(m.group("what"))
        if condition is None:
            # Roto P2 counted things ("card you've drawn this turn").
            from mtgz_ir.cards.parser.roto_zones import (
                roto_zones_per_each_condition,
            )

            condition = roto_zones_per_each_condition(m.group("what"))
        if condition is None:
            return None
        return CostReduction(amount=int(m.group(1)), condition=condition)
    # Per-differently-named-land variable-amount X-cost reduction
    # (Fungal Colossus). Tried first because the ``{X} less`` template
    # is a unique gate that can't be confused with the ``{N} less``
    # numeric-template shapes below; recognizing the whole rider in
    # one shot keeps the canonical multiplier mapping adjacent to the
    # literal surface check.
    if stripped == _COST_REDUCTION_LINE_X_PER_DIFFERENTLY_NAMED_LAND:
        return CostReduction(
            amount=1, condition=PerDifferentlyNamedLand(),
        )
    # Void-prefix shape (Temporal Intervention). Tried first because
    # the ``Void — `` prefix is a unique gate; recognizing the whole
    # rider in one shot keeps the canonical Void-state mapping
    # adjacent to the prefix detection. The ``if`` clause body is
    # part of the regex literal so a non-canonical body wouldn't
    # silently map to ``VoidActiveCondition``.
    m = _COST_REDUCTION_LINE_VOID_PREFIX.match(stripped)
    if m is not None:
        return CostReduction(
            amount=int(m.group(1)), condition=VoidActiveCondition(),
        )
    m = _COST_REDUCTION_LINE.match(stripped)
    if m is not None:
        condition_phrase = m.group("condition").strip()
        condition = _COST_REDUCTION_CONDITIONS.get(condition_phrase)
        if condition is None:
            # Roto P1: "it targets a <spell np>" (Mystical Dispute).
            from mtgz_ir.cards.parser.roto_stack import (
                roto_cost_reduction_condition,
            )

            condition = roto_cost_reduction_condition(condition_phrase)
        if condition is None:
            return None
        return CostReduction(
            amount=int(m.group(1)), condition=condition,
        )
    # No-``if`` shape (Mental Modulation): the closed connector list
    # gates which surface phrases qualify so an unknown trailing
    # phrase falls through and the spell is left unclaimed rather
    # than silently mapping to a nonsense condition.
    m = _COST_REDUCTION_LINE_NO_IF.match(stripped)
    if m is None:
        return None
    condition_phrase = m.group("condition").strip()
    condition = _COST_REDUCTION_CONDITIONS_NO_IF.get(condition_phrase)
    if condition is None:
        return None
    return CostReduction(
        amount=int(m.group(1)), condition=condition,
    )


# ``Affinity for <subtype>`` keyword line (after reminder text is
# stripped). Scryfall prints Affinity as a standalone keyword word line
# — ``Affinity for Slivers`` — with the per-type reminder text in
# parentheses. After ``_strip_reminder_text`` removes the parenthetical,
# the bare ``Affinity for Slivers`` form reaches the per-line parsers.
#
# Maps to ``CostReduction(amount=1, condition=PerControlled(HasSubtype
# ("<subtype>")))`` — the engine's ``apply_cost_reductions`` multiplies
# the unit amount by ``condition.multiplier`` (the count of permanents
# matching the subtype that the caster controls at cast time), giving
# ``{1}`` off per matching permanent in play per CR 702.41.
#
# The subtype capture is open-ended (``(.+)``). Only subtypes that
# ``HasSubtype`` can test at runtime are meaningful; an unrecognized
# subtype still produces a structurally valid ``CostReduction`` whose
# ``PerControlled(HasSubtype("<unrecognized>"))`` condition will always
# evaluate to zero (no permanents have an unknown subtype), so the
# printing degrades gracefully rather than hard-failing.
_AFFINITY_FOR_LINE = re.compile(r"^Affinity for (.+)$")


def _parse_affinity_line(line: str) -> CostReduction | None:
    """Return a ``CostReduction`` for an ``Affinity for <subtype>``
    keyword line (reminder text already stripped), or ``None`` if
    the line doesn't match.

    Produces ``CostReduction(amount=1, condition=PerControlled(
    HasSubtype("<subtype>")))`` — the same structural shape the
    Thrumming Hivepool bespoke factory hard-codes for
    ``Affinity for Slivers``. The engine's
    ``apply_cost_reductions`` multiplies the unit amount by the
    ``PerControlled`` multiplier (the count of matching permanents
    the caster controls at cast time), giving a per-permanent
    discount per CR 702.41.
    """
    m = _AFFINITY_FOR_LINE.match(line.strip())
    if m is None:
        return None
    if m.group(1).strip() == "artifacts":
        # A card type, not a subtype (Emry, Lurker of the Loch).
        from mtgz_ir.effects.object_predicates import IsArtifact

        return CostReduction(amount=1, condition=PerControlled(IsArtifact()))
    subtype = m.group(1).strip().rstrip("s")  # normalize plural → singular
    # Scryfall prints the subtype in plural form: ``Affinity for Slivers``.
    # ``HasSubtype`` tests against the singular subtype name (``"Sliver"``),
    # so we strip a trailing ``s`` from the captured form. This heuristic
    # is correct for all current Affinity printings (Slivers → Sliver,
    # Artifacts → Artifact); a future printing whose plural doesn't strip
    # cleanly to the singular (e.g. ``Wolves`` → ``Wolve`` vs. ``Wolf``)
    # would evaluate to zero at runtime (no permanents with subtype
    # ``"Wolve"``), degrading gracefully rather than hard-failing.
    return CostReduction(
        amount=1,
        condition=PerControlled(HasSubtype(subtype)),
    )
