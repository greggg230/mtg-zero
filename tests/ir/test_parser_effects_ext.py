"""Parse-shape tests for ``mtgz_ir.cards.parser.effects_ext`` (the
unified-grammar extension recognizers) and the planeswalker builder's
line recognizers."""

from __future__ import annotations

import pytest

from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.cards.parser.effects_ext import (
    parse_ext_effect,
    parse_target_noun_phrase,
)
from mtgz_ir.cards.parser.planeswalker_builder import (
    _build_planeswalker_printing,
    _parse_loyalty_effect,
)
from mtgz_ir.effects.combinators import MayDo, Sequence
from mtgz_ir.effects.generic.bounce import Bounce
from mtgz_ir.effects.generic.cube_walkers import (
    HasManaValueAtLeast,
    HasManaValueAtMost,
    HasPrintedCardType,
    IfCondition,
    IsPermanent,
    LookTopNMayRevealMatchingToHandRestBottomRandom,
    MayCastSorceriesAsThoughFlashUntilYourNextTurn,
    PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords,
    PutPlusOneCountersOnTargetThenGrantKeywordsUEOT,
    SacrificeCreatureThenDamageEqualToItsPower,
    WinTheGame,
    YourLibraryHasNoCards,
)
from mtgz_ir.effects.generic.destroy import Destroy
from mtgz_ir.effects.generic.exile import Exile
from mtgz_ir.effects.generic.tokens import CreateToken
from mtgz_ir.effects.object_predicates import (
    ControlledByOpponent,
    ControlledByYou,
    HasKeyword,
    IsArtifact,
    IsAttacking,
    IsCreature,
    IsEnchantment,
    IsLand,
    IsTapped,
)


# ---------------------------------------------------------------------------
# Target noun phrases
# ---------------------------------------------------------------------------


def test_noun_simple_and_adjective():
    assert parse_target_noun_phrase("creature") == IsCreature()
    assert parse_target_noun_phrase("tapped creature") == (
        IsCreature() & IsTapped()
    )
    assert parse_target_noun_phrase("untapped creature") == (
        IsCreature() & ~IsTapped()
    )


def test_noun_type_list_union():
    assert parse_target_noun_phrase(
        "artifact, creature, or enchantment"
    ) == (IsArtifact() | IsCreature()) | IsEnchantment()
    assert parse_target_noun_phrase("artifact or creature") == (
        IsArtifact() | IsCreature()
    )


def test_noun_controller_and_mana_value():
    assert parse_target_noun_phrase(
        "creature an opponent controls with mana value 3 or greater"
    ) == (IsCreature() & ControlledByOpponent()) & HasManaValueAtLeast(3)
    assert parse_target_noun_phrase(
        "creature you control with mana value 2 or less"
    ) == (IsCreature() & ControlledByYou()) & HasManaValueAtMost(2)
    pred = parse_target_noun_phrase("creature you don't control")
    assert pred == IsCreature() & ~ControlledByYou()


def test_noun_keyword_qualifier():
    assert parse_target_noun_phrase("creature with trample or haste") == (
        IsCreature() & (HasKeyword("trample") | HasKeyword("haste"))
    )
    assert parse_target_noun_phrase("creature with flying") == (
        IsCreature() & HasKeyword("flying")
    )


def test_noun_permanent_and_nonland_permanent():
    assert parse_target_noun_phrase("permanent") == IsPermanent()
    assert parse_target_noun_phrase("nonland permanent") == (
        IsPermanent() & ~IsLand()
    )
    assert parse_target_noun_phrase("permanent an opponent controls") == (
        IsPermanent() & ControlledByOpponent()
    )
    assert parse_target_noun_phrase("attacking creature") == (
        IsCreature() & IsAttacking()
    )


@pytest.mark.parametrize("phrase", [
    "creature with a +1/+1 counter on it",   # unknown qualifier
    "spell",                                  # not a permanent noun
    "creature card in a graveyard",           # zone phrase
    "tapped",                                 # adjective only
    "artifact, , or creature",                # malformed list
])
def test_noun_rejects_unknown_shapes(phrase):
    assert parse_target_noun_phrase(phrase) is None


def test_is_permanent_rejects_players():
    from mtgz_ir.game import GameState, Player

    s = GameState()
    s.players.append(Player(id=0))
    assert not IsPermanent().test(s.players[0], s)


# ---------------------------------------------------------------------------
# Effect recognizers
# ---------------------------------------------------------------------------


def test_win_the_game():
    assert _parse_effect("you win the game") == WinTheGame()
    assert _parse_effect("win the game") == WinTheGame()


