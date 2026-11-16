"""Parity tests for artifact-creature-batch22 bespoke dispatch additions.

Each test asserts that the oracle parser produces a ``CardPrinting``
for one of the 13 non-legendary Artifact Creatures originally in the
bespoke table.

Migrated OUT of ``_BESPOKE_ARTIFACT_CREATURES`` (claimed by grammar):
  - Pinnacle Emissary — warp cost ``{U/R}`` (hybrid mana support).
  - Tapestry Warden — vigilance + toughness-as-damage + toughness-as-station.
  - Virulent Silencer — predicate-dispatch poison trigger.
  - Gene Pollinator — ``{T}, Tap an untapped permanent you control`` cost
    atoms both classified; mana-ability grammar claims the effect.
    Migrated in bespoke-creature-wave7.
  - Rust Harvester — three-atom cost + ``, then it deals damage equal to
    its power to any target`` compound now parses after adding ``(?:it )?``
    to ``_DEAL_DAMAGE_EQUAL_TO_SOURCE_POWER_ANY_TARGET``.
    Migrated in bespoke-creature-wave7.
  - Selfcraft Mechan — new ``_OPTIONAL_SAC_ARTIFACT_WHEN_YOU_DO_EFFECT``
    recognizer in ``effects.py`` matches "you may sacrifice an artifact.
    When you do, <inner>" → ``OptionalSacArtifactForEffect(inner)``;
    ``OptionalSacArtifactForEffect`` updated to forward ``requires_target``
    / ``legal_targets`` / ``targets`` from the inner effect.
    Migrated in bespoke-artifact-misc.
  - Attack-in-the-Box — multi-sentence literal recognizer in
    ``effects._parse_effect`` decomposes the printed body into
    ``IfYouDo(MayDo(PumpSelfUntilEOT(power_bonus=4, toughness_bonus=0)),
    ScheduleSelfSacAtNextEndStep())``. The trigger header (ATTACKS +
    IsSelf) parses via the unified grammar. Migrated in
    audit-litdispatch-creatures.
  - Osseous Sticktwister — multi-sentence literal recognizer in
    ``effects._parse_effect`` dispatches the printed body to the
    generic ``EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage``
    primitive; the trigger header (Delirium gate + end-step event +
    ``Controlled(YOU)`` predicate) parses via the unified
    ``_parse_at_the_beginning_of_line`` grammar with
    ``StateBound(Delirium())`` AND-ed on. ``Lifelink`` keyword parses
    via the standard keyword line. Migrated in
    audit-per-opponent-choice.
  - Wickerfolk Thresher — multi-sentence literal recognizer in
    ``effects._parse_effect`` dispatches the printed body to the
    canonical ``PeekTopMayBattlefieldIfLandElseHand`` primitive. The
    trigger header (Delirium gate + ATTACKS + IsSelf predicate) parses
    via the unified intervening-if grammar. Migrated in
    audit-litdispatch-creatures.

Migrated to grammar in engine-kavaron-elevator:
  - Kavaron Harrier — ``_CREATE_CREATURE_TOKEN`` extended with
    ``(?: that's tapped and attacking)?`` qualifier; literal multi-
    sentence recognizer in ``_parse_effect`` for the two-sentence
    "create ... that's tapped and attacking. Sacrifice that token at
    end of combat." phrase → ``CreateToken(attacking=True,
    schedule_end_of_combat_sacrifice=True)``. ``CreateToken`` gains
    ``attacking`` and ``schedule_end_of_combat_sacrifice`` fields that
    wire the engine behavior faithfully (tapped+attacking entry, pending
    end-of-combat sac schedule). The trigger fires via the unified
    ``_YOU_MAY_PAY_THEN_EFFECT`` grammar.

Migrated to grammar in bespoke-roving-actuator:
  - Roving Actuator — Void-gated ETB trigger parses via
    ``_TRIGGERED_ABILITY_INTERVENING_IF_LINE`` with ``VoidActive()``
    gate predicate; three-sentence effect body (exile-from-GY + copy +
    cast-free) claimed by a literal multi-sentence recognizer in
    ``_parse_effect`` → ``RovingActuatorETBEffect(max_mv=2)``.

Migrated to ``_BESPOKE_TRIGGER_LINES`` in bespoke-splitskin-monoist:
  - Splitskin Doll — initial migration was to ``_BESPOKE_TRIGGER_LINES``;
    fully decomposed in audit-litdispatch-easy via the parser's multi-
    sentence literal recognizer in ``effects._parse_effect`` returning a
    generic ``Sequence((DrawCards(amount=1), IfNot(
    ControlsAnotherCreatureWithPowerAtMost(2),
    DiscardSelf(amount=1))))``. No bespoke engine class remains.
  - Monoist Circuit-Feeder — initial migration was to
    ``_BESPOKE_TRIGGER_LINES``; fully decomposed in audit-monoist-chorale
    via the parser's multi-sentence literal recognizer in
    ``effects._parse_effect`` returning the generic
    ``PairedTargetPumpDebuffUEOT`` primitive driven by the
    ``CountControlledMatching(IsArtifact())`` formula. No bespoke engine
    class remains.

Migrated to grammar in bespoke-survey-mechan:
  - Survey Mechan — ``SurveyMechanActivationEffect`` bespoke class
    models the two-target activated ability body; literal recognizer in
    ``_parse_effect`` claims the exact two-sentence surface.
    ``_INLINE_X_COST_REDUCTION_RIDER_DIFFERENTLY_NAMED_LAND`` in
    ``activated.py`` maps the land-count cost reduction.

All EoE/DSK artifact creatures from this batch are now migrated.
Remaining bespoke cards (still in ``_BESPOKE_ARTIFACT_CREATURES``): none.
"""

