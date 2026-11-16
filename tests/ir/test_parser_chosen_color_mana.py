"""Choose-one-of-N-colors single-coin mana ability grammar.

The unified effect grammar learned two new shapes this batch:

  * ``add {<c1>} or {<c2>}`` — two-color choice (Haunted Screen's
    ``{T}: Add {W} or {B}.``).
  * ``add {<c1>}, {<c2>}, or {<c3>}`` — three-color choice (Haunted
    Screen's ``{T}, Pay 1 life: Add {G}, {U}, or {R}.``).

Both produce ``AddNManaOfChosenColor(amount=1, options=(<colors>))`` —
the engine class accepts an arbitrary ``options`` tuple, so adding a
second printing with the same shape parses without further grammar
work. The captured colors land in oracle order on the engine class's
``options`` field, faithful to the printed list.

The ``is_mana_ability`` flag fires for both: ``_is_mana_ability_
effect`` in ``activated.py`` recognizes ``AddNManaOfChosenColor``
alongside ``AddManaToPool`` / ``AddChargeCounterManaOfChosenColor``,
so CR 605 stack-bypass applies on activation just like the other
mana primitives.
"""

from __future__ import annotations

from mtgz_ir.cards.base import ActivatedAbility, ActivationCost
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.generic import AddNManaOfChosenColor


# --- Effect-grammar level tests ---------------------------------------------


def test_add_two_color_choice_parses():
    """``add {W} or {B}`` parses to
    ``AddNManaOfChosenColor(amount=1, options=("W", "B"))``."""
    eff = _parse_effect("add {W} or {B}")
    assert eff == AddNManaOfChosenColor(amount=1, options=("W", "B"))


def test_add_two_color_choice_preserves_oracle_order():
    """Captured color order is the printed order — ``add {U} or {R}``
    yields ``options=("U", "R")``, NOT alphabetized."""
    eff = _parse_effect("add {U} or {R}")
    assert eff == AddNManaOfChosenColor(amount=1, options=("U", "R"))


def test_add_three_color_choice_parses():
    """``add {G}, {U}, or {R}`` parses to
    ``AddNManaOfChosenColor(amount=1, options=("G", "U", "R"))`` —
    Haunted Screen's second activation body."""
    eff = _parse_effect("add {G}, {U}, or {R}")
    assert eff == AddNManaOfChosenColor(
        amount=1, options=("G", "U", "R"),
    )


def test_add_three_color_choice_preserves_oracle_order():
    eff = _parse_effect("add {W}, {B}, or {G}")
    assert eff == AddNManaOfChosenColor(
        amount=1, options=("W", "B", "G"),
    )


def test_add_two_color_choice_does_not_steal_single_color():
    """The bare ``add {<color>}`` shape still wins for single-color
    mana abilities — the new two-color regex's ``or {…}`` tail
    forbids it from stealing the bare form."""
    from mtgz_ir.cards.base import AddManaToPool

    eff = _parse_effect("add {W}")
    assert eff == AddManaToPool(produces=(("W", 1),))


def test_add_three_color_choice_does_not_overlap_two_color():
    """The two-color and three-color regexes are disjoint — a two-
    color phrase doesn't accidentally claim through the three-color
    arm or vice versa."""
    eff_two = _parse_effect("add {W} or {B}")
    eff_three = _parse_effect("add {W}, {U}, or {B}")
    assert isinstance(eff_two, AddNManaOfChosenColor)
    assert isinstance(eff_three, AddNManaOfChosenColor)
    assert len(eff_two.options) == 2
    assert len(eff_three.options) == 3


# --- Activated-ability level tests ------------------------------------------


def test_two_color_choice_activation_is_mana_ability():
    """``{T}: Add {W} or {B}.`` parses to a complete activated ability
    whose effect is ``AddNManaOfChosenColor(amount=1, options=("W",
    "B"))`` and which is auto-flagged ``is_mana_ability=True`` (CR
    605 stack-bypass on activation)."""
    ab = _parse_activated_ability_line("{T}: Add {W} or {B}.")
    assert ab is not None
    assert ab.cost == ActivationCost(tap_self=True)
    assert ab.effect == AddNManaOfChosenColor(
        amount=1, options=("W", "B"),
    )
    assert ab.is_mana_ability is True
    assert ab.sorcery_speed is False


def test_three_color_choice_activation_pay_life_cost():
    """``{T}, Pay 1 life: Add {G}, {U}, or {R}.`` parses end-to-end:
    the cost is ``ActivationCost(tap_self=True, pay_life=1)`` (the
    existing two-piece cost grammar) and the body is the three-color
    choice primitive."""
    ab = _parse_activated_ability_line(
        "{T}, Pay 1 life: Add {G}, {U}, or {R}."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(tap_self=True, pay_life=1)
    assert ab.effect == AddNManaOfChosenColor(
        amount=1, options=("G", "U", "R"),
    )
    assert ab.is_mana_ability is True
