"""Card-pool registry: which Scryfall JSON dumps feed the parser.

The project's **target** pool is the PowerLSV cube
(``data/cube_power_lsv.json``, regenerate via ``scripts/fetch_cube.py``).
The Edge of Eternities pools (``eoe`` + Stellar Sights + EoE Special
Guests + Duskmourn) remain loadable because the engine's test fixtures
are written against those cards.

Pool names (``MTG_BOT_CARD_POOL`` env var):

- ``all`` (default): EoE pools + cube + Vintage Roto, deduped by name
  (first file wins: an EoE-era record, then the cube's, then roto's).
  Production and tests see the same source; nothing is dropped. Roto
  joined the default on 2026-10-03 (user decision); every card it
  shares with the EoE-era dumps or the cube keeps the record (and so
  the printing) it had before.
- ``cube``: the PowerLSV cube + basic lands only. Use this for
  coverage measurement, cube deck-pool generation, and cube self-play.
- ``eoe``: the legacy four EoE-era files only (the Rust parity
  campaign's pool, ``parity.effect_coverage.PARITY_POOL``).
- ``roto``: Retireddevil's Vintage Roto 1.1 (``data/cube_roto.json``,
  860 Scryfall cards from the draft sheet's list + basic lands;
  regenerate via ``scripts/fetch_cube.py --names
  data/cube_roto.sheet.json``) plus ``data/cube_roto_custom.json``, the
  hand-authored records of the sheet's custom cards the engine models
  (Meddling Wizard; see :data:`CUSTOM_CARD_FILES`). Part of ``all``;
  selectable alone for coverage measurement
  (``docs/ROTO_POOL_COVERAGE.md``).

Custom cards: a pool's authority list can name cards with no Scryfall
record (the roto sheet's four custom conspiracies). ``fetch_cube.py``
lists them in the dump's ``.meta.json`` sidecar and never fakes them into
the dump. A custom card the engine supports gets a hand-authored,
Scryfall-shaped record in a ``*_custom.json`` file of the pool
(``"custom_card": true``, its ``custom_source`` naming the sheet row and
the sheet's text); :func:`pool_custom_cards` lists only the ones still
without any record.

Unknown values raise so a typo never silently becomes a different pool.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

POOL_ENV_VAR = "MTG_BOT_CARD_POOL"
# mtg-zero: the pools' Scryfall dumps live in ir/data (this file is ir/mtgz_ir/cards/pools.py).
DATA_DIR = Path(__file__).resolve().parents[2] / "data"

CUBE_POOL_FILES: tuple[str, ...] = ("cube_power_lsv.json",)
# Hand-authored records for custom cards (no Scryfall record); never
# written by a fetch script.
CUSTOM_CARD_FILES: tuple[str, ...] = ("cube_roto_custom.json",)
ROTO_POOL_FILES: tuple[str, ...] = ("cube_roto.json",) + CUSTOM_CARD_FILES
# The cards of the Legacy Izzet Delver 75's maindeck (mtg-zero's Delver
# mirror) that are not in the cube: Murktide Regent, Prismari Charm; Scryfall
# records, no basic lands (regenerate via ``scripts/fetch_cube.py --names
# data/legacy_delver.sheet.json --no-basics --cube-name "Legacy Izzet Delver
# (mtg-zero)" --out data/legacy_delver.json``). In no other pool.
# Also the cards of mtg-zero's Legacy Doomsday deck outside the cube, lane A:
# Doomsday, Pact of Negation, Personal Tutor, Flow State.
# Also the Delver 75's five sideboard cards outside the cube (mtg-zero
# ENGINE.md 122): Consign to Memory, Hydroblast, Meltdown, Price of Progress,
# Rough // Tumble.
LEGACY_POOL_FILES: tuple[str, ...] = ("legacy_delver.json",)
EOE_POOL_FILES: tuple[str, ...] = (
    "edge_of_eternities.json",
    "stellar_sights.json",
    "special_guests_eoe.json",
    "duskmourn.json",
)
POOLS: dict[str, tuple[str, ...]] = {
    "all": EOE_POOL_FILES + CUBE_POOL_FILES + ROTO_POOL_FILES,
    "cube": CUBE_POOL_FILES,
    "eoe": EOE_POOL_FILES,
    "roto": ROTO_POOL_FILES,
    "legacy": LEGACY_POOL_FILES,
}
DEFAULT_POOL = "all"
# The pool the modeling campaign, deck pool, and self-play target.
TARGET_POOL = "cube"


def resolve_pool_name(value: str | None) -> str:
    """Normalize an env-var / CLI value to a registered pool name."""
    normalized = (value or DEFAULT_POOL).strip().lower()
    if normalized not in POOLS:
        raise ValueError(
            f"unknown {POOL_ENV_VAR} value: {value!r} "
            f"(expected one of {sorted(POOLS)})"
        )
    return normalized


def active_pool_name() -> str:
    return resolve_pool_name(os.environ.get(POOL_ENV_VAR))


def pool_data_paths(pool: str | None = None, *, data_dir: Path = DATA_DIR) -> tuple[Path, ...]:
    """Absolute paths of the JSON dumps that make up ``pool``
    (``None`` -> the env-selected pool)."""
    name = resolve_pool_name(pool) if pool is not None else active_pool_name()
    return tuple(data_dir / f for f in POOLS[name])


def load_pool_cards(pool: str | None = None, *, data_dir: Path = DATA_DIR) -> list[dict]:
    """Every Scryfall card object in ``pool``, deduped by name (first
    file wins). A missing dump raises ``FileNotFoundError`` naming the
    script that regenerates it -- a half-loaded pool must not ship."""
    cards: list[dict] = []
    seen: set[str] = set()
    for path in pool_data_paths(pool, data_dir=data_dir):
        if not path.exists():
            if path.name in CUBE_POOL_FILES:
                hint = "scripts/fetch_cube.py"
            elif path.name in CUSTOM_CARD_FILES:
                hint = "nothing -- it is hand-authored; restore it from git"
            elif path.name in ROTO_POOL_FILES:
                hint = "scripts/fetch_cube.py --names data/cube_roto.sheet.json"
            else:
                hint = "scripts/fetch_*.py"
            raise FileNotFoundError(
                f"card pool file missing: {path} (regenerate via {hint})"
            )
        with path.open("r", encoding="utf-8") as f:
            for card in json.load(f):
                name = card["name"]
                if name in seen:
                    continue
                seen.add(name)
                cards.append(card)
    return cards


def pool_card_names(pool: str, *, data_dir: Path = DATA_DIR) -> frozenset[str]:
    """Names present in ``pool``'s raw dumps (claimed or not)."""
    return frozenset(c["name"] for c in load_pool_cards(pool, data_dir=data_dir))


def pool_custom_cards(pool: str, *, data_dir: Path = DATA_DIR) -> list[dict]:
    """Cards a pool's authority list names that have no Scryfall record
    (recorded by ``scripts/fetch_cube.py --names`` in each dump's
    ``.meta.json`` sidecar as ``custom_cards_without_scryfall``) and no
    hand-authored record in the pool either (:data:`CUSTOM_CARD_FILES`).
    They are in no file of the pool; coverage reports list them apart."""
    custom: list[dict] = []
    for path in pool_data_paths(pool, data_dir=data_dir):
        meta_path = path.with_suffix(".meta.json")
        if not meta_path.exists():
            continue
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
        custom.extend(meta.get("custom_cards_without_scryfall") or ())
    if not custom:
        return custom
    authored = pool_card_names(pool, data_dir=data_dir)
    return [c for c in custom if c["name"] not in authored]
