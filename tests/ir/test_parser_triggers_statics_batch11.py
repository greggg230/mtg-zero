"""Batch-11 trigger phrases + static-condition phrasebook additions.

Two surfaces grow this batch — the type-qualified self-ETB / self-attack
trigger family on the trigger side and a forward-looking N=3 spell-cast
threshold on the static side.

Trigger phrases (5 new entries on the predicate path):

* ``this Spacecraft enters`` → ``(Event.ENTERS, IsSelf())``. 13 EoE
  printings carry this phrase verbatim (Atmospheric Greenhouse,
  Debris Field Crusher, Extinguisher Battleship, Fell Gravship, Larval
  Scoutlander, Pinnacle Kill-Ship, Rescue Skiff, Specimen Freighter,
  Susurian Dirgecraft, Uthros Scanship, Warmaker Gunship, Wedgelight
  Rammer, Wurmwall Sweeper). Every EoE Spacecraft is an
  artifact-by-default permanent that gains a creature side via the
  Station mechanic, so its oracle text doesn't currently flow through
  the creature-builder ``_parse_clause`` pipeline that reads this
  phrasebook — the addition is forward-looking grammar coverage in the
  same shape as batch-10's ``"it's your turn"`` static-condition entry.
* ``this artifact enters`` → ``(Event.ENTERS, IsSelf())``. Three EoE
  printings (Dubious Delicacy, Melded Moxite, Pinnacle Starcage). All
  non-creature artifacts.
* ``this enchantment enters`` → ``(Event.ENTERS, IsSelf())``. Three EoE
  printings (Banishing Light, Seam Rip, Weftwalking).
* ``this Aura enters`` → ``(Event.ENTERS, IsSelf())``. Three EoE
  printings (Hardlight Containment, Pain for All, Tractor Beam).
* ``this Spacecraft attacks`` → ``(Event.ATTACKS, IsSelf())``. Three
  EoE printings (Entropic Battlecruiser, Sledge-Class Seedship,
  Specimen Freighter).

Each entry maps to the same ``IsSelf()`` predicate as the pre-existing
``"this creature enters"`` / ``"this creature attacks"`` siblings —
the predicate's object-identity check is type-blind, so the type
qualifier in the printed text doesn't change the semantics. The
static-card registry already builds every Spacecraft / artifact / Aura
ETB-or-attack trigger via ``IsSelf()`` directly (see
``cards/eoe_spells.py``: Pinnacle Starcage, Tractor Beam, Larval
Scoutlander, Specimen Freighter, etc.) so parser/static-registry
parity holds out of the box for the day a creature-typed printing
ships a type-qualified phrase that does flow through the parser.

Static-ability conditions (1 new entry):

* ``you've cast three or more spells this turn`` →
  ``requires_spells_cast_this_turn=3``. Forward-looking N=3 variant of
  the existing Brightspear Zealot ``two or more`` phrase. Same engine
  gate field; the engine's
  ``StaticAbility.requires_spells_cast_this_turn`` accepts any
  non-negative integer threshold.

No newly-supported card claims its oracle text end-to-end this batch:
every EoE printing that carries one of the new trigger phrases is a
non-creature card type whose builder doesn't route through the
creature-builder ``_parse_clause`` pipeline, and the new static phrase
has no EoE printing today. The batch is grammar coverage only,
mirroring the structure of batch 10.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DrawCards,
    PutPlusOneCounterOnSelf,
    PutPlusOneCounterOnTargetCreature,
    StaticAbility,
    StaticScope,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.statics import (
    _STATIC_CONDITION_KWARGS,
    _parse_static_ability_line,
)
from mtgz_ir.cards.parser.triggers import (
    _PREDICATE_TRIGGER_PHRASES,
    _parse_predicate_trigger_phrase,
    _parse_trigger_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.effects.conditions import AtLeastNSpellsCastThisTurn
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


# Type-qualified self-ETB / self-attack trigger phrases — every entry
# maps to the same ``IsSelf()`` predicate as the existing
# ``"this creature enters"`` / ``"this creature attacks"`` siblings.
_SELF_ETB_PRED = IsSelf()
_SELF_ATTACKS_PRED = IsSelf()


# ---------- this Spacecraft enters ----------

def test_parse_predicate_trigger_phrase_recognizes_this_spacecraft_enters():
    # 13 EoE Spacecraft printings carry this phrase verbatim. The
    # legacy string-key lookup must NOT also claim it — phases 4-9
    # emptied that table for every parsed phrase.
    assert _parse_trigger_phrase("this Spacecraft enters") is None
    assert _parse_predicate_trigger_phrase(
        "this Spacecraft enters"
    ) == (Event.ENTERS, _SELF_ETB_PRED)


def test_this_spacecraft_enters_end_to_end():
    # Spacecraft ETB effects in EoE are uniformly bespoke (return-from-
    # graveyard, deal-damage-to-each-creature, mill-and-reanimate,
    # etc.) and not in the effect grammar today; pair the new condition
    # with the existing ``draw a card`` effect so the end-to-end test
    # focuses on the trigger-phrase shape. The parsed ability mirrors
    # the static registry's Spacecraft ETB pattern exactly — every
    # EoE Spacecraft static printing builds via
    # ``TriggeredAbility(event=Event.ENTERS, predicate=IsSelf(), …)``.
    parsed = _parse_triggered_ability_line(
        "When this Spacecraft enters, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_SELF_ETB_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- this artifact enters ----------

def test_parse_predicate_trigger_phrase_recognizes_this_artifact_enters():
    # Pinnacle Starcage, Dubious Delicacy, Melded Moxite carry this
    # phrase verbatim. Distinct from the existing
    # ``"an artifact you control enters"`` / ``"another artifact you
    # control enters"`` (broader scope) phrases — those are the
    # observer pattern (any artifact you control, possibly excluding
    # the source); this one is the source's own ETB.
    assert _parse_trigger_phrase("this artifact enters") is None
    assert _parse_predicate_trigger_phrase(
        "this artifact enters"
    ) == (Event.ENTERS, _SELF_ETB_PRED)


def test_this_artifact_enters_end_to_end():
    parsed = _parse_triggered_ability_line(
        "When this artifact enters, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_SELF_ETB_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- this enchantment enters ----------

def test_parse_predicate_trigger_phrase_recognizes_this_enchantment_enters():
    # Banishing Light, Seam Rip, Weftwalking carry this phrase verbatim.
    # The ``this Aura enters`` sibling (different surface phrase) is its
    # own entry below — Banishing Light is an enchantment but not an
    # Aura, so the two phrasings really are distinct.
    assert _parse_trigger_phrase("this enchantment enters") is None
    assert _parse_predicate_trigger_phrase(
        "this enchantment enters"
    ) == (Event.ENTERS, _SELF_ETB_PRED)


def test_this_enchantment_enters_end_to_end():
    parsed = _parse_triggered_ability_line(
        "When this enchantment enters, draw a card."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_SELF_ETB_PRED,
        effect=DrawCards(amount=1),
    )


# ---------- this Aura enters ----------

def test_parse_predicate_trigger_phrase_recognizes_this_aura_enters():
    # Hardlight Containment, Pain for All, Tractor Beam carry this
    # phrase verbatim. The static registry for each of those auras
    # already builds this trigger via
    # ``TriggeredAbility(event=Event.ENTERS, predicate=IsSelf(), …)``
    # so parity holds for any future creature-typed Aura that flows
    # through the parser pipeline.
    assert _parse_trigger_phrase("this Aura enters") is None
    assert _parse_predicate_trigger_phrase(
        "this Aura enters"
    ) == (Event.ENTERS, _SELF_ETB_PRED)


def test_this_aura_enters_end_to_end():
    # Pair with ``put a +1/+1 counter on target creature`` for variety
    # — the Aura ETB effects in EoE (tap enchanted permanent, exile
    # opponent's creature, deal damage equal to power) aren't in the
    # effect grammar today.
    parsed = _parse_triggered_ability_line(
        "When this Aura enters, put a +1/+1 counter on target creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.ENTERS, predicate=_SELF_ETB_PRED,
        effect=PutPlusOneCounterOnTargetCreature(amount=1),
    )


# ---------- this Spacecraft attacks ----------

def test_parse_predicate_trigger_phrase_recognizes_this_spacecraft_attacks():
    # Entropic Battlecruiser, Sledge-Class Seedship, Specimen Freighter
    # carry this phrase verbatim. Same predicate as the pre-existing
    # ``"this creature attacks"`` (Spacecraft attack triggers, Stellar
    # Sights creatures) — the attacking object IS the source.
    assert _parse_trigger_phrase("this Spacecraft attacks") is None
    assert _parse_predicate_trigger_phrase(
        "this Spacecraft attacks"
    ) == (Event.ATTACKS, _SELF_ATTACKS_PRED)


def test_this_spacecraft_attacks_end_to_end():
    # Pair with ``put a +1/+1 counter on this creature`` — the
    # Spacecraft attack effects in EoE (mill, opponent-discard, put a
    # creature card from hand onto the battlefield) aren't in the
    # effect grammar today.
    parsed = _parse_triggered_ability_line(
        "Whenever this Spacecraft attacks, put a +1/+1 counter on this "
        "creature."
    )
    assert parsed == TriggeredAbility(
        event=Event.ATTACKS, predicate=_SELF_ATTACKS_PRED,
        effect=PutPlusOneCounterOnSelf(amount=1),
    )


# ---------- Cross-phrase parity: every new trigger maps to IsSelf() ----------

def test_all_new_trigger_phrases_share_the_self_predicate():
    # A predicate-shape regression check — if a future refactor splits
    # ``IsSelf()`` into multiple subject-scoped variants, this test
    # tells you which entries need to be revisited. The five new
    # phrases all collapse to the same identity-based predicate; only
    # the event differs (ENTERS for the four ETB variants, ATTACKS for
    # the one attack variant).
    enters_phrases = [
        "this Spacecraft enters",
        "this artifact enters",
        "this enchantment enters",
        "this Aura enters",
    ]
    for phrase in enters_phrases:
        match = _parse_predicate_trigger_phrase(phrase)
        assert match == (Event.ENTERS, IsSelf()), phrase
    attacks_match = _parse_predicate_trigger_phrase(
        "this Spacecraft attacks"
    )
    assert attacks_match == (Event.ATTACKS, IsSelf())


# ---------- Phrasebook integrity (trigger side) ----------

def test_new_trigger_phrases_pinned_in_phrasebook():
    # Pin every new entry so an accidental revert is loud.
    for phrase in (
        "this Spacecraft enters",
        "this artifact enters",
        "this enchantment enters",
        "this Aura enters",
    ):
        assert phrase in _PREDICATE_TRIGGER_PHRASES, phrase
        event, pred = _PREDICATE_TRIGGER_PHRASES[phrase]
        assert event == Event.ENTERS
        assert pred == IsSelf()
    assert "this Spacecraft attacks" in _PREDICATE_TRIGGER_PHRASES
    event, pred = _PREDICATE_TRIGGER_PHRASES["this Spacecraft attacks"]
    assert event == Event.ATTACKS
    assert pred == IsSelf()


# ---------- Negative trigger cases ----------

def test_unrelated_type_qualifier_returns_none():
    # ``this planeswalker enters`` would be the natural extension to
    # planeswalkers but they aren't in the EoE pool and the engine
    # doesn't model them; don't claim the phrase silently.
    assert _parse_predicate_trigger_phrase(
        "this planeswalker enters"
    ) is None


def test_creature_phrase_still_resolves_after_batch11():
    # The pre-existing ``"this creature enters"`` / ``"this creature
    # attacks"`` entries must still resolve after the type-qualified
    # additions — adding more keys to the dict can't shadow existing
    # ones, but pinning the behavior here makes that contract explicit.
    assert _parse_predicate_trigger_phrase(
        "this creature enters"
    ) == (Event.ENTERS, IsSelf())
    assert _parse_predicate_trigger_phrase(
        "this creature attacks"
    ) == (Event.ATTACKS, IsSelf())


# ---------- Static-condition: "you've cast three or more spells this turn" ----------

def test_three_or_more_spells_leading_shape_parses_to_static():
    # PR-D continuous-effects shape: P/T contribution rides on a
    # layer-7c ContinuousModifier(PowerToughnessBonus); the legacy
    # requires_spells_cast_this_turn gate stays as the surface today.
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, PowerToughnessBonus,
    )
    parsed = _parse_static_ability_line(
        "As long as you've cast three or more spells this turn, this "
        "creature gets +3/+0."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=3, toughness_bonus=0),
        ),
        condition=AtLeastNSpellsCastThisTurn(3),
    ),)


def test_three_or_more_spells_trailing_shape_parses_to_static():
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, PowerToughnessBonus,
    )
    parsed = _parse_static_ability_line(
        "This creature gets +3/+0 as long as you've cast three or more "
        "spells this turn."
    )
    assert parsed == (StaticAbility(
        scope=StaticScope.SELF,
        continuous_effect=ContinuousModifier(
            layer=7,
            payload=PowerToughnessBonus(power_bonus=3, toughness_bonus=0),
        ),
        condition=AtLeastNSpellsCastThisTurn(3),
    ),)


def test_three_or_more_spells_leading_and_trailing_are_equivalent():
    leading = _parse_static_ability_line(
        "As long as you've cast three or more spells this turn, this "
        "creature gets +1/+1."
    )
    trailing = _parse_static_ability_line(
        "This creature gets +1/+1 as long as you've cast three or more "
        "spells this turn."
    )
    assert leading is not None
    assert trailing is not None
    assert leading == trailing


def test_three_or_more_spells_with_keyword_rider_leading():
    # Compound P/T + keyword statics fan out into two StaticAbility
    # instances — one for the layer-7c P/T contribution and one for
    # the layer-6 GrantKeyword.
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, GrantKeyword, PowerToughnessBonus,
    )
    parsed = _parse_static_ability_line(
        "As long as you've cast three or more spells this turn, this "
        "creature gets +2/+0 and has trample."
    )
    assert parsed == (
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(
                    power_bonus=2, toughness_bonus=0,
                ),
            ),
            condition=AtLeastNSpellsCastThisTurn(3),
        ),
        StaticAbility(
            scope=StaticScope.SELF,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("trample"),
            ),
            condition=AtLeastNSpellsCastThisTurn(3),
        ),
    )


def test_three_or_more_spells_distinct_from_two_or_more():
    # The ``two or more`` and ``three or more`` phrases must produce
    # ``StaticAbility`` instances that disagree on
    # ``requires_spells_cast_this_turn`` — otherwise the engine gate
    # at ``StaticAbility.applies_to`` would treat them identically and
    # the two thresholds would collapse to "any positive number".
    two = _parse_static_ability_line(
        "As long as you've cast two or more spells this turn, this "
        "creature gets +1/+0."
    )
    three = _parse_static_ability_line(
        "As long as you've cast three or more spells this turn, this "
        "creature gets +1/+0."
    )
    assert len(two) == 1
    assert len(three) == 1
    assert two[0].condition == AtLeastNSpellsCastThisTurn(2)
    assert three[0].condition == AtLeastNSpellsCastThisTurn(3)
    assert two != three


def test_three_or_more_spells_does_not_set_other_gates():
    # A condition must set its own gate field and leave every other
    # gate at its default — the same per-condition-isolation contract
    # batch 10 pinned for ``it's your turn``.
    parsed = _parse_static_ability_line(
        "As long as you've cast three or more spells this turn, this "
        "creature gets +1/+1."
    )
    assert len(parsed) == 1
    static = parsed[0]
    assert static.condition == AtLeastNSpellsCastThisTurn(3)
    assert static.tier_threshold == 0


# ---------- Phrasebook integrity (static side) ----------

def test_three_or_more_spells_phrase_pinned_in_phrasebook():
    # Pin the new entry so an accidental revert is loud.
    assert "you've cast three or more spells this turn" in (
        _STATIC_CONDITION_KWARGS
    )
    assert _STATIC_CONDITION_KWARGS[
        "you've cast three or more spells this turn"
    ] == {"condition": AtLeastNSpellsCastThisTurn(3)}


def test_existing_static_conditions_still_resolve():
    # Belt-and-suspenders: the three pre-existing static conditions
    # (``you control an artifact``, ``you've cast two or more spells
    # this turn``, ``it's your turn``) must still resolve after the
    # batch-11 addition. Adding a new dict key can't shadow existing
    # ones, but pinning the behavior here makes that contract explicit.
    for phrase in (
        "you control an artifact",
        "you've cast two or more spells this turn",
        "it's your turn",
    ):
        assert phrase in _STATIC_CONDITION_KWARGS, phrase