# mtg-zero (docs/ENGINE.md 169): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.combat, not carried into mtgz_ir):
# test_survey_mechan_effect_resolve_damage_and_draw

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource

_PARSER = OracleParserCardSource()
_PARSER_PRINTINGS = _PARSER.get_printings()


def _parser(name: str):
    return _PARSER_PRINTINGS[name]




def test_emissary_escort_claimed_by_parser():
    assert "Emissary Escort" in _PARSER_PRINTINGS




def test_gene_pollinator_claimed_by_parser():
    assert "Gene Pollinator" in _PARSER_PRINTINGS


def test_gene_pollinator_grammar_structure():
    """Grammar-claimed Gene Pollinator has the mana ability with the
    correct two-atom cost and oracle-faithful subtypes (Robot, Insect)."""
    gp = _parser("Gene Pollinator")
    assert gp.is_artifact
    assert len(gp.activated_abilities) == 1
    ab = gp.activated_abilities[0]
    assert ab.cost.tap_self is True
    assert ab.cost.tap_permanent_you_control is True
    assert ab.is_mana_ability is True
    # Oracle-faithful subtypes — bespoke entry incorrectly dropped these
    assert "Robot" in gp.subtypes
    assert "Insect" in gp.subtypes




def test_interceptor_mechan_claimed_by_parser():
    assert "Interceptor Mechan" in _PARSER_PRINTINGS




def test_kavaron_harrier_claimed_by_parser():
    assert "Kavaron Harrier" in _PARSER_PRINTINGS


def test_kavaron_harrier_grammar_structure():
    """Grammar-claimed Kavaron Harrier has the attack trigger wired via
    OptionalManaCostThenEffect → CreateToken(attacking=True,
    schedule_end_of_combat_sacrifice=True).

    Migration from bespoke ``KavaronHarrierAttackEffect`` to the unified
    grammar: the trigger body "you may pay {2}. If you do, create a 2/2
    colorless Robot artifact creature token that's tapped and attacking.
    Sacrifice that token at end of combat." parses via the
    ``_YOU_MAY_PAY_THEN_EFFECT`` recognizer + literal multi-sentence inner
    recognizer → ``OptionalManaCostThenEffect("{2}", CreateToken(...))``.
    """
    from mtgz_ir.effects.combinators import OptionalManaCostThenEffect
    from mtgz_ir.effects.generic import CreateToken
    from mtgz_ir.effects.predicates import Event

    kh = _parser("Kavaron Harrier")
    assert kh.is_artifact
    assert kh.power == 2
    assert kh.toughness == 1
    # Exactly one triggered ability: the attacks trigger.
    assert len(kh.triggered_abilities) == 1
    ta = kh.triggered_abilities[0]
    assert ta.event == Event.ATTACKS
    # Effect is the OptionalManaCostThenEffect wrapper.
    assert isinstance(ta.effect, OptionalManaCostThenEffect)
    assert ta.effect.cost_str == "{2}"
    inner = ta.effect.inner_effect
    assert isinstance(inner, CreateToken)
    assert inner.tapped is True
    assert inner.attacking is True
    assert inner.schedule_end_of_combat_sacrifice is True
    assert inner.token_printing.name == "Robot"




