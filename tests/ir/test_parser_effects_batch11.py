"""Tests for batch-11 unified-effect-grammar additions:

* ``target creature [you control] gets +N/+M and gains <kw>[ and <kw2>...]
  until end of turn`` — pump + grant-keywords compound that produces
  ``PumpTargetCreatureUntilEOT(power_bonus, toughness_bonus,
  granted_keywords, you_control_only)``.
* ``you may discard a card. If you do, draw N cards`` — composes the
  generic ``IfYouDo(MayDo(DiscardSelf(amount=1)),
  DrawCards(amount=N))`` shape. Singular ``draw a card`` and
  spelled-out small cardinals supported.
* ``you may sacrifice another creature or artifact. If you do, this
  creature gets +N/+M until end of turn`` — specialized self-pump
  wrapper ``OptionalSacAnotherForPumpSelfUEOT(power_bonus,
  toughness_bonus)``.
* ``you may sacrifice another creature or artifact. If you do,
  <inner-effect>`` — generic ``OptionalSacAnotherForEffect(inner_effect=
  ...)`` wrapper. Inner phrase parsed recursively via the unified
  grammar.

Each new effect is exercised three ways: directly via ``_parse_effect``
on the normalized phrase, via ``_parse_effect_phrase`` (creature-trigger
surface form), and via ``_parse_spell_effect_phrase`` (spell sentence
form, capitalized first letter and trailing period). This three-way
coverage verifies a single grammar addition lights up both contexts.

A claim test confirms the parser end-to-end-claims the four EoE cards
unlocked by this batch (Rig for War, Dark Endurance, Comet Crawler,
Swarm Culler, Kavaron Turbodrone).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DiscardSelf,
    DrawCards,
    IfYouDo,
    MayDo,
    OptionalSacAnotherForEffect,
    OptionalSacAnotherForPumpSelfUEOT,
    PumpTargetCreatureUntilEOT,
    PutPlusOneCounterOnSelf,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


# ---------- pump + grant keywords UEOT ----------

def test_parse_effect_recognizes_pump_and_gains_single_keyword():
    # Dark Endurance's effect: +2/+0 and grants indestructible.
    assert _parse_effect(
        "target creature gets +2/+0 and gains indestructible until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=2, toughness_bonus=0,
        granted_keywords=frozenset({"indestructible"}),
    )


def test_parse_effect_recognizes_pump_and_gains_multiple_keywords():
    # Rig for War: +3/+0 and grants first strike + reach.
    assert _parse_effect(
        "target creature gets +3/+0 and gains first strike and reach "
        "until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=3, toughness_bonus=0,
        granted_keywords=frozenset({"first strike", "reach"}),
    )


def test_parse_effect_recognizes_pump_and_gains_you_control_variant():
    # ``target creature you control`` restricts the legal-target pool —
    # threaded through ``you_control_only=True`` on the engine class.
    assert _parse_effect(
        "target creature you control gets +1/+1 and gains haste "
        "until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=1, toughness_bonus=1,
        granted_keywords=frozenset({"haste"}),
        you_control_only=True,
    )


def test_parse_effect_recognizes_pump_and_gains_negative_pump():
    # Negative pump folds through the same effect with negative bonuses.
    # No EoE card prints this exact shape today but the grammar is
    # uniform — keeps a future debuff-and-keyword printing parseable
    # without further regex work.
    assert _parse_effect(
        "target creature gets -1/-1 and gains menace until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=-1, toughness_bonus=-1,
        granted_keywords=frozenset({"menace"}),
    )


def test_parse_effect_phrase_recognizes_pump_and_gains():
    # Creature-trigger surface form (no ``it`` subject — the trigger's
    # consequence buffs ANOTHER creature, not the firing source).
    assert _parse_effect_phrase(
        "target creature gets +2/+2 and gains trample until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=2, toughness_bonus=2,
        granted_keywords=frozenset({"trample"}),
    )


def test_parse_spell_effect_phrase_recognizes_pump_and_gains():
    # Spell surface form: capitalized first letter + trailing period.
    # Mirrors Rig for War's printed sentence.
    assert _parse_spell_effect_phrase(
        "Target creature gets +3/+0 and gains first strike and reach "
        "until end of turn.",
        "Bogus Rig for War",
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=3, toughness_bonus=0,
        granted_keywords=frozenset({"first strike", "reach"}),
    )


def test_parse_effect_returns_none_for_unknown_granted_keyword():
    # ``banding`` is not a modeled keyword — the keyword-grant body
    # validation rejects, the whole pump-and-gain phrase rejects, and
    # the wrapping card stays unclaimed rather than silently dropping
    # the granted ability. (``protection from black`` used to be the
    # example here; protection is modeled now — see
    # ``mtgz_ir.keyword_rules`` — so it parses.)
    assert _parse_effect(
        "target creature gets +1/+1 and gains banding "
        "until end of turn"
    ) is None
    assert _parse_effect(
        "target creature gets +1/+1 and gains protection from black "
        "until end of turn"
    ) is not None


def test_parse_effect_returns_none_for_pump_and_gains_without_duration():
    # Missing ``until end of turn`` — no permanent grant in this
    # grammar; the wrapping card stays unclaimed.
    assert _parse_effect(
        "target creature gets +1/+1 and gains haste"
    ) is None


def test_parse_effect_returns_none_for_pump_and_gains_missing_sign():
    # Plain integers aren't valid in pump notation — the existing
    # bare-pump grammar requires explicit sign.
    assert _parse_effect(
        "target creature gets 1/1 and gains haste until end of turn"
    ) is None


# ---------- you may discard a card. If you do, draw N cards ----------

def _looter(amount: int):
    """Helper: the parser-built shape for "you may discard a card.
    If you do, draw <amount> cards." Composes the generic combinators
    the parser produces."""
    return IfYouDo(
        do_action=MayDo(DiscardSelf(amount=1)),
        then_action=DrawCards(amount=amount),
    )


def test_parse_effect_recognizes_optional_discard_then_draw_two():
    # Melded Moxite's printed surface form (the ``two`` spelled-out
    # cardinal is the Scryfall convention for small counts).
    assert _parse_effect(
        "you may discard a card. If you do, draw two cards"
    ) == _looter(2)


def test_parse_effect_recognizes_optional_discard_then_draw_singular():
    # ``draw a card`` collapses to amount=1.
    assert _parse_effect(
        "you may discard a card. If you do, draw a card"
    ) == _looter(1)


def test_parse_effect_recognizes_optional_discard_then_draw_digit():
    # Forward-compat: digits parse via ``_parse_amount`` too.
    assert _parse_effect(
        "you may discard a card. If you do, draw 3 cards"
    ) == _looter(3)


def test_parse_effect_phrase_recognizes_optional_discard_then_draw():
    # Creature-trigger surface form.
    assert _parse_effect_phrase(
        "you may discard a card. If you do, draw two cards"
    ) == _looter(2)


def test_parse_spell_effect_phrase_recognizes_optional_discard_then_draw():
    # Spell surface form.
    assert _parse_spell_effect_phrase(
        "You may discard a card. If you do, draw two cards.",
        "Bogus Looter",
    ) == _looter(2)


def test_parse_effect_returns_none_for_malformed_optional_discard():
    # Wrong inner shape — the regex requires ``draw <count> cards``.
    assert _parse_effect(
        "you may discard a card. If you do, gain 1 life"
    ) is None
    # Missing the ``If you do`` gate: not the optional-discard-then-
    # draw primitive. The generic ``you may <effect>`` recognizer scopes
    # the optional over the whole conjunction (both halves optional
    # together), never over the discard alone.
    from mtgz_ir.effects.combinators import MayDo, Sequence
    eff = _parse_effect("you may discard a card and draw two cards")
    assert isinstance(eff, MayDo)
    assert isinstance(eff.inner, Sequence)


# ---------- you may sac another for self-pump UEOT ----------

def test_parse_effect_recognizes_optional_sac_for_self_pump_ueot():
    # Comet Crawler's attack-trigger surface form.
    assert _parse_effect(
        "you may sacrifice another creature or artifact. "
        "If you do, this creature gets +2/+0 until end of turn"
    ) == OptionalSacAnotherForPumpSelfUEOT(
        power_bonus=2, toughness_bonus=0,
    )


def test_parse_effect_recognizes_optional_sac_for_self_pump_negative_bonus():
    # Negative self-pump folds through the same engine field; no EoE
    # card prints that today but the grammar accepts it for symmetry.
    assert _parse_effect(
        "you may sacrifice another creature or artifact. "
        "If you do, this creature gets -1/-1 until end of turn"
    ) == OptionalSacAnotherForPumpSelfUEOT(
        power_bonus=-1, toughness_bonus=-1,
    )


def test_parse_effect_phrase_recognizes_optional_sac_for_self_pump():
    # Creature-trigger surface form (no ``it`` prefix because the
    # subject of the trigger's effect is the player).
    assert _parse_effect_phrase(
        "you may sacrifice another creature or artifact. "
        "If you do, this creature gets +2/+0 until end of turn"
    ) == OptionalSacAnotherForPumpSelfUEOT(
        power_bonus=2, toughness_bonus=0,
    )


def test_parse_spell_effect_phrase_recognizes_optional_sac_for_self_pump():
    # Spell surface form (no real EoE printing of this shape on a spell
    # but the grammar is uniform across contexts).
    assert _parse_spell_effect_phrase(
        "You may sacrifice another creature or artifact. "
        "If you do, this creature gets +1/+0 until end of turn.",
        "Bogus Berserk",
    ) == OptionalSacAnotherForPumpSelfUEOT(
        power_bonus=1, toughness_bonus=0,
    )


# ---------- you may sac another for generic effect ----------

def test_parse_effect_recognizes_optional_sac_for_draw_a_card():
    # Swarm Culler's becomes-tapped trigger surface form.
    assert _parse_effect(
        "you may sacrifice another creature or artifact. "
        "If you do, draw a card"
    ) == OptionalSacAnotherForEffect(inner_effect=DrawCards(amount=1))


def test_parse_effect_recognizes_optional_sac_for_inner_compound():
    # Inner consequence can itself be a compound effect (parsed via
    # the unified grammar's recursion through ``_parse_effect``).
    eff = _parse_effect(
        "you may sacrifice another creature or artifact. "
        "If you do, put a +1/+1 counter on this creature"
    )
    assert eff == OptionalSacAnotherForEffect(
        inner_effect=PutPlusOneCounterOnSelf(amount=1),
    )


def test_parse_effect_phrase_recognizes_optional_sac_for_effect():
    assert _parse_effect_phrase(
        "you may sacrifice another creature or artifact. "
        "If you do, draw a card"
    ) == OptionalSacAnotherForEffect(inner_effect=DrawCards(amount=1))


def test_parse_spell_effect_phrase_recognizes_optional_sac_for_effect():
    assert _parse_spell_effect_phrase(
        "You may sacrifice another creature or artifact. "
        "If you do, draw a card.",
        "Bogus Sacrifice Spell",
    ) == OptionalSacAnotherForEffect(inner_effect=DrawCards(amount=1))


def test_parse_effect_returns_none_for_optional_sac_with_unparseable_inner():
    # ``invent a brand-new effect`` doesn't parse — the whole optional-
    # sac wrapper rejects so the wrapping card stays unclaimed rather
    # than silently dropping the consequence.
    assert _parse_effect(
        "you may sacrifice another creature or artifact. "
        "If you do, invent a brand-new effect"
    ) is None


def test_parse_effect_returns_none_for_optional_sac_without_another():
    # ``you may sacrifice a creature or artifact`` (no ``another``) is a
    # different shape — Selfcraft Mechan-style ``When you do`` triggers.
    # The regex requires ``another`` and rejects this.
    assert _parse_effect(
        "you may sacrifice a creature or artifact. If you do, draw a card"
    ) is None


def test_self_pump_sac_takes_priority_over_generic_sac():
    # The more-specific self-pump shape MUST be matched first so the
    # specialized engine class is produced. If the dispatcher tried the
    # generic regex first it would attempt to parse ``this creature
    # gets +2/+0 until end of turn`` as the inner effect — which the
    # unified grammar doesn't recognize as a primitive (only the
    # ``target creature`` pump shape parses), so it would silently fall
    # through to None instead of producing ``OptionalSacAnotherForPump
    # SelfUEOT``. Asserting the type ensures dispatch ordering stays
    # specific-before-general.
    eff = _parse_effect(
        "you may sacrifice another creature or artifact. "
        "If you do, this creature gets +1/+1 until end of turn"
    )
    assert isinstance(eff, OptionalSacAnotherForPumpSelfUEOT)
    assert eff.power_bonus == 1
    assert eff.toughness_bonus == 1


# ---------- end-to-end card-claim test ----------

_PARSER = OracleParserCardSource()


def test_parser_claims_rig_for_war_via_pump_and_gains():
    # Bare ``target creature gets +N/+M and gains <kw> and <kw2> UEOT``.
    assert "Rig for War" in _PARSER.claimed_names


def test_parser_claims_dark_endurance_via_pump_and_gains_with_cost_reduction():
    # Bare ``target creature gets +N/+M and gains <kw> UEOT`` plus the
    # already-supported ``This spell costs {N} less to cast if it
    # targets a blocking creature.`` rider.
    assert "Dark Endurance" in _PARSER.claimed_names


def test_parser_claims_kavaron_turbodrone_via_pump_and_gains_you_control():
    # ``{T}: Target creature you control gets +1/+1 and gains haste UEOT.
    # Activate only as a sorcery.`` — uses the ``you control`` variant of
    # the pump-and-gains regex plus existing activated-ability support.
    assert "Kavaron Turbodrone" in _PARSER.claimed_names


def test_parser_claims_comet_crawler_via_optional_sac_for_self_pump():
    # ``Whenever this creature attacks, you may sacrifice another
    # creature or artifact. If you do, this creature gets +2/+0 UEOT.``
    assert "Comet Crawler" in _PARSER.claimed_names


def test_parser_claims_swarm_culler_via_optional_sac_for_effect():
    # ``Whenever this creature becomes tapped, you may sacrifice another
    # creature or artifact. If you do, draw a card.``
    assert "Swarm Culler" in _PARSER.claimed_names
