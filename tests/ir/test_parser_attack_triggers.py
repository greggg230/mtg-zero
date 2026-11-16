"""Tests for the parser ATTACKS-event grammar additions.

The pre-existing parser already recognizes the ``"this creature
attacks"`` trigger phrase (phase-5 migration to the predicate-trigger
path). What's missing is the **effect** vocabulary that the EoE
attacks-trigger cluster prints. This batch lands one new effect
primitive plus a forward-looking compound trigger phrase:

* ``put a +1/+1 counter on target creature an opponent controls`` →
  ``PutPlusOneCounterOnOpponentsCreature(amount=1)``. Mirror image of
  the existing ``put a +1/+1 counter on target creature you control``
  restricted variant — legal targets are creatures NOT controlled by
  the resolving controller. Surfaces on Sunset Saboteur's attack
  trigger ("Whenever this creature attacks, put a +1/+1 counter on
  target creature an opponent controls."). Sunset Saboteur itself
  doesn't claim end-to-end yet because its Ward—Discard rider sits on
  a separate parser scope (``printing_keywords.py``); this primitive
  is the trigger-effect half of that pending claim.

* ``this creature enters or attacks`` →
  ``((Event.ENTERS, IsSelf()), (Event.ATTACKS, IsSelf()))`` compound
  predicate-trigger phrase. Mirrors the existing ``this creature
  enters or dies`` (Thawbringer) entry verbatim — same ``IsSelf()``
  predicate per arm, same per-arm fan-out at the static-registry
  level. Surfaces on Haliya, Ascendant Cadet and Syr Vondam, the
  Lucent (both print the named-self form ``"Whenever <name> enters
  or attacks"``); neither claims end-to-end yet because the parser
  doesn't substitute the printing's own name onto ``"this creature"``
  in trigger-condition phrases. This compound entry is the trigger-
  phrase half of those pending claims.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DealDamageToOpponentCreature,
    DrawCards,
    PutPlusOneCounterOnOpponentsCreature,
    PutPlusOneCounterOnSelf,
    PutPlusOneCounterOnTargetCreature,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
)
from mtgz_ir.cards.parser.triggers import (
    _parse_compound_predicate_trigger_phrase,
    _parse_predicate_trigger_phrase,
    _parse_trigger_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# ---------- put a +1/+1 counter on target creature an opponent controls ----------


def test_parse_effect_recognizes_counter_on_target_opponent_creature():
    # Sunset Saboteur's attack-trigger payload. Maps to the
    # opponent-restricted engine class whose ``legal_targets`` filters
    # to creatures NOT controlled by the resolving controller.
    assert _parse_effect(
        "put a +1/+1 counter on target creature an opponent controls"
    ) == PutPlusOneCounterOnOpponentsCreature(amount=1)


def test_parse_effect_phrase_recognizes_counter_on_target_opponent_creature():
    # Creature-trigger surface form — no source-subject pronoun on
    # this phrase (the verb ``put`` starts the clause), so the phrase
    # passes through ``_normalize_creature_effect_phrase`` unchanged.
    assert _parse_effect_phrase(
        "put a +1/+1 counter on target creature an opponent controls"
    ) == PutPlusOneCounterOnOpponentsCreature(amount=1)


def test_parse_effect_does_not_collide_with_bare_counter_on_target_creature():
    # The bare ``put a +1/+1 counter on target creature`` regex (which
    # produces ``PutPlusOneCounterOnTargetCreature``) shouldn't fire on
    # the ``an opponent controls`` variant. The bare regex's ``$``
    # anchor only matches phrases that END at ``creature`` with no
    # qualifier, so the two surface shapes are disjoint and produce
    # different engine classes.
    bare = _parse_effect("put a +1/+1 counter on target creature")
    opp = _parse_effect(
        "put a +1/+1 counter on target creature an opponent controls"
    )
    assert isinstance(bare, PutPlusOneCounterOnTargetCreature)
    assert isinstance(opp, PutPlusOneCounterOnOpponentsCreature)


def test_parse_effect_does_not_collide_with_you_control_counter_variant():
    # The ``you control`` variant produces a different engine class
    # (``PutPlusOneCounterOnCreatureYouControl``) than the
    # ``an opponent controls`` variant introduced here. Pin both
    # shapes to confirm the two restricted variants stay disjoint.
    you = _parse_effect(
        "put a +1/+1 counter on target creature you control"
    )
    opp = _parse_effect(
        "put a +1/+1 counter on target creature an opponent controls"
    )
    assert type(you).__name__ == "PutPlusOneCounterOnCreatureYouControl"
    assert isinstance(opp, PutPlusOneCounterOnOpponentsCreature)


def test_attacks_trigger_with_counter_on_opponent_end_to_end():
    # Sunset Saboteur's full trigger line — the predicate-trigger
    # ``this creature attacks`` phrase composes with the new effect
    # primitive to produce the static-registry-shaped TriggeredAbility.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature attacks, put a +1/+1 counter on target "
        "creature an opponent controls."
    )
    assert parsed == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf(),
        effect=PutPlusOneCounterOnOpponentsCreature(amount=1),
    )


# ---------- this creature enters or attacks (compound) ----------


def test_parse_compound_predicate_recognizes_enters_or_attacks():
    # Haliya, Ascendant Cadet's and Syr Vondam, the Lucent's compound
    # condition — both print ``"Whenever <name> enters or attacks"``.
    # The phrase fans out to two ``(Event, Predicate)`` arms sharing
    # the same ``IsSelf()`` predicate, mirroring the static registry's
    # two-entry-per-line layout for these printings.
    assert _parse_compound_predicate_trigger_phrase(
        "this creature enters or attacks"
    ) == (
        (Event.ENTERS, IsSelf()),
        (Event.ATTACKS, IsSelf()),
    )


def test_legacy_string_trigger_phrase_does_not_claim_enters_or_attacks():
    # The legacy string-key trigger table is empty for parsed phrases
    # (phases 4-9 emptied it). Pin the compound's absence from the
    # legacy path so a future drift of the migration ordering would
    # surface as a test failure here.
    assert _parse_trigger_phrase("this creature enters or attacks") is None


def test_single_predicate_trigger_phrase_does_not_claim_enters_or_attacks():
    # The compound phrase must NOT live on the single-arm predicate
    # table (it would silently collapse to a single trigger arm if it
    # did). The single-arm table is consulted only after the compound
    # lookup misses, so a duplicate entry would fan one arm wrong;
    # pin the absence to keep the precedence intent visible.
    assert _parse_predicate_trigger_phrase(
        "this creature enters or attacks"
    ) is None


def test_enters_or_attacks_end_to_end_fans_out_two_arms():
    # End-to-end: the trigger-line parser produces a 2-tuple of
    # ``TriggeredAbility`` (one per arm) with the shared effect
    # attached to each. The clause layer treats the tuple as if each
    # arm appeared on its own oracle line — matching the static-
    # registry layout for these compound printings.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature enters or attacks, "
        "put a +1/+1 counter on target creature."
    )
    assert parsed == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=PutPlusOneCounterOnTargetCreature(amount=1),
        ),
        TriggeredAbility(
            event=Event.ATTACKS, predicate=IsSelf(),
            effect=PutPlusOneCounterOnTargetCreature(amount=1),
        ),
    )


def test_enters_or_attacks_supports_arbitrary_effects():
    # The compound trigger is effect-agnostic: any single effect that
    # parses on its own can ride the line. Pin a different effect
    # primitive (``draw a card``) to confirm the two-arm fan-out
    # composes uniformly with the effect grammar — same as the
    # existing Thawbringer ``enters or dies`` precedent.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature enters or attacks, draw a card."
    )
    assert parsed == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
        TriggeredAbility(
            event=Event.ATTACKS, predicate=IsSelf(),
            effect=DrawCards(amount=1),
        ),
    )


def test_enters_or_attacks_rejects_unparseable_effect():
    # Per the standard trigger-line contract: if the effect half
    # doesn't parse, the whole line is rejected (no silent fan-out
    # with a half-broken effect). The compound path uses the same
    # ``_parse_effect_phrase`` rejection as the single-arm path.
    assert _parse_triggered_ability_line(
        "Whenever this creature enters or attacks, do something weird."
    ) is None


# ---------- regression: existing attack-trigger phrase still parses ----------


def test_existing_this_creature_attacks_phrase_unchanged():
    # The ``"this creature attacks"`` single-arm predicate-trigger
    # phrase already lives in ``_PREDICATE_TRIGGER_PHRASES`` (phase 5
    # migration). Pin it here so the new compound entry's precedence
    # ordering doesn't accidentally shadow the single-arm phrase.
    assert _parse_predicate_trigger_phrase(
        "this creature attacks"
    ) == (Event.ATTACKS, IsSelf())


def test_attacks_trigger_with_existing_effect_still_parses():
    # Smoke test: a non-novel attack-trigger effect (deals N damage
    # to target creature an opponent controls — Nova Hellkite's
    # primitive) composes with the attacks predicate without help
    # from the new compound entry. The compound table is consulted
    # FIRST in the trigger-line dispatcher, so a regression that
    # accidentally claimed ``"this creature attacks"`` on the
    # compound side would surface here as the line returning a
    # tuple instead of a single ``TriggeredAbility``.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature attacks, it deals 1 damage to target "
        "creature an opponent controls."
    )
    assert parsed == TriggeredAbility(
        event=Event.ATTACKS, predicate=IsSelf(),
        effect=DealDamageToOpponentCreature(amount=1),
    )


def test_self_attack_trigger_pump_self_smoke():
    # Another smoke test pinning the single-arm path: attack-trigger
    # composing with ``put a +1/+1 counter on this creature``. The
    # ``PutPlusOneCounterOnSelf`` primitive is already in the effect
    # grammar; a future EoE attack-trigger printing with this exact
    # body would parse without further grammar work.
    parsed = _parse_triggered_ability_line(
        "Whenever this creature attacks, put a +1/+1 counter on this creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.ATTACKS, predicate=IsSelf(),
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- mass anthem: other / and-gain extensions ----------


def test_mass_pump_your_creatures_bare_form_unchanged():
    # The pre-existing bare ``creatures you control get +N/+M until end
    # of turn`` form (Dual-Sun Adepts) keeps producing the same engine
    # class with no exclude-source / no granted-keywords flags set —
    # the regex extension is backward-compatible.
    from mtgz_ir.cards.base import PumpCreaturesYouControlUntilEOT
    assert _parse_effect(
        "creatures you control get +1/+1 until end of turn"
    ) == PumpCreaturesYouControlUntilEOT(
        power_bonus=1, toughness_bonus=1,
        exclude_source=False, granted_keywords=frozenset(),
    )


def test_mass_pump_other_creatures_you_control_sets_exclude_source():
    # The ``other`` qualifier flips ``exclude_source=True`` — the
    # mass-pump skips the resolving source. Forward-looking shape; no
    # printed EoE card uses bare-other-no-grant, but the symmetry with
    # Syr Vondam's longer body keeps the grammar uniform.
    from mtgz_ir.cards.base import PumpCreaturesYouControlUntilEOT
    assert _parse_effect(
        "other creatures you control get +1/+0 until end of turn"
    ) == PumpCreaturesYouControlUntilEOT(
        power_bonus=1, toughness_bonus=0,
        exclude_source=True, granted_keywords=frozenset(),
    )


def test_mass_pump_creatures_you_control_with_granted_keyword():
    # Bare-no-other form with a single granted keyword. Forward-
    # looking; not on any current EoE printing but parses uniformly
    # with the other-and-gain shape below.
    from mtgz_ir.cards.base import PumpCreaturesYouControlUntilEOT
    assert _parse_effect(
        "creatures you control get +1/+0 and gain trample until end of turn"
    ) == PumpCreaturesYouControlUntilEOT(
        power_bonus=1, toughness_bonus=0,
        exclude_source=False,
        granted_keywords=frozenset({"trample"}),
    )


def test_mass_pump_other_creatures_with_granted_keyword():
    # Syr Vondam, the Lucent's enters/attacks-trigger payload — the
    # canonical attack-trigger surface for this batch. ``other``
    # flips exclude_source; ``and gain deathtouch`` populates
    # granted_keywords. Mirrors the static registry's Syr Vondam,
    # the Lucent pump effect verbatim.
    from mtgz_ir.cards.base import PumpCreaturesYouControlUntilEOT
    assert _parse_effect(
        "other creatures you control get +1/+0 and gain deathtouch "
        "until end of turn"
    ) == PumpCreaturesYouControlUntilEOT(
        power_bonus=1, toughness_bonus=0,
        exclude_source=True,
        granted_keywords=frozenset({"deathtouch"}),
    )


def test_mass_pump_with_multi_keyword_grant():
    # Glacier Godmaw's landfall-trigger payload — bare ``creatures you
    # control`` with multiple granted keywords (vigilance and haste).
    # The grant body splits on `` and `` and validates each piece
    # against the keyword-line whitelist.
    from mtgz_ir.cards.base import PumpCreaturesYouControlUntilEOT
    assert _parse_effect(
        "creatures you control get +1/+1 and gain vigilance and haste "
        "until end of turn"
    ) == PumpCreaturesYouControlUntilEOT(
        power_bonus=1, toughness_bonus=1,
        exclude_source=False,
        granted_keywords=frozenset({"vigilance", "haste"}),
    )


def test_mass_pump_rejects_unknown_granted_keyword():
    # An unknown keyword in the grant body rejects the WHOLE phrase
    # rather than silently dropping the unknown. Mirrors the
    # ``_PUMP_AND_GAIN_KEYWORDS_UEOT`` precedent: a missing grant is
    # never silently dropped, and the wrapping card stays unclaimed.
    assert _parse_effect(
        "other creatures you control get +1/+0 and gain pajamas "
        "until end of turn"
    ) is None


# ---------- named-self substitution in trigger lines ----------


def test_substitute_self_name_short_form_replaces_legendary_short():
    # Haliya, Ascendant Cadet's printed trigger uses the short-form
    # name. After substitution, the line is in the canonical
    # ``"this creature"`` shape that the trigger-condition tables key
    # off of.
    from mtgz_ir.cards.parser.triggers import _substitute_self_name
    assert _substitute_self_name(
        "Whenever Haliya enters or attacks, draw a card.",
        "Haliya, Ascendant Cadet",
    ) == "Whenever this creature enters or attacks, draw a card."


def test_substitute_self_name_full_form_replaces_full():
    # Forward-looking shape — a printing whose oracle text uses the
    # full name verbatim (rare but possible). The full name is tried
    # before the short form so the more-specific replacement wins.
    from mtgz_ir.cards.parser.triggers import _substitute_self_name
    assert _substitute_self_name(
        "Whenever Haliya, Ascendant Cadet attacks, draw a card.",
        "Haliya, Ascendant Cadet",
    ) == "Whenever this creature attacks, draw a card."


def test_substitute_self_name_non_legendary_passthrough():
    # A non-legendary printing's name has no comma-separated short
    # form; a printing whose oracle text doesn't reference itself by
    # name passes through unchanged.
    from mtgz_ir.cards.parser.triggers import _substitute_self_name
    assert _substitute_self_name(
        "Whenever this creature attacks, draw a card.",
        "Sunset Saboteur",
    ) == "Whenever this creature attacks, draw a card."


def test_substitute_self_name_word_boundary_safe():
    # The substitution is whole-word anchored (regex word-boundary)
    # so a substring match inside a longer token can't accidentally
    # fire. Pin the boundary behavior with a synthetic line where the
    # short name appears as a substring of an unrelated word.
    from mtgz_ir.cards.parser.triggers import _substitute_self_name
    assert _substitute_self_name(
        "Whenever this creature attacks, sample text Haliya alone.",
        "Haliya, Ascendant Cadet",
    ) == "Whenever this creature attacks, sample text this creature alone."


def test_parse_triggered_ability_line_with_name_substitutes_short():
    # End-to-end: a trigger line that uses the printing's short-form
    # name parses to the same TriggeredAbility shape as the canonical
    # ``"this creature"`` form when ``name`` is supplied.
    parsed = _parse_triggered_ability_line(
        "Whenever Haliya enters or attacks, "
        "put a +1/+1 counter on target creature.",
        name="Haliya, Ascendant Cadet",
    )
    assert parsed == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=PutPlusOneCounterOnTargetCreature(amount=1),
        ),
        TriggeredAbility(
            event=Event.ATTACKS, predicate=IsSelf(),
            effect=PutPlusOneCounterOnTargetCreature(amount=1),
        ),
    )


def test_parse_triggered_ability_line_without_name_keeps_named_self():
    # Without ``name`` the substitution doesn't run, so a named-self
    # trigger line stays unrecognized — there's no trigger-condition
    # phrase keyed on the literal ``"Haliya enters or attacks"``.
    # Earlier batches' callers (``_parse_clause`` invocations from
    # other builders) don't supply ``name``, so this preserves their
    # existing behavior.
    assert _parse_triggered_ability_line(
        "Whenever Haliya enters or attacks, "
        "put a +1/+1 counter on target creature.",
    ) is None


def test_parse_triggered_ability_line_with_name_passthrough_for_canonical():
    # When the line already uses the canonical ``"this creature"``
    # form, supplying ``name`` is a no-op — the substitution helper
    # only fires on actual name occurrences. Pin a single-arm
    # canonical-form trigger to confirm the no-name and with-name
    # paths produce identical output for a name-free line.
    canonical = TriggeredAbility(
        event=Event.ATTACKS, predicate=IsSelf(),
        effect=PutPlusOneCounterOnSelf(amount=1),
    )
    assert _parse_triggered_ability_line(
        "Whenever this creature attacks, "
        "put a +1/+1 counter on this creature.",
        name="Sunset Saboteur",
    ) == canonical
    assert _parse_triggered_ability_line(
        "Whenever this creature attacks, "
        "put a +1/+1 counter on this creature.",
    ) == canonical




