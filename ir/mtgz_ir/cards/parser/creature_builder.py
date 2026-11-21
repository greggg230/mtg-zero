"""Creature ``CardPrinting`` builder.

Walks each line of the oracle text in order. Pre-clause checks
(Warp, Ward, Devour, cost-reduction rider, sorcery-speed rider)
consume the line directly; everything else goes through
``_parse_clause`` which dispatches to per-clause-type parsers. Any
unrecognized line skips the whole card (returns ``None``).

Every creature claim now flows through this unified oracle-text
grammar — the historical ``_BESPOKE_CREATURES``,
``_BESPOKE_ARTIFACT_CREATURES``, and ``_BESPOKE_LEGENDARY_CREATURES``
name-keyed dispatch tables were retired on 2026-05-01 (every entry
had been migrated to a parameterized recognizer; the tables sat
empty).
"""

from __future__ import annotations

import re
from dataclasses import replace

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
    AnticausalVestigeLeavesEffect,
    AstelliReclaimerETBEffect,
    BoilerbilgesRipperETBEffect,
    CardPrinting,
    CostReduction,
    DealDamageEqualToSourcePowerAnyTarget,
    DiscardSelf,
    DrawCards,
    ExileTargetCreatureUntilSourceLeaves,
    ExternalCostReduction,
    FanaticOfTheHarrowingETBEffect,
    HandAbilityGrant,
    HouseCartographerSurvivalEffect,
    KavaronHarrierAttackEffect,
    ModalChooseOne,
    MoveSelfCountersToTargetCreatureYouControl,
    OwnerPutsTargetWarpedExileCardOnBottomOfLibrary,
    PerigeeBeckonerDiesReturnTappedEffect,
    PossibilityTechnicianETBEffect,
    PumpTargetCreatureUntilEOT,
    PutPlusOneCounterOnCreatureYouControl,
    PutPlusOneCounterOnEachCreatureYouControl,
    PutPlusOneCounterOnSelf,
    PutPoisonCountersOnTargetPlayer,
    ReturnCreatureFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToHand,
    SearchLibraryForCard,
    SelfMill,
    Sequence,
    StaticAbility,
    StaticScope,
    TerrapactIntimidatorETBEffect,
    TerritorialBruntarLandfallEffect,
    TriggeredAbility,
    cost,
    make_creature,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _is_sorcery_rider_line
from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.creature_wg_lines import (
    link_exile_abilities,
    parse_wg_creature_line,
)
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.cards.parser.cost_reduction import (
    _parse_cost_reduction_line,
    _parse_external_cost_reduction_line,
)
from mtgz_ir.cards.parser.printing_keywords import (
    _parse_additional_land_per_turn_line,
    _parse_artifacts_have_blue_mana_ability_line,
    _parse_attack_bypass_if_artifact_entered_line,
    _parse_cast_artifact_from_top_line,
    _parse_combat_damage_cant_be_prevented_line,
    _parse_creature_spells_uncounterable_line,
    _parse_devour_line,
    _parse_dynamic_pt_formula_line,
    _parse_enters_tapped_line,
    _parse_enters_with_mana_spent_counters_line,
    _parse_enters_with_minus_one_counters_from_hand_cast_line,
    _parse_enters_with_plus_one_counter_if_creature_died_line,
    _parse_enters_with_x_counters_line,
    _parse_extra_etb_trigger_line,
    _parse_impending_line,
    _parse_landcycling_line,
    _parse_look_at_top_card_any_time_line,
    _parse_must_be_blocked_line,
    _parse_nonbasic_lands_are_mountains_line,
    _parse_play_lands_from_graveyard_line,
    _parse_replaces_draw_when_hand_at_most_one_line,
    _parse_spell_cant_be_countered_line,
    _parse_token_doubler_line,
    _parse_grants_food_and_ability_line,
    _parse_hand_warp_grant_line,
    _parse_toughness_as_damage_line,
    _parse_toughness_as_station_power_line,
    _parse_unblockable_if_two_spells_cast_this_turn_line,
    _parse_ward_discard_line,
    _parse_ward_line,
    _parse_ward_pay_life_line,
    _parse_keyword_line_with_ward,
    _parse_chooses_player_line,
    _parse_self_has_keywords_line,
    _parse_warp_castable_from_graveyard_line,
    _parse_warp_line,
    _parse_warp_with_life_cost_line,
)
from mtgz_ir.cards.parser.creature_rmc_grammar import (
    parse_rmc_creature_line_post,
    parse_rmc_creature_line_pre,
)
from mtgz_ir.cards.parser.cube_creatures_ub import parse_ub_creature_line
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.triggers import (
    _parse_predicate_trigger_phrase,
    _substitute_self_name,
    _strip_ability_word_prefix,
)
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.effects.object_predicates import (
    HasSubtype,
    IsArtifact,
    IsCreature,
    IsLand,
    IsTapped,
)
from mtgz_ir.effects.conditions import (
    AtLeastNSpellsCastThisTurn,
    ControlledArtifactEnteredThisTurn,
    HandSizeAtMost,
)
from mtgz_ir.effects.modifiers import (
    AddInstances,
    AddToCount,
    CantBeBlocked,
    ContinuousModifier,
    EventPattern,
    MultiplyCount,
    RemoveKeyword,
    ReplacementEffect,
)
from mtgz_ir.effects.predicates import Event
from mtgz_ir.effects.state_predicates import (
    Delirium,
    SpellsCastThisTurn,
)
from mtgz_ir.effects.predicates import (
    YOU,
    Controlled,
    Event,
    IsSelf,
    Source,
    StateBound,
    Subject,
)


# ── Modal creature trigger support ────────────────────────────────────────
#
# A modal triggered ability on a creature prints as:
#   ``When/Whenever <condition>, choose one —``       ← trigger header
#   ``• <mode 1 sentence>.``                           ← mode line(s)
#   ``• <mode 2 sentence>.``
#
# The header line ends with ``, choose one —`` (em-dash U+2014);
# each mode line begins with ``• `` (bullet U+2022 + space).
# The creature-builder loop processes lines one at a time, so modal
# blocks are pre-grouped before the main dispatch runs.
#
# Modal mode bodies use spell-style normalization (strip trailing period,
# lowercase first letter) because each mode is a full printed sentence
# rather than the ``it deals …`` creature-trigger form.
_MODAL_CHOOSE_ONE_SUFFIX = ", choose one —"
# "choose up to one —" (Ertai Resurrected): the same block plus a
# "choose no mode" option.
_MODAL_CHOOSE_UP_TO_ONE_SUFFIX = ", choose up to one —"
# "choose one that hasn't been chosen —" (Gollum, Riddle Master).
_MODAL_CHOOSE_UNUSED_SUFFIX = ", choose one that hasn't been chosen —"
_MODAL_MODE_PREFIX = "• "

