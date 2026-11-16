"""Land ``CardPrinting`` builder.

Currently supports three land kinds:

- **Basic lands** (``Basic`` supertype + ``Land`` card type), whose
  oracle text is the single reminder line ``"({T}: Add {<color>}.)"``.
  The captured color feeds ``make_basic_land``.
- **Shock lands** (non-basic ``Land`` with a two-color subtype line):
  oracle text is the dual-mana reminder + the fixed
  ``"As this land enters, you may pay 2 life. If you don't, it
  enters tapped."`` rider. Both colors come from the reminder line and
  the subtype string off the type line; both feed ``make_shock_land``.
- **Colorless utility lands** (plain ``Land`` type line, no subtypes,
  no supertypes) whose first oracle line is exactly
  ``"{T}: Add {C}."``. The remaining lines (typically tier-gated
  activated abilities like Secluded Starforge's tap-X-artifacts and
  Robot-token activations) are intentionally dropped — they mirror
  the static-side ``make_colorless_land`` simplification, which also
  ignores those abilities. The land taps for generic via the
  auto-tap-for-cost generic-mana fallback.
- **EoS bespoke lands** — historical name-keyed dispatch table
  ``_BESPOKE_EOS_LANDS`` retired on 2026-05-01 (every entry had been
  migrated to a parameterized recognizer; the table sat empty).
- **Planet lands** (``Land — Planet`` subtype, EoE) — claimed by the
  parameterized ``_try_build_planet_land`` recognizer that captures
  the produced color and drops Station + tier-gated abilities.
- **Command Bridge** — EoE Land (sac-self-unless-tap ETB +
  any-color tap) claimed by ``_try_build_command_bridge_land``.
- **Opening-hand luck-counter lands** (Gemstone Caverns) — no longer a
  whole-card builder: the line-composed grammar (``land_lines``) claims
  the opening-hand line (``opening_hand_lines``) and the counter-gated
  ``{T}: Add {C}`` line.
- **Chosen-color Cave lands** (Sunken Citadel) — claimed by
  ``_try_build_chosen_color_cave_land`` for the simplified
  "any-color" static collapse of the printed
  "as it enters, choose a color" clause.
- **Artifact-restricted mana lands** (Power Depot) — claimed by
  ``_try_build_artifact_restricted_mana_land`` for the
  ``Artifact Land`` four-line shape with the printed
  ``{T}: Add {C}.`` and ``Modular 1`` lines dropped per the
  static-side simplification.
"""

from __future__ import annotations

import dataclasses
import re

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
    AddNManaOfChosenColor,
    AddOneManaFromControlledLandsProductions,
    AddOneManaOfChosenPermanentColor,
    BecomeLandCreatureUEOT,
    BecomesCopyOfTargetLand,
    CardPrinting,
    ChangeControllerToCombatDamageSource,
    ConditionalIfYouControlAtLeast,
    CopyTargetInstantOrSorceryYouControl,
    CreateTokenCopyOfTargetCreatureYouControl,
    DealDamageToControllerAndAddMana,
    DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters,
    DestroyTargetLand,
    DrawCards,
    Effect,
    ExileTargetAttackingCreatureUntilEndStep,
    ExileTargetCreatureUntilSourceLeaves,
    GainLife,
    GrantLifeToEachOpponent,
    ManaCost,
    PumpCreaturesYouControlUntilEOT,
    PumpSelfXPowerUEOT,
    PumpTargetCreatureUntilEOT,
    PutBasicLandFromHandOntoBattlefieldTapped,
    PutChargeCountersOnSelf,
    PutPlusOneCounterOnSelf,
    RegenerateTargetCreature,
    ReturnTargetLandFromGraveyardToHand,
    SacrificeNLandsYouControl,
    SacrificeSelfUnlessTapPermanent,
    SearchLibraryForBasicLandTapped,
    SearchLibraryForCard,
    Sequence,
    SwitchSourcePowerToughnessUEOT,
    TriggeredAbility,
    UntapTargetLand,
    make_basic_land,
    make_colorless_land,
    make_planet_land,
    make_shock_land,
    make_utility_land,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.land_lines import _try_build_line_composed_land
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.effects.generic.card_flow import CardFilter
from mtgz_ir.effects.generic.enchantment_mechanics import ExileLinkedToSource
from mtgz_ir.effects.generic.eoe_fidelity import (
    MoveCounterBetweenTargets,
    ReturnCardsExiledWithSourceToBattlefield,
)
from mtgz_ir.effects.object_predicates import ControlledByYou, IsCreature, IsPlayer
from mtgz_ir.effects.value_predicates import IsPermanent
from mtgz_ir.effects.targets import TargetSpec
from mtgz_ir.effects.predicates import YOU, Controlled, Event, IsSelf, Subject


_COLOR_WORD_TO_LETTER: dict[str, str] = {
    "white": "W",
    "blue": "U",
    "black": "B",
    "red": "R",
    "green": "G",
}


# Manlands ("creature lands") share a fixed three-line oracle shape:
#   1. "This land enters tapped."
#   2. "{T}: Add {<C1>} or {<C2>}."
#   3. "<COST>: Until end of turn, this land becomes a <P>/<T>
#      <c1word> and <c2word> Elemental creature with <KEYWORDS>.
#      It's still a land."
# The animated land becomes the two named colours and an Elemental
# (``set_colors`` / ``granted_subtype`` on BecomeLandCreatureUEOT, as
# the generic "this land becomes" recognizer builds them for Restless
# Vinestalk). Variants without the
# trailing ``with <KEYWORDS>`` clause (Creeping Tar Pit's "It can't
# be blocked this turn." rider) are handled separately and don't
# match this pattern.
_MANLAND_TAPPED_LINE = "This land enters tapped."
_MANLAND_TAP_FOR_DUAL_RE = re.compile(
    r"^\{T\}: Add \{([WUBRG])\} or \{([WUBRG])\}\.$"
)
_MANLAND_ANIMATE_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+): Until end of turn, this land becomes a "
    r"(?P<power>\d+)/(?P<toughness>\d+) "
    r"(?P<c1word>white|blue|black|red|green) and "
    r"(?P<c2word>white|blue|black|red|green) Elemental creature "
    r"with (?P<keywords>[^.]+)\. It's still a land\.$"
)
# Tar-Pit-shape variant: same as the standard manland line but the
# keyword tail is replaced with the literal ``It can't be blocked
# this turn.`` rider after ``It's still a land.``. Maps to
# ``BecomeLandCreatureUEOT(unblockable=True)`` with no
# ``granted_keywords``. Currently the only Stellar Sights manland
# matching this shape is Creeping Tar Pit.
_MANLAND_ANIMATE_UNBLOCKABLE_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+): Until end of turn, this land becomes a "
    r"(?P<power>\d+)/(?P<toughness>\d+) "
    r"(?P<c1word>white|blue|black|red|green) and "
    r"(?P<c2word>white|blue|black|red|green) Elemental creature\. "
    r"It's still a land\. It can't be blocked this turn\.$"
)
# Granted-ability-shape variant: ``... with "<quoted body>". It's
# still a land.``. The quoted body is dispatched through
# ``_GRANTED_ABILITY_BODIES`` to produce either a granted activated
# ability (Lavaclaw Reaches's X-pump, Wandering Fumarole's P/T swap)
# or a granted triggered ability (Raging Ravine's attacks → +1/+1
# counter). Bodies outside the dispatch table fall through.
_MANLAND_ANIMATE_GRANTED_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+): Until end of turn, this land becomes a "
    r"(?P<power>\d+)/(?P<toughness>\d+) "
    r"(?P<c1word>white|blue|black|red|green) and "
    r"(?P<c2word>white|blue|black|red|green) Elemental creature "
    r"with \"(?P<body>[^\"]+)\.\" It's still a land\.$"
)


# Whitelist of granted-ability bodies the parser knows how to model.
# Each value is a tuple ``(granted_activated_abilities,
# granted_triggered_abilities)`` to plug straight into
# ``BecomeLandCreatureUEOT``. Lavaclaw Reaches and Wandering Fumarole
# grant activated abilities; Raging Ravine grants a triggered one.
# Adding a new body is a one-line table extension; unrecognized bodies
# leave the manland unclaimed (silent skip).
def _granted_x_pump() -> tuple:
    return (
        (
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{X}"), mana_str="{X}",
                ),
                effect=PumpSelfXPowerUEOT(),
            ),
        ),
        (),
    )


def _granted_pt_swap() -> tuple:
    return (
        (
            ActivatedAbility(
                cost=ActivationCost(),
                effect=SwitchSourcePowerToughnessUEOT(),
            ),
        ),
        (),
    )


def _granted_attacks_plus_one_counter() -> tuple:
    return (
        (),
        (
            TriggeredAbility(
                event=Event.ATTACKS, predicate=IsSelf(),
                effect=PutPlusOneCounterOnSelf(amount=1),
            ),
        ),
    )


_GRANTED_ABILITY_BODIES: dict[str, "callable"] = {
    "{X}: This creature gets +X/+0 until end of turn":
        _granted_x_pump,
    "{0}: Switch this creature's power and toughness until end of turn":
        _granted_pt_swap,
    "Whenever this creature attacks, put a +1/+1 counter on it":
        _granted_attacks_plus_one_counter,
}


def _parse_manland_keywords(text: str) -> frozenset[str] | None:
    """Parse the ``with <X>[, <Y>][, and <Z>]`` keyword tail of a
    manland animation line into a normalized frozenset.

    The Stellar Sights manlands grant simple keywords whose oracle
    surface form matches their printing-keyword name verbatim (flying,
    vigilance, deathtouch, hexproof, double strike, lifelink, reach).
    Returns ``None`` if any token doesn't look like a known keyword
    surface — keeps the parser conservative; unrecognized shapes fall
    through to the silent-skip path.
    """
    parts = re.split(r",\s+|\s+and\s+", text.strip())
    cleaned = [p.strip() for p in parts if p.strip()]
    if not cleaned:
        return None
    allowed = {
        "flying", "vigilance", "deathtouch", "hexproof",
        "double strike", "lifelink", "reach", "trample",
        "first strike", "menace", "haste", "defender",
    }
    for kw in cleaned:
        if kw not in allowed:
            return None
    return frozenset(cleaned)


# Basic-land reminder text. Scryfall always prints the produced color in
# parentheses on the single oracle line: ``"({T}: Add {W}.)"`` etc. The
# capture group identifies which color the land taps for and feeds it
# straight into ``make_basic_land``. Colorless basics aren't a thing in
# EoE today, but ``C`` is included for completeness so a future
# colorless basic doesn't trip the parser.
_BASIC_LAND_REMINDER = re.compile(r"^\(\{T\}: Add \{([WUBRGC])\}\.\)$")


# Shock-land oracle text is exactly two lines, in this order on Scryfall:
#   1. The dual-mana reminder: ``"({T}: Add {<c1>} or {<c2>}.)"``
#   2. The pay-2-life-or-enter-tapped rider:
#      ``"As this land enters, you may pay 2 life. If you don't, it
#      enters tapped."``
# Both colors come from the reminder line and are passed straight to
# ``make_shock_land``. The subtype string (e.g. ``"Forest Island"``)
# comes from the type line. Variants with a different life cost or a
# different tap-condition phrasing intentionally do not match — they
# fall through to the skip-silently behavior.
_SHOCK_LAND_REMINDER = re.compile(
    r"^\(\{T\}: Add \{([WUBRGC])\} or \{([WUBRGC])\}\.\)$"
)
_SHOCK_LAND_LIFE_RIDER = (
    "As this land enters, you may pay 2 life. "
    "If you don't, it enters tapped."
)


# Colorless utility-land "base" line: ``"{T}: Add {C}."``. When this is
# the first line of a plain ``Land`` (no subtypes, no supertypes) we
# build a colorless utility land, dropping any further activated-ability
# lines. Mirrors the static-side ``make_colorless_land`` simplification
# used for Secluded Starforge.
_COLORLESS_TAP_LINE = "{T}: Add {C}."


# ---------------------------------------------------------------------------
# Plaza of Heroes (Stellar Sights) recognizer.
#
# "Legendary Land" with exactly four oracle lines:
#   1. "{T}: Add {C}."
#   2. "{T}: Add one mana of any color. Spend this mana only to cast a
#      legendary spell."
#   3. "{T}: Add one mana of any color among legendary permanents you
#      control."
#   4. "{3}, {T}, Exile this land: Target legendary creature gains
#      hexproof and indestructible until end of turn."
#
# Lines 2–4 parse via ``_parse_activated_ability_line`` (the grammar now
# has recognizers for all three shapes); line 1 is a hard-coded
# ``{T}: Add {C}.`` constant that the grammar doesn't claim (colorless
# mana in activated.py is handled differently — see ``_COLORLESS_TAP_LINE``).
#
# Any future "Legendary Land" with this exact four-line oracle layout
# (same surface, possibly different name) claims through this recognizer
# at zero additional code.
# ---------------------------------------------------------------------------