def test_if_condition_known_and_unknown():
    eff = _parse_effect(
        "if your library has no cards in it, you win the game"
    )
    assert eff == IfCondition(
        condition=YourLibraryHasNoCards(), then=WinTheGame(),
    )
    assert parse_ext_effect(
        "if you control a Wizard, you win the game"
    ) is None
    assert parse_ext_effect(
        "if your library has no cards in it, frobnicate"
    ) is None


def test_you_may_wraps_inner_effect():
    eff = _parse_effect("you may draw a card")
    assert isinstance(eff, MayDo)
    assert parse_ext_effect("you may frobnicate") is None


def test_destroy_exile_bounce_generic_targets():
    d = _parse_effect(
        "destroy target creature an opponent controls with mana value "
        "3 or greater"
    )
    assert isinstance(d, Destroy)
    assert d.targets[0].count_min == 1
    e = _parse_effect("exile target tapped creature")
    assert isinstance(e, Exile)
    b = _parse_effect(
        "return up to one target artifact, creature, or enchantment to "
        "its owner's hand"
    )
    assert isinstance(b, Bounce)
    assert b.targets[0].count_min == 0
    assert parse_ext_effect("destroy target spell") is None
    assert parse_ext_effect("return target spell to its owner's hand") is None


def test_counters_on_target_with_and_without_rider():
    eff = _parse_effect(
        "put a +1/+1 counter on up to one target creature. It gains "
        "first strike until end of turn"
    )
    assert isinstance(eff, PutPlusOneCountersOnTargetThenGrantKeywordsUEOT)
    assert eff.amount == 1
    assert eff.granted_keywords == frozenset({"first strike"})
    assert eff.targets[0].count_min == 0
    plain = _parse_effect(
        "put three +1/+1 counters on up to one target creature with "
        "trample or haste"
    )
    assert plain.amount == 3 and plain.granted_keywords == frozenset()
    assert parse_ext_effect(
        "put a +1/+1 counter on target creature. It gains wibble until "
        "end of turn"
    ) is None


def test_counters_on_each_then_gain():
    eff = _parse_effect(
        "put a +1/+1 counter on each creature you control. Those "
        "creatures gain flying until your next turn"
    )
    assert eff == PutPlusOneCounterOnEachCreatureYouControlThenGrantKeywords(
        amount=1, granted_keywords=frozenset({"flying"}),
        until_your_next_turn=True,
    )
    eot = _parse_effect(
        "put a +1/+1 counter on each creature you control. Those "
        "creatures gain vigilance and trample until end of turn"
    )
    assert eot.until_your_next_turn is False
    assert eot.granted_keywords == frozenset({"vigilance", "trample"})
    assert parse_ext_effect(
        "put a +1/+1 counter on each creature you control. Those "
        "creatures gain wibble until end of turn"
    ) is None


def test_look_top_may_reveal_rest_bottom():
    eff = _parse_effect(
        "look at the top four cards of your library. You may reveal a "
        "noncreature, nonland card from among them and put it into your "
        "hand. Put the rest on the bottom of your library in a random "
        "order"
    )
    assert isinstance(eff, LookTopNMayRevealMatchingToHandRestBottomRandom)
    assert eff.n == 4
    assert eff.card_filter == (
        ~HasPrintedCardType("Creature") & ~HasPrintedCardType("Land")
    )
    one = _parse_effect(
        "look at the top five cards of your library. You may reveal an "
        "artifact card from among them and put it into your hand. Put "
        "the rest on the bottom of your library in a random order"
    )
    assert one.card_filter == HasPrintedCardType("Artifact")
    assert parse_ext_effect(
        "look at the top four cards of your library. You may reveal a "
        "legendary card from among them and put it into your hand. Put "
        "the rest on the bottom of your library in a random order"
    ) is None


def test_sorceries_as_flash():
    assert _parse_effect(
        "until your next turn, you may cast sorcery spells as though "
        "they had flash"
    ) == MayCastSorceriesAsThoughFlashUntilYourNextTurn()


def test_synthesized_stat_token():
    eff = _parse_effect("create two 1/1 white Soldier creature tokens")
    assert isinstance(eff, CreateToken)
    assert eff.count == 2
    pr = eff.token_printing
    assert pr.name == "Soldier" and pr.subtypes == ("Soldier",)
    tapped = _parse_effect(
        "create a tapped 3/3 colorless Golem artifact creature token "
        "with vigilance"
    )
    assert tapped.tapped
    assert tapped.token_printing.is_artifact
    assert "vigilance" in tapped.token_printing.keywords
    assert parse_ext_effect(
        "create a 2/2 white Knight creature token with wibble"
    ) is None


def test_registered_token_types_are_not_resynthesized():
    # "Robot" is a registered token; the registry-backed recognizer in
    # effects.py claims it, and the extension declines a variant the
    # registry path rejected.
    assert parse_ext_effect(
        "create a 2/2 colorless Robot artifact creature token"
    ) is None


