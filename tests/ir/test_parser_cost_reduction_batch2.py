"""Batch-2 cost-reduction parser additions.

Extends ``mtgz_ir.cards.parser.cost_reduction`` to recognize the
no-``if`` connector shape ``This spell costs {N} less to cast
<connector>.``. The closed connector list seeds with ``"during your
turn"`` (Mental Modulation), mapped to the engine condition
``during_your_turn`` newly wired in ``actions._evaluate_cost_reduction``.

The recognizer continues to accept the original ``...less to cast if
<condition>.`` shape with no behavioral change — these tests guard the
new connector path AND check the cross-table integrity (no condition
phrase appears on both whitelists).

Coverage targets:
  - The recognizer extracts the new ``during_your_turn`` rule from
    the no-``if`` shape with the right amount.
  - End-to-end Mental Modulation parses through ``_build_spell_
    printing`` with the new cost-reduction rule on the resulting
    printing.
  - The engine path (``_evaluate_cost_reduction``) reduces a cast cost
    by the rule amount when the caster is the active player and
    leaves it unchanged otherwise.
  - ``_detect_cost_reductions`` surfaces the no-``if`` rule the same
    way it surfaces ``if``-shape rules.
  - Integrity: connector phrase ``"during your turn"`` must NOT have
    crept onto the ``if``-shape whitelist (which would silently
    accept a malformed ``...less to cast if during your turn.``
    line); the two whitelists are disjoint by construction.
"""

# mtg-zero (docs/ENGINE.md 169): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.creature_statics_wg, not carried into mtgz_ir):
# test_apply_cost_reductions_threads_during_your_turn

from __future__ import annotations

import json
from pathlib import Path

from mtgz_ir.actions import _evaluate_cost_reduction
from mtgz_ir.cards.base import (
    CostReduction,
    DrawCards,
    ManaCost,
    Sequence,
    TapTargetArtifactOrCreature,
    make_instant,
)
from mtgz_ir.cards.parser.cost_reduction import (
    _COST_REDUCTION_CONDITIONS,
    _COST_REDUCTION_CONDITIONS_NO_IF,
    _parse_cost_reduction_line,
)
from mtgz_ir.effects.conditions import Condition, DuringYourTurn
from mtgz_ir.cards.parser.spell_builder import (
    _build_spell_printing,
    _detect_cost_reductions,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


_DATA_PATH = (
    (Path(__file__).resolve().parents[2] / "ir")
    / "data"
    / "edge_of_eternities.json"
)


def _eoe_card(name: str) -> dict:
    with _DATA_PATH.open("r", encoding="utf-8") as f:
        for card in json.load(f):
            if card["name"] == name:
                return card
    raise LookupError(f"no EoE card named {name!r}")


# ---------- recognizer: no-``if`` connector shape ----------


def test_recognizer_extracts_during_your_turn():
    line = "This spell costs {1} less to cast during your turn."
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=1, condition=DuringYourTurn(),
    )


def test_recognizer_extracts_during_your_turn_higher_amount():
    # Defensive — the regex captures any ``\d+`` amount, not just 1.
    # Synthetic since the only EoE printing ships at {1}.
    line = "This spell costs {3} less to cast during your turn."
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=3, condition=DuringYourTurn(),
    )


def test_recognizer_handles_surrounding_whitespace_no_if_shape():
    line = "  This spell costs {1} less to cast during your turn.  "
    assert _parse_cost_reduction_line(line) == CostReduction(
        amount=1, condition=DuringYourTurn(),
    )


# ---------- _detect_cost_reductions: integration ----------


def test_detect_cost_reductions_surfaces_during_your_turn():
    text = (
        "This spell costs {1} less to cast during your turn.\n"
        "Tap target artifact or creature.\n"
        "Draw a card."
    )
    assert _detect_cost_reductions(text) == (
        CostReduction(amount=1, condition=DuringYourTurn()),
    )


# ---------- end-to-end: Mental Modulation claim + parity ----------


def test_mental_modulation_now_claimed_with_during_your_turn_rider():
    card = _eoe_card("Mental Modulation")
    parsed = _build_spell_printing(card)
    assert parsed is not None
    assert parsed.name == "Mental Modulation"
    assert parsed.cost_reductions == (
        CostReduction(amount=1, condition=DuringYourTurn()),
    )
    # Body effect parses to the same Sequence the static side ships.
    assert parsed.spell_effect == Sequence(effects=(
        TapTargetArtifactOrCreature(),
        DrawCards(amount=1),
    ))




# ---------- engine: _evaluate_cost_reduction during_your_turn branch ----------


def _two_player_state():
    """Spin up a minimal two-player ``GameState`` whose active player
    is player 0. Mirrors the lightweight fixture used by other
    cost-reduction engine tests (e.g. ``tests/test_actions.py``)."""
    from mtgz_ir.game import GameState, Player

    s = GameState()
    s.players.append(Player(id=0))
    s.players.append(Player(id=1))
    s.active_player_idx = 0
    return s


def test_during_your_turn_reduces_when_caster_is_active_player():
    s = _two_player_state()
    rule = CostReduction(amount=1, condition=DuringYourTurn())
    # Active player (id=0) is the caster: reduction applies.
    assert _evaluate_cost_reduction(rule, s, caster_id=0) == 1


def test_during_your_turn_no_reduction_when_caster_is_not_active():
    s = _two_player_state()
    rule = CostReduction(amount=1, condition=DuringYourTurn())
    # Caster is the non-active player (id=1): no reduction.
    assert _evaluate_cost_reduction(rule, s, caster_id=1) == 0




# ---------- whitelist integrity ----------


def test_during_your_turn_only_on_no_if_table():
    # The two whitelists are disjoint — ``"during your turn"`` lives
    # only on the no-``if`` table, NOT on the ``if``-shape table.
    # Otherwise the recognizer's two regexes could both claim a
    # synthetic ``...less to cast if during your turn.`` line and
    # the precedence between them would matter; today the disjoint
    # invariant means precedence is moot.
    assert "during your turn" in _COST_REDUCTION_CONDITIONS_NO_IF
    assert "during your turn" not in _COST_REDUCTION_CONDITIONS


def test_no_if_table_keys_map_to_condition_objects():
    # Defensive — every entry maps to a structural ``Condition``
    # instance (caught by the parity tests indirectly, but explicit
    # here so a typo localizes to this file).
    for phrase, condition in _COST_REDUCTION_CONDITIONS_NO_IF.items():
        assert isinstance(condition, Condition), (
            f"phrase {phrase!r} maps to non-Condition {condition!r}"
        )
