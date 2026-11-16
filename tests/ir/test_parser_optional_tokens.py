"""Tests for the generalized token-creation grammar and the
``you may pay {N}. If you do, …`` optional-mana-cost wrapper.

Replaces the narrow ``_CREATE_TAPPED_2_2_ROBOT`` slice (which only
matched one exact phrase) with a parameterized matcher covering every
EoE token-creation surface form that resolves to a registered
``TOKEN_PRINTINGS`` entry. Also exercises the ``OptionalManaCostThenEffect``
wrapper, the ``LoseLife`` primitive, and verifies that newly-claimed
EoE creatures (Galactic Wayfarer, Beamsaw Prospector, Knight Luminary,
Oreplate Pangolin, Seedship Agrarian) match their static counterparts.

Each effect is exercised through the unified ``_parse_effect`` entry
point — the per-context wrappers (``_parse_effect_phrase`` for creature
triggers and ``_parse_spell_effect_phrase`` for spells) are covered for
the most behaviorally-distinct shapes to confirm the grammar is reachable
from both contexts. The Gravpack-Monoist exact phrase is re-checked to
make sure the parameterized matcher hasn't regressed the original
2/2 colorless Robot tapped slice.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import (
    CreateToken,
    DrawCards,
    LoseLife,
    OptionalManaCostThenEffect,
    PutPlusOneCounterOnSelf,
    Sequence,
)
from mtgz_ir.cards.parser.effects import (
    _parse_create_token_effect,
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
    _parse_token_count,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


# ---------- generalized token-creation: stat-bearing creature shape ----------

@pytest.mark.parametrize(
    "phrase,token_name,count,tapped",
    [
        # Gravpack Monoist — the exact slice the old narrow regex
        # covered. Must still parse identically post-generalization.
        (
            "create a tapped 2/2 colorless Robot artifact creature token",
            "Robot", 1, True,
        ),
        # Untapped Robot (Mechan Assembler / Wedgelight Rammer surface).
        (
            "create a 2/2 colorless Robot artifact creature token",
            "Robot", 1, False,
        ),
        # White Human Soldier (Honored Knight-Captain / Knight Luminary /
        # Rayblade Trooper). Two-word subtype.
        (
            "create a 1/1 white Human Soldier creature token",
            "Human Soldier", 1, False,
        ),
        # Two-token count, spelled out (Cosmogrand Zenith).
        (
            "create two 1/1 white Human Soldier creature tokens",
            "Human Soldier", 2, False,
        ),
        # Two-token count, spelled out, colorless creature (Thrumming
        # Hivepool's upkeep trigger).
        (
            "create two 1/1 colorless Sliver creature tokens",
            "Sliver", 2, False,
        ),
        # Digit count works too (no EoE card prints "create 3 tokens"
        # today, but the count slot accepts both spellings via
        # ``_TOKEN_COUNT_PATTERN``).
        (
            "create 3 1/1 colorless Sliver creature tokens",
            "Sliver", 3, False,
        ),
    ],
)
def test_parse_creature_token_stat_bearing_shape(
    phrase, token_name, count, tapped,
):
    assert _parse_effect(phrase) == CreateToken(
        token_printing=TOKEN_PRINTINGS[token_name],
        count=count, tapped=tapped,
    )


# ---------- generalized token-creation: bare-name shape ----------

@pytest.mark.parametrize(
    "phrase,token_name,count,tapped",
    [
        # Lander single (Galactic Wayfarer / Beamsaw Prospector /
        # Seedship Agrarian and many others).
        ("create a Lander token", "Lander", 1, False),
        # Lander plural (Terrapact Intimidator's two-Lander mode).
        ("create two Lander tokens", "Lander", 2, False),
        # Tapped slot is reserved for forward-compat — no EoE card
        # currently prints a tapped Lander, but the regex accepts it.
        ("create a tapped Lander token", "Lander", 1, True),
    ],
)
def test_parse_bare_name_token_shape(
    phrase, token_name, count, tapped,
):
    assert _parse_effect(phrase) == CreateToken(
        token_printing=TOKEN_PRINTINGS[token_name],
        count=count, tapped=tapped,
    )


# ---------- token-creation: negative cases ----------

def test_parse_creature_token_unknown_subtype_is_synthesized():
    # A stat-bearing creature token whose subtype isn't registered in
    # TOKEN_PRINTINGS is built from its printed characteristics
    # (``parser.keyword_actions._synthesize_creature_token_effect``,
    # CR 111.4). An unmodeled rider still refuses the parse.
    eff = _parse_effect("create a 4/4 red Dragon creature token")
    assert eff.token_printing.name == "Dragon"
    assert (eff.token_printing.power, eff.token_printing.toughness) == (4, 4)
    assert eff.token_printing.color_indicator == frozenset({"R"})
    assert _parse_effect(
        "create a 4/4 red Dragon creature token with banding"
    ) is None


def test_parse_bare_name_token_unknown_returns_none():
    # Same negative as above, but for the bare-name shape — a name
    # that isn't a registered token printing fails the lookup and
    # the parse returns None.
    assert _parse_effect("create a Wibble token") is None


def test_parse_creature_token_malformed_shape_returns_none():
    # No N/M, no color, no creature-token tail — the regex requires
    # the full surface form. A ``create … token`` line that doesn't
    # match either shape returns None so the wrapping card stays
    # unclaimed (and a deliberate static-side bespoke effect fills
    # the gap if needed).
    assert _parse_effect("create a fancy artifact thing") is None


def test_parse_create_token_helper_returns_none_for_non_create_phrase():
    # The helper short-circuits cleanly when the phrase doesn't even
    # start with ``create`` — used by callers that try multiple
    # grammar branches in sequence.
    assert _parse_create_token_effect("draw a card") is None


# ---------- token-count helper ----------

@pytest.mark.parametrize(
    "token,expected",
    [("a", 1), ("an", 1), ("1", 1), ("3", 3), ("two", 2), ("ten", 10)],
)
def test_parse_token_count_round_trip(token, expected):
    # ``a`` / ``an`` collapse to 1 alongside digit and spelled-out
    # cardinal forms — covers every shape the count slot accepts.
    assert _parse_token_count(token) == expected


# ---------- per-context wrappers light up generalized token grammar ----------

def test_parse_effect_phrase_recognizes_generalized_token():
    # Creature-trigger surface form: no "it" subject (token creation
    # is always a controller-side effect, not a creature-side one).
    assert _parse_effect_phrase(
        "create a 1/1 white Human Soldier creature token"
    ) == CreateToken(
        token_printing=TOKEN_PRINTINGS["Human Soldier"], count=1,
    )


def test_parse_spell_effect_phrase_recognizes_generalized_token():
    # Spell surface form: capitalized first letter + trailing period.
    assert _parse_spell_effect_phrase(
        "Create a Lander token.", name="Bogus Ramp Sorcery",
    ) == CreateToken(token_printing=TOKEN_PRINTINGS["Lander"], count=1)


# ---------- you may pay {N}. If you do, … ----------

def test_parse_effect_recognizes_oreplate_pangolin_inner_phrase():
    # Real Oreplate Pangolin trigger payload: "you may pay {1}. If you
    # do, put a +1/+1 counter on this creature." The parser wraps the
    # PutPlusOneCounterOnSelf primitive in OptionalManaCostThenEffect.
    assert _parse_effect(
        "you may pay {1}. If you do, put a +1/+1 counter on this creature"
    ) == OptionalManaCostThenEffect(
        cost_str="{1}",
        inner_effect=PutPlusOneCounterOnSelf(amount=1),
    )


def test_parse_effect_recognizes_multi_color_cost():
    # Mana cost can be multi-coin — the regex accepts any sequence
    # of braced glyphs concatenated. ``{1}{R}`` is what
    # ``parse_mana_cost`` expects at resolve time.
    assert _parse_effect(
        "you may pay {1}{R}. If you do, draw a card"
    ) == OptionalManaCostThenEffect(
        cost_str="{1}{R}",
        inner_effect=DrawCards(amount=1),
    )


def test_parse_effect_optional_mana_cost_with_unparseable_inner_returns_none():
    # If the inner-effect phrase doesn't parse, the whole "you may pay"
    # line rejects — silently dropping the consequence would change
    # game behavior, so the parser declines and lets the card stay
    # unclaimed (or land on a bespoke static effect).
    assert _parse_effect(
        "you may pay {2}. If you do, undertake a complex bespoke ritual"
    ) is None


def test_parse_effect_phrase_recognizes_optional_mana_cost():
    # Creature-trigger surface form (Oreplate Pangolin's actual
    # parsed effect text after the trigger pattern strips the
    # ``Whenever ..., `` and trailing period).
    assert _parse_effect_phrase(
        "you may pay {1}. If you do, put a +1/+1 counter on this creature"
    ) == OptionalManaCostThenEffect(
        cost_str="{1}",
        inner_effect=PutPlusOneCounterOnSelf(amount=1),
    )


def test_parse_spell_effect_phrase_recognizes_optional_mana_cost():
    # Spell surface form: capitalized leading "You" gets lowercased,
    # trailing outer period stripped — the inner sentence-boundary
    # period stays as a literal match. Synthetic phrasing because no
    # EoE instant/sorcery prints this exact shape today (Kavaron
    # Harrier is a creature trigger, not a spell).
    assert _parse_spell_effect_phrase(
        "You may pay {2}. If you do, draw a card.",
        name="Bogus Optional Spell",
    ) == OptionalManaCostThenEffect(
        cost_str="{2}",
        inner_effect=DrawCards(amount=1),
    )


# ---------- LoseLife primitive ----------

@pytest.mark.parametrize("phrase,amount", [
    # Bare form ("you" carried over from a preceding compound piece —
    # how Decode Transmissions / Voidforged Titan / Hymn of the Faller
    # print the lose-life half of their compound effects).
    ("lose 1 life", 1),
    ("lose 2 life", 2),
    # Explicit-subject form (parallel to "you gain N life").
    ("you lose 1 life", 1),
    ("you lose 5 life", 5),
])
def test_parse_effect_recognizes_lose_n_life(phrase, amount):
    assert _parse_effect(phrase) == LoseLife(amount=amount)


def test_compound_draw_and_lose_life_parses_to_sequence():
    # The most common EoE compound shape that needs LoseLife:
    # "you draw a card and lose 1 life" (Elegy Acolyte) — the
    # compound combinator splits on " and ", and each half lands on
    # its own primitive. Note: ``draw a card`` (no "you") is the
    # form the existing _DRAW_A_CARD regex accepts; the ``you draw``
    # variant isn't in this slice's scope.
    assert _parse_effect(
        "draw a card and lose 1 life"
    ) == Sequence(effects=(
        DrawCards(amount=1), LoseLife(amount=1),
    ))


# ---------- card-level claim tests for newly-unlocked EoE creatures ----------

# Build the parser/static sources once at module import — every
# parametrized claim test reads from these dicts. Mirrors the
# top-level structural-parity test setup so the per-card assertions
# stay cheap.
_PARSER = OracleParserCardSource()
_PARSER_PRINTINGS = _PARSER.get_printings()


