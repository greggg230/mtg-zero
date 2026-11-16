"""Tests for the Duskmourn spells / enchantments / artifacts /
equipment batch-1 parser additions.

This batch adds grammar primitives and dispatch entries that claim
several Duskmourn cards by composing existing engine classes — no
new engine fields or mechanics. Each new primitive is reusable across
contexts (spells AND creature triggered abilities) and across sets,
so the regex additions in ``parser.effects`` aren't DSK-specific even
though every card claimed by this batch ships in Duskmourn.

New effect-grammar primitives (``parser.effects``):

* ``deals N damage to each creature`` →
  ``DealNDamageToEachCreature(amount=N)``. Mass board-wipe burn
  surface, parallel to the existing ``deals N damage to each
  opponent`` primitive. Already-modeled engine class — exercised
  inside Lithobraking's reflexive optional-sac sequence and
  Extinguisher Battleship's ETB sequence in EoE — was missing only
  this surface form.

* ``target creature gains <kw>[ and <kw2>...] until end of turn`` →
  ``PumpTargetCreatureUntilEOT(granted_keywords=...)``. Pure-keyword-
  grant variant on a bare creature target with no P/T pump and no
  control restriction. Distinct from the existing
  ``another target creature you control gains …`` variant (Blooming
  Stinger) — that one carries the ``you control`` + ``another``
  qualifiers; this one is for phrases without any qualifier (Horrid
  Vigor's printed body).

New whole-spell exact-text dispatch entries
(``_WHOLE_SPELL_EXACT_TEXT`` in ``parser.spell_builder``):

* **Don't Make a Sound** ``{1}{U}`` Instant — ``Counter target spell
  unless its controller pays {2}. If they do, surveil 2.``. The
  static ships the bare ``CounterTargetSpell()`` because the engine
  doesn't yet model the pay-cost-or-counter Mana Leak shape. Mirrors
  Divert Disaster's parity simplification — the base mechanic, a
  counterspell, survives the rider drop.

New name-keyed enchantment dispatch entries
(``_ENCHANTMENT_BUILDERS`` in ``parser.enchantment_builder``):

* **Duskmourn's Domination** ``{4}{U}{U}`` Aura — gain control + lose
  all abilities + ``-3/-0`` continuous bonus on the enchanted
  creature. Composes existing ``make_aura`` flags
  (``attached_grants_control``, ``attached_grants_lose_all_abilities``,
  ``power_bonus=-3``); no new engine fields needed.

Cards newly claimed via the new primitive:

* **Pyroclasm** ``{1}{R}`` Sorcery — single-line ``Pyroclasm deals 2
  damage to each creature.``. Parses through the standard single-
  effect path now that the regex is in place.
* **Don't Make a Sound** ``{1}{U}`` Instant — claimed via the new
  whole-spell exact-text dispatch entry above.
* **Duskmourn's Domination** ``{4}{U}{U}`` Aura — claimed via the new
  name-keyed dispatch entry above.
* **Horrid Vigor** ``{1}{G}`` Instant — single-line ``Target creature
  gains deathtouch and indestructible until end of turn.``. Parses
  through the standard single-effect path now that the bare-target
  keyword-grant primitive is in place.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CounterTargetSpell,
    DealNDamageToEachCreature,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- effect-phrase grammar primitive ----------

def test_parse_deals_n_damage_each_creature_spell_form():
    # Spell form: ``<Name> deals N damage to each creature.``. The
    # spell-effect normalizer strips the source-name prefix, so the
    # surface reaching ``_parse_effect`` is exactly
    # ``deals N damage to each creature``.
    phrase = "Pyroclasm deals 2 damage to each creature."
    assert _parse_spell_effect_phrase(phrase, "Pyroclasm") == (
        DealNDamageToEachCreature(amount=2)
    )


def test_parse_deals_n_damage_each_creature_normalized():
    # Bare normalized form (no source prefix) — what feeds
    # ``_parse_effect`` from both spell and creature-trigger paths.
    assert _parse_effect("deals 2 damage to each creature") == (
        DealNDamageToEachCreature(amount=2)
    )


def test_parse_deals_n_damage_each_creature_this_creature_prefix():
    # Optional ``this creature `` prefix surfaces when a creature's
    # trigger body names the source explicitly (mirrors the parallel
    # ``deals N damage to each opponent`` primitive's optional prefix).
    # No DSK card uses this form today; the primitive accepts it for
    # parity with the each-opponent shape so future surfaces parse
    # without further grammar work.
    assert _parse_effect("this creature deals 1 damage to each creature") == (
        DealNDamageToEachCreature(amount=1)
    )


# ---------- claim test (end-to-end via the parser source) ----------

def test_pyroclasm_claimed_by_parser():
    # Pyroclasm ``{1}{R}`` Sorcery — single-line ``Pyroclasm deals 2
    # damage to each creature.``. End-to-end via the parser source,
    # via the new primitive.
    assert "Pyroclasm" in _claimed_names()


def test_pyroclasm_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the correct engine class (not just any spell effect).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Pyroclasm"]
    assert p.spell_effect == DealNDamageToEachCreature(amount=2)


# ---------- whole-spell exact-text dispatch (Don't Make a Sound) ----------

def test_dont_make_a_sound_claimed_by_parser():
    # Don't Make a Sound ``{1}{U}`` Instant — pay-cost counterspell
    # with a conditional surveil rider. Whole-spell exact-text dispatch
    # returns the bare ``CounterTargetSpell()`` engine class the static
    # side ships (the pay-cost branching and surveil rider are dropped
    # — same simplification as Divert Disaster, since the base mechanic
    # of a counterspell survives the rider drop per the parser-is-right
    # rule).
    assert "Don't Make a Sound" in _claimed_names()


def test_dont_make_a_sound_parses_via_per_clause_grammar():
    # Belt-and-suspenders: per-clause grammar now claims "unless pays"
    # counterspells with "If they do," consequents.
    from mtgz_ir.cards.parser.spell_builder import _parse_spell_oracle_text
    eff = _parse_spell_oracle_text(
        "Counter target spell unless its controller pays {2}. "
        "If they do, surveil 2.",
        "Don't Make a Sound",
    )
    assert eff == CounterTargetSpell()


# ---------- enchantment dispatch (Duskmourn's Domination) ----------

def test_duskmourns_domination_claimed_by_parser():
    # Duskmourn's Domination ``{4}{U}{U}`` Aura — gain control + lose
    # all abilities + -3/-0. Claimed via the name-keyed
    # ``_ENCHANTMENT_BUILDERS`` dispatch in
    # ``parser.enchantment_builder``.
    assert "Duskmourn's Domination" in _claimed_names()


def test_duskmourns_domination_carries_aura_fields():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the three hooks the static side ships:
    # ``attached_grants_control``, ``attached_grants_lose_all_abilities``,
    # and the ``-3/-0`` ``equipment_bonuses`` triple.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Duskmourn's Domination"]
    assert p.attached_grants_control is True
    assert p.attached_grants_lose_all_abilities is True
    assert p.equipment_bonuses == (-3, 0, frozenset())


# ---------- bare-target keyword grant primitive (Horrid Vigor) ----------

def test_parse_target_creature_gains_keywords_ueot_single():
    # Single-keyword grant on a bare creature target. Distinct from
    # the existing ``you control`` / ``opponent controls`` / ``another
    # target creature you control`` variants — this primitive is for
    # phrases without those qualifiers (Horrid Vigor's bare-target
    # shape in DSK).
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT
    eff = _parse_effect("target creature gains flying until end of turn")
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"flying"}),
    )


def test_parse_target_creature_gains_keywords_ueot_multi():
    # Two-keyword grant — ``deathtouch and indestructible`` is exactly
    # Horrid Vigor's printed body. Validates the keyword-line
    # whitelist runs across the comma-joined surface (here joined by
    # ``and``, validated by ``_parse_keyword_grant_body``).
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT
    eff = _parse_effect(
        "target creature gains deathtouch and indestructible "
        "until end of turn"
    )
    assert eff == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"deathtouch", "indestructible"}),
    )


def test_parse_target_creature_gains_unknown_keyword_returns_none():
    # An unrecognized keyword fails the keyword-line whitelist
    # validation in ``_parse_keyword_grant_body`` and returns ``None``
    # — so a card that printed an unknown keyword would stay unclaimed
    # rather than silently dropping the grant.
    assert _parse_effect(
        "target creature gains unicornsplosion until end of turn"
    ) is None


def test_horrid_vigor_claimed_by_parser():
    # Horrid Vigor ``{1}{G}`` Instant — single-line ``Target creature
    # gains deathtouch and indestructible until end of turn.``. End-
    # to-end via the new bare-target keyword-grant primitive.
    assert "Horrid Vigor" in _claimed_names()


def test_horrid_vigor_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the right keyword set and zero bonuses (no control restriction).
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT
    parser = OracleParserCardSource()
    p = parser.get_printings()["Horrid Vigor"]
    assert p.spell_effect == PumpTargetCreatureUntilEOT(
        power_bonus=0, toughness_bonus=0,
        granted_keywords=frozenset({"deathtouch", "indestructible"}),
    )
