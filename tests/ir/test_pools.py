"""``mtgz_ir.cards.pools`` — pool registry + env selection."""

# mtg-zero (docs/ENGINE.md 170): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, not carried into mtgz_ir):
# test_roto_helpers_build_through_the_claim_path

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mtgz_ir.cards import pools
from mtgz_ir.cards.parser_source import OracleParserCardSource


def _write(path: Path, cards: list[dict]) -> None:
    path.write_text(json.dumps(cards), encoding="utf-8")


@pytest.fixture
def fake_data_dir(tmp_path: Path) -> Path:
    for fname in pools.EOE_POOL_FILES:
        _write(tmp_path / fname, [{"name": f"EoE {fname}"}])
    _write(tmp_path / pools.EOE_POOL_FILES[0], [
        {"name": "Shared"}, {"name": "Only EoE"},
    ])
    _write(tmp_path / pools.CUBE_POOL_FILES[0], [
        {"name": "Shared", "set": "cube"}, {"name": "Only Cube"},
        {"name": "Cube And Roto", "set": "cube"},
    ])
    for fname in pools.ROTO_POOL_FILES:
        _write(tmp_path / fname, [])
    _write(tmp_path / pools.ROTO_POOL_FILES[0], [
        {"name": "Shared", "set": "roto"},
        {"name": "Cube And Roto", "set": "roto"},
        {"name": "Only Roto"},
    ])
    return tmp_path


# ------------------------------------------------------------------ names


def test_resolve_pool_name_defaults_and_normalizes():
    assert pools.resolve_pool_name(None) == pools.DEFAULT_POOL
    assert pools.resolve_pool_name("") == pools.DEFAULT_POOL
    assert pools.resolve_pool_name(" CUBE ") == "cube"
    assert pools.resolve_pool_name("eoe") == "eoe"


def test_resolve_pool_name_rejects_unknown():
    with pytest.raises(ValueError, match="unknown MTG_BOT_CARD_POOL"):
        pools.resolve_pool_name("vintage")


def test_active_pool_name_reads_env(monkeypatch):
    monkeypatch.delenv(pools.POOL_ENV_VAR, raising=False)
    assert pools.active_pool_name() == pools.DEFAULT_POOL
    monkeypatch.setenv(pools.POOL_ENV_VAR, "cube")
    assert pools.active_pool_name() == "cube"


def test_registry_shape():
    assert pools.TARGET_POOL == "cube"
    assert pools.DEFAULT_POOL == "all"
    assert set(pools.POOLS) == {"all", "cube", "eoe", "roto", "legacy"}
    assert pools.POOLS["all"] == (
        pools.EOE_POOL_FILES + pools.CUBE_POOL_FILES + pools.ROTO_POOL_FILES
    )
    assert pools.POOLS["cube"] == pools.CUBE_POOL_FILES
    assert pools.POOLS["roto"] == pools.ROTO_POOL_FILES
    # The Delver mirror's maindeck cards outside the cube (mtg-zero's card
    # IR); in no other pool.
    assert pools.POOLS["legacy"] == pools.LEGACY_POOL_FILES == ("legacy_delver.json",)
    assert all("legacy_delver.json" not in files for name, files in pools.POOLS.items() if name != "legacy")


def test_roto_pool_is_part_of_the_default():
    """Since 2026-10-03 (user decision) the Vintage Roto pool is folded
    into ``all``, after the EoE-era dumps and the cube so every card it
    shares with them keeps its earlier record. It stays selectable on
    its own and is not part of the cube target pool."""
    assert pools.resolve_pool_name(" Roto ") == "roto"
    for fname in pools.ROTO_POOL_FILES:
        assert fname in pools.POOLS["all"]
        assert fname not in pools.POOLS["cube"]
        assert fname not in pools.POOLS["eoe"]


# ------------------------------------------------------------------ paths


def test_pool_data_paths_explicit_and_env(monkeypatch, tmp_path):
    paths = pools.pool_data_paths("cube", data_dir=tmp_path)
    assert paths == (tmp_path / "cube_power_lsv.json",)
    monkeypatch.setenv(pools.POOL_ENV_VAR, "eoe")
    env_paths = pools.pool_data_paths(data_dir=tmp_path)
    assert [p.name for p in env_paths] == list(pools.EOE_POOL_FILES)


def test_real_pool_files_exist():
    """Every registered dump is committed — a fresh checkout must be
    able to build the default source without a fetch step."""
    for name in pools.POOLS:
        for path in pools.pool_data_paths(name):
            assert path.exists(), path


# ------------------------------------------------------------------ load


