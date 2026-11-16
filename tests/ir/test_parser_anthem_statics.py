"""Tests for the anthem-shape static-ability grammar.

The static-ability parser also handles permanents whose static abilities
target *other* creatures (not the source itself). The recognized surface
shapes are:

* ``Creatures you control get +N/+M[ and have <keyword>].``
* ``Other creatures you control get +N/+M[ and have <keyword>].``
* ``Creatures you control have <keyword>.``
* ``Other creatures you control have <keyword>.``
* Any of the above prefixed with ``During your turn, `` to set the
  ``active_turn_only`` flag (Memorial Team Leader's surface shape).

The new EoE card unlocked end-to-end by this grammar is **Memorial
Team Leader** (``During your turn, other creatures you control get
+1/+0.``). Tannuk, Steadfast Second is in the same family but has an
unmodeled ``Artifact cards and red creature cards in your hand have
warp {2}{R}.`` second clause that the parser deliberately can't claim,
so the structural-parity gate skips it; that's covered by an explicit
negative test below.

PR D continuous-effects shape (full anthem migration): the parser now
returns a ``tuple[StaticAbility, ...]``. Pure P/T anthems produce a
1-tuple carrying a layer-7c ``ContinuousModifier(PowerToughnessBonus)``;
single-keyword anthems produce a 1-tuple carrying a layer-6
``ContinuousModifier(GrantKeyword)``; multi-keyword and compound
P/T+keyword anthems fan out into multiple instances. The legacy
``granted_keywords`` field is empty on migrated statics.
"""

from __future__ import annotations

import pytest

from mtgz_ir.cards.base import StaticAbility, StaticScope
from mtgz_ir.cards.parser.statics import (
    _ANTHEM_KEYWORD_LINE,
    _ANTHEM_PT_LINE,
    _parse_anthem_static_line,
    _parse_static_ability_line,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.conditions import DuringYourTurn
from mtgz_ir.effects.modifiers import (
    ContinuousModifier,
    GrantKeyword,
    PowerToughnessBonus,
)


# ---------- Anthem P/T-only happy paths ----------


def test_inclusive_anthem_pt_only_parses_to_static():
    # ``Creatures you control get +N/+M.`` — inclusive subject, no
    # ``Other``, no keyword tail. Maps to CREATURES_YOU_CONTROL.
    # PR-D shape: pure P/T anthems are a layer-7
    # ContinuousModifier(PowerToughnessBonus) carried on a single
    # StaticAbility. The legacy power_bonus/toughness_bonus fields
    # default to 0 and the bonus is contributed by
    # ``_static_bonuses``'s layer-7c walker.
    parsed = _parse_static_ability_line(
        "Creatures you control get +1/+1."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=1),
            ),
        ),
    )


def test_exclusive_anthem_pt_only_parses_to_static():
    # ``Other creatures you control get +N/+M.`` — exclusive subject
    # ("Other") maps to OTHER_CREATURES_YOU_CONTROL. Same PR-D
    # ContinuousModifier shape as the inclusive case.
    parsed = _parse_static_ability_line(
        "Other creatures you control get +2/+1."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(power_bonus=2, toughness_bonus=1),
            ),
        ),
    )


def test_anthem_pt_with_keyword_tail():
    # ``... get +N/+M and have <keyword>.`` — combined P/T + keyword
    # grant. The plural ``have`` (vs. self-scope's ``has``) is what
    # distinguishes the anthem grammar from the self-scope grammar.
    # PR-D shape: P/T rides on a layer-7c ContinuousModifier on the
    # first StaticAbility, the keyword grant rides on a layer-6
    # ContinuousModifier(GrantKeyword) on a second StaticAbility —
    # same scope/active_turn_only.
    parsed = _parse_static_ability_line(
        "Creatures you control get +1/+1 and have trample."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=1),
            ),
        ),
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("trample"),
            ),
        ),
    )


