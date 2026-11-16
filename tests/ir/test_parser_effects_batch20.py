"""Tests for batch-20 effect-grammar and whole-spell additions.

One effect-grammar primitive lands in this batch:

* ``double the power of target creature you control until end of
  turn`` → ``DoubleTargetCreaturesPowerUEOT()``. Mightform
  Harmonizer's landfall trigger body. Distinct from the existing
  ``gets +N/+0`` UEOT pump shapes — the bonus is computed at
  resolve time from the target's current effective power rather
  than being a fixed constant. The engine class adds the
  measured-at-resolve power as a temp_power_bonus that clears at
  end-of-turn cleanup.

One creature newly claims through this primitive:

* **Mightform Harmonizer** — ``Landfall — Whenever a land you
  control enters, double the power of target creature you control
  until end of turn.`` The ability-word strip and landfall trigger
  predicate already existed; the only blocker was the
  unrecognized inner-effect grammar.

One whole-spell exact-text dispatch entry also lands in this batch
(see ``_WHOLE_SPELL_EXACT_TEXT`` in ``parser.spell_builder``):

* **Scrounge for Eternity** ``{2}{B}`` Sorcery — additional-cost
  sacrifice + ``Return target creature or Spacecraft card with mana
  value 5 or less from your graveyard to the battlefield. Then
  create a Lander token.`` The static drops the target-type
  widening, the mana-value cap, and the Lander rider; ships
  ``ReturnCreatureFromYourGraveyardToBattlefield()`` plus the
  ``additional_cost_sacrifice_creature_or_artifact=True`` printing
  flag (the latter wired separately via
  ``_detect_additional_cost_flags``). Per the parser-is-right rule
  (codified in batch 19), the parser claims a deliberately-
  simplified spell only when the printed effect's intent survives
  the simplification — Scrounge's base mechanic ("a graveyard
  return") survives, so the whole-spell tier matches; Pull Through
  the Weft stays unclaimed because the static replaces its composite
  with an effect of a different intent. Terminal Velocity was previously
  in this category but has since been claimed via ``TerminalVelocityEffect()``
  (bespoke-spells5) — the new engine class faithfully models all three
  printed sub-effects.

The shared parametrized parity test in ``tests/test_parser_parity.py``
enforces full ``CardPrinting`` parity, so this file focuses on the
new grammar / dispatch entries in isolation plus the claim
assertions.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CreateToken,
    DoubleTargetCreaturesPowerUEOT,
    ReturnCreatureFromYourGraveyardToBattlefield,
    Sequence,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.effects import _parse_effect_phrase
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.effects.object_predicates import (
    IsLand,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    Subject,
    YOU,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- effect-phrase grammar ----------

def test_parse_double_target_creature_you_control_power_ueot():
    # The new effect-phrase primitive. Maps to the engine class with
    # no parameters — the legal-target predicate (``target creature
    # you control``) and duration (``until end of turn``) are encoded
    # by the class.
    phrase = (
        "double the power of target creature you control until end of "
        "turn"
    )
    assert _parse_effect_phrase(phrase) == DoubleTargetCreaturesPowerUEOT()


# ---------- trigger-line end-to-end ----------

def test_mightform_harmonizer_landfall_trigger_line():
    # End-to-end with the landfall ability-word prefix. Mirrors the
    # static-registry layout exactly — the ability-word is stripped,
    # the condition phrase ``a land you control enters`` resolves
    # via the existing landfall predicate, and the inner effect goes
    # through the new grammar.
    line = (
        "Landfall — Whenever a land you control enters, double "
        "the power of target creature you control until end of turn."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.ENTERS,
        predicate=Subject(None, IsLand()) & Controlled(YOU),
        effect=DoubleTargetCreaturesPowerUEOT(),
    )


# ---------- claim test (end-to-end via the parser source) ----------

def test_mightform_harmonizer_claimed_by_parser():
    assert "Mightform Harmonizer" in _claimed_names()


# ---------- per-clause grammar migration (Scrounge for Eternity) ----------
# Scrounge for Eternity was migrated from the whole-spell exact-text
# table to the per-clause grammar in bespoke-spells3. The new
# ``_RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV``
# recognizer in ``effects.py`` claims the reanimation clause (returning
# ``ReturnCreatureFromYourGraveyardToBattlefield(or_spacecraft=True,
# max_mv=5)``); the ``Then`` prefix stripping in
# ``_normalize_spell_effect_phrase`` lets "Then create a Lander token."
# parse via ``_CREATE_BARE_NAME_TOKEN``; the Sequence combinator assembles
# both. This is MORE faithful than the previous simplification (which
# returned bare ``ReturnCreatureFromYourGraveyardToBattlefield()``
# dropping the ``or Spacecraft``, ``max_mv=5``, and Lander rider).

def test_scrounge_for_eternity_claimed_by_parser():
    # Scrounge for Eternity ``{2}{B}`` Sorcery — now claimed via the
    # per-clause grammar.
    assert "Scrounge for Eternity" in _claimed_names()


def test_scrounge_for_eternity_parses_via_per_clause_grammar():
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    from mtgz_ir.cards.base import ReturnCreatureFromYourGraveyardToBattlefield
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    eff = _parse_spell_oracle_text(
        "As an additional cost to cast this spell, sacrifice an "
        "artifact or creature.\n"
        "Return target creature or Spacecraft card with mana value 5 "
        "or less from your graveyard to the battlefield. "
        "Then create a Lander token.",
        "Scrounge for Eternity",
    )
    assert isinstance(eff, Sequence)
    assert len(eff.effects) == 2
    assert eff.effects[0] == ReturnCreatureFromYourGraveyardToBattlefield(
        or_spacecraft=True, max_mv=5
    )
    assert eff.effects[1] == CreateToken(
        token_printing=TOKEN_PRINTINGS["Lander"], count=1,
    )


def test_scrounge_for_eternity_carries_additional_cost_flag():
    # The additional-cost flag is wired separately via
    # ``_detect_additional_cost_flags`` — the per-clause grammar
    # returns only the spell-effect. End-to-end the produced
    # ``CardPrinting`` carries
    # ``additional_cost_sacrifice_creature_or_artifact=True`` so the
    # engine prompts the caster for the cost at cast time.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Scrounge for Eternity"]
    assert p.additional_cost_sacrifice_creature_or_artifact is True