def test_named_token():
    eff = _parse_effect(
        "create Boo, a legendary 1/1 red Hamster creature token with "
        "trample and haste"
    )
    pr = eff.token_printing
    assert pr.name == "Boo"
    assert "Legendary" in pr.supertypes
    assert pr.keywords == frozenset({"trample", "haste"})
    plain = _parse_effect("create Fido, a 2/2 green Dog creature token")
    assert "Legendary" not in plain.token_printing.supertypes
    assert parse_ext_effect(
        "create Fido, a 2/2 green Dog creature token with wibble"
    ) is None


def test_sacrifice_reflexive_damage():
    eff = _parse_effect(
        "sacrifice a creature. When you do, deals X damage to any "
        "target, where X is that creature's power. If the sacrificed "
        "creature was a Hamster, draw X cards"
    )
    assert eff == SacrificeCreatureThenDamageEqualToItsPower(
        draw_if_subtype="Hamster",
    )


def test_unrelated_phrase_returns_none():
    assert parse_ext_effect("frobnicate the widget") is None


# ---------------------------------------------------------------------------
# Planeswalker builder line recognizers
# ---------------------------------------------------------------------------


def _walker(oracle: str, *, name="Test Walker, the Probe", loyalty="3",
            type_line="Legendary Planeswalker — Probe"):
    return {
        "name": name, "mana_cost": "{2}{W}", "type_line": type_line,
        "loyalty": loyalty, "oracle_text": oracle,
    }


def test_loyalty_short_name_subject_is_stripped():
    eff = _parse_loyalty_effect(
        "Test deals 3 damage to any target.", "Test, the Probe",
    )
    from mtgz_ir.effects.generic import DealDamageAnyTarget
    assert eff == DealDamageAnyTarget(amount=3)


def test_unparseable_loyalty_line_fails_the_card():
    assert _build_planeswalker_printing(
        _walker("+1: Draw a card.\n−2: Frobnicate target widget.")
    ) is None
    ok = _build_planeswalker_printing(_walker("+1: Draw a card."))
    assert ok is not None and len(ok.activated_abilities) == 1


def test_instant_speed_loyalty_line_must_name_the_card():
    good = _build_planeswalker_printing(_walker(
        "As long as Test Walker, the Probe entered this turn, you may "
        "activate her loyalty abilities any time you could cast an "
        "instant.\n+1: Draw a card."
    ))
    assert good.loyalty_abilities_at_instant_speed_if_entered_this_turn
    assert _build_planeswalker_printing(_walker(
        "As long as Someone Else entered this turn, you may activate her "
        "loyalty abilities any time you could cast an instant.\n"
        "+1: Draw a card."
    )) is None


def test_commander_line_must_name_the_card():
    good = _build_planeswalker_printing(_walker(
        "+1: Draw a card.\nTest Walker, the Probe can be your commander."
    ))
    assert good is not None
    assert _build_planeswalker_printing(_walker(
        "+1: Draw a card.\nSomeone Else can be your commander."
    )) is None


def test_two_arm_trigger_rejects_unparseable_body():
    assert _build_planeswalker_printing(_walker(
        "When Test Walker, the Probe enters and at the beginning of your "
        "upkeep, frobnicate.\n+1: Draw a card."
    )) is None


def test_unparseable_static_line_fails_the_card():
    assert _build_planeswalker_printing(_walker(
        "Creatures you control can't frobnicate.\n+1: Draw a card."
    )) is None


def test_multiple_subtypes_rejected_zero_allowed():
    assert _build_planeswalker_printing(_walker(
        "+1: Draw a card.", type_line="Legendary Planeswalker — A B",
    )) is None
    no_sub = _build_planeswalker_printing(_walker(
        "+1: Draw a card.", type_line="Legendary Planeswalker",
    ))
    assert no_sub.subtypes == ()


def test_jace_minus_eight_parses_to_sequence():
    eff = _parse_loyalty_effect(
        "Draw seven cards. Then if your library has no cards in it, you "
        "win the game.", "Jace, Wielder of Mysteries",
    )
    assert isinstance(eff, Sequence)
    assert isinstance(eff.effects[1], IfCondition)


def test_mana_value_predicates_reject_players():
    from mtgz_ir.game import GameState, Player

    s = GameState()
    s.players.append(Player(id=0))
    assert not HasManaValueAtLeast(1).test(s.players[0], s)
    assert not HasManaValueAtMost(1).test(s.players[0], s)
    assert not HasPrintedCardType("Creature").test(s.players[0], s)


def test_is_enchantment_union_member_used():
    # Guard that the enchantment arm of the noun table is wired.
    assert parse_target_noun_phrase("enchantment") == IsEnchantment()