def _try_build_plaza_of_heroes_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a ``CardPrinting`` for Plaza of Heroes' four-line
    plain ``Land`` shape if ``text`` matches, else ``None``.

    The four-line shape is:
      1. ``{T}: Add {C}.``  — colorless tap (hard-matched; colorless mana
         isn't routed through the activated-ability grammar today).
      2. ``{T}: Add one mana of any color. Spend this mana only to cast
         a legendary spell.``  — legendary-spell restricted mana (parsed
         via ``_parse_activated_ability_line``).
      3. ``{T}: Add one mana of any color among legendary permanents you
         control.``  — ``AddOneManaOfColorAmongControlled(legendary=True)``
         (parsed via ``_parse_activated_ability_line``).
      4. ``{3}, {T}, Exile this land: Target legendary creature gains
         hexproof and indestructible until end of turn.``  — exile-self
         keyword grant to a legendary creature (parsed via
         ``_parse_activated_ability_line``).

    Plain ``Land`` only (no supertypes, no subtypes). Plaza of Heroes
    is NOT a legendary permanent — its type line is bare ``Land``
    (``supertypes == ()``); the word "legendary" appears only in its
    ability text (it produces mana for legendary spells and grants
    hexproof/indestructible to legendary creatures). Any other
    supertype/subtype combination falls through.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 4:
        return None
    if lines[0] != _COLORLESS_TAP_LINE:
        return None
    # Lines 2–4: parse via the activated-ability grammar.
    a1 = _parse_activated_ability_line(lines[1])
    if a1 is None:
        return None
    a2 = _parse_activated_ability_line(lines[2])
    if a2 is None:
        return None
    a3 = _parse_activated_ability_line(lines[3])
    if a3 is None:
        return None
    # Line 1 is the plain {T}: Add {C}. mana ability — build it directly
    # since the colorless-mana effect isn't in the activated-ability grammar.
    a0 = ActivatedAbility(
        cost=ActivationCost(tap_self=True),
        effect=AddManaToPool(produces=(("C", 1),)),
        is_mana_ability=True,
    )
    return make_utility_land(
        card["name"],
        activated_abilities=(a0, a1, a2, a3),
    )


# ---------------------------------------------------------------------------
# Bespoke EoS (Stellar Sights) land dispatch table.
#
# Lands whose oracle shape doesn't map cleanly onto any of the generic
# grammar arms (multi-step manland, tap-sac, restricted mana, …) are
# pre-built here as complete ``CardPrinting`` objects and looked up by
# name before the grammar arms run. The table is keyed purely by name;
# the dispatcher in ``_build_land_printing`` gates on
# ``card.get("set") in {"eos", "eoe", "spg"}`` so main-set cards with
# the same name can't accidentally be claimed here.
#
# Migrated to parameterized grammar arms (no longer bespoke):
#   - Swarmyard          → _try_build_tap_regenerate_land
#   - Endless Sands      → _try_build_desert_exile_return_land
#   - Inventors' Fair    → _try_build_legendary_artifact_search_land
#   - Plaza of Heroes    → _try_build_plaza_of_heroes_land
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Tap-and-regenerate utility land.
#
# Plain ``Land`` (no supertypes, no subtypes), exactly two lines:
#   1. ``{T}: Add {C}.``          (intentionally dropped — the engine's
#      auto-tap-for-cost generic-mana fallback covers tapping for ``{C}``)
#   2. ``{T}: Regenerate target <type-list>.``  (with optional reminder
#      text stripped)
# where <type-list> is a comma+or-separated list of creature-subtype
# words (capitalized, e.g. "Insect, Rat, Spider, or Squirrel").
#
# The printed regenerate subtype list is captured as a frozenset and
# passed to ``RegenerateTargetCreature(allowed_subtypes=...)``. Adding
# a new card with the same two-line shape and a different subtype list
# costs zero code — the regex captures the list and the parser splits
# it at ", " and " or " boundaries.
#
# Currently the only EoS land in the pool matching this shape is
# Swarmyard: ``{T}: Regenerate target Insect, Rat, Spider, or Squirrel.``
# ---------------------------------------------------------------------------
_REGENERATE_TAP_RE = re.compile(
    r"^\{T\}: Regenerate target (?P<types>[A-Z][a-z]+(?:,\s*[A-Z][a-z]+)*"
    r"(?:,?\s+or\s+[A-Z][a-z]+)?)\.$"
)


def _try_build_tap_regenerate_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a tap-and-regenerate ``CardPrinting`` if ``text`` matches
    the strict two-line ``{T}: Add {C}.`` + ``{T}: Regenerate target
    <type-list>.`` shape on a plain ``Land`` type line, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The first line
    ``{T}: Add {C}.`` is intentionally not modeled as an explicit
    activated ability — the engine's auto-tap-for-cost generic-mana
    fallback covers it. The regenerate type list is a comma+or-joined
    sequence of capitalized creature-subtype words captured by regex
    and split into the ``allowed_subtypes`` frozenset.

    Currently claims Swarmyard (``Insect, Rat, Spider, or Squirrel``).
    A future card with the same two-line shape and a different subtype
    list claims automatically via the parameterized regex.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 2:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _TAP_FOR_C_LINE:
        return None
    regen_match = _REGENERATE_TAP_RE.match(lines[1])
    if regen_match is None:
        return None
    # Parse the subtype list: split on ", " then strip a leading "or ".
    raw_types = re.split(r",\s+", regen_match.group("types"))
    allowed: list[str] = []
    for t in raw_types:
        word = re.sub(r"^or\s+", "", t).strip()
        if not word or not word[0].isupper():
            return None
        allowed.append(word)
    if not allowed:
        return None
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=RegenerateTargetCreature(
                    allowed_subtypes=frozenset(allowed),
                ),
            ),
        ),
    )


# ---------------------------------------------------------------------------
# Desert exile-and-return land (Endless Sands shape).
#
# ``Land — Desert`` (no supertypes, single ``Desert`` subtype), exactly
# three lines:
#   1. ``{T}: Add {C}.``          (intentionally dropped)
#   2. ``{N}, {T}: Exile target creature you control.``
#   3. ``{M}, {T}, Sacrifice this land: Return each creature card
#      exiled with this land to the battlefield under its owner's
#      control.``
# where N and M are all-generic mana costs (e.g. ``{2}`` and ``{4}``).
#
# The exile activation is a linked exile (``ExileLinkedToSource``,
# CR 607.2a: no duration); the sac-and-return activation returns the
# linked creature cards (``ReturnCardsExiledWithSourceToBattlefield``).
# Adding a new Desert with the same three-line shape and
# different mana costs claims automatically via the regex captures.
#
# Currently the only EoS land matching this shape is Endless Sands.
# ---------------------------------------------------------------------------
_DESERT_EXILE_LINE_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}: Exile target creature you "
    r"control\.$"
)
_DESERT_SAC_RETURN_LINE_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}, Sacrifice this land: Return "
    r"each creature card exiled with this land to the battlefield "
    r"under its owner's control\.$"
)


def _try_build_desert_exile_return_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Desert exile-and-return ``CardPrinting`` if ``text``
    matches the strict three-line Endless Sands shape on a
    ``Land — Desert`` type line, else ``None``.

    ``Land — Desert`` only (no supertypes, single ``Desert`` subtype).
    The first line ``{T}: Add {C}.`` is intentionally dropped. The
    second line's mana cost and ``Exile target creature you control``
    body map to an ``ExileLinkedToSource`` activation (the cards are
    "exiled with this land", CR 607.2a). The third line's mana cost and
    ``Sacrifice this land: Return …`` body map to
    ``ReturnCardsExiledWithSourceToBattlefield`` over creature cards.

    Currently claims Endless Sands (exile cost ``{2}``, sac-return
    cost ``{4}``). Any future Desert with the same shape but different
    costs claims automatically.
    """
    if supertypes:
        return None
    if subtypes != ("Desert",):
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 3:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _TAP_FOR_C_LINE:
        return None
    exile_match = _DESERT_EXILE_LINE_RE.match(lines[1])
    if exile_match is None:
        return None
    sac_match = _DESERT_SAC_RETURN_LINE_RE.match(lines[2])
    if sac_match is None:
        return None
    exile_cost_str = exile_match.group("cost")
    sac_cost_str = sac_match.group("cost")
    return make_utility_land(
        card["name"], subtypes_str="Desert",
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(exile_cost_str),
                    mana_str=exile_cost_str,
                    tap_self=True,
                ),
                # A linked exile with no duration (CR 607.2a), not "until
                # this leaves the battlefield" (mtg-zero docs/ir_defects.md
                # 61).
                effect=ExileLinkedToSource(targets=(
                    TargetSpec(filter=IsCreature() & ControlledByYou()),
                )),
            ),
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(sac_cost_str),
                    mana_str=sac_cost_str,
                    tap_self=True,
                    sacrifice_self=True,
                ),
                effect=ReturnCardsExiledWithSourceToBattlefield(
                    card_filter=CardFilter(any_types=("Creature",)),
                ),
            ),
        ),
    )


# ---------------------------------------------------------------------------
# Legendary upkeep-artifact-search land (Inventors' Fair shape).
#
# ``Legendary Land`` (legendary supertype, no subtypes), exactly three
# lines:
#   1. ``At the beginning of your upkeep, if you control <N> or more
#      artifacts, you gain 1 life.``
#   2. ``{T}: Add {C}.``          (intentionally dropped)
#   3. ``{M}, {T}, Sacrifice <NAME>: Search your library for an
#      artifact card, reveal it, put it into your hand, then shuffle.
#      Activate only if you control <N> or more artifacts.``
# where <N> is a spelled-out threshold number (three, four, …) and
# <NAME> is the card's own name. Both thresholds must be equal.
#
# The upkeep trigger uses ``ConditionalIfYouControlAtLeast(threshold,
# "artifact", GainLife(1))``. The search activation uses
# ``SearchLibraryForCard(filter_name="artifact", may_fail=False)``
# with ``requires_own_artifact_count_at_least=threshold`` on the cost
# (so ``legal.py`` gates out the activation when the controller has
# fewer than N artifacts). The ``{T}: Add {C}.`` line is intentionally
# dropped — the engine's auto-tap-for-cost fallback covers it.
#
# Currently the only EoS land matching this shape is Inventors' Fair
# (threshold=3, cost ``{4}``). A future card with the same shape and
# different N or mana cost claims automatically.
# ---------------------------------------------------------------------------
_UPKEEP_ARTIFACT_THRESHOLD_LINE_RE = re.compile(
    r"^At the beginning of your upkeep, if you control "
    r"(?P<threshold>three|four|five|six|seven) or more artifacts, "
    r"you gain 1 life\.$"
)
_ARTIFACT_SEARCH_SAC_NAME_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}, Sacrifice (?P<name>.+): "
    r"Search your library for an artifact card, reveal it, put it "
    r"into your hand, then shuffle\. "
    r"Activate only if you control "
    r"(?P<threshold2>three|four|five|six|seven) or more artifacts\.$"
)
_THRESHOLD_WORD_TO_INT: dict[str, int] = {
    "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7,
}


def _try_build_legendary_artifact_search_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return an Inventors'-Fair-shape ``CardPrinting`` if ``text``
    matches the strict three-line ``Legendary Land`` shape, else
    ``None``.

    ``Legendary Land`` only (legendary supertype, no subtypes). The
    upkeep trigger threshold and the activation threshold must be equal,
    and the activation sacrifice clause must name the card itself. The
    ``{T}: Add {C}.`` line (line 2) is intentionally dropped. The
    activation cost includes ``requires_own_artifact_count_at_least``
    so that ``legal.py`` gates out the activation when the controller
    controls fewer than N artifacts, faithful to the printed
    ``Activate only if you control N or more artifacts.`` clause.

    Currently claims Inventors' Fair (threshold=3, activation cost
    ``{4}``). Any future Legendary Land with the same shape but
    different thresholds or mana cost claims automatically.
    """
    if supertypes != ("Legendary",):
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 3:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    upkeep_match = _UPKEEP_ARTIFACT_THRESHOLD_LINE_RE.match(lines[0])
    if upkeep_match is None:
        return None
    if lines[1] != _TAP_FOR_C_LINE:
        return None
    search_match = _ARTIFACT_SEARCH_SAC_NAME_RE.match(lines[2])
    if search_match is None:
        return None
    threshold_word = upkeep_match.group("threshold")
    threshold2_word = search_match.group("threshold2")
    if threshold_word != threshold2_word:
        return None
    sac_name = search_match.group("name")
    if sac_name != card["name"]:
        return None
    threshold = _THRESHOLD_WORD_TO_INT[threshold_word]
    cost_str = search_match.group("cost")
    return make_utility_land(
        card["name"], legendary=True,
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.BEGINNING_OF_UPKEEP, predicate=Controlled(YOU),
                effect=ConditionalIfYouControlAtLeast(
                    threshold=threshold,
                    predicate="artifact",
                    inner=GainLife(amount=1),
                ),
            ),
        ),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(cost_str),
                    mana_str=cost_str,
                    tap_self=True,
                    sacrifice_self=True,
                    requires_own_artifact_count_at_least=threshold,
                ),
                effect=SearchLibraryForCard(
                    filter_name="artifact", may_fail=False,
                ),
            ),
        ),
    )


# Planet lands (EoE).
#
# ``Land — Planet`` cards (no supertypes, single ``Planet`` subtype)
# whose oracle text is the strict four-line shape:
#   1. ``This land enters tapped.``
#   2. ``{T}: Add {<C>}.``
#   3. ``Station (Tap another creature you control: ...)`` — keyword
#      line with reminder text, stripped to the bare keyword
#      ``Station`` (CR 702.184a).
#   4. ``12+ | <ABILITY>``  — the striation: an activated ability the
#      planet has while it has N or more charge counters (CR 722.2a),
#      parsed by the activated-ability grammar with
#      ``tier_threshold=N``; N is also the printing's
#      ``station_threshold`` (no power/toughness box, so it never becomes
#      a creature).
#
# The captured color in line 2 feeds ``make_planet_land``. Adding
# more Planets with the same shape is zero code: this recognizer
# already claims any ``Land — Planet`` card with the four-line layout
# and a single basic-color tap line. Variants (e.g. multi-color
# Planets) would need the regex relaxed; today's printed cycle is
# all single-color.
_PLANET_TAPPED_LINE = "This land enters tapped."
_PLANET_TAP_FOR_COLOR_RE = re.compile(r"^\{T\}: Add \{([WUBRG])\}\.$")
_PLANET_STATION_KEYWORD_LINE = "Station"
_PLANET_TIERED_ABILITY_RE = re.compile(r"^(\d+)\+ \| (.+)$")