def test_anthem_pt_with_multiword_keyword_tail():
    # Multi-word evergreens (``first strike``, ``double strike``) must
    # round-trip through ``str.capitalize()`` to find their
    # ``_KEYWORD_LINES`` entry — same convention as the self-scope
    # grammar. Same compound-anthem PR-D shape: layer-7 P/T plus
    # layer-6 GrantKeyword in two StaticAbility instances.
    parsed = _parse_static_ability_line(
        "Other creatures you control get +0/+1 and have first strike."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(power_bonus=0, toughness_bonus=1),
            ),
        ),
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("first strike"),
            ),
        ),
    )


# ---------- Anthem keyword-only happy paths ----------


def test_anthem_keyword_only_parses_to_static():
    # ``Other creatures you control have <keyword>.`` — Tannuk shape,
    # no P/T component. PR-D shape: a single StaticAbility carrying a
    # layer-6 ContinuousModifier(GrantKeyword).
    parsed = _parse_static_ability_line(
        "Other creatures you control have haste."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("haste"),
            ),
        ),
    )


def test_anthem_keyword_only_inclusive_subject():
    # The inclusive subject also accepts the keyword-only shape — the
    # two regex variants share the same subject alternation.
    parsed = _parse_static_ability_line(
        "Creatures you control have flying."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("flying"),
            ),
        ),
    )


def test_anthem_multi_keyword_fans_out_into_multiple_statics():
    # Multi-keyword anthem (Thrumming Hivepool's
    # ``Slivers you control have double strike and haste.``) splits
    # into one StaticAbility per keyword — each carrying a layer-6
    # GrantKeyword payload. The shared scope is duplicated across
    # the emitted instances; ``applies_to`` therefore fires on each
    # one consistently.
    parsed = _parse_static_ability_line(
        "Slivers you control have double strike and haste."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.SLIVERS_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("double strike"),
            ),
        ),
        StaticAbility(
            scope=StaticScope.SLIVERS_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("haste"),
            ),
        ),
    )


# ---------- ``During your turn, `` rider ----------


def test_during_your_turn_pt_anthem_sets_active_turn_only():
    # Memorial Team Leader's exact oracle line. The leading ``During
    # your turn, `` rider sets ``active_turn_only=True``; the rest of
    # the parse proceeds as if the rider weren't present (subject
    # capitalization shifts to lowercase, which the regex tolerates
    # via the ``[Oo]`` character class). Pure P/T anthem → single
    # StaticAbility carrying a layer-7c ContinuousModifier.
    parsed = _parse_static_ability_line(
        "During your turn, other creatures you control get +1/+0."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            condition=DuringYourTurn(),
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=0),
            ),
        ),
    )


def test_during_your_turn_keyword_anthem_sets_active_turn_only():
    # Forward-looking grammar coverage: no EoE printing combines the
    # turn rider with the keyword-only anthem body today, but the
    # grammar accepts it for free because the rider lives at the same
    # position in both anthem regexes. The active_turn_only flag rides
    # on the StaticAbility wrapping the layer-6 GrantKeyword.
    parsed = _parse_static_ability_line(
        "During your turn, other creatures you control have haste."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            condition=DuringYourTurn(),
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("haste"),
            ),
        ),
    )


def test_during_your_turn_inclusive_subject():
    # The rider also rides on the inclusive (``Creatures you control``)
    # subject. After the rider the subject is lowercase ("creatures"),
    # which the ``[Cc]`` character class accepts. Pure P/T anthem →
    # PR-D-pilot ContinuousModifier shape.
    parsed = _parse_static_ability_line(
        "During your turn, creatures you control get +1/+1."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            condition=DuringYourTurn(),
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=1),
            ),
        ),
    )


# ---------- Regex sanity ----------


def test_pt_regex_extracts_named_groups():
    # The compiled P/T regex must expose the named groups the parse
    # function reads (``turn_rider``, ``subject``, ``power``,
    # ``toughness``, ``keyword``); a regression in the regex shape
    # would silently change the ``StaticAbility`` produced.
    m = _ANTHEM_PT_LINE.match(
        "During your turn, other creatures you control get +1/+0 "
        "and have trample."
    )
    assert m is not None
    assert m.group("turn_rider") == "During your turn, "
    assert m.group("subject") == "other creatures you control"
    assert m.group("power") == "1"
    assert m.group("toughness") == "0"
    assert m.group("keyword") == "trample"