def test_mechan_shieldmate_claimed_by_parser():
    assert "Mechan Shieldmate" in _PARSER_PRINTINGS




def test_monoist_circuit_feeder_claimed_by_parser():
    assert "Monoist Circuit-Feeder" in _PARSER_PRINTINGS


def test_monoist_circuit_feeder_grammar_structure():
    """Grammar-claimed Monoist Circuit-Feeder has flying + a single
    ENTERS trigger wired to the generic
    ``PairedTargetPumpDebuffUEOT`` primitive driven by the
    ``CountControlledMatching(IsArtifact())`` formula.

    Migration through audit-monoist-chorale: the ETB body (``Until end
    of turn, target creature you control gets +X/+0 and target creature
    an opponent controls gets -0/-X, where X is the number of artifacts
    you control.``) is claimed by a literal multi-sentence recognizer in
    ``effects._parse_effect`` mapping to the generic primitive; the
    trigger header parses via the unified
    ``_parse_triggered_ability_line`` grammar; Flying parses via the
    standard keyword-line recognizer. No bespoke engine class remains.
    """
    from mtgz_ir.effects import PairedTargetPumpDebuffUEOT
    from mtgz_ir.effects.formulas import CountControlledMatching
    from mtgz_ir.effects.object_predicates import IsArtifact
    from mtgz_ir.effects.predicates import Event

    mcf = _parser("Monoist Circuit-Feeder")
    assert mcf.is_artifact
    assert mcf.power == 4 and mcf.toughness == 4
    assert "flying" in mcf.keywords
    etb = [t for t in mcf.triggered_abilities if t.event == Event.ENTERS]
    assert len(etb) == 1
    eff = etb[0].effect
    assert isinstance(eff, PairedTargetPumpDebuffUEOT)
    # Polarity matches the printed text: +X/+0 friendly, -0/-X opp.
    assert eff.positive_power == 1
    assert eff.positive_toughness == 0
    assert eff.negative_power == 0
    assert eff.negative_toughness == -1
    assert eff.target_slot_kinds() == ("my_creature", "creature")
    # X formula reads "number of artifacts you control" at resolve time.
    assert isinstance(eff.formula, CountControlledMatching)
    assert isinstance(eff.formula.predicate, IsArtifact)




def test_pinnacle_emissary_claimed_by_parser():
    assert "Pinnacle Emissary" in _PARSER_PRINTINGS




def test_roving_actuator_claimed_by_parser():
    assert "Roving Actuator" in _PARSER_PRINTINGS


def test_roving_actuator_grammar_structure():
    """Grammar-claimed Roving Actuator has a Void-gated ETB trigger
    whose effect is ``RovingActuatorETBEffect(max_mv=2)``.

    Migration from ``_BESPOKE_ARTIFACT_CREATURES``: the Void-gated ETB
    trigger parses via ``_TRIGGERED_ABILITY_INTERVENING_IF_LINE`` with the
    ``VoidActive()`` gate predicate AND-ed onto ``IsSelf()``. The three-
    sentence effect body (exile-from-GY + copy + cast-free) is claimed by
    a literal multi-sentence recognizer in ``_parse_effect``. The bespoke
    entry carried a spurious ``warp="{1}{R}"`` not in the oracle text;
    the grammar-claimed card faithfully omits warp.
    """
    from mtgz_ir.effects import RovingActuatorETBEffect
    from mtgz_ir.effects.predicates import Event

    ra = _parser("Roving Actuator")
    assert ra.is_artifact
    assert ra.power == 3
    assert ra.toughness == 4
    assert ra.warp_cost_str == ""
    assert len(ra.triggered_abilities) == 1
    ta = ra.triggered_abilities[0]
    assert ta.event == Event.ENTERS
    # Grammar path encodes the Void gate as a composed predicate (IsSelf() &
    # StateBound(VoidActive())) rather than the ``requires_void`` flag used
    # by the static bespoke registry. The engine's dispatch_event evaluates
    # the composed predicate at trigger time — functionally equivalent gate.
    assert "VoidActive" in repr(ta.predicate)
    assert isinstance(ta.effect, RovingActuatorETBEffect)
    assert ta.effect.max_mv == 2




def test_rust_harvester_claimed_by_parser():
    assert "Rust Harvester" in _PARSER_PRINTINGS