def _try_build_planet_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Planet ``CardPrinting`` if ``text`` matches the strict
    four-line ``This land enters tapped.`` + ``{T}: Add {<C>}.`` +
    ``Station (...)`` + ``<N>+ | <ABILITY>`` shape and the type line
    is ``Land — Planet``, else ``None``.

    The Station reminder is stripped per-line; the bare ``Station``
    keyword sets ``station_threshold`` to the striation's N, and the
    striation's ability (an activated ability the activated-ability
    grammar must claim, else the card is not claimed) carries
    ``tier_threshold=N``. The captured color produces a single-color
    tapped land via ``make_planet_land``.
    """
    if supertypes:
        return None
    if subtypes != ("Planet",):
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 4:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _PLANET_TAPPED_LINE:
        return None
    tap_match = _PLANET_TAP_FOR_COLOR_RE.match(lines[1])
    if tap_match is None:
        return None
    if lines[2] != _PLANET_STATION_KEYWORD_LINE:
        return None
    tier_match = _PLANET_TIERED_ABILITY_RE.match(raw_lines[3])
    if tier_match is None:
        return None
    threshold = int(tier_match.group(1))
    ability = _parse_activated_ability_line(tier_match.group(2))
    if ability is None:
        return None
    return make_planet_land(
        card["name"], tap_match.group(1),
        station_threshold=threshold,
        activated_abilities=(
            dataclasses.replace(ability, tier_threshold=threshold),
        ),
    )


# Command Bridge — EoE plain Land with three-line shape:
#   1. ``This land enters tapped.``
#   2. ``When this land enters, sacrifice it unless you tap an
#      untapped permanent you control.``
#   3. ``{T}: Add one mana of any color.``
# Maps to ``make_utility_land(enters_tapped=True,
# triggered_abilities=(<sac-unless-tap ETB>,),
# activated_abilities=(<tap-for-any-color mana>,))``. Only Command
# Bridge in the EoE pool fits this exact shape today; deferred
# generalization is to fold the recognizer into a parameterized arm
# whose middle clause and mana-tail capture vary, when a sibling
# printing of similar shape exists.
_COMMAND_BRIDGE_ENTERS_TAPPED_LINE = "This land enters tapped."
_COMMAND_BRIDGE_SAC_UNLESS_TAP_LINE = (
    "When this land enters, sacrifice it unless you tap an untapped "
    "permanent you control."
)
_COMMAND_BRIDGE_TAP_FOR_ANY_LINE = "{T}: Add one mana of any color."


def _try_build_command_bridge_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Command-Bridge-shape ``CardPrinting`` if ``text``
    matches the strict three-line ``enters tapped`` +
    ``sacrifice-unless-tap-permanent`` ETB +
    ``{T}: Add one mana of any color.`` shape on a plain
    ``Land`` type line, else ``None``.

    Only Command Bridge in the EoE pool fits this shape today
    (literal-line recognizer per the architectural rule —
    deferred-generalization noted above the constants). The
    set-gate matches the previous ``_BESPOKE_EOE_LANDS`` entry's
    ``card.set == "eoe"`` filter so a hypothetical reprint of the
    same name from a different set wouldn't accidentally claim.
    """
    if card.get("set") != "eoe":
        return None
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 3:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _COMMAND_BRIDGE_ENTERS_TAPPED_LINE:
        return None
    if lines[1] != _COMMAND_BRIDGE_SAC_UNLESS_TAP_LINE:
        return None
    if lines[2] != _COMMAND_BRIDGE_TAP_FOR_ANY_LINE:
        return None
    return make_utility_land(
        card["name"],
        enters_tapped=True,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=SacrificeSelfUnlessTapPermanent(),
            ),
        ),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("any", 1),)),
                is_mana_ability=True,
            ),
        ),
    )


def _build_land_printing(card: dict) -> CardPrinting | None:
    """Try to produce a ``CardPrinting`` for a land ``card`` (a Scryfall
    record). Returns ``None`` if the card is not a land or if its shape
    isn't one the land grammar yet supports."""
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Land" not in card_types:
        return None
    text = card.get("oracle_text", "").strip()
    if "Basic" in supertypes:
        m = _BASIC_LAND_REMINDER.match(text)
        if m is not None:
            return make_basic_land(name=card["name"], color=m.group(1))
        # Wastes: the only basic with no basic land type, so its mana
        # ability is printed rules text rather than parenthesized reminder.
        if not subtypes and text == _COLORLESS_TAP_LINE:
            return make_basic_land(
                name=card["name"], color="C", type_line=card.get("type_line", "Basic Land"),
            )
        return None
    # Non-basic land shapes follow.
    plaza = _try_build_plaza_of_heroes_land(
        card, text, supertypes, subtypes,
    )
    if plaza is not None:
        return plaza
    tap_regen = _try_build_tap_regenerate_land(
        card, text, supertypes, subtypes,
    )
    if tap_regen is not None:
        return tap_regen
    desert_exile = _try_build_desert_exile_return_land(
        card, text, supertypes, subtypes,
    )
    if desert_exile is not None:
        return desert_exile
    legendary_artifact_search = _try_build_legendary_artifact_search_land(
        card, text, supertypes, subtypes,
    )
    if legendary_artifact_search is not None:
        return legendary_artifact_search
    planet = _try_build_planet_land(card, text, supertypes, subtypes)
    if planet is not None:
        return planet
    command_bridge = _try_build_command_bridge_land(
        card, text, supertypes, subtypes,
    )
    if command_bridge is not None:
        return command_bridge
    shock = _try_build_shock_land(card, text, subtypes)
    if shock is not None:
        return shock
    manland = _try_build_manland(card, text, supertypes, subtypes)
    if manland is not None:
        return manland
    tap_sac = _try_build_tap_sac_utility_land(
        card, text, supertypes, subtypes,
    )
    if tap_sac is not None:
        return tap_sac
    fancy_mana = _try_build_fancy_mana_land(
        card, text, supertypes, subtypes,
    )
    if fancy_mana is not None:
        return fancy_mana
    ancient_tomb = _try_build_pain_two_mana_land(
        card, text, supertypes, subtypes,
    )
    if ancient_tomb is not None:
        return ancient_tomb
    paid_fancy = _try_build_paid_fancy_mana_land(
        card, text, supertypes, subtypes,
    )
    if paid_fancy is not None:
        return paid_fancy
    keyword_colorless = _try_build_keyword_colorless_utility_land(
        card, text, supertypes, subtypes,
    )
    if keyword_colorless is not None:
        return keyword_colorless
    paid_effect = _try_build_paid_effect_utility_land(
        card, text, supertypes, subtypes,
    )
    if paid_effect is not None:
        return paid_effect
    restricted_mana = _try_build_restricted_mana_land(
        card, text, supertypes, subtypes,
    )
    if restricted_mana is not None:
        return restricted_mana
    verge = _try_build_verge_dual_land(
        card, text, supertypes, subtypes,
    )
    if verge is not None:
        return verge
    low_life_dual = _try_build_low_life_dual_land(
        card, text, supertypes, subtypes,
    )
    if low_life_dual is not None:
        return low_life_dual
    fetch = _try_build_fetch_land(
        card, text, supertypes, subtypes,
    )
    if fetch is not None:
        return fetch
    optional_copy_cave = _try_build_optional_copy_cave_land(
        card, text, supertypes, subtypes,
    )
    if optional_copy_cave is not None:
        return optional_copy_cave
    chosen_color_cave = _try_build_chosen_color_cave_land(
        card, text, supertypes, subtypes,
    )
    if chosen_color_cave is not None:
        return chosen_color_cave
    artifact_restricted = _try_build_artifact_restricted_mana_land(
        card, text, supertypes, card_types, subtypes,
    )
    if artifact_restricted is not None:
        return artifact_restricted
    pain_dual = _try_build_pain_dual_color_land(
        card, text, supertypes, subtypes,
    )
    if pain_dual is not None:
        return pain_dual
    lotus_field = _try_build_keyword_etb_sac_chosen_color_land(
        card, text, supertypes, subtypes,
    )
    if lotus_field is not None:
        return lotus_field
    mirrorpool = _try_build_two_paid_sac_self_land(
        card, text, supertypes, subtypes,
    )
    if mirrorpool is not None:
        return mirrorpool
    blast_zone = _try_build_blast_zone_land(
        card, text, supertypes, subtypes,
    )
    if blast_zone is not None:
        return blast_zone
    contested_war_zone = _try_build_contested_war_zone_land(
        card, text, supertypes, subtypes,
    )
    if contested_war_zone is not None:
        return contested_war_zone
    blinkmoth_nexus = _try_build_blinkmoth_nexus_land(
        card, text, supertypes, subtypes,
    )
    if blinkmoth_nexus is not None:
        return blinkmoth_nexus
    colorless = _try_build_colorless_utility_land(
        card, text, supertypes, subtypes,
    )
    if colorless is not None:
        return colorless
    # Line-composed grammar (``land_lines``): every oracle line through
    # a parameterized per-line recognizer. Last so every whole-card
    # shape above keeps its claim.
    return _try_build_line_composed_land(
        card, text, supertypes, card_types, subtypes,
    )


def _try_build_shock_land(
    card: dict, text: str, subtypes: tuple[str, ...]
) -> CardPrinting | None:
    """Return a shock-land ``CardPrinting`` if ``text`` matches the
    two-line shock-land shape, else ``None``.

    The shape is exactly: a dual-mana reminder line followed by the
    fixed pay-2-life-or-enter-tapped rider. Anything else (extra lines,
    different life cost, different rider wording) falls through.
    """
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    m = _SHOCK_LAND_REMINDER.match(lines[0])
    if m is None:
        return None
    if lines[1] != _SHOCK_LAND_LIFE_RIDER:
        return None
    if not subtypes:
        return None
    colors = (m.group(1), m.group(2))
    return make_shock_land(
        name=card["name"],
        colors=colors,
        subtypes_str=" ".join(subtypes),
    )


def _try_build_manland(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a manland (creature-land) ``CardPrinting`` if ``text``
    matches the standard 3-line manland shape, else ``None``.

    Shape:
      1. "This land enters tapped."
      2. "{T}: Add {<C1>} or {<C2>}."
      3. "<COST>: Until end of turn, this land becomes a <P>/<T>
          <c1word> and <c2word> Elemental creature with <KEYWORDS>.
          It's still a land."

    The colors named in the type-2 mana ability must match the colors
    named in the type-3 animation line (after color-word translation).
    Variants with extra abilities, X-cost activations, attacks
    triggers, or "It can't be blocked this turn." riders fall through.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 3:
        return None
    if lines[0] != _MANLAND_TAPPED_LINE:
        return None
    tap_match = _MANLAND_TAP_FOR_DUAL_RE.match(lines[1])
    if tap_match is None:
        return None
    animate_match = _MANLAND_ANIMATE_RE.match(lines[2])
    keywords: frozenset[str] = frozenset()
    unblockable = False
    granted_activated: tuple = ()
    granted_triggered: tuple = ()
    if animate_match is not None:
        keywords = _parse_manland_keywords(animate_match.group("keywords"))
        if keywords is None:
            return None
    else:
        animate_match = _MANLAND_ANIMATE_UNBLOCKABLE_RE.match(lines[2])
        if animate_match is not None:
            unblockable = True
        else:
            animate_match = _MANLAND_ANIMATE_GRANTED_RE.match(lines[2])
            if animate_match is None:
                return None
            body = animate_match.group("body")
            builder = _GRANTED_ABILITY_BODIES.get(body)
            if builder is None:
                return None
            granted_activated, granted_triggered = builder()
    c1, c2 = tap_match.group(1), tap_match.group(2)
    c1_animate = _COLOR_WORD_TO_LETTER[animate_match.group("c1word")]
    c2_animate = _COLOR_WORD_TO_LETTER[animate_match.group("c2word")]
    if {c1, c2} != {c1_animate, c2_animate}:
        return None
    cost_str = animate_match.group("cost")
    return make_utility_land(
        card["name"],
        produces=frozenset({c1, c2}),
        enters_tapped=True,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(cost_str),
                    mana_str=cost_str,
                ),
                effect=BecomeLandCreatureUEOT(
                    power=int(animate_match.group("power")),
                    toughness=int(animate_match.group("toughness")),
                    granted_keywords=keywords,
                    unblockable=unblockable,
                    granted_subtype="Elemental",
                    granted_activated_abilities=granted_activated,
                    granted_triggered_abilities=granted_triggered,
                    set_colors=frozenset({c1_animate, c2_animate}),
                ),
            ),
        ),
    )


# Tap-and-sac utility land builder.
#
# Recognizes plain colorless utility lands whose oracle text is the
# fixed two-line shape:
#   1. ``{T}: Add {C}.``
#   2. ``<COST>: <BODY>.``
# where ``<COST>`` is one of a small set of recognized cost prefixes
# (``{T}, Sacrifice this land``, ``{N}, {T}, Sacrifice a land``, etc.)
# and ``<BODY>`` is on a per-cost-shape effect whitelist below.
# Bodies / costs outside the table fall through silently.
#
# Built on the same ``make_utility_land`` factory as the manland
# branch — the {C} mana ability as ``produces``, a single activated
# ability whose cost flags carry the sac semantics, the effect from the
# dispatch.
_TAP_FOR_C_LINE = "{T}: Add {C}."
# The first line, "{T}: Add {C}.", is the land's mana ability: folded
# into ``produces`` (the line-composed land grammar's form for a plain
# tap-for-one-mana ability, ``land_lines``) with
# ``mana_only_from_produces`` so the land pays exactly {C}, not through
# the legacy "any land pays one generic" fallback.
_TAP_FOR_C_PRODUCES = frozenset({"C"})
_TAP_AND_SAC_SELF_RE = re.compile(
    r"^\{T\}, Sacrifice this land: (?P<body>.+)\.$"
)
_MANA_TAP_AND_SAC_LAND_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}, Sacrifice a land: (?P<body>.+)\.$"
)
_TAP_AND_SAC_CREATURE_RE = re.compile(
    r"^\{T\}, Sacrifice a creature: (?P<body>.+)\.$"
)
_MANA_TAP_AND_SAC_DESERT_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}, Sacrifice a Desert: (?P<body>.+)\.$"
)


