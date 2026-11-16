"""Pure-Artifact ``CardPrinting`` builder.

A "pure" Artifact is a card whose type line is exactly ``"Artifact"``
(or ``"Legendary Artifact"`` / an ``"Artifact - <subtype>"`` form like
Food) **and** is not also a Creature, Equipment, or Spacecraft. Those
sister card types each have their own dedicated builder
(``creature_builder``, ``equipment_builder``, ``spacecraft_builder``)
with builder-specific grammar, so this builder claims only the
remainder.

The grammar this builder claims today is the simplest end-to-end shape
we can compose from the unified ``_parse_clause`` orchestrator already
used by the creature and spacecraft builders:

- zero or more keyword lines (e.g. ``"Indestructible"``);
- zero or more triggered ability lines (typed self-triggers like
  ``"When this artifact enters, ..."`` whose effect resolves through
  the unified effect grammar);
- zero or more activated ability lines (printed-cost activations like
  ``"{3}, Sacrifice this artifact: <effect>."``);
- zero or more static ability lines that the unified static parser
  already recognizes.

Anything outside that — composite triggers (``"...enters or
leaves..."``), state-based zone-change triggers
(``"is put into a graveyard"``), affinity-style cost reductions,
quoted granted abilities, or any line the unified parser doesn't
recognize — falls through and the artifact is silently skipped,
mirroring the creature/spell/land/equipment/spacecraft builders.

Today the EoE pure-artifacts the unified clause grammar claims are:

* **Melded Moxite** — typed-self ETB optional discard-then-draw plus a
  ``{3}, Sacrifice this artifact`` token-creating activation.
* **Thaumaton Torpedo** — single ``{6}, {T}, Sacrifice this artifact``
  activation with a ``destroy target nonland permanent`` payload and
  the inline ``This ability costs {3} less to activate if you attacked
  with a Spacecraft this turn.`` cost-reduction rider.
* **Cryogen Relic** — compound ``"enters or leaves the battlefield"``
  trigger drawing a card on each arm, plus a ``{1}{U}, Sacrifice this
  artifact`` activation that puts a stun counter on up to one tapped
  creature.
* **Dubious Delicacy** — Flash, an ``up to one`` optional ETB debuff
  ``-3/-3 UEOT``, and two ``{2}, {T}, Sacrifice this artifact``
  activations (gain 3 life / target opponent loses 3 life).
* **Nutrient Block** — Indestructible, ``{2}, {T}, Sacrifice this
  artifact: You gain 3 life.``, and the graveyard-from-battlefield
  zone-change trigger drawing a card (modeled via LEAVES_BATTLEFIELD
  to match the static-registry simplification).

In addition, several single-card oracle-line shapes that were once
matched as literal strings against a bespoke-class registry now
decompose into generic primitives via the unified ``_parse_clause``
grammar (the multi-sentence literal-effect recognizer in
``effects.py`` keeps the anaphoric body intact while still mapping
to a ``Sequence`` of generic primitives):

* **Requiem Monolith** — ``{T}: Until end of turn, target creature
  gains "Whenever this creature is dealt damage, you draw that many
  cards and lose that much life." That creature's controller may
  have this artifact deal 1 damage to it. Activate only as a sorcery.``
  decomposes into ``Sequence(GrantTriggeredAbilityUEOT(ability=
  TriggeredAbility(event=Event.DEALT_DAMAGE, predicate=IsSelf("obj"),
  effect=DrawCardsAndLoseLifeEqualToResolvingDamage())),
  MayHaveSourceDealDamageToTarget(amount=1))`` via the multi-sentence
  literal recognizer in
  ``effects.py``. The trailing ``Activate only as a sorcery.`` rider
  flips ``sorcery_speed=True`` via the existing inline-rider strip in
  ``activated.py``.
* **Haunted Screen** — three activations. The first two are mana
  abilities claimed by the unified clause grammar via the
  ``add {<c1>} or {<c2>}`` / ``add {<c1>}, {<c2>}, or {<c3>}``
  recognizers mapping to ``AddNManaOfChosenColor``. The third
  ``{7}: Put seven +1/+1 counters on this artifact. It becomes a 0/0
  Spirit creature in addition to its other types. Activate only once.``
  decomposes into ``Sequence(PutPlusOneCounterOnSelf(amount=7),
  BecomeType(types_to_add=("creature",), subtypes_to_add=("Spirit",),
  base_pt=(0, 0)))`` via a multi-sentence literal recognizer in
  ``effects.py``. The trailing ``Activate only once.`` rider flips
  ``activate_only_once=True`` via the new inline-rider strip in
  ``activated.py``; the per-source ``has_activated_once_only_ability``
  flag is now latched at the activation gate (``_activate_ability``
  in ``actions.py``) and read by ``legal_actions`` to filter out
  re-activation.
* **All-Fates Scroll** — two-line activation, both claimed via the
  unified clause grammar: ``{T}: Add one mana of any color.`` parses
  through the existing ``add one mana of any color`` recognizer, and
  ``{7}, {T}, Sacrifice this artifact: Draw X cards, where X is the
  number of differently named lands you control.`` parses through
  the new ``draw X cards, where X is the number of differently named
  lands you control`` recognizer mapping to the generic
  ``DrawCardsByConditionMultiplier(condition=PerDifferentlyNamedLand
  (), description_fragment=…)``.
* **The Endstone** (Legendary) — both trigger lines now claim through
  the unified ``_parse_clause`` grammar. The composite ``Whenever you
  play a land or cast a spell, draw a card.`` parses via the new
  ``"you play a land or cast a spell"`` entry in
  ``_COMPOUND_PREDICATE_TRIGGER_PHRASES`` (one ``TriggeredAbility``
  per arm — IS_CAST and PLAYS_LAND, both gated on
  ``Controlled(YOU, subject="controller")``); ``At the beginning of
  your end step, your life total becomes half your starting life
  total, rounded up.`` parses via the existing ``"your end step"``
  ``_AT_BEGINNING_OF_PHRASES`` entry plus the new
  ``"your life total becomes half your starting life total, rounded
  up"`` literal-phrase recognizer in ``effects.py`` mapping to the
  generic ``SetLifeToHalfStartingRoundedUp`` engine class.
* **Pinnacle Starcage** — the ETB line ``When this artifact enters,
  exile all artifacts and creatures with mana value 2 or less until
  this artifact leaves the battlefield.`` claims through the unified
  clause grammar via the existing ``"this artifact enters"`` predicate
  trigger phrase plus the ``_EXILE_ALL_MATCHING_UNTIL_LEAVES``
  effect-phrase regex in ``effects.py`` (mapping to the generic
  ``ExileAllMatchingUntilSourceLeaves(match_artifact=True,
  match_creature=True, max_mv=2)`` engine class). The activation
  ``{6}{W}{W}: Put each card exiled ... Sacrifice this artifact.``
  decomposes into a ``Sequence`` of three new generic primitives via
  the multi-sentence literal-effect recognizer in ``effects.py``:
  ``MoveEachExiledWithSourceToOwnersGraveyard`` (the recall side of
  the new "exiled-with-this" infrastructure), ``CreateNTokensFromLast
  ExileRecall(token_name="Robot")`` (reads
  ``state.last_exile_recall_count`` set by the recall step), and
  ``SacrificeSelf`` (effect-body sacrifice; the printed
  ``Sacrifice this artifact`` is part of the body, not the cost).
* **Ghost Vacuum** — two activations. The first ability
  ``{T}: Exile target card from a graveyard.`` parses through the
  unified activated grammar via the ``ExileTargetCardFromGraveyard``
  primitive (which tags the chosen card with
  ``exiled_by_source = source.id`` so the recall side has something
  to consume). The second ``{6}, {T}, Sacrifice this artifact``
  recall body decomposes into the parameterized
  ``PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYour
  Control`` primitive (with ``grant_flying_counter=True`` and
  ``grant_subtype_and_pt=("Spirit", 1, 1)``) via the multi-sentence
  literal-effect recognizer in ``effects.py``.

Two additional cards are claimed via the expanded unified grammar
(historically claimed by the now-retired ``_BESPOKE_ARTIFACT_BUILDERS``
table — both factories were deleted on 2026-05-01):

* **Thrumming Hivepool** — ``Affinity for Slivers`` keyword line
  (parsed by the new ``_parse_affinity_line`` recognizer in
  ``cost_reduction.py``) → ``CostReduction(amount=1, condition=
  PerControlled(HasSubtype("Sliver")))``. ``Slivers you control have
  double strike and haste.`` anthem (new ``"slivers you control"``
  entry in ``_ANTHEM_SUBJECT_SCOPES`` in ``statics.py`` with
  multi-keyword " and "-split logic) → ``StaticAbility(scope=
  SLIVERS_YOU_CONTROL, granted_keywords={"double strike","haste"})``.
  ``At the beginning of your upkeep, create two 1/1 colorless Sliver
  creature tokens.`` trigger (new ``"your upkeep"`` entry in
  ``_AT_BEGINNING_OF_PHRASES`` in ``triggers.py``).
* **Found Footage** — ``You may look at face-down creatures your
  opponents control any time.`` claims via
  ``_parse_looks_at_opponents_face_down_line`` (a printing-keyword
  recognizer in ``printing_keywords.py`` alongside Warp / Ward /
  Devour / Impending), which sets
  ``looks_at_opponents_face_down_creatures=True`` on
  ``make_artifact``. ``{2}, Sacrifice this artifact: Surveil 2, then
  draw a card.`` (unified ``_parse_clause`` activated-ability grammar).
  The Clue subtype is extracted from the Scryfall type-line via the
  new ``artifact_subtype`` step in ``_build_artifact_printing``.

PowerLSV cube additions (``feature/cube-artifacts``): printing-level
statics (``This artifact enters tapped.`` / ``doesn't untap during your
untap step`` / ``enters with N <kind> counters`` / ``You may play lands
from your graveyard.`` / Mox Diamond's discard-a-land entry / Time
Vault's turn skip / ``Sunburst`` / Bolas's Citadel's play-from-top and
look-at-top), Vehicles (``Crew N`` plus printed P/T), ``∞ —``
harness-gated triggers, and self-name substitution so legendary
artifacts naming themselves ("The One Ring", "Esika's Chariot") parse
through the same phrase tables. Effect bodies specific to this bucket
live in ``parser/artifact_effects.py``.

Pure artifacts share the unified clause grammar with creatures - no
``Equipped creature ...`` or ``Station`` lines appear here - so this
builder is structurally simpler than the equipment and spacecraft
builders: it just dispatches each line through ``_parse_clause`` and
sorts the result into the appropriate ``make_artifact`` bucket. The
literal-line table is consulted FIRST — a recognized literal short-
circuits the unified ``_parse_clause`` dispatch for that one line so
the bespoke effect class is preserved end-to-end.
"""

