"""Tests for the third Duskmourn creature parser batch.

This batch grows the parser to claim a small cluster of Duskmourn
creatures whose oracle text needs new trigger / effect / static-gate
primitives layered on top of the existing engine surface — no new
events or core data fields are required.

* **``HasPowerAtMost`` predicate** + **``another creature you control
  with power 2 or less enters`` trigger phrase** — Vicious Clown
  (``{2}{R}`` 2/3) prints this trigger condition with the existing
  ``this creature gets +N/+0 until end of turn`` self-pump body
  (``PumpSelfUntilEOT``). The same condition phrase is also used by
  Irreverent Gremlin and is forward-looking grammar coverage today
  (Gremlin's effect body needs a separate optional-discard-then-draw
  primitive that's out of scope for this batch).
"""

# mtg-zero (docs/ENGINE.md 169): 10 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_grant_unblockable_attacking_only_filters_legal_targets, test_grant_unblockable_exclude_self_filters_source, test_grant_unblockable_power_at_most_filters_high_power_targets, test_has_power_at_most_handles_missing_subject, test_has_power_at_most_matches_low_power_creature, test_has_power_at_most_rejects_higher_power_creature, test_has_power_at_most_rejects_player_subject, test_static_unlocked_doors_gate_active_at_threshold, test_static_unlocked_doors_gate_ignores_opponent_rooms, test_static_unlocked_doors_gate_inactive_below_threshold

from __future__ import annotations

