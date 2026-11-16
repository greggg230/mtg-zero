"""Tests for the Duskmourn spells / enchantments / equipment batch-2
parser additions.

This batch claims more unclaimed Duskmourn cards by composing existing
engine classes (with one targeted widening of ``DestroyTargetCreature``
for the ``or_enchantment`` flag) and minor grammar additions. Each new
primitive is reusable across cards and across sets, so the regex
additions in ``parser.effects`` aren't DSK-specific even though every
card claimed by this batch ships in Duskmourn.

New engine-class field:

* ``DestroyTargetCreature.or_enchantment`` — when True, widens the
  legal-target pool to also include any enchantment (Aura subtype or
  not), mirroring the existing ``or_spacecraft`` flag's shape. The
  enchantment branch emits ``("enchantment", id)`` target tuples; the
  resolve accepts either kind and finds the object by id.

New effect-grammar primitives (``parser.effects``):

* ``destroy target creature or enchantment`` →
  ``DestroyTargetCreature(or_enchantment=True)``. Mirrors the existing
  ``destroy target creature or Spacecraft`` shape but for the
  enchantment widening.

Cards newly claimed:

* **Withering Torment** ``{2}{B}`` Instant — ``Destroy target creature
  or enchantment. You lose 2 life.`` Two-sentence body parses through
  the existing Sequence combinator path (sentence-split) once the new
  destroy-target-creature-or-enchantment primitive lands.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DestroyTargetCreature,
    DrawCards,
    ExileTargetCreature,
    LoseLife,
    Sequence,
)
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.conditions import TargetMatches
from mtgz_ir.effects.generic import CreateToken, GrantUnblockableUEOT
from mtgz_ir.effects.object_predicates import IsCreature, IsTapped


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- destroy-target-creature-or-enchantment primitive ----------

def test_parse_destroy_target_creature_or_enchantment():
    # New primitive: widens DestroyTargetCreature's target pool to
    # also include any enchantment (Aura or not). Engine flag
    # parallels the existing ``or_spacecraft``.
    eff = _parse_effect("destroy target creature or enchantment")
    assert eff == DestroyTargetCreature(or_enchantment=True)


def test_destroy_target_creature_or_enchantment_legal_targets_widening():
    # Belt-and-suspenders that the engine flag actually widens the
    # legal-target pool. With ``or_enchantment=True``, an enchantment
    # on the battlefield should be a legal target alongside any
    # creature.
    from mtgz_ir.cards.base import (
        make_creature,
        make_enchantment,
    )
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    creature_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    ench_pr = make_enchantment("Tome", "{1}{U}")
    creature_obj = GameObject(
        id=1, printing=creature_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    ench_obj = GameObject(
        id=2, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[creature_obj, ench_obj],
    )
    eff = DestroyTargetCreature(or_enchantment=True)
    legal = eff.legal_targets(state, controller_id=0)
    # Both should be legal. After the unified ``Destroy`` migration,
    # ``Effect._enumerate_for_filter`` tags non-creature non-
    # planeswalker non-battle objects as the generic ``"permanent"``
    # kind — the resolve no longer dispatches on kind, just looks up
    # the object by id, so the tag flattening is safe.
    assert ("creature", 1) in legal
    assert ("permanent", 2) in legal


def test_destroy_target_creature_default_excludes_enchantment():
    # Without ``or_enchantment``, the bare ``destroy target creature``
    # primitive must NOT pick up enchantments — the widening only
    # applies when the flag is set.
    from mtgz_ir.cards.base import make_enchantment
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    ench_pr = make_enchantment("Tome", "{1}{U}")
    ench_obj = GameObject(
        id=2, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[ench_obj],
    )
    eff = DestroyTargetCreature()
    assert eff.legal_targets(state, controller_id=0) == []


def test_destroy_target_creature_describe_includes_enchantment_suffix():
    # The ``describe`` helper should reflect the new flag in its
    # English-ish output, mirroring the ``or_spacecraft`` suffix.
    eff = DestroyTargetCreature(or_enchantment=True)
    assert "enchantment" in eff.describe()


# ---------- Withering Torment claim ----------

def test_withering_torment_claimed_by_parser():
    # Withering Torment ``{2}{B}`` Instant — two-sentence body that
    # combines the new destroy-target-creature-or-enchantment
    # primitive with the existing ``you lose 2 life`` primitive,
    # composed via Sequence by the spell-effect parser's sentence-
    # split path.
    assert "Withering Torment" in _claimed_names()


def test_withering_torment_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the right Sequence shape (destroy with or_enchantment, then
    # lose 2 life).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Withering Torment"]
    assert p.spell_effect == Sequence(effects=(
        DestroyTargetCreature(or_enchantment=True),
        LoseLife(amount=2),
    ))


# ---------- draw N cards (spelled-out cardinal) ----------

def test_parse_draw_two_cards():
    # Extension of ``_DRAW_N_CARDS`` to accept spelled-out small
    # cardinals — Glimmerburst's printed ``Draw two cards.`` prefix
    # needed this. Existing digit form must still work.
    assert _parse_effect("draw two cards") == DrawCards(amount=2)


def test_parse_draw_three_cards_spelled():
    # Forward-looking grammar coverage — no DSK card prints
    # ``draw three cards`` today, but the regex shape allows it
    # without further grammar work.
    assert _parse_effect("draw three cards") == DrawCards(amount=3)


def test_parse_draw_n_cards_digit_form_still_works():
    # The pre-existing digit form must continue to parse to the same
    # engine class.
    assert _parse_effect("draw 2 cards") == DrawCards(amount=2)


def test_parse_draw_unknown_word_returns_none():
    # An unrecognized word in the cardinal slot must not parse as a
    # draw effect (the regex's whitelist of small cardinals enforces
    # this — anything outside the table fails the alternation).
    assert _parse_effect("draw eleventy cards") is None


# ---------- Glimmerburst claim ----------

def test_glimmerburst_claimed_by_parser():
    # Glimmerburst ``{2}{W}{U}`` Instant — two-sentence body with
    # ``Draw two cards.`` + ``Create a 1/1 white Glimmer enchantment
    # creature token.`` Both pieces parse via existing primitives
    # (with the new draw-N-cards-spelled-out cardinal extension);
    # composed via Sequence by the spell-effect parser.
    assert "Glimmerburst" in _claimed_names()


def test_glimmerburst_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the right Sequence shape (draw 2, then mint Glimmer token).
    parser = OracleParserCardSource()
    from dataclasses import replace
    p = parser.get_printings()["Glimmerburst"]
    # The printed "white Glimmer" colour is the token's colour indicator.
    assert p.spell_effect == Sequence(effects=(
        DrawCards(amount=2),
        CreateToken(token_printing=replace(TOKEN_PRINTINGS["Glimmer"], color_indicator=frozenset({"W"})), count=1),
    ))


# ---------- Final Vengeance: additional-cost preamble flag ----------

def test_final_vengeance_claimed_by_parser():
    # Final Vengeance ``{1}{B}{B}`` Sorcery — additional-cost-sacrifice
    # preamble (creature-or-enchantment variant) + ``Exile target
    # creature.`` Claimed via the new printing flag on the additional-
    # cost preamble dispatcher; base effect is the existing
    # ``ExileTargetCreature``.
    assert "Final Vengeance" in _claimed_names()


def test_final_vengeance_carries_additional_cost_flag():
    # The additional-cost-sacrifice-creature-or-enchantment flag must
    # be threaded onto the ``CardPrinting`` so the engine's cast-time
    # cost-paying path knows to demand the sacrifice. The base spell
    # effect is the bare ``ExileTargetCreature`` — the additional-cost
    # preamble itself doesn't contribute an effect.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Final Vengeance"]
    assert p.additional_cost_sacrifice_creature_or_enchantment is True
    assert p.spell_effect == ExileTargetCreature()


def test_final_vengeance_additional_cost_gates_legal_actions():
    # Belt-and-suspenders that the new printing flag actually gates
    # casting at legal-actions time — without a creature OR enchantment
    # to sacrifice, the spell is not castable.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone
    from mtgz_ir.legal import _additional_costs_payable

    parser = OracleParserCardSource()
    final_vengeance = parser.get_printings()["Final Vengeance"]
    # Build a minimal hand-card and battlefield. The card itself is
    # in P0's hand; the battlefield is empty so the cost is unpayable.
    card_obj = GameObject(
        id=10, printing=final_vengeance, owner_id=0, controller_id=0,
        zone=Zone.HAND,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[],
    )
    assert _additional_costs_payable(state, 0, card_obj) is False
    # Now put a creature on P0's battlefield — the cost becomes payable.
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    state.battlefield.append(GameObject(
        id=11, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    ))
    assert _additional_costs_payable(state, 0, card_obj) is True


def test_final_vengeance_additional_cost_accepts_enchantment_sacrifice():
    # An Enchantment on P0's battlefield is also a valid sac target —
    # Final Vengeance's printed cost says "creature OR enchantment".
    from mtgz_ir.cards.base import make_enchantment
    from mtgz_ir.game import GameObject, GameState, Player, Zone
    from mtgz_ir.legal import _additional_costs_payable

    parser = OracleParserCardSource()
    final_vengeance = parser.get_printings()["Final Vengeance"]
    card_obj = GameObject(
        id=10, printing=final_vengeance, owner_id=0, controller_id=0,
        zone=Zone.HAND,
    )
    ench_pr = make_enchantment("Tome", "{1}{U}")
    ench_obj = GameObject(
        id=12, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[ench_obj],
    )
    assert _additional_costs_payable(state, 0, card_obj) is True


# ---------- Frantic Strength: Aura with keywords (Flash) ----------

def test_make_aura_accepts_keywords_kwarg():
    # New ``keywords`` kwarg on ``make_aura`` mirrors the equipment
    # builder's same-named field. The frozen-set lands on the
    # ``CardPrinting.keywords`` field that ``legal.py`` reads to
    # surface a permanent spell at instant speed.
    from mtgz_ir.cards.base import make_aura

    aura = make_aura(
        "Test Aura", "{1}{U}",
        power_bonus=1, toughness_bonus=1,
        keywords=frozenset({"flash"}),
    )
    assert "flash" in aura.keywords
    assert aura.equipment_bonuses == (1, 1, frozenset())


def test_make_aura_keywords_default_empty():
    # Backward-compat sanity: the new kwarg defaults to an empty set,
    # so existing call sites that don't pass ``keywords=`` keep their
    # previous behavior (no keywords on the Aura itself).
    from mtgz_ir.cards.base import make_aura

    aura = make_aura("Plain Aura", "{U}")
    assert aura.keywords == frozenset()


def test_frantic_strength_claimed_by_parser():
    # Frantic Strength ``{1}{R}`` Aura — Flash + Enchant creature +
    # +2/+2 + trample. Claimed via the name-keyed
    # ``_ENCHANTMENT_BUILDERS`` dispatch in
    # ``parser.enchantment_builder``.
    assert "Frantic Strength" in _claimed_names()


def test_frantic_strength_carries_aura_fields():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the three hooks the static side ships:
    # - Flash on the Aura itself (``keywords`` containing ``flash``);
    # - +2/+2 in the ``equipment_bonuses`` triple;
    # - trample as a granted keyword on the enchanted creature.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Frantic Strength"]
    assert "flash" in p.keywords
    assert p.equipment_bonuses == (2, 2, frozenset({"trample"}))


def test_frantic_strength_is_aura_subtype():
    # Sanity that the Aura-builder route is what claimed it (not the
    # spell-builder or some other path) — the Aura subtype must be
    # present on the printing.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Frantic Strength"]
    assert p.is_aura is True
    assert p.aura_enchant_type == "creature"


# ---------- target creature can't be blocked this turn ----------

def test_parse_target_creature_cant_be_blocked_this_turn():
    # New primitive: bare unblockable-UEOT grant on a creature target.
    # Distinct from the pump-with-unblockable-rider shapes already in
    # the grammar (Atomic Microsizer's attack trigger); this is the
    # bare make-unblockable shape — Enter the Enigma's prefix.
    eff = _parse_effect("target creature can't be blocked this turn")
    assert eff == GrantUnblockableUEOT()


def test_grant_unblockable_ueot_legal_targets_lists_creatures_only():
    # The new primitive's ``legal_targets`` returns ("creature", id)
    # tuples for every battlefield creature (and animated land that
    # is currently a creature). Non-creature permanents are excluded.
    from mtgz_ir.cards.base import (
        make_creature,
        make_enchantment,
    )
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    ench_pr = make_enchantment("Tome", "{1}{U}")
    bear_obj = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    ench_obj = GameObject(
        id=2, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear_obj, ench_obj],
    )
    legal = GrantUnblockableUEOT().legal_targets(state, controller_id=0)
    assert ("creature", 1) in legal
    assert ("creature", 2) not in legal
    assert ("enchantment", 2) not in legal


def test_grant_unblockable_ueot_resolve_grants_keyword():
    # End-to-end resolve sanity: pick a target creature, run resolve,
    # verify a layer-6 ``cant_be_blocked`` registry entry appears for
    # the target. ``is_cant_be_blocked`` reads this entry, and
    # ``combat.can_block`` / ``combat.declare_blockers`` consult
    # ``is_cant_be_blocked`` rather than ``effective_keywords``
    # because "can't be blocked" is a continuous gameplay-action
    # restriction (CR 509.1b), not a keyword ability.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import (
        GameObject, GameState, Player, Zone, is_cant_be_blocked,
    )

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear_obj = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear_obj],
    )
    eff = GrantUnblockableUEOT()
    assert not is_cant_be_blocked(state, bear_obj)
    eff.resolve(state, controller_id=0, targets=(("creature", 1),))
    assert is_cant_be_blocked(state, bear_obj)


def test_grant_unblockable_ueot_resolve_no_target_is_no_op():
    # The primitive requires a target — but a fizzled resolve (target
    # gone, or empty target tuple) must be a silent no-op without
    # crashing.
    from mtgz_ir.game import GameState, Player

    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[],
    )
    eff = GrantUnblockableUEOT()
    # Empty targets — nothing to do.
    eff.resolve(state, controller_id=0, targets=())
    # Wrong-kind target — also a no-op (defensive against the
    # ``("enchantment", id)`` widening on DestroyTargetCreature; the
    # bare unblockable primitive only works against creatures).
    eff.resolve(state, controller_id=0, targets=(("enchantment", 99),))


def test_enter_the_enigma_claimed_by_parser():
    # Enter the Enigma ``{2}{U}`` Sorcery — ``Target creature can't be
    # blocked this turn. Draw a card.`` Two-sentence body that
    # combines the new bare-make-unblockable primitive with the
    # existing ``draw a card`` primitive, composed via Sequence by
    # the spell-effect parser's sentence-split path.
    assert "Enter the Enigma" in _claimed_names()


def test_enter_the_enigma_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the right Sequence shape (unblockable-UEOT, then draw 1).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Enter the Enigma"]
    assert p.spell_effect == Sequence(effects=(
        GrantUnblockableUEOT(),
        DrawCards(amount=1),
    ))


# ---------- Seized from Slumber: tapped-target cost reduction ----------

def test_parse_cost_reduction_targets_tapped_creature():
    # New cost-reduction condition: ``it targets a tapped creature``
    # maps to the engine condition key ``target_is_tapped_creature``.
    # Parallel to the existing ``it targets a blocking creature`` shape.
    from mtgz_ir.cards.base import CostReduction
    from mtgz_ir.cards.parser.cost_reduction import (
        _parse_cost_reduction_line,
    )

    rule = _parse_cost_reduction_line(
        "This spell costs {3} less to cast if it targets a tapped creature."
    )
    assert rule == CostReduction(
        amount=3, condition=TargetMatches(IsCreature() & IsTapped()),
    )


def test_seized_from_slumber_claimed_by_parser():
    # Seized from Slumber ``{2}{B}`` Instant — cost-reduction rider
    # ``This spell costs {3} less to cast if it targets a tapped
    # creature.`` plus a base ``Destroy target creature.`` body. The
    # rider rides the existing CostReduction machinery (with the new
    # condition key); the base body is the existing
    # ``DestroyTargetCreature`` primitive.
    assert "Seized from Slumber" in _claimed_names()


def test_seized_from_slumber_carries_cost_reduction_rule():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # both the cost-reduction rule and the bare destroy effect.
    from mtgz_ir.cards.base import CostReduction

    parser = OracleParserCardSource()
    p = parser.get_printings()["Seized from Slumber"]
    assert p.cost_reductions == (
        CostReduction(amount=3, condition=TargetMatches(IsCreature() & IsTapped())),
    )
    assert p.spell_effect == DestroyTargetCreature()


def test_target_is_tapped_creature_evaluator_optimistic_when_no_targets():
    # The cost-reduction evaluator's optimistic ``chosen_targets is
    # None`` branch — used by the legal-actions affordability probe
    # before the spell's targets are committed — must apply the
    # discount when ANY tapped creature is on the battlefield (the
    # caster could pick it).
    from mtgz_ir.actions import _evaluate_cost_reduction
    from mtgz_ir.cards.base import CostReduction, make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    rule = CostReduction(amount=3, condition=TargetMatches(IsCreature() & IsTapped()))
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear_obj = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD, tapped=True,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear_obj],
    )
    # A tapped creature exists — the optimistic probe applies the
    # discount.
    assert _evaluate_cost_reduction(
        rule, state, caster_id=0, chosen_targets=None,
    ) == 3
    # Untap the creature — no tapped creature on the battlefield, so
    # the optimistic probe doesn't apply the discount.
    bear_obj.tapped = False
    assert _evaluate_cost_reduction(
        rule, state, caster_id=0, chosen_targets=None,
    ) == 0


def test_target_is_tapped_creature_evaluator_committed_target():
    # When the caster has committed to a target, the discount applies
    # only if THAT target is tapped (not just any tapped creature).
    from mtgz_ir.actions import _evaluate_cost_reduction
    from mtgz_ir.cards.base import CostReduction, make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    rule = CostReduction(amount=3, condition=TargetMatches(IsCreature() & IsTapped()))
    tapped_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    untapped_pr = make_creature("Bird", "{U}", power=1, toughness=1)
    tapped_obj = GameObject(
        id=1, printing=tapped_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD, tapped=True,
    )
    untapped_obj = GameObject(
        id=2, printing=untapped_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD, tapped=False,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[tapped_obj, untapped_obj],
    )
    # Pointing at the tapped creature — discount applies.
    assert _evaluate_cost_reduction(
        rule, state, caster_id=0,
        chosen_targets=(("creature", 1),),
    ) == 3
    # Pointing at the untapped creature — no discount.
    assert _evaluate_cost_reduction(
        rule, state, caster_id=0,
        chosen_targets=(("creature", 2),),
    ) == 0
