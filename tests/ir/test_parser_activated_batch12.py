"""Activation-cost atoms: ``Remove a +1/+1 counter from a creature you
control`` (batch-12 slice).

The bag-merge classifier in ``parser/activated.py`` learned a new cost
atom this batch — Sunstar Chaplain's
``{2}, Remove a +1/+1 counter from a creature you control: Tap target
artifact or creature.`` is the only EoE card that prints the literal
phrase, but the atom is wired through the full classifier so it
composes order-insensitively with mana / tap / sacrifice / pay-life.

Other unsupported cost atoms surveyed during this batch but explicitly
NOT added (no engine field on ``ActivationCost``):

  * ``Discard a card`` — no field. (No EoE card uses it as a cost.)
  * ``Exile an artifact card from your graveyard`` — no field.
    Single EoE example: Rust Harvester. The card's effect ("put a
    +1/+1 counter, then deal damage equal to its power") is also
    unsupported by the unified grammar today, so even a placeholder
    cost field wouldn't claim the card. Documented as out of scope.
  * ``Tap an untapped permanent you control`` (Gene Pollinator),
    ``Tap X untapped artifacts you control`` (Secluded Starforge),
    ``Exile this land`` (Plaza of Heroes) — no engine fields, all
    skipped per the "no engine support → document and skip" rule.

Sunstar Chaplain itself isn't yet end-to-end parser-claimed: its
triggered ability uses ``ConditionalOnTappedCreatures`` (no parser
support) and the activated effect ``Tap target artifact or creature``
isn't recognized by the unified ``_parse_effect``. So the static
registry retains the card; this batch only proves the cost atom is
wired correctly via direct ``_parse_activation_cost`` /
``_parse_activated_ability_line`` tests with effects the unified
grammar already knows.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    ActivationCost,
    DrawCards,
    GainLife,
    PutPlusOneCounterOnSelf,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import (
    _parse_activated_ability_line,
    _parse_activation_cost,
)


# --- Pure single-piece counter-removal cost ---------------------------------

def test_pure_remove_plus_one_counter_cost_parses():
    """The standalone ``Remove a +1/+1 counter from a creature you
    control`` atom isn't observed in EoE on its own (Sunstar Chaplain
    pairs it with a mana piece), but the bag merge accepts a single
    classified piece for symmetry with sacrifice-self / pay-life /
    etc."""
    cost = _parse_activation_cost(
        "Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        remove_plus_one_counter_from_your_creature=True
    )


def test_pure_remove_plus_one_counter_cost_strips_whitespace():
    """The dispatcher trims comma-split pieces, but
    ``_parse_activation_cost`` is documented to accept a leading /
    trailing-whitespace input as well."""
    cost = _parse_activation_cost(
        "  Remove a +1/+1 counter from a creature you control  "
    )
    assert cost == ActivationCost(
        remove_plus_one_counter_from_your_creature=True
    )


# --- Two-piece costs: mana + counter-removal --------------------------------

def test_mana_then_remove_plus_one_counter_cost_parses():
    """Sunstar Chaplain's exact cost shape, mana piece first."""
    cost = _parse_activation_cost(
        "{2}, Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        remove_plus_one_counter_from_your_creature=True,
    )


def test_remove_plus_one_counter_then_mana_cost_parses():
    """Order-insensitive: the same bag with the counter-removal piece
    listed first parses identically."""
    cost = _parse_activation_cost(
        "Remove a +1/+1 counter from a creature you control, {2}"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        remove_plus_one_counter_from_your_creature=True,
    )


def test_colored_mana_plus_remove_plus_one_counter_cost_parses():
    """Colored mana piece survives ``parse_mana_cost`` validation and
    composes with counter-removal."""
    cost = _parse_activation_cost(
        "{1}{W}, Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}{W}"),
        mana_str="{1}{W}",
        remove_plus_one_counter_from_your_creature=True,
    )


# --- Two-piece costs: tap + counter-removal ---------------------------------

def test_tap_then_remove_plus_one_counter_cost_parses():
    """``{T}, Remove a +1/+1 counter from a creature you control`` —
    no EoE card prints this 2-piece shape, but the bag merge accepts
    it for grammar symmetry."""
    cost = _parse_activation_cost(
        "{T}, Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        tap_self=True,
        remove_plus_one_counter_from_your_creature=True,
    )