from mtgz_ir.cards.base import (
    PumpSelfUntilEOT,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.effects.conditions import UnlockedDoorsAmongRoomsAtLeast
from mtgz_ir.effects.object_predicates import (
    HasPowerAtMost,
    IsCreature,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    StateBound,
    Subject,
    YOU,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- ``HasPowerAtMost`` predicate ----------










# ---------- trigger-phrase entry ----------


def test_parse_another_creature_with_power_2_or_less_enters_trigger():
    # End-to-end through the trigger-line parser. The condition phrase
    # claims via the predicate-trigger phrasebook with an
    # ``IsCreature() & Controlled(YOU) & HasPowerAtMost(n=2) & ~IsSelf()``
    # composite predicate; the body parses via the existing
    # ``_PUMP_SELF_UEOT`` primitive.
    line = (
        "Whenever another creature you control with power 2 or less "
        "enters, this creature gets +2/+0 until end of turn."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.ENTERS,
        predicate=(
            Subject(None, IsCreature() & HasPowerAtMost(n=2))
            & Controlled(YOU)
            & ~IsSelf()
        ),
        effect=PumpSelfUntilEOT(power_bonus=2, toughness_bonus=0),
    )


# ---------- end-to-end claim ----------


def test_vicious_clown_claimed_by_parser():
    # Vicious Clown ``{2}{R}`` 2/3 Human Clown. Single oracle line:
    # ``Whenever another creature you control with power 2 or less
    # enters, this creature gets +2/+0 until end of turn.`` Claims
    # via the new trigger-phrase entry feeding the existing
    # ``PumpSelfUntilEOT`` self-pump primitive.
    assert "Vicious Clown" in _claimed_names()


# ---------- ``GrantUnblockableUEOT`` attacking_only / exclude_self ----------






def test_grant_unblockable_describe_attacking_other():
    # The ``describe`` output reflects the attacking-only +
    # exclude-self combo as ``another target attacking creature can't
    # be blocked this turn`` — the Clammy Prowler surface form.
    from mtgz_ir.cards.base import GrantUnblockableUEOT
    eff = GrantUnblockableUEOT(attacking_only=True, exclude_self=True)
    assert eff.describe() == (
        "another target attacking creature can't be blocked this turn"
    )


def test_grant_unblockable_describe_attacking_only():
    # Attacking-only without exclude-self describes as ``target
    # attacking creature can't be blocked this turn`` — forward-looking
    # surface form not used by any DSK printing today but kept
    # symmetric with the other flag combinations.
    from mtgz_ir.cards.base import GrantUnblockableUEOT
    eff = GrantUnblockableUEOT(attacking_only=True)
    assert eff.describe() == (
        "target attacking creature can't be blocked this turn"
    )


def test_parse_another_target_attacking_creature_unblockable():
    # End-to-end through the effect-phrase parser. The phrase
    # ``another target attacking creature can't be blocked this turn``
    # parses to ``GrantUnblockableUEOT(attacking_only=True,
    # exclude_self=True)``.
    from mtgz_ir.cards.base import GrantUnblockableUEOT
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    parsed = _parse_effect_phrase(
        "another target attacking creature can't be blocked this turn"
    )
    assert parsed == GrantUnblockableUEOT(
        attacking_only=True, exclude_self=True,
    )


def test_clammy_prowler_claimed_by_parser():
    # Clammy Prowler ``{3}{U}`` 2/5 Enchantment Creature — Horror.
    # Single oracle line: ``Whenever this creature attacks, another
    # target attacking creature can't be blocked this turn.`` Claims
    # via the existing ``this creature attacks`` predicate-trigger
    # phrase + the new ``another target attacking creature`` variant
    # of the unblockable-UEOT primitive.
    assert "Clammy Prowler" in _claimed_names()


# ---------- ``unlocked doors among Rooms`` static gate ----------








def test_rampaging_soulrager_claimed_by_parser():
    # Rampaging Soulrager ``{2}{R}`` 1/4 Spirit. Single oracle line:
    # ``This creature gets +3/+0 as long as there are two or more
    # unlocked doors among Rooms you control.`` Claims via the
    # existing trailing ``as long as <condition>`` static-ability
    # grammar feeding the new ``requires_unlocked_doors_among_rooms``
    # gate field on ``StaticAbility``.
    assert "Rampaging Soulrager" in _claimed_names()


def test_rampaging_soulrager_carries_unlocked_doors_static():
    # The claimed printing must actually carry the new gate field on
    # its static ability — otherwise the engine wouldn't gate the +3/+0
    # bonus on the door threshold and the printing would over-pump
    # whenever it's on the battlefield. PR-D continuous-effects shape:
    # P/T contribution rides on a layer-7c
    # ContinuousModifier(PowerToughnessBonus); the legacy
    # requires_unlocked_doors_among_rooms gate stays as the surface today.
    from mtgz_ir.cards.parser.source import OracleParserCardSource
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, PowerToughnessBonus,
    )
    src = OracleParserCardSource()
    soulrager = src.get_printings()["Rampaging Soulrager"]
    statics = soulrager.static_abilities
    assert len(statics) == 1
    static = statics[0]
    assert static.continuous_effect == ContinuousModifier(
        layer=7,
        payload=PowerToughnessBonus(power_bonus=3, toughness_bonus=0),
    )
    assert static.condition == UnlockedDoorsAmongRoomsAtLeast(2)


# ---------- intervening-``while`` trigger shape ----------


def test_parse_pump_self_with_it_subject_pronoun():
    # Belt-and-suspenders that the relaxed self-pump regexes accept the
    # ``it`` source-subject pronoun (``it gets +N/+M ...``) — Hand That
    # Feeds prints its trigger body with ``it`` rather than the
    # ``this creature`` form Eerie cards use, and the
    # ``_normalize_creature_effect_phrase`` normalizer strips ``it ``
    # leaving the bare ``gets +N/+M`` shape that the regexes now
    # tolerate via an optional ``(?:this creature )?`` prefix.
    from mtgz_ir.cards.base import PumpSelfUntilEOT
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    parsed = _parse_effect_phrase(
        "it gets +2/+0 and gains menace until end of turn"
    )
    assert parsed == PumpSelfUntilEOT(
        power_bonus=2, toughness_bonus=0,
        granted_keywords=frozenset({"menace"}),
    )


def test_parse_pump_self_with_this_creature_subject_unchanged():
    # Belt-and-suspenders that adding the optional ``(?:this creature
    # )?`` prefix didn't break the canonical surface — the
    # ``this creature gets +N/+M`` form Eerie cards use must continue
    # to claim the same engine class.
    from mtgz_ir.cards.base import PumpSelfUntilEOT
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    parsed = _parse_effect_phrase(
        "this creature gets +2/+0 and gains lifelink until end of turn"
    )
    assert parsed == PumpSelfUntilEOT(
        power_bonus=2, toughness_bonus=0,
        granted_keywords=frozenset({"lifelink"}),
    )


def test_parse_intervening_while_trigger_line():
    # End-to-end through the trigger-line parser. The intervening-
    # ``while`` shape composes the trigger condition (``this creature
    # attacks``) with the ``while`` gate predicate (``Delirium``) via
    # plain ``And``, mirroring the middle-clause intervening-if path.
    from mtgz_ir.cards.base import (
        PumpSelfUntilEOT,
        TriggeredAbility,
    )
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    from mtgz_ir.effects.state_predicates import (
        Delirium,
    )
    from mtgz_ir.effects.predicates import (
        Event,
        IsSelf,
    )
    line = (
        "Whenever this creature attacks while there are four or more "
        "card types among cards in your graveyard, it gets +2/+0 and "
        "gains menace until end of turn."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=PumpSelfUntilEOT(
            power_bonus=2, toughness_bonus=0,
            granted_keywords=frozenset({"menace"}),
        ),
    )


def test_parse_intervening_while_unrecognized_gate_rejects():
    # An unrecognized ``while`` gate phrase rejects the whole line —
    # same don't-silently-drop-the-gate discipline as the
    # ``, if `` middle-clause path. The line shape regex matches
    # because the ``while`` boundary is present, but the gate phrase
    # lookup fails so the parser declines.
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    line = (
        "Whenever this creature attacks while the moon is full, "
        "it gets +1/+0 until end of turn."
    )
    assert _parse_triggered_ability_line(line) is None


def test_hand_that_feeds_claimed_by_parser():
    # Hand That Feeds ``{1}{R}`` 2/2 Mutant. Single oracle line:
    # ``Delirium — Whenever this creature attacks while there are four
    # or more card types among cards in your graveyard, it gets +2/+0
    # and gains menace until end of turn.`` Claims via the existing
    # ``Delirium —`` ability-word prefix strip + the new
    # intervening-``while`` line shape feeding the existing
    # ``StateBound(Delirium())`` gate predicate, with the body parsing through the
    # relaxed ``it``-or-``this creature`` self-pump-and-grant grammar.
    assert "Hand That Feeds" in _claimed_names()


# ---------- ``GrantUnblockableUEOT`` power_at_most filter ----------




def test_grant_unblockable_power_at_most_describe():
    # The describe output spells out the power threshold so log
    # messages are readable.
    from mtgz_ir.cards.base import GrantUnblockableUEOT
    eff = GrantUnblockableUEOT(power_at_most=2)
    assert eff.describe() == (
        "target creature with power 2 or less can't be blocked this turn"
    )


def test_parse_target_creature_with_power_at_most_unblockable():
    # End-to-end through the effect-phrase parser. The phrase
    # ``target creature with power 2 or less can't be blocked this
    # turn`` parses to ``GrantUnblockableUEOT(power_at_most=2)``.
    from mtgz_ir.cards.base import GrantUnblockableUEOT
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    parsed = _parse_effect_phrase(
        "target creature with power 2 or less can't be blocked this turn"
    )
    assert parsed == GrantUnblockableUEOT(power_at_most=2)


def test_ragged_playmate_claimed_by_parser():
    # Ragged Playmate ``{1}{R}`` 2/2 Artifact Creature — Toy. Single
    # oracle line: ``{1}, {T}: Target creature with power 2 or less
    # can't be blocked this turn.`` Claims via the existing
    # ``{mana}, {T}`` activation-cost grammar plus the new
    # ``power_at_most`` variant of the unblockable-UEOT primitive.
    assert "Ragged Playmate" in _claimed_names()
