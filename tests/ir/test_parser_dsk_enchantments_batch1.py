"""Tests for the Duskmourn enchantments batch-1 parser additions.

Each commit in this batch lands one DSK enchantment via name-keyed
dispatch on the existing ``enchantment_builder`` table.

Cards newly claimed in this batch:

* **Shardmage's Rescue** ``{W}`` Enchantment — Aura. Flash +
  Enchant-creature-you-control + ``Enchanted creature gets +1/+1``.
  The conditional ``As long as this Aura entered this turn, enchanted
  creature has hexproof`` line is dropped as an honest no-op:
  hexproof is engine-unenforced metadata in this codebase (the
  rooms-batch6 Lecture Hall precedent), so a conditional grant is
  identically a no-op.

* **Cracked Skull** ``{2}{B}`` Enchantment — Aura. ETB look-at-target-
  opp-hand may-discard + DEALT_DAMAGE-destroys-enchanted. The new
  ``TargetOpponentLookHandMayDiscard`` engine class honours the
  printed ``You may`` gate via ``Agent.choose_yes_no`` rather than
  forcing the discard the way ``TargetOpponentDiscardsCasterChoice``
  does. ``target player`` is narrowed to ``target opponent`` per the
  standard EoE / DSK simplification — self-targeting is never
  strategically correct for the controller. The destroys-host
  trigger rides the same shape Cryoshatter's bottom trigger uses.

* **Sporogenic Infection** ``{1}{B}`` Enchantment — Aura. ETB target-
  opp-sacs-creature-other-than-enchanted + DEALT_DAMAGE-destroys-
  enchanted. The new
  ``TargetOpponentSacrificesCreatureOtherThanEnchanted`` engine
  class identifies the host via ``state.resolving_source.attached_to``
  and excludes it from the target's sac options. ``target player``
  narrows to ``target opponent`` per the same simplification Cracked
  Skull rides.

* **Leyline of Mutation** ``{2}{G}{G}`` Enchantment. Honest no-op
  claim — both printed lines (the opening-hand-replacement Leyline
  mechanic and the ``You may pay {W}{U}{B}{R}{G} rather than pay
  the mana cost`` alt cost) are inert in this engine today. Cost-
  and-name-only printing; same permission-not-offered shape Lecture
  Hall ships per dsk-rooms-batch6.

* **Stay Hidden, Stay Silent** ``{1}{U}`` Enchantment — Aura. ETB-
  tap-enchanted via the new ``TapEnchantedPermanent`` engine class
  + ``attached_prevents_untap=True`` static (Tractor Beam
  precedent). The ``{4}{U}{U}: Shuffle enchanted creature into its
  owner's library, then manifest dread. Activate only as a
  sorcery.`` activated ability is dropped as honest no-op — the
  activation is optional (controller's choice to pay), so not
  offering it through ``legal`` is the unoffered-alt-cost shape.
"""

# mtg-zero (docs/ENGINE.md 170): 2 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.player_protections, not carried into mtgz_ir):
# test_may_discard_yes_no_yes_path_discards_chosen_card, test_sac_other_than_enchanted_excludes_host_from_options

from __future__ import annotations

from mtgz_ir.cards.parser_source import OracleParserCardSource


def _claimed_names() -> tuple[str, ...]:
    return OracleParserCardSource().claimed_names


# ---------- Shardmage's Rescue ----------


def test_shardmages_rescue_claimed_by_parser():
    # Shardmage's Rescue ``{W}`` Enchantment — Aura — claimed via the
    # name-keyed ``_ENCHANTMENT_BUILDERS`` dispatch in
    # ``parser.enchantment_builder``.
    assert "Shardmage's Rescue" in _claimed_names()


def test_shardmages_rescue_carries_flash_and_yours_only():
    # The produced ``CardPrinting`` carries:
    # - Flash on the Aura itself (``keywords`` contains "flash"),
    #   gating instant-speed casting via the same ``legal.py`` check
    #   Frantic Strength rides;
    # - ``aura_yours_only=True`` for the printed
    #   ``Enchant creature you control`` line — same flag Sheltered by
    #   Ghosts already ships.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Shardmage's Rescue"]
    assert "flash" in p.keywords
    assert p.aura_yours_only is True


def test_shardmages_rescue_grants_plus_one_plus_one():
    # ``Enchanted creature gets +1/+1`` rides the shared
    # ``equipment_bonuses`` machinery (the (power, toughness, keywords)
    # tuple ``make_aura`` materializes from its kwargs). The granted
    # keyword set is empty — the printed conditional hexproof line is
    # dropped as an honest no-op since hexproof is engine-unenforced
    # metadata in this codebase.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Shardmage's Rescue"]
    power_bonus, toughness_bonus, granted = p.equipment_bonuses
    assert power_bonus == 1
    assert toughness_bonus == 1
    assert granted == frozenset()