# Trigger-header shape for a modal triggered ability. The leading
# ``When``/``Whenever`` and the trailing ``, choose one —`` are fixed;
# the condition body (group 1) is captured non-greedily.
_MODAL_TRIGGER_HEADER_RE = re.compile(
    r"^(?:When|Whenever) (.+?), choose (up to )?one"
    r"( that hasn't been chosen)? —$"
)


def _parse_modal_mode_phrase(raw_mode: str) -> "Effect | None":
    """Parse a modal mode line body into an ``Effect``.

    ``raw_mode`` is the stripped line after the ``• `` prefix — a full
    sentence with trailing period and uppercase initial letter (e.g.
    ``"Create two 1/1 white Human Soldier creature tokens."``). Applies
    spell-style normalization (strip period, lowercase first letter)
    before dispatching through the unified effect grammar.
    """
    phrase = raw_mode.strip()
    if phrase.endswith("."):
        phrase = phrase[:-1]
    if phrase and phrase[0].isupper():
        phrase = phrase[0].lower() + phrase[1:]
    return _parse_effect(phrase)


def _parse_modal_trigger_block(
    header: str,
    mode_lines: list[str],
    *,
    name: str | None = None,
) -> "TriggeredAbility | None":
    """Parse a modal trigger block (header + mode lines) into a single
    ``TriggeredAbility`` with a ``ModalChooseOne`` effect.

    ``header`` is the trigger-header line after stripping reminder text
    (e.g. ``"Whenever you cast your second spell each turn, choose one —"``).
    ``mode_lines`` are the subsequent ``• <mode>`` lines, also stripped.
    ``name`` is the card's own printed name (forwarded to trigger-phrase
    normalization, same as the regular trigger parser).

    Returns ``None`` if any piece fails to parse — the whole modal block
    is rejected rather than silently losing modes. Rejection propagates
    as a parse failure for the oracle clause, keeping the card unclaimed.
    """
    # Substitute self-name references and strip ability-word prefixes
    # on the header before matching the trigger condition — mirrors the
    # flow in ``_parse_triggered_ability_line`` for regular triggers.
    h = header
    if name is not None:
        h = _substitute_self_name(h, name)
    h = _strip_ability_word_prefix(h)
    m = _MODAL_TRIGGER_HEADER_RE.match(h)
    if m is None:
        return None
    condition = m.group(1)
    pred_match = _parse_predicate_trigger_phrase(condition)
    if pred_match is None:
        return None
    event, predicate = pred_match
    # Parse each mode body. At least two modes are required.
    if len(mode_lines) < 2:
        return None
    parsed_modes: list["Effect"] = []
    for raw in mode_lines:
        if not raw.startswith(_MODAL_MODE_PREFIX):
            return None
        body = raw[len(_MODAL_MODE_PREFIX):]
        if name is not None:
            body = _substitute_self_name(body, name)
        eff = _parse_modal_mode_phrase(body)
        if eff is None:
            return None
        parsed_modes.append(eff)
    if m.group(2):
        from mtgz_ir.effects.generic.cube_creatures_ub import ChooseNoMode

        parsed_modes.append(ChooseNoMode())
    if m.group(3):
        from mtgz_ir.effects.generic.cube_creatures_ub import (
            ModalChooseOneNotYetChosen,
        )

        if any(e.requires_target() for e in parsed_modes):
            return None
        return TriggeredAbility(
            event=event,
            predicate=predicate,
            effect=ModalChooseOneNotYetChosen(modes=tuple(parsed_modes)),
        )
    return TriggeredAbility(
        event=event,
        predicate=predicate,
        effect=ModalChooseOne(modes=tuple(parsed_modes)),
    )



# Historical note: a literal trigger-line dispatch table
# (``_BESPOKE_TRIGGER_LINES``) used to live here for creature oracle
# lines whose effect body used a bespoke engine class outside the
# ``_parse_effect_phrase`` grammar. Every entry was migrated to the
# unified grammar — Attack-in-the-Box, Wickerfolk Thresher, and
# Osseous Sticktwister via multi-sentence literal recognizers in
# ``effects._parse_effect`` (audit-litdispatch-creatures and
# audit-per-opponent-choice); Monoist Circuit-Feeder via the
# ``PairedTargetPumpDebuffUEOT`` primitive (audit-monoist-chorale).
# After the table sat empty for some time it was retired in
# retire-empty-bespoke-shells.