def test_load_pool_cards_dedupes_first_file_wins(fake_data_dir):
    cards = pools.load_pool_cards("all", data_dir=fake_data_dir)
    names = [c["name"] for c in cards]
    assert names.count("Shared") == 1
    shared = next(c for c in cards if c["name"] == "Shared")
    assert "set" not in shared  # the EoE-file record came first
    # Roto comes after the cube: a card on both lists keeps the cube's.
    both = next(c for c in cards if c["name"] == "Cube And Roto")
    assert both["set"] == "cube"
    assert {"Only Cube", "Only EoE", "Only Roto"} <= set(names)


def test_load_pool_cards_cube_only(fake_data_dir):
    names = pools.pool_card_names("cube", data_dir=fake_data_dir)
    assert names == frozenset({"Shared", "Only Cube", "Cube And Roto"})


def test_load_pool_cards_missing_file_is_loud(tmp_path):
    with pytest.raises(FileNotFoundError, match="fetch_cube.py"):
        pools.load_pool_cards("cube", data_dir=tmp_path)
    with pytest.raises(FileNotFoundError, match="--names data/cube_roto.sheet.json"):
        pools.load_pool_cards("roto", data_dir=tmp_path)
    (tmp_path / "cube_roto.json").write_text("[]", encoding="utf-8")
    with pytest.raises(FileNotFoundError, match="hand-authored; restore it from git"):
        pools.load_pool_cards("roto", data_dir=tmp_path)
    (tmp_path / pools.CUBE_POOL_FILES[0]).write_text("[]", encoding="utf-8")
    with pytest.raises(FileNotFoundError, match=r"fetch_\*\.py"):
        pools.load_pool_cards("all", data_dir=tmp_path)


# ------------------------------------------------------------------ source


def test_parser_source_honors_explicit_pool():
    cube = OracleParserCardSource(pool="cube")
    assert cube.pool_name == "cube"
    claimed = set(cube.claimed_names)
    # Basics ride along with the cube list; power is claimed.
    assert {"Plains", "Island", "Swamp", "Mountain", "Forest"} <= claimed
    assert "Mox Sapphire" in claimed
    # An EoE-only card is not in a cube-only source.
    assert "Anticausal Vestige" not in claimed


def test_parser_source_default_pool_is_union(monkeypatch):
    monkeypatch.delenv(pools.POOL_ENV_VAR, raising=False)
    union = OracleParserCardSource()
    assert union.pool_name == "all"
    claimed = set(union.claimed_names)
    assert "Mox Sapphire" in claimed
    assert "Lightning Bolt" in claimed
    assert "Anticausal Vestige" in claimed  # EoE-era
    assert "Smokestack" in claimed  # roto-only


def test_parser_source_rejects_unknown_pool():
    with pytest.raises(ValueError):
        OracleParserCardSource(pool="nope")


def test_real_roto_pool_contents():
    """The committed roto pool: the dump's 860 Scryfall-backed cards +
    the five basics and Wastes, plus the one hand-authored custom card (Meddling
    Wizard, ``data/cube_roto_custom.json``), unique by name. The custom
    conspiracies are never faked into the Scryfall dump, and the three
    left alone have no record at all."""
    names = pools.pool_card_names("roto")
    assert len(names) == 867
    assert {"Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"} <= names
    assert {"Black Lotus", "Smokestack", "Spectacular Spider-Man",
            "Life // Death", "Regrowth"} <= names
    assert "Meddling Wizard" in names
    assert "ANOTHER HEAD?!" not in names
    dump = json.loads(
        (pools.DATA_DIR / "cube_roto.json").read_text(encoding="utf-8")
    )
    assert len(dump) == 866
    assert "Meddling Wizard" not in {c["name"] for c in dump}
    assert pools.CUSTOM_CARD_FILES == ("cube_roto_custom.json",)
    assert pools.ROTO_POOL_FILES == ("cube_roto.json", "cube_roto_custom.json")


def test_default_source_is_the_union_of_the_three_pools(monkeypatch):
    """``all`` holds every card of the EoE-era dumps, the cube and the
    roto list, each once; a roto-only card (Smokestack) is in it."""
    monkeypatch.delenv(pools.POOL_ENV_VAR, raising=False)
    default_names = pools.pool_card_names(pools.DEFAULT_POOL)
    assert default_names == (
        pools.pool_card_names("eoe") | pools.pool_card_names("cube")
        | pools.pool_card_names("roto")
    )
    assert "Smokestack" in default_names
    assert "Smokestack" not in pools.pool_card_names("cube")


