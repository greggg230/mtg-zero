"""Mana-ability detection on parsed activated abilities, plus the
``Sacrifice another creature or artifact`` cost shape.

Two concerns covered here:

1. **Mana-ability flag.** When ``_parse_activated_ability_line``
   produces an ``ActivatedAbility`` whose effect is a mana ability
   (today: ``AddManaToPool`` with no targets — see
   ``_is_mana_ability_effect``), the resulting ability must carry
   ``is_mana_ability=True`` so the engine bypasses the stack on
   activation per CR 605. Non-mana effects (Draw a Card, Pump,
   Surveil, …) leave the flag at its default ``False``.

2. **Sacrifice-another-creature-or-artifact cost.** Umbral Collar
   Zealot's ``Sacrifice another creature or artifact: Surveil 1.``
   needs the new pure-sac-another cost shape recognized by
   ``_parse_activation_cost``. The corresponding
   ``ActivationCost.sacrifice_another_creature_or_artifact`` field
   already exists; this slice just teaches the cost grammar to emit
   it.

End-to-end parser claim of Umbral Collar Zealot is also asserted as
the headline real-card case.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
    DrawCards,
    Surveil,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import (
    _is_mana_ability_effect,
    _parse_activated_ability_line,
    _parse_activation_cost,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


# --- _is_mana_ability_effect predicate --------------------------------------

def test_is_mana_ability_effect_true_for_add_mana_to_pool():
    # Single-color, single-coin AddManaToPool — the canonical mana
    # ability shape for an EoE land or creature ``{T}: Add {<C>}.``
    assert _is_mana_ability_effect(
        AddManaToPool(produces=(("R", 1),))
    ) is True


def test_is_mana_ability_effect_true_for_multi_coin_add_mana():
    # Multi-coin still qualifies — "produces mana, no target" is the
    # rule, not "produces exactly one coin".
    assert _is_mana_ability_effect(
        AddManaToPool(produces=(("U", 2),))
    ) is True


def test_is_mana_ability_effect_true_for_add_mana_with_spend_restriction():
    # A spend-restricted coin (Steelswarm Operator's "spend only to
    # cast an artifact spell") is still a mana ability — the
    # restriction tags the coin, it doesn't change the ability type.
    assert _is_mana_ability_effect(
        AddManaToPool(
            produces=(("U", 1),), spend_restriction="artifact_spell"
        )
    ) is True


def test_is_mana_ability_effect_false_for_draw_cards():
    # DrawCards is the headline non-mana effect.
    assert _is_mana_ability_effect(DrawCards(amount=1)) is False


def test_is_mana_ability_effect_false_for_surveil():
    # Surveil is non-mana too — Umbral Collar Zealot's effect.
    assert _is_mana_ability_effect(Surveil(amount=1)) is False


# --- Parser sets is_mana_ability=True on Add-mana activations ---------------

def test_parser_marks_tap_add_mana_as_mana_ability():
    # ``{T}: Add {R}.`` → mana ability flagged True.
    ab = _parse_activated_ability_line("{T}: Add {R}.")
    assert ab is not None
    assert ab.cost == ActivationCost(tap_self=True)
    assert ab.effect == AddManaToPool(produces=(("R", 1),))
    assert ab.is_mana_ability is True
    assert ab.sorcery_speed is False


def test_parser_marks_each_color_tap_add_mana_as_mana_ability():
    # All five colors round-trip with the flag set.
    for color in ("W", "U", "B", "R", "G"):
        ab = _parse_activated_ability_line(f"{{T}}: Add {{{color}}}.")
        assert ab is not None
        assert ab.effect == AddManaToPool(produces=((color, 1),))
        assert ab.is_mana_ability is True


def test_parser_marks_pure_mana_cost_add_mana_as_mana_ability():
    # ``{1}: Add {U}.`` (hypothetical — no EoE card prints this exact
    # shape today, but the rule is the same: cost shape doesn't gate
    # the flag, the effect does).
    ab = _parse_activated_ability_line("{1}: Add {U}.")
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{1}"), mana_str="{1}",
    )
    assert ab.effect == AddManaToPool(produces=(("U", 1),))
    assert ab.is_mana_ability is True


# --- Parser does NOT mark non-mana activations as mana abilities ------------

def test_parser_does_not_mark_draw_card_activation_as_mana_ability():
    # ``{2}: Draw a card.`` (synthetic) — non-mana effect, flag
    # stays False.
    ab = _parse_activated_ability_line("{2}: Draw a card.")
    assert ab is not None
    assert ab.effect == DrawCards(amount=1)
    assert ab.is_mana_ability is False


def test_parser_does_not_mark_sacrifice_draw_activation_as_mana_ability():
    # Illvoi Galeblade's ``{2}, Sacrifice this creature: Draw a card.``
    # — sac+mana cost, non-mana effect, flag stays False.
    ab = _parse_activated_ability_line(
        "{2}, Sacrifice this creature: Draw a card."
    )
    assert ab is not None
    assert ab.is_mana_ability is False


def test_parser_does_not_mark_surveil_activation_as_mana_ability():
    # Umbral Collar Zealot's ``Sacrifice another creature or artifact:
    # Surveil 1.`` — non-mana effect, flag stays False.
    ab = _parse_activated_ability_line(
        "Sacrifice another creature or artifact: Surveil 1."
    )
    assert ab is not None
    assert ab.effect == Surveil(amount=1)
    assert ab.is_mana_ability is False


# --- Inline sorcery rider composes with the mana-ability flag ---------------
# (Defensive: no real card combines a mana ability with the sorcery rider —
# mana abilities are timing-flexible by definition. But the parser path
# composes orthogonally: rider sets sorcery_speed, effect sets
# is_mana_ability. They must coexist on the resulting ability.)

def test_mana_ability_flag_coexists_with_inline_sorcery_rider():
    ab = _parse_activated_ability_line(
        "{T}: Add {R}. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.effect == AddManaToPool(produces=(("R", 1),))
    assert ab.is_mana_ability is True
    assert ab.sorcery_speed is True


# --- _parse_activation_cost: pure sacrifice-another-creature-or-artifact ----

def test_pure_sacrifice_another_creature_or_artifact_cost_parses():
    cost = _parse_activation_cost(
        "Sacrifice another creature or artifact"
    )
    assert cost == ActivationCost(
        sacrifice_another_creature_or_artifact=True,
    )


def test_pure_sacrifice_another_cost_parses_with_surrounding_whitespace():
    """The dispatcher hands us already-trimmed pieces, but the helper
    is documented to accept either form — exercise the strip path."""
    cost = _parse_activation_cost(
        "  Sacrifice another creature or artifact  "
    )
    assert cost == ActivationCost(
        sacrifice_another_creature_or_artifact=True,
    )


def test_sacrifice_another_with_other_filter():
    """The multi-piece slice added classifier atoms for
    ``Sacrifice another artifact`` and ``Sacrifice another artifact or
    land`` (each maps to its own ``ActivationCost`` boolean field).
    ``Sacrifice another creature`` and ``Sacrifice another permanent``
    have no dedicated field: ``Sacrifice another creature`` is a
    composable ``extra_costs`` atom (cube creatures bucket);
    ``Sacrifice another permanent`` stays unrecognized."""
    assert _parse_activation_cost("Sacrifice another creature").extra_costs
    assert _parse_activation_cost("Sacrifice another permanent") is None
    assert _parse_activation_cost("Sacrifice another artifact") == (
        ActivationCost(sacrifice_another_artifact=True)
    )
    assert _parse_activation_cost("Sacrifice another artifact or land") == (
        ActivationCost(sacrifice_another_artifact_or_land=True)
    )


def test_sacrifice_another_paired_in_two_piece_cost():
    """The bag-merge classifier added in the multi-piece-costs slice
    treats ``Sacrifice another creature or artifact`` as a sacrifice-
    flavor atom that composes with tap (and mana, etc.) like any
    other sacrifice piece. EoE has no card with this two-piece shape
    today, but the grammar accepts it for symmetry with the named-
    sacrifice shapes."""
    expected = ActivationCost(
        tap_self=True,
        sacrifice_another_creature_or_artifact=True,
    )
    assert _parse_activation_cost(
        "{T}, Sacrifice another creature or artifact"
    ) == expected
    assert _parse_activation_cost(
        "Sacrifice another creature or artifact, {T}"
    ) == expected


# --- Full activated-ability lines for sacrifice-another --------------------

def test_pure_sacrifice_another_line_with_surveil_effect():
    """Umbral Collar Zealot's exact line: pure sac-another cost +
    surveil-1 effect, no sorcery rider."""
    ab = _parse_activated_ability_line(
        "Sacrifice another creature or artifact: Surveil 1."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        sacrifice_another_creature_or_artifact=True,
    )
    assert ab.effect == Surveil(amount=1)
    assert ab.sorcery_speed is False
    assert ab.is_mana_ability is False


def test_sacrifice_another_line_combines_with_inline_sorcery_rider():
    """Sorcery rider must compose with the new cost shape the same way
    it composes with the existing cost shapes — strip the rider, parse
    the remainder, mark sorcery_speed True."""
    ab = _parse_activated_ability_line(
        "Sacrifice another creature or artifact: Surveil 1. "
        "Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        sacrifice_another_creature_or_artifact=True,
    )
    assert ab.effect == Surveil(amount=1)
    assert ab.sorcery_speed is True


def test_malformed_sacrifice_another_line_does_not_match():
    """A sac-another cost paired with an effect the unified grammar
    doesn't support still bails out — no synthetic ability."""
    ab = _parse_activated_ability_line(
        "Sacrifice another creature or artifact: Bend the rules of "
        "physics."
    )
    assert ab is None


# --- Card-level: Umbral Collar Zealot is now claimed end-to-end -------------