from __future__ import annotations

from dataclasses import replace

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CardPrinting,
    CostReduction,
    DrawCards,
    StaticAbility,
    StaticScope,
    TriggeredAbility,
    make_artifact,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.cost_reduction import _parse_affinity_line
from mtgz_ir.cards.parser.printing_keywords import (
    _parse_deck_construction_statement_line,
    _parse_crew_line,
    _parse_discard_land_to_enter_line,
    _parse_doesnt_untap_line,
    _parse_enters_tapped_line,
    _parse_enters_with_named_counters_line,
    _parse_look_at_top_card_any_time_line,
    _parse_looks_at_opponents_face_down_line,
    _parse_play_from_top_paying_life_line,
    _parse_play_lands_from_graveyard_line,
    _parse_skip_turn_to_untap_line,
    _parse_sunburst_line,
)
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.combinators import Sequence
from mtgz_ir.effects.conditions import PerControlled
from mtgz_ir.effects.generic import CreateToken, Surveil
from mtgz_ir.effects.object_predicates import HasSubtype
from mtgz_ir.effects.object_predicates import IsHarnessed
from mtgz_ir.effects.predicates import Controlled, Event, Source, YOU


# ``∞ — `` prefix of an Infinity Stone's harness-gated abilities.
_INFINITY_PREFIX = "∞ — "


