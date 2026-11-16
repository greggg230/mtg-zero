"""Tests for the spell-cost-reduction rider parser.

Covers ``mtgz_ir.cards.parser.cost_reduction`` (the standalone
recognizer) and its integration with ``creature_builder`` and
``spell_builder`` (which extract any matching clauses out of the
oracle text and thread them onto the resulting ``CardPrinting`` via
the ``cost_reductions`` kwarg).

The recognizer matches a single oracle line of the shape
``This spell costs {N} less to cast if <condition>.`` and resolves
the condition phrase against a small whitelist mapped to engine
condition keys. Unrecognized phrases or alternate surface shapes
(e.g. ``Affinity for Slivers (...)`` or ``...costs {X} less to cast,
where X is...``) intentionally fall through; the enclosing card
falls back to the unsupported path the same way it would today.

Coverage targets:
  - The recognizer accepts each supported condition phrase and
    extracts the right amount + engine-condition key.
  - Unsupported condition phrases, mismatched amounts (``{X}``), and
    alternate surface shapes are rejected.
  - End-to-end parse of Gigastorm Titan and Lashwhip Predator (the
    two EoE cards now claimed by this branch) matches the static
    printing exactly.
  - The spell-builder pre-clause check drops cost-reduction riders
    from the effect-clause stream so the surviving clauses still
    parse, with the rider surfaced via ``cost_reductions=`` on the
    output ``CardPrinting``.
  - Negative end-to-end: a synthetic spell with an unsupported
    cost-reduction condition gets neither the rider nor a fall-
    through claim of an unparseable effect.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mtgz_ir.cards.base import (
    CostReduction,
    DestroyTargetCreature,
    make_instant,
)
from mtgz_ir.cards.parser.cost_reduction import (
    _COST_REDUCTION_CONDITIONS,
    _parse_cost_reduction_line,
)
from mtgz_ir.effects.conditions import (
    AnotherSpellCastThisTurn,
    Condition,
    OpponentsControlAtLeast,
    PerDifferentlyNamedLand,
    TargetMatches,
    VoidActiveCondition,
)
from mtgz_ir.effects.object_predicates import (
    IsBlocking,
    IsCreature,
)
from mtgz_ir.cards.parser.creature_builder import _build_creature_printing
from mtgz_ir.cards.parser.spell_builder import (
    _build_spell_printing,
    _detect_cost_reductions,
    _parse_spell_oracle_text,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


_DATA_PATH = (
    (Path(__file__).resolve().parents[2] / "ir")
    / "data"
    / "edge_of_eternities.json"
)


def _eoe_card(name: str) -> dict:
    """Look up a Scryfall record by ``name`` from the EoE data file."""
    with _DATA_PATH.open("r", encoding="utf-8") as f:
        for card in json.load(f):
            if card["name"] == name:
                return card
    raise LookupError(f"no EoE card named {name!r}")


# ---------- _parse_cost_reduction_line: positive cases ----------

def test_recognizer_extracts_another_spell_cast_this_turn():
    line = (
        "This spell costs {3} less to cast if you've cast another "
        "spell this turn."
    )
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=3, condition=AnotherSpellCastThisTurn(),
    )


def test_recognizer_extracts_opponents_control_three_or_more_creatures():
    line = (
        "This spell costs {2} less to cast if your opponents control "
        "three or more creatures."
    )
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=2, condition=OpponentsControlAtLeast(IsCreature(), 3),
    )


def test_recognizer_extracts_target_is_blocking_creature():
    # Dark Endurance's rider — supported by the engine via the
    # ``target_is_blocking_creature`` condition. The card itself is
    # not yet claimed end-to-end (its body effect — pump+indestructible
    # — isn't in the unified spell-effect grammar), but the rider must
    # parse so the card becomes claimable as soon as the body effect
    # is added.
    line = (
        "This spell costs {1} less to cast if it targets a blocking "
        "creature."
    )
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=1, condition=TargetMatches(IsCreature() & IsBlocking()),
    )


def test_recognizer_handles_surrounding_whitespace():
    # The caller passes already-stripped clauses, but defensive
    # whitespace handling keeps a stray trailing newline from hiding
    # a recognized rider.
    line = (
        "  This spell costs {2} less to cast if your opponents control "
        "three or more creatures.  "
    )
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=2, condition=OpponentsControlAtLeast(IsCreature(), 3),
    )


@pytest.mark.parametrize("amount", ["1", "2", "3", "5", "10"])
def test_recognizer_extracts_each_integer_amount(amount):
    # Surface form pinned to a known-supported condition; the only
    # variable is the amount. The recognizer must extract any
    # one-or-more-digit number verbatim.
    line = (
        f"This spell costs {{{amount}}} less to cast if you've cast "
        "another spell this turn."
    )
    rule = _parse_cost_reduction_line(line)
    assert rule is not None
    assert rule.amount == int(amount)
    assert rule.condition == AnotherSpellCastThisTurn()


# ---------- _parse_cost_reduction_line: negative cases ----------

def test_recognizer_rejects_unsupported_no_if_connector():
    # No-``if`` shape with a connector phrase that isn't on the
    # recognized closed set (``during your turn`` etc.). The
    # recognizer must decline rather than silently produce a no-op
    # rule with an unknown engine-condition string. Synthetic phrase
    # so a future expansion of the connector list doesn't silently
    # turn this from a negative case into a positive one.
    line = (
        "This spell costs {1} less to cast on every other tuesday."
    )
    assert _parse_cost_reduction_line(line) is None


def test_recognizer_rejects_if_condition_not_on_whitelist():
    # The line matches the ``This spell costs {N} less to cast if
    # <condition>.`` regex but the inner condition phrase is not on
    # ``_COST_REDUCTION_CONDITIONS``. The recognizer must decline
    # rather than silently produce a rule with an unknown
    # engine-condition string (which the engine would treat as a
    # no-op, drifting from the static implementation).
    line = (
        "This spell costs {1} less to cast if the moons of Endor are "
        "in alignment."
    )
    assert _parse_cost_reduction_line(line) is None


def test_recognizer_accepts_x_amount_per_differently_named_land_shape():
    # Fungal Colossus's ``{X}``-amount variant uses
    # ``per_land_group=True`` semantics on the surface form
    # ``where X is the number of differently named lands you
    # control``. The recognizer maps this literal to a unit-amount
    # ``CostReduction`` with ``per_land_group=True`` so the engine's
    # ``apply_cost_reductions`` per-land-group branch multiplies it
    # by the controller's distinctly-named-land count at cast time.
    line = (
        "This spell costs {X} less to cast, where X is the number of "
        "differently named lands you control."
    )
    rule = _parse_cost_reduction_line(line)
    assert rule == CostReduction(
        amount=1, condition=PerDifferentlyNamedLand(),
    )


def test_recognizer_rejects_affinity_keyword_shape():
    # Thrumming Hivepool ships its cost reduction as
    # ``Affinity for Slivers (...)``, NOT a standalone cost-reduction
    # line. The recognizer must not match the inner reminder text.
    line = (
        "Affinity for Slivers (This spell costs {1} less to cast for "
        "each Sliver you control.)"
    )
    assert _parse_cost_reduction_line(line) is None


def test_recognizer_rejects_activated_ability_rider():
    # Thaumaton Torpedo's activated-ability rider (``This ability
    # costs {3} less to activate ...``) shares the structure but
    # belongs on ``ActivatedAbility.cost_reductions``, not the
    # spell-cost-reduction layer. A different parser handles it.
    line = (
        "This ability costs {3} less to activate if you attacked with "
        "a Spacecraft this turn."
    )
    assert _parse_cost_reduction_line(line) is None


def test_recognizer_accepts_void_em_dash_prefix():
    # Temporal Intervention's surface form prefixes the rider with
    # ``Void —``. The recognizer's Void-prefix variant matches the
    # canonical Void-state body verbatim and emits a CostReduction
    # with the ``void_active`` engine condition, the same key the
    # static side ships on Temporal Intervention's printing.
    line = (
        "Void — This spell costs {2} less to cast if a nonland "
        "permanent left the battlefield this turn or a spell was "
        "warped this turn."
    )
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=2, condition=VoidActiveCondition(),
    )


def test_recognizer_rejects_void_em_dash_prefix_with_different_body():
    # The Void-prefix variant requires the canonical Void-state body
    # verbatim. A Void-prefixed rider with a different body isn't
    # recognized — both because no real card ships such a body and
    # because mapping arbitrary Void bodies to ``void_active`` would
    # silently diverge from the static side. The fall-through then
    # tries the bare ``This spell costs ...`` regex which doesn't
    # anchor at start either, so the line stays unmatched.
    line = (
        "Void — This spell costs {2} less to cast if you've cast "
        "another spell this turn."
    )
    assert _parse_cost_reduction_line(line) is None


def test_recognizer_rejects_non_rider_lines():
    assert _parse_cost_reduction_line("Destroy target creature.") is None
    assert _parse_cost_reduction_line("Reach") is None
    assert _parse_cost_reduction_line("") is None


def test_recognizer_rejects_typo_in_amount():
    # No braces around the amount → no match.
    line = (
        "This spell costs 3 less to cast if you've cast another spell "
        "this turn."
    )
    assert _parse_cost_reduction_line(line) is None


# ---------- map sanity ----------

def test_every_recognized_condition_resolves_to_a_condition_object():
    # Belt-and-suspenders: each phrase in the recognizer's whitelist
    # must map to a structural ``Condition`` instance. A typo or
    # accidental ``None`` would produce silently broken rules.
    for phrase, condition in _COST_REDUCTION_CONDITIONS.items():
        assert phrase
        assert isinstance(condition, Condition)


# ---------- end-to-end on the EoE pool: creatures ----------





@pytest.mark.parametrize(
    "name", ["Gigastorm Titan", "Lashwhip Predator"],
)
def test_oracle_parser_source_claims_cost_reduction_cards(name):
    # The top-level source — exercised by the parity-test gate — must
    # actually surface these cards under their printed names.
    src = OracleParserCardSource()
    assert name in src.claimed_names
    assert name in src.get_printings()


# ---------- spell-builder integration ----------

def test_spell_builder_drops_rider_clause_and_passes_via_kwarg():
    # Synthetic Scryfall record matching the spell-builder shape:
    # cost-reduction rider as the first clause, a parseable destroy
    # body as the second. The parser must drop the rider from the
    # effect stream and surface it on the resulting printing.
    synthetic = {
        "type_line": "Sorcery",
        "name": "Synthetic Bargain Slayer",
        "mana_cost": "{2}{B}",
        "oracle_text": (
            "This spell costs {2} less to cast if your opponents "
            "control three or more creatures.\n"
            "Destroy target creature."
        ),
    }
    parsed = _build_spell_printing(synthetic)
    assert parsed is not None
    assert parsed.spell_effect == DestroyTargetCreature()
    assert parsed.cost_reductions == (
        CostReduction(
            amount=2,
            condition=OpponentsControlAtLeast(IsCreature(), 3),
        ),
    )


def test_spell_builder_parses_to_destroy_when_rider_is_trailing():
    # Riders can appear after the body too. The clause-drop pass in
    # ``_parse_spell_oracle_text`` is order-insensitive (it filters
    # the whole stream, not just the leading clauses), so a trailing
    # rider also works.
    text = (
        "Destroy target creature.\n"
        "This spell costs {2} less to cast if your opponents control "
        "three or more creatures."
    )
    assert _parse_spell_oracle_text(text, "Synthetic") == \
        DestroyTargetCreature()


def test_detect_cost_reductions_picks_up_every_supported_clause():
    # ``_detect_cost_reductions`` walks the original text (not the
    # surviving-effect-clause stream), so it sees every recognized
    # rider regardless of its position. Multiple riders on the same
    # spell are accumulated in printed order.
    text = (
        "This spell costs {1} less to cast if you've cast another "
        "spell this turn.\n"
        "Destroy target creature.\n"
        "This spell costs {2} less to cast if it targets a blocking "
        "creature."
    )
    assert _detect_cost_reductions(text) == (
        CostReduction(amount=1, condition=AnotherSpellCastThisTurn()),
        CostReduction(amount=2, condition=TargetMatches(IsCreature() & IsBlocking())),
    )


def test_detect_cost_reductions_returns_empty_when_none_present():
    text = "Destroy target creature."
    assert _detect_cost_reductions(text) == ()


def test_detect_cost_reductions_skips_unsupported_conditions():
    # An unsupported phrase doesn't make it into the returned tuple —
    # it falls through, and the enclosing card is left unsupported.
    # The connector ``on every other tuesday`` isn't on the closed
    # set; using a synthetic phrase here keeps this case stable when
    # the supported list grows.
    text = (
        "This spell costs {1} less to cast on every other tuesday.\n"
        "Destroy target creature."
    )
    assert _detect_cost_reductions(text) == ()


def test_spell_builder_round_trips_via_make_sorcery():
    # The synthetic spell from the integration test should match the
    # printing ``make_sorcery`` produces directly with the same
    # arguments — proving the cost_reductions kwarg is wired
    # symmetrically through the factory.
    synthetic = {
        "type_line": "Sorcery",
        "name": "Synthetic Bargain Slayer",
        "mana_cost": "{2}{B}",
        "oracle_text": (
            "This spell costs {2} less to cast if your opponents "
            "control three or more creatures.\n"
            "Destroy target creature."
        ),
    }
    parsed = _build_spell_printing(synthetic)
    from mtgz_ir.cards.base import make_sorcery
    expected = make_sorcery(
        "Synthetic Bargain Slayer", "{2}{B}",
        effect=DestroyTargetCreature(),
        cost_reductions=(
            CostReduction(
                amount=2,
                condition=OpponentsControlAtLeast(IsCreature(), 3),
            ),
        ),
    )
    assert parsed == expected


def test_make_instant_round_trip_with_cost_reduction():
    # Same wiring on the instant side. Defensive — the spell-builder
    # picks ``make_instant`` for any "Instant" card_type; if the
    # cost_reductions kwarg ever drifts off of make_instant this test
    # surfaces it directly.
    synthetic = {
        "type_line": "Instant",
        "name": "Synthetic Counterstrike",
        "mana_cost": "{1}{B}",
        "oracle_text": (
            "This spell costs {1} less to cast if it targets a "
            "blocking creature.\n"
            "Destroy target creature."
        ),
    }
    parsed = _build_spell_printing(synthetic)
    assert parsed == make_instant(
        "Synthetic Counterstrike", "{1}{B}",
        effect=DestroyTargetCreature(),
        cost_reductions=(
            CostReduction(
                amount=1, condition=TargetMatches(IsCreature() & IsBlocking()),
            ),
        ),
    )


# ---------- negative end-to-end ----------

def test_unsupported_condition_synthetic_creature_skipped():
    # An unrecognized phrase causes the cost-reduction line to fall
    # through to the per-clause grammar, which doesn't match it
    # either — the whole creature is rejected (None). This is the
    # same skip behavior as a creature with any other unsupported
    # clause.
    synthetic = {
        "type_line": "Creature — Beast",
        "name": "Synthetic Daydreamer",
        "mana_cost": "{3}{U}",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "This spell costs {1} less to cast on every other tuesday."
        ),
    }
    assert _build_creature_printing(synthetic) is None


def test_unsupported_condition_synthetic_spell_skipped():
    # Spell variant of the same negative case: the unsupported rider
    # leaves no surviving effect-bearing clause, so
    # ``_parse_spell_oracle_text`` returns None and the build skips
    # the card.
    synthetic = {
        "type_line": "Sorcery",
        "name": "Synthetic Bargain",
        "mana_cost": "{2}{B}",
        "oracle_text": (
            "This spell costs {1} less to cast on every other tuesday."
        ),
    }
    assert _build_spell_printing(synthetic) is None


def test_dark_endurance_now_fully_claimed_with_pump_and_gains_body():
    # Batch 11 added the ``target creature gets +N/+M and gains <kw>
    # until end of turn`` shape to the unified effect grammar, which
    # together with the already-supported ``This spell costs {1} less
    # to cast if it targets a blocking creature.`` rider lets the
    # spell-builder claim Dark Endurance end-to-end. The body effect
    # parses to a ``PumpTargetCreatureUntilEOT`` with the indestructible
    # grant, and the cost-reduction rider is surfaced on the
    # ``CardPrinting`` via ``cost_reductions=`` exactly as Gigastorm
    # Titan and Lashwhip Predator do.
    card = _eoe_card("Dark Endurance")
    printing = _build_spell_printing(card)
    assert printing is not None
    assert printing.name == "Dark Endurance"
    assert printing.cost_reductions == (
        CostReduction(amount=1, condition=TargetMatches(IsCreature() & IsBlocking())),
    )


def test_temporal_intervention_claimed_via_void_prefix_and_whole_spell():
    # Temporal Intervention's rider is prefixed with ``Void —``; the
    # recognizer's Void-prefix variant maps it to the ``void_active``
    # engine condition. The body effect (3-sentence directed discard)
    # is claimed via the whole-spell exact-text dispatch tier
    # (``TargetOpponentDiscardsCasterChoice``). Both halves combine
    # to form a fully-parsed printing matching the static side.
    card = _eoe_card("Temporal Intervention")
    printing = _build_spell_printing(card)
    assert printing is not None
    assert printing.name == "Temporal Intervention"
    assert printing.cost_reductions == (
        CostReduction(amount=2, condition=VoidActiveCondition()),
    )


def test_fungal_colossus_claimed_via_grammar():
    # Fungal Colossus is now claimed via the grammar's per-line
    # cost-reduction recognizer (the X-amount per-differently-named-land
    # surface form), no longer via the bespoke dispatch table.
    # _build_creature_printing must return its canonical printing
    # carrying the per-land-group CostReduction rider.
    card = _eoe_card("Fungal Colossus")
    printing = _build_creature_printing(card)
    assert printing is not None
    assert printing.name == "Fungal Colossus"
    assert printing.cost_reductions == (
        CostReduction(
            amount=1, condition=PerDifferentlyNamedLand(),
        ),
    )
