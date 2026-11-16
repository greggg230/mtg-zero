"""Tests for batch-14 unified-effect-grammar additions:

* ``this creature gets +N/+M until end of turn`` →
  ``PumpSelfUntilEOT(power_bonus=N, toughness_bonus=M)``. Surfaces on
  Skystinger's block trigger and the Landfall self-pump family
  (Icecave Crasher +1/+0, Remnant Elemental +2/+0).
* ``another target creature you control gains <kw>[ and <kw2>...]
  until end of turn`` →
  ``PumpTargetCreatureUntilEOT(power_bonus=0, toughness_bonus=0,
  granted_keywords=..., you_control_only=True, exclude_self=True)``.
  Pure keyword-grant variant (no P/T pump). Surfaces on Blooming
  Stinger's ETB.
* ``target player mills cards equal to your life total`` →
  ``MillTargetPlayer(use_controller_life=True)``. Space-Time
  Anomaly's printed form.
* ``target player mills N cards`` →
  ``MillTargetPlayer(amount=N)``. Forward-looking bare-amount mill
  (no EoE card prints this exact shape today).
* ``each player creates a <Name> token`` →
  ``CreateToken(token_printing=..., count=N, each_player=True)``.
  Surfaces on Edge Rover's dies trigger.

Each new effect is exercised three ways: directly via
``_parse_effect`` on the normalized phrase, via
``_parse_effect_phrase`` (creature-trigger surface form), and via
``_parse_spell_effect_phrase`` (spell sentence form, capitalized
first letter and trailing period). Negative cases verify the regex
stays narrow.

Card-claim tests confirm the parser end-to-end-claims the six EoE
cards unlocked by this batch: Skystinger, Icecave Crasher, Remnant
Elemental, Blooming Stinger, Edge Rover, Space-Time Anomaly. Three
of those (Icecave Crasher, Remnant Elemental, Edge Rover, Blooming
Stinger) also required matching static-side fixes — adding the
printed creature subtypes and, for Blooming Stinger, the
``exclude_self=True`` flag implied by the printed "another"
qualifier — so the parity gate stays green.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CreateToken,
    GainLife,
    MillTargetPlayer,
    PumpSelfUntilEOT,
    PumpTargetCreatureUntilEOT,
    Sequence,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


# ---------- this creature gets +N/+M until end of turn ----------

def test_parse_effect_recognizes_pump_self_ueot_positive():
    # Skystinger's block-trigger body — source pumps itself +5/+0
    # until end of turn. Maps to ``PumpSelfUntilEOT`` with the
    # captured bonuses; ``granted_keywords`` defaults to the empty
    # frozenset since this primitive doesn't accept a keyword tail.
    assert _parse_effect(
        "this creature gets +5/+0 until end of turn"
    ) == PumpSelfUntilEOT(power_bonus=5, toughness_bonus=0)


def test_parse_effect_recognizes_pump_self_ueot_landfall_small():
    # Icecave Crasher's landfall trigger — +1/+0 self-pump. Forward-
    # symmetric with the +5/+0 form above; the regex captures both
    # bonuses uniformly so any printing using this surface parses
    # without further grammar work.
    assert _parse_effect(
        "this creature gets +1/+0 until end of turn"
    ) == PumpSelfUntilEOT(power_bonus=1, toughness_bonus=0)


def test_parse_effect_recognizes_pump_self_ueot_two_bonuses():
    # Both bonuses non-zero — no EoE card prints this exact surface
    # for the self-pump shape today (the +N/+0 forms dominate), but
    # the regex accepts mixed bonuses so a future printing that
    # self-pumps +2/+2 doesn't need a new regex slot.
    assert _parse_effect(
        "this creature gets +2/+2 until end of turn"
    ) == PumpSelfUntilEOT(power_bonus=2, toughness_bonus=2)


def test_parse_effect_recognizes_pump_self_ueot_negative():
    # Negative bonuses are accepted for symmetry with the targeted
    # ``_PUMP_TARGET_CREATURE_UEOT`` regex above (the engine class
    # folds positive and negative bonuses through the same pump
    # path). No EoE card self-debuffs today but this keeps the
    # grammar uniform.
    assert _parse_effect(
        "this creature gets -1/-1 until end of turn"
    ) == PumpSelfUntilEOT(power_bonus=-1, toughness_bonus=-1)


def test_parse_effect_phrase_recognizes_pump_self_ueot():
    # Creature-trigger surface form. The ``it`` source-subject prefix
    # isn't present here because the printed text uses the explicit
    # ``this creature`` self-reference; the normalizer leaves the
    # phrase as-is and the regex matches.
    assert _parse_effect_phrase(
        "this creature gets +1/+0 until end of turn"
    ) == PumpSelfUntilEOT(power_bonus=1, toughness_bonus=0)


def test_parse_spell_effect_phrase_recognizes_pump_self_ueot():
    # Spell sentence form. No EoE spell prints this exact bare
    # clause as a top-level effect (the targeted variant
    # ``Target creature gets ...`` is what spells use), but the
    # unified grammar makes it available across contexts so a
    # future printing where a spell self-pumps a single specific
    # creature (e.g. via a self-naming subject) parses without
    # further grammar work.
    assert _parse_spell_effect_phrase(
        "This creature gets +3/+3 until end of turn.",
        "Bogus Self-Pump Spell",
    ) == PumpSelfUntilEOT(power_bonus=3, toughness_bonus=3)


def test_parse_effect_returns_none_for_target_creature_self_pump_phrase():
    # ``target creature gets +N/+M until end of turn`` is the
    # targeted pump variant (already covered by
    # ``_PUMP_TARGET_CREATURE_UEOT``) and produces a different
    # engine class. Sanity-check that the new self-pump regex
    # doesn't accidentally claim the targeted surface — they live
    # in disjoint surface forms.
    eff = _parse_effect("target creature gets +1/+0 until end of turn")
    assert eff is not None
    assert not isinstance(eff, PumpSelfUntilEOT)


def test_parse_effect_returns_none_for_pump_self_without_ueot_tail():
    # The trailing ``until end of turn`` is required — a permanent
    # +N/+M static buff would be a different (unsupported)
    # primitive and shouldn't accidentally match the regex via a
    # leading-clause-only.
    assert _parse_effect("this creature gets +1/+0") is None


# ---------- another target creature you control gains <kw> UEOT ----------

def test_parse_effect_recognizes_grant_keyword_to_another_creature():
    # Blooming Stinger's ETB — grants deathtouch to another creature
    # you control. Maps to the bare-pump engine class with all three
    # filter flags set: ``you_control_only=True`` (printed
    # "you control"), ``exclude_self=True`` (printed "another"),
    # and ``granted_keywords={"deathtouch"}``.
    assert _parse_effect(
        "another target creature you control gains deathtouch until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"deathtouch"}),
        you_control_only=True,
        exclude_self=True,
    )


def test_parse_effect_recognizes_grant_multiple_keywords_to_another():
    # Multi-keyword body — joined by `` and `` and validated against
    # the keyword-line whitelist. No EoE card prints a multi-keyword
    # grant on this exact surface today, but the regex tail (``.+``)
    # plus the existing ``_parse_keyword_grant_body`` helper compose
    # naturally — a future printing of ``... gains first strike and
    # vigilance until end of turn`` parses without further grammar
    # work.
    assert _parse_effect(
        "another target creature you control gains "
        "first strike and vigilance until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"first strike", "vigilance"}),
        you_control_only=True,
        exclude_self=True,
    )


def test_parse_effect_phrase_recognizes_grant_keyword_to_another():
    # Creature-trigger surface form (Blooming Stinger's printed
    # line). The ``it`` source-subject prefix isn't present because
    # the consequence is targeted at another creature, not the
    # firing source.
    assert _parse_effect_phrase(
        "another target creature you control gains deathtouch until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"deathtouch"}),
        you_control_only=True,
        exclude_self=True,
    )


def test_parse_spell_effect_phrase_recognizes_grant_keyword_to_another():
    # Spell sentence form. No EoE spell prints this exact bare
    # clause today but the unified grammar makes it available to
    # spells via the same ``_parse_effect`` path used by triggered
    # abilities.
    assert _parse_spell_effect_phrase(
        "Another target creature you control gains deathtouch until end of turn.",
        "Bogus Grant Spell",
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"deathtouch"}),
        you_control_only=True,
        exclude_self=True,
    )


def test_parse_effect_returns_none_for_unknown_granted_keyword():
    # If the granted-keyword body fails the whitelist check, the
    # whole pure-grant phrase rejects so the wrapping card stays
    # unclaimed (rather than silently dropping the grant).
    # ``foobar`` is not a recognized evergreen keyword.
    assert _parse_effect(
        "another target creature you control gains foobar until end of turn"
    ) is None


def test_bare_you_control_grant_maps_to_no_exclude_self_primitive():
    # Without ``another`` the bare ``target creature you control gains
    # <kw> until end of turn`` shape (cube: Sylvan Safekeeper) parses
    # through ``parser.keyword_actions`` to the no-exclude-self
    # primitive — the source itself is a legal target.
    eff = _parse_effect(
        "target creature you control gains deathtouch until end of turn"
    )
    assert eff.you_control_only and not eff.exclude_self


def test_parse_effect_returns_none_for_grant_without_you_control():
    # The ``you control`` filter is required by this regex — the
    # bare ``another target creature gains <kw>`` shape would imply
    # a different primitive (no controller filter, possibly even
    # opponent-targeted). Rejecting it keeps the grammar narrow.
    assert _parse_effect(
        "another target creature gains deathtouch until end of turn"
    ) is None


# ---------- target player mills cards equal to your life total ----------

def test_parse_effect_recognizes_mill_target_player_equal_to_life():
    # Space-Time Anomaly's printed form. Maps to the engine class's
    # ``use_controller_life=True`` flag, which substitutes the
    # controller's current life total for the bare amount at resolve
    # time. No numeric capture — the surface is a literal match.
    assert _parse_effect(
        "target player mills cards equal to your life total"
    ) == MillTargetPlayer(use_controller_life=True)


def test_parse_effect_phrase_recognizes_mill_target_player_equal_to_life():
    # Creature-trigger surface form. No EoE creature prints a mill-
    # equal-to-your-life trigger today (Space-Time Anomaly is a
    # spell), but the unified grammar makes it available across
    # contexts.
    assert _parse_effect_phrase(
        "target player mills cards equal to your life total"
    ) == MillTargetPlayer(use_controller_life=True)


def test_parse_spell_effect_phrase_recognizes_mill_equal_to_life():
    # Spell sentence form — Space-Time Anomaly's exact printed
    # sentence (capitalized first letter, trailing period).
    assert _parse_spell_effect_phrase(
        "Target player mills cards equal to your life total.",
        "Space-Time Anomaly",
    ) == MillTargetPlayer(use_controller_life=True)


# ---------- target player mills N cards (bare amount) ----------

def test_parse_effect_recognizes_mill_target_player_n_cards_digit():
    # Forward-looking bare-amount mill — no EoE card prints
    # ``target player mills 3 cards`` today, but the regex accepts
    # bare digits so a future printing parses without further
    # grammar work.
    assert _parse_effect(
        "target player mills 3 cards"
    ) == MillTargetPlayer(amount=3)


def test_parse_effect_recognizes_mill_target_player_n_cards_word():
    # Spelled-out small cardinal — also forward-looking. Scryfall's
    # convention is to spell out 1–10 in oracle text so this is
    # the form a future printing would actually use.
    assert _parse_effect(
        "target player mills three cards"
    ) == MillTargetPlayer(amount=3)


def test_parse_effect_returns_none_for_mill_target_player_singular():
    # Singular ``mills 1 card`` / ``mills one card`` is intentionally
    # unsupported — Scryfall's convention reserves the article ``a``
    # for the singular form (``target player mills a card``), and no
    # EoE card prints that exact shape today. The regex's plural
    # ``cards`` requirement keeps the grammar narrow.
    assert _parse_effect("target player mills 1 card") is None
    assert _parse_effect("target player mills one card") is None


# ---------- each player creates a <Name> token ----------

def test_parse_effect_recognizes_each_player_creates_lander_token():
    # Edge Rover's dies-trigger body. Every living player creates
    # the Lander token under their own control via the engine
    # class's ``each_player=True`` flag. The printing is looked up
    # in ``TOKEN_PRINTINGS`` so a registry entry must exist for the
    # subtype name (Lander does — see ``cards/tokens.py``).
    assert _parse_effect(
        "each player creates a Lander token"
    ) == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"],
        count=1,
        each_player=True,
    )


def test_parse_effect_phrase_recognizes_each_player_creates_token():
    # Creature-trigger surface form (Edge Rover's actual line
    # post-reminder-stripping). The ``it`` prefix isn't present
    # because the subject is each player, not the firing source.
    assert _parse_effect_phrase(
        "each player creates a Lander token"
    ) == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"],
        count=1,
        each_player=True,
    )


def test_parse_spell_effect_phrase_recognizes_each_player_creates_token():
    # Spell sentence form. No EoE spell prints this exact bare
    # clause today but the unified grammar makes it available to
    # spells via the same ``_parse_effect`` path used by triggered
    # abilities.
    assert _parse_spell_effect_phrase(
        "Each player creates a Lander token.",
        "Bogus Each-Player Lander Spell",
    ) == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"],
        count=1,
        each_player=True,
    )


def test_parse_effect_returns_none_for_unknown_each_player_token_name():
    # Token-name not in ``TOKEN_PRINTINGS`` rejects — the parser
    # doesn't fabricate token printings on the fly. ``Bogus`` isn't
    # a registered subtype.
    assert _parse_effect("each player creates a Bogus token") is None


def test_parse_effect_returns_none_for_each_player_creates_creature_token():
    # The stat-bearing creature-token shape (``a 2/2 colorless Robot
    # artifact creature token``) is intentionally NOT extended to
    # the each-player subject in this batch — no EoE card prints
    # that surface today. Rejecting it keeps the regex narrow.
    assert _parse_effect(
        "each player creates a 2/2 colorless Robot artifact creature token"
    ) is None


def test_parse_effect_bare_create_lander_still_parses():
    # The bare ``create a Lander token`` regex still claims its own
    # surface (controller-implicit subject). Adding the each-player
    # variant doesn't disturb the simpler form.
    assert _parse_effect(
        "create a Lander token"
    ) == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"],
        count=1,
    )


# ---------- end-to-end card-claim tests ----------

_PARSER = OracleParserCardSource()


def test_parser_claims_skystinger_via_pump_self_ueot():
    # Skystinger prints as ``Reach\nWhenever this creature blocks a
    # creature with flying, this creature gets +5/+0 until end of
    # turn.`` The trigger condition was already in the predicate
    # phrasebook (batch 9); this batch adds the
    # ``PumpSelfUntilEOT`` effect-side primitive that completes the
    # claim.
    assert "Skystinger" in _PARSER.claimed_names


def test_parser_claims_icecave_crasher_via_landfall_pump_self():
    # Icecave Crasher prints as ``Trample\nLandfall — Whenever a
    # land you control enters, this creature gets +1/+0 until end of
    # turn.`` The landfall trigger condition was already in the
    # predicate phrasebook; this batch adds the ``PumpSelfUntilEOT``
    # effect-side primitive that completes the claim. Static-side
    # fix added the printed ``Beast`` subtype to keep parity.
    assert "Icecave Crasher" in _PARSER.claimed_names


def test_parser_claims_remnant_elemental_via_landfall_pump_self():
    # Remnant Elemental prints as ``Reach\nLandfall — Whenever a
    # land you control enters, this creature gets +2/+0 until end of
    # turn.`` Same shape as Icecave Crasher but +2/+0 vs +1/+0;
    # claimed by the same effect-side primitive. Static-side fix
    # added the printed ``Elemental`` subtype to keep parity.
    assert "Remnant Elemental" in _PARSER.claimed_names


def test_parser_claims_blooming_stinger_via_grant_keyword_to_another():
    # Blooming Stinger prints as ``Deathtouch\nWhen this creature
    # enters, another target creature you control gains deathtouch
    # until end of turn.`` The ETB trigger condition was already in
    # the predicate phrasebook; this batch adds the pure-grant-to-
    # another effect-side primitive that completes the claim.
    # Static-side fix added the printed ``Plant Scorpion`` subtypes
    # AND the ``exclude_self=True`` flag implied by the printed
    # "another" qualifier.
    assert "Blooming Stinger" in _PARSER.claimed_names


def test_parser_claims_edge_rover_via_each_player_create_token():
    # Edge Rover prints as ``Reach\nWhen this creature dies, each
    # player creates a Lander token.`` The dies trigger condition
    # was already in the predicate phrasebook; this batch adds the
    # ``each player creates`` token-creation primitive that
    # completes the claim. Static-side fix added the printed
    # ``Robot Scout`` subtypes AND the ``Artifact`` card type
    # (printed type line: ``Artifact Creature — Robot Scout``).
    assert "Edge Rover" in _PARSER.claimed_names


def test_parser_claims_space_time_anomaly_via_mill_equal_to_life():
    # Space-Time Anomaly prints as ``Target player mills cards
    # equal to your life total.`` — single-line spell parsed
    # straight via the new ``MillTargetPlayer(use_controller_life
    # =True)`` primitive.
    assert "Space-Time Anomaly" in _PARSER.claimed_names


def test_parser_claimed_skystinger_has_pump_self_ueot_block_trigger():
    # End-to-end shape check: the produced Skystinger printing
    # carries one triggered ability (the block trigger) whose
    # effect is exactly ``PumpSelfUntilEOT(power_bonus=5,
    # toughness_bonus=0)``. The keyword line ``Reach`` lives on
    # the printing's ``keywords`` set, not as a triggered ability.
    p = _PARSER.get_printings()["Skystinger"]
    assert "reach" in p.keywords
    assert len(p.triggered_abilities) == 1
    assert p.triggered_abilities[0].effect == PumpSelfUntilEOT(
        power_bonus=5, toughness_bonus=0,
    )


def test_parser_claimed_blooming_stinger_has_grant_to_another_etb():
    # End-to-end shape check: Blooming Stinger's ETB grants
    # deathtouch to another creature you control with all three
    # filter flags set (``you_control_only``, ``exclude_self``, and
    # the ``granted_keywords`` frozenset).
    p = _PARSER.get_printings()["Blooming Stinger"]
    assert "deathtouch" in p.keywords
    assert len(p.triggered_abilities) == 1
    assert p.triggered_abilities[0].effect == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"deathtouch"}),
        you_control_only=True,
        exclude_self=True,
    )


def test_parser_claimed_edge_rover_has_each_player_lander_dies_trigger():
    # End-to-end shape check: Edge Rover's dies trigger fires a
    # ``CreateToken(... each_player=True)`` for the Lander token —
    # matching the static printing exactly post-fix.
    p = _PARSER.get_printings()["Edge Rover"]
    assert "reach" in p.keywords
    assert len(p.triggered_abilities) == 1
    assert p.triggered_abilities[0].effect == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"],
        count=1,
        each_player=True,
    )


def test_parser_claimed_space_time_anomaly_has_mill_use_life():
    # End-to-end shape check: Space-Time Anomaly's spell effect is
    # the bare ``MillTargetPlayer(use_controller_life=True)`` engine
    # class — matching the static side exactly.
    p = _PARSER.get_printings()["Space-Time Anomaly"]
    assert p.spell_effect == MillTargetPlayer(use_controller_life=True)


# ---------- compound interactions ----------

def test_parse_compound_with_pump_self_ueot():
    # Compound effects compose with the new self-pump primitive.
    # No EoE card prints this exact compound today but the
    # ``and``-joiner path treats every primitive uniformly so a
    # future printing of ``you gain 1 life and this creature gets
    # +1/+0 until end of turn`` parses cleanly.
    assert _parse_effect(
        "you gain 1 life and this creature gets +1/+0 until end of turn"
    ) == Sequence(effects=(
        GainLife(amount=1),
        PumpSelfUntilEOT(power_bonus=1, toughness_bonus=0),
    ))