# Body-text → effect-factory whitelist. Each factory returns a fresh
# ``Effect`` instance; matching bodies outside the table makes the
# recognizer fall through. Adding a new card is one entry per cost
# shape — keep the surface form exact (no trailing period — the
# regexes strip it).
_TAP_SAC_SELF_BODIES: dict[str, "callable"] = {
    "Destroy target land": lambda: DestroyTargetLand(),
    "Return target land card from your graveyard to your hand":
        lambda: ReturnTargetLandFromGraveyardToHand(),
}
_TAP_SAC_LAND_BODIES: dict[str, "callable"] = {
    "Destroy target nonbasic land":
        lambda: DestroyTargetLand(nonbasic_only=True),
}
_TAP_SAC_CREATURE_BODIES: dict[str, "callable"] = {
    "You gain 1 life": lambda: GainLife(amount=1),
}


# Bodies for the Desert-subtyped sac-a-Desert path. Imports are
# resolved lazily inside the factory because some of these effect
# classes are bespoke (e.g. ``ExileAllGraveyards``) and pulling them
# into the top-level imports keeps the import graph predictable.
def _exile_all_graveyards() -> Effect:
    from mtgz_ir.cards.base import ExileAllGraveyards
    return ExileAllGraveyards()


_TAP_SAC_DESERT_BODIES: dict[str, "callable"] = {
    "Exile all graveyards": _exile_all_graveyards,
}


