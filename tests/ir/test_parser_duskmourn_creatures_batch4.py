"""Tests for the fourth Duskmourn creature parser batch.

This batch grows the parser to claim a new cluster of Duskmourn
creatures by layering small additive primitives on top of the existing
engine surface — no new events or core data fields are required beyond
two trailing-rider gates on ``StaticAbility``.

* **``requires_delirium`` static gate** + **static ability-word prefix
  stripper** — Wildfire Wickerfolk (``{R}{G}`` 3/2 Artifact Creature —
  Scarecrow). Prints ``Haste`` plus
  ``Delirium — This creature gets +1/+1 and has trample as long as
  there are four or more card types among cards in your graveyard.``
  The ``Delirium —`` prefix carries no rules consequence (CR 207.2c)
  so the new ``_strip_static_ability_word_prefix`` helper drops it
  before the trailing static grammar dispatches; the ``as long as``
  tail then maps to ``requires_delirium=True`` via a new
  ``_STATIC_CONDITION_KWARGS`` entry. The engine's
  ``StaticAbility.applies_to`` evaluates the gate by walking the
  controller's graveyard and counting distinct card types — same
  semantics as the trigger-side ``StateBound(Delirium())`` predicate.
"""

# mtg-zero (docs/ENGINE.md 170): 7 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_look_at_top_n_counter_on_miss_false_skips_counter_branch, test_return_creature_from_graveyard_max_mv_filters_legal_targets, test_return_creature_from_graveyard_max_mv_zero_unbounded, test_static_delirium_gate_active_at_threshold, test_static_delirium_gate_ignores_opponent_graveyard, test_static_delirium_gate_inactive_below_threshold, test_static_delirium_gate_requires_state_object

from __future__ import annotations

from mtgz_ir.cards.base import StaticAbility, StaticScope
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.statics import (
    _parse_static_ability_line,
    _strip_static_ability_word_prefix,
)
from mtgz_ir.effects.conditions import DeliriumActive


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- ``_strip_static_ability_word_prefix`` ----------


def test_strip_static_ability_word_prefix_strips_delirium():
    # The canonical use case: a ``Delirium — This creature gets ...``
    # static line. The ability-word prefix peels off and the body is
    # returned unchanged so the existing trailing-static grammar can
    # handle it.
    stripped = _strip_static_ability_word_prefix(
        "Delirium — This creature gets +1/+1 and has trample as long "
        "as there are four or more card types among cards in your "
        "graveyard."
    )
    assert stripped == (
        "This creature gets +1/+1 and has trample as long as there are "
        "four or more card types among cards in your graveyard."
    )


def test_strip_static_ability_word_prefix_strips_anthem_body():
    # Forward-looking grammar coverage for an ability-word + anthem-
    # body printing (no DSK card today, but the regex permits the
    # combination because the body matcher includes the
    # ``Creatures you control`` family). The stripper drops the prefix
    # so the body parses through ``_parse_anthem_static_line``.
    stripped = _strip_static_ability_word_prefix(
        "Delirium — Creatures you control get +1/+1."
    )
    assert stripped == "Creatures you control get +1/+1."


def test_strip_static_ability_word_prefix_leaves_trigger_lines_alone():
    # The static stripper deliberately doesn't peel a prefix from a
    # ``When``/``Whenever`` body — that family is the trigger-layer
    # ``_strip_ability_word_prefix``'s job. Returning the line unchanged
    # here lets the static dispatcher fail back to the trigger path on
    # the original surface form.
    line = "Delirium — Whenever this creature attacks, draw a card."
    assert _strip_static_ability_word_prefix(line) == line


def test_strip_static_ability_word_prefix_no_prefix_unchanged():
    # A static line with no leading ``<Word> — `` ability-word prefix
    # passes through untouched so non-Delirium statics keep their
    # exact surface form into the grammar.
    line = "This creature gets +1/+1 as long as it's your turn."
    assert _strip_static_ability_word_prefix(line) == line


# ---------- ``requires_delirium`` static gate ----------










# ---------- end-to-end static-line parse ----------