def test_remove_plus_one_counter_then_tap_cost_parses():
    """Reverse order, same result."""
    cost = _parse_activation_cost(
        "Remove a +1/+1 counter from a creature you control, {T}"
    )
    assert cost == ActivationCost(
        tap_self=True,
        remove_plus_one_counter_from_your_creature=True,
    )


# --- Three-piece costs: mana + tap + counter-removal ------------------------

def test_three_piece_mana_tap_remove_plus_one_counter():
    """Three-piece bag: mana + tap + counter-removal. Synthetic shape
    (no EoE card prints it today) but the classifier accepts it."""
    cost = _parse_activation_cost(
        "{2}, {T}, Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        remove_plus_one_counter_from_your_creature=True,
    )


@pytest.mark.parametrize(
    "ordered",
    [
        "{2}, {T}, Remove a +1/+1 counter from a creature you control",
        "{2}, Remove a +1/+1 counter from a creature you control, {T}",
        "{T}, {2}, Remove a +1/+1 counter from a creature you control",
        "{T}, Remove a +1/+1 counter from a creature you control, {2}",
        "Remove a +1/+1 counter from a creature you control, {2}, {T}",
        "Remove a +1/+1 counter from a creature you control, {T}, {2}",
    ],
)
def test_three_piece_remove_counter_order_insensitive(ordered):
    """All six permutations of mana / tap / counter-removal parse to
    the same ``ActivationCost`` — the bag merge is order-insensitive
    (matches MTG cost-payment semantics: the order of cost atoms in
    oracle text doesn't change what you pay)."""
    expected = ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        remove_plus_one_counter_from_your_creature=True,
    )
    assert _parse_activation_cost(ordered) == expected


# --- Three-piece costs: counter-removal composes with sacrifice atoms -------

def test_three_piece_mana_remove_counter_sacrifice_self():
    """Counter-removal pairs with a sacrifice flavor cleanly — the two
    atoms live in different exclusion groups (counter-removal isn't
    "sacrifice-flavored")."""
    cost = _parse_activation_cost(
        "{1}, Remove a +1/+1 counter from a creature you control, "
        "Sacrifice this creature"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{1}"),
        mana_str="{1}",
        sacrifice_self=True,
        remove_plus_one_counter_from_your_creature=True,
    )


def test_three_piece_tap_remove_counter_pay_life():
    """Counter-removal pairs with pay-life cleanly — both have their
    own exclusion groups, so a single bag may contain one of each."""
    cost = _parse_activation_cost(
        "{T}, Pay 1 life, Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        tap_self=True,
        pay_life=1,
        remove_plus_one_counter_from_your_creature=True,
    )


# --- Negative: duplicate counter-removal ------------------------------------

def test_two_remove_counter_pieces_returns_none():
    """Two counter-removal pieces in one cost is nonsensical and
    rejected by the bag-merge dedup check (same shape as the dedup
    on mana / tap / pay-life)."""
    assert _parse_activation_cost(
        "Remove a +1/+1 counter from a creature you control, "
        "Remove a +1/+1 counter from a creature you control"
    ) is None


def test_three_piece_with_two_remove_counter_pieces_returns_none():
    """Same dedup, three-piece variant: mana + counter + counter is
    rejected."""
    assert _parse_activation_cost(
        "{1}, Remove a +1/+1 counter from a creature you control, "
        "Remove a +1/+1 counter from a creature you control"
    ) is None


# --- Negative: variant phrasings are NOT recognized -------------------------

def test_remove_two_counters_phrasing_returns_none():
    """``Remove TWO +1/+1 counters …`` isn't the recognized literal —
    the engine field is a boolean (single counter), so a count
    variant has no field to map to. Skip cleanly."""
    assert _parse_activation_cost(
        "Remove two +1/+1 counters from a creature you control"
    ) is None


def test_remove_counter_from_self_phrasing_is_a_composable_atom():
    """``Remove a +1/+1 counter from this creature`` (counter-removal
    from the source — Walking Ballista) is a composable
    ``ActivationCost.extra_costs`` atom (cube creatures bucket), distinct
    from the "from a creature you control" dedicated field."""
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        RemoveCountersFromSelfCost,
    )

    cost = _parse_activation_cost("Remove a +1/+1 counter from this creature")
    assert cost.extra_costs == (RemoveCountersFromSelfCost(count=1),)
    assert not cost.remove_plus_one_counter_from_your_creature


