"""Tests for the Duskmourn artifacts / Equipment batch-1 parser
additions.

Each commit in this batch lands one card via a small additive change:
a literal-line entry on an existing builder, a name-keyed dispatch
factory on an existing builder, or a small flag widening on an
existing engine class. New bespoke effect classes are introduced in
``effects.bespoke`` only when no existing primitive captures the
printed body.

Cards newly claimed in this batch:

* **Saw** ``{2}`` Artifact - Equipment - ``Equipped creature gets
  +2/+0`` + ``Whenever equipped creature attacks, you may sacrifice a
  permanent other than that creature or this Equipment. If you do,
  draw a card.`` + ``Equip {2}``. Claimed by extending the
  equipment_builder's literal-trigger-line table with the printed
  attack-trigger line, dispatched to the new
  ``SawAttacksOptionalSacDrawEffect`` engine class. The bespoke
  effect encodes the (sacrifice-other-permanent, draw) combinator
  with the strict "permanent other than equipped creature or this
  Equipment" exclusion. ``IsHost(subject="attacker")`` matches the
  ``Whenever equipped creature attacks`` trigger condition (same
  predicate Atomic Microsizer's bespoke factory uses).
"""

# mtg-zero (docs/ENGINE.md 170): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.player_protections, not carried into mtgz_ir):
# test_saw_effect_sacrifices_and_draws

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.effects.predicates import (
    Event,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Saw: equipped-attacks may-sac-permanent draw ----------

def test_saw_claimed_by_parser():
    # Saw ``{2}`` Artifact - Equipment - claimed via the new literal
    # trigger line in ``equipment_builder``. The bespoke effect class
    # encodes the (optional-sacrifice, draw) combinator with the
    # printed "permanent other than that creature or this Equipment"
    # exclusion.
    assert "Saw" in _claimed_names()


def test_saw_carries_attacks_trigger_with_bespoke_effect():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the equipped-attacks trigger via the bespoke
    # ``SawAttacksOptionalSacDrawEffect`` engine class. The trigger
    # event is ATTACKS gated by ``IsHost(subject="attacker")`` -
    # same shape Atomic Microsizer's bespoke factory wires its
    # equipped-attacks trigger.
    from mtgz_ir.effects.bespoke import SawAttacksOptionalSacDrawEffect

    parser = OracleParserCardSource()
    p = parser.get_printings()["Saw"]
    attacks_triggers = [
        t for t in p.triggered_abilities
        if t.event == Event.ATTACKS
    ]
    assert len(attacks_triggers) == 1
    assert isinstance(
        attacks_triggers[0].effect, SawAttacksOptionalSacDrawEffect,
    )


def test_saw_carries_equipment_pump_and_equip_cost():
    # The remaining lines (``Equipped creature gets +2/+0.``,
    # ``Equip {2}``) ride the existing equipment_builder grammar - the
    # +2/+0 lands on ``equipment_bonuses`` and the equip cost lands
    # as an activated ability synthesized by ``make_equipment``.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Saw"]
    assert p.equipment_bonuses == (2, 0, frozenset())
    equip_costs = [
        a.cost.mana_str for a in p.activated_abilities
        if a.cost.mana_str
    ]
    assert "{2}" in equip_costs


# ---------- SawAttacksOptionalSacDrawEffect engine class ----------

def test_saw_effect_describe():
    # Belt-and-suspenders that the describe helper reflects the
    # combined behavior - "may sacrifice a permanent other than that
    # creature or this Equipment; if you do, draw a card."
    from mtgz_ir.effects.bespoke import SawAttacksOptionalSacDrawEffect

    desc = SawAttacksOptionalSacDrawEffect().describe()
    assert "sacrifice" in desc
    assert "draw a card" in desc


def test_saw_effect_no_op_with_no_options():
    # When the controller controls only the Equipment and the
    # equipped creature, no permanent is a legal sacrifice target so
    # the effect is a silent no-op (no draw).
    from mtgz_ir.cards.base import make_creature, make_equipment
    from mtgz_ir.effects.bespoke import SawAttacksOptionalSacDrawEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    eq_pr = make_equipment(
        "Saw", "{2}",
        power_bonus=2, toughness_bonus=0,
        equip_cost="{2}",
    )
    eq_obj = GameObject(
        id=1, printing=eq_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    host = GameObject(
        id=2, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    eq_obj.attached_to = host.id
    p0 = Player(id=0)
    p1 = Player(id=1)
    state = GameState(
        players=[p0, p1],
        battlefield=[eq_obj, host],
        resolving_source=eq_obj,
    )
    # No third permanent → no sacrifice option → silent no-op.
    SawAttacksOptionalSacDrawEffect().resolve(
        state, controller_id=0, targets=()
    )
    assert len(p0.hand) == 0




def test_saw_effect_decline_skips_sacrifice_and_draw():
    # When the controller's agent declines the optional sacrifice, no
    # permanent is sacrificed and no card is drawn.
    from mtgz_ir.cards.base import (
        make_artifact, make_creature, make_equipment,
    )
    from mtgz_ir.effects.bespoke import SawAttacksOptionalSacDrawEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    class _AgentDeclines:
        def choose_yes_no(self, state, pid, prompt):
            return False

        def choose_sacrifice(self, state, pid, legal_object_ids):
            # Should never be called when choose_yes_no returns False.
            raise AssertionError("choose_sacrifice should not be called")

    eq_pr = make_equipment(
        "Saw", "{2}",
        power_bonus=2, toughness_bonus=0,
        equip_cost="{2}",
    )
    eq_obj = GameObject(
        id=1, printing=eq_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    host = GameObject(
        id=2, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    eq_obj.attached_to = host.id
    art_pr = make_artifact("Token Art", "{1}")
    other = GameObject(
        id=3, printing=art_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    library_card = GameObject(
        id=10, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.LIBRARY,
    )
    p0 = Player(id=0, agent=_AgentDeclines(), library=[library_card])
    p1 = Player(id=1)
    state = GameState(
        players=[p0, p1],
        battlefield=[eq_obj, host, other],
        resolving_source=eq_obj,
    )
    SawAttacksOptionalSacDrawEffect().resolve(
        state, controller_id=0, targets=()
    )
    assert other in state.battlefield
    assert library_card not in p0.hand


def test_saw_effect_no_op_without_resolving_source():
    # Defensive: if ``resolving_source`` is None (engine misuse), the
    # effect short-circuits to a silent no-op rather than raising.
    from mtgz_ir.effects.bespoke import SawAttacksOptionalSacDrawEffect
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    SawAttacksOptionalSacDrawEffect().resolve(
        state, controller_id=0, targets=()
    )
    # No exception, nothing changed.
    assert len(state.battlefield) == 0


# ---------- Baseball Bat: equipped-attacks tap-up-to-one ----------

def test_baseball_bat_claimed_by_parser():
    # Baseball Bat ``{G}{W}`` Artifact - Equipment - claimed via the
    # new literal trigger line in ``equipment_builder``. The
    # ``Whenever equipped creature attacks, tap up to one target
    # creature.`` body maps to the new generic
    # ``TapUpToOneTargetCreature`` engine class.
    assert "Baseball Bat" in _claimed_names()


def test_baseball_bat_carries_attacks_trigger_with_tap_effect():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the equipped-attacks trigger via the new generic
    # ``TapUpToOneTargetCreature`` engine class. The trigger event is
    # ATTACKS gated by ``IsHost(subject="attacker")``.
    from mtgz_ir.effects import TapUpToOneTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Baseball Bat"]
    attacks_triggers = [
        t for t in p.triggered_abilities
        if t.event == Event.ATTACKS
    ]
    assert len(attacks_triggers) == 1
    assert isinstance(
        attacks_triggers[0].effect, TapUpToOneTargetCreature,
    )


def test_baseball_bat_carries_etb_attach_and_pump():
    # The ETB ``When this Equipment enters, attach it to target
    # creature you control.`` line lands as an ENTERS trigger, the
    # ``Equipped creature gets +1/+1.`` lands on
    # ``equipment_bonuses``, and the ``Equip {3}`` lands as a
    # synthesized activated ability.
    from mtgz_ir.effects import AttachToTargetCreatureYouControl

    parser = OracleParserCardSource()
    p = parser.get_printings()["Baseball Bat"]
    etb_triggers = [
        t for t in p.triggered_abilities
        if t.event == Event.ENTERS
    ]
    assert len(etb_triggers) == 1
    assert isinstance(
        etb_triggers[0].effect, AttachToTargetCreatureYouControl,
    )
    assert p.equipment_bonuses == (1, 1, frozenset())
    equip_costs = [
        a.cost.mana_str for a in p.activated_abilities
        if a.cost.mana_str
    ]
    assert "{3}" in equip_costs


# ---------- TapUpToOneTargetCreature engine class ----------

def test_tap_up_to_one_target_creature_target_range():
    # Up-to-one means 0 or 1 target, distinguishing this from sibling
    # ``TapAnotherTargetCreature`` (strict 1) and
    # ``TapTargetArtifactOrCreature`` (strict 1). After the unified
    # ``Tap`` migration ``requires_target()`` follows the structural
    # rule "any slot with count_min >= 1" — for an up-to-one slot
    # (count_min=0) the answer is False, which is CR-correct (0
    # targets is a legal resolution).
    from mtgz_ir.effects import TapUpToOneTargetCreature

    eff = TapUpToOneTargetCreature()
    assert eff.requires_target() is False
    assert eff.target_count_range() == (0, 1)


def test_tap_up_to_one_target_creature_legal_targets_includes_all_creatures():
    # The legal-target pool is ALL creatures on the battlefield
    # (controller-agnostic and source-agnostic) - the printed surface
    # form has no controller restriction.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import TapUpToOneTargetCreature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    yours = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    theirs = GameObject(
        id=2, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[yours, theirs],
    )
    legal = TapUpToOneTargetCreature().legal_targets(state, controller_id=0)
    assert ("creature", 1) in legal
    assert ("creature", 2) in legal


def test_tap_up_to_one_target_creature_taps_chosen():
    # End-to-end resolve: with one target, the chosen creature is
    # tapped via ``turn.tap_permanent``.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import TapUpToOneTargetCreature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    target = GameObject(
        id=2, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    assert target.tapped is False
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[target],
    )
    TapUpToOneTargetCreature().resolve(
        state, controller_id=0, targets=(("creature", 2),)
    )
    assert target.tapped is True


def test_tap_up_to_one_target_creature_no_targets_is_noop():
    # 0-target resolution is a silent no-op (the controller chose not
    # to target any creature).
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import TapUpToOneTargetCreature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    target = GameObject(
        id=2, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[target],
    )
    TapUpToOneTargetCreature().resolve(
        state, controller_id=0, targets=()
    )
    assert target.tapped is False


def test_tap_up_to_one_target_creature_describe():
    from mtgz_ir.effects import TapUpToOneTargetCreature

    assert TapUpToOneTargetCreature().describe() == (
        "tap up to one target creature"
    )


def test_tap_up_to_one_target_creature_invalid_target_is_noop():
    # Defensive: if the target is non-creature (engine misuse) or no
    # battlefield object matches the id, the effect is a silent no-op
    # rather than raising.
    from mtgz_ir.effects import TapUpToOneTargetCreature
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    # Wrong kind.
    TapUpToOneTargetCreature().resolve(
        state, controller_id=0, targets=(("permanent", 999),),
    )
    # Missing object.
    TapUpToOneTargetCreature().resolve(
        state, controller_id=0, targets=(("creature", 999),),
    )


# ---------- Found Footage: Clue artifact bespoke factory ----------

def test_found_footage_claimed_by_parser():
    # Found Footage ``{1}`` Artifact - Clue - claimed via a name-keyed
    # bespoke factory in ``artifact_builder``. The static
    # ``You may look at face-down creatures your opponents control any
    # time.`` is informational (no engine consequence today); the
    # activated body uses the unified-grammar Sequence(Surveil(2),
    # DrawCards(1)) materialized directly so the bespoke factory shape
    # matches a parser round-trip exactly.
    assert "Found Footage" in _claimed_names()


def test_found_footage_carries_clue_subtype():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the Clue subtype on its type line via the new ``subtype`` kwarg
    # on ``make_artifact``.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Found Footage"]
    assert "Clue" in p.subtypes
    assert "Artifact" in p.card_types


def test_found_footage_carries_face_down_static_flag():
    # The static ``You may look at face-down creatures your opponents
    # control any time.`` lifts to a printing flag. Engine-wise the
    # flag is informational today (no hidden-information modeling),
    # but it carries the printed static for fidelity.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Found Footage"]
    assert p.looks_at_opponents_face_down_creatures is True


def test_face_down_look_recognizer_matches_printed_line():
    # The recognizer lives in ``printing_keywords.py`` alongside Warp /
    # Ward / Devour / Impending; audit-sothera-foundfootage migrated
    # the ``_LITERAL_FLAG_LINES`` table entry into the standard
    # printing-keyword recognizer path.
    from mtgz_ir.cards.parser.printing_keywords import (
        _parse_looks_at_opponents_face_down_line,
    )
    assert _parse_looks_at_opponents_face_down_line(
        "You may look at face-down creatures your opponents control any time."
    ) is True
    # Other lines don't claim it.
    assert _parse_looks_at_opponents_face_down_line(
        "Surveil 2, then draw a card."
    ) is False
    # Empty / whitespace lines don't claim it.
    assert _parse_looks_at_opponents_face_down_line("") is False


def test_found_footage_carries_sac_surveil_draw_activation():
    # The activated ``{2}, Sacrifice this artifact: Surveil 2, then
    # draw a card.`` lands as a single ActivatedAbility with the
    # standard mana+sac-self cost shape and a Sequence effect body
    # matching the unified-grammar parser output for the same line.
    from mtgz_ir.effects import Surveil
    from mtgz_ir.effects.combinators import Sequence
    from mtgz_ir.effects.generic import DrawCards

    parser = OracleParserCardSource()
    p = parser.get_printings()["Found Footage"]
    assert len(p.activated_abilities) == 1
    a = p.activated_abilities[0]
    assert a.cost.mana_str == "{2}"
    assert a.cost.sacrifice_self is True
    assert isinstance(a.effect, Sequence)
    assert len(a.effect.effects) == 2
    assert isinstance(a.effect.effects[0], Surveil)
    assert a.effect.effects[0].amount == 2
    assert isinstance(a.effect.effects[1], DrawCards)
    assert a.effect.effects[1].amount == 1


# ---------- make_artifact subtype kwarg ----------

def test_make_artifact_subtype_kwarg_lands_on_subtypes():
    # New ``subtype`` kwarg on ``make_artifact`` extends the type line
    # with the ``- <subtype>`` suffix and lands the subtype on the
    # printing's ``subtypes`` tuple.
    from mtgz_ir.cards.base import make_artifact

    art = make_artifact("Test Clue", "{1}", subtype="Clue")
    assert "Clue" in art.subtypes
    assert "Artifact" in art.card_types
    assert "Clue" in art.type_line


def test_make_artifact_subtype_default_none():
    # Backward-compat sanity: omitting ``subtype=`` produces a plain
    # ``Artifact`` type line with no subtype.
    from mtgz_ir.cards.base import make_artifact

    art = make_artifact("Plain Art", "{1}")
    assert art.subtypes == ()
    assert art.type_line == "Artifact"


def test_make_artifact_legendary_with_subtype():
    # Legendary + subtype composes the prefix and suffix correctly.
    from mtgz_ir.cards.base import make_artifact

    art = make_artifact(
        "Legend Clue", "{1}", legendary=True, subtype="Clue",
    )
    assert "Legendary" in art.supertypes
    assert "Clue" in art.subtypes
    assert "Legendary Artifact" in art.type_line


# ---------- looks_at_opponents_face_down_creatures field ----------

def test_make_artifact_face_down_kwarg_lands_on_field():
    # New ``looks_at_opponents_face_down_creatures`` kwarg on
    # ``make_artifact`` lands on the same-named field on the printing.
    from mtgz_ir.cards.base import make_artifact

    art = make_artifact(
        "Test", "{1}",
        looks_at_opponents_face_down_creatures=True,
    )
    assert art.looks_at_opponents_face_down_creatures is True


def test_make_artifact_face_down_default_false():
    # Backward-compat sanity: the kwarg defaults to False, so existing
    # call sites that don't pass it keep their previous behavior.
    from mtgz_ir.cards.base import make_artifact

    art = make_artifact("Plain", "{1}")
    assert art.looks_at_opponents_face_down_creatures is False


# ---------- Keys to the House: Room lock/unlock activation ----------

def test_keys_to_the_house_claimed_by_parser():
    # Keys to the House ``{1}`` Artifact - both activations claim
    # through the unified activated grammar: the first ability
    # (basic-land tutor) via the ``SearchLibraryForCard`` recognizer,
    # and the second ability (``lock or unlock a door of target Room
    # you control``) via the new ``_LOCK_OR_UNLOCK_DOOR_TARGET_ROOM_
    # YOU_CONTROL`` regex in ``effects.py`` mapping to the generic
    # ``LockOrUnlockDoorOfTargetRoomYouControl`` engine class.
    assert "Keys to the House" in _claimed_names()


def test_keys_to_the_house_carries_two_activations():
    # First ability: ``{1}, {T}, Sacrifice this artifact`` cost +
    # SearchLibraryForCard(filter_name="basic_land") body via the
    # unified activated grammar.
    # Second ability: ``{3}, {T}, Sacrifice this artifact`` cost
    # (sorcery-speed) + LockOrUnlockDoorOfTargetRoomYouControl body —
    # both pieces also via the unified activated grammar (cost atoms
    # plus the new ``lock or unlock a door of target Room you control``
    # effect-grammar recognizer); ``Activate only as a sorcery.``
    # tail rider lifts to ``sorcery_speed=True`` on the ability.
    from mtgz_ir.effects import (
        LockOrUnlockDoorOfTargetRoomYouControl,
        SearchLibraryForCard,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Keys to the House"]
    assert len(p.activated_abilities) == 2
    # Tutor ability
    tutor = next(
        a for a in p.activated_abilities
        if isinstance(a.effect, SearchLibraryForCard)
    )
    assert tutor.cost.mana_str == "{1}"
    assert tutor.cost.tap_self is True
    assert tutor.cost.sacrifice_self is True
    assert tutor.sorcery_speed is False
    # Lock/unlock ability
    lock = next(
        a for a in p.activated_abilities
        if isinstance(a.effect, LockOrUnlockDoorOfTargetRoomYouControl)
    )
    assert lock.cost.mana_str == "{3}"
    assert lock.cost.tap_self is True
    assert lock.cost.sacrifice_self is True
    assert lock.sorcery_speed is True


# ---------- LockOrUnlockDoorOfTargetRoomYouControl engine class ----------

def test_lock_unlock_room_legal_targets_filters_to_your_rooms():
    # Legal-target pool is "Rooms YOU control" - opponents' Rooms and
    # non-Rooms are excluded.
    from mtgz_ir.cards.base import (
        ManaCost, RoomHalf, make_creature,
    )
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    # Build a fake Room printing.
    half_a = RoomHalf(name="A", mana_cost_str="{W}", mana_cost=ManaCost(generic=0))
    half_b = RoomHalf(name="B", mana_cost_str="{U}", mana_cost=ManaCost(generic=0))
    room_pr = CardPrinting(
        name="Test Room",
        mana_cost_str="{W}",
        mana_cost=ManaCost(generic=0),
        type_line="Enchantment - Room",
        supertypes=(),
        card_types=("Enchantment",),
        subtypes=("Room",),
        room_halves=(half_a, half_b),
    )
    yours_room = GameObject(
        id=1, printing=room_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
        unlocked_halves=frozenset({"A"}),
    )
    theirs_room = GameObject(
        id=2, printing=room_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
        unlocked_halves=frozenset({"A"}),
    )
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear = GameObject(
        id=3, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[yours_room, theirs_room, bear],
    )
    legal = LockOrUnlockDoorOfTargetRoomYouControl().legal_targets(
        state, controller_id=0,
    )
    assert ("permanent", 1) in legal  # your Room
    assert ("permanent", 2) not in legal  # opponent's Room
    assert ("permanent", 3) not in legal  # non-Room


def test_lock_unlock_room_describe():
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl

    desc = LockOrUnlockDoorOfTargetRoomYouControl().describe()
    assert "lock" in desc
    assert "unlock" in desc
    assert "Room" in desc


def test_lock_unlock_room_unlocks_chosen_door():
    # Default agent (no choose_mode override) picks index 0. With one
    # door locked and one unlocked, the labels are
    # ["lock A", "unlock B"]; pick=0 picks "lock A" → unlocks_halves
    # loses "A".
    from mtgz_ir.cards.base import ManaCost, RoomHalf
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    class _AgentPicksUnlock:
        def choose_mode(self, state, pid, labels):
            for i, lab in enumerate(labels):
                if lab.startswith("unlock"):
                    return i
            return 0

    half_a = RoomHalf(
        name="A", mana_cost_str="{W}", mana_cost=ManaCost(generic=0),
    )
    half_b = RoomHalf(
        name="B", mana_cost_str="{U}", mana_cost=ManaCost(generic=0),
    )
    room_pr = CardPrinting(
        name="Test Room",
        mana_cost_str="{W}",
        mana_cost=ManaCost(generic=0),
        type_line="Enchantment - Room",
        supertypes=(),
        card_types=("Enchantment",),
        subtypes=("Room",),
        room_halves=(half_a, half_b),
    )
    room = GameObject(
        id=1, printing=room_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
        unlocked_halves=frozenset({"A"}),  # B is locked
    )
    p0 = Player(id=0, agent=_AgentPicksUnlock())
    state = GameState(
        players=[p0, Player(id=1)],
        battlefield=[room],
    )
    LockOrUnlockDoorOfTargetRoomYouControl().resolve(
        state, controller_id=0, targets=(("permanent", 1),),
    )
    # B is now unlocked.
    assert "B" in room.unlocked_halves
    assert "A" in room.unlocked_halves


def test_lock_unlock_room_locks_chosen_door():
    # Picking the "lock <X>" label removes that half from
    # unlocked_halves.
    from mtgz_ir.cards.base import ManaCost, RoomHalf
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    class _AgentPicksLock:
        def choose_mode(self, state, pid, labels):
            for i, lab in enumerate(labels):
                if lab.startswith("lock"):
                    return i
            return 0

    half_a = RoomHalf(
        name="A", mana_cost_str="{W}", mana_cost=ManaCost(generic=0),
    )
    half_b = RoomHalf(
        name="B", mana_cost_str="{U}", mana_cost=ManaCost(generic=0),
    )
    room_pr = CardPrinting(
        name="Test Room",
        mana_cost_str="{W}",
        mana_cost=ManaCost(generic=0),
        type_line="Enchantment - Room",
        supertypes=(),
        card_types=("Enchantment",),
        subtypes=("Room",),
        room_halves=(half_a, half_b),
    )
    room = GameObject(
        id=1, printing=room_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
        unlocked_halves=frozenset({"A", "B"}),  # both unlocked
    )
    p0 = Player(id=0, agent=_AgentPicksLock())
    state = GameState(
        players=[p0, Player(id=1)],
        battlefield=[room],
    )
    LockOrUnlockDoorOfTargetRoomYouControl().resolve(
        state, controller_id=0, targets=(("permanent", 1),),
    )
    # Only one half is now unlocked (whichever was picked).
    assert len(room.unlocked_halves) == 1


def test_lock_unlock_room_no_target_is_noop():
    # A 0-target resolution is a silent no-op.
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    LockOrUnlockDoorOfTargetRoomYouControl().resolve(
        state, controller_id=0, targets=(),
    )
    # Nothing to assert beyond "didn't raise".


def test_lock_unlock_room_invalid_target_is_noop():
    # Defensive: non-Room target, missing object, wrong kind, or
    # opponent-controlled Room are all silent no-ops.
    from mtgz_ir.cards.base import (
        ManaCost, RoomHalf, make_creature,
    )
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear = GameObject(
        id=3, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    half_a = RoomHalf(
        name="A", mana_cost_str="{W}", mana_cost=ManaCost(generic=0),
    )
    half_b = RoomHalf(
        name="B", mana_cost_str="{U}", mana_cost=ManaCost(generic=0),
    )
    room_pr = CardPrinting(
        name="Test Room",
        mana_cost_str="{W}",
        mana_cost=ManaCost(generic=0),
        type_line="Enchantment - Room",
        supertypes=(),
        card_types=("Enchantment",),
        subtypes=("Room",),
        room_halves=(half_a, half_b),
    )
    theirs_room = GameObject(
        id=2, printing=room_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
        unlocked_halves=frozenset({"A"}),
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear, theirs_room],
    )
    eff = LockOrUnlockDoorOfTargetRoomYouControl()
    # Wrong kind.
    eff.resolve(
        state, controller_id=0, targets=(("creature", 3),),
    )
    # Missing object.
    eff.resolve(
        state, controller_id=0, targets=(("permanent", 999),),
    )
    # Non-Room.
    eff.resolve(
        state, controller_id=0, targets=(("permanent", 3),),
    )
    # Opponent-controlled Room.
    eff.resolve(
        state, controller_id=0, targets=(("permanent", 2),),
    )
    # All no-ops; opponent Room state unchanged.
    assert theirs_room.unlocked_halves == frozenset({"A"})


def test_lock_unlock_room_no_agent_uses_index_zero():
    # When the controller has no agent, the effect picks index 0 of
    # the labels list (the first door's natural lock-or-unlock toggle)
    # rather than raising.
    from mtgz_ir.cards.base import ManaCost, RoomHalf
    from mtgz_ir.cards.base import CardPrinting
    from mtgz_ir.effects import LockOrUnlockDoorOfTargetRoomYouControl
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    half_a = RoomHalf(
        name="A", mana_cost_str="{W}", mana_cost=ManaCost(generic=0),
    )
    half_b = RoomHalf(
        name="B", mana_cost_str="{U}", mana_cost=ManaCost(generic=0),
    )
    room_pr = CardPrinting(
        name="Test Room",
        mana_cost_str="{W}",
        mana_cost=ManaCost(generic=0),
        type_line="Enchantment - Room",
        supertypes=(),
        card_types=("Enchantment",),
        subtypes=("Room",),
        room_halves=(half_a, half_b),
    )
    room = GameObject(
        id=1, printing=room_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
        unlocked_halves=frozenset({"A"}),
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[room],
    )
    # No agent on player 0 → picks index 0 = "lock A" → A removed.
    LockOrUnlockDoorOfTargetRoomYouControl().resolve(
        state, controller_id=0, targets=(("permanent", 1),),
    )
    assert "A" not in room.unlocked_halves