def test_parse_delirium_trailing_static_line():
    # End-to-end through ``_parse_static_ability_line``. The
    # ``Delirium —`` ability-word prefix strips through the new
    # ``_strip_static_ability_word_prefix`` helper, then the trailing
    # ``as long as <gate>`` shape claims with the new
    # ``requires_delirium`` kwarg from the gate phrasebook.
    # PR-D continuous-effects shape: P/T contribution rides on a
    # layer-7c ContinuousModifier(PowerToughnessBonus); the keyword
    # rides on a layer-6 ContinuousModifier(GrantKeyword); compound
    # statics fan out into one StaticAbility per ContinuousModifier.
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, GrantKeyword, PowerToughnessBonus,
    )
    parsed = _parse_static_ability_line(
        "Delirium — This creature gets +1/+1 and has trample as long "
        "as there are four or more card types among cards in your "
        "graveyard."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(
                    power_bonus=1, toughness_bonus=1,
                ),
            ),
            condition=DeliriumActive(),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("trample"),
            ),
            condition=DeliriumActive(),
        ),
    )


def test_parse_static_line_unrecognized_gate_after_delirium_prefix():
    # An unrecognized ``as long as`` gate phrase still rejects the
    # whole line even with the ``Delirium —`` prefix stripped — same
    # don't-silently-drop-the-gate discipline every other condition
    # arm uses. The stripper's job is purely surface peeling; the
    # gate phrasebook is the single source of truth for which
    # conditions are claimed.
    parsed = _parse_static_ability_line(
        "Delirium — This creature gets +1/+1 as long as the moon is full."
    )
    assert parsed == ()


# ---------- end-to-end claim ----------


def test_wildfire_wickerfolk_claimed_by_parser():
    # Wildfire Wickerfolk ``{R}{G}`` 3/2 Artifact Creature — Scarecrow.
    # Two oracle lines: ``Haste`` and
    # ``Delirium — This creature gets +1/+1 and has trample as long as
    # there are four or more card types among cards in your graveyard.``
    # Claims via the existing keyword-line grammar for the first line
    # plus the new ability-word + Delirium-static gate combo for the
    # second.
    assert "Wildfire Wickerfolk" in _claimed_names()


def test_wildfire_wickerfolk_carries_delirium_static():
    # The claimed printing must actually carry the
    # ``requires_delirium=True`` field on its static ability so the
    # engine gates the +1/+1 / trample bonus on the four-card-types
    # threshold rather than applying it unconditionally. PR-D
    # continuous-effects shape: compound P/T + keyword statics fan out
    # into two StaticAbility instances, both gated by requires_delirium.
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, GrantKeyword, PowerToughnessBonus,
    )
    src = OracleParserCardSource()
    ww = src.get_printings()["Wildfire Wickerfolk"]
    assert "haste" in ww.keywords
    assert len(ww.static_abilities) == 2
    pt_static, kw_static = ww.static_abilities
    assert pt_static.continuous_effect == ContinuousModifier(
        layer=7,
        payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=1),
    )
    assert pt_static.condition == DeliriumActive()
    assert kw_static.continuous_effect == ContinuousModifier(
        layer=6, payload=GrantKeyword("trample"),
    )
    assert kw_static.condition == DeliriumActive()


# ---------- ``basic_land`` SearchLibraryForCard filter ----------


def test_search_library_for_card_basic_land_filter_matches_basics():
    # ``filter_name="basic_land"`` matches every basic land printing —
    # Plains / Island / Swamp / Mountain / Forest. Belt-and-suspenders
    # that the new generic filter covers the full set rather than
    # accidentally pinning a single basic by name.
    from mtgz_ir.cards import BASIC_LANDS
    from mtgz_ir.effects.generic import SearchLibraryForCard
    eff = SearchLibraryForCard(filter_name="basic_land")
    for basic in ("Plains", "Island", "Swamp", "Mountain", "Forest"):
        assert eff._matches(BASIC_LANDS[basic]) is True


def test_search_library_for_card_basic_land_filter_rejects_nonbasic():
    # The ``basic_land`` filter must NOT match a non-basic land
    # (Terramorphic Expanse) or a non-land (Wild Bear). Pinning the
    # rejection here keeps a future regression where ``basic_land`` got
    # widened to ``is_land`` from going unnoticed.
    from mtgz_ir.cards import SYNTHETIC_CREATURES
    from mtgz_ir.cards.parser.source import OracleParserCardSource
    from mtgz_ir.effects.generic import SearchLibraryForCard
    eff = SearchLibraryForCard(filter_name="basic_land")
    printings = OracleParserCardSource().get_printings()
    if "Terramorphic Expanse" in printings:
        assert eff._matches(printings["Terramorphic Expanse"]) is False
    assert eff._matches(SYNTHETIC_CREATURES["Wild Bear"]) is False


