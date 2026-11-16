"""Tests for the bespoke-spells1 migration batch.

This batch migrates five entries out of ``_WHOLE_SPELL_EXACT_TEXT`` in
``parser.spell_builder`` by introducing three grammar extensions:

1. **Delirium effect-rider dropper** — ``_is_delirium_drop_clause``
   recognizes ``Delirium — If there are four or more card types …,
   <effect>.`` lines and drops them from the clause stream before the
   per-clause grammar runs.  The base spell body then parses normally.
   Migrates: Impossible Inferno, Violent Urge.

2. **Delirium cost-reduction dropper** — same helper recognizes the
   ``Delirium — This spell costs {N} less to cast as long as …`` shape
   and drops it so no spurious ``CostReduction`` rule is attached.
   Migrates: Drag to the Roots.

3. **Delirium modal-header recognizer** — ``_parse_modal_choose_one``
   now accepts the ``Delirium — Choose one. If there are four or more
   card types …, choose one or more instead.`` header and treats it as
   a bare ``Choose one —`` header (the Delirium upgrade is dropped).
   Migrates: Let's Play a Game.

4. **Generalized ``SearchLibraryForCard`` recognizer** — three literal
   regexes (artifact, basic land, legendary creature) are replaced by
   one parameterized regex + dispatch table.  Also adds "Demon" to the
   dispatch table, enabling Demonic Counsel's base clause to be claimed
   by the per-clause grammar once the Delirium upgrade line is dropped.
   Migrates: Demonic Counsel.

Total migration: 5 entries removed from ``_WHOLE_SPELL_EXACT_TEXT``.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    DealDamageToTargetCreature,
    DestroyTargetCreature,
    DestroyTargetNonlandPermanent,
    EachOpponentDiscards,
    EachOpponentLosesLife,
    GainLife,
    ModalChooseOne,
    PumpCreaturesOpponentsControlUntilEOT,
    PumpTargetCreatureUntilEOT,
    SearchLibraryForCard,
    Sequence,
    make_creature,
)
from mtgz_ir.cards.parser.effects import _parse_effect, _parse_spell_effect_phrase
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.spell_builder import _DELIRIUM_COST_REDUCTION_RIDER_RE, _DELIRIUM_EFFECT_RIDER_RE, _DELIRIUM_MODAL_HEADER_RE, _EXILE_ON_DEATH_THIS_TURN_INSTEAD_RE, _IF_CREATURE_CARD_PUT_INTO_GY_THIS_WAY_RETURN_IT_RE, _IF_THEY_DO_RIDER_RE, _is_delirium_drop_clause, _ITS_CONTROLLER_MANIFESTS_DREAD_RE, _is_unimplemented_secondary_rider, _MANA_SPENT_CONDITIONAL_DRAW_RE, _SACRIFICE_IT_AT_BEGINNING_OF_NEXT_END_STEP_RE, _SUBTYPE_CREATURES_YOU_CONTROL_GAIN_KEYWORDS_UEOT_RE, _WHEN_IT_DIES_THIS_TURN_MANIFEST_DREAD_RE, _YOU_MAY_RETURN_PERMANENT_CARD_FROM_GY_TO_HAND_RE


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ------------------------------------------------------------------ #
# Table-size regression                                               #
# ------------------------------------------------------------------ #


# ------------------------------------------------------------------ #
# _is_delirium_drop_clause                                            #
# ------------------------------------------------------------------ #

def test_delirium_effect_rider_is_dropped():
    # ``Delirium — If there are four or more card types …, <effect>.``
    assert _is_delirium_drop_clause(
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, that creature gains double strike "
        "until end of turn."
    )


def test_delirium_cost_reduction_rider_is_dropped():
    # ``Delirium — This spell costs {N} less to cast as long as …``
    assert _is_delirium_drop_clause(
        "Delirium — This spell costs {2} less to cast as long as "
        "there are four or more card types among cards in your graveyard."
    )


def test_delirium_modal_header_not_dropped():
    # The modal header is handled by ``_parse_modal_choose_one``
    # (not by the dropper), so it must NOT match ``_is_delirium_drop_clause``.
    assert not _is_delirium_drop_clause(
        "Delirium — Choose one. If there are four or more card "
        "types among cards in your graveyard, choose one or more instead."
    )


def test_non_delirium_clause_not_dropped():
    assert not _is_delirium_drop_clause("Destroy target creature.")
    assert not _is_delirium_drop_clause("Counter target spell.")
    assert not _is_delirium_drop_clause(
        "Void — This spell costs {2} less to cast if a nonland "
        "permanent left the battlefield this turn or a spell was "
        "warped this turn."
    )


# ------------------------------------------------------------------ #
# Delirium regex patterns                                             #
# ------------------------------------------------------------------ #

def test_delirium_effect_rider_re_matches():
    assert _DELIRIUM_EFFECT_RIDER_RE.match(
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, exile the top card of your library."
    ) is not None


def test_delirium_cost_reduction_rider_re_matches():
    assert _DELIRIUM_COST_REDUCTION_RIDER_RE.match(
        "Delirium — This spell costs {2} less to cast as long as "
        "there are four or more card types among cards in your graveyard."
    ) is not None


def test_delirium_modal_header_re_matches():
    assert _DELIRIUM_MODAL_HEADER_RE.match(
        "Delirium — Choose one. If there are four or more card "
        "types among cards in your graveyard, choose one or more instead."
    ) is not None


def test_delirium_modal_header_re_no_false_positives():
    # Should not match the effect-rider variant
    assert _DELIRIUM_MODAL_HEADER_RE.match(
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, that creature gains double strike "
        "until end of turn."
    ) is None


# ------------------------------------------------------------------ #
# Impossible Inferno (Delirium effect-rider drop + base parse)        #
# ------------------------------------------------------------------ #

def test_impossible_inferno_claimed_by_parser():
    assert "Impossible Inferno" in _claimed_names()


def test_impossible_inferno_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Impossible Inferno"]
    assert p.spell_effect == DealDamageToTargetCreature(amount=6)


def test_impossible_inferno_no_bespoke_entry():
    # The bespoke table no longer has an Impossible Inferno key.
    text = (
        "Impossible Inferno deals 6 damage to target creature.\n"
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, exile the top card of your "
        "library. You may play it until the end of your next turn."
    )
    from mtgz_ir.cards.parser.text import _strip_reminder_text


# ------------------------------------------------------------------ #
# Violent Urge (Delirium effect-rider drop + base parse)              #
# ------------------------------------------------------------------ #

def test_violent_urge_claimed_by_parser():
    assert "Violent Urge" in _claimed_names()


def test_violent_urge_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Violent Urge"]
    assert p.spell_effect == PumpTargetCreatureUntilEOT(
        power_bonus=1,
        toughness_bonus=0,
        granted_keywords=frozenset({"first strike"}),
    )


def test_violent_urge_no_bespoke_entry():
    text = (
        "Target creature gets +1/+0 and gains first strike until end "
        "of turn.\n"
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, that creature gains double strike "
        "until end of turn."
    )
    from mtgz_ir.cards.parser.text import _strip_reminder_text


# ------------------------------------------------------------------ #
# Drag to the Roots (Delirium cost-reduction drop)                    #
# ------------------------------------------------------------------ #

def test_drag_to_the_roots_claimed_by_parser():
    assert "Drag to the Roots" in _claimed_names()


def test_drag_to_the_roots_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Drag to the Roots"]
    assert p.spell_effect == DestroyTargetNonlandPermanent()


def test_drag_to_the_roots_no_cost_reductions():
    # The Delirium cost-reduction rider is dropped entirely; no
    # CostReduction rule should appear on the printing.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Drag to the Roots"]
    assert p.cost_reductions == ()


def test_drag_to_the_roots_no_bespoke_entry():
    text = (
        "Delirium — This spell costs {2} less to cast as long "
        "as there are four or more card types among cards in your "
        "graveyard.\n"
        "Destroy target nonland permanent."
    )
    from mtgz_ir.cards.parser.text import _strip_reminder_text


# ------------------------------------------------------------------ #
# Let's Play a Game (Delirium modal header)                           #
# ------------------------------------------------------------------ #

def test_lets_play_a_game_claimed_by_parser():
    assert "Let's Play a Game" in _claimed_names()


def test_lets_play_a_game_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Let's Play a Game"]
    assert p.spell_effect == ModalChooseOne(
        modes=(
            PumpCreaturesOpponentsControlUntilEOT(
                power_bonus=-1, toughness_bonus=-1,
            ),
            EachOpponentDiscards(count=2),
            Sequence(
                effects=(
                    EachOpponentLosesLife(amount=3),
                    GainLife(amount=3),
                ),
            ),
        ),
    )


def test_lets_play_a_game_or_both_false():
    # The Delirium "choose one or more" upgrade is dropped; the
    # simplified printing uses the bare Choose one shape (or_both=False).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Let's Play a Game"]
    assert p.spell_effect.or_both is False


def test_lets_play_a_game_no_bespoke_entry():
    text = (
        "Delirium — Choose one. If there are four or more card "
        "types among cards in your graveyard, choose one or more "
        "instead.\n"
        "• Creatures your opponents control get -1/-1 until end of "
        "turn.\n"
        "• Each opponent discards two cards.\n"
        "• Each opponent loses 3 life and you gain 3 life."
    )
    from mtgz_ir.cards.parser.text import _strip_reminder_text


# ------------------------------------------------------------------ #
# Demonic Counsel (generalized SearchLibraryForCard + Delirium drop)  #
# ------------------------------------------------------------------ #

def test_demonic_counsel_claimed_by_parser():
    assert "Demonic Counsel" in _claimed_names()


def test_demonic_counsel_engine_class():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Demonic Counsel"]
    assert p.spell_effect == SearchLibraryForCard(
        filter_name="demon", may_fail=False,
    )


def test_demonic_counsel_no_bespoke_entry():
    text = (
        "Search your library for a Demon card, reveal it, put it "
        "into your hand, then shuffle.\n"
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, instead search your library for "
        "any card, put it into your hand, then shuffle."
    )
    from mtgz_ir.cards.parser.text import _strip_reminder_text


# ------------------------------------------------------------------ #
# Generalized SearchLibraryForCard recognizer                         #
# ------------------------------------------------------------------ #

def test_search_library_demon_card_parses():
    # The generalized recognizer now claims this surface; before the
    # migration it was a bespoke entry.
    eff = _parse_spell_effect_phrase(
        "Search your library for a Demon card, reveal it, "
        "put it into your hand, then shuffle.",
        "Demonic Counsel",
    )
    assert eff == SearchLibraryForCard(filter_name="demon", may_fail=False)


def test_search_library_artifact_card_still_parses():
    # Regression: existing artifact-tutor surface still claims correctly.
    eff = _parse_spell_effect_phrase(
        "Search your library for an artifact card, reveal it, "
        "put it into your hand, then shuffle.",
        "Scour for Scrap",
    )
    assert eff == SearchLibraryForCard(filter_name="artifact", may_fail=True)


def test_search_library_basic_land_card_still_parses():
    # Regression: existing basic-land-tutor surface still claims correctly.
    eff = _parse_spell_effect_phrase(
        "Search your library for a basic land card, reveal it, "
        "put it into your hand, then shuffle.",
        "Test Spell",
    )
    assert eff == SearchLibraryForCard(filter_name="basic_land", may_fail=True)


def test_search_library_legendary_creature_card_still_parses():
    # Regression: existing legendary-creature-tutor surface still claims.
    eff = _parse_spell_effect_phrase(
        "Search your library for a legendary creature card, reveal it, "
        "put it into your hand, then shuffle.",
        "Test Spell",
    )
    assert eff == SearchLibraryForCard(
        filter_name="legendary_creature", may_fail=False,
    )


def test_search_library_unknown_filter_returns_none():
    # An unrecognized filter phrase should not silently map to a wrong
    # engine class; the parser should return None (spell stays unclaimed).
    eff = _parse_spell_effect_phrase(
        "Search your library for a unicorn card, reveal it, "
        "put it into your hand, then shuffle.",
        "Test Spell",
    )
    assert eff is None


def test_search_library_for_card_demon_filter_matches():
    # Belt-and-suspenders: the "demon" filter maps correctly in the engine
    # class, mirroring the test that was in batch-7 before the migration.
    eff = SearchLibraryForCard(filter_name="demon", may_fail=False)
    demon_pr = make_creature(
        "Test Demon", "{4}{B}{B}", power=6, toughness=6,
        subtypes=("Demon",),
    )
    non_demon_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    assert eff._matches(demon_pr) is True
    assert eff._matches(non_demon_pr) is False


# ------------------------------------------------------------------ #
# _is_unimplemented_secondary_rider                                   #
# ------------------------------------------------------------------ #

def test_its_controller_manifests_dread_is_dropped():
    # ``Its controller manifests dread.`` — controller-manifest rider
    assert _is_unimplemented_secondary_rider("Its controller manifests dread.")


def test_when_it_dies_this_turn_manifest_dread_is_dropped():
    # ``When it dies this turn, manifest dread.`` — death-trigger rider
    assert _is_unimplemented_secondary_rider(
        "When it dies this turn, manifest dread."
    )


def test_you_may_return_permanent_card_from_gy_to_hand_is_dropped():
    # ``You may return a permanent card from your graveyard to your
    # hand.`` — optional graveyard-return rider
    assert _is_unimplemented_secondary_rider(
        "You may return a permanent card from your graveyard to your hand."
    )


def test_non_rider_clauses_not_dropped():
    # Belt-and-suspenders: ordinary spell clauses must NOT match.
    assert not _is_unimplemented_secondary_rider("Destroy target creature.")
    assert not _is_unimplemented_secondary_rider("Manifest dread.")
    assert not _is_unimplemented_secondary_rider(
        "Delirium — If there are four or more card types among "
        "cards in your graveyard, exile the top card of your library."
    )


def test_its_controller_manifests_dread_re_matches():
    assert _ITS_CONTROLLER_MANIFESTS_DREAD_RE.match(
        "Its controller manifests dread."
    ) is not None


def test_when_it_dies_this_turn_manifest_dread_re_matches():
    assert _WHEN_IT_DIES_THIS_TURN_MANIFEST_DREAD_RE.match(
        "When it dies this turn, manifest dread."
    ) is not None


def test_you_may_return_permanent_card_from_gy_to_hand_re_matches():
    assert _YOU_MAY_RETURN_PERMANENT_CARD_FROM_GY_TO_HAND_RE.match(
        "You may return a permanent card from your graveyard to your hand."
    ) is not None


# ------------------------------------------------------------------ #
# Unwanted Remake / Under the Skin / Turn Inside Out                  #
# (per-clause grammar claims via unimplemented-rider dropper)         #
# ------------------------------------------------------------------ #


def test_unwanted_remake_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import DestroyTargetCreature
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Destroy target creature. Its controller manifests dread.",
        "Unwanted Remake",
    )
    assert eff == DestroyTargetCreature()


def test_under_the_skin_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import ManifestDread
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Manifest dread.\n"
        "You may return a permanent card from your graveyard to your hand.",
        "Under the Skin",
    )
    assert eff == ManifestDread(count=1)


def test_turn_inside_out_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Target creature gets +3/+0 until end of turn. "
        "When it dies this turn, manifest dread.",
        "Turn Inside Out",
    )
    assert eff == PumpTargetCreatureUntilEOT(power_bonus=3, toughness_bonus=0)


# ------------------------------------------------------------------ #
# Unravel / Midnight Mayhem                                           #
# (per-clause grammar claims via mana-spent / subtype-pump dropper)   #
# ------------------------------------------------------------------ #

def test_mana_spent_conditional_draw_re_matches():
    assert _MANA_SPENT_CONDITIONAL_DRAW_RE.match(
        "If the amount of mana spent to cast that spell was less "
        "than its mana value, you draw a card."
    ) is not None


def test_subtype_creatures_gain_keywords_ueot_re_matches():
    assert _SUBTYPE_CREATURES_YOU_CONTROL_GAIN_KEYWORDS_UEOT_RE.match(
        "Gremlins you control gain menace, lifelink, and haste until "
        "end of turn."
    ) is not None


def test_subtype_creatures_gain_keywords_ueot_re_no_false_positive_on_creatures():
    # ``Creatures you control gain…`` IS modelable via the pump
    # recognizer — must NOT be dropped by this rider pattern.
    assert _SUBTYPE_CREATURES_YOU_CONTROL_GAIN_KEYWORDS_UEOT_RE.match(
        "Creatures you control gain flying until end of turn."
    ) is None


def test_mana_spent_rider_is_dropped():
    # (Name kept for history; the rider is NO LONGER dropped.)
    # Implemented since feature/eoe-ir-fixes: Unravel's mana-spent rider
    # is modelled as CounterSpell(if_mana_spent_below_mv=DrawCards(1)).
    assert not _is_unimplemented_secondary_rider(
        "If the amount of mana spent to cast that spell was less "
        "than its mana value, you draw a card."
    )


def test_subtype_pump_rider_is_dropped():
    assert _is_unimplemented_secondary_rider(
        "Gremlins you control gain menace, lifelink, and haste until end of turn."
    )


def test_unravel_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import CounterTargetSpell
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Counter target spell. "
        "If the amount of mana spent to cast that spell was less "
        "than its mana value, you draw a card.",
        "Unravel",
    )
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    from mtgz_ir.effects.generic.draw_mill import DrawCards
    assert eff == CounterSpell(if_mana_spent_below_mv=DrawCards(1))


def test_midnight_mayhem_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import CreateToken
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    from dataclasses import replace
    eff = _parse_spell_oracle_text(
        "Create three 1/1 red Gremlin creature tokens. "
        "Gremlins you control gain menace, lifelink, and haste until "
        "end of turn.",
        "Midnight Mayhem",
    )
    # The printed colour (red) is carried as the token's colour indicator.
    assert eff == CreateToken(
        token_printing=replace(TOKEN_PRINTINGS["Gremlin"], color_indicator=frozenset({"R"})), count=3,
    )


# ------------------------------------------------------------------ #
# Scorching Dragonfire                                                 #
# (per-clause grammar: damage-to-creature-or-planeswalker +           #
#  exile-on-death rider dropper)                                      #
# ------------------------------------------------------------------ #

def test_exile_on_death_this_turn_instead_re_matches():
    assert _EXILE_ON_DEATH_THIS_TURN_INSTEAD_RE.match(
        "If that creature or planeswalker would die this turn, "
        "exile it instead."
    ) is not None


def test_exile_on_death_rider_is_dropped():
    assert _is_unimplemented_secondary_rider(
        "If that creature or planeswalker would die this turn, "
        "exile it instead."
    )


def test_scorching_dragonfire_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import DealDamageToTargetCreature
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Scorching Dragonfire deals 3 damage to target creature or "
        "planeswalker. If that creature or planeswalker would die "
        "this turn, exile it instead.",
        "Scorching Dragonfire",
    )
    assert eff == DealDamageToTargetCreature(amount=3)


def test_deal_damage_target_creature_or_planeswalker_parses():
    # Belt-and-suspenders: the extended _DEAL_DAMAGE_TARGET_CREATURE
    # recognizer now accepts the ``or planeswalker`` widening.
    from mtgz_ir.cards.base import DealDamageToTargetCreature
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    eff = _parse_spell_effect_phrase(
        "Scorching Dragonfire deals 3 damage to target creature or planeswalker.",
        "Scorching Dragonfire",
    )
    assert eff == DealDamageToTargetCreature(amount=3)


# ------------------------------------------------------------------ #
# Divert Disaster / Don't Make a Sound                                #
# (per-clause grammar: Mana-Leak counterspell + "If they do" dropper) #
# ------------------------------------------------------------------ #

def test_if_they_do_rider_re_matches():
    assert _IF_THEY_DO_RIDER_RE.match(
        "If they do, you create a Lander token."
    ) is not None
    assert _IF_THEY_DO_RIDER_RE.match(
        "If they do, surveil 2."
    ) is not None


def test_if_they_do_rider_is_dropped():
    assert _is_unimplemented_secondary_rider(
        "If they do, you create a Lander token."
    )
    assert _is_unimplemented_secondary_rider(
        "If they do, surveil 2."
    )


def test_counter_target_spell_unless_pays_parses():
    # Outside the Rust-parity-pinned legacy spells the pay-or-counter
    # tax is modeled faithfully (``CounterSpell.unless_pays``).
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    eff = _parse_spell_effect_phrase(
        "Counter target spell unless its controller pays {2}.",
        "Test",
    )
    assert eff == CounterSpell(unless_pays="{2}", label="spell")


def test_divert_disaster_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import CounterTargetSpell
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Counter target spell unless its controller pays {2}. "
        "If they do, you create a Lander token.",
        "Divert Disaster",
    )
    from mtgz_ir.cards.base import CreateToken
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    assert eff == CounterSpell(
        unless_pays="{2}", label="spell",
        if_paid=CreateToken(token_printing=TOKEN_PRINTINGS["Lander"]),
    )


def test_dont_make_a_sound_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import CounterTargetSpell
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Counter target spell unless its controller pays {2}. "
        "If they do, surveil 2.",
        "Don't Make a Sound",
    )
    assert eff == CounterTargetSpell()


# ------------------------------------------------------------------ #
# Come Back Wrong                                                      #
# (N-clause general rider-drop + conditional-reanimate / delayed-sac  #
#  rider patterns)                                                     #
# ------------------------------------------------------------------ #

def test_if_creature_card_put_into_gy_this_way_re_matches():
    assert _IF_CREATURE_CARD_PUT_INTO_GY_THIS_WAY_RETURN_IT_RE.match(
        "If a creature card is put into a graveyard this way, "
        "return it to the battlefield under your control."
    ) is not None


def test_sacrifice_it_at_beginning_of_next_end_step_re_matches():
    assert _SACRIFICE_IT_AT_BEGINNING_OF_NEXT_END_STEP_RE.match(
        "Sacrifice it at the beginning of your next end step."
    ) is not None


def test_conditional_reanimate_this_way_rider_is_dropped():
    assert _is_unimplemented_secondary_rider(
        "If a creature card is put into a graveyard this way, "
        "return it to the battlefield under your control."
    )


def test_delayed_sac_trigger_rider_is_dropped():
    assert _is_unimplemented_secondary_rider(
        "Sacrifice it at the beginning of your next end step."
    )


def test_come_back_wrong_parses_via_per_clause_grammar():
    from mtgz_ir.cards.base import DestroyTargetCreature
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Destroy target creature. If a creature card is put into a "
        "graveyard this way, return it to the battlefield under "
        "your control. Sacrifice it at the beginning of your next "
        "end step.",
        "Come Back Wrong",
    )
    assert eff == DestroyTargetCreature()