def test_remove_minus_one_counter_phrasing_returns_none():
    """``Remove a -1/-1 counter …`` (different counter kind) has no
    engine field — rejected cleanly."""
    assert _parse_activation_cost(
        "Remove a -1/-1 counter from a creature you control"
    ) is None


def test_remove_counter_paired_with_unknown_atom_returns_none():
    """Counter-removal paired with a piece that doesn't classify
    (here the still-unsupported ``Mill a card``) bails out — the
    bag-merge fails fast on the first unrecognized piece."""
    assert _parse_activation_cost(
        "Remove a +1/+1 counter from a creature you control, Mill a card"
    ) is None


# --- Full activated-ability lines (cost + effect) ---------------------------

def test_remove_counter_line_with_draw_card_effect():
    """End-to-end on a synthetic line: ``Remove a +1/+1 counter from a
    creature you control: Draw a card.`` Confirms the new cost atom
    composes with the line-level parser (cost + effect dispatch)."""
    ab = _parse_activated_ability_line(
        "Remove a +1/+1 counter from a creature you control: Draw a card."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        remove_plus_one_counter_from_your_creature=True
    )
    assert ab.effect == DrawCards(amount=1)
    assert ab.sorcery_speed is False


def test_mana_plus_remove_counter_line_with_gain_life_effect():
    """Two-piece cost end-to-end: ``{2}, Remove a +1/+1 counter from a
    creature you control: You gain 3 life.`` This is the closest
    structural match to Sunstar Chaplain's exact line (same cost
    shape, but with a unified-grammar effect substituted for the
    bespoke ``Tap target artifact or creature.``)."""
    ab = _parse_activated_ability_line(
        "{2}, Remove a +1/+1 counter from a creature you control: "
        "You gain 3 life."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        remove_plus_one_counter_from_your_creature=True,
    )
    assert ab.effect == GainLife(amount=3)


def test_remove_counter_line_with_inline_sorcery_rider():
    """Inline ``Activate only as a sorcery.`` rider composes with the
    new counter-removal cost the same way it composes with mana / tap
    / sacrifice — strip the rider, parse the remainder, mark
    sorcery_speed=True."""
    ab = _parse_activated_ability_line(
        "Remove a +1/+1 counter from a creature you control: "
        "Put a +1/+1 counter on this creature. "
        "Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        remove_plus_one_counter_from_your_creature=True
    )
    assert ab.effect == PutPlusOneCounterOnSelf(amount=1)
    assert ab.sorcery_speed is True


def test_remove_counter_line_with_unparseable_effect_returns_none():
    """The new cost atom paired with an effect the unified grammar
    doesn't recognize still returns None — partial parses would let
    the ability fire without an effect, which is a silent bug."""
    ab = _parse_activated_ability_line(
        "Remove a +1/+1 counter from a creature you control: "
        "Bend the rules of physics."
    )
    assert ab is None


def test_sunstar_chaplain_cost_shape_parses_via_line_helper():
    """Sunstar Chaplain's exact activated-ability line — the cost
    parses correctly via the bag-merge classifier added in batch 12,
    and as of batch 13 the ``Tap target artifact or creature`` effect
    is also recognized by the unified grammar. The full line therefore
    now produces a complete ``ActivatedAbility``.

    Verified two ways: feeding the cost segment alone to
    ``_parse_activation_cost`` (mirroring what the line helper does
    after splitting on the colon), and then running the full line
    through ``_parse_activated_ability_line``.

    Sunstar Chaplain itself is still NOT end-to-end claimed by the
    parser — the card's other ability is an
    ``At the beginning of your end step, if you control two or more
    tapped creatures, …`` triggered ability whose conditional gate
    isn't yet in the trigger phrasebook. Once that gate is added the
    card will become parser-claimed without further activated-grammar
    work."""
    from mtgz_ir.cards.base import TapTargetArtifactOrCreature

    cost = _parse_activation_cost(
        "{2}, Remove a +1/+1 counter from a creature you control"
    )
    assert cost == ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        remove_plus_one_counter_from_your_creature=True,
    )
    ab = _parse_activated_ability_line(
        "{2}, Remove a +1/+1 counter from a creature you control: "
        "Tap target artifact or creature."
    )
    assert ab is not None
    assert ab.cost == cost
    assert ab.effect == TapTargetArtifactOrCreature()