def test_parse_search_library_for_basic_land_card_phrase():
    # End-to-end through the effect-phrase parser. The phrase
    # ``search your library for a basic land card, reveal it, put it
    # into your hand, then shuffle`` parses to
    # ``SearchLibraryForCard(filter_name="basic_land", may_fail=True)``.
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    from mtgz_ir.effects.generic import SearchLibraryForCard
    parsed = _parse_effect_phrase(
        "search your library for a basic land card, reveal it, "
        "put it into your hand, then shuffle"
    )
    assert parsed == SearchLibraryForCard(
        filter_name="basic_land", may_fail=True,
    )


def test_spineseeker_centipede_claimed_by_parser():
    # Spineseeker Centipede ``{2}{G}`` 2/1 Insect. Two oracle lines:
    # ``When this creature enters, search your library for a basic
    # land card, reveal it, put it into your hand, then shuffle.``
    # and ``Delirium — This creature gets +1/+2 and has vigilance as
    # long as there are four or more card types among cards in your
    # graveyard.`` Claims via the existing ETB-trigger grammar fed by
    # the new ``basic_land`` SearchLibraryForCard filter, plus the new
    # Delirium static gate from the Wildfire Wickerfolk slice.
    assert "Spineseeker Centipede" in _claimed_names()


def test_spineseeker_centipede_carries_basic_land_etb_and_delirium_static():
    # Belt-and-suspenders that the claimed printing actually wires the
    # new ``basic_land`` filter into its ETB trigger and the
    # ``requires_delirium=True`` field into its static — otherwise the
    # parser could mis-claim the card with a different effect that
    # passed the surface-text gate. PR-D continuous-effects shape:
    # compound P/T + keyword statics fan out into two StaticAbility
    # instances.
    from mtgz_ir.effects.generic import SearchLibraryForCard
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, GrantKeyword, PowerToughnessBonus,
    )
    src = OracleParserCardSource()
    sc = src.get_printings()["Spineseeker Centipede"]
    assert len(sc.triggered_abilities) == 1
    trig = sc.triggered_abilities[0]
    assert trig.effect == SearchLibraryForCard(
        filter_name="basic_land", may_fail=True,
    )
    assert len(sc.static_abilities) == 2
    pt_static, kw_static = sc.static_abilities
    assert pt_static.continuous_effect == ContinuousModifier(
        layer=7,
        payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=2),
    )
    assert pt_static.condition == DeliriumActive()
    assert kw_static.continuous_effect == ContinuousModifier(
        layer=6, payload=GrantKeyword("vigilance"),
    )
    assert kw_static.condition == DeliriumActive()


# ---------- ``ReturnCreatureFromYourGraveyardToHand`` max_mv ----------






def test_return_creature_from_graveyard_max_mv_describe():
    # The describe output spells out the mana-value cap so log messages
    # disambiguate the narrowed reanimator from the unbounded sibling.
    from mtgz_ir.effects.generic import ReturnCreatureFromYourGraveyardToHand
    eff = ReturnCreatureFromYourGraveyardToHand(max_mv=3)
    assert eff.describe() == (
        "return target creature card with mana value 3 or less from "
        "your graveyard to your hand"
    )


def test_parse_return_creature_from_graveyard_max_mv_phrase():
    # End-to-end through the effect-phrase parser. The phrase
    # ``return target creature card with mana value 3 or less from your
    # graveyard to your hand`` parses to the new max_mv=3 variant.
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    from mtgz_ir.effects.generic import ReturnCreatureFromYourGraveyardToHand
    parsed = _parse_effect_phrase(
        "return target creature card with mana value 3 or less from "
        "your graveyard to your hand"
    )
    assert parsed == ReturnCreatureFromYourGraveyardToHand(max_mv=3)


def test_savior_of_the_small_claimed_by_parser():
    # Savior of the Small ``{3}{W}`` 3/4 Kor Survivor. Single oracle
    # line: ``Survival — At the beginning of your second main phase,
    # if this creature is tapped, return target creature card with
    # mana value 3 or less from your graveyard to your hand.`` Claims
    # via the existing Survival ability-word + post-combat-main
    # trigger plumbing, fed by the new max_mv variant of the
    # graveyard-to-hand reanimator.
    assert "Savior of the Small" in _claimed_names()