def _try_build_tap_sac_utility_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a tap-and-sac utility-land ``CardPrinting`` if ``text``
    matches one of the recognized two-line shapes, else ``None``.

    Supported shapes (each gated by its own body whitelist):

    - **Sac-self, no mana cost**: ``{T}: Add {C}.`` then
      ``{T}, Sacrifice this land: <BODY>.`` — Strip Mine,
      Petrified Field.
    - **Sac-a-land with mana cost**: ``{T}: Add {C}.`` then
      ``<COST>, {T}, Sacrifice a land: <BODY>.`` — Dust Bowl.
    - **Sac-a-creature, no mana cost**: ``{T}: Add {C}.`` then
      ``{T}, Sacrifice a creature: <BODY>.`` — High Market
      (``sacrifice_a_permanent_type="creature"``: a creature only).
    """
    if supertypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    if lines[0] != _TAP_FOR_C_LINE:
        return None
    second = lines[1]

    sac_self = _TAP_AND_SAC_SELF_RE.match(second)
    if sac_self is not None and not subtypes:
        body = sac_self.group("body")
        factory = _TAP_SAC_SELF_BODIES.get(body)
        if factory is None:
            return None
        return make_utility_land(
            card["name"],
            produces=_TAP_FOR_C_PRODUCES,
            mana_only_from_produces=True,
            activated_abilities=(
                ActivatedAbility(
                    cost=ActivationCost(
                        tap_self=True, sacrifice_self=True,
                    ),
                    effect=factory(),
                ),
            ),
        )

    sac_land = _MANA_TAP_AND_SAC_LAND_RE.match(second)
    if sac_land is not None and not subtypes:
        body = sac_land.group("body")
        factory = _TAP_SAC_LAND_BODIES.get(body)
        if factory is None:
            return None
        cost_str = sac_land.group("cost")
        return make_utility_land(
            card["name"],
            produces=_TAP_FOR_C_PRODUCES,
            mana_only_from_produces=True,
            activated_abilities=(
                ActivatedAbility(
                    cost=ActivationCost(
                        mana=parse_mana_cost(cost_str),
                        mana_str=cost_str,
                        tap_self=True,
                        sacrifice_a_land=True,
                    ),
                    effect=factory(),
                ),
            ),
        )

    sac_creature = _TAP_AND_SAC_CREATURE_RE.match(second)
    if sac_creature is not None and not subtypes:
        body = sac_creature.group("body")
        factory = _TAP_SAC_CREATURE_BODIES.get(body)
        if factory is None:
            return None
        return make_utility_land(
            card["name"],
            produces=_TAP_FOR_C_PRODUCES,
            mana_only_from_produces=True,
            activated_abilities=(
                ActivatedAbility(
                    cost=ActivationCost(
                        tap_self=True,
                        sacrifice_a_permanent_type="creature",
                    ),
                    effect=factory(),
                ),
            ),
        )

    sac_desert = _MANA_TAP_AND_SAC_DESERT_RE.match(second)
    if sac_desert is not None and subtypes == ("Desert",):
        body = sac_desert.group("body")
        factory = _TAP_SAC_DESERT_BODIES.get(body)
        if factory is None:
            return None
        cost_str = sac_desert.group("cost")
        return make_utility_land(
            card["name"],
            produces=_TAP_FOR_C_PRODUCES,
            mana_only_from_produces=True,
            subtypes_str="Desert",
            activated_abilities=(
                ActivatedAbility(
                    cost=ActivationCost(
                        mana=parse_mana_cost(cost_str),
                        mana_str=cost_str,
                        tap_self=True,
                        sacrifice_a_desert=True,
                    ),
                    effect=factory(),
                ),
            ),
        )

    return None


# Fancy-mana single-ability lands.
#
# Plain ``Land`` cards (no supertypes, no subtypes) whose entire
# oracle text is a single activated mana ability that produces an
# any-color or chosen-color coin (rather than a fixed colorless or
# dual-color coin). Each card pairs an exact oracle line with a
# factory that returns the matching ``(cost, effect)`` pair.
#
# Distinct from the manland branch (lands that animate into
# creatures) and the tap-and-sac branch (lands that pay self-sac
# costs). The ``produces`` set is empty in the static — mana-source
# tracking treats "any" coins as untyped at compile time.
def _fancy_pay_one_life_any_color() -> tuple:
    return (
        ActivationCost(tap_self=True, pay_life=1),
        AddManaToPool(produces=(("any", 1),), spend_restriction=None),
    )


def _fancy_chosen_permanent_color() -> tuple:
    return (
        ActivationCost(tap_self=True),
        AddOneManaOfChosenPermanentColor(),
    )


def _fancy_controlled_lands_productions() -> tuple:
    return (
        ActivationCost(tap_self=True),
        AddOneManaFromControlledLandsProductions(),
    )


_FANCY_MANA_BODIES: dict[str, "callable"] = {
    "{T}, Pay 1 life: Add one mana of any color.":
        _fancy_pay_one_life_any_color,
    "{T}: Choose a color of a permanent you control. "
    "Add one mana of that color.":
        _fancy_chosen_permanent_color,
    "{T}: Add one mana of any type that a land you "
    "control could produce.":
        _fancy_controlled_lands_productions,
}


def _try_build_fancy_mana_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a fancy-mana single-ability ``CardPrinting`` if the
    card's entire oracle text matches an entry in
    ``_FANCY_MANA_BODIES`` and the type line is plain ``Land``,
    else ``None``.

    Currently claims Mana Confluence (``{T}, Pay 1 life``),
    Meteor Crater (chosen-permanent-color), and Reflecting Pool
    (any-from-controlled-lands). Each is a single line whose
    factory returns the canonical static ``(cost, effect)`` pair.
    Future entries are one-liners: add the surface form and a
    factory.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    factory = _FANCY_MANA_BODIES.get(text.strip())
    if factory is None:
        return None
    cost, effect = factory()
    return make_utility_land(
        card["name"],
        activated_abilities=(
            ActivatedAbility(
                cost=cost, effect=effect, is_mana_ability=True,
            ),
        ),
    )


# Ancient-Tomb-shape "pain land" — single-line plain Land whose
# ``{T}`` mana ability fuses an Add-N-mana production with a
# damage-to-controller rider on the same activation. Currently the
# only EoE printing matching this shape is Ancient Tomb itself; the
# regex captures the produced colorless count and damage amount so a
# future printing of similar shape (e.g. ``Add {C}{C}{C}. This land
# deals 3 damage to you.``) can claim by table extension rather than
# code change. The effect class
# (``DealDamageToControllerAndAddMana``) is the bespoke engine
# primitive the static side already uses; the parser only constructs
# it from the parsed values.
_PAIN_TWO_MANA_LINE_RE = re.compile(
    r"^\{T\}: Add (?P<mana>(?:\{[CWUBRG]\})+)\. "
    r"This land deals (?P<damage>\d+) damage to you\.$"
)


def _try_build_pain_two_mana_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a CardPrinting for a single-line pain-shape mana land
    matching the Ancient Tomb pattern, else None.

    Constraint: plain ``Land`` (no supertypes, no subtypes) whose entire
    oracle text is one line whose mana production is an all-colorless
    ``{C}+`` chain (one or more ``{C}`` braces). Other color
    combinations would need a per-color tally; today's only printing
    produces colorless, and adding colored variants is a one-line regex
    relaxation when the day comes.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    line = text.strip()
    m = _PAIN_TWO_MANA_LINE_RE.match(line)
    if m is None:
        return None
    mana_str = m.group("mana")
    pips = re.findall(r"\{([CWUBRG])\}", mana_str)
    if not pips or any(p != "C" for p in pips):
        return None
    return make_utility_land(
        card["name"],
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=DealDamageToControllerAndAddMana(
                    damage=int(m.group("damage")),
                    produces=(("C", len(pips)),),
                ),
                is_mana_ability=True,
            ),
        ),
    )


# "Paid fancy mana" lands: plain ``Land`` whose oracle text is
# (optionally) an evergreen-keyword line, then ``{T}: Add {C}.`` (the
# basic tap-for-colorless ability — *not* modeled on the static side;
# the engine's auto-tap-for-cost generic-mana fallback covers it),
# then a single ``<COST>, {T}: <BODY>.`` paid-mana activation whose
# body is on a per-shape whitelist.
#
# Currently claims:
# - Crystal Quarry: ``{5}, {T}: Add {W}{U}{B}{R}{G}.``
# - Cascading Cataracts: ``{5}, {T}: Add five mana in any combination
#   of colors.`` plus the ``Indestructible`` keyword line.
_KEYWORD_LINE_RE = re.compile(r"^([A-Z][a-z]+)$")
_DROPPED_TAP_FOR_C_LINE = "{T}: Add {C}."
_PAID_FANCY_MANA_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}: (?P<body>.+)\.$"
)


def _add_wubrg_factory() -> Effect:
    return AddManaToPool(
        produces=(
            ("W", 1), ("U", 1), ("B", 1), ("R", 1), ("G", 1),
        ),
    )


def _add_five_any_factory() -> Effect:
    return AddManaToPool(produces=(("any", 5),))


_PAID_FANCY_MANA_BODIES: dict[str, "callable"] = {
    "Add {W}{U}{B}{R}{G}": _add_wubrg_factory,
    "Add five mana in any combination of colors": _add_five_any_factory,
}


_LAND_KEYWORD_WHITELIST = frozenset(
    {"indestructible", "exalted", "hexproof"},
)


def _try_build_paid_fancy_mana_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a paid-fancy-mana CardPrinting if ``text`` matches the
    optional-keyword + dropped-{T}:{C} + paid-mana shape, else None.

    The leading optional-keyword line, when present, must be a single
    capitalized word on the keyword whitelist (today: ``Indestructible``
    for Cascading Cataracts). The static side carries this keyword as
    a frozenset on the ``CardPrinting``; the parser threads it through.
    The middle line ``{T}: Add {C}.`` is intentionally dropped — the
    static-side ``make_utility_land`` doesn't add an explicit ability
    for it, relying on the engine's auto-tap-for-cost fallback.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) not in (2, 3):
        return None
    keywords: frozenset[str] = frozenset()
    if len(lines) == 3:
        kw_match = _KEYWORD_LINE_RE.match(lines[0])
        if kw_match is None:
            return None
        kw = kw_match.group(1).lower()
        if kw not in _LAND_KEYWORD_WHITELIST:
            return None
        keywords = frozenset({kw})
        lines = lines[1:]
    if lines[0] != _DROPPED_TAP_FOR_C_LINE:
        return None
    paid_match = _PAID_FANCY_MANA_RE.match(lines[1])
    if paid_match is None:
        return None
    body = paid_match.group("body")
    factory = _PAID_FANCY_MANA_BODIES.get(body)
    if factory is None:
        return None
    cost_str = paid_match.group("cost")
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        keywords=keywords,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(cost_str),
                    mana_str=cost_str,
                    tap_self=True,
                ),
                effect=factory(),
                is_mana_ability=True,
            ),
        ),
    )


# Keyword-only utility lands.
#
# Plain ``Land`` cards (no supertypes, no subtypes) whose oracle text
# is the strict three-line shape:
#   1. ``This land enters tapped.``
#   2. <single-keyword-line>      (on ``_LAND_KEYWORD_WHITELIST``)
#   3. ``{T}: Add {C}.``          (intentionally dropped — the
#      static-side ``make_utility_land`` doesn't add an explicit
#      ability for it; the engine's auto-tap-for-cost generic-mana
#      fallback covers tapping for ``{C}``)
#
# Currently claims:
# - Cathedral of War — ``This land enters tapped.`` +
#   ``Exalted (...)`` + ``{T}: Add {C}.``
#
# The keyword-line reminder text is stripped per-line via
# ``_strip_reminder_text`` before the ``_KEYWORD_LINE_RE`` lookup.
# Distinct from the paid-fancy-mana branch (which expects a paid
# mana ability after ``{T}: Add {C}.``) and the colorless-utility
# branch (which doesn't model ``enters_tapped`` and doesn't accept
# a keyword line).
_KEYWORD_TAPPED_TAP_FOR_C_LINE = "This land enters tapped."


def _try_build_keyword_colorless_utility_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a keyword-only colorless utility ``CardPrinting`` if
    ``text`` matches the strict three-line ``This land enters
    tapped.`` + <keyword> + ``{T}: Add {C}.`` shape, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The keyword
    line is restricted to a single capitalized word on
    ``_LAND_KEYWORD_WHITELIST`` (today: ``Exalted`` for Cathedral of
    War). The ``{T}: Add {C}.`` line is intentionally not modeled as
    an explicit activated ability — the engine's auto-tap-for-cost
    generic-mana fallback covers it, matching the static side's
    ``make_utility_land(enters_tapped=True, keywords=...)`` shape.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 3:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _KEYWORD_TAPPED_TAP_FOR_C_LINE:
        return None
    kw_match = _KEYWORD_LINE_RE.match(lines[1])
    if kw_match is None:
        return None
    kw = kw_match.group(1).lower()
    if kw not in _LAND_KEYWORD_WHITELIST:
        return None
    if lines[2] != _DROPPED_TAP_FOR_C_LINE:
        return None
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        enters_tapped=True,
        keywords=frozenset({kw}),
    )


# Generic-paid single-effect utility lands.
#
# Plain ``Land`` cards (no supertypes, no subtypes) whose oracle text
# is the strict two-line shape:
#   1. ``{T}: Add {C}.``
#   2. ``<MANA-COST>, {T}: <BODY>.[ <RIDER>.]``
# where ``<MANA-COST>`` is a generic-mana cost (e.g. ``{1}``, ``{2}``,
# ``{4}``) and ``<BODY>`` is on a per-card body whitelist that maps to
# a single bespoke ``Effect`` instance. The static side wraps that
# effect in a single ``ActivatedAbility`` whose cost carries the parsed
# mana plus ``tap_self=True``. The optional trailing ``<RIDER>`` is a
# fixed activation-condition phrase parsed off ``_PAID_EFFECT_RIDERS``
# and surfaced as an extra flag on the ``ActivationCost``.
#
# Distinct from the tap-and-sac branch (those costs include a sac
# clause) and the paid-fancy-mana branch (those bodies produce mana).
# Currently claims:
# - Bonders' Enclave — ``{3}, {T}: Draw a card. Activate only if you
#   control a creature with power 4 or greater.``
# - Deserted Temple — ``{1}, {T}: Untap target land.``
# - Mystifying Maze — ``{4}, {T}: Exile target attacking creature an
#   opponent controls. At the beginning of the next end step, return
#   it to the battlefield tapped under its owner's control.``
# - Terrain Generator — ``{2}, {T}: You may put a basic land card from
#   your hand onto the battlefield tapped.``
#
# Adding a new card with the same shape is a one-line table extension:
# add the exact body surface form (no trailing period — the regex
# strips it) to ``_PAID_EFFECT_BODIES`` paired with a factory that
# returns a fresh ``Effect`` instance. Bodies / cost shapes outside
# the table fall through to the silent-skip path.
#
# The ``{T}`` portion of the activation cost is optional in the regex:
# Mutavault's ``{1}: This land becomes a 2/2 creature ...`` line has
# no tap component, only a generic-mana payment. When the regex
# matches without ``{T}`` (i.e. the ``tap`` named group is empty),
# ``tap_self`` is omitted from the resulting ``ActivationCost``.
_PAID_EFFECT_RE = re.compile(
    r"^(?P<cost>(?:\{\d+\})+)(?P<tap>, \{T\})?: (?P<rest>.+)\.$"
)


def _paid_effect_untap_target_land() -> Effect:
    return UntapTargetLand()


def _paid_effect_exile_attacking_until_end_step() -> Effect:
    return ExileTargetAttackingCreatureUntilEndStep()


def _paid_effect_put_basic_from_hand_tapped() -> Effect:
    return PutBasicLandFromHandOntoBattlefieldTapped()


def _paid_effect_draw_a_card() -> Effect:
    return DrawCards(amount=1)


def _paid_effect_becomes_copy_of_target_land() -> Effect:
    return BecomesCopyOfTargetLand()


def _paid_effect_move_counter_between_permanents() -> Effect:
    # "Move a counter from target permanent you control onto a second
    # target permanent": a counter of any kind, onto a permanent anyone
    # controls (mtg-zero docs/ir_defects.md 27).
    return MoveCounterBetweenTargets(
        targets=(
            TargetSpec(IsPermanent() & ControlledByYou()),
            TargetSpec(IsPermanent(), distinct_from=(0,)),
        ),
    )


def _paid_effect_become_2_2_all_creature_types() -> Effect:
    # "with all creature types" (CR 205.3m; mtg-zero docs/ir_defects.md 41).
    return BecomeLandCreatureUEOT(power=2, toughness=2, all_creature_types=True)


def _paid_effect_become_inkmoth_nexus() -> Effect:
    # "a 1/1 Phyrexian Blinkmoth artifact creature with flying and infect"
    return BecomeLandCreatureUEOT(
        power=1, toughness=1,
        granted_keywords=frozenset({"flying", "infect"}),
        granted_card_types=("Artifact",),
        granted_subtypes=("Phyrexian", "Blinkmoth"),
    )


_PAID_EFFECT_BODIES: dict[str, "callable"] = {
    "Untap target land": _paid_effect_untap_target_land,
    "Exile target attacking creature an opponent controls. "
    "At the beginning of the next end step, return it to the "
    "battlefield tapped under its owner's control":
        _paid_effect_exile_attacking_until_end_step,
    "You may put a basic land card from your hand onto the "
    "battlefield tapped":
        _paid_effect_put_basic_from_hand_tapped,
    "Draw a card": _paid_effect_draw_a_card,
    # Thespian's Stage's ``{2}, {T}: This land becomes a copy of
    # target land, except it has this ability.`` This recognizer
    # claims the land via the standard paid-effect arm; the
    # ``except it has this ability`` clause is part of the body
    # surface form (``BecomesCopyOfTargetLand`` already implements
    # the ability-preserving copy).
    "This land becomes a copy of target land, except it has this "
    "ability":
        _paid_effect_becomes_copy_of_target_land,
    # Nesting Grounds' ``{1}, {T}: Move a counter from target permanent
    # you control onto a second target permanent.`` (paired with the
    # ``Activate only as a sorcery`` rider, which lifts to
    # ``sorcery_speed=True`` on the ability).
    "Move a counter from target permanent you control onto a second "
    "target permanent":
        _paid_effect_move_counter_between_permanents,
    # Mutavault's ``{1}: This land becomes a 2/2 creature with all
    # creature types until end of turn. It's still a land.`` (no
    # ``{T}`` in the activation cost — the regex's optional ``tap``
    # group is empty for this body, so ``tap_self`` is dropped from
    # the resulting ``ActivationCost``). The static side simplifies
    # the printed "with all creature types" clause to a plain 2/2
    # animation; the IR carries it (``all_creature_types``, CR 205.3m):
    # ``BecomeLandCreatureUEOT(power=2, toughness=2, all_creature_types=True)``.
    # Currently the only EoS land matching this body literal is
    # Mutavault; deferred generalization would parameterize the
    # P/T capture and the keyword/subtype tail when a sibling
    # printing appears.
    "This land becomes a 2/2 creature with all creature types "
    "until end of turn. It's still a land":
        _paid_effect_become_2_2_all_creature_types,
    # Inkmoth Nexus's ``{1}: This land becomes a 1/1 Phyrexian
    # Blinkmoth artifact creature with flying and infect until end
    # of turn. It's still a land.`` (with the inline infect
    # reminder stripped earlier): an artifact creature with the
    # Phyrexian and Blinkmoth creature types (``granted_card_types``,
    # ``granted_subtypes``), flying and infect. Currently
    # the only EoS land matching this body literal is Inkmoth
    # Nexus; deferred generalization would parameterize the P/T
    # capture, granted-subtype/type capture, and keyword tail when a
    # sibling printing of similar shape (Blinkmoth Nexus, on its
    # own multi-ability shape, doesn't fit this two-line arm)
    # appears.
    "This land becomes a 1/1 Phyrexian Blinkmoth artifact "
    "creature with flying and infect until end of turn. It's "
    "still a land":
        _paid_effect_become_inkmoth_nexus,
}


# Activation-condition rider phrases. Each entry maps an exact phrase
# (no trailing period) to a callable that mutates either the
# ``ActivationCost`` keyword-arg dict or the ``ActivatedAbility``
# keyword-arg dict in place (or both). The mutator's signature is
# ``(cost_kwargs, ability_kwargs)`` — riders that only affect the
# cost ignore ``ability_kwargs`` and vice versa. Riders outside the
# table fall through to the silent-skip path; bodies without a rider
# also pass through — the recognizer treats the rider as optional.
def _rider_creature_power_four_or_greater(
    cost_kwargs: dict, ability_kwargs: dict,
) -> None:
    cost_kwargs["requires_own_creature_power_at_least"] = 4


def _rider_activate_only_as_a_sorcery(
    cost_kwargs: dict, ability_kwargs: dict,
) -> None:
    ability_kwargs["sorcery_speed"] = True


_PAID_EFFECT_RIDERS: dict[str, "callable"] = {
    "Activate only if you control a creature with power 4 or greater":
        _rider_creature_power_four_or_greater,
    # Nesting Grounds — sorcery-speed rider lifts to
    # ``ActivatedAbility(sorcery_speed=True)``.
    "Activate only as a sorcery": _rider_activate_only_as_a_sorcery,
}


def _try_build_paid_effect_utility_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a generic-paid single-effect utility ``CardPrinting`` if
    ``text`` matches the strict two-line ``{T}: Add {C}.`` +
    ``<MANA>[, {T}]: <BODY>.[ <RIDER>.]`` shape with a body on
    ``_PAID_EFFECT_BODIES`` (and rider, if present, on
    ``_PAID_EFFECT_RIDERS``), else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The mana-cost
    capture is restricted to all-generic pips (``{1}``, ``{2}``, …) —
    cards with colored or hybrid pips on the activation cost would
    need a different cost shape and don't fit the current set.

    The ``{T}`` part of the activation cost is optional: Mutavault's
    ``{1}: This land becomes a 2/2 creature ...`` line activates
    without tapping, so ``tap_self`` is conditionally set on the
    ``ActivationCost`` only when the regex's ``tap`` group matched.

    Per-line reminder text is stripped via ``_strip_reminder_text``
    before any matching: Inkmoth Nexus's animation line carries an
    inline ``(It deals damage to creatures in the form of -1/-1
    counters and to players in the form of poison counters.)``
    parenthetical that the engine doesn't need to see; stripping it
    here keeps the body literal in ``_PAID_EFFECT_BODIES`` clean.

    The optional rider is a fixed activation-condition phrase (e.g.
    Bonders' Enclave's ``Activate only if you control a creature
    with power 4 or greater``, Nesting Grounds' ``Activate only as a
    sorcery``) that maps to either an ``ActivationCost`` flag or an
    ``ActivatedAbility`` flag (or both, depending on the rider). The
    rider mutator receives both kwargs dicts and writes into whichever
    is appropriate. Bodies / cost shapes / riders outside their tables
    fall through silently.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 2:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _TAP_FOR_C_LINE:
        return None
    paid_match = _PAID_EFFECT_RE.match(lines[1])
    if paid_match is None:
        return None
    rest = paid_match.group("rest")
    # Try to peel off a recognized rider sentence first; matching is
    # exact on the trailing ``. <RIDER>`` slice. Whichever known rider
    # the body ends with wins (``startswith`` checks pin the body to
    # the front of ``rest`` after trimming the rider).
    body = rest
    cost_extra: dict = {}
    ability_extra: dict = {}
    for rider, mutator in _PAID_EFFECT_RIDERS.items():
        suffix = ". " + rider
        if rest.endswith(suffix):
            body = rest[: -len(suffix)]
            mutator(cost_extra, ability_extra)
            break
    factory = _PAID_EFFECT_BODIES.get(body)
    if factory is None:
        return None
    cost_str = paid_match.group("cost")
    if paid_match.group("tap"):
        cost_extra["tap_self"] = True
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(cost_str),
                    mana_str=cost_str,
                    **cost_extra,
                ),
                effect=factory(),
                **ability_extra,
            ),
        ),
    )


# Restricted-mana lands.
#
# Plain ``Land`` cards (no supertypes, no subtypes) whose oracle text
# is the strict two-line shape:
#   1. ``{T}: Add {C}.``
#   2. ``{T}: Add <MANA>. <RESTRICTION-PHRASE>.``
# The first line is modeled as an explicit ``AddManaToPool`` mana
# ability (NOT dropped to the auto-tap-for-cost fallback) — restricted
# lands carry both abilities on the static side because the
# unrestricted coin from line 1 is independently spendable. The second
# line's restriction phrase is on a small whitelist that maps to
# ``AddManaToPool.spend_restriction`` tag values; the engine's
# ``_coin_satisfies_context`` consults the tag during cost-pay.
#
# Currently claims:
# - Eldrazi Temple — ``{T}: Add {C}{C}.`` + colorless-Eldrazi
#   restriction ("Spend this mana only to cast colorless Eldrazi
#   spells or activate abilities of colorless Eldrazi.").
#
# Adding a new restriction is a one-line table extension keyed on the
# exact phrase (no trailing period). Adding a new produced-mana shape
# (e.g. ``{T}: Add {U}{U}``) needs the second-line regex to
# accommodate the colored pip set; today's only printing produces
# all-colorless.
_RESTRICTED_MANA_LINE_RE = re.compile(
    r"^\{T\}: Add (?P<mana>(?:\{[CWUBRG]\})+)\. (?P<phrase>.+)\.$"
)


_RESTRICTED_MANA_PHRASES: dict[str, str] = {
    "Spend this mana only to cast colorless Eldrazi spells or "
    "activate abilities of colorless Eldrazi": "colorless_eldrazi",
}


def _try_build_restricted_mana_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a restricted-mana ``CardPrinting`` if ``text`` matches
    the strict two-line ``{T}: Add {C}.`` + ``{T}: Add <MANA>.
    <PHRASE>.`` shape with a phrase on ``_RESTRICTED_MANA_PHRASES``,
    else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The mana line
    must be all-colorless ``{C}+``. Both abilities are modeled
    explicitly on the printing's ``activated_abilities`` (the first
    is the unrestricted base coin; the second carries the
    restriction tag).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    if lines[0] != _TAP_FOR_C_LINE:
        return None
    m = _RESTRICTED_MANA_LINE_RE.match(lines[1])
    if m is None:
        return None
    mana_str = m.group("mana")
    pips = re.findall(r"\{([CWUBRG])\}", mana_str)
    if not pips or any(p != "C" for p in pips):
        return None
    phrase = m.group("phrase")
    tag = _RESTRICTED_MANA_PHRASES.get(phrase)
    if tag is None:
        return None
    return make_utility_land(
        card["name"],
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("C", 1),)),
                is_mana_ability=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(
                    produces=(("C", len(pips)),),
                    spend_restriction=tag,
                ),
                is_mana_ability=True,
            ),
        ),
    )


# Duskmourn "Verge cycle" dual lands (Blazemire Verge, Floodfarm Verge,
# Gloomlake Verge, Hushwood Verge, Thornspire Verge). Plain ``Land``,
# no supertypes/subtypes, exactly two lines:
#   1. ``{T}: Add {<C1>}.``
#   2. ``{T}: Add {<C2>}. Activate only if you control a <T1> or a
#      <T2>.``
# The first ability is unconditional; the second produces the partner
# color but is gated on the activator controlling at least one
# permanent of either of the two named subtypes (always basic-land
# subtypes on Verges). The gate threads through ``ActivationCost.
# requires_control_of_subtype``; legal_actions consults effective_subtypes.
_VERGE_FIRST_LINE_RE = re.compile(r"^\{T\}: Add \{([WUBRGC])\}\.$")
_VERGE_SECOND_LINE_RE = re.compile(
    r"^\{T\}: Add \{(?P<color>[WUBRGC])\}\. "
    r"Activate only if you control an? (?P<t1>[A-Z][a-z]+) "
    r"or an? (?P<t2>[A-Z][a-z]+)\.$"
)


def _try_build_verge_dual_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Verge-dual ``CardPrinting`` if ``text`` matches the
    two-line Duskmourn Verge shape, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The two colors
    must differ; the gated second activation requires the activator to
    control a permanent whose effective subtypes include at least one
    of the two named subtypes (capitalized words on the right of the
    "Activate only if you control a X or a Y" clause).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    first = _VERGE_FIRST_LINE_RE.match(lines[0])
    if first is None:
        return None
    second = _VERGE_SECOND_LINE_RE.match(lines[1])
    if second is None:
        return None
    c1 = first.group(1)
    c2 = second.group("color")
    if c1 == c2:
        return None
    t1 = second.group("t1")
    t2 = second.group("t2")
    return make_utility_land(
        card["name"],
        produces=frozenset({c1, c2}),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=((c1, 1),)),
                is_mana_ability=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(
                    tap_self=True,
                    requires_control_of_subtype=(t1, t2),
                ),
                effect=AddManaToPool(produces=((c2, 1),)),
                is_mana_ability=True,
            ),
        ),
    )


# Pure tap-and-sac fetch lands (Terramorphic Expanse). A plain
# ``Land`` (no supertypes, no subtypes) whose ENTIRE oracle text is
# the single line:
#
#     {T}, Sacrifice this land: <BODY>.
#
# Distinct from ``_try_build_tap_sac_utility_land`` which gates on a
# preceding ``{T}: Add {C}.`` line. Bodies are on a per-card whitelist
# pinning the exact surface form to a fresh ``Effect`` instance. The
# produces set is empty — these lands tap only to pay their own
# sacrifice activation, never to mint mana.
_FETCH_LAND_BODIES: dict[str, "callable"] = {
    "Search your library for a basic land card, put it onto the "
    "battlefield tapped, then shuffle":
        lambda: SearchLibraryForBasicLandTapped(count=1),
}


def _try_build_fetch_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a fetch-land ``CardPrinting`` if ``text`` is a single
    ``{T}, Sacrifice this land: <BODY>.`` line whose body is on
    ``_FETCH_LAND_BODIES``, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The cost is
    ``tap_self`` + ``sacrifice_self``; ``produces`` stays empty —
    fetch lands are not a mana source.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    line = text.strip()
    if "\n" in line:
        return None
    m = _TAP_AND_SAC_SELF_RE.match(line)
    if m is None:
        return None
    body = m.group("body")
    factory = _FETCH_LAND_BODIES.get(body)
    if factory is None:
        return None
    return make_utility_land(
        card["name"],
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    tap_self=True, sacrifice_self=True,
                ),
                effect=factory(),
            ),
        ),
    )


# Duskmourn "horror dual" cycle (Abandoned Campground, Bleeding Woods,
# Etched Cornfield, Lakeside Shack, Murky Sewer, Neglected Manor,
# Peculiar Lighthouse, Raucous Carnival, Razortrap Gorge, Strangled
# Cemetery). Plain ``Land``, no supertypes/subtypes, exactly two
# lines:
#   1. ``This land enters tapped unless a player has <N> or less life.``
#   2. ``{T}: Add {<C1>} or {<C2>}.``
# The condition is a deterministic state check at ETB (no agent
# choice). The threshold flows through
# ``CardPrinting.enters_tapped_unless_any_player_life_at_most``;
# ``_play_land`` reads it after applying the default ``enters_tapped``.
_LOW_LIFE_TAPPED_LINE_RE = re.compile(
    r"^This land enters tapped unless a player has "
    r"(?P<threshold>\d+) or less life\.$"
)
_LOW_LIFE_DUAL_TAP_LINE_RE = re.compile(
    r"^\{T\}: Add \{(?P<c1>[WUBRG])\} or \{(?P<c2>[WUBRG])\}\.$"
)


def _try_build_low_life_dual_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a low-life-dual ``CardPrinting`` if ``text`` matches the
    Duskmourn horror-dual two-line shape, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The condition
    line names the threshold (always 13 in the printed cycle, but
    captured generically). The mana line names two distinct colors;
    they're set as the ``produces`` frozenset and the dual-mana
    activation goes on the printing as a single mana ability via the
    ``AddNManaOfChosenColor`` primitive.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    cond = _LOW_LIFE_TAPPED_LINE_RE.match(lines[0])
    if cond is None:
        return None
    tap = _LOW_LIFE_DUAL_TAP_LINE_RE.match(lines[1])
    if tap is None:
        return None
    c1 = tap.group("c1")
    c2 = tap.group("c2")
    if c1 == c2:
        return None
    threshold = int(cond.group("threshold"))
    return make_utility_land(
        card["name"],
        produces=frozenset({c1, c2}),
        enters_tapped_unless_any_player_life_at_most=threshold,
    )


# Optional-copy-from-graveyard Cave land. Plain ``Land — Cave`` (no
# supertypes), exactly two lines:
#   1. ``You may have this land enter tapped as a copy of any land card
#      in a graveyard, except it's a Cave in addition to its other
#      types.``
#   2. ``{T}: Add {C}.``  (intentionally dropped — the engine's
#      auto-tap-for-cost generic-mana fallback covers tapping for
#      ``{C}``; ``make_utility_land`` doesn't add an explicit ability.)
#
# The first line maps to ``optional_copy_land_from_graveyard=True`` on
# the ``CardPrinting``, threading the static-side flag through.
# Currently the only EoS land matching this exact two-line shape is
# Echoing Deeps; recognizer is a literal-line match (deferred
# generalization to a parameterized "<subtype>-in-addition-to-its-
# other-types" recognizer if a sibling printing appears).
_OPTIONAL_COPY_CAVE_FIRST_LINE = (
    "You may have this land enter tapped as a copy of any land card "
    "in a graveyard, except it's a Cave in addition to its other "
    "types."
)


def _try_build_optional_copy_cave_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return an optional-copy-from-graveyard Cave ``CardPrinting`` if
    ``text`` matches the strict two-line Echoing Deeps shape, else
    ``None``.

    ``Land — Cave`` only (no supertypes, single ``Cave`` subtype). The
    first line is a literal match against the printed copy-from-
    graveyard sentence; the second line is the dropped
    ``{T}: Add {C}.``. The static-side
    ``optional_copy_land_from_graveyard=True`` flag carries the
    one-card behavior. Today's only printing matching this shape is
    Echoing Deeps; deferred generalization to a parameterized
    ``<subtype>-in-addition-to-its-other-types`` recognizer would
    happen if a sibling printing appears.
    """
    if supertypes:
        return None
    if subtypes != ("Cave",):
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    if lines[0] != _OPTIONAL_COPY_CAVE_FIRST_LINE:
        return None
    if lines[1] != _DROPPED_TAP_FOR_C_LINE:
        return None
    return make_utility_land(
        card["name"], subtypes_str="Cave",
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        optional_copy_land_from_graveyard=True,
    )


