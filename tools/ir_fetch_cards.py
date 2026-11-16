"""Fetch a CubeCobra cube's card list as Scryfall card objects.

Default cube is LSV's powered Vintage cube (``PowerLSV``) -- the
project's target card pool. Writes ``data/cube_power_lsv.json``: a JSON
array of Scryfall card objects, deduped by card name, plus the five
basic lands (a cube list never contains basics, but every deck needs
them). A sidecar ``data/cube_power_lsv.meta.json`` records the cube's
``dateLastUpdated`` / card count so staleness is visible.

Data flow:

1. ``GET https://cubecobra.com/cube/api/cubeJSON/<cube-id>`` -- the
   cube's mainboard with each card's ``details.scryfall_id`` (the
   exact printing the cube owner chose).
2. ``POST https://api.scryfall.com/cards/collection`` in batches of
   75 identifiers -- Scryfall returns full card objects (oracle
   text is always the current Oracle wording regardless of printing).

**Name-list mode** (``--names PATH``): when the authoritative card list
lives somewhere other than CubeCobra (the Vintage Roto draft's Google
Sheet, vendored as ``data/cube_roto.sheet.json``), the pool is built
from that list instead. Each entry is ``{"name": <list spelling>,
"scryfall_name": <Scryfall oracle name or null>}``; cards resolve by
front-face name through ``/cards/collection`` and every returned card
must carry exactly the expected oracle name (one ``/cards/named?exact=``
retry per mismatch, then a loud failure). Entries whose
``scryfall_name`` is null are custom cards with no Scryfall record:
they are listed in the sidecar (with their ``custom_text`` when the
list provides it) and never faked into the pool. ``--compare-cube ID``
additionally resolves a CubeCobra cube's ids to oracle names and
records the two-way difference in the sidecar.

mtg-zero (docs/ENGINE.md 170): this is mtg-bot's ``scripts/fetch_cube.py``. Its ``data/`` is the
card-IR pipeline's ``ir/data`` (the relative paths above are relative to ``ir/``; ``--out`` and
``--names`` take any path). It fetches card data (JSON) only, never images. From the repo root::

    python tools/ir_fetch_cards.py
    python tools/ir_fetch_cards.py --cube LSVCube --out ir/data/cube_lsv.json
    python tools/ir_fetch_cards.py \\
        --names ir/data/cube_roto.sheet.json --cube-name "Retireddevil's Vintage Roto 1.1" \\
        --compare-cube 43fd5f2a-942e-40c3-a563-bf3a7ce0cc22 --out ir/data/cube_roto.json
    python tools/ir_fetch_cards.py --names ir/data/legacy_delver.sheet.json --no-basics \\
        --cube-name "Legacy Izzet Delver (mtg-zero)" --out ir/data/legacy_delver.json

Scryfall asks for a User-Agent and a small delay between requests; we obey.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Callable, Iterable, Iterator

DEFAULT_CUBE_ID = "PowerLSV"
# The card-IR pipeline's root (ir/): the dumps live in its data/, and a name list's path is recorded
# relative to it in the sidecar ("data/legacy_delver.sheet.json").
REPO_ROOT = Path(__file__).resolve().parent.parent / "ir"
DEFAULT_OUT_PATH = REPO_ROOT / "data" / "cube_power_lsv.json"
USER_AGENT = "mtg-zero-card-ir/0.1 (card data for an open-source rules engine)"
REQUEST_DELAY_SEC = 0.1
SCRYFALL_COLLECTION_URL = "https://api.scryfall.com/cards/collection"
SCRYFALL_COLLECTION_BATCH = 75  # documented hard limit per request
SCRYFALL_NAMED_URL = "https://api.scryfall.com/cards/named?exact={name}"
CUBECOBRA_JSON_URL = "https://cubecobra.com/cube/api/cubeJSON/{cube_id}"

BASIC_LAND_NAMES: tuple[str, ...] = (
    "Plains", "Island", "Swamp", "Mountain", "Forest",
)
# A name list (the roto draft sheet) never lists basics, but its drafters
# build colorless decks on Wastes, so name-list pools carry it too. Kept out
# of BASIC_LAND_NAMES so the CubeCobra pools (and their claimed sets) don't move.
NAME_LIST_BASIC_LAND_NAMES: tuple[str, ...] = BASIC_LAND_NAMES + ("Wastes",)

# Injectable HTTP seams so tests never touch the network.
GetJson = Callable[[str], dict]
PostJson = Callable[[str, dict], dict]


def _request(url: str, data: bytes | None = None) -> dict:
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    if data is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read().decode("utf-8"))


def get_json(url: str) -> dict:
    return _request(url)


def post_json(url: str, payload: dict) -> dict:
    return _request(url, json.dumps(payload).encode("utf-8"))


def extract_scryfall_ids(cube_json: dict) -> list[str]:
    """Mainboard ``details.scryfall_id`` values, in cube order, deduped.

    Raises ``ValueError`` when the payload has no mainboard or a card
    lacks a Scryfall id -- a half-fetched pool must never ship silently.
    """
    boards = cube_json.get("cards")
    if not isinstance(boards, dict) or "mainboard" not in boards:
        raise ValueError("cube JSON has no cards.mainboard")
    ids: list[str] = []
    seen: set[str] = set()
    for entry in boards["mainboard"]:
        details = entry.get("details") or {}
        sid = details.get("scryfall_id") or entry.get("cardID")
        if not sid:
            raise ValueError(
                f"cube card {entry.get('name') or details.get('name')!r} "
                "has no scryfall_id"
            )
        if sid not in seen:
            seen.add(sid)
            ids.append(sid)
    if not ids:
        raise ValueError("cube mainboard is empty")
    return ids


def chunked(items: Iterable, size: int) -> Iterator[list]:
    batch: list = []
    for item in items:
        batch.append(item)
        if len(batch) == size:
            yield batch
            batch = []
    if batch:
        yield batch


def fetch_collection(
    identifiers: list[dict],
    *,
    post: PostJson | None = None,
    delay_sec: float = REQUEST_DELAY_SEC,
) -> list[dict]:
    """Resolve Scryfall identifiers (``{"id": ...}`` / ``{"name": ...}``)
    to card objects. Raises if Scryfall reports any identifier as
    not found -- silent drops would shrink the pool unnoticed."""
    # Resolved at call time (not as a def-time default) so tests can
    # monkeypatch the module-level ``post_json``.
    post = post or post_json
    cards: list[dict] = []
    not_found: list[dict] = []
    for batch in chunked(identifiers, SCRYFALL_COLLECTION_BATCH):
        page = post(SCRYFALL_COLLECTION_URL, {"identifiers": batch})
        cards.extend(page.get("data", []))
        not_found.extend(page.get("not_found", []))
        if delay_sec:
            time.sleep(delay_sec)
    if not_found:
        raise ValueError(f"Scryfall could not resolve {len(not_found)} "
                         f"identifiers: {not_found[:5]}")
    return cards


def dedupe_by_name(cards: Iterable[dict]) -> list[dict]:
    seen: set[str] = set()
    out: list[dict] = []
    for c in cards:
        name = c.get("name", "")
        if name in seen:
            continue
        seen.add(name)
        out.append(c)
    return out


def build_pool(
    cube_json: dict,
    *,
    post: PostJson | None = None,
    delay_sec: float = REQUEST_DELAY_SEC,
    basics: tuple[str, ...] = BASIC_LAND_NAMES,
) -> list[dict]:
    """Cube mainboard as Scryfall objects + the basic lands, deduped."""
    ids = extract_scryfall_ids(cube_json)
    identifiers = [{"id": sid} for sid in ids]
    identifiers += [{"name": n} for n in basics]
    cards = fetch_collection(identifiers, post=post, delay_sec=delay_sec)
    return dedupe_by_name(cards)


def cube_metadata(cube_json: dict, n_cards: int) -> dict:
    updated_ms = cube_json.get("dateLastUpdated")
    updated_iso = (
        _dt.datetime.fromtimestamp(updated_ms / 1000, _dt.timezone.utc)
        .isoformat()
        if isinstance(updated_ms, (int, float)) else None
    )
    return {
        "cube_id": cube_json.get("shortId"),
        "cube_name": cube_json.get("name"),
        "cube_card_count": cube_json.get("cardCount"),
        "cube_last_updated": updated_iso,
        "fetched_at": _dt.datetime.now(_dt.timezone.utc).isoformat(),
        "pool_card_count": n_cards,
        "includes_basic_lands": list(BASIC_LAND_NAMES),
    }


# ---------------------------------------------------------------- name list


def front_face(name: str) -> str:
    """``"A // B"`` -> ``"A"`` (what ``/cards/collection`` resolves)."""
    return name.split(" // ")[0]


def load_name_list(entries: list[dict]) -> tuple[list[str], list[dict]]:
    """Split a name-list payload into (Scryfall oracle names in list
    order, deduped; custom cards with no Scryfall record).

    Raises ``ValueError`` on an empty list or an entry without a
    ``name`` -- a malformed authority list must not shrink the pool."""
    if not entries:
        raise ValueError("name list is empty")
    names: list[str] = []
    seen: set[str] = set()
    custom: list[dict] = []
    for entry in entries:
        list_name = entry.get("name")
        if not list_name:
            raise ValueError(f"name-list entry has no name: {entry!r}")
        scryfall_name = entry.get("scryfall_name")
        if not scryfall_name:
            custom.append({
                "name": list_name,
                "custom_text": entry.get("custom_text"),
            })
            continue
        if scryfall_name not in seen:
            seen.add(scryfall_name)
            names.append(scryfall_name)
    return names, custom


def fetch_named_exact(name: str, *, get: GetJson | None = None) -> dict:
    get = get or get_json
    return get(SCRYFALL_NAMED_URL.format(name=urllib.parse.quote(name)))


def resolve_names(
    names: list[str],
    *,
    post: PostJson | None = None,
    get: GetJson | None = None,
    delay_sec: float = REQUEST_DELAY_SEC,
) -> list[dict]:
    """Scryfall card objects for ``names`` (full oracle names), in order.

    Looks each card up by front-face name through ``/cards/collection``
    (which does not accept ``A // B`` names). A returned card whose
    oracle name differs from the requested one -- e.g. a newer card
    whose *second face* shares the name -- gets one ``/cards/named?exact=``
    retry; anything still unresolved raises."""
    post = post or post_json
    get = get or get_json
    by_name: dict[str, dict] = {}
    for batch in chunked(names, SCRYFALL_COLLECTION_BATCH):
        page = post(SCRYFALL_COLLECTION_URL, {
            "identifiers": [{"name": front_face(n)} for n in batch],
        })
        for card in page.get("data", []):
            by_name.setdefault(card.get("name", ""), card)
        if delay_sec:
            time.sleep(delay_sec)
    unresolved: list[str] = []
    for name in names:
        if name in by_name:
            continue
        try:
            card = fetch_named_exact(name, get=get)
        except Exception:  # noqa: BLE001 - reported below, loudly
            card = {}
        if delay_sec:
            time.sleep(delay_sec)
        if card.get("name") == name:
            by_name[name] = card
        else:
            unresolved.append(name)
    if unresolved:
        raise ValueError(
            f"Scryfall could not resolve {len(unresolved)} names to the "
            f"expected oracle card: {unresolved[:5]}"
        )
    return [by_name[n] for n in names]


def build_pool_from_names(
    names: list[str],
    *,
    post: PostJson | None = None,
    get: GetJson | None = None,
    delay_sec: float = REQUEST_DELAY_SEC,
    basics: tuple[str, ...] = NAME_LIST_BASIC_LAND_NAMES,
) -> list[dict]:
    """Name list as Scryfall objects + the basic lands (and Wastes), deduped."""
    extra = [b for b in basics if b not in set(names)]
    cards = resolve_names(
        list(names) + extra, post=post, get=get, delay_sec=delay_sec,
    )
    return dedupe_by_name(cards)


def compare_with_cube(
    list_names: list[str],
    cube_json: dict,
    *,
    post: PostJson | None = None,
    delay_sec: float = REQUEST_DELAY_SEC,
) -> dict:
    """Two-way difference between an authority name list and a CubeCobra
    cube, compared on Scryfall oracle names (CubeCobra stores front-face
    or printed names, so its ids are resolved through Scryfall first)."""
    ids = extract_scryfall_ids(cube_json)
    cards = fetch_collection(
        [{"id": sid} for sid in ids], post=post, delay_sec=delay_sec,
    )
    cube_names = {c["name"] for c in cards}
    printed: dict[str, str] = {}
    for entry in cube_json["cards"]["mainboard"]:
        details = entry.get("details") or {}
        sid = details.get("scryfall_id") or entry.get("cardID")
        if details.get("name"):
            printed[sid] = details["name"]
    renamed = sorted(
        (printed[c["id"]], c["name"]) for c in cards
        if c.get("id") in printed
        and printed[c["id"]] != c["name"]
        and printed[c["id"]] != front_face(c["name"])
    )
    listed = set(list_names)
    updated_ms = cube_json.get("dateLastUpdated")
    return {
        "cube_id": cube_json.get("id") or cube_json.get("shortId"),
        "cube_name": cube_json.get("name"),
        "cube_card_count": cube_json.get("cardCount"),
        "cube_last_updated": (
            _dt.datetime.fromtimestamp(updated_ms / 1000, _dt.timezone.utc)
            .isoformat()
            if isinstance(updated_ms, (int, float)) else None
        ),
        "only_in_list": sorted(listed - cube_names),
        "only_in_cube": sorted(cube_names - listed),
        "cube_printed_name_differs": [list(p) for p in renamed],
    }


def name_list_metadata(
    *,
    names_path: Path,
    cube_name: str | None,
    n_list_entries: int,
    scryfall_names: list[str],
    custom: list[dict],
    n_cards: int,
    comparison: dict | None,
) -> dict:
    try:
        source = names_path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        source = names_path.as_posix()
    return {
        "source": "name_list",
        "name_list_path": source,
        "cube_name": cube_name,
        "list_entry_count": n_list_entries,
        "scryfall_card_count": len(scryfall_names),
        "custom_cards_without_scryfall": custom,
        "fetched_at": _dt.datetime.now(_dt.timezone.utc).isoformat(),
        "pool_card_count": n_cards,
        "includes_basic_lands": list(NAME_LIST_BASIC_LAND_NAMES),
        "cubecobra_comparison": comparison,
    }


def _main_name_list(args) -> int:
    entries = json.loads(args.names.read_text(encoding="utf-8"))
    names, custom = load_name_list(entries)
    print(f"Resolving {len(names)} names from {args.names} on Scryfall "
          f"({len(custom)} custom cards without a Scryfall record)...")
    basics = () if args.no_basics else NAME_LIST_BASIC_LAND_NAMES
    pool = build_pool_from_names(names, basics=basics)
    comparison = None
    if args.compare_cube:
        print(f"Comparing against CubeCobra cube {args.compare_cube!r}...")
        cube_json = get_json(CUBECOBRA_JSON_URL.format(cube_id=args.compare_cube))
        comparison = compare_with_cube(names, cube_json)
        print(f"  only in list: {comparison['only_in_list']}")
        print(f"  only in CubeCobra: {comparison['only_in_cube']}")
    for card in custom:
        print(f"  custom (no Scryfall record): {card['name']}")
    meta = name_list_metadata(
        names_path=args.names, cube_name=args.cube_name,
        n_list_entries=len(entries), scryfall_names=names, custom=custom,
        n_cards=len(pool), comparison=comparison,
    )
    meta["includes_basic_lands"] = list(basics)
    _write_pool(args.out, pool, meta)
    return 0


def _write_pool(out: Path, pool: list[dict], meta: dict) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(pool, indent=2), encoding="utf-8")
    meta_path = out.with_suffix(".meta.json")
    meta_path.write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(f"Wrote {len(pool)} cards to {out} (meta: {meta_path.name})")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--cube", default=DEFAULT_CUBE_ID,
                        help="CubeCobra cube id (default PowerLSV)")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT_PATH,
                        help="output JSON path")
    parser.add_argument("--names", type=Path, default=None,
                        help="authoritative name-list JSON; builds the pool "
                             "from it instead of from --cube")
    parser.add_argument("--cube-name", default=None,
                        help="display name recorded in the --names sidecar")
    parser.add_argument("--no-basics", action="store_true",
                        help="with --names: no basic lands added (a pool of "
                             "extra cards, e.g. data/legacy_delver.json)")
    parser.add_argument("--compare-cube", default=None,
                        help="with --names: CubeCobra cube id to diff the "
                             "list against (recorded in the sidecar)")
    args = parser.parse_args(argv)

    if args.names is not None:
        return _main_name_list(args)

    print(f"Fetching cube {args.cube!r} from CubeCobra...")
    cube_json = get_json(CUBECOBRA_JSON_URL.format(cube_id=args.cube))
    pool = build_pool(cube_json)
    if not pool:
        print("No cards resolved.", file=sys.stderr)
        return 1

    _write_pool(args.out, pool, cube_metadata(cube_json, len(pool)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