def test_savior_of_the_small_carries_max_mv_reanimator():
    # The claimed printing must actually carry the ``max_mv=3`` field
    # on its triggered effect — otherwise a regression that dropped the
    # MV rider would silently widen the legal-target pool to the full
    # graveyard rather than just creatures with mana value 3 or less.
    from mtgz_ir.effects.generic import ReturnCreatureFromYourGraveyardToHand
    src = OracleParserCardSource()
    s = src.get_printings()["Savior of the Small"]
    assert len(s.triggered_abilities) == 1
    trig = s.triggered_abilities[0]
    assert trig.effect == ReturnCreatureFromYourGraveyardToHand(max_mv=3)


# ---------- ``LookAtTopNRevealOrCounter`` max_power + counter_on_miss ----------


def test_look_at_top_n_creature_with_max_power_filter_excludes_strong():
    # ``filter_name="creature"`` + ``max_power=2`` matches a creature
    # printing whose printed power is at most 2 and rejects one with
    # higher power. Mirrors the pattern's narrowing for Living Phone's
    # ``creature card with power 2 or less`` clause.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects.generic import LookAtTopNRevealOrCounter
    eff = LookAtTopNRevealOrCounter(
        n=5, filter_name="creature", max_power=2, counter_on_miss=False,
    )
    small = make_creature("Smol", "{1}{G}", 2, 2)
    big = make_creature("Hench", "{3}{G}", 4, 4)
    assert eff._matches(small) is True
    assert eff._matches(big) is False


def test_look_at_top_n_creature_with_max_power_zero_no_cap():
    # ``max_power=0`` (the default) keeps the existing Pulsar Squadron
    # Ace behaviour where every creature card qualifies regardless of
    # power. Belt-and-suspenders against a future regression that
    # flipped the sentinel.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects.generic import LookAtTopNRevealOrCounter
    eff = LookAtTopNRevealOrCounter(n=5, filter_name="creature")
    big = make_creature("Hench", "{3}{G}", 4, 4)
    assert eff._matches(big) is True




def test_look_at_top_n_describe_with_max_power_no_counter():
    # The describe output spells out the power threshold and omits the
    # counter clause when ``counter_on_miss=False``. Forward-looking
    # log readability check.
    from mtgz_ir.effects.generic import LookAtTopNRevealOrCounter
    eff = LookAtTopNRevealOrCounter(
        n=5, filter_name="creature", max_power=2, counter_on_miss=False,
    )
    desc = eff.describe()
    assert "creature card with power 2 or less" in desc
    assert "+1/+1 counter" not in desc


def test_parse_living_phone_dies_phrase():
    # End-to-end through the effect-phrase parser. The full three-
    # sentence body parses to a ``LookAtTopNRevealOrCounter`` carrying
    # the new ``max_power`` and ``counter_on_miss=False`` fields —
    # distinct from Pulsar Squadron Ace's bespoke counter-rider variant.
    from mtgz_ir.cards.parser.effects import _parse_effect_phrase
    from mtgz_ir.effects.generic import LookAtTopNRevealOrCounter
    parsed = _parse_effect_phrase(
        "look at the top five cards of your library. You may reveal a "
        "creature card with power 2 or less from among them and put it "
        "into your hand. Put the rest on the bottom of your library in "
        "a random order"
    )
    assert parsed == LookAtTopNRevealOrCounter(
        n=5, filter_name="creature", max_power=2, counter_on_miss=False,
    )


def test_living_phone_claimed_by_parser():
    # Living Phone ``{2}{W}`` 2/1 Artifact Creature — Toy. Single oracle
    # line: ``When this creature dies, look at the top five cards of
    # your library. You may reveal a creature card with power 2 or less
    # from among them and put it into your hand. Put the rest on the
    # bottom of your library in a random order.`` Claims via the
    # existing dies-trigger grammar fed by the new max_power +
    # counter_on_miss=False variant of LookAtTopNRevealOrCounter.
    assert "Living Phone" in _claimed_names()


def test_living_phone_carries_max_power_no_counter_effect():
    # Belt-and-suspenders that the claimed printing actually wires the
    # narrow ``max_power=2`` filter and the ``counter_on_miss=False``
    # flag — the difference from Pulsar Squadron Ace's variant. A
    # regression that defaulted ``counter_on_miss`` back to True would
    # silently add a +1/+1 counter to the dying source on miss, which
    # Living Phone's printed text doesn't include.
    from mtgz_ir.effects.generic import LookAtTopNRevealOrCounter
    src = OracleParserCardSource()
    lp = src.get_printings()["Living Phone"]
    assert len(lp.triggered_abilities) == 1
    trig = lp.triggered_abilities[0]
    assert trig.effect == LookAtTopNRevealOrCounter(
        n=5, filter_name="creature", max_power=2, counter_on_miss=False,
    )
