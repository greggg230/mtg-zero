"""Tests for batch-9 unified-effect-grammar additions: ``add {<color>}``
single-color mana, ``target opponent loses N life``, two graveyard-return
shapes for creature cards (to-hand and to-battlefield), the
artifact-card graveyard-return-to-hand, and the
``target creature gets +/-N/+/-M until end of turn`` pump.

Each effect is exercised three ways: directly via ``_parse_effect`` on
the normalized phrase, via ``_parse_effect_phrase`` (creature-trigger
surface form), and via ``_parse_spell_effect_phrase`` (spell sentence
form, capitalized first letter and trailing period). This three-way
coverage verifies a single grammar addition lights up both contexts.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    AddManaToPool,
    PumpTargetCreatureUntilEOT,
    ReturnCreatureFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToHand,
    ReturnTargetArtifactCardFromYourGraveyardToHand,
    TargetOpponentLosesLife,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)


# ---------- add {<color>} ----------

def test_parse_effect_recognizes_add_single_color_mana_each_color():
    # All five colors round-trip through AddManaToPool with a single coin.
    for color in ("W", "U", "B", "R", "G"):
        assert _parse_effect(f"add {{{color}}}") == AddManaToPool(
            produces=((color, 1),)
        )


def test_parse_effect_phrase_recognizes_add_single_color_mana():
    # Creature-trigger surface form has no "it" subject — mana abilities
    # mint coins for the controller, not "the creature."
    assert _parse_effect_phrase("add {R}") == AddManaToPool(
        produces=(("R", 1),)
    )


def test_parse_spell_effect_phrase_recognizes_add_single_color_mana():
    # Spell surface form: capitalized first letter + trailing period.
    # "Add {U}." → AddManaToPool(("U", 1)).
    assert _parse_spell_effect_phrase(
        "Add {U}.", "Bogus Lotus Petal"
    ) == AddManaToPool(produces=(("U", 1),))


# ---------- target opponent loses N life ----------

def test_parse_effect_recognizes_target_opponent_loses_3_life():
    assert _parse_effect("target opponent loses 3 life") == \
        TargetOpponentLosesLife(amount=3)


def test_parse_effect_recognizes_target_opponent_loses_arbitrary_n():
    assert _parse_effect("target opponent loses 1 life") == \
        TargetOpponentLosesLife(amount=1)
    assert _parse_effect("target opponent loses 7 life") == \
        TargetOpponentLosesLife(amount=7)


def test_parse_effect_phrase_recognizes_target_opponent_loses_n_life():
    # Creature-trigger surface form (no "it" subject — the opponent
    # loses life, not the creature).
    assert _parse_effect_phrase("target opponent loses 2 life") == \
        TargetOpponentLosesLife(amount=2)


def test_parse_spell_effect_phrase_recognizes_target_opponent_loses_n_life():
    assert _parse_spell_effect_phrase(
        "Target opponent loses 3 life.", "Bogus Drain"
    ) == TargetOpponentLosesLife(amount=3)


# ---------- return target creature card from your graveyard to your hand ----

def test_parse_effect_recognizes_return_target_creature_grave_to_hand():
    assert _parse_effect(
        "return target creature card from your graveyard to your hand"
    ) == ReturnCreatureFromYourGraveyardToHand()


def test_parse_effect_phrase_recognizes_return_target_creature_grave_to_hand():
    assert _parse_effect_phrase(
        "return target creature card from your graveyard to your hand"
    ) == ReturnCreatureFromYourGraveyardToHand()


def test_parse_spell_effect_phrase_recognizes_return_creature_grave_to_hand():
    assert _parse_spell_effect_phrase(
        "Return target creature card from your graveyard to your hand.",
        "Bogus Raise Dead",
    ) == ReturnCreatureFromYourGraveyardToHand()


# ---------- return target creature card from your graveyard to battlefield --

def test_parse_effect_recognizes_return_target_creature_grave_to_battlefield():
    assert _parse_effect(
        "return target creature card from your graveyard to the battlefield"
    ) == ReturnCreatureFromYourGraveyardToBattlefield()


def test_parse_effect_phrase_recognizes_return_creature_grave_to_battlefield():
    assert _parse_effect_phrase(
        "return target creature card from your graveyard to the battlefield"
    ) == ReturnCreatureFromYourGraveyardToBattlefield()


def test_parse_spell_effect_phrase_recognizes_return_creature_to_battlefield():
    assert _parse_spell_effect_phrase(
        "Return target creature card from your graveyard to the battlefield.",
        "Bogus Reanimate",
    ) == ReturnCreatureFromYourGraveyardToBattlefield()


# ---------- return target artifact card from your graveyard to your hand ----

def test_parse_effect_recognizes_return_target_artifact_grave_to_hand():
    assert _parse_effect(
        "return target artifact card from your graveyard to your hand"
    ) == ReturnTargetArtifactCardFromYourGraveyardToHand()


def test_parse_effect_phrase_recognizes_return_target_artifact_grave_to_hand():
    assert _parse_effect_phrase(
        "return target artifact card from your graveyard to your hand"
    ) == ReturnTargetArtifactCardFromYourGraveyardToHand()


def test_parse_spell_effect_phrase_recognizes_return_artifact_grave_to_hand():
    assert _parse_spell_effect_phrase(
        "Return target artifact card from your graveyard to your hand.",
        "Bogus Scour for Scrap",
    ) == ReturnTargetArtifactCardFromYourGraveyardToHand()


# ---------- target creature gets +/-N/+/-M until end of turn ----------

def test_parse_effect_recognizes_target_creature_gets_positive_pump_ueot():
    assert _parse_effect(
        "target creature gets +3/+3 until end of turn"
    ) == PumpTargetCreatureUntilEOT(power_bonus=3, toughness_bonus=3)


def test_parse_effect_recognizes_target_creature_gets_negative_pump_ueot():
    # Negative pump folds through the same effect with negative bonuses
    # (PumpTargetCreatureUntilEOT treats sign uniformly).
    assert _parse_effect(
        "target creature gets -2/-2 until end of turn"
    ) == PumpTargetCreatureUntilEOT(power_bonus=-2, toughness_bonus=-2)


def test_parse_effect_recognizes_target_creature_gets_mixed_pump_ueot():
    # Mixed sign isn't observed in EoE today but the grammar accepts
    # it for forward-compat (no card prints +1/-1 but the engine path
    # is identical).
    assert _parse_effect(
        "target creature gets +1/-1 until end of turn"
    ) == PumpTargetCreatureUntilEOT(power_bonus=1, toughness_bonus=-1)


def test_parse_effect_phrase_recognizes_target_creature_gets_pump_ueot():
    assert _parse_effect_phrase(
        "target creature gets +2/+2 until end of turn"
    ) == PumpTargetCreatureUntilEOT(power_bonus=2, toughness_bonus=2)


def test_parse_spell_effect_phrase_recognizes_target_creature_gets_pump_ueot():
    assert _parse_spell_effect_phrase(
        "Target creature gets +1/+1 until end of turn.", "Bogus Giant Growth"
    ) == PumpTargetCreatureUntilEOT(power_bonus=1, toughness_bonus=1)


# ---------- negative cases ----------

def test_parse_effect_returns_none_for_malformed_add_mana():
    # Three-or-more colored-coin shape isn't wired — only single-coin
    # ``{<color>}`` and two-same-color ``{<color>}{<color>}`` are
    # recognized for WUBRG coins (Steelswarm Operator's largest add is
    # two colored coins). Colorless ``{C}`` repetitions ARE recognized
    # via ``_ADD_N_COLORLESS_MANA`` (The Eternity Elevator's
    # ``{T}: Add {C}{C}{C}``), but three-or-more colored coins remain
    # unsupported.
    # Three-or-more pips now claim through the card-flow grammar's
    # multi-pip recognizer (Dark Ritual's ``Add {B}{B}{B}``).
    assert _parse_effect("add {U}{U}{U}") == AddManaToPool(
        produces=(("U", 3),),
    )
    assert _parse_effect("add {U}{U}{Q}") is None
    # ``add one mana of any color`` IS recognized (All-Fates Scroll).
    # See ``test_parser_artifact_all_fates_scroll`` for the positive
    # case. Plural / amount-N shapes are not recognized:
    assert _parse_effect("add two mana of any color") is None
    assert _parse_effect("add one mana of any one color") is None
    # Bracketed amount instead of a color symbol.
    assert _parse_effect("add {1}") is None


def test_parse_effect_returns_none_for_malformed_target_opponent_loses_life():
    assert _parse_effect("target opponent loses life") is None
    assert _parse_effect("target opponent loses two life") is None
    assert _parse_effect("target opponent loses 3") is None
    # "target player" widens to the caster too: the opponent-only
    # effect can't express it, so it maps to the any-player targeted
    # action-list effect (U/B cube creatures) instead.
    from mtgz_ir.effects.generic.cube_creatures_ub import (
        TargetPlayerPerformsActions,
    )

    assert _parse_effect("target player loses 2 life") == (
        TargetPlayerPerformsActions(
            actions=(("lose_life", 2),), opponent_only=False,
        )
    )


def test_parse_effect_returns_none_for_malformed_return_creature_to_hand():
    # Wrong source zone.
    assert _parse_effect(
        "return target creature card from exile to your hand"
    ) is None
    # Wrong filter.
    assert _parse_effect(
        "return target artifact creature card from your graveyard to your hand"
    ) is None
    # Trailing junk.
    assert _parse_effect(
        "return target creature card from your graveyard to your hand tapped"
    ) is None


def test_parse_effect_returns_none_for_malformed_return_creature_to_battlefield():
    assert _parse_effect(
        "return target creature card from your graveyard to play"
    ) is None
    # Trailing junk.
    assert _parse_effect(
        "return target creature card from your graveyard to the battlefield "
        "tapped"
    ) is None


def test_parse_effect_returns_none_for_malformed_return_artifact_to_hand():
    assert _parse_effect(
        "return target artifact from your graveyard to your hand"
    ) is None  # missing "card"
    assert _parse_effect(
        "return target artifact card from your hand to your graveyard"
    ) is None


def test_parse_effect_returns_none_for_malformed_pump_ueot():
    # Missing sign — plain integers aren't valid in pump notation.
    assert _parse_effect(
        "target creature gets 1/1 until end of turn"
    ) is None
    # Missing the "until end of turn" duration.
    assert _parse_effect("target creature gets +1/+1") is None
    # Wrong subject.
    assert _parse_effect(
        "each creature gets +1/+1 until end of turn"
    ) is None
    # Trailing junk after the duration.
    assert _parse_effect(
        "target creature gets +1/+1 until end of turn and gains flying"
    ) is None


def test_parse_spell_effect_phrase_returns_none_for_malformed_phrase():
    assert _parse_spell_effect_phrase(
        "Add ten mana of any color.", "Bogus"
    ) is None
    assert _parse_spell_effect_phrase(
        "Target opponent loses the game.", "Bogus"
    ) is None
    assert _parse_spell_effect_phrase(
        "Return target creature card from exile to your hand.", "Bogus"
    ) is None
