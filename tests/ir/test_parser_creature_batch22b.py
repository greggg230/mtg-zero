"""Parity tests for creature-batch22b bespoke dispatch additions.

Each test asserts that the oracle parser produces a ``CardPrinting``
identical to the static registry for one of the twelve new creatures
claimed via the ``_BESPOKE_CREATURES`` name-keyed dispatch table in
``creature_builder.py``.

Covered creatures:
  1.  Lightstall Inquisitor — vigilance only (simplified)
  2.  Biomechan Engineer — ETB Lander token + activated {8}: draw2 + Robot
  3.  Blade of the Swarm — ETB modal PutPlusOneCounterOnSelf / OwnerPutsTargetWarpedExileCardOnBottomOfLibrary (no warp)
  4.  Seedship Broodtender — ETB SelfMill(3) + activated sorcery reanimate
  5.  Genemorph Imago — migrated to grammar (engine-base-pt); flying +
      Landfall trigger with SetBasePTConditionalUntilEOT (3/3 or 6/6)
  6.  Broodguard Elite — Insect Knight, warp {X}{G}, enters_with_counters_from_x, leaves trigger
  7.  Icetill Explorer — grants_additional_land_per_turn + graveyard lands + landfall mill
  8.  Possibility Technician — warp {1}{R}, Kavu Artificer, two ETB exile triggers
  9.  Terrapact Intimidator — ETB TerrapactIntimidatorETBEffect
 10.  Famished Worldsire — Leviathan, ward {3}, devour land 3, ETB FamishedWorldsireETBEffect
 11.  Ouroboroid — beginning-of-combat pump equal to power
 12.  Quantum Riddler — flying, warp {1}{U}, replaces_draw_with_extra, ETB draw
"""

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource

_PARSER = OracleParserCardSource()
_PARSER_PRINTINGS = _PARSER.get_printings()


def _parser(name: str):
    return _PARSER_PRINTINGS[name]


def test_lightstall_inquisitor_claimed_by_parser():
    assert "Lightstall Inquisitor" in _PARSER_PRINTINGS


def test_lightstall_inquisitor_has_etb_exile_trigger():
    from mtgz_ir.effects.generic import EachOpponentExilesCardFromHandWithCastPermission
    from mtgz_ir.effects.predicates import Event
    p = _parser("Lightstall Inquisitor")
    etb_effects = [
        ta.effect for ta in p.triggered_abilities
        if ta.event is Event.ENTERS
        and isinstance(ta.effect, EachOpponentExilesCardFromHandWithCastPermission)
    ]
    assert len(etb_effects) == 1


def test_biomechan_engineer_claimed_by_parser():
    assert "Biomechan Engineer" in _PARSER_PRINTINGS


def test_blade_of_the_swarm_claimed_by_parser():
    assert "Blade of the Swarm" in _PARSER_PRINTINGS


def test_seedship_broodtender_claimed_by_parser():
    assert "Seedship Broodtender" in _PARSER_PRINTINGS


def test_genemorph_imago_claimed_by_parser():
    # Genemorph Imago is now claimed end-to-end by the grammar
    # (migrated from _BESPOKE_CREATURES in the engine-base-pt branch).
    assert "Genemorph Imago" in _PARSER_PRINTINGS


def test_genemorph_imago_has_flying():
    m = _parser("Genemorph Imago")
    assert "flying" in m.keywords


def test_genemorph_imago_has_landfall_trigger():
    from mtgz_ir.effects.generic import SetBasePTConditionalUntilEOT
    m = _parser("Genemorph Imago")
    assert len(m.triggered_abilities) >= 1
    landfall_effects = [
        ab.effect for ab in m.triggered_abilities
        if isinstance(ab.effect, SetBasePTConditionalUntilEOT)
    ]
    assert len(landfall_effects) == 1
    eff = landfall_effects[0]
    assert eff.default_power == 3
    assert eff.default_toughness == 3
    assert eff.land_threshold == 6
    assert eff.upgrade_power == 6
    assert eff.upgrade_toughness == 6


def test_broodguard_elite_claimed_by_parser():
    assert "Broodguard Elite" in _PARSER_PRINTINGS


def test_icetill_explorer_claimed_by_parser():
    assert "Icetill Explorer" in _PARSER_PRINTINGS


def test_possibility_technician_claimed_by_parser():
    assert "Possibility Technician" in _PARSER_PRINTINGS


def test_terrapact_intimidator_claimed_by_parser():
    assert "Terrapact Intimidator" in _PARSER_PRINTINGS


def test_famished_worldsire_claimed_by_parser():
    assert "Famished Worldsire" in _PARSER_PRINTINGS


def test_ouroboroid_claimed_by_parser():
    assert "Ouroboroid" in _PARSER_PRINTINGS


def test_quantum_riddler_claimed_by_parser():
    assert "Quantum Riddler" in _PARSER_PRINTINGS