def test_default_claims_are_the_union_of_the_pools_claims(monkeypatch):
    """Folding roto in moves no other card: the default source claims
    exactly the union of the three pools' claims, and a card the pools
    share is built identically wherever it is claimed."""
    monkeypatch.delenv(pools.POOL_ENV_VAR, raising=False)
    union = OracleParserCardSource().get_printings()
    parts = [OracleParserCardSource(pool=p).get_printings()
             for p in ("eoe", "cube", "roto")]
    assert set(union) == set().union(*parts)
    for part in parts:
        for name, printing in part.items():
            assert union[name] == printing, name


# ------------------------------------------------------- custom cards


def test_pool_custom_cards_reads_sidecars(tmp_path):
    for fname in pools.ROTO_POOL_FILES:
        _write(tmp_path / fname, [])
    (tmp_path / "cube_roto.meta.json").write_text(json.dumps({
        "custom_cards_without_scryfall": [
            {"name": "X", "custom_text": "t"},
            {"name": "Authored", "custom_text": "u"},
        ],
    }), encoding="utf-8")
    assert pools.pool_custom_cards("roto", data_dir=tmp_path) == [
        {"name": "X", "custom_text": "t"},
        {"name": "Authored", "custom_text": "u"},
    ]
    # A hand-authored record takes a custom card off the list.
    _write(tmp_path / pools.CUSTOM_CARD_FILES[0], [
        {"name": "Authored", "custom_card": True},
    ])
    assert pools.pool_custom_cards("roto", data_dir=tmp_path) == [
        {"name": "X", "custom_text": "t"},
    ]
    # A pool whose dumps have no sidecar, or a sidecar without the key.
    _write(tmp_path / pools.CUBE_POOL_FILES[0], [])
    assert pools.pool_custom_cards("cube", data_dir=tmp_path) == []
    (tmp_path / "cube_power_lsv.meta.json").write_text("{}", encoding="utf-8")
    assert pools.pool_custom_cards("cube", data_dir=tmp_path) == []


def test_real_roto_custom_cards():
    """Meddling Wizard has a hand-authored record (2026-10-03, user
    decision); the other three custom conspiracies stay without one."""
    assert [c["name"] for c in pools.pool_custom_cards("roto")] == [
        "Quanitity Over Quality", "I've Made a Huge Mistake",
        "ANOTHER HEAD?!",
    ]
    assert pools.pool_custom_cards("cube") == []


# ------------------------------------------------- single-record builds


def test_build_card_record_is_the_source_claim_path():
    from mtgz_ir.cards.parser.source import build_card_record

    records = {c["name"]: c for c in pools.load_pool_cards("cube")}
    built = build_card_record(records["Mox Sapphire"])
    assert built is not None and built.name == "Mox Sapphire"
    # Hand-activated abilities (cycling family) are re-attached.
    cyclers = [c for c in records.values()
               if "Cycling" in (c.get("oracle_text") or "")
               and "Land" not in c.get("type_line", "")
               and not c.get("card_faces")]
    assert cyclers, "the cube has single-faced nonland cyclers"
    claimed = set(OracleParserCardSource(pool="cube").claimed_names)
    for rec in cyclers:
        if rec["name"] in claimed:
            assert build_card_record(rec).hand_activated_abilities
            break
    else:  # pragma: no cover - every cube card is claimed
        raise AssertionError("no claimed cycler")
    assert build_card_record({"name": "Nope", "type_line": "Instant",
                              "oracle_text": "Zorble the frobnitz."}) is None


def test_unrepresentable_mana_cost_declines_instead_of_crashing():
    """A mana symbol the engine doesn't model leaves the card unclaimed
    (and in the coverage report) rather than crashing pool construction
    or claiming with a bogus cost. (Snow ``{S}``, the original example,
    is modeled since the Vintage Roto P3 snow support.)"""
    from mtgz_ir.cards.parser.source import (
        _printed_mana_costs_supported,
        build_card_record,
    )

    astrolabe = next(c for c in pools.load_pool_cards("roto")
                     if c["name"] == "Arcum's Astrolabe")
    unmodeled = dict(astrolabe, mana_cost="{Q}")
    assert not _printed_mana_costs_supported(unmodeled)
    assert build_card_record(unmodeled) is None
    assert _printed_mana_costs_supported(astrolabe)
    assert not _printed_mana_costs_supported({
        "mana_cost": "", "card_faces": [{"mana_cost": "{1}"}, {"mana_cost": "{Q}"}],
    })
    assert _printed_mana_costs_supported({
        "mana_cost": "{1}{G} // {2}{U}",
        "card_faces": [{"mana_cost": "{1}{G}"}, {}],
    })
    roto = OracleParserCardSource(pool="roto")
    assert roto.pool_name == "roto"
    assert "Arcum's Astrolabe" in roto.claimed_names
    assert "Black Lotus" in roto.claimed_names


