"""Tests for the Duskmourn artifacts / Equipment batch-3 parser
additions.

This batch originally landed three DSK artifact creatures via name-keyed
bespoke dispatch on ``_BESPOKE_ARTIFACT_CREATURES``. They have since been
migrated (bespoke-delirium) to the literal trigger-line dispatch in
``_BESPOKE_TRIGGER_LINES`` — their complex trigger bodies remain bespoke
effect classes, but all other oracle lines (keywords, P/T, subtypes,
mana cost, artifact type) now route through the unified grammar.

Additionally, the bespoke-delirium branch wires Delirium-gate support for
``Delirium — At the beginning of <when>`` trigger shapes in
``triggers._parse_at_the_beginning_of_line`` via the new ``_DELIRIUM_PREFIX``
/ ``_DELIRIUM_AT_BEGINNING_GATE_PREFIX`` constants — enabling any future
card whose Delirium-gated end-step (or upkeep, or combat) body is
expressible by the unified effect grammar to claim end-to-end with zero
new code.
"""

# mtg-zero (docs/ENGINE.md 169): 2 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.layouts, not carried into mtgz_ir):
# test_arabella_effect_deals_damage_and_gains_life, test_arabella_effect_excludes_high_power_creatures

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.effects.predicates import (
    Event,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Splitskin Doll ----------

def test_splitskin_doll_claimed_by_parser():
    # Splitskin Doll claims via the oracle-text parser end-to-end. The
    # ETB body is decomposed by the parser's multi-sentence literal
    # recognizer in ``effects._parse_effect`` into a generic
    # ``Sequence((DrawCards, IfNot(ControlsAnother..., DiscardSelf)))``.
    # No entry in ``_BESPOKE_TRIGGER_LINES`` and no bespoke engine class.
    assert "Splitskin Doll" in _claimed_names()


def test_splitskin_doll_carries_etb_with_decomposed_sequence():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries a
    # single ENTERS trigger whose effect is a generic ``Sequence`` of
    # ``DrawCards(amount=1)`` and an ``IfNot`` gating ``DiscardSelf(
    # amount=1)`` on
    # ``ControlsAnotherCreatureWithPowerAtMost(threshold=2)``.
    from mtgz_ir.effects.combinators import IfNot, Sequence
    from mtgz_ir.effects.conditions import (
        ControlsAnotherCreatureWithPowerAtMost,
    )
    from mtgz_ir.effects.generic import DiscardSelf, DrawCards

    p = OracleParserCardSource().get_printings()["Splitskin Doll"]
    assert p.power == 2 and p.toughness == 1
    assert p.is_artifact is True
    assert "Toy" in p.subtypes
    etb = [t for t in p.triggered_abilities if t.event == Event.ENTERS]
    assert len(etb) == 1
    eff = etb[0].effect
    assert isinstance(eff, Sequence)
    assert len(eff.effects) == 2
    assert eff.effects[0] == DrawCards(amount=1)
    second = eff.effects[1]
    assert isinstance(second, IfNot)
    assert second.condition == ControlsAnotherCreatureWithPowerAtMost(
        threshold=2,
    )
    assert second.then == DiscardSelf(amount=1)


# ---------- Splitskin Doll behavioral tests ----------

def _resolve_splitskin_etb(state, controller_id):
    """Helper: resolve Splitskin Doll's ETB by fetching the parser-
    produced trigger effect and resolving it directly. Mirrors how the
    engine's trigger machinery would invoke it at resolve time.
    """
    p = OracleParserCardSource().get_printings()["Splitskin Doll"]
    etb = [
        t for t in p.triggered_abilities if t.event == Event.ENTERS
    ][0]
    etb.effect.resolve(state, controller_id=controller_id, targets=())


def test_splitskin_effect_dodges_discard_when_another_creature_with_power_2_present():
    # When the controller controls another creature whose effective
    # power is 2 or less, the discard is dodged — only the draw fires.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    class _Agent:
        def choose_discard(self, state, pid, hand_ids):
            return hand_ids[0]

    doll_pr = make_creature(
        "Splitskin Doll", "{1}{W}", 2, 1, artifact=True,
        subtypes=("Toy",),
    )
    bear_pr = make_creature("Bear", "{1}{G}", 2, 2)
    doll = GameObject(
        id=1, printing=doll_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    other = GameObject(
        id=2, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    deck = GameObject(
        id=10, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.LIBRARY,
    )
    hand_card = GameObject(
        id=11, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.HAND,
    )
    p0 = Player(id=0, agent=_Agent(), library=[deck], hand=[hand_card])
    p1 = Player(id=1)
    state = GameState(
        players=[p0, p1],
        battlefield=[doll, other],
        resolving_source=doll,
    )
    _resolve_splitskin_etb(state, controller_id=0)
    # Drew the deck card; discard was dodged so hand_card stays.
    assert deck in p0.hand
    assert hand_card in p0.hand
    assert hand_card not in p0.graveyard


def test_splitskin_effect_discards_when_no_other_small_creature():
    # Without another creature of power 2 or less, the discard fires
    # — the controller's agent picks via ``choose_discard``.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    class _Agent:
        def choose_discard(self, state, pid, hand_ids):
            return hand_ids[0]

    doll_pr = make_creature(
        "Splitskin Doll", "{1}{W}", 2, 1, artifact=True,
        subtypes=("Toy",),
    )
    big_pr = make_creature("Big", "{4}{G}", 5, 5)
    doll = GameObject(
        id=1, printing=doll_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    big = GameObject(
        id=2, printing=big_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    bear_pr = make_creature("Bear", "{1}{G}", 2, 2)
    deck = GameObject(
        id=10, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.LIBRARY,
    )
    hand_card = GameObject(
        id=11, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.HAND,
    )
    p0 = Player(id=0, agent=_Agent(), library=[deck], hand=[hand_card])
    p1 = Player(id=1)
    state = GameState(
        players=[p0, p1],
        battlefield=[doll, big],
        resolving_source=doll,
    )
    _resolve_splitskin_etb(state, controller_id=0)
    # Drew the deck card; discarded the hand card (no dodger present —
    # the doll itself is excluded by the "another creature" clause and
    # ``big`` has power 5).
    assert deck in p0.hand
    assert hand_card not in p0.hand
    assert hand_card in p0.graveyard


def test_splitskin_effect_doll_does_not_count_as_its_own_dodger():
    # The doll itself has printed power 2, but the printed clause says
    # "another creature" — so the source must be excluded from the
    # dodger pool. With the doll as the only creature, the discard
    # fires.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    class _Agent:
        def choose_discard(self, state, pid, hand_ids):
            return hand_ids[0]

    doll_pr = make_creature(
        "Splitskin Doll", "{1}{W}", 2, 1, artifact=True,
        subtypes=("Toy",),
    )
    doll = GameObject(
        id=1, printing=doll_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    bear_pr = make_creature("Bear", "{1}{G}", 2, 2)
    deck = GameObject(
        id=10, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.LIBRARY,
    )
    hand_card = GameObject(
        id=11, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.HAND,
    )
    p0 = Player(id=0, agent=_Agent(), library=[deck], hand=[hand_card])
    p1 = Player(id=1)
    state = GameState(
        players=[p0, p1],
        battlefield=[doll],
        resolving_source=doll,
    )
    _resolve_splitskin_etb(state, controller_id=0)
    assert hand_card in p0.graveyard


def test_splitskin_effect_no_op_on_empty_hand_after_no_dodger_path():
    # No dodger and an empty hand → the discard branch silently no-ops
    # (no exception, nothing to discard). ``DiscardSelf`` returns False
    # on empty hand, which is the standard generic behavior.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    doll_pr = make_creature(
        "Splitskin Doll", "{1}{W}", 2, 1, artifact=True,
        subtypes=("Toy",),
    )
    doll = GameObject(
        id=1, printing=doll_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    p0 = Player(id=0, library=[], hand=[])
    p1 = Player(id=1)
    state = GameState(
        players=[p0, p1],
        battlefield=[doll],
        resolving_source=doll,
    )
    _resolve_splitskin_etb(state, controller_id=0)


# ---------- Arabella, Abandoned Doll ----------

def test_arabella_claimed_by_parser():
    # Arabella lands via name-keyed bespoke dispatch in
    # ``_BESPOKE_LEGENDARY_CREATURES``.
    assert "Arabella, Abandoned Doll" in _claimed_names()


def test_arabella_carries_attacks_trigger_with_bespoke_effect():
    # Belt-and-suspenders that the produced ``CardPrinting`` is the
    # legendary artifact creature carrying the bespoke
    # ``ArabellaAttackEffect`` engine class on a single ATTACKS trigger.
    from mtgz_ir.effects.bespoke import ArabellaAttackEffect

    p = OracleParserCardSource().get_printings()["Arabella, Abandoned Doll"]
    assert p.power == 1 and p.toughness == 3
    assert p.is_artifact is True
    assert "Legendary" in p.supertypes
    assert "Toy" in p.subtypes
    attacks = [
        t for t in p.triggered_abilities if t.event == Event.ATTACKS
    ]
    assert len(attacks) == 1
    assert isinstance(attacks[0].effect, ArabellaAttackEffect)


def test_arabella_effect_describe():
    from mtgz_ir.effects.bespoke import ArabellaAttackEffect

    desc = ArabellaAttackEffect().describe()
    assert "X damage" in desc
    assert "power 2 or less" in desc






def test_arabella_effect_no_op_when_x_is_zero():
    # Without any creature of power 2 or less, X = 0 — the effect is
    # a silent no-op (no damage, no life gain).
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects.bespoke import ArabellaAttackEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    big_pr = make_creature("Big", "{4}{G}", 5, 5)
    big = GameObject(
        id=1, printing=big_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    p0 = Player(id=0, life=10)
    p1 = Player(id=1, life=20)
    state = GameState(
        players=[p0, p1], battlefield=[big],
        resolving_source=big,
    )
    ArabellaAttackEffect().resolve(state, controller_id=0, targets=())
    assert p1.life == 20
    assert p0.life == 10


# ---------- Attack-in-the-Box ----------

def test_attack_in_the_box_claimed_by_parser():
    # Attack-in-the-Box now claims via the oracle-text parser end-to-
    # end: every line (mana cost, P/T, artifact type, Toy subtype,
    # attack trigger) routes through the unified grammar. The attack-
    # trigger body matches a multi-sentence literal recognizer in
    # ``effects._parse_effect`` and decomposes into
    # ``IfYouDo(MayDo(PumpSelfUntilEOT(...)),
    # ScheduleSelfSacAtNextEndStep())``. Migrated in
    # audit-litdispatch-creatures.
    assert "Attack-in-the-Box" in _claimed_names()


def test_attack_in_the_box_carries_attacks_trigger_with_combinator_tree():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the attack trigger as the canonical
    # ``IfYouDo(MayDo(PumpSelfUntilEOT(power_bonus=4, toughness_bonus=0)),
    # ScheduleSelfSacAtNextEndStep())`` composition rather than any
    # per-card class.
    from mtgz_ir.effects.combinators import IfYouDo, MayDo
    from mtgz_ir.effects.generic import (
        PumpSelfUntilEOT, ScheduleSelfSacAtNextEndStep,
    )

    p = OracleParserCardSource().get_printings()["Attack-in-the-Box"]
    assert p.power == 2 and p.toughness == 4
    assert p.is_artifact is True
    assert "Toy" in p.subtypes
    attacks = [
        t for t in p.triggered_abilities if t.event == Event.ATTACKS
    ]
    assert len(attacks) == 1
    expected = IfYouDo(
        do_action=MayDo(
            PumpSelfUntilEOT(power_bonus=4, toughness_bonus=0),
        ),
        then_action=ScheduleSelfSacAtNextEndStep(),
    )
    assert attacks[0].effect == expected


# ---------- Wickerfolk Thresher ----------

def test_wickerfolk_thresher_claimed_by_parser():
    # Wickerfolk Thresher now claims via the oracle-text parser end-to-
    # end: every line (mana cost, P/T, artifact type, Scarecrow
    # subtype, Delirium attack trigger) routes through the unified
    # grammar. The trigger body dispatches to the canonical
    # ``PeekTopMayBattlefieldIfLandElseHand`` primitive via a multi-
    # sentence literal recognizer in ``effects._parse_effect``.
    # Migrated in audit-litdispatch-creatures.
    assert "Wickerfolk Thresher" in _claimed_names()


def test_wickerfolk_thresher_carries_attacks_trigger_with_delirium_gate():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the attack trigger as ``PeekTopMayBattlefieldIfLandElseHand``,
    # with the ``StateBound(Delirium())`` predicate composed as an
    # intervening-if gate on top of the ``IsSelf()`` base.
    from mtgz_ir.effects.generic import (
        PeekTopMayBattlefieldIfLandElseHand,
    )

    p = OracleParserCardSource().get_printings()["Wickerfolk Thresher"]
    assert p.power == 5 and p.toughness == 4
    assert p.is_artifact is True
    assert "Scarecrow" in p.subtypes
    attacks = [
        t for t in p.triggered_abilities if t.event == Event.ATTACKS
    ]
    assert len(attacks) == 1
    assert isinstance(
        attacks[0].effect, PeekTopMayBattlefieldIfLandElseHand,
    )
    # StateBound(Delirium()) should be present in the predicate tree.
    pred_repr = repr(attacks[0].predicate)
    assert "Delirium" in pred_repr


# ---------- Osseous Sticktwister ----------
#
# The bespoke ``OsseousSticktwisterEndStepEffect`` was deleted in
# audit-per-opponent-choice. Osseous now claims end-to-end through the
# unified parser: the Delirium-gated end-step trigger header parses via
# ``_parse_at_the_beginning_of_line`` (with ``StateBound(Delirium())``
# composed onto the predicate); the body decomposes into the new
# generic ``EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage``
# primitive (in ``effects/generic.py``) via a multi-sentence literal
# recognizer in ``cards/parser/effects.py``. Behavioral coverage of
# every choice branch lives in
# ``tests/test_per_opponent_choice_primitives.py``; the cases below
# verify the parser dispatch and end-to-end shape only.

def test_osseous_sticktwister_claimed_by_parser():
    assert "Osseous Sticktwister" in _claimed_names()


def test_osseous_sticktwister_carries_lifelink_and_delirium_end_step_trigger():
    # Verify the produced ``CardPrinting`` carries lifelink and the
    # Delirium-gated end-step trigger wired to the new generic primitive.
    from mtgz_ir.effects import (
        EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage,
    )

    p = OracleParserCardSource().get_printings()["Osseous Sticktwister"]
    assert p.power == 2 and p.toughness == 2
    assert p.is_artifact is True
    assert "Scarecrow" in p.subtypes
    assert "lifelink" in p.keywords
    end_step = [
        t for t in p.triggered_abilities
        if t.event == Event.BEGINNING_OF_END_STEP
    ]
    assert len(end_step) == 1
    assert isinstance(
        end_step[0].effect,
        EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage,
    )
    pred_repr = repr(end_step[0].predicate)
    assert "Delirium" in pred_repr