# "This creature enters with a +1/+1 counter on it." / "... with N
# +1/+1 counters on it." (Dockworker Drone) / "... with N -1/-1 counters on
# it." (Moonshadow's six): the kind and the count, else None. A count that
# depends on the board ("for each ...") or a condition ("if you cast it
# from your hand") is not this shape.
_ENTERS_WITH_PT_COUNTERS_LINE = re.compile(
    r"^This creature enters with "
    r"(a|one|two|three|four|five|six|seven|eight|nine|ten|\d+) "
    r"(\+1/\+1|-1/-1) counters? on it\.$"
)
_ENTERS_WITH_COUNT_WORDS = {
    "a": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
# "This creature enters with a +1/+1 counter on it for each <noun> you
# control." (Luxknight Breacher): the counted permanents, as an
# ``ObjectPredicate`` (``CardPrinting.enters_with_plus_one_counters_per``).
_ENTERS_WITH_PLUS_ONE_COUNTER_PER_LINE = re.compile(
    r"^This creature enters with a \+1/\+1 counter on it for each "
    r"(?P<noun>.+) you control\.$"
)


def _enters_with_per_noun(noun: str):
    from mtgz_ir.effects.object_predicates import (
        ControlledByYou, IsArtifact, IsCreature, IsSource,
    )

    other = noun.startswith("other ")
    if other:
        noun = noun[len("other "):]
    kinds = {
        "creature": IsCreature(),
        "artifact": IsArtifact(),
        "creature and/or artifact": IsCreature() | IsArtifact(),
        "artifact and/or creature": IsArtifact() | IsCreature(),
    }
    pred = kinds.get(noun)
    if pred is None:
        return None
    pred = pred & ControlledByYou()
    return pred & ~IsSource() if other else pred


def _parse_enters_with_pt_counters_line(line: str) -> tuple[str, int] | None:
    """``(kind, n)`` of "This creature enters with <n> <+1/+1 | -1/-1>
    counter(s) on it.", else ``None``."""
    m = _ENTERS_WITH_PT_COUNTERS_LINE.match(line)
    if m is None:
        return None
    word = m.group(1)
    n = int(word) if word.isdigit() else _ENTERS_WITH_COUNT_WORDS[word]
    return (m.group(2), n)


def _build_creature_printing(card: dict) -> CardPrinting | None:
    """Try to produce a ``CardPrinting`` for ``card`` (a Scryfall
    record). Returns ``None`` if the card is not a creature, has
    non-integer power/toughness (e.g. ``*``), or contains any
    oracle-text clause the slice doesn't yet support.

    Every Creature claim now flows through the unified oracle-text
    grammar — the historical ``_BESPOKE_CREATURES``,
    ``_BESPOKE_ARTIFACT_CREATURES``, and ``_BESPOKE_LEGENDARY_CREATURES``
    name-keyed dispatch tables were retired on 2026-05-01 (every entry
    had been migrated to a parameterized recognizer; the tables sat
    empty).
    """
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Creature" not in card_types:
        return None

    # Pre-scan oracle text to detect dynamic P/T formulas before the
    # main line loop. A ``*``-based Scryfall power/toughness value is
    # only valid when the oracle text contains a known dynamic-PT line;
    # cards with ``*`` P/T but no such line (e.g. a vanilla ``*``-power
    # card whose formula isn't implemented yet) still return None.
    _raw_text = card.get("oracle_text", "")
    _has_dynamic_pt = any(
        _parse_dynamic_pt_formula_line(_strip_reminder_text(l))
        for l in _raw_text.split("\n")
        if _strip_reminder_text(l)
    )

    def _star_pt(val: str) -> int | None:
        """Return numeric base for a ``*``-based P/T string, else None.
        ``"*"`` → 0; ``"1+*"`` / ``"*+1"`` → 1; other offsets likewise.
        """
        import re as _re
        v = val.strip()
        if v == "*":
            return 0
        m = _re.match(r"^(\d+)\+\*$", v) or _re.match(r"^\*\+(\d+)$", v)
        return int(m.group(1)) if m else None

    try:
        power = int(card["power"])
    except (KeyError, ValueError):
        power = _star_pt(card.get("power", "")) if _has_dynamic_pt else None
        if power is None:
            return None
    try:
        toughness = int(card["toughness"])
    except (KeyError, ValueError):
        toughness = _star_pt(card.get("toughness", "")) if _has_dynamic_pt else None
        if toughness is None:
            return None

    keywords: set[str] = set()
    triggers: list[TriggeredAbility] = []
    activated: list[ActivatedAbility] = []
    statics: list[StaticAbility] = []
    cost_reductions: list[CostReduction] = []
    external_cost_reductions: list[ExternalCostReduction] = []
    warp: str | None = None
    warp_additional_life_cost: int = 0
    warp_castable_from_graveyard: bool = False
    ward: str | None = None
    ward_discard_chosen = False
    ward_discard_random = False
    ward_life_cost = 0
    chooses_player_as_enters = False
    devour: int = 0
    impending: str | None = None
    impending_count: int = 0
    enchantment: bool = "Enchantment" in card_types
    enters_tapped = False
    must_be_blocked_if_able = False
    nonbasic_lands_are_mountains = False
    landcycling: tuple[str, str] | None = None
    enters_with_plus_one_counter_if_creature_died_this_turn = False
    enters_with_minus_one_counters_from_hand_cast = 0
    # "This creature enters with N +1/+1 counters on it." (Dockworker
    # Drone): a replacement effect (CR 614.1c, 122.6), not a trigger.
    enters_with_named_counters: list[tuple[str, int]] = []
    enters_with_plus_one_counters_per = None
    dynamic_pt_formula = None
    enters_with_counters_from_x: str | None = None
    grants_additional_land_per_turn = 0
    grants_play_lands_from_graveyard = False
    grants_look_at_top_of_library = False
    grants_cast_artifact_from_top_of_library = False
    grants_blue_mana_ability_to_your_artifacts = False
    spell_is_uncounterable = False
    combat_damage_cant_be_prevented = False
    grants_toughness_as_damage_for_creatures_you_control = False
    grants_toughness_as_station_power_for_creatures_you_control = False
    hand_ability_grants: "tuple[HandAbilityGrant, ...]" = ()
    grants_food_to_your_artifacts = False
    grants_food_ability_to_your_artifacts = False
    # Cube creatures (R / multicolor / colorless) bucket contributions.
    rmc_hand_abilities: list = []
    rmc_graveyard_abilities: list = []
    rmc_printing_fields: dict = {}

    def _absorb_rmc_line(rmc) -> None:
        keywords.update(rmc.keywords)
        triggers.extend(rmc.triggers)
        activated.extend(rmc.activated)
        statics.extend(rmc.statics)
        rmc_hand_abilities.extend(rmc.hand_abilities)
        rmc_graveyard_abilities.extend(rmc.graveyard_abilities)
        rmc_printing_fields.update(rmc.printing_fields)

    # Cube W/G creature bucket (``creature_wg_lines``): printing fields
    # contributed by lines the pre-existing grammar declines.
    wg_fields: dict = {}
    ub_fields: dict = {}
    ub_hand_abilities: list[ActivatedAbility] = []
    text = card.get("oracle_text", "")
    # Pre-process oracle lines into groups. Most lines become a 1-element
    # group ``(stripped_line,)``. A modal trigger block (header ending
    # with ``, choose one —`` followed by ``• <mode>`` lines) becomes a
    # 2-element group ``(header_line, [mode_line, ...])``. Pre-grouping
    # lets the main loop stay a simple ``for group in groups`` dispatch
    # without look-ahead, and modal groups are rejected as a unit if any
    # piece fails to parse (no partial progress).
    _raw_lines = [
        _strip_reminder_text(l)
        for l in text.split("\n")
        if _strip_reminder_text(l)
    ]
    _groups: list[tuple] = []
    _i = 0
    while _i < len(_raw_lines):
        _ln = _raw_lines[_i]
        if _ln.endswith(_MODAL_CHOOSE_ONE_SUFFIX) or _ln.endswith(
            _MODAL_CHOOSE_UP_TO_ONE_SUFFIX
        ) or _ln.endswith(_MODAL_CHOOSE_UNUSED_SUFFIX):
            # Modal trigger header — collect subsequent bullet mode lines.
            _modes: list[str] = []
            _j = _i + 1
            while _j < len(_raw_lines) and _raw_lines[_j].startswith(_MODAL_MODE_PREFIX):
                _modes.append(_raw_lines[_j])
                _j += 1
            if _modes:
                _groups.append((_ln, _modes))
                _i = _j
                continue
        _groups.append((_ln,))
        _i += 1
    for _group in _groups:
        # Unpack the group: single-element groups are normal lines;
        # two-element groups are modal trigger blocks.
        if len(_group) == 2:
            # Modal trigger block: ``(header, [mode_line, ...])``.
            # Delegate to ``_parse_modal_trigger_block``; append the
            # resulting ``TriggeredAbility`` to ``triggers``, or
            # return None if the block fails to parse.
            _header, _mode_lines = _group
            _modal_ta = _parse_modal_trigger_block(
                _header, _mode_lines, name=card["name"],
            )
            if _modal_ta is None:
                return None
            triggers.append(_modal_ta)
            continue
        line = _group[0]
        if not line:
            continue
        warp_life_result = _parse_warp_with_life_cost_line(line)
        if warp_life_result is not None:
            # ``Warp—{cost}, Pay N life.`` — em-dash warp with an
            # additional life cost (Timeline Culler). Sets both the
            # ``warp`` cost and ``warp_additional_life_cost``.
            warp, warp_additional_life_cost = warp_life_result
            continue
        warp_cost = _parse_warp_line(line)
        if warp_cost is not None:
            # A card should only have one Warp clause; if a malformed
            # printing somehow had two we just keep the last — parity
            # against the static registry will catch any drift.
            warp = warp_cost
            continue
        hand_warp = _parse_hand_warp_grant_line(line)
        if hand_warp is not None:
            # ``<types> cards in your hand have warp {cost}.`` — Tannuk,
            # Steadfast Second's printed hand-warp grant. ``hand_warp``
            # is a structured ``HandAbilityGrant`` carrying both the
            # type filter and the granted warp cost. The legal-actions
            # enumerator in ``legal.py`` walks this tuple to offer
            # warp-cast actions for matching hand cards.
            hand_ability_grants = hand_ability_grants + (hand_warp,)
            continue
        if _parse_warp_castable_from_graveyard_line(line):
            # ``You may cast this card from your graveyard using its warp
            # ability.`` — Timeline Culler's graveyard-castability rider.
            # Flips the ``warp_castable_from_graveyard`` kwarg below.
            warp_castable_from_graveyard = True
            continue
        ward_cost = _parse_ward_line(line)
        if ward_cost is not None:
            # A card should only have one Ward clause; if a malformed
            # printing somehow had two we just keep the last — parity
            # against the static registry will catch any drift.
            ward = ward_cost
            continue
        ward_discard_kind = _parse_ward_discard_line(line)
        if ward_discard_kind is not None:
            # Em-dash discard-cost ward riders. The tag string from the
            # parser ("chosen" or "random") maps 1:1 to the matching
            # boolean kwarg on ``make_creature``. As with the bare
            # ``Ward {cost}`` clause above, a single card carries at
            # most one Ward and parity against the static registry
            # catches any drift; we keep the last-seen marker if a
            # malformed printing somehow listed two.
            if ward_discard_kind == "random":
                ward_discard_random = True
            else:
                ward_discard_chosen = True
            continue
        ward_life = _parse_ward_pay_life_line(line)
        if ward_life is not None:
            ward_life_cost = ward_life
            continue
        kw_with_ward = _parse_keyword_line_with_ward(line)
        if kw_with_ward is not None:
            # ``Flying, ward {1}`` — keyword line whose last piece is a
            # Ward {cost} (Raffine, Scheming Seer).
            keywords.update(kw_with_ward[0])
            ward = kw_with_ward[1]
            continue
        devour_n = _parse_devour_line(line)
        if devour_n is not None:
            # A card should only have one Devour clause; if a malformed
            # printing somehow had two we just keep the last — parity
            # against the static registry will catch any drift.
            devour = devour_n
            continue
        impending_result = _parse_impending_line(line)
        if impending_result is not None:
            # ``Impending N—{cost}`` — Duskmourn Overlord cycle alt-cost.
            # When cast for the impending cost the permanent enters with N
            # time counters and ``impending_active=True``, suppressing its
            # printed creature type (``is_creature_now`` returns False)
            # until the last counter is removed at the active player's end
            # step (``run_impending_decrement_turn_based_action``). A card
            # carries at most one Impending clause; if a malformed printing
            # somehow had two we keep the last — parity against the static
            # registry will catch drift.
            impending_count, impending = impending_result
            continue
        if _parse_chooses_player_line(line):
            chooses_player_as_enters = True
            continue
        self_kws = _parse_self_has_keywords_line(line)
        if self_kws is not None:
            keywords.update(self_kws)
            continue
        if _parse_enters_tapped_line(line):
            # ``This creature enters tapped.`` — printing-level static
            # replacement. Flips the ``enters_tapped`` kwarg below; the
            # engine already consults the printing's ``enters_tapped``
            # field at ETB. Idempotent: a hypothetical malformed
            # printing with the line twice still ends up with the
            # field set once.
            enters_tapped = True
            continue
        if _parse_enters_with_plus_one_counter_if_creature_died_line(line):
            # ``This creature enters with a +1/+1 counter on it if a
            # creature died this turn.`` — printing-level ETB
            # replacement. Flips the matching kwarg below; the engine
            # reads ``state.creatures_died_this_turn`` at ETB and adds
            # one +1/+1 counter when positive (Cackling Slasher).
            enters_with_plus_one_counter_if_creature_died_this_turn = True
            continue
        pt_counters = _parse_enters_with_pt_counters_line(line)
        if pt_counters is not None:
            # ``This creature enters with a +1/+1 counter on it.`` (or N
            # -1/-1 counters, Moonshadow) — the permanent is on the
            # battlefield with the counters from the start (CR 614.1c,
            # 122.6): ``enters_with_named_counters``.
            enters_with_named_counters.append(pt_counters)
            continue
        m_per = _ENTERS_WITH_PLUS_ONE_COUNTER_PER_LINE.match(line)
        if m_per is not None:
            # A replacement (CR 614.1c), not an enters trigger (mtg-zero
            # docs/ir_defects.md 21).
            per = _enters_with_per_noun(m_per.group("noun"))
            if per is None:
                return None
            enters_with_plus_one_counters_per = per
            continue
        n_minus = _parse_enters_with_minus_one_counters_from_hand_cast_line(
            line,
        )
        if n_minus is not None:
            # ``This creature enters with N -1/-1 counters on it if you
            # cast it from your hand.`` — printing-level ETB
            # replacement. The cast-from-hand origin is tracked on the
            # GameObject via ``cast_via_hand`` set in
            # ``actions._cast_spell``'s cast-from-hand branch and
            # cleared on every other cast path; the engine adds N
            # -1/-1 counters at ETB only when the flag is set
            # (Patched Plaything).
            enters_with_minus_one_counters_from_hand_cast = n_minus
            continue
        if _parse_must_be_blocked_line(line):
            # ``This creature must be blocked if able.`` — printing-
            # level block requirement (CR 509.1c). Flips the
            # ``must_be_blocked_if_able`` kwarg below; the engine's
            # ``_declare_blockers`` enforces the requirement by
            # forcing a block when the defending player skipped it
            # but has an eligible blocker.
            must_be_blocked_if_able = True
            continue
        if _parse_nonbasic_lands_are_mountains_line(line):
            # ``Nonbasic lands are Mountains.`` — Magus of the Moon's
            # printing-level type-replacement static. Flips the
            # ``nonbasic_lands_are_mountains`` kwarg below; the engine
            # consumes the flag at land ETB and layer-4 type-
            # replacement application time. Idempotent: a malformed
            # printing with the line twice ends up with the field
            # set once.
            nonbasic_lands_are_mountains = True
            continue
        if _parse_token_doubler_line(line):
            # ``If one or more tokens would be created under your
            # control, twice that many of those tokens are created
            # instead.`` — Exalted Sunborn's printed token-doubler
            # replacement effect. Expressed as a SELF-scope StaticAbility
            # carrying a ReplacementEffect that multiplies the
            # WOULD_CREATE_TOKEN count by 2. The dispatcher's implicit
            # gate (source's controller IS the affected player) handles
            # the printed "under your control" qualifier, so no
            # additional subject_filter is needed.
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
        if _parse_extra_etb_trigger_line(line):
            # ``If a permanent entering the battlefield causes a
            # triggered ability of a permanent you control to trigger,
            # that ability triggers an additional time.`` — Starfield
            # Vocalist's Panharmonicon-style ETB-trigger doubler.
            # Builds a SELF-scope StaticAbility carrying a
            # ReplacementEffect(EventPattern(
            # TRIGGERED_ABILITY_TRIGGERS, subject_filters),
            # AddInstances(1)). Each matched line produces its own
            # StaticAbility, so a hypothetical printing with the line
            # twice gets two replacements that compose additively in
            # CR 616.1 order (no card in the pool today carries the
            # line more than once). The subject filters say which
            # triggers (mtg-zero docs/ir_defects.md 66): the ability's
            # source is a permanent (not an emblem, not a card in a
            # graveyard) and the trigger was caused by a permanent
            # entering the battlefield; the dispatcher's implicit
            # "source's controller is the affected player" gate
            # (``affects`` you) handles "you control".
            from mtgz_ir.effects.value_predicates import IsPermanent

            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                replacement_effect=ReplacementEffect(
                    event_pattern=EventPattern(
                        event=Event.TRIGGERED_ABILITY_TRIGGERS,
                        subject_filters={
                            "ability_source": IsPermanent(),
                            "entering_obj": IsPermanent(),
                        },
                    ),
                    transform=AddInstances(delta=1),
                ),
            ))
            continue
        if _parse_replaces_draw_when_hand_at_most_one_line(line):
            # ``As long as you have one or fewer cards in hand, if you
            # would draw one or more cards, you draw that many cards
            # plus one instead.`` — Quantum Riddler's printed
            # draw-replacement static. SELF-scope StaticAbility gated
            # by HandSizeAtMost(1) (state-aware Condition checked at
            # replacement-collection time), carrying a
            # ReplacementEffect(EventPattern(WOULD_DRAW_CARDS),
            # AddToCount(1)). The dispatcher's implicit "source's
            # controller is the affected player" gate handles the
            # "you would draw" qualifier; HandSizeAtMost handles the
            # printed "as long as you have one or fewer cards in
            # hand" rider.
            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                condition=HandSizeAtMost(1),
                replacement_effect=ReplacementEffect(
                    event_pattern=EventPattern(
                        event=Event.WOULD_DRAW_CARDS,
                    ),
                    transform=AddToCount(delta=1),
                ),
            ))
            continue
        if _parse_attack_bypass_if_artifact_entered_line(line):
            # ``As long as an artifact entered the battlefield under
            # your control this turn, this creature can attack as
            # though it didn't have defender.`` — Mechan Shieldmate's
            # printed defender-bypass. Expressed as a SELF-scope
            # StaticAbility carrying a ContinuousModifier that lets this
            # creature attack as though it didn't have defender while
            # ControlledArtifactEnteredThisTurn() holds; it keeps the
            # keyword (CR 702.3b; mtg-zero docs/ir_defects.md 24).
            from mtgz_ir.effects.modifiers import CanAttackAsThoughNoDefender

            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                condition=ControlledArtifactEnteredThisTurn(),
                continuous_effect=ContinuousModifier(
                    layer=6,
                    payload=CanAttackAsThoughNoDefender(),
                ),
            ))
            continue
        if _parse_unblockable_if_two_spells_cast_this_turn_line(line):
            # ``This creature can't be blocked if you've cast two or
            # more spells this turn.`` — Illvoi Infiltrator's printed
            # conditional-unblockable. Expressed as a SELF-scope
            # StaticAbility carrying a ContinuousModifier whose
            # payload is ``CantBeBlocked()`` (a layer-6 continuous
            # gameplay-action restriction, structurally distinct from
            # real keyword grants) while AtLeastNSpellsCastThisTurn(2)
            # holds. combat.can_block / declare_blockers reach this
            # via game.is_cant_be_blocked, which walks
            # walk_continuous_modifiers for ``CantBeBlocked`` payloads.
            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                condition=AtLeastNSpellsCastThisTurn(2),
                continuous_effect=ContinuousModifier(
                    layer=6,
                    payload=CantBeBlocked(),
                ),
            ))
            continue
        dpt_formula = _parse_dynamic_pt_formula_line(line)
        if dpt_formula is not None:
            # Dynamic P/T formula line (e.g. Emissary Escort, Cosmogoyf,
            # Harmonious Grovestrider). The ``PTFormula`` is forwarded as
            # ``dynamic_pt_formula`` to ``make_creature``;
            # ``game._dynamic_base_pt`` asks it for the actual P/T at query
            # time. A card carries at most one such clause.
            dynamic_pt_formula = dpt_formula
            continue
        if _parse_enters_with_x_counters_line(line):
            # ``This creature enters with X +1/+1 counters on it.``
            # Sets ``enters_with_counters_from_x="x"``; the engine reads
            # ``state.resolving_x_value`` at ETB and adds that many
            # +1/+1 counters (Broodguard Elite). Idempotent.
            enters_with_counters_from_x = "x"
            continue
        if _parse_additional_land_per_turn_line(line):
            # ``You may play <N> additional land(s) on each of your
            # turns.`` Adds N to the grant counter (Azusa: two), matching
            # the engine field's int semantics.
            from mtgz_ir.cards.parser.printing_keywords import (
                parse_additional_lands_per_turn,
            )

            grants_additional_land_per_turn += parse_additional_lands_per_turn(
                line,
            )
            continue
        if _parse_play_lands_from_graveyard_line(line):
            # ``You may play lands from your graveyard.`` — Icetill
            # Explorer's graveyard land-play static. Idempotent.
            grants_play_lands_from_graveyard = True
            continue
        if _parse_enters_with_mana_spent_counters_line(line):
            # ``Dyadrine enters with a number of +1/+1 counters on it
            # equal to the amount of mana spent to cast it.`` —
            # Dyadrine, Synthesis Amalgam's printed ETB replacement.
            # Sets ``enters_with_counters_from_x="mana_spent"``; the
            # engine computes the total mana paid at ETB time.
            enters_with_counters_from_x = "mana_spent"
            continue
        if _parse_look_at_top_card_any_time_line(line):
            # ``You may look at the top card of your library any time.``
            # — Mm'menon's passive information-access clause. Recorded
            # faithfully on the printing; simulation behavior unchanged
            # (both players have perfect information in the sim).
            grants_look_at_top_of_library = True
            continue
        if _parse_cast_artifact_from_top_line(line):
            # ``You may cast artifact spells from the top of your
            # library.`` — Mm'menon's printed library-top artifact-cast
            # grant. Sets ``grants_cast_artifact_from_top_of_library``;
            # ``legal_actions`` reads the flag to offer top-of-library
            # artifact casts.
            grants_cast_artifact_from_top_of_library = True
            continue
        if _parse_artifacts_have_blue_mana_ability_line(line):
            # ``Artifacts you control have "{T}: Add {U}. Spend this
            # mana only to cast a spell from anywhere other than your
            # hand."`` — Mm'menon's printed static-grants-activated-
            # ability clause. Sets
            # ``grants_blue_mana_ability_to_your_artifacts``; the engine
            # appends ``MMNEMON_BLUE_MANA_GRANTED_ABILITY`` to artifact
            # activations for the controller.
            grants_blue_mana_ability_to_your_artifacts = True
            continue
        if _parse_spell_cant_be_countered_line(line):
            # ``This spell can't be countered.`` — printing-level immunity
            # to counter effects. Flips ``spell_is_uncounterable`` kwarg
            # below; ``CounterTargetSpell`` excludes spells with this flag
            # from its legal-target list and fizzles at resolve time.
            spell_is_uncounterable = True
            continue
        if _parse_creature_spells_uncounterable_line(line):
            # ``Creature spells you control can't be countered.`` —
            # static ability granting uncounterability to all creature
            # spells the controller casts while this permanent is on the
            # battlefield. Wired as a SELF-scope ``StaticAbility`` with
            # ``grants_creature_spells_uncounterable=True``; the flag is
            # checked in ``CounterTargetSpell`` against the spell
            # controller's battlefield.
            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                grants_creature_spells_uncounterable=True,
            ))
            continue
        if _parse_combat_damage_cant_be_prevented_line(line):
            # ``Combat damage can't be prevented.`` — printing-level
            # static causing this creature's damage events to bypass
            # global prevention (Darkness-style). Flips the
            # ``combat_damage_cant_be_prevented`` kwarg below; the engine
            # checks the flag per-source in
            # ``combat._source_bypasses_prevention``.
            combat_damage_cant_be_prevented = True
            continue
        if _parse_toughness_as_damage_line(line):
            # ``Each creature you control with toughness greater than its
            # power assigns combat damage equal to its toughness rather
            # than its power.`` — CR 510.1b replacement effect. Flips the
            # ``grants_toughness_as_damage_for_creatures_you_control``
            # kwarg below; the engine checks the flag per-controller in
            # ``combat._effective_combat_damage_amount``.
            grants_toughness_as_damage_for_creatures_you_control = True
            continue
        if _parse_toughness_as_station_power_line(line):
            # ``Each creature you control with toughness greater than its
            # power stations permanents using its toughness rather than
            # its power.`` — station-power replacement effect. Flips the
            # ``grants_toughness_as_station_power_for_creatures_you_control``
            # kwarg below; the engine checks the flag per-controller in
            # ``actions._station_spacecraft``.
            grants_toughness_as_station_power_for_creatures_you_control = True
            continue
        landcycling_match = _parse_landcycling_line(line)
        if landcycling_match is not None:
            # ``Plainscycling/Islandcycling/Swampcycling/Mountaincycling/
            # Forestcycling {cost}`` — basic-landcycling. Stored as
            # declarative metadata on the printing
            # (``CardPrinting.landcycling = (subtype, cost_str)``); the
            # activation pipeline isn't wired today, so the cycling
            # ability cannot be paid in simulation. The field is
            # populated honestly so the printing reflects oracle text;
            # claiming the card on the basis of this recognition keeps
            # parser parity with the printed oracle line. A card
            # carries at most one landcycling clause; if a malformed
            # printing somehow listed two we keep the last (matching
            # the Warp/Ward/Devour precedent above).
            landcycling = landcycling_match
            continue
        cost_reduction = _parse_cost_reduction_line(line)
        if cost_reduction is not None:
            # ``This spell costs {N} less to cast if <condition>.``
            # rider — accumulate; multiple riders on the same card are
            # legal in principle (no EoE printing has more than one
            # today, but the engine handles a tuple).
            cost_reductions.append(cost_reduction)
            continue
        external_cost_reduction = _parse_external_cost_reduction_line(line)
        if external_cost_reduction is not None:
            # Permanent-on-battlefield static that grants a flat cost
            # reduction to OTHER spells the controller casts (e.g.
            # Uthros Psionicist's ``The second spell you cast each turn
            # costs {2} less to cast.``). Accumulated; the engine's
            # ``apply_cost_reductions`` scans every ETB permanent for
            # these at cast time.
            external_cost_reductions.append(external_cost_reduction)
            continue
        if _parse_grants_food_and_ability_line(line):
            # ``Artifacts you control are Foods in addition to their other
            # types and have "{2}, {T}, Sacrifice this artifact: You gain 3
            # life."`` — Ragost, Deft Gastronaut. Sets both the
            # ``grants_food_to_your_artifacts`` (game.effective_subtypes
            # injects "Food") and ``grants_food_ability_to_your_artifacts``
            # (game.effective_activated_abilities appends the Food sac
            # ability) flags on the printing.
            grants_food_to_your_artifacts = True
            grants_food_ability_to_your_artifacts = True
            continue
        if _is_sorcery_rider_line(line):
            # The rider attaches to the immediately preceding activated
            # ability on the same card. With no preceding ability there's
            # nothing to mark sorcery-speed and the card is skipped — a
            # bare rider line on its own is malformed oracle text.
            if not activated:
                return None
            activated[-1] = replace(activated[-1], sorcery_speed=True)
            continue
        # Cube creatures (R / multicolor / colorless) bucket: abilities
        # that function from the hand / graveyard, Exhaust, Dash, ...
        # must be routed before the generic clause dispatcher files them
        # as battlefield abilities.
        rmc = parse_rmc_creature_line_pre(line, card["name"])
        if rmc is not None:
            _absorb_rmc_line(rmc)
            continue
        # U/B cube-creature grammar (``parser.cube_creatures_ub``):
        # draw replacements, Ninjutsu, ... Tried before the generic
        # clause dispatch; contributes abilities and printing fields.
        # Legacy creatures outside the cube (Murktide Regent: delve, "enters
        # with a +1/+1 counter for each ... card exiled with it", a card
        # leaving your graveyard; ``legacy_creatures``).
        from mtgz_ir.cards.parser.legacy_creatures import (
            parse_legacy_creature_line,
        )

        legacy_line = parse_legacy_creature_line(line, card["name"])
        if legacy_line is not None:
            triggers.extend(legacy_line.triggers)
            ub_fields.update(legacy_line.fields)
            continue
        ub_line = parse_ub_creature_line(
            _substitute_self_name(line, card["name"]), card["name"],
        )
        if ub_line is not None:
            keywords.update(ub_line.keywords)
            triggers.extend(ub_line.triggers)
            activated.extend(ub_line.activated)
            statics.extend(ub_line.statics)
            ub_hand_abilities.extend(ub_line.hand_abilities)
            cost_reductions.extend(ub_line.cost_reductions)
            ub_fields.update(ub_line.fields)
            continue
        # Pass the printing's own name to the clause dispatcher so the
        # triggered-ability layer can substitute name references
        # (``Haliya``, ``Syr Vondam``, etc.) with the canonical
        # ``this creature`` surface before condition / effect lookup.
        # Non-trigger clause types ignore the parameter today.
        parsed = _parse_clause(line, name=card["name"])
        if parsed is None:
            rmc = parse_rmc_creature_line_post(line, card["name"])
            if rmc is not None:
                _absorb_rmc_line(rmc)
                continue
            # Cube W/G creature bucket: consulted only for lines the
            # grammar above declined, so it can't change a claim.
            wg = parse_wg_creature_line(line, card)
            if wg is None:
                # Cube layouts bucket: lines of the bucket's faces.
                from mtgz_ir.cards.parser.layout_face_grammar import (
                    parse_face_creature_line,
                )

                wg = parse_face_creature_line(line, card)
            if wg is None:
                # Roto P1 (stack & restrictions) lines.
                from mtgz_ir.cards.parser.roto_stack import (
                    merge_fields, parse_roto_stack_line,
                )

                roto = parse_roto_stack_line(line, card)
                if roto is not None:
                    merge_fields(wg_fields, roto.fields)
                    keywords.update(roto.keywords)
                    triggers.extend(roto.triggers)
                    activated.extend(roto.activated)
                    statics.extend(roto.statics)
                    continue
                # Vintage Roto P3 (mana triggers, ...).
                from mtgz_ir.cards.parser.roto_mana import (
                    parse_roto_creature_line,
                )

                wg = parse_roto_creature_line(line, card)
            if wg is None:
                # Vintage Roto P5 lines (board rules, ...).
                from mtgz_ir.cards.parser.roto_board import (
                    parse_roto_board_line_into,
                )

                wg = parse_roto_board_line_into(line, card, wg_fields)
            if wg is None:
                # Roto P2 static lines (graveyard exile-instead, ...).
                from mtgz_ir.cards.parser.roto_zones import (
                    parse_roto_zones_creature_line,
                )

                wg = parse_roto_zones_creature_line(line, card)
            if wg is None:
                return None
            if "zone_abilities" in wg.fields:
                # Roto P2 zone abilities accumulate across lines.
                wg_fields["zone_abilities"] = tuple(
                    wg_fields.get("zone_abilities", ())
                ) + tuple(wg.fields["zone_abilities"])
                wg = replace(wg, fields={
                    k: v for k, v in wg.fields.items() if k != "zone_abilities"
                })
            wg_fields.update(wg.fields)
            keywords.update(wg.keywords)
            triggers.extend(wg.triggers)
            activated.extend(wg.activated)
            statics.extend(wg.statics)
            continue
        keywords.update(parsed.keywords)
        # ``triggers`` is a tuple — one entry per condition arm. A
        # standard single-condition line yields a 1-tuple; a compound
        # condition like ``When this creature enters or dies, …`` yields
        # a 2-tuple (one ``TriggeredAbility`` per event). The builder
        # treats each arm as if it were its own oracle line, mirroring
        # the static-registry shape (Thawbringer's static carries two
        # ``TriggeredAbility`` entries for its single oracle line).
        triggers.extend(parsed.triggers)
        if parsed.activated is not None:
            activated.append(parsed.activated)
        if parsed.statics:
            statics.extend(parsed.statics)

    # Cube W/G creature bucket: linked "exiled with" ability pairs.
    link_exile_abilities(triggers, activated, statics)
    printing = make_creature(
        name=card["name"],
        cost=card["mana_cost"],
        power=power,
        toughness=toughness,
        keywords=keywords,
        triggered_abilities=tuple(triggers),
        activated_abilities=tuple(activated),
        static_abilities=tuple(statics),
        cost_reductions=tuple(cost_reductions),
        external_cost_reductions=tuple(external_cost_reductions),
        subtypes=subtypes,
        legendary="Legendary" in supertypes,
        artifact="Artifact" in card_types,
        enchantment=enchantment,
        warp=warp,
        warp_additional_life_cost=warp_additional_life_cost,
        warp_castable_from_graveyard=warp_castable_from_graveyard,
        ward=ward,
        ward_discard_chosen=ward_discard_chosen,
        ward_discard_random=ward_discard_random,
        ward_life_cost=ward_life_cost,
        chooses_player_as_enters=chooses_player_as_enters,
        devour_land_multiplier=devour,
        impending=impending,
        impending_count=impending_count,
        enters_tapped=enters_tapped,
        must_be_blocked_if_able=must_be_blocked_if_able,
        landcycling=landcycling,
        enters_with_plus_one_counter_if_creature_died_this_turn=(
            enters_with_plus_one_counter_if_creature_died_this_turn
        ),
        enters_with_minus_one_counters_from_hand_cast=(
            enters_with_minus_one_counters_from_hand_cast
        ),
        nonbasic_lands_are_mountains=nonbasic_lands_are_mountains,
        dynamic_pt_formula=dynamic_pt_formula,
        enters_with_counters_from_x=enters_with_counters_from_x,
        grants_additional_land_per_turn=grants_additional_land_per_turn,
        grants_play_lands_from_graveyard=grants_play_lands_from_graveyard,
        grants_look_at_top_of_library=grants_look_at_top_of_library,
        grants_cast_artifact_from_top_of_library=(
            grants_cast_artifact_from_top_of_library
        ),
        grants_blue_mana_ability_to_your_artifacts=(
            grants_blue_mana_ability_to_your_artifacts
        ),
        spell_is_uncounterable=spell_is_uncounterable,
        combat_damage_cant_be_prevented=combat_damage_cant_be_prevented,
        grants_toughness_as_damage_for_creatures_you_control=(
            grants_toughness_as_damage_for_creatures_you_control
        ),
        grants_toughness_as_station_power_for_creatures_you_control=(
            grants_toughness_as_station_power_for_creatures_you_control
        ),
        hand_ability_grants=hand_ability_grants,
        grants_food_to_your_artifacts=grants_food_to_your_artifacts,
        grants_food_ability_to_your_artifacts=(
            grants_food_ability_to_your_artifacts
        ),
    )
    if rmc_hand_abilities:
        rmc_printing_fields["hand_activated_abilities"] = (
            tuple(printing.hand_activated_abilities)
            + tuple(rmc_hand_abilities)
        )
    if rmc_graveyard_abilities:
        rmc_printing_fields["graveyard_activated_abilities"] = tuple(
            rmc_graveyard_abilities
        )
    # "As this creature enters, choose a color." only means something
    # together with the line that uses the chosen color (Painter's
    # Servant): claim both or neither.
    from mtgz_ir.cards.parser.creature_rmc_grammar import (
        assemble_die_roll_trigger,
    )

    die_trigger = assemble_die_roll_trigger(rmc_printing_fields)
    if die_trigger == "invalid":
        return None
    if die_trigger is not None:
        rmc_printing_fields["triggered_abilities"] = (
            tuple(printing.triggered_abilities) + (die_trigger,)
        )
    chooses_color = rmc_printing_fields.pop("_chooses_color", False)
    if chooses_color != rmc_printing_fields.get(
        "paints_all_objects_chosen_color_at_etb", False,
    ):
        return None
    if enters_with_named_counters:
        printing = replace(
            printing,
            enters_with_named_counters=tuple(enters_with_named_counters),
        )
    if enters_with_plus_one_counters_per is not None:
        printing = replace(
            printing,
            enters_with_plus_one_counters_per=enters_with_plus_one_counters_per,
        )
    if rmc_printing_fields:
        printing = replace(printing, **rmc_printing_fields)
    if wg_fields:
        if "graveyard_activated_abilities" in wg_fields:
            wg_fields["graveyard_activated_abilities"] = (
                tuple(printing.graveyard_activated_abilities)
                + tuple(wg_fields["graveyard_activated_abilities"])
            )
        printing = replace(printing, **wg_fields)
    if ub_hand_abilities:
        ub_fields["hand_activated_abilities"] = (
            tuple(printing.hand_activated_abilities) + tuple(ub_hand_abilities)
        )
    if ub_fields:
        printing = replace(printing, **ub_fields)
    from mtgz_ir.cards.parser.cube_creatures_ub import link_exiled_with_recall

    printing = link_exiled_with_recall(printing)
    if "Land" in card_types:
        # A land creature (Dryad Arbor): its land type and the intrinsic
        # mana ability of its basic land types (CR 305.6; docs/ir_defects.md
        # 440).
        from mtgz_ir.cards.parser.legacy_hogaak_c import as_land_creature

        return as_land_creature(printing, card)
    return printing