def test_pt_regex_optional_keyword_group_is_none():
    # When the ``and have <keyword>`` tail is absent the keyword group
    # must be ``None`` so the parse function falls through to the
    # empty-frozenset default.
    m = _ANTHEM_PT_LINE.match("Creatures you control get +1/+1.")
    assert m is not None
    assert m.group("keyword") is None
    assert m.group("turn_rider") is None


def test_keyword_regex_extracts_named_groups():
    # The keyword-only regex shares the same ``turn_rider`` and
    # ``subject`` groups as the P/T regex; the parse function reads
    # them from either match by name without caring which regex hit.
    m = _ANTHEM_KEYWORD_LINE.match(
        "Other creatures you control have haste."
    )
    assert m is not None
    assert m.group("turn_rider") is None
    assert m.group("subject") == "Other creatures you control"
    assert m.group("keyword") == "haste"


def test_anthem_helper_is_callable_directly():
    # The dispatcher calls the helper as a fallback after the
    # self-scope regexes; pinning the direct call documents that the
    # helper is a stable public-ish entry point inside the module.
    parsed = _parse_anthem_static_line(
        "Creatures you control have flying."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.CREATURES_YOU_CONTROL,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("flying"),
            ),
        ),
    )


# ---------- Negatives ----------


def test_unknown_keyword_in_anthem_returns_empty_tuple():
    # Conservative behavior matching the self-scope grammar: a
    # non-evergreen keyword in the ``and have`` / ``have`` tail fails
    # the parse rather than silently dropping the rider. Empty tuple
    # signals "no match" in the new return shape.
    assert (
        _parse_static_ability_line(
            "Creatures you control get +1/+1 and have scry."
        )
        == ()
    )
    assert (
        _parse_static_ability_line(
            "Creatures you control have scry."
        )
        == ()
    )


def test_missing_period_returns_empty_tuple():
    # The line anchor requires a terminating period; an oracle line
    # without one is malformed and must not match.
    assert (
        _parse_static_ability_line(
            "Creatures you control get +1/+1"
        )
        == ()
    )


def test_negative_pt_returns_empty_tuple():
    # The regex only accepts ``+N/+M`` modifiers; negative modifiers
    # (a different game effect) must not match the anthem grammar.
    assert (
        _parse_static_ability_line(
            "Creatures you control get -1/-1."
        )
        == ()
    )


def test_until_end_of_turn_anthem_returns_empty_tuple():
    # Spell-style temporary anthems (``... until end of turn``) are
    # not static abilities and must not parse here. The terminating
    # ``.`` after ``+N/+M`` in the regex is what enforces this — a
    # ``until end of turn`` tail breaks the anchor.
    assert (
        _parse_static_ability_line(
            "Creatures you control get +1/+1 until end of turn."
        )
        == ()
    )


def test_unrelated_subject_returns_empty_tuple():
    # The subject alternation is closed: only ``Creatures you control``
    # and ``Other creatures you control`` are accepted. A different
    # subject (``Artifacts you control``) doesn't match the anthem
    # grammar and falls through to the dispatcher's empty-tuple
    # return.
    assert (
        _parse_static_ability_line(
            "Artifacts you control get +1/+1."
        )
        == ()
    )


def test_during_your_turn_self_has_keyword_produces_self_scope_static():
    # ``During your turn, this creature has <keyword>.`` is now a
    # recognized self-scope form: condition=DuringYourTurn(), SELF
    # scope, keyword granted via a layer-6
    # ContinuousModifier(GrantKeyword). Alpharael, Dreaming Acolyte
    # prints in this exact surface form. Returns a 1-tuple in the new
    # shape.
    from mtgz_ir.cards.base import StaticScope
    from mtgz_ir.effects.conditions import DuringYourTurn
    from mtgz_ir.effects.modifiers import ContinuousModifier, GrantKeyword
    result = _parse_static_ability_line(
        "During your turn, this creature has deathtouch."
    )
    assert len(result) == 1
    static = result[0]
    assert static.scope == StaticScope.SELF
    assert static.condition == DuringYourTurn()
    assert static.continuous_effect == ContinuousModifier(
        layer=6, payload=GrantKeyword("deathtouch"),
    )
    assert static.granted_keywords == frozenset()