# Printing-level flag dispatch is handled via the standard
# printing-keyword recognizer path in
# ``mtgz_ir.cards.parser.printing_keywords`` — see
# ``_parse_looks_at_opponents_face_down_line`` (Found Footage's
# ``You may look at face-down creatures your opponents control any
# time.`` static). Earlier waves carried a dedicated
# ``_LITERAL_FLAG_LINES`` table here; that table was removed in
# audit-sothera-foundfootage in favor of the printing-keyword
# recognizer pattern (Warp, Ward, Devour, Impending), keeping all
# printing-keyword dispatch in one place.


# Literal oracle lines whose bespoke effect class lives in
# ``mtgz_ir.effects.bespoke`` and isn't (and shouldn't be) part of
# the unified ``_parse_effect`` grammar. Empty today — every
# previously-literal artifact activation has been decomposed into
# generic primitives and now claims through the unified
# ``_parse_clause`` grammar:
#
# * Requiem Monolith — ``{T}: Until end of turn, target creature gains
#   "Whenever this creature is dealt damage, you draw that many cards
#   and lose that much life." That creature's controller may have this
#   artifact deal 1 damage to it. Activate only as a sorcery.`` claims
#   through the inline-sorcery-rider strip in ``activated.py`` plus a
#   multi-sentence literal recognizer in ``effects.py`` mapping to
#   ``Sequence(GrantTriggeredAbilityUEOT(ability=TriggeredAbility(
#   event=Event.DEALT_DAMAGE, predicate=IsSelf("obj"), effect=
#   DrawCardsAndLoseLifeEqualToResolvingDamage())),
#   MayHaveSourceDealDamageToTarget(amount=1))``.
# * Haunted Screen — ``{7}: Put seven +1/+1 counters on this artifact.
#   It becomes a 0/0 Spirit creature in addition to its other types.
#   Activate only once.`` claims through the new inline-activate-only-
#   once-rider strip in ``activated.py`` (sets ``activate_only_once=
#   True`` on the ability) plus a multi-sentence literal recognizer in
#   ``effects.py`` mapping to ``Sequence(PutPlusOneCounterOnSelf(
#   amount=7), BecomeType(types_to_add=("creature",),
#   subtypes_to_add=("Spirit",), base_pt=(0, 0)))``.
#
# Pinnacle Starcage and Ghost Vacuum had previously been migrated out
# via the generic "exiled-with-this" recall infrastructure (Sequence
# of ``MoveEachExiledWithSourceToOwnersGraveyard`` /
# ``CreateNTokensFromLastExileRecall`` / ``SacrificeSelf`` for the
# former; ``PutEachCreatureCardExiledWithSourceOntoBattlefieldUnder
# YourControl`` for the latter).
def _build_artifact_printing(card: dict) -> CardPrinting | None:
    """Try to produce a ``CardPrinting`` for a pure-Artifact ``card`` (a
    Scryfall record). Returns ``None`` if the card is not a pure
    Artifact, or if any oracle line falls outside the unified
    ``_parse_clause`` grammar.

    "Pure" here means: ``Artifact`` is in ``card_types`` and none of
    ``Creature`` / ``Equipment`` (subtype) / ``Spacecraft`` (subtype)
    are - those have their own dedicated builders and we must not
    poach their printings.

    Legendary pure artifacts (e.g. The Endstone) are not skipped at
    the type-line level - ``make_artifact(legendary=True)`` carries the
    supertype faithfully. They still fall through if their body
    contains lines outside the unified grammar.
    """
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Artifact" not in card_types:
        return None
    # Creatures, Equipment, and Spacecraft each have their own builder
    # with builder-specific grammar (P/T, ``Equipped creature ...`` /
    # ``Equip {N}``, ``Station`` / tier lines). Defer to them by
    # rejecting any printing they would also accept.
    if "Creature" in card_types:
        return None
    # An artifact land is a land (playable as a land drop); the land
    # builders own it -- never claim it as a plain artifact.
    if "Land" in card_types:
        return None
    if "Equipment" in subtypes:
        return None
    if "Spacecraft" in subtypes:
        return None

    # Extract the non-Equipment, non-Spacecraft subtype for ``make_artifact``
    # (e.g. ``"Clue"`` from ``"Artifact — Clue"``). Creature, Equipment, and
    # Spacecraft subtypes are already gated out above; any remaining subtype
    # is a pure-artifact subtype like Clue or Food.
    artifact_subtype: str | None = " ".join(subtypes) if subtypes else None

    text = card.get("oracle_text", "")
    keywords: set[str] = set()
    triggers: list[TriggeredAbility] = []
    activated: list[ActivatedAbility] = []
    statics: list[StaticAbility] = []
    cost_reductions: list[CostReduction] = []
    looks_at_face_down: bool = False
    enters_tapped: bool = False
    doesnt_untap: bool = False
    play_lands_from_graveyard: bool = False
    discard_land_to_enter: bool = False
    enters_with_counters: list[tuple[str, int]] = []
    skip_turn_to_untap: bool = False
    sunburst: bool = False
    look_at_top: bool = False
    play_from_top_paying_life: bool = False
    # Vintage Roto P3 lines (modal "When ~ enters, choose one —" blocks,
    # library-top cast grants, ...) are lifted out first.
    from mtgz_ir.cards.parser.roto_mana import extract_roto_artifact_lines

    extracted = extract_roto_artifact_lines(text, card["name"])
    if extracted is None:
        return None
    text, roto_triggers, roto_fields = extracted
    triggers.extend(roto_triggers)

    for raw_line in text.split("\n"):
        line = _strip_reminder_text(raw_line)
        if not line:
            continue
        # Printing-level flag dispatch — lines that set a boolean flag on
        # ``make_artifact`` rather than producing an ability. Tried first
        # so these lines are consumed before the unified clause grammar.
        # Each printing-keyword recognizer lives in
        # ``printing_keywords.py`` alongside Warp/Ward/Devour/Impending;
        # the dispatch here mirrors the creature builder's
        # printing-keyword fan-out.
        if _parse_looks_at_opponents_face_down_line(line):
            looks_at_face_down = True
            continue
        # ``This artifact enters tapped.`` / ``This artifact doesn't
        # untap during your untap step.`` / ``You may play lands from
        # your graveyard.`` — printing-level statics (Time Vault,
        # Wizard's Rockets, Grim Monolith, Crucible of Worlds).
        if _parse_enters_tapped_line(line):
            enters_tapped = True
            continue
        if _parse_doesnt_untap_line(line):
            doesnt_untap = True
            continue
        if _parse_play_lands_from_graveyard_line(line):
            play_lands_from_graveyard = True
            continue
        if _parse_discard_land_to_enter_line(line):
            discard_land_to_enter = True
            continue
        counters = _parse_enters_with_named_counters_line(line)
        if counters is not None:
            enters_with_counters.append(counters)
            continue
        if _parse_skip_turn_to_untap_line(line):
            skip_turn_to_untap = True
            continue
        if _parse_sunburst_line(line):
            sunburst = True
            continue
        if _parse_look_at_top_card_any_time_line(line):
            look_at_top = True
            continue
        if _parse_play_from_top_paying_life_line(line):
            play_from_top_paying_life = True
            continue
        if _parse_deck_construction_statement_line(line, card["name"]):
            # "This card is banned." -- constrains deck building only.
            continue
        crew_n = _parse_crew_line(line)
        if crew_n is not None:
            if "Vehicle" not in subtypes:
                return None
            activated.append(_crew_ability(crew_n))
            continue
        # Affinity keyword line dispatch — ``Affinity for <subtype>``
        # (reminder text already stripped). Produces a
        # ``CostReduction(amount=1, condition=PerControlled(HasSubtype
        # ("<subtype>")))`` and is consumed here before ``_parse_clause``
        # (which doesn't know about cost-reduction lines). Tried after the
        # flag dispatch so a future card whose oracle somehow matched both
        # would take the flag path first (no such card exists today).
        affinity_reduction = _parse_affinity_line(line)
        if affinity_reduction is not None:
            cost_reductions.append(affinity_reduction)
            continue
        # Literal-line dispatch for bespoke single-card effect bodies.
        # Empty today — Requiem Monolith, Haunted Screen, Pinnacle
        # Starcage, and Ghost Vacuum have all been decomposed into
        # generic primitives that claim through the unified
        # ``_parse_clause`` grammar (the multi-sentence literal-effect
        # recognizers in ``effects.py`` keep the anaphoric bodies
        # intact while still mapping to ``Sequence`` of generic
        # primitives).
        # ``∞ — <triggered ability>`` (Infinity Stones): the ability
        # functions only while the permanent is harnessed (CR 702.186).
        # Parsed as a normal trigger with ``Source(IsHarnessed())`` AND-ed
        # onto its predicate.
        if line.startswith(_INFINITY_PREFIX):
            inf_clause = _parse_clause(
                line[len(_INFINITY_PREFIX):], name=card["name"],
            )
            if inf_clause is None or not inf_clause.triggers:
                return None
            triggers.extend(
                replace(t, predicate=t.predicate & Source(IsHarnessed()))
                for t in inf_clause.triggers
            )
            continue
        clause = _parse_clause(line, name=card["name"])
        if clause is None:
            # Roto P1 (stack & restrictions) lines -- consulted only for
            # lines the grammar above declined.
            from mtgz_ir.cards.parser.roto_stack import (
                merge_fields, parse_roto_stack_line,
            )

            roto = parse_roto_stack_line(line, card)
            if roto is not None:
                merge_fields(roto_fields, roto.fields)
                keywords.update(roto.keywords)
                triggers.extend(roto.triggers)
                activated.extend(roto.activated)
                statics.extend(roto.statics)
                continue
            # Vintage Roto P5 lines (printing-level statics, ...).
            from mtgz_ir.cards.parser.roto_board import (
                merge_board_fields,
                parse_roto_board_line,
            )

            board = parse_roto_board_line(line, card)
            if board is not None:
                keywords.update(board.keywords)
                triggers.extend(board.triggers)
                activated.extend(board.activated)
                statics.extend(board.statics)
                merge_board_fields(roto_fields, board.fields)
                continue
            # Anything the unified parser doesn't recognize - composite
            # triggers, zone-change triggers, custom effects - falls
            # through and we skip the whole card rather than half-claim
            # a printing with silently-dropped abilities.
            return None
        # ``_ParsedClause`` populates exactly one of
        # ``keywords``/``triggers``/``activated``/``static`` per line.
        # We sort each into its own bucket; ``make_artifact`` consumes
        # them as four parallel kwargs. Multi-trigger lines (compound
        # ``enters or dies``-style conditions) fan out into the
        # builder's ``triggers`` list — same shape as the per-arm
        # static-registry layout.
        if clause.keywords:
            keywords.update(clause.keywords)
            continue
        if clause.triggers:
            triggers.extend(clause.triggers)
            continue
        if clause.activated is not None:
            activated.append(clause.activated)
            continue
        if clause.statics:
            statics.extend(clause.statics)
            continue
        # Defensive: an empty ``_ParsedClause`` (no fields populated)
        # should be impossible per the per-clause parser contract, but
        # if one ever surfaces, treat it as unrecognized rather than
        # silently dropping the line.
        return None

    printing = make_artifact(
        name=card["name"],
        cost=card["mana_cost"],
        legendary=("Legendary" in supertypes),
        subtype=artifact_subtype,
        keywords=frozenset(keywords),
        triggered_abilities=tuple(triggers),
        activated_abilities=tuple(activated),
        static_abilities=tuple(statics),
        cost_reductions=tuple(cost_reductions),
        looks_at_opponents_face_down_creatures=looks_at_face_down,
        enters_tapped=enters_tapped,
        doesnt_untap_during_untap_step=doesnt_untap,
        grants_play_lands_from_graveyard=play_lands_from_graveyard,
    )
    if discard_land_to_enter:
        printing = replace(printing, enters_only_by_discarding_land=True)
    if enters_with_counters:
        printing = replace(
            printing, enters_with_named_counters=tuple(enters_with_counters),
        )
    if skip_turn_to_untap:
        printing = replace(printing, may_skip_turn_to_untap=True)
    if sunburst:
        printing = replace(printing, sunburst=True)
    if look_at_top:
        printing = replace(printing, grants_look_at_top_of_library=True)
    if play_from_top_paying_life:
        printing = replace(printing, grants_play_from_top_paying_life=True)
    if roto_fields:
        # P5's accumulated fields (board rules, named counters) extend
        # what the printing already carries; the rest are set.
        from mtgz_ir.cards.parser.roto_board import apply_board_fields

        printing = apply_board_fields(printing, roto_fields)
    if "Vehicle" in subtypes:
        # Vehicles carry printed P/T that applies while they're
        # creatures (crewed / animated).
        try:
            power = int(card.get("power", ""))
            toughness = int(card.get("toughness", ""))
        except ValueError:
            return None
        printing = replace(printing, power=power, toughness=toughness)
    return printing


def _crew_ability(n: int) -> ActivatedAbility:
    """``Crew N``: tap untapped creatures with total power N or more:
    this Vehicle becomes an artifact creature until end of turn."""
    from mtgz_ir.effects.combinators import ApplyEffect
    from mtgz_ir.effects.durations import EndOfTurn
    from mtgz_ir.effects.generic import BecomeType

    return ActivatedAbility(
        cost=ActivationCost(crew=n),
        effect=ApplyEffect(
            BecomeType(types_to_add=("creature",)), ends_at=EndOfTurn(),
        ),
    )