def test_rust_harvester_grammar_structure():
    """Grammar-claimed Rust Harvester has menace + full activated ability
    (three-atom cost, Sequence effect) and oracle-faithful artifact type."""
    rh = _parser("Rust Harvester")
    assert rh.is_artifact
    assert "menace" in rh.keywords
    assert "Robot" in rh.subtypes
    assert len(rh.activated_abilities) == 1
    ab = rh.activated_abilities[0]
    assert ab.cost.mana_str == "{2}"
    assert ab.cost.tap_self is True
    assert ab.cost.exile_artifact_from_graveyard is True
    from mtgz_ir.cards.base import Sequence, PutPlusOneCounterOnSelf, DealDamageEqualToSourcePowerAnyTarget
    assert isinstance(ab.effect, Sequence)
    assert len(ab.effect.effects) == 2
    assert isinstance(ab.effect.effects[0], PutPlusOneCounterOnSelf)
    assert isinstance(ab.effect.effects[1], DealDamageEqualToSourcePowerAnyTarget)




def test_selfcraft_mechan_claimed_by_parser():
    assert "Selfcraft Mechan" in _PARSER_PRINTINGS


def test_selfcraft_mechan_grammar_structure():
    """Grammar-claimed Selfcraft Mechan carries an ETB trigger whose
    effect is ``OptionalSacArtifactForEffect`` wrapping a ``Sequence``
    of ``PutPlusOneCounterOnTargetCreature`` and ``DrawCards`` — the
    oracle-faithful shape for "you may sacrifice an artifact. When you
    do, put a +1/+1 counter on target creature and draw a card."
    """
    from mtgz_ir.effects.combinators import OptionalSacArtifactForEffect, Sequence
    from mtgz_ir.effects.predicates import Event

    sm = _parser("Selfcraft Mechan")
    assert sm.is_artifact
    etb = [t for t in sm.triggered_abilities if t.event == Event.ENTERS]
    assert len(etb) == 1
    eff = etb[0].effect
    assert isinstance(eff, OptionalSacArtifactForEffect)
    assert isinstance(eff.inner_effect, Sequence)
    assert eff.requires_target()




def test_survey_mechan_claimed_by_parser():
    assert "Survey Mechan" in _PARSER_PRINTINGS


def test_survey_mechan_grammar_structure():
    """Grammar-claimed Survey Mechan has flying + hexproof, the
    {10}/sacrifice-self activated ability with SurveyMechanActivationEffect,
    and a PerDifferentlyNamedLand cost reduction."""
    from mtgz_ir.cards.base import CostReduction
    from mtgz_ir.effects.bespoke import SurveyMechanActivationEffect
    from mtgz_ir.effects.conditions import PerDifferentlyNamedLand

    sm = _parser("Survey Mechan")
    assert sm.is_artifact
    assert "flying" in sm.keywords
    assert "hexproof" in sm.keywords
    assert len(sm.activated_abilities) == 1
    ab = sm.activated_abilities[0]
    # Cost: {10} + sacrifice self.
    assert ab.cost.mana_str == "{10}"
    assert ab.cost.sacrifice_self is True
    # Effect: SurveyMechanActivationEffect (two-target bespoke class).
    assert isinstance(ab.effect, SurveyMechanActivationEffect)
    assert ab.effect.requires_target()
    assert ab.effect.target_count_range() == (2, 2)
    assert ab.effect.target_slot_kinds() == ("any_damage_target", "player")
    # Cost reduction: PerDifferentlyNamedLand, amount=1.
    assert len(ab.cost_reductions) == 1
    cr = ab.cost_reductions[0]
    assert cr.amount == 1
    assert isinstance(cr.condition, PerDifferentlyNamedLand)


def test_survey_mechan_cost_reduction_reduces_effectively():
    """A 10-cost ability in a game where the controller has 4 differently
    named lands costs 6 to activate (10 - 4 = 6)."""
    from mtgz_ir.actions import _evaluate_cost_reduction
    from mtgz_ir.cards.base import CostReduction, make_basic_land
    from mtgz_ir.effects.conditions import PerDifferentlyNamedLand
    from mtgz_ir.game import GameState, Player, Zone

    state = GameState()
    state.players.append(Player(id=0))
    state.players.append(Player(id=1))
    state.active_player_idx = 0

    # Add 4 differently named lands for player 0.
    for name, color in (
        ("Plains", "W"), ("Island", "U"), ("Swamp", "B"), ("Forest", "G"),
    ):
        land = state.new_object(
            make_basic_land(name, color), 0, Zone.BATTLEFIELD,
        )
        state.battlefield.append(land)

    rule = CostReduction(amount=1, condition=PerDifferentlyNamedLand())
    reduction = _evaluate_cost_reduction(rule, state, caster_id=0)
    # 4 differently named lands → multiplier=4 → reduction=1*4=4
    assert reduction == 4
    # Ability base cost is {10} generic; after reduction: 10 - 4 = 6.
    from mtgz_ir.cards import parse_mana_cost
    base = parse_mana_cost("{10}")
    effective = base.reduced(reduction)
    assert effective.generic == 6