def test_self_scope_line_does_not_match_anthem_directly():
    # Cross-check: a self-scope line that the dispatcher routes to the
    # leading/trailing regex must not also match the anthem regex —
    # otherwise the dispatcher's fallthrough order would matter and
    # double-matching could produce surprises.
    assert (
        _parse_anthem_static_line(
            "This creature gets +1/+0 as long as you control an "
            "artifact."
        )
        == ()
    )


# ---------- Parametrized scope-mapping cross-check ----------


@pytest.mark.parametrize(
    "subject,scope",
    [
        ("Creatures you control", StaticScope.CREATURES_YOU_CONTROL),
        (
            "Other creatures you control",
            StaticScope.OTHER_CREATURES_YOU_CONTROL,
        ),
    ],
)
def test_subject_to_scope_mapping_is_exhaustive(subject, scope):
    # Exhaustive mapping check: every subject the regex accepts must
    # correspond to a scope, both for the P/T body and the keyword-only
    # body. A new subject added to the regex without a scope mapping
    # would silently produce an empty tuple from the parse function —
    # this test would catch that drift before it reached the
    # structural-parity gate.
    pt = _parse_static_ability_line(f"{subject} get +1/+1.")
    assert len(pt) == 1
    assert pt[0].scope == scope
    kw = _parse_static_ability_line(f"{subject} have flying.")
    assert len(kw) == 1
    assert kw[0].scope == scope


# ---------- End-to-end claim ----------


def test_memorial_team_leader_is_claimed_end_to_end():
    # Memorial Team Leader is the EoE card the anthem grammar +
    # ``During your turn,`` rider unlocks. Parser must claim it
    # through the full builder pipeline; structural parity vs. the
    # static registry is verified separately by
    # ``test_parser_parity.py``.
    claimed = OracleParserCardSource().claimed_names
    assert "Memorial Team Leader" in claimed


def test_tannuk_steadfast_second_claimed_by_grammar():
    # Tannuk, Steadfast Second is now claimed by the oracle-text grammar:
    # Line 1 ("Other creatures you control have haste.") parses via the
    # keyword-only anthem recognizer; line 2 ("Artifact cards and red
    # creature cards in your hand have warp {2}{R}.") parses via the
    # _parse_hand_warp_grant_line recognizer — produces a structured
    # HandAbilityGrant on the printing's hand_ability_grants tuple.
    # The engine consults the grant in legal.py to offer warp-cast
    # actions for matching hand cards.
    from mtgz_ir.cards.parser.source import OracleParserCardSource
    p = OracleParserCardSource().get_printings()["Tannuk, Steadfast Second"]
    assert "Tannuk, Steadfast Second" in OracleParserCardSource().claimed_names
    assert len(p.hand_ability_grants) == 1
    grant = p.hand_ability_grants[0]
    # Filter is a disjunction of (type, optional color) predicates.
    pred_set = set(grant.card_filter.predicates)
    assert ("Artifact", None) in pred_set
    assert ("Creature", "R") in pred_set
    # Granted warp cost is parsed into a ManaCost.
    assert grant.granted_warp_cost is not None
    assert grant.granted_warp_cost_str == "{2}{R}"
    assert grant.granted_warp_cost.total == 3
    # Haste anthem static still present — migrated to a layer-6
    # ContinuousModifier(GrantKeyword("haste")) declaration.
    from mtgz_ir.cards.base import StaticScope
    haste_statics = [
        s for s in p.static_abilities
        if s.scope == StaticScope.OTHER_CREATURES_YOU_CONTROL
        and s.continuous_effect is not None
        and isinstance(s.continuous_effect.payload, GrantKeyword)
        and s.continuous_effect.payload.keyword == "haste"
    ]
    assert len(haste_statics) == 1