# Artifact-restricted any-color mana land. ``Artifact Land``
# (card_types ``("Artifact", "Land")``, no supertypes, no subtypes),
# exactly four lines:
#   1. ``This land enters tapped.``
#   2. ``{T}: Add {C}.``                  (intentionally dropped — the
#      static side relies on the engine's auto-tap-for-cost generic-
#      mana fallback for tapping for ``{C}``.)
#   3. ``{T}: Add one mana of any color. Spend this mana only to cast
#      artifact spells or activate abilities of artifacts.``
#   4. ``Modular 1``                      (the keyword, CR 702.43)
#
# The restricted any-color line's restriction is the printed "artifact
# spells or abilities of artifacts" (``artifact_spell_or_activation``);
# ``{T}: Add {C}.`` is ``produces``; ``Modular 1`` is the keyword
# (mtg-zero docs/ir_defects.md 47).
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring notes deferred generalization. Power Depot is the
# only Artifact Land in the EoS pool; broader grammar (parameterized
# over the restriction phrase, or extended to non-Artifact restricted
# any-color lands) deferred until a sibling printing exists.
_ARTIFACT_RESTRICTED_LAND_LINE_1 = "This land enters tapped."
_ARTIFACT_RESTRICTED_LAND_LINE_2 = "{T}: Add {C}."
_ARTIFACT_RESTRICTED_LAND_LINE_3 = (
    "{T}: Add one mana of any color. Spend this mana only to cast "
    "artifact spells or activate abilities of artifacts."
)
_ARTIFACT_RESTRICTED_LAND_LINE_4 = "Modular 1"


def _try_build_artifact_restricted_mana_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    card_types: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return an Artifact-Land CardPrinting if ``text`` matches the
    strict four-line Power Depot shape, else ``None``.

    Type-line gate: ``Artifact Land`` (card_types exactly
    ``("Artifact", "Land")``, no supertypes, no subtypes). The static
    side drops both the explicit ``{T}: Add {C}.`` ability (covered by
    the engine's auto-tap-for-cost fallback) and the printed
    ``Modular 1`` keyword (Modular not modeled). The single retained
    activated ability produces ``("any", 1)`` mana with
    ``spend_restriction="artifact_spell"``.

    Per parser/CLAUDE.md: a literal-line recognizer is acceptable
    here because Power Depot is "the only card in the current data
    pool that fits that printed surface" and "broader grammar deferred
    until a second example exists" (e.g. another Artifact Land with a
    similarly restricted any-color activation).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    if card_types != ("Artifact", "Land"):
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 4:
        return None
    if lines[0] != _ARTIFACT_RESTRICTED_LAND_LINE_1:
        return None
    if lines[1] != _ARTIFACT_RESTRICTED_LAND_LINE_2:
        return None
    if lines[2] != _ARTIFACT_RESTRICTED_LAND_LINE_3:
        return None
    if lines[3] != _ARTIFACT_RESTRICTED_LAND_LINE_4:
        return None
    return CardPrinting(
        name=card["name"],
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line="Artifact Land",
        supertypes=(),
        card_types=("Artifact", "Land"),
        subtypes=(),
        produces=_TAP_FOR_C_PRODUCES,
        mana_only_from_produces=True,
        enters_tapped=True,
        keywords=frozenset({"modular 1"}),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(
                    produces=(("any", 1),),
                    spend_restriction="artifact_spell_or_activation",
                ),
                is_mana_ability=True,
            ),
        ),
    )


# Chosen-color Cave land. ``Land — Cave`` (no supertypes, single
# ``Cave`` subtype), exactly three lines:
#   1. ``This land enters tapped. As it enters, choose a color.``
#   2. ``{T}: Add one mana of the chosen color.``
#   3. ``{T}: Add two mana of the chosen color. Spend this mana only to
#      activate abilities of land sources.``
#
# The ``As it enters, choose a color`` clause and the per-activation
# "of the chosen color" specifier are simplified on the static side to
# the ``"any"`` color (the engine's mana primitive doesn't carry a
# per-permanent chosen-color slot for lands today). The two
# activations land as separate mana abilities: the first unrestricted,
# the second tagged with ``spend_restriction="land_activation"``.
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring notes deferred generalization. Sunken Citadel is
# the only EoS land matching this chosen-color-Cave shape; broader
# grammar (parameterized over the second-line restriction phrase, or
# extended to non-Cave chosen-color lands) deferred until a sibling
# printing exists.
_CHOSEN_COLOR_CAVE_LINE_1 = "This land enters tapped. As it enters, choose a color."
_CHOSEN_COLOR_CAVE_LINE_2 = "{T}: Add one mana of the chosen color."
_CHOSEN_COLOR_CAVE_LINE_3 = (
    "{T}: Add two mana of the chosen color. Spend this mana only to "
    "activate abilities of land sources."
)


