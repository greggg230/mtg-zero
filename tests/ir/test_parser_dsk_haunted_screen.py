"""Tests for the DSK Haunted Screen parser claim.

Haunted Screen prints three activations:

  1. ``{T}: Add {W} or {B}.`` — mana ability claimed via the unified
     ``add {<c1>} or {<c2>}`` recognizer →
     ``AddNManaOfChosenColor(amount=1, options=("W","B"))`` flagged
     ``is_mana_ability=True``.
  2. ``{T}, Pay 1 life: Add {G}, {U}, or {R}.`` — mana ability claimed
     via the unified ``add {<c1>}, {<c2>}, or {<c3>}`` recognizer →
     ``AddNManaOfChosenColor(amount=1, options=("G","U","R"))`` flagged
     ``is_mana_ability=True``, with ``pay_life=1``.
  3. ``{7}: Put seven +1/+1 counters on this artifact. It becomes a
     0/0 Spirit creature in addition to its other types. Activate
     only once.`` — once-only activation. The body decomposes into
     ``Sequence(PutPlusOneCounterOnSelf(amount=7),
     BecomeType(types_to_add=("creature",),
     subtypes_to_add=("Spirit",), base_pt=(0, 0)))`` via the
     multi-sentence literal recognizer in ``effects.py``. The trailing
     ``Activate only once.`` rider lifts to ``activate_only_once=True``
     on the ability via the inline-rider strip in ``activated.py``.

The per-source ``GameObject.has_activated_once_only_ability`` flag is
latched at activation time (in ``_activate_ability`` in ``actions.py``)
and read by ``legal_actions``'s gate (``if ability.activate_only_once
and obj.has_activated_once_only_ability: continue``) so a second
activation — even before the first resolves off the stack — is
filtered out at legality time.
"""

# mtg-zero (docs/ENGINE.md 169): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_haunted_screen_once_only_legal_only_once

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Haunted Screen ----------

def test_haunted_screen_claimed_by_parser():
    assert "Haunted Screen" in _claimed_names()


def test_haunted_screen_carries_three_activations():
    from mtgz_ir.effects import (
        AddNManaOfChosenColor,
        BecomeType,
        PutPlusOneCounterOnSelf,
        Sequence,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Haunted Screen"]
    assert len(p.activated_abilities) == 3
    mana_abilities = [
        a for a in p.activated_abilities
        if isinstance(a.effect, AddNManaOfChosenColor)
    ]
    assert len(mana_abilities) == 2
    first = next(
        a for a in mana_abilities
        if a.cost.pay_life == 0
    )
    assert first.cost.tap_self is True
    assert first.cost.mana_str == ""
    assert first.is_mana_ability is True
    assert first.effect.options == ("W", "B")
    assert first.effect.amount == 1
    second = next(
        a for a in mana_abilities
        if a.cost.pay_life == 1
    )
    assert second.cost.tap_self is True
    assert second.is_mana_ability is True
    assert second.effect.options == ("G", "U", "R")
    assert second.effect.amount == 1
    third = next(
        a for a in p.activated_abilities
        if isinstance(a.effect, Sequence)
    )
    assert third.cost.mana_str == "{7}"
    assert third.cost.tap_self is False
    assert third.cost.sacrifice_self is False
    assert third.sorcery_speed is False
    assert third.is_mana_ability is False
    assert third.activate_only_once is True
    # Body decomposes into two generic primitives.
    assert len(third.effect.effects) == 2
    counters, become = third.effect.effects
    assert isinstance(counters, PutPlusOneCounterOnSelf)
    assert counters.amount == 7
    assert isinstance(become, BecomeType)
    assert become.types_to_add == ("creature",)
    assert become.subtypes_to_add == ("Spirit",)
    assert become.base_pt == (0, 0)


def test_haunted_screen_has_no_triggers_or_statics():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Haunted Screen"]
    assert p.triggered_abilities == ()
    assert p.static_abilities == ()


# ---------- BecomeType engine class ----------

def test_become_type_describe():
    from mtgz_ir.effects import BecomeType

    desc = BecomeType(
        types_to_add=("creature",),
        subtypes_to_add=("Spirit",),
        base_pt=(0, 0),
    ).describe()
    assert "Spirit" in desc
    assert "0/0" in desc
    assert "in addition to its other types" in desc


def test_become_type_first_resolve_transforms():
    """First resolve writes the layer-4/7 registry entries with
    ``expires_at=None`` plus the ``permanent_is_creature`` cache."""
    from mtgz_ir.cards.base import make_artifact
    from mtgz_ir.effects import BecomeType
    from mtgz_ir.game import (
        GameObject, GameState, Player, Zone,
        effective_subtypes, is_creature_now,
    )

    art_pr = make_artifact("Haunted Screen", "{3}")
    source_obj = GameObject(
        id=10, printing=art_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[source_obj],
        resolving_source=source_obj,
    )
    BecomeType(
        types_to_add=("creature",),
        subtypes_to_add=("Spirit",),
        base_pt=(0, 0),
    ).resolve(state, controller_id=0, targets=())
    assert source_obj.permanent_is_creature is True
    assert is_creature_now(source_obj)
    assert "Spirit" in effective_subtypes(state, source_obj)
    # All written registry entries are permanent (expires_at=None).
    assert all(
        ce.expires_at is None for ce in state.continuous_effects
    )


def test_become_type_no_source_is_noop():
    """``resolving_source`` is None → silent no-op."""
    from mtgz_ir.effects import BecomeType
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    BecomeType(
        types_to_add=("creature",),
        subtypes_to_add=("Spirit",),
        base_pt=(0, 0),
    ).resolve(state, controller_id=0, targets=())
    # Nothing to assert beyond "didn't raise".
    assert state.continuous_effects == []


def test_become_type_no_subtype_skips_subtype_grant():
    """Empty ``subtypes_to_add`` skips the layer-4 add_subtype registry
    entry but still writes the layer-4 add_type and layer-7
    set_base_pt."""
    from mtgz_ir.cards.base import make_artifact
    from mtgz_ir.effects import BecomeType
    from mtgz_ir.game import (
        GameObject, GameState, Player, Zone,
        is_creature_now,
    )

    art_pr = make_artifact("Some Artifact", "{1}")
    source_obj = GameObject(
        id=10, printing=art_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[source_obj],
        resolving_source=source_obj,
    )
    BecomeType(
        types_to_add=("creature",),
        base_pt=(2, 2),
    ).resolve(state, controller_id=0, targets=())
    assert is_creature_now(source_obj)
    # No add_subtype entries.
    add_subtype_entries = [
        ce for ce in state.continuous_effects
        if ce.kind == "add_subtype"
    ]
    assert add_subtype_entries == []


def test_become_type_subtype_only_skips_pt_and_type_grants():
    """Empty types_to_add and base_pt → only add_subtype entries written."""
    from mtgz_ir.cards.base import make_artifact
    from mtgz_ir.effects import BecomeType
    from mtgz_ir.game import (
        GameObject, GameState, Player, Zone,
        effective_subtypes,
    )

    art_pr = make_artifact("Curio", "{2}")
    source_obj = GameObject(
        id=11, printing=art_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[source_obj],
        resolving_source=source_obj,
    )
    BecomeType(subtypes_to_add=("Treasure",)).resolve(
        state, controller_id=0, targets=(),
    )
    assert source_obj.permanent_is_creature is False
    assert "Treasure" in effective_subtypes(state, source_obj)
    set_base_pt_entries = [
        ce for ce in state.continuous_effects
        if ce.kind == "set_base_pt"
    ]
    assert set_base_pt_entries == []


# ---------- Once-only activation gating ----------

