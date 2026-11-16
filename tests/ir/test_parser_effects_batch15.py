"""Tests for batch-15 unified-effect-grammar additions:

* ``put N charge counters on target Spacecraft or Planet you control``
  → ``PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(amount=N)``.
  Surfaces on Drill Too Deep mode 0 (the printing uses the spelled-out
  cardinal "five"). Amount accepts both digit and small-cardinal
  spellings.
* ``deals N damage to target tapped creature`` →
  ``DealDamageToTargetTappedCreature(amount=N)``. Surfaces on Reroute
  Systems mode 1; the engine class's ``legal_targets`` filters to
  creatures that are currently tapped.
* ``target artifact or creature gains <kw>[ and <kw2>...] until end of
  turn`` →
  ``PumpTargetArtifactOrCreatureUntilEOT(granted_keywords=...)``. Pure
  keyword grant on a permanent target whose legal pool spans both
  artifacts and creatures. Surfaces on Reroute Systems mode 0
  (indestructible).
* ``destroy target artifact, enchantment, or creature with flying`` →
  ``DestroyTargetArtifactEnchantmentOrFlyingCreature()``. Surfaces on
  Shattered Wings's destroy clause; the legal-targets filter spans
  artifacts (any), enchantments (any), and creatures with flying.
* ``target artifact or creature's owner puts it on their choice of the
  top or bottom of their library`` →
  ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary()``. Lost
  in Space's bounce-to-library clause; the chosen target's owner (not
  the spell's controller) decides top vs bottom at resolution.

Each new effect is exercised three ways: directly via
``_parse_effect`` on the normalized phrase, via
``_parse_effect_phrase`` (creature-trigger surface form), and via
``_parse_spell_effect_phrase`` (spell sentence form, capitalized
first letter and trailing period). Negative cases verify the regex
stays narrow.

Card-claim tests confirm the parser end-to-end-claims the four EoE
spells unlocked by this batch: Drill Too Deep, Reroute Systems,
Shattered Wings, Lost in Space. Each is a multi-clause spell whose
claim depends on both a new primitive and an existing combinator
(ModalChooseOne for the first two, Sequence for the latter two).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ConditionalIfVoid,
    DealDamageToTargetTappedCreature,
    DestroyTargetArtifact,
    DestroyTargetArtifactEnchantmentOrFlyingCreature,
    ModalChooseOne,
    OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    PumpTargetArtifactOrCreatureUntilEOT,
    PumpTargetCreatureUntilEOT,
    PutChargeCountersOnTargetSpacecraftOrPlanetYouControl,
    Sequence,
    Surveil,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser.spell_builder import (
    _additional_cost_flags,
    _detect_additional_cost_flags,
    _is_additional_cost_preamble,
    _parse_spell_oracle_text,
    _parse_void_pump_replacement,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


# ---------- put N charge counters on target Spacecraft or Planet ----------

def test_parse_effect_recognizes_charge_counters_spelled_out():
    # Drill Too Deep mode 0's printed surface uses the spelled-out
    # cardinal ``five``. The amount goes through ``_parse_amount``
    # to land as the int 5 on the engine class.
    assert _parse_effect(
        "put five charge counters on target Spacecraft or Planet you control"
    ) == PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(amount=5)


def test_parse_effect_recognizes_charge_counters_digit():
    # Forward-symmetric digit form. No EoE card prints this exact
    # surface today, but the regex accepts both digit and small-
    # cardinal spellings so a future printing parses without
    # further grammar work.
    assert _parse_effect(
        "put 3 charge counters on target Spacecraft or Planet you control"
    ) == PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(amount=3)


def test_parse_effect_phrase_recognizes_charge_counters():
    # Creature-trigger surface — the ``it`` source-subject prefix
    # isn't present (the targeted phrase has no source-relative
    # subject) and the phrase passes through to ``_parse_effect``
    # unchanged.
    assert _parse_effect_phrase(
        "put five charge counters on target Spacecraft or Planet you control"
    ) == PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(amount=5)


def test_parse_spell_effect_phrase_recognizes_charge_counters():
    # Spell sentence form — capitalized first letter, trailing
    # period. The normalizer lowercases the first letter and
    # strips the period before regex match.
    assert _parse_spell_effect_phrase(
        "Put five charge counters on target Spacecraft or Planet you control.",
        "Drill Too Deep",
    ) == PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(amount=5)


def test_parse_effect_returns_none_for_charge_counters_no_filter():
    # Without the ``you control`` suffix the surface is a different
    # primitive (no engine class for ``put N charge counters on
    # any Spacecraft`` exists today). Sanity-check the regex stays
    # narrow.
    assert _parse_effect(
        "put five charge counters on target Spacecraft or Planet"
    ) is None


# ---------- deals N damage to target tapped creature ----------

def test_parse_effect_recognizes_damage_target_tapped_creature():
    # Reroute Systems mode 1 — 2 damage to a tapped creature.
    # Maps to the engine class with the captured int.
    assert _parse_effect(
        "deals 2 damage to target tapped creature"
    ) == DealDamageToTargetTappedCreature(amount=2)


def test_parse_effect_recognizes_damage_target_tapped_creature_larger():
    # Forward-symmetric larger-amount form. No EoE card prints
    # higher amounts on this surface today, but the regex captures
    # any digit so a future printing parses without further work.
    assert _parse_effect(
        "deals 5 damage to target tapped creature"
    ) == DealDamageToTargetTappedCreature(amount=5)


def test_parse_spell_effect_phrase_recognizes_damage_tapped_with_subject():
    # Spell surface — the source-naming subject (``Reroute Systems``)
    # is stripped by ``_normalize_spell_effect_phrase`` before regex
    # match.
    assert _parse_spell_effect_phrase(
        "Reroute Systems deals 2 damage to target tapped creature.",
        "Reroute Systems",
    ) == DealDamageToTargetTappedCreature(amount=2)


def test_parse_effect_does_not_collide_with_bare_target_creature_damage():
    # The bare ``deals N damage to target creature`` regex (which
    # produces ``DealDamageToTargetCreature``) shouldn't accidentally
    # fire on the tapped variant. The ``$`` anchor on the bare regex
    # means it only matches phrases that END at ``creature`` with
    # no trailing qualifier; the tapped form has ``tapped creature``
    # before the end, so the two surface shapes are disjoint.
    bare = _parse_effect("deals 2 damage to target creature")
    tapped = _parse_effect("deals 2 damage to target tapped creature")
    assert bare is not None
    assert tapped is not None
    assert not isinstance(bare, DealDamageToTargetTappedCreature)
    assert isinstance(tapped, DealDamageToTargetTappedCreature)


# ---------- target artifact or creature gains <kw> UEOT ----------

def test_parse_effect_recognizes_pump_artifact_or_creature_indestructible():
    # Reroute Systems mode 0 — grants indestructible to either an
    # artifact or a creature. No P/T pump (the engine class
    # defaults +0/+0); only the keyword grant fires.
    assert _parse_effect(
        "target artifact or creature gains indestructible until end of turn"
    ) == PumpTargetArtifactOrCreatureUntilEOT(
        granted_keywords=frozenset({"indestructible"}),
    )


def test_parse_effect_recognizes_pump_artifact_or_creature_multi_kw():
    # Multi-keyword body — joined by `` and `` and validated against
    # the keyword-line whitelist. No EoE card prints a multi-keyword
    # grant on this exact surface today, but the regex tail (``.+``)
    # plus the existing ``_parse_keyword_grant_body`` helper compose
    # naturally.
    assert _parse_effect(
        "target artifact or creature gains "
        "first strike and vigilance until end of turn"
    ) == PumpTargetArtifactOrCreatureUntilEOT(
        granted_keywords=frozenset({"first strike", "vigilance"}),
    )


def test_parse_effect_returns_none_for_pump_artifact_or_creature_unknown_kw():
    # An unknown keyword fails ``_parse_keyword_grant_body``'s
    # whitelist check, so the wrapping phrase rejects rather than
    # silently dropping the grant. Mirrors the behavior of the
    # creature-only pump-and-grant primitive.
    assert _parse_effect(
        "target artifact or creature gains foobar until end of turn"
    ) is None


def test_parse_spell_effect_phrase_recognizes_pump_artifact_or_creature():
    # Spell surface — capitalized first letter, trailing period.
    assert _parse_spell_effect_phrase(
        "Target artifact or creature gains indestructible until end of turn.",
        "Reroute Systems",
    ) == PumpTargetArtifactOrCreatureUntilEOT(
        granted_keywords=frozenset({"indestructible"}),
    )


# ---------- destroy target artifact, enchantment, or creature with flying ----------

def test_parse_effect_recognizes_destroy_artifact_enchantment_or_flying():
    # Shattered Wings's destroy clause — the legal-targets filter
    # spans artifacts (any), enchantments (any), or creatures with
    # flying. Maps to the engine class with no parameters; the
    # surface form is a literal match.
    assert _parse_effect(
        "destroy target artifact, enchantment, or creature with flying"
    ) == DestroyTargetArtifactEnchantmentOrFlyingCreature()


def test_parse_spell_effect_phrase_recognizes_destroy_three_way():
    # Spell sentence form. Note the Oxford comma after
    # ``enchantment`` is required by the regex.
    assert _parse_spell_effect_phrase(
        "Destroy target artifact, enchantment, or creature with flying.",
        "Shattered Wings",
    ) == DestroyTargetArtifactEnchantmentOrFlyingCreature()


def test_parse_effect_returns_none_for_destroy_three_way_no_oxford_comma():
    # Without the Oxford comma the surface doesn't match — Scryfall
    # always prints the comma, so a missing one would be a parse
    # failure rather than a silent acceptance. Sanity-check the
    # regex stays narrow.
    assert _parse_effect(
        "destroy target artifact, enchantment or creature with flying"
    ) is None


def test_parse_effect_does_not_collide_with_bare_destroy_artifact():
    # The bare ``destroy target artifact`` regex has a ``$`` anchor,
    # so the three-way surface (which has more text after
    # ``artifact``) doesn't collide. Sanity-check both regexes
    # produce their expected (different) engine classes.
    bare = _parse_effect("destroy target artifact")
    three_way = _parse_effect(
        "destroy target artifact, enchantment, or creature with flying"
    )
    assert isinstance(bare, DestroyTargetArtifact)
    assert isinstance(three_way, DestroyTargetArtifactEnchantmentOrFlyingCreature)


# ---------- target artifact or creature's owner puts it ... ----------

def test_parse_effect_recognizes_owner_puts_top_or_bottom():
    # Lost in Space's bounce-to-library clause. The chosen
    # target's owner decides top vs bottom at resolution. Maps to
    # the engine class with no parameters.
    assert _parse_effect(
        "target artifact or creature's owner puts it on their choice of "
        "the top or bottom of their library"
    ) == OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary()


def test_parse_spell_effect_phrase_recognizes_owner_puts_top_or_bottom():
    # Spell surface — capitalized first letter, trailing period.
    assert _parse_spell_effect_phrase(
        "Target artifact or creature's owner puts it on their choice of "
        "the top or bottom of their library.",
        "Lost in Space",
    ) == OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary()


def test_parse_effect_returns_none_for_owner_puts_creature_only():
    # A bare creature-only variant doesn't match — no EoE card
    # prints that shape and the engine class is artifact-or-
    # creature scope. Sanity-check the regex stays narrow.
    assert _parse_effect(
        "target creature's owner puts it on their choice of "
        "the top or bottom of their library"
    ) is None


# ---------- end-to-end card claims ----------

def _claimed_names() -> set[str]:
    return set(OracleParserCardSource().claimed_names)


def test_drill_too_deep_claimed_by_parser():
    # Drill Too Deep `{1}{R}` Instant — Choose one:
    #   - Put five charge counters on target Spacecraft or Planet
    #     you control.
    #   - Destroy target artifact.
    # Mode 0 unlocked by the new charge-counters primitive; mode 1
    # was already supported by the bare ``destroy target artifact``
    # regex. Both modes parse, the modal combinator wraps them in
    # ``ModalChooseOne``, and the parity gate confirms the parser
    # printing matches the static.
    assert "Drill Too Deep" in _claimed_names()


def test_reroute_systems_claimed_by_parser():
    # Reroute Systems `{W}` Instant — Choose one:
    #   - Target artifact or creature gains indestructible until
    #     end of turn.
    #   - Reroute Systems deals 2 damage to target tapped creature.
    # Both modes unlocked by this batch's two new primitives:
    # ``_PUMP_TARGET_ARTIFACT_OR_CREATURE_GAINS_KEYWORDS_UEOT`` for
    # mode 0 and ``_DEAL_DAMAGE_TARGET_TAPPED_CREATURE`` for
    # mode 1.
    assert "Reroute Systems" in _claimed_names()


def test_shattered_wings_claimed_by_parser():
    # Shattered Wings `{2}{G}` Sorcery — destroy
    # artifact/enchantment/creature-with-flying, then Surveil 1.
    # Two-clause Sequence: the first clause unlocked by this
    # batch's destroy primitive; the second clause was already
    # supported by ``_SURVEIL_N``.
    assert "Shattered Wings" in _claimed_names()


def test_lost_in_space_claimed_by_parser():
    # Lost in Space `{3}{U}` Instant — bounce target artifact or
    # creature to top or bottom of owner's library, then Surveil 1.
    # Two-clause Sequence: the first clause unlocked by this
    # batch's owner-puts primitive; the second was already
    # supported by ``_SURVEIL_N``.
    assert "Lost in Space" in _claimed_names()


def test_drill_too_deep_modal_shape():
    # Spot-check the structural shape of the parser's claim — a
    # ``ModalChooseOne`` with two modes, neither of which is
    # ``or_both``. The parity gate covers exact equality with the
    # static; this test pins down the combinator-level shape so
    # accidental Sequence-instead-of-Modal regressions are caught
    # close to the cause.
    printings = OracleParserCardSource().get_printings()
    eff = printings["Drill Too Deep"].spell_effect
    assert isinstance(eff, ModalChooseOne)
    assert eff.or_both is False
    assert len(eff.modes) == 2


def test_shattered_wings_sequence_shape():
    # Spot-check the structural shape — a ``Sequence`` of two
    # primitives. As above, the parity gate covers exact equality;
    # this test pins the combinator level.
    printings = OracleParserCardSource().get_printings()
    eff = printings["Shattered Wings"].spell_effect
    assert isinstance(eff, Sequence)
    assert len(eff.effects) == 2
    assert isinstance(
        eff.effects[0], DestroyTargetArtifactEnchantmentOrFlyingCreature
    )
    assert isinstance(eff.effects[1], Surveil)


# ---------- Void rider replacing pump bonuses (Tragic Trajectory) ----------

def test_parse_void_pump_replacement_recognizes_tragic_trajectory():
    # Tragic Trajectory's printed Void rider — replaces -2/-2 with
    # -10/-10 when Void is active. The base-clause's bonuses don't
    # influence the void-branch result; only the rider's own
    # captured bonuses do.
    base = PumpTargetCreatureUntilEOT(power_bonus=-2, toughness_bonus=-2)
    rider = (
        "Void — That creature gets -10/-10 until end of turn instead "
        "if a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn."
    )
    assert _parse_void_pump_replacement(rider, base) == (
        PumpTargetCreatureUntilEOT(power_bonus=-10, toughness_bonus=-10)
    )


def test_parse_void_pump_replacement_returns_none_for_wrong_base_type():
    # The void rider only matches when the base effect is a pump
    # (``PumpTargetCreatureUntilEOT``). A damage-base + pump-rider
    # mismatch returns ``None`` so the caller can try the next
    # base/rider pairing in turn (see the dispatch chain in
    # ``_parse_spell_oracle_text``).
    from mtgz_ir.cards.base import DealDamageAnyTarget

    base = DealDamageAnyTarget(amount=2)
    rider = (
        "Void — That creature gets -10/-10 until end of turn instead "
        "if a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn."
    )
    assert _parse_void_pump_replacement(rider, base) is None


def test_parse_void_pump_replacement_returns_none_for_wrong_rider_shape():
    # A rider whose surface form doesn't match (e.g. wrong subject
    # or missing the ``instead if …`` tail) returns ``None``. The
    # rider regex is anchored top-to-bottom so partial matches
    # don't accidentally claim the clause.
    base = PumpTargetCreatureUntilEOT(power_bonus=-2, toughness_bonus=-2)
    bad_rider = "Void — That creature gets -10/-10 until end of turn."
    assert _parse_void_pump_replacement(bad_rider, base) is None


def test_parse_spell_oracle_text_assembles_tragic_trajectory():
    # End-to-end: the two-clause oracle text combines via
    # ``ConditionalIfVoid`` with the pump rider as the ``then``
    # branch and the base pump as the ``else`` branch.
    text = (
        "Target creature gets -2/-2 until end of turn.\n"
        "Void — That creature gets -10/-10 until end of turn instead "
        "if a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn."
    )
    assert _parse_spell_oracle_text(text, "Tragic Trajectory") == (
        ConditionalIfVoid(
            then_effect=PumpTargetCreatureUntilEOT(
                power_bonus=-10, toughness_bonus=-10,
            ),
            else_effect=PumpTargetCreatureUntilEOT(
                power_bonus=-2, toughness_bonus=-2,
            ),
        )
    )


def test_tragic_trajectory_claimed_by_parser():
    # Tragic Trajectory `{B}` Sorcery — Void rider on a pump
    # debuff. Unlocked by this batch's ``_parse_void_pump_
    # replacement`` helper that pairs with the existing
    # ``PumpTargetCreatureUntilEOT`` base via the two-clause Void
    # combinator path.
    assert "Tragic Trajectory" in _claimed_names()


def test_tragic_trajectory_conditional_void_shape():
    # Spot-check structural shape — ``ConditionalIfVoid`` wrapping
    # two ``PumpTargetCreatureUntilEOT`` effects with disjoint
    # bonuses. The parity gate covers exact equality with the
    # static; this test pins the combinator-level shape.
    printings = OracleParserCardSource().get_printings()
    eff = printings["Tragic Trajectory"].spell_effect
    assert isinstance(eff, ConditionalIfVoid)
    assert isinstance(eff.then_effect, PumpTargetCreatureUntilEOT)
    assert isinstance(eff.else_effect, PumpTargetCreatureUntilEOT)
    assert (eff.then_effect.power_bonus, eff.then_effect.toughness_bonus) == (
        -10, -10,
    )
    assert (eff.else_effect.power_bonus, eff.else_effect.toughness_bonus) == (
        -2, -2,
    )


def test_is_additional_cost_preamble_recognizes_sacrifice_body():
    # The exact whitelisted body (Embrace Oblivion / Scrounge for
    # Eternity).
    assert _is_additional_cost_preamble(
        "As an additional cost to cast this spell, "
        "sacrifice an artifact or creature."
    )


def test_is_additional_cost_preamble_rejects_unrecognized_body():
    # An unrecognized cost body returns ``False`` so the wrapping
    # spell stays unclaimed (the preamble isn't dropped). The
    # whitelist gates which bodies the parser is willing to
    # silently consume.
    assert not _is_additional_cost_preamble(
        "As an additional cost to cast this spell, pay 3 life."
    )


def test_is_additional_cost_preamble_rejects_non_preamble():
    # A plain effect clause that doesn't start with the
    # additional-cost prefix returns ``False`` — defensive against
    # future cards that mention "additional cost" mid-sentence
    # without printing the preamble shape.
    assert not _is_additional_cost_preamble("Destroy target creature.")


def test_additional_cost_flags_returns_dict_for_recognized_body():
    # Whitelisted body returns the corresponding flag dict.
    flags = _additional_cost_flags(
        "As an additional cost to cast this spell, "
        "sacrifice an artifact or creature."
    )
    assert flags == {
        "additional_cost_sacrifice_creature_or_artifact": True,
    }


def test_additional_cost_flags_returns_empty_for_unrecognized_body():
    # Unrecognized body returns the empty dict — defensively, so
    # a caller that doesn't first check ``_is_additional_cost_
    # preamble`` doesn't accidentally apply spurious flags to the
    # printing.
    assert _additional_cost_flags(
        "As an additional cost to cast this spell, pay 3 life."
    ) == {}


def test_detect_additional_cost_flags_finds_sacrifice_preamble():
    # End-to-end on the full oracle text — the helper walks the
    # leading preamble clauses and returns the accumulated flag
    # dict.
    text = (
        "As an additional cost to cast this spell, "
        "sacrifice an artifact or creature.\n"
        "Destroy target creature or Spacecraft."
    )
    assert _detect_additional_cost_flags(text) == {
        "additional_cost_sacrifice_creature_or_artifact": True,
    }


def test_detect_additional_cost_flags_returns_empty_when_absent():
    # No preamble → empty dict. A spell whose oracle text is just
    # a plain effect line shouldn't carry any spurious cost flags.
    text = "Destroy target creature."
    assert _detect_additional_cost_flags(text) == {}


def test_embrace_oblivion_claimed_by_parser():
    # Embrace Oblivion `{B}` Sorcery — additional cost: sacrifice
    # an artifact or creature; effect: destroy target creature or
    # Spacecraft. Unlocked by:
    #   - the new ``As an additional cost to cast this spell,
    #     sacrifice an artifact or creature.`` preamble parser
    #     (sets ``additional_cost_sacrifice_creature_or_artifact=
    #     True`` on the printing).
    #   - the new ``destroy target creature or Spacecraft`` primitive
    #     (maps to ``DestroyTargetCreature(or_spacecraft=True)``).
    assert "Embrace Oblivion" in _claimed_names()


def test_embrace_oblivion_carries_additional_cost_flag():
    # Spot-check that the additional-cost preamble lands on the
    # printing as the expected flag — the parity gate covers full
    # equality with the static, but pinning the flag here keeps
    # regression noise close to the cause if the preamble parser
    # silently stops setting the flag.
    printings = OracleParserCardSource().get_printings()
    p = printings["Embrace Oblivion"]
    assert p.additional_cost_sacrifice_creature_or_artifact is True


def test_parse_effect_recognizes_destroy_target_creature_or_spacecraft():
    # Embrace Oblivion's destroy clause. Maps to
    # ``DestroyTargetCreature`` with the ``or_spacecraft=True`` flag
    # flipped — the engine class's ``legal_targets`` extends the
    # bare creature pool to also include unstationed Spacecraft.
    from mtgz_ir.cards.base import DestroyTargetCreature

    eff = _parse_effect("destroy target creature or Spacecraft")
    assert eff == DestroyTargetCreature(or_spacecraft=True)


def test_parse_effect_does_not_collide_with_bare_destroy_creature():
    # The bare ``destroy target creature`` regex has a ``$`` anchor
    # so the Spacecraft-tail variant doesn't accidentally match it.
    # Sanity-check both regexes produce their expected (different)
    # engine-class instances.
    from mtgz_ir.cards.base import DestroyTargetCreature

    bare = _parse_effect("destroy target creature")
    spacecraft = _parse_effect("destroy target creature or Spacecraft")
    assert bare == DestroyTargetCreature()
    assert spacecraft == DestroyTargetCreature(or_spacecraft=True)
    # Specifically, ``or_spacecraft`` is False on the bare and True
    # on the variant — the flag is what distinguishes the two
    # engine-class instances.
    assert bare.or_spacecraft is False
    assert spacecraft.or_spacecraft is True


# ---------- whole-spell exact-text dispatch ----------

def test_cut_propulsion_claimed_by_parser():
    # Cut Propulsion `{2}{R}` Instant — target creature self-damages
    # by its power; flying targets self-damage twice. Migrated from the
    # whole-spell exact-text tier to the per-clause grammar: the new
    # multi-sentence literal in ``effects.py`` claims the full
    # two-sentence body as ``CutPropulsionEffect()``.
    assert "Cut Propulsion" in _claimed_names()


def test_cut_propulsion_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the two-sentence body (self-damage
    # by power; flying doubles the damage) as ``CutPropulsionEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    from mtgz_ir.cards.base import CutPropulsionEffect
    body = (
        "Target creature deals damage to itself equal to its power. "
        "If that creature has flying, "
        "it deals twice that much damage to itself instead."
    )
    eff = _parse_spell_effect_phrase(body, "Cut Propulsion")
    assert eff == CutPropulsionEffect()


def test_mutinous_massacre_claimed_by_parser():
    # Mutinous Massacre `{3}{B}{B}{R}{R}` Sorcery — odd/even parity
    # wrath + steal-and-haste-all-creatures rider. The reminder tail
    # ``(Zero is even.)`` is stripped before lookup. Migrated from the
    # whole-spell exact-text tier to the per-clause grammar: the new
    # multi-sentence literal in ``effects.py`` claims the full
    # five-sentence body as ``MutinousMassacreEffect()``.
    assert "Mutinous Massacre" in _claimed_names()


def test_mutinous_massacre_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the five-sentence body
    # as ``MutinousMassacreEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    from mtgz_ir.cards.base import MutinousMassacreEffect
    body = (
        "Choose odd or even. "
        "Destroy each creature with mana value of the chosen quality. "
        "Then gain control of all creatures until end of turn. "
        "Untap them. They gain haste until end of turn."
    )
    eff = _parse_spell_effect_phrase(body, "Mutinous Massacre")
    assert eff == MutinousMassacreEffect()


def test_singularity_rupture_claimed_by_parser():
    # Singularity Rupture `{3}{U}{B}{B}` Sorcery — wrath + opt-in
    # mass mill of half-library. Unlocked by the whole-spell exact-
    # text dispatcher.
    assert "Singularity Rupture" in _claimed_names()


def test_dispatch_singularity_rupture_returns_engine_class():
    # Singularity Rupture migrated from the whole-spell exact-text tier
    # to the per-clause grammar (effects.py's ``_SINGULARITY_RUPTURE``);
    # whole-spell dispatch now falls through. End-to-end claim verified
    # via the active source's printing.

    from mtgz_ir.cards.source import get_active_source
    sr = get_active_source().get_printings().get("Singularity Rupture")
    assert sr is not None
    from mtgz_ir.cards.base import SingularityRuptureEffect
    assert sr.spell_effect == SingularityRuptureEffect()


def test_whole_spell_exact_match_strips_mutinous_massacre_reminder():
    # End-to-end through ``_parse_spell_oracle_text``: Mutinous
    # Massacre's printed oracle text carries a ``(Zero is even.)``
    # reminder tail; the parser must strip that reminder before
    # the dispatcher lookup. Verify the full oracle text (with
    # reminder intact) lands as the dedicated engine class.
    from mtgz_ir.cards.base import MutinousMassacreEffect

    text = (
        "Choose odd or even. "
        "Destroy each creature with mana value of the chosen quality. "
        "Then gain control of all creatures until end of turn. "
        "Untap them. They gain haste until end of turn. "
        "(Zero is even.)"
    )
    assert _parse_spell_oracle_text(text, "Mutinous Massacre") == (
        MutinousMassacreEffect()
    )


def test_void_pump_rider_does_not_perturb_plasma_bolt():
    # Plasma Bolt's existing Void rider (damage-replacement) must
    # still claim Plasma Bolt unchanged after adding the pump-
    # rider helper. The dispatch tries the damage rider first and
    # only falls through to the pump rider when the damage rider
    # rejects, so the Plasma Bolt path is unperturbed.
    assert "Plasma Bolt" in _claimed_names()
    printings = OracleParserCardSource().get_printings()
    eff = printings["Plasma Bolt"].spell_effect
    assert isinstance(eff, ConditionalIfVoid)
    # Plasma Bolt's branches are damage effects, not pumps.
    from mtgz_ir.cards.base import DealDamageAnyTarget
    assert isinstance(eff.then_effect, DealDamageAnyTarget)
    assert isinstance(eff.else_effect, DealDamageAnyTarget)