def _try_build_chosen_color_cave_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a chosen-color Cave ``CardPrinting`` if ``text`` matches
    the strict three-line Sunken Citadel shape, else ``None``.

    ``Land — Cave`` only (no supertypes, single ``Cave`` subtype). The
    static-side simplification maps "the chosen color" to the
    engine's ``"any"`` color tag for both produced amounts; the second
    activation carries a ``spend_restriction="land_activation"`` tag.

    Per parser/CLAUDE.md: a literal-line recognizer is acceptable
    here because Sunken Citadel is "the only card in the current data
    pool that fits that printed surface" and "broader grammar deferred
    until a second example exists" (e.g. another chosen-color Cave or
    a non-Cave chosen-color land that simplifies the same way).
    """
    if supertypes:
        return None
    if subtypes != ("Cave",):
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 3:
        return None
    if lines[0] != _CHOSEN_COLOR_CAVE_LINE_1:
        return None
    if lines[1] != _CHOSEN_COLOR_CAVE_LINE_2:
        return None
    if lines[2] != _CHOSEN_COLOR_CAVE_LINE_3:
        return None
    # "As it enters, choose a color" and mana "of the chosen color"
    # (CR 614.12; mtg-zero docs/ir_defects.md 50).
    printing = make_utility_land(
        card["name"], subtypes_str="Cave", enters_tapped=True,
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("chosen", 1),)),
                is_mana_ability=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(
                    produces=(("chosen", 2),),
                    spend_restriction="land_activation",
                ),
                is_mana_ability=True,
            ),
        ),
    )
    return dataclasses.replace(printing, enters_choosing_color=True)


# Mirrorpool-shape: enters-tapped + dropped-{T}:{C} + two
# paid-mana-and-sac-self activations whose bodies dispatch through a
# table. Plain ``Land`` (no supertypes, no subtypes), exactly four
# lines:
#   1. ``This land enters tapped.``
#   2. ``{T}: Add {C}.``  (intentionally dropped — the engine's
#      auto-tap-for-cost generic-mana fallback covers tapping for
#      ``{C}``; the static side carries no explicit ability for line 2)
#   3. ``<COST>, {T}, Sacrifice this land: <BODY>.``
#   4. ``<COST>, {T}, Sacrifice this land: <BODY>.``
# where each ``<COST>`` is a colored-pip-bearing mana cost (e.g.
# ``{2}{C}``, ``{4}{C}``) and each ``<BODY>`` is on the
# ``_PAID_AND_SAC_SELF_BODIES`` whitelist that maps the printed
# surface form to a fresh ``Effect`` instance.
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring "explicitly notes why it isn't generalized today
# (typically: 'only card in the pool with this shape; broader grammar
# deferred until a second example exists')." Mirrorpool is the only
# EoS land with this exact two-paid-and-sac-self activation shape;
# broader grammar (parameterized over the activation count, or
# extended to non-paired activation shapes) deferred until a sibling
# printing exists. The cost capture handles colored pips so the
# recognizer is faithful to the printed ``{2}{C}``/``{4}{C}`` surface
# even though only Mirrorpool fits today.
_MIRRORPOOL_TAPPED_LINE = "This land enters tapped."
_MIRRORPOOL_DROPPED_TAP_FOR_C_LINE = "{T}: Add {C}."
_MIRRORPOOL_PAID_AND_SAC_SELF_RE = re.compile(
    r"^(?P<cost>(?:\{[^}]+\})+), \{T\}, Sacrifice this land: "
    r"(?P<body>.+)\.$"
)


def _mirrorpool_copy_instant_or_sorcery() -> Effect:
    return CopyTargetInstantOrSorceryYouControl()


def _mirrorpool_token_copy_creature() -> Effect:
    return CreateTokenCopyOfTargetCreatureYouControl()


_PAID_AND_SAC_SELF_BODIES: dict[str, "callable"] = {
    # Mirrorpool's first activation: ``{2}{C}, {T}, Sacrifice this
    # land: Copy target instant or sorcery spell you control. You may
    # choose new targets for the copy.`` The "you may choose new
    # targets" clause is part of the body literal because the engine's
    # ``CopyTargetInstantOrSorceryYouControl`` already implements
    # fresh target selection on resolve.
    "Copy target instant or sorcery spell you control. You may "
    "choose new targets for the copy":
        _mirrorpool_copy_instant_or_sorcery,
    # Mirrorpool's second activation: ``{4}{C}, {T}, Sacrifice this
    # land: Create a token that's a copy of target creature you
    # control.``
    "Create a token that's a copy of target creature you control":
        _mirrorpool_token_copy_creature,
}


def _try_build_two_paid_sac_self_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Mirrorpool-shape ``CardPrinting`` if ``text`` matches
    the strict four-line ``This land enters tapped.`` + dropped
    ``{T}: Add {C}.`` + two ``<COST>, {T}, Sacrifice this land:
    <BODY>.`` activations on a plain ``Land`` type line, else
    ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). Each
    activation's body must be on ``_PAID_AND_SAC_SELF_BODIES`` (else
    falls through). The activation cost capture accepts arbitrary
    brace-pip sequences (colored or generic) — Mirrorpool's
    ``{2}{C}`` / ``{4}{C}`` is the printed shape today.

    Per parser/CLAUDE.md: literal-line recognizer (Mirrorpool is the
    only EoS land matching this shape today; broader grammar deferred
    until a second example exists).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 4:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _MIRRORPOOL_TAPPED_LINE:
        return None
    if lines[1] != _MIRRORPOOL_DROPPED_TAP_FOR_C_LINE:
        return None
    abilities: list[ActivatedAbility] = []
    for line in lines[2:]:
        m = _MIRRORPOOL_PAID_AND_SAC_SELF_RE.match(line)
        if m is None:
            return None
        body = m.group("body")
        factory = _PAID_AND_SAC_SELF_BODIES.get(body)
        if factory is None:
            return None
        cost_str = m.group("cost")
        abilities.append(
            ActivatedAbility(
                cost=ActivationCost(
                    # ``{C}`` is a colorless pip, not generic (CR 107.4b;
                    # mtg-zero docs/ir_defects.md 67).
                    mana=parse_mana_cost(cost_str, colorless_pips=True),
                    mana_str=cost_str,
                    tap_self=True,
                    sacrifice_self=True,
                ),
                effect=factory(),
            ),
        )
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        enters_tapped=True,
        activated_abilities=tuple(abilities),
    )


# Lotus-Field-shape: keyword + tapped + ETB-sac-N-lands trigger +
# tap-for-N-any-color mana ability. Plain ``Land`` (no supertypes,
# no subtypes), exactly four lines:
#   1. <single-keyword-line>      (on ``_LAND_KEYWORD_WHITELIST``,
#      with reminder text stripped)
#   2. ``This land enters tapped.``
#   3. ``When this land enters, sacrifice <N> lands.`` (ETB trigger
#      that sacrifices a fixed number of other lands the controller
#      controls; ``SacrificeNLandsYouControl(count=<N>)`` carries the
#      semantics — the sac excludes the entering land per the
#      static side, matching the printed "two lands" reading)
#   4. ``{T}: Add <K> mana of any one color.``  (chosen-color tap;
#      ``AddNManaOfChosenColor(amount=<K>)`` produces the K-coin)
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring "explicitly notes why it isn't generalized today
# (typically: 'only card in the pool with this shape; broader grammar
# deferred until a second example exists')." Lotus Field is the only
# EoS land matching this four-line keyword + ETB-sac + chosen-color
# shape; broader grammar (parameterized over the keyword set, the sac
# count, or the produced-mana count) deferred until a sibling printing
# of similar shape exists. The ``<N>`` sac count and ``<K>`` mana
# count are captured so the recognizer is faithful to the printed
# surface even though only the (2, 3) pairing exists today.
_LOTUS_FIELD_TAPPED_LINE = "This land enters tapped."
_LOTUS_FIELD_SAC_LANDS_RE = re.compile(
    r"^When this land enters, sacrifice "
    r"(?P<count>two|three|four|five|six) lands\.$"
)
_LOTUS_FIELD_TAP_FOR_ANY_COLOR_RE = re.compile(
    r"^\{T\}: Add (?P<amount>two|three|four|five|six) mana of any one "
    r"color\.$"
)
_NUMBER_WORD_TO_INT: dict[str, int] = {
    "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
}


def _try_build_keyword_etb_sac_chosen_color_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Lotus-Field-shape ``CardPrinting`` if ``text`` matches
    the strict four-line keyword + ``This land enters tapped.`` +
    ``When this land enters, sacrifice <N> lands.`` ETB trigger +
    ``{T}: Add <K> mana of any one color.`` shape on a plain ``Land``
    type line, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The keyword line
    is restricted to a single capitalized word on
    ``_LAND_KEYWORD_WHITELIST`` (today: ``Hexproof`` for Lotus Field).
    Reminder text on the keyword line is stripped via
    ``_strip_reminder_text`` before the keyword lookup. The ``<N>``
    sac count and ``<K>`` produced-mana count are captured number
    words; the recognizer is faithful to the printed surface even
    though only the ``(2, 3)`` pairing exists today.

    Per parser/CLAUDE.md: literal-line recognizers are acceptable only
    when "exactly one card in the current data pool fits that printed
    surface" and the docstring explicitly notes deferred generalization.
    Lotus Field is the only EoS land matching this shape; broader
    grammar deferred until a second example exists.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 4:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    kw_match = _KEYWORD_LINE_RE.match(lines[0])
    if kw_match is None:
        return None
    kw = kw_match.group(1).lower()
    if kw not in _LAND_KEYWORD_WHITELIST:
        return None
    if lines[1] != _LOTUS_FIELD_TAPPED_LINE:
        return None
    sac_match = _LOTUS_FIELD_SAC_LANDS_RE.match(lines[2])
    if sac_match is None:
        return None
    tap_match = _LOTUS_FIELD_TAP_FOR_ANY_COLOR_RE.match(lines[3])
    if tap_match is None:
        return None
    sac_count = _NUMBER_WORD_TO_INT[sac_match.group("count")]
    mana_amount = _NUMBER_WORD_TO_INT[tap_match.group("amount")]
    return make_utility_land(
        card["name"],
        keywords=frozenset({kw}),
        enters_tapped=True,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=SacrificeNLandsYouControl(count=sac_count),
            ),
        ),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddNManaOfChosenColor(amount=mana_amount),
                is_mana_ability=True,
            ),
        ),
    )


# Painful-dual-tap-for-C-plus-dual-color land. Plain ``Land`` (no
# supertypes, no subtypes), exactly two lines:
#   1. ``{T}: Add {C}.``
#   2. ``{T}: Add {<C1>} or {<C2>}. Each opponent gains <N> life.``
#
# Two explicit mana abilities are produced:
# - The colorless tap from line 1 lands as a plain
#   ``AddManaToPool(produces=(("C", 1),))`` mana ability (NOT dropped to
#   the auto-tap-for-cost fallback — Grove ships the colorless coin
#   as an independently-spendable mana ability on the static side).
# - The dual-color tap from line 2 lands as a ``Sequence`` of
#   ``AddNManaOfChosenColor(amount=1, options=(<C1>, <C2>))`` followed
#   by ``GrantLifeToEachOpponent(amount=<N>)``, kept on a single
#   ``ActivatedAbility`` so the chosen-color coin and the life-gain
#   resolve as one activation.
#
# The two colors and the opponent-life-gain count are captured (so a
# sibling printing with a different color pair or rider amount fits
# without code change). The captured ``produces`` set goes on the
# printing as ``frozenset({"C", <C1>, <C2>})``. Plain ``Land`` only —
# variants with subtypes, additional lines, or different riders fall
# through to the silent-skip path.
_PAIN_DUAL_TAP_FOR_C_LINE = "{T}: Add {C}."
_PAIN_DUAL_COLOR_RIDER_RE = re.compile(
    r"^\{T\}: Add \{(?P<c1>[WUBRG])\} or \{(?P<c2>[WUBRG])\}\. "
    r"Each opponent gains (?P<life>\d+) life\.$"
)


def _try_build_pain_dual_color_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Grove-of-the-Burnwillows-shape ``CardPrinting`` if
    ``text`` matches the strict two-line ``{T}: Add {C}.`` +
    ``{T}: Add {<C1>} or {<C2>}. Each opponent gains <N> life.``
    shape on a plain ``Land`` type line, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The two colors
    must differ. Both abilities are explicit on the resulting
    printing's ``activated_abilities``: the line-1 colorless coin and
    the line-2 dual-color-plus-opponent-life-gain Sequence.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    if lines[0] != _PAIN_DUAL_TAP_FOR_C_LINE:
        return None
    m = _PAIN_DUAL_COLOR_RIDER_RE.match(lines[1])
    if m is None:
        return None
    c1 = m.group("c1")
    c2 = m.group("c2")
    if c1 == c2:
        return None
    life = int(m.group("life"))
    return make_utility_land(
        card["name"],
        produces=frozenset({"C", c1, c2}),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("C", 1),)),
                is_mana_ability=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=Sequence(effects=(
                    AddNManaOfChosenColor(
                        amount=1, options=(c1, c2),
                    ),
                    GrantLifeToEachOpponent(amount=life),
                )),
                is_mana_ability=True,
            ),
        ),
    )


# Blast Zone — plain ``Land`` (no supertypes, no subtypes) whose
# oracle text is the strict four-line shape:
#   1. ``This land enters with a charge counter on it.``
#   2. ``{T}: Add {C}.``  (``produces={"C"}``)
#   3. ``{X}{X}, {T}: Put X charge counters on this land.``
#   4. ``{3}, {T}, Sacrifice this land: Destroy each nonland permanent
#      with mana value equal to the number of charge counters on this
#      land.``
#
# The ETB charge-counter line maps to ``enters_with_named_counters``
# (a replacement effect, CR 614.1c). The third line maps to an
# activated ability with ``PutChargeCountersOnSelf(amount_from_x=True)``.
# The fourth line maps to an activated ability with
# ``DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters()``.
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring "explicitly notes why it isn't generalized today
# (typically: 'only card in the pool with this shape; broader grammar
# deferred until a second example exists')." Blast Zone is the only
# EoS land with this charge-counter ETB + X-charge activation +
# MV-keyed sweeper shape; broader grammar (parameterized over the
# sweeper cost or the per-line surface forms) deferred until a sibling
# printing exists.
_BLAST_ZONE_ETB_LINE = "This land enters with a charge counter on it."
_BLAST_ZONE_TAP_FOR_C_LINE = "{T}: Add {C}."
_BLAST_ZONE_X_CHARGE_LINE = (
    "{X}{X}, {T}: Put X charge counters on this land."
)
_BLAST_ZONE_DESTROY_LINE = (
    "{3}, {T}, Sacrifice this land: Destroy each nonland permanent "
    "with mana value equal to the number of charge counters on this "
    "land."
)


def _try_build_blast_zone_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Blast-Zone-shape ``CardPrinting`` if ``text`` matches
    the strict four-line ETB-charge + ``{T}: Add {C}.`` + X-charge +
    MV-sweeper shape on a plain ``Land`` type line, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The line-2
    ``{T}: Add {C}.`` is intentionally not modeled as an explicit
    activated ability — the engine's auto-tap-for-cost generic-mana
    fallback covers tapping for ``{C}``, matching the static-side
    ``make_utility_land`` shape (no explicit colorless tap ability).

    Per parser/CLAUDE.md: a literal-line recognizer is acceptable
    here because Blast Zone is "the only card in the current data
    pool that fits that printed surface" and "broader grammar deferred
    until a second example exists" (e.g. another land with an ETB
    charge counter, an X-cost charge-counter pump, and an MV-keyed
    sweeper).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 4:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _BLAST_ZONE_ETB_LINE:
        return None
    if lines[1] != _BLAST_ZONE_TAP_FOR_C_LINE:
        return None
    if lines[2] != _BLAST_ZONE_X_CHARGE_LINE:
        return None
    if lines[3] != _BLAST_ZONE_DESTROY_LINE:
        return None
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        # "This land enters with a charge counter on it." (CR 614.1c,
        # 122.6: a replacement effect, not a trigger).
        enters_with_named_counters=(("charge", 1),),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{X}{X}"),
                    mana_str="{X}{X}",
                    tap_self=True,
                ),
                effect=PutChargeCountersOnSelf(amount_from_x=True),
            ),
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{3}"),
                    mana_str="{3}",
                    tap_self=True,
                    sacrifice_self=True,
                ),
                effect=(
                    DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters()
                ),
            ),
        ),
    )


# Contested War Zone — plain ``Land`` (no supertypes, no subtypes)
# whose oracle text is the strict three-line shape:
#   1. ``Whenever a creature deals combat damage to you, that
#      creature's controller gains control of this land.``
#   2. ``{T}: Add {C}.``  (intentionally dropped — the engine's
#      auto-tap-for-cost generic-mana fallback covers tapping for
#      ``{C}``; the static side carries no explicit ability for line 2.)
#   3. ``{1}, {T}: Attacking creatures get +1/+0 until end of turn.``
#
# The line-1 trigger maps to a triggered ability on
# ``Event.DEALS_COMBAT_DAMAGE`` with the ``IsCreature(source) &
# IsPlayer(target) & Controlled(YOU, target)`` predicate (the damage
# source is a creature, the damaged one is you, a player) firing
# ``ChangeControllerToCombatDamageSource()``. The line-3 paid mana
# activation maps to ``PumpCreaturesYouControlUntilEOT(power_bonus=1,
# toughness_bonus=0, attacking_only=True)``.
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring "explicitly notes why it isn't generalized today
# (typically: 'only card in the pool with this shape; broader grammar
# deferred until a second example exists')." Contested War Zone is the
# only EoS land with this combat-damage-control-change trigger plus
# attacking-creatures-pump activation shape; broader grammar (parameterized
# over the per-line surface forms) deferred until a sibling printing
# exists.
_CONTESTED_WAR_ZONE_TRIGGER_LINE = (
    "Whenever a creature deals combat damage to you, that creature's "
    "controller gains control of this land."
)
_CONTESTED_WAR_ZONE_TAP_FOR_C_LINE = "{T}: Add {C}."
_CONTESTED_WAR_ZONE_PUMP_LINE = (
    "{1}, {T}: Attacking creatures get +1/+0 until end of turn."
)


def _try_build_contested_war_zone_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Contested-War-Zone-shape ``CardPrinting`` if ``text``
    matches the strict three-line combat-damage-trigger + ``{T}: Add
    {C}.`` + ``{1}, {T}: Attacking creatures get +1/+0 until end of
    turn.`` shape on a plain ``Land`` type line, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). The line-2
    ``{T}: Add {C}.`` is intentionally not modeled as an explicit
    activated ability — the engine's auto-tap-for-cost generic-mana
    fallback covers tapping for ``{C}``, matching the static-side
    ``make_utility_land`` shape (no explicit colorless tap ability).

    Per parser/CLAUDE.md: a literal-line recognizer is acceptable
    here because Contested War Zone is "the only card in the current
    data pool that fits that printed surface" and "broader grammar
    deferred until a second example exists" (e.g. another land with
    a combat-damage-triggered control change plus an
    attacking-creatures pump activation).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 3:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _CONTESTED_WAR_ZONE_TRIGGER_LINE:
        return None
    if lines[1] != _CONTESTED_WAR_ZONE_TAP_FOR_C_LINE:
        return None
    if lines[2] != _CONTESTED_WAR_ZONE_PUMP_LINE:
        return None
    return make_utility_land(
        card["name"],
        produces=_TAP_FOR_C_PRODUCES, mana_only_from_produces=True,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.DEALS_COMBAT_DAMAGE,
                # "deals combat damage to you": the damaged one is a
                # player, the War Zone's controller (mtg-zero
                # docs/ir_defects.md 161) -- not a creature or
                # planeswalker you control.
                predicate=(
                    Subject("source", IsCreature())
                    & Subject("target", IsPlayer())
                    & Controlled(YOU, subject="target")
                ),
                effect=ChangeControllerToCombatDamageSource(),
            ),
        ),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{1}"),
                    mana_str="{1}",
                    tap_self=True,
                ),
                effect=PumpCreaturesYouControlUntilEOT(
                    power_bonus=1, toughness_bonus=0,
                    attacking_only=True,
                ),
            ),
        ),
    )


# Blinkmoth Nexus — plain ``Land`` (no supertypes, no subtypes) whose
# oracle text is the strict three-line shape:
#   1. ``{T}: Add {C}.``
#   2. ``{1}: This land becomes a 1/1 Blinkmoth artifact creature with
#      flying until end of turn. It's still a land.``
#   3. ``{1}, {T}: Target Blinkmoth creature gets +1/+1 until end of
#      turn.``
#
# Distinct from Inkmoth Nexus (which fits the two-line
# ``_try_build_paid_effect_utility_land`` arm via its dropped
# ``{T}: Add {C}.`` followed by a single animation activation; it
# lacks the third-line subtype-targeted pump). Blinkmoth Nexus
# carries three abilities on the static side: an explicit mana
# ability for line 1, a no-tap animation activation for line 2 (which
# grants the ``Blinkmoth`` subtype to the animated land), and a
# tap-and-pay-{1} activation for line 3 (pumps a Blinkmoth-subtyped
# creature). The static side sets ``produces=frozenset({"C"})``
# (because the explicit mana ability is modeled, unlike Inkmoth's
# fall-through ``{T}: Add {C}.``).
#
# Per parser/CLAUDE.md: literal-line recognizers are acceptable when
# "exactly one card in the current data pool fits that printed surface"
# and the docstring "explicitly notes why it isn't generalized today
# (typically: 'only card in the pool with this shape; broader grammar
# deferred until a second example exists')." Blinkmoth Nexus is the
# only EoS land with this three-line ``{T}: Add {C}.`` plus
# subtype-granting animation plus subtype-targeted pump shape;
# broader grammar (parameterized over the granted subtype, the P/T,
# and the keyword set) deferred until a sibling printing exists.
_BLINKMOTH_NEXUS_TAP_FOR_C_LINE = "{T}: Add {C}."
_BLINKMOTH_NEXUS_ANIMATE_LINE = (
    "{1}: This land becomes a 1/1 Blinkmoth artifact creature with "
    "flying until end of turn. It's still a land."
)
_BLINKMOTH_NEXUS_PUMP_LINE = (
    "{1}, {T}: Target Blinkmoth creature gets +1/+1 until end of turn."
)


def _try_build_blinkmoth_nexus_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a Blinkmoth-Nexus-shape ``CardPrinting`` if ``text``
    matches the strict three-line ``{T}: Add {C}.`` +
    Blinkmoth-animation + Blinkmoth-pump shape on a plain ``Land``
    type line, else ``None``.

    Plain ``Land`` only (no supertypes, no subtypes). Both line-1
    (explicit mana ability) and lines 2-3 (animation + subtype-targeted
    pump) are modeled on the resulting printing's
    ``activated_abilities``. ``produces=frozenset({"C"})`` is set
    because the line-1 mana ability is explicit (mirrors the static
    side, distinct from Inkmoth Nexus which drops the line and falls
    through to the engine's auto-tap-for-cost generic-mana fallback).

    Per parser/CLAUDE.md: a literal-line recognizer is acceptable
    here because Blinkmoth Nexus is "the only card in the current
    data pool that fits that printed surface" and "broader grammar
    deferred until a second example exists" (e.g. another land with
    a subtype-granting animation plus a subtype-targeted pump
    activation).
    """
    if supertypes:
        return None
    if subtypes:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if len(raw_lines) != 3:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _BLINKMOTH_NEXUS_TAP_FOR_C_LINE:
        return None
    if lines[1] != _BLINKMOTH_NEXUS_ANIMATE_LINE:
        return None
    if lines[2] != _BLINKMOTH_NEXUS_PUMP_LINE:
        return None
    return make_utility_land(
        card["name"],
        produces=frozenset({"C"}),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{1}"), mana_str="{1}",
                ),
                effect=BecomeLandCreatureUEOT(
                    power=1, toughness=1,
                    granted_keywords=frozenset({"flying"}),
                    granted_subtype="Blinkmoth",
                    # "a 1/1 Blinkmoth artifact creature"
                    granted_card_types=("Artifact",),
                ),
            ),
            ActivatedAbility(
                cost=ActivationCost(tap_self=True),
                effect=AddManaToPool(produces=(("C", 1),)),
                is_mana_ability=True,
            ),
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{1}"), mana_str="{1}",
                    tap_self=True,
                ),
                effect=PumpTargetCreatureUntilEOT(
                    power_bonus=1, toughness_bonus=1,
                    target_subtype="Blinkmoth",
                ),
            ),
        ),
    )