def test_shardmages_rescue_has_no_etb_trigger():
    # No ETB trigger is wired — the printed text only carries the
    # static rider and the conditional (dropped) hexproof grant. A
    # spurious trigger would be a regression; assert empty.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Shardmage's Rescue"]
    assert p.triggered_abilities == ()


def test_shardmages_rescue_is_aura_with_correct_cost_and_type():
    # Cost is ``{W}`` (single white). Type line ``Enchantment — Aura``;
    # ``aura_enchant_type`` defaults to ``"creature"`` (the printed
    # ``Enchant creature you control`` restriction is the
    # ``aura_yours_only`` flag, not a type change).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Shardmage's Rescue"]
    assert p.mana_cost_str == "{W}"
    assert p.is_aura is True
    assert p.aura_enchant_type == "creature"


# ---------- Cracked Skull ----------


def test_cracked_skull_claimed_by_parser():
    # Cracked Skull ``{2}{B}`` Enchantment — Aura — claimed via the
    # name-keyed ``_ENCHANTMENT_BUILDERS`` dispatch.
    assert "Cracked Skull" in _claimed_names()


def test_cracked_skull_carries_etb_and_dealt_damage_triggers():
    # The produced ``CardPrinting`` carries two triggers, in printed
    # order:
    # 1. ETB-look-may-discard via ``TargetOpponentLookHandMayDiscard``
    # 2. DEALT_DAMAGE-destroys-host via ``DestroyEnchantedPermanent``
    from mtgz_ir.effects import (
        DestroyEnchantedPermanent, TargetOpponentLookHandMayDiscard,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Cracked Skull"]
    assert len(p.triggered_abilities) == 2
    etb, dealt = p.triggered_abilities
    assert isinstance(etb.effect, TargetOpponentLookHandMayDiscard)
    assert isinstance(dealt.effect, DestroyEnchantedPermanent)


def test_cracked_skull_is_aura_with_correct_cost():
    # Cost is ``{2}{B}``. ``aura_enchant_type`` defaults to
    # ``"creature"`` (printed ``Enchant creature``, not the yours-only
    # variant).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Cracked Skull"]
    assert p.mana_cost_str == "{2}{B}"
    assert p.is_aura is True
    assert p.aura_enchant_type == "creature"
    assert p.aura_yours_only is False


# ---------- TargetOpponentLookHandMayDiscard engine class ----------


def test_may_discard_legal_targets_is_opponents_only():
    # The simplified ``target player`` -> ``target opponent`` legality
    # narrowing means the caster is never offered themselves as a
    # target. Two-player smoke test: P0 controls; only P1 should be
    # listed.
    from mtgz_ir.effects import TargetOpponentLookHandMayDiscard
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    eff = TargetOpponentLookHandMayDiscard()
    legal = eff.legal_targets(state, controller_id=0)
    assert ("player", 1) in legal
    assert ("player", 0) not in legal
    assert eff.requires_target() is True




def test_may_discard_yes_no_no_path_keeps_hand_intact():
    # The caster declines via ``choose_yes_no`` returning False; no
    # discard happens. Honors the printed ``You may`` gate.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import TargetOpponentLookHandMayDiscard
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear_card = GameObject(
        id=10, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.HAND,
    )
    p0 = Player(id=0)
    p1 = Player(id=1, hand=[bear_card])

    class _NoAgent:
        def choose_yes_no(self, state, player_id, prompt):
            return False

        def choose_card_from_opponent_hand(
            self, state, player_id, opponent_hand_ids,
        ):
            # Should never be reached when the may-gate is declined.
            raise AssertionError(
                "choose_card_from_opponent_hand called after decline"
            )

    p0.agent = _NoAgent()
    state = GameState(players=[p0, p1])
    eff = TargetOpponentLookHandMayDiscard()
    eff.resolve(state, controller_id=0, targets=(("player", 1),))
    # Hand unchanged.
    assert bear_card in p1.hand
    assert p1.graveyard == []


def test_may_discard_no_nonland_options_silently_no_ops():
    # If the target's hand has no nonland card, the directed discard
    # silently no-ops — the may-gate is never even prompted.
    from mtgz_ir.cards.lands import BASIC_LANDS
    from mtgz_ir.effects import TargetOpponentLookHandMayDiscard
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    forest_pr = BASIC_LANDS["Forest"]
    forest_card = GameObject(
        id=11, printing=forest_pr, owner_id=1, controller_id=1,
        zone=Zone.HAND,
    )
    p0 = Player(id=0)
    p1 = Player(id=1, hand=[forest_card])

    class _AssertAgent:
        def choose_yes_no(self, state, player_id, prompt):
            raise AssertionError("may-gate prompted with no options")

        def choose_card_from_opponent_hand(
            self, state, player_id, opponent_hand_ids,
        ):
            raise AssertionError("never called")

    p0.agent = _AssertAgent()
    state = GameState(players=[p0, p1])
    eff = TargetOpponentLookHandMayDiscard()
    eff.resolve(state, controller_id=0, targets=(("player", 1),))
    assert forest_card in p1.hand
    assert p1.graveyard == []


def test_may_discard_no_agent_silently_declines():
    # When the caster has no agent, the directed discard silently
    # declines — the engine never auto-discards on the caster's
    # behalf.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import TargetOpponentLookHandMayDiscard
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear_card = GameObject(
        id=10, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.HAND,
    )
    p0 = Player(id=0)  # no agent
    p1 = Player(id=1, hand=[bear_card])
    state = GameState(players=[p0, p1])
    eff = TargetOpponentLookHandMayDiscard()
    eff.resolve(state, controller_id=0, targets=(("player", 1),))
    assert bear_card in p1.hand
    assert p1.graveyard == []


def test_may_discard_describe_mentions_may_and_nonland():
    from mtgz_ir.effects import TargetOpponentLookHandMayDiscard

    desc = TargetOpponentLookHandMayDiscard().describe()
    assert "may" in desc
    assert "nonland" in desc


# ---------- Sporogenic Infection ----------


def test_sporogenic_infection_claimed_by_parser():
    # Sporogenic Infection ``{1}{B}`` Enchantment — Aura — claimed via
    # the name-keyed ``_ENCHANTMENT_BUILDERS`` dispatch.
    assert "Sporogenic Infection" in _claimed_names()


def test_sporogenic_infection_carries_etb_and_dealt_damage_triggers():
    # The produced ``CardPrinting`` carries two triggers, in printed
    # order:
    # 1. ETB target-opp-sacs-creature-other-than-enchanted via the
    #    new ``TargetOpponentSacrificesCreatureOtherThanEnchanted``
    #    engine class.
    # 2. DEALT_DAMAGE-destroys-host via ``DestroyEnchantedPermanent``.
    from mtgz_ir.effects import (
        DestroyEnchantedPermanent,
        TargetOpponentSacrificesCreatureOtherThanEnchanted,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Sporogenic Infection"]
    assert len(p.triggered_abilities) == 2
    etb, dealt = p.triggered_abilities
    assert isinstance(
        etb.effect,
        TargetOpponentSacrificesCreatureOtherThanEnchanted,
    )
    assert isinstance(dealt.effect, DestroyEnchantedPermanent)


def test_sporogenic_infection_is_aura_with_correct_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Sporogenic Infection"]
    assert p.mana_cost_str == "{1}{B}"
    assert p.is_aura is True
    assert p.aura_enchant_type == "creature"
    # Printed ``Enchant creature`` (any creature), not the yours-only
    # restriction. Self-targeting at cast-time pays for the printed
    # text; the ETB's target narrowing is a separate axis.
    assert p.aura_yours_only is False


# ---------- TargetOpponentSacrificesCreatureOtherThanEnchanted ----------


def test_sac_other_than_enchanted_legal_targets_is_opps_only():
    from mtgz_ir.effects import (
        TargetOpponentSacrificesCreatureOtherThanEnchanted,
    )
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    eff = TargetOpponentSacrificesCreatureOtherThanEnchanted()
    legal = eff.legal_targets(state, controller_id=0)
    assert ("player", 1) in legal
    assert ("player", 0) not in legal
    assert eff.requires_target() is True




def test_sac_other_than_enchanted_no_legal_silently_no_ops():
    # When the target opp's only creature is the enchanted creature,
    # there are no legal sac options — the trigger silently no-ops.
    from mtgz_ir.cards.base import make_aura, make_creature
    from mtgz_ir.effects import (
        TargetOpponentSacrificesCreatureOtherThanEnchanted,
    )
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    aura_pr = make_aura("TestSac", "{1}{B}")
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    host = GameObject(
        id=10, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    aura = GameObject(
        id=20, printing=aura_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    aura.attached_to = host.id

    class _AssertAgent:
        def choose_sacrifice(self, state, player_id, options):
            raise AssertionError("should not be prompted with no options")

    p0 = Player(id=0)
    p1 = Player(id=1)
    p1.agent = _AssertAgent()
    state = GameState(
        players=[p0, p1],
        battlefield=[host, aura],
        resolving_source=aura,
    )
    eff = TargetOpponentSacrificesCreatureOtherThanEnchanted()
    eff.resolve(state, controller_id=0, targets=(("player", 1),))
    # Host survives.
    assert host in state.battlefield


def test_sac_other_than_enchanted_describe():
    from mtgz_ir.effects import (
        TargetOpponentSacrificesCreatureOtherThanEnchanted,
    )

    desc = TargetOpponentSacrificesCreatureOtherThanEnchanted().describe()
    assert "sacrif" in desc
    assert "other than enchanted" in desc


# ---------- Leyline of Mutation ----------


def test_leyline_of_mutation_claimed_by_parser():
    # Leyline of Mutation ``{2}{G}{G}`` — claimed via the name-keyed
    # ``_ENCHANTMENT_BUILDERS`` dispatch.
    assert "Leyline of Mutation" in _claimed_names()


def test_stay_hidden_stay_silent_claimed_by_parser():
    # Stay Hidden, Stay Silent ``{1}{U}`` Enchantment — Aura — claimed
    # via the name-keyed ``_ENCHANTMENT_BUILDERS`` dispatch.
    assert "Stay Hidden, Stay Silent" in _claimed_names()


def test_stay_hidden_stay_silent_carries_etb_tap_and_static():
    # The produced ``CardPrinting`` carries:
    # - one ETB trigger (``TapEnchantedPermanent``);
    # - the printing-level ``attached_prevents_untap`` flag for the
    #   printed ``doesn't untap during its controller's untap step``
    #   static rider — same flag Tractor Beam ships.
    # - no activated abilities (dropped as honest no-op for the
    #   ``{4}{U}{U}: Shuffle ... Activate only as a sorcery.`` line).
    from mtgz_ir.effects import TapEnchantedPermanent

    parser = OracleParserCardSource()
    p = parser.get_printings()["Stay Hidden, Stay Silent"]
    assert len(p.triggered_abilities) == 1
    assert isinstance(p.triggered_abilities[0].effect, TapEnchantedPermanent)
    assert p.attached_prevents_untap is True
    assert p.activated_abilities == ()


def test_stay_hidden_stay_silent_is_aura_with_correct_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Stay Hidden, Stay Silent"]
    assert p.mana_cost_str == "{1}{U}"
    assert p.is_aura is True


# ---------- TapEnchantedPermanent ----------


def test_tap_enchanted_permanent_taps_host():
    # End-to-end resolve sanity: the host is currently untapped; after
    # the effect resolves it's tapped.
    from mtgz_ir.cards.base import make_aura, make_creature
    from mtgz_ir.effects import TapEnchantedPermanent
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    aura_pr = make_aura("Test Aura", "{U}")
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    host = GameObject(
        id=10, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    aura = GameObject(
        id=20, printing=aura_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    aura.attached_to = host.id
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[host, aura],
        resolving_source=aura,
    )
    eff = TapEnchantedPermanent()
    eff.resolve(state, controller_id=0)
    assert host.tapped is True


def test_tap_enchanted_permanent_idempotent_when_already_tapped():
    # Already-tapped host: no double-tap event, no log spam, silent
    # no-op. The ETB happily resolves.
    from mtgz_ir.cards.base import make_aura, make_creature
    from mtgz_ir.effects import TapEnchantedPermanent
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    aura_pr = make_aura("Test Aura", "{U}")
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    host = GameObject(
        id=10, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD, tapped=True,
    )
    aura = GameObject(
        id=20, printing=aura_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    aura.attached_to = host.id
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[host, aura],
        resolving_source=aura,
    )
    eff = TapEnchantedPermanent()
    eff.resolve(state, controller_id=0)
    # Still tapped, no double-tap log line.
    assert host.tapped is True


def test_tap_enchanted_permanent_no_host_silent_no_op():
    # If the source isn't attached (somehow lost its host between
    # trigger and resolve), the effect silently no-ops.
    from mtgz_ir.cards.base import make_aura
    from mtgz_ir.effects import TapEnchantedPermanent
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    aura_pr = make_aura("Test Aura", "{U}")
    aura = GameObject(
        id=20, printing=aura_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    # No attached_to set.
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[aura],
        resolving_source=aura,
    )
    eff = TapEnchantedPermanent()
    eff.resolve(state, controller_id=0)
    # Nothing to assert beyond not raising.


def test_tap_enchanted_permanent_describe():
    from mtgz_ir.effects import TapEnchantedPermanent

    desc = TapEnchantedPermanent().describe()
    assert "tap" in desc
    assert "enchanted" in desc


def test_leyline_of_mutation_is_honest_no_op_static_enchantment():
    # Both printed lines are inert in this engine: the opening-hand
    # Leyline replacement isn't modeled, and the ``You may pay
    # {W}{U}{B}{R}{G} rather than pay the mana cost`` alt cost isn't
    # offered by ``legal.py``. The printing therefore carries no
    # triggers, no static abilities, no activated abilities — same
    # honest-no-op shape Lecture Hall ships per dsk-rooms-batch6.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Leyline of Mutation"]
    assert p.mana_cost_str == "{2}{G}{G}"
    assert "Enchantment" in p.card_types
    assert p.is_aura is False
    assert p.triggered_abilities == ()
    assert p.static_abilities == ()
    assert p.activated_abilities == ()
