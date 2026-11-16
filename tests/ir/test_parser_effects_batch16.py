"""Tests for batch-16 whole-spell exact-text dispatch additions.

This batch extends the ``_WHOLE_SPELL_EXACT_TEXT`` dispatcher in
``parser.spell_builder`` with seven entries — the cluster of EoE
spells whose oracle text is a single sentence-block (or a tightly
bound two-/three-sentence block) that the static registry models
with one engine class. The per-clause grammar can't claim these
because at least one of their internal sentences (e.g. the
``Then if that creature's power is 0 or less, destroy it.`` rider
on Depressurize, or the ``If it has a +1/+1 counter on it, draw a
card.`` rider on Dual-Sun Technique) doesn't parse as a standalone
effect — and matching the per-sentence grammar to those forms
would yield primitives no other card uses.

Newly claimed spells:

* **Depressurize** ``{1}{B}`` Instant — target creature gets -3/-0
  UEOT plus a state-based death check. Maps to
  ``DepressurizeEffect(power_penalty=3)``.
* **Focus Fire** ``{W}`` Instant — variable-X burn whose X reads
  ``2 plus the number of creatures and/or Spacecraft you control``.
  Maps to ``FocusFireEffect(base=2)``.
* **Diplomatic Relations** ``{2}{G}`` Instant — pump-and-grant-
  vigilance on your creature plus a fight-shaped damage rider.
  Two-target spell. Maps to ``DiplomaticRelationsEffect()``.
* **Biosynthic Burst** ``{1}{G}`` Instant — counter + grant-three-
  keywords + untap on a creature you control. Maps to
  ``BiosynthicBurstEffect()``.
* **Zealous Display** ``{2}{W}`` Instant — anthem with an opp-turn-
  untap rider. Maps to
  ``PumpCreaturesYouControlUntilEOT(power_bonus=2,
  also_untap_if_not_your_turn=True)``.
* **Dual-Sun Technique** ``{1}{W}`` Instant — grant double strike
  + conditional draw rider. Maps to
  ``PumpTargetCreatureUntilEOT`` with
  ``draw_if_target_has_plus_one_counter=1``.
* **Invasive Maneuvers** ``{1}{R}`` Instant — bare burn with a
  Spacecraft-state boost. Maps to
  ``DealDamageToTargetCreatureSpacecraftBoosted(base=3, boosted=5)``.

Each spell is exercised by:
  - A claim test confirming the parser's ``claimed_names`` includes
    it.
  - The shared parametrized parity test in
    ``tests/test_parser_parity.py`` enforces that the produced
    ``CardPrinting`` matches the static one (no separate per-card
    parity assert needed here).

(The whole-spell exact-text dispatch table that originally backed
these spells was retired in retire-empty-bespoke-shells once every
entry had been migrated to per-clause / multi-sentence-literal
recognizers in ``effects.py``.)
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    BiosynthicBurstEffect,
    ConditionalIfVoid,
    CreateXTokensOfTargetPermanentWithHasteEndStepSac,
    DealDamageToTargetCreatureSpacecraftBoosted,
    DealDamageToTargetCreatureWithExcessLanderRider,
    DepressurizeEffect,
    DestroyTargetNonlandPermanentWithLanderForController,
    DiplomaticRelationsEffect,
    DrawCards,
    EachOpponentLosesLife,
    FocusFireEffect,
    LoseLife,
    PumpCreaturesYouControlUntilEOT,
    PumpTargetCreatureUntilEOT,
    ReturnTargetCreatureToOwnersHand,
    ReturnUpToNSmallCreaturesFromGraveyardWithCounters,
    SeedshipImpactEffect,
    Sequence,
    Surveil,
    SystemsOverrideEffect,
    ZeroPointBalladEffect,
)
from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
from mtgz_ir.cards.parser_source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- claim tests (end-to-end via the parser source) ----------

def test_depressurize_claimed_by_parser():
    # Depressurize ``{1}{B}`` Instant — -3/-0 UEOT plus the
    # ``Then if that creature's power is 0 or less, destroy it.``
    # rider. Migrated to per-clause grammar in bespoke-spells2 via the
    # new ``_DEPRESSURIZE`` multi-sentence literal in ``effects.py``.
    assert "Depressurize" in _claimed_names()


def test_focus_fire_claimed_by_parser():
    # Focus Fire ``{W}`` Instant — variable-X burn (N plus creatures
    # and/or Spacecraft you control). Claimed via the per-clause
    # grammar (effects.py's
    # ``_DEAL_DAMAGE_X_BASE_PLUS_FRIENDLY_CREATURE_OR_SPACECRAFT_COUNT``
    # recognizer, parameterized over the integer base, mapping to
    # ``FocusFireEffect(base=N)``).
    assert "Focus Fire" in _claimed_names()


def test_diplomatic_relations_claimed_by_parser():
    # Diplomatic Relations ``{2}{G}`` Instant — two-target spell
    # (pump + grant-vigilance on yours, then fight-shaped damage at
    # an opponent's). Two-target compounds aren't supported by the
    # per-clause Sequence combinator, so the spell is claimed via
    # the whole-spell exact-text dispatcher.
    assert "Diplomatic Relations" in _claimed_names()


def test_biosynthic_burst_claimed_by_parser():
    # Biosynthic Burst ``{1}{G}`` Instant — counter + grant three
    # keywords + untap on a creature you control. Migrated from the
    # whole-spell exact-text tier to the per-clause grammar: the new
    # multi-sentence literal recognizer in ``effects.py`` claims the
    # full three-sentence body as ``BiosynthicBurstEffect()``.
    assert "Biosynthic Burst" in _claimed_names()


def test_zealous_display_claimed_by_parser():
    # Zealous Display ``{2}{W}`` Instant — anthem with the
    # ``If it's not your turn, untap those creatures.`` rider.
    # Migrated from the whole-spell exact-text tier to the per-clause
    # grammar (effects.py's
    # ``_PUMP_YOUR_CREATURES_UEOT_UNTAP_IF_NOT_YOUR_TURN``).
    assert "Zealous Display" in _claimed_names()


def test_dual_sun_technique_claimed_by_parser():
    # Dual-Sun Technique ``{1}{W}`` Instant — grant double strike
    # plus conditional draw rider (``If it has a +1/+1 counter on
    # it, draw a card.``). Migrated from the whole-spell exact-text
    # tier to the per-clause grammar (effects.py's
    # ``_GRANT_KEYWORDS_TO_TARGET_CREATURE_YOU_CONTROL_DRAW_IF_COUNTER_
    # UEOT``); claim is end-to-end via the active source's printing.
    assert "Dual-Sun Technique" in _claimed_names()


def test_invasive_maneuvers_claimed_by_parser():
    # Invasive Maneuvers ``{1}{R}`` Instant — bare burn with a
    # Spacecraft-state boost. Migrated from the whole-spell exact-
    # text tier to the per-clause grammar (effects.py's
    # ``_DEAL_DAMAGE_N_TARGET_CREATURE_M_IF_SPACECRAFT``); claim is
    # end-to-end via the active source's printing.
    assert "Invasive Maneuvers" in _claimed_names()


# ---------- dispatch tests (direct lookup) ----------


def test_dispatch_focus_fire_returns_engine_class():
    # Focus Fire migrated from the whole-spell exact-text tier to the
    # per-clause grammar (effects.py's
    # ``_DEAL_DAMAGE_X_BASE_PLUS_FRIENDLY_CREATURE_OR_SPACECRAFT_COUNT``);
    # whole-spell dispatch now falls through. End-to-end claim
    # verified via the active source's printing — the recognizer's
    # captured ``base=2`` integer parameterizes the engine class.
    from mtgz_ir.cards.source import get_active_source
    p = get_active_source().get_printings().get("Focus Fire")
    assert p is not None
    assert p.spell_effect == FocusFireEffect(base=2)


def test_diplomatic_relations_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the full two-sentence body
    # (pump + vigilance UEOT on your creature; anaphoric "It" deals
    # power damage to opp creature) as ``DiplomaticRelationsEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Target creature you control gets +1/+0 and gains vigilance "
        "until end of turn. It deals damage equal to its power to "
        "target creature an opponent controls."
    )
    eff = _parse_spell_effect_phrase(body, "Diplomatic Relations")
    assert eff == DiplomaticRelationsEffect()


def test_biosynthic_burst_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the full three-sentence body
    # as ``BiosynthicBurstEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Put a +1/+1 counter on target creature you control. "
        "It gains reach, trample, and indestructible until end of "
        "turn. Untap it."
    )
    eff = _parse_spell_effect_phrase(body, "Biosynthic Burst")
    assert eff == BiosynthicBurstEffect()


def test_zealous_display_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar claims the 2-sentence
    # body with the untap-if-not-your-turn rider.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Creatures you control get +2/+0 until end of turn. "
        "If it's not your turn, untap those creatures."
    )
    eff = _parse_spell_effect_phrase(body, "Zealous Display")
    assert eff == PumpCreaturesYouControlUntilEOT(
        power_bonus=2,
        toughness_bonus=0,
        also_untap_if_not_your_turn=True,
    )


def test_dual_sun_technique_parses_via_per_clause_grammar():
    # Belt-and-suspenders that the per-clause grammar correctly claims
    # the 2-sentence body with the keyword grant and conditional draw.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Target creature you control gains double strike until end "
        "of turn. If it has a +1/+1 counter on it, draw a card."
    )
    eff = _parse_spell_effect_phrase(body, "Dual-Sun Technique")
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=0,
        toughness_bonus=0,
        granted_keywords=frozenset({"double strike"}),
        you_control_only=True,
        draw_if_target_has_plus_one_counter=1,
    )


def test_dispatch_invasive_maneuvers_returns_engine_class():
    # Invasive Maneuvers migrated from the whole-spell exact-text
    # tier to the per-clause grammar (effects.py's
    # ``_DEAL_DAMAGE_N_TARGET_CREATURE_M_IF_SPACECRAFT``); whole-
    # spell dispatch now falls through. End-to-end claim verified
    # via the active source's printing carrying
    # ``DealDamageToTargetCreatureSpacecraftBoosted(base=3,
    # boosted=5)`` — the captured integers (``3``, ``5``)
    # parameterize the engine class off the printed base/boosted
    # damage values.
    from mtgz_ir.cards.source import get_active_source
    im = get_active_source().get_printings().get("Invasive Maneuvers")
    assert im is not None
    assert im.spell_effect == DealDamageToTargetCreatureSpacecraftBoosted(
        base=3, boosted=5,
    )


# ---------- end-to-end through ``_parse_spell_oracle_text`` ----------

def test_parse_spell_oracle_text_depressurize_parses_via_per_clause_grammar():
    # Depressurize migrated to per-clause grammar in bespoke-spells2:
    # the new ``_DEPRESSURIZE`` multi-sentence literal in ``effects.py``
    # claims the two-sentence body before the compound-effect splitter
    # runs, because the splitter's second piece ("Then if that creature's
    # power is 0 or less, destroy it") doesn't parse as a standalone
    # effect and the compound would reject.
    text = (
        "Target creature gets -3/-0 until end of turn. "
        "Then if that creature's power is 0 or less, destroy it."
    )
    assert _parse_spell_oracle_text(text, "Depressurize") == (
        DepressurizeEffect(power_penalty=3)
    )


def test_parse_spell_oracle_text_strips_biosynthic_burst_reminder():
    # Biosynthic Burst's printed oracle text carries a parenthesized
    # reminder tail describing indestructible (``(Damage and effects
    # that say "destroy" don't destroy it.)``). The parser must strip
    # that reminder before the per-clause recognizer fires. Now claimed
    # via the per-clause multi-sentence literal in ``effects.py`` rather
    # than the whole-spell exact-text tier (migrated in bespoke-spells4).
    text = (
        "Put a +1/+1 counter on target creature you control. "
        "It gains reach, trample, and indestructible until end of "
        "turn. Untap it. (Damage and effects that say \"destroy\" "
        "don't destroy it.)"
    )
    assert _parse_spell_oracle_text(text, "Biosynthic Burst") == (
        BiosynthicBurstEffect()
    )


def test_parse_depressurize_typo_no_match():
    # A one-word paraphrase of Depressurize's oracle text ("0 or fewer"
    # vs "0 or less") must not be claimed by the per-clause grammar's
    # ``_DEPRESSURIZE`` regex — verifying the recognizer is exact-match,
    # not fuzzy.  Both the whole-spell tier and the per-clause grammar
    # should return None for the malformed text.
    assert _parse_spell_oracle_text(
        "Target creature gets -3/-0 until end of turn. "
        "Then if that creature's power is 0 or fewer, destroy it.",
        "Depressurize",
    ) is None


# ---------- Sequence-shaped spells (Cerebral Download,
#            Decode Transmissions, Hymn of the Faller) ----------
#
# Decode Transmissions and Hymn of the Faller have a Void rider that
# REPLACES / ADDS to the base; both migrated to the per-clause grammar
# in bespoke-spells1.
#
# Cerebral Download ("Surveil X, where X is the number of artifacts you
# control. Then draw three cards.") migrated to the per-clause grammar in
# bespoke-spells2: the new ``_SURVEIL_X_ARTIFACT_COUNT`` recognizer in
# ``effects.py`` claims the first sentence as
# ``Surveil(amount=1, use_artifact_count=True)`` and ``_DRAW_N_CARDS``
# claims the second sentence after ``_split_effect_sentences`` strips the
# leading "then "; the Sequence combinator assembles the pair.

def test_cerebral_download_claimed_by_parser():
    # Cerebral Download ``{4}{U}`` Instant — variable surveil + draw
    # 3. Now claimed by the per-clause grammar via the new
    # ``_SURVEIL_X_ARTIFACT_COUNT`` recognizer.
    assert "Cerebral Download" in _claimed_names()


def test_decode_transmissions_claimed_by_parser():
    # Decode Transmissions ``{2}{B}`` Sorcery — draw 2 + Void-
    # "instead" rider. Migrated from the whole-spell exact-text tier
    # to the per-clause grammar via the new Void "instead" rider
    # recognizer (_SPELL_VOID_INSTEAD_EFFECT_RE).
    assert "Decode Transmissions" in _claimed_names()


def test_hymn_of_the_faller_claimed_by_parser():
    # Hymn of the Faller ``{1}{B}`` Sorcery — surveil 1, draw 1,
    # lose 1, plus a Void-conditional extra draw rider. Migrated
    # from the whole-spell exact-text tier to the per-clause grammar
    # via the new Void "adds-draw" rider recognizer.
    assert "Hymn of the Faller" in _claimed_names()


def test_cerebral_download_parses_via_per_clause_grammar():
    # The per-clause grammar produces Sequence(Surveil(...), DrawCards(3))
    # for Cerebral Download's printed oracle text — the same structure the
    # static side ships. The ``_SURVEIL_X_ARTIFACT_COUNT`` recognizer
    # claims the first sentence; ``_split_effect_sentences`` strips the
    # "then " prefix from the second sentence so ``_DRAW_N_CARDS`` can
    # match "draw three cards".
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Surveil X, where X is the number of artifacts you control. "
        "Then draw three cards.",
        "Cerebral Download",
    )
    assert eff == Sequence(effects=(
        Surveil(amount=1, use_artifact_count=True),
        DrawCards(amount=3),
    ))


def test_decode_transmissions_parses_via_per_clause_grammar():
    # The per-clause grammar produces ConditionalIfVoid whose then-
    # branch is the full "instead" effect and else-branch is the base.
    # Both branches draw 2 cards and differ only in the life-loss
    # target — semantically equivalent to the former bespoke form
    # Sequence(DrawCards(2), ConditionalIfVoid(EachOpponentLosesLife(2),
    # LoseLife(2))).
    from mtgz_ir.cards.base import EachOpponentLosesLife
    eff = _parse_spell_oracle_text(
        "You draw two cards and lose 2 life.\n"
        "Void — If a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn, instead you draw two cards "
        "and each opponent loses 2 life.",
        "Decode Transmissions",
    )
    assert eff == ConditionalIfVoid(
        then_effect=Sequence(effects=(
            DrawCards(amount=2),
            EachOpponentLosesLife(amount=2),
        )),
        else_effect=Sequence(effects=(
            DrawCards(amount=2),
            LoseLife(amount=2),
        )),
    )


def test_hymn_of_the_faller_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar produces the same
    # Sequence structure as the static registry.
    eff = _parse_spell_oracle_text(
        "Surveil 1, then you draw a card and lose 1 life.\n"
        "Void — If a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn, draw another card.",
        "Hymn of the Faller",
    )
    assert eff == Sequence(effects=(
        Surveil(amount=1, use_artifact_count=False),
        DrawCards(amount=1),
        LoseLife(amount=1),
        ConditionalIfVoid(
            then_effect=DrawCards(amount=1),
            else_effect=None,
        ),
    ))


def test_desculpting_blast_claimed_by_parser():
    # Desculpting Blast ``{1}{U}`` Instant — bounce-target-permanent
    # plus a Drone-token rider for the attacking-permanent branch.
    assert "Desculpting Blast" in _claimed_names()


def test_emergency_eject_claimed_by_parser():
    # Emergency Eject ``{2}{W}`` Instant — destroy + Lander-for-
    # the-destroyed-permanent's-controller rider. Migrated from the
    # whole-spell exact-text tier to the per-clause grammar (effects.py's
    # ``_DESTROY_TARGET_NONLAND_PERMANENT_ITS_CONTROLLER_CREATES_LANDER``).
    assert "Emergency Eject" in _claimed_names()


def test_orbital_plunge_claimed_by_parser():
    # Orbital Plunge ``{3}{R}`` Sorcery — 6-damage burn + excess-
    # damage Lander rider. Migrated from the whole-spell exact-text
    # tier to the per-clause grammar (effects.py's
    # ``_DEAL_DAMAGE_N_TARGET_CREATURE_WITH_EXCESS_LANDER_RIDER``);
    # claim is end-to-end via the active source's printing.
    assert "Orbital Plunge" in _claimed_names()


def test_seedship_impact_claimed_by_parser():
    # Seedship Impact ``{1}{G}`` Instant — destroy artifact-or-
    # enchantment + small-mv Lander rider. Migrated from the whole-
    # spell exact-text tier to the per-clause grammar (effects.py's
    # ``_SEEDSHIP_IMPACT``); claim is end-to-end via the active
    # source's printing.
    assert "Seedship Impact" in _claimed_names()


def test_scout_for_survivors_claimed_by_parser():
    # Scout for Survivors ``{2}{W}`` Sorcery — return up to three
    # small creature cards from graveyard to battlefield with a
    # +1/+1 counter on each.
    assert "Scout for Survivors" in _claimed_names()


def test_zero_point_ballad_claimed_by_parser():
    # Zero Point Ballad ``{X}{B}`` Sorcery — variable-X wrath +
    # life-pay + reanimation-on-large-X rider.
    assert "Zero Point Ballad" in _claimed_names()


def test_systems_override_claimed_by_parser():
    # Systems Override ``{2}{R}`` Sorcery — gain-control + untap +
    # haste plus a Spacecraft-only ten-charge-counter rider. Migrated
    # from the whole-spell exact-text tier to the per-clause grammar:
    # the new multi-sentence literal recognizer in ``effects.py`` claims
    # the full five-sentence body as ``SystemsOverrideEffect()``.
    assert "Systems Override" in _claimed_names()


def test_devastating_onslaught_claimed_by_parser():
    # Devastating Onslaught ``{X}{X}{R}`` Sorcery — variable-X
    # token-copy spell with haste and end-step sac.
    assert "Devastating Onslaught" in _claimed_names()


def test_full_bore_claimed_by_parser():
    # Full Bore ``{R}`` Instant — pump-your-creature with warp-
    # conditional extra keywords (trample, haste). Migrated from
    # the whole-spell exact-text tier to the per-clause grammar
    # (effects.py's ``_PUMP_TARGET_CREATURE_YOU_CONTROL_WARP_
    # KEYWORDS_RIDER_UEOT``).
    assert "Full Bore" in _claimed_names()


def test_desculpting_blast_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the two-sentence body (bounce
    # nonland permanent; create Drone token if it was attacking) as
    # ``ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True,
    # create_drone_if_was_attacking=True)``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Return target nonland permanent to its owner's hand. "
        "If it was attacking, create a 1/1 colorless Drone artifact "
        "creature token with flying and \"This token can block only "
        "creatures with flying.\""
    )
    eff = _parse_spell_effect_phrase(body, "Desculpting Blast")
    from mtgz_ir.cards.base import CreateToken
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    assert eff == ReturnTargetCreatureToOwnersHand(
        allow_nonland_permanent=True,
        if_was_attacking=CreateToken(
            token_printing=TOKEN_PRINTINGS["Drone"],
        ),
    )


def test_emergency_eject_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar claims the 2-sentence
    # body with the Lander-for-controller rider.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Destroy target nonland permanent. "
        "Its controller creates a Lander token."
    )
    eff = _parse_spell_effect_phrase(body, "Emergency Eject")
    assert eff == DestroyTargetNonlandPermanentWithLanderForController()


def test_dispatch_orbital_plunge_returns_engine_class():
    # Orbital Plunge migrated from the whole-spell exact-text tier
    # to the per-clause grammar (effects.py's
    # ``_DEAL_DAMAGE_N_TARGET_CREATURE_WITH_EXCESS_LANDER_RIDER``);
    # whole-spell dispatch now falls through. End-to-end claim
    # verified via the active source's printing carrying
    # ``DealDamageToTargetCreatureWithExcessLanderRider(amount=6)``
    # — the captured integer (``6``) parameterizes the engine class
    # off the printed damage.
    from mtgz_ir.cards.source import get_active_source
    op = get_active_source().get_printings().get("Orbital Plunge")
    assert op is not None
    assert op.spell_effect == DealDamageToTargetCreatureWithExcessLanderRider(
        amount=6,
    )


def test_dispatch_seedship_impact_returns_engine_class():
    # Seedship Impact migrated from the whole-spell exact-text tier
    # to the per-clause grammar (effects.py's ``_SEEDSHIP_IMPACT``);
    # whole-spell dispatch now falls through. End-to-end claim
    # verified via the active source's printing carrying
    # ``SeedshipImpactEffect()`` (parameterless, only card with this
    # shape today — see the recognizer's docstring for the deferred-
    # generalization plan).
    from mtgz_ir.cards.source import get_active_source
    si = get_active_source().get_printings().get("Seedship Impact")
    assert si is not None
    assert si.spell_effect == SeedshipImpactEffect()


def test_scout_for_survivors_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the two-sentence body (return up
    # to three creature cards with total MV ≤ 3 from GY to battlefield;
    # put a +1/+1 counter on each) as
    # ``ReturnUpToNSmallCreaturesFromGraveyardWithCounters(max_n=3,
    # max_total_mv=3)``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Return up to three target creature cards with total mana "
        "value 3 or less from your graveyard to the battlefield. "
        "Put a +1/+1 counter on each of them."
    )
    eff = _parse_spell_effect_phrase(body, "Scout for Survivors")
    assert eff == ReturnUpToNSmallCreaturesFromGraveyardWithCounters(
        max_n=3, max_total_mv=3,
    )


def test_zero_point_ballad_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the three-sentence body
    # (X-wrath + X-life-loss + conditional reanimate) as
    # ``ZeroPointBalladEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Destroy all creatures with toughness X or less. You lose "
        "X life. If X is 6 or more, return a creature card put into "
        "a graveyard this way to the battlefield under your control."
    )
    eff = _parse_spell_effect_phrase(body, "Zero Point Ballad")
    assert eff == ZeroPointBalladEffect()


def test_systems_override_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the full five-sentence body
    # (Threaten core + Spacecraft charge-counter rider) as
    # ``SystemsOverrideEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Gain control of target artifact or creature until end of "
        "turn. Untap that permanent. It gains haste until end of "
        "turn. If it's a Spacecraft, put ten charge counters on it. "
        "If you do, remove ten charge counters from it at the "
        "beginning of the next end step."
    )
    eff = _parse_spell_effect_phrase(body, "Systems Override")
    assert eff == SystemsOverrideEffect()


def test_devastating_onslaught_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the three-sentence body
    # (create X token-copies + grant haste + end-step sac) as
    # ``CreateXTokensOfTargetPermanentWithHasteEndStepSac()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Create X tokens that are copies of target artifact or "
        "creature you control. Those tokens gain haste until end "
        "of turn. Sacrifice them at the beginning of the next end "
        "step."
    )
    eff = _parse_spell_effect_phrase(body, "Devastating Onslaught")
    assert eff == CreateXTokensOfTargetPermanentWithHasteEndStepSac()


def test_full_bore_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar claims the 2-sentence
    # body with the warp-conditional keyword grant.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    body = (
        "Target creature you control gets +3/+2 until end of turn. "
        "If that creature was cast for its warp cost, it also gains "
        "trample and haste until end of turn."
    )
    eff = _parse_spell_effect_phrase(body, "Full Bore")
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=3,
        toughness_bonus=2,
        you_control_only=True,
        granted_keywords_if_warp_cast=frozenset({"trample", "haste"}),
    )


def test_dispatch_strips_hymn_of_the_faller_reminder():
    # End-to-end through ``_parse_spell_oracle_text``: the surveil-1
    # parenthesized reminder ``(To surveil 1, ...)`` is per-clause
    # stripped BEFORE the dispatcher key lookup. The Void rider line
    # has no parenthesized reminder, so it survives intact.
    text = (
        "Surveil 1, then you draw a card and lose 1 life. "
        "(To surveil 1, look at the top card of your library. "
        "You may put it into your graveyard.)\n"
        "Void — If a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn, draw another card."
    )
    assert _parse_spell_oracle_text(text, "Hymn of the Faller") == (
        Sequence(effects=(
            Surveil(amount=1, use_artifact_count=False),
            DrawCards(amount=1),
            LoseLife(amount=1),
            ConditionalIfVoid(
                then_effect=DrawCards(amount=1),
                else_effect=None,
            ),
        ))
    )