def _try_build_colorless_utility_land(
    card: dict,
    text: str,
    supertypes: tuple[str, ...],
    subtypes: tuple[str, ...],
) -> CardPrinting | None:
    """Return a colorless-utility-land ``CardPrinting`` if ``card`` has
    a plain ``Land`` type line (no supertypes, no subtypes) and its
    oracle text starts with ``"{T}: Add {C}."``, else ``None``.

    Matches the simplified Secluded Starforge shape: tap-for-{C} plus
    additional activated abilities the parser doesn't model. The extra
    abilities are intentionally dropped, mirroring the static-side
    ``make_colorless_land`` simplification — the land still works as a
    generic-mana source via the engine's auto-tap-for-cost fallback.

    Defensive: requires ``produced_mana`` (when present on the Scryfall
    record) to contain only colorless. This blocks accidentally
    matching a hypothetical colored-utility land whose first line
    happens to be ``{T}: Add {C}.``.

    Set-gate: skips cards from the Stellar Sights (``eos``) and
    Special Guests (``spg``) bonus sheets. Those reprints have their
    activated abilities fully modeled in the static registry via
    ``make_utility_land`` (and per-card bespoke effects), so the
    simplification would produce a parity mismatch. They are claimed
    via dedicated builders (e.g. ``_try_build_manland``) or skipped
    silently. Synthetic test fixtures and EoE main set cards (no set
    or ``set == "eoe"``) fall through normally.
    """
    if supertypes:
        return None
    if subtypes:
        return None
    if card.get("set") in {"eos", "spg"}:
        return None
    raw_lines = [ln.strip() for ln in text.split("\n") if ln.strip()]
    if not raw_lines:
        return None
    lines = [_strip_reminder_text(ln) for ln in raw_lines]
    if lines[0] != _COLORLESS_TAP_LINE:
        return None
    produced = card.get("produced_mana")
    if produced is not None and set(produced) - {"C"}:
        return None
    # Single-line case: a plain ``{T}: Add {C}.`` colorless land with no
    # other abilities. Preserve the historical ``make_colorless_land``
    # simplification (byte-identical to before) — the land taps for {C}
    # via the explicit-produces path / generic-mana fallback.
    if len(lines) == 1:
        return make_colorless_land(name=card["name"])
    # Multi-line case (Secluded Starforge): the land prints additional
    # activated abilities after the tap-for-{C} line. Route EVERY line
    # through the activated-ability grammar so the full printed surface
    # is faithful — NO silent drop of the trailing AAs. If ANY line
    # fails to parse, fall through to ``None`` (don't claim) so an
    # unmodeled rider is never silently dropped (parser CLAUDE.md
    # no-op-recognizer rule).
    abilities = []
    for ln in lines:
        ab = _parse_activated_ability_line(ln)
        if ab is None:
            return None
        abilities.append(ab)
    # ``produces`` is the union of colorless/colored mana the mana
    # abilities add (here just {C} from AA1). Derived from the parsed
    # AddManaToPool effects so it always matches the printed surface.
    produces: set[str] = set()
    for ab in abilities:
        if ab.is_mana_ability and isinstance(ab.effect, AddManaToPool):
            produces.update(sym for sym, _ in ab.effect.produces)
    return make_utility_land(
        card["name"],
        produces=frozenset(produces),
        activated_abilities=tuple(abilities),
    )