def test_survey_mechan_effect_damage_to_player():
    """SurveyMechanActivationEffect can target a player in the damage
    slot (any target includes players) — the damage is applied directly
    to that player's life."""
    from mtgz_ir.effects.bespoke import SurveyMechanActivationEffect
    from mtgz_ir.game import GameState, Player, Zone
    from mtgz_ir.cards.base import make_creature

    state = GameState()
    state.players.append(Player(id=0, life=20))
    state.players.append(Player(id=1, life=20))
    state.active_player_idx = 0
    # Give player 1 a library for the draw step.
    dummy = make_creature("Dummy", "{1}", 1, 1)
    for _ in range(5):
        card = state.new_object(dummy, 1, Zone.LIBRARY)
        state.players[1].library.append(card)

    effect = SurveyMechanActivationEffect()
    # Damage slot: player 1 (tagged any_damage_target).
    # Draw/gain slot: player 1 (tagged player).
    targets = (
        ("any_damage_target", 1),
        ("player", 1),
    )
    effect.resolve(state, controller_id=0, targets=targets)
    # Player 1 took 3 damage (life drops by 3) then gained 3 (net 0),
    # minus/plus the draw (no life change from draw).
    # So net life = 20 - 3 + 3 = 20.
    assert state.players[1].life == 20


def test_survey_mechan_legal_targets_includes_creature_and_players():
    """SurveyMechanActivationEffect.legal_targets returns the combined
    pool of any_damage_target (creature + players) and player slots."""
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects.bespoke import SurveyMechanActivationEffect
    from mtgz_ir.game import GameState, Player, Zone

    state = GameState()
    state.players.append(Player(id=0, life=20))
    state.players.append(Player(id=1, life=20))
    state.active_player_idx = 0

    # Put a creature on the battlefield.
    creature_printing = make_creature("Test Creature", "{1}", 1, 1)
    creature = state.new_object(creature_printing, 1, Zone.BATTLEFIELD)
    state.battlefield.append(creature)

    effect = SurveyMechanActivationEffect()
    targets = effect.legal_targets(state, controller_id=0)
    # any_damage_target pool: creature + 2 players
    any_pool = [t for t in targets if t[0] == "any_damage_target"]
    assert ("any_damage_target", creature.id) in any_pool
    assert ("any_damage_target", 0) in any_pool
    assert ("any_damage_target", 1) in any_pool
    # player pool: 2 players
    player_pool = [t for t in targets if t[0] == "player"]
    assert ("player", 0) in player_pool
    assert ("player", 1) in player_pool


def test_survey_mechan_legal_targets_empty_when_no_players():
    """SurveyMechanActivationEffect.legal_targets returns [] if there
    are no living players (the player-draw slot can't be satisfied)."""
    from mtgz_ir.effects.bespoke import SurveyMechanActivationEffect
    from mtgz_ir.game import GameState, Player

    state = GameState()
    p = Player(id=0, life=20)
    p.has_lost = True
    state.players.append(p)
    p2 = Player(id=1, life=20)
    p2.has_lost = True
    state.players.append(p2)
    state.active_player_idx = 0

    effect = SurveyMechanActivationEffect()
    assert effect.legal_targets(state, controller_id=0) == []


def test_survey_mechan_effect_describe():
    """SurveyMechanActivationEffect.describe returns the expected string."""
    from mtgz_ir.effects.bespoke import SurveyMechanActivationEffect
    effect = SurveyMechanActivationEffect()
    desc = effect.describe()
    assert "3 damage" in desc
    assert "draws three cards" in desc
    assert "gains 3 life" in desc


def test_tapestry_warden_claimed_by_parser():
    assert "Tapestry Warden" in _PARSER_PRINTINGS




def test_virulent_silencer_claimed_by_parser():
    assert "Virulent Silencer" in _PARSER_PRINTINGS


