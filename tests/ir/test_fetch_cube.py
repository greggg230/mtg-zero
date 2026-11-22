"""``scripts/fetch_cube.py`` — CubeCobra → Scryfall pool fetch, offline."""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest

SCRIPT_PATH = Path(__file__).resolve().parents[2] / "tools" / "ir_fetch_cards.py"


@pytest.fixture(scope="module")
def fc():
    spec = importlib.util.spec_from_file_location("_fetch_cube_under_test", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


def _cube_json(ids: list[str]) -> dict:
    return {
        "shortId": "TestCube", "name": "Test Cube", "cardCount": len(ids),
        "dateLastUpdated": 1_700_000_000_000,
        "cards": {"mainboard": [
            {"name": f"Card {i}", "cardID": sid, "details": {"scryfall_id": sid}}
            for i, sid in enumerate(ids)
        ], "maybeboard": []},
    }


def test_extract_scryfall_ids_in_order_deduped(fc):
    ids = fc.extract_scryfall_ids(_cube_json(["a", "b", "a", "c"]))
    assert ids == ["a", "b", "c"]


def test_extract_scryfall_ids_falls_back_to_cardID(fc):
    payload = {"cards": {"mainboard": [{"name": "X", "cardID": "zz", "details": {}}]}}
    assert fc.extract_scryfall_ids(payload) == ["zz"]


def test_extract_scryfall_ids_rejects_bad_payloads(fc):
    with pytest.raises(ValueError, match="no cards.mainboard"):
        fc.extract_scryfall_ids({"cards": []})
    with pytest.raises(ValueError, match="empty"):
        fc.extract_scryfall_ids({"cards": {"mainboard": []}})
    with pytest.raises(ValueError, match="no scryfall_id"):
        fc.extract_scryfall_ids({"cards": {"mainboard": [{"name": "X", "details": {}}]}})


def test_chunked(fc):
    assert list(fc.chunked(range(5), 2)) == [[0, 1], [2, 3], [4]]
    assert list(fc.chunked([], 3)) == []


def test_fetch_collection_batches_and_collects(fc):
    calls: list[dict] = []

    def post(url, payload):
        calls.append(payload)
        return {"data": [{"name": i["id"]} for i in payload["identifiers"]], "not_found": []}

    idents = [{"id": str(i)} for i in range(fc.SCRYFALL_COLLECTION_BATCH + 1)]
    cards = fc.fetch_collection(idents, post=post, delay_sec=0)
    assert len(cards) == len(idents)
    assert [len(c["identifiers"]) for c in calls] == [fc.SCRYFALL_COLLECTION_BATCH, 1]


def test_fetch_collection_not_found_is_loud(fc):
    def post(url, payload):
        return {"data": [], "not_found": [{"id": "missing"}]}

    with pytest.raises(ValueError, match="could not resolve 1"):
        fc.fetch_collection([{"id": "missing"}], post=post, delay_sec=0)


def test_dedupe_by_name(fc):
    out = fc.dedupe_by_name([{"name": "A", "v": 1}, {"name": "A", "v": 2}, {"name": "B"}])
    assert [(c["name"], c.get("v")) for c in out] == [("A", 1), ("B", None)]


def test_build_pool_appends_basics_and_dedupes(fc):
    def post(url, payload):
        data = []
        for ident in payload["identifiers"]:
            if "id" in ident:
                data.append({"name": f"Card {ident['id']}", "id": ident["id"]})
            else:
                data.append({"name": ident["name"], "id": f"basic-{ident['name']}"})
        return {"data": data, "not_found": []}

    pool = fc.build_pool(_cube_json(["a", "b"]), post=post, delay_sec=0)
    names = [c["name"] for c in pool]
    assert names == ["Card a", "Card b", *fc.BASIC_LAND_NAMES]


def test_cube_metadata(fc):
    meta = fc.cube_metadata(_cube_json(["a"]), n_cards=6)
    assert meta["cube_id"] == "TestCube"
    assert meta["cube_card_count"] == 1
    assert meta["pool_card_count"] == 6
    assert meta["cube_last_updated"].startswith("2023-11-14")
    assert meta["includes_basic_lands"] == list(fc.BASIC_LAND_NAMES)
    assert fc.cube_metadata({}, n_cards=0)["cube_last_updated"] is None


def test_main_writes_pool_and_meta(fc, tmp_path, monkeypatch, capsys):
    monkeypatch.setattr(fc, "get_json", lambda url: _cube_json(["a"]))
    monkeypatch.setattr(fc, "post_json", lambda url, payload: {
        "data": [{"name": i.get("name") or f"Card {i['id']}", "id": "x"} for i in payload["identifiers"]],
        "not_found": [],
    })
    monkeypatch.setattr(fc, "REQUEST_DELAY_SEC", 0)
    out = tmp_path / "pool.json"
    assert fc.main(["--cube", "TestCube", "--out", str(out)]) == 0
    cards = json.loads(out.read_text(encoding="utf-8"))
    assert len(cards) == 1 + len(fc.BASIC_LAND_NAMES)
    meta = json.loads((tmp_path / "pool.meta.json").read_text(encoding="utf-8"))
    assert meta["cube_id"] == "TestCube"
    assert "Wrote" in capsys.readouterr().out


def test_main_empty_pool_fails(fc, tmp_path, monkeypatch):
    monkeypatch.setattr(fc, "get_json", lambda url: _cube_json(["a"]))
    monkeypatch.setattr(fc, "build_pool", lambda cube_json: [])
    assert fc.main(["--out", str(tmp_path / "pool.json")]) == 1


def test_committed_pool_matches_meta(fc):
    """The committed dump is what the script would write: 541 cube cards
    + 5 basics, and the sidecar agrees."""
    pool_path = fc.DEFAULT_OUT_PATH
    meta = json.loads(pool_path.with_suffix(".meta.json").read_text(encoding="utf-8"))
    cards = json.loads(pool_path.read_text(encoding="utf-8"))
    assert meta["pool_card_count"] == len(cards)
    assert meta["cube_id"] == fc.DEFAULT_CUBE_ID
    names = {c["name"] for c in cards}
    assert set(fc.BASIC_LAND_NAMES) <= names
    assert len(names) == len(cards)


# ------------------------------------------------------------ name-list mode


def _collection_by_front_face(catalog: dict[str, dict]):
    """A fake ``/cards/collection`` resolving ``{"name": front}`` /
    ``{"id": ...}`` identifiers against ``catalog``."""
    calls: list[dict] = []

    def post(url, payload):
        calls.append(payload)
        data, nf = [], []
        for ident in payload["identifiers"]:
            key = ident.get("name") or ident.get("id")
            if key in catalog:
                data.append(catalog[key])
            else:
                nf.append(ident)
        return {"data": data, "not_found": nf}

    post.calls = calls
    return post


def test_front_face(fc):
    assert fc.front_face("Life // Death") == "Life"
    assert fc.front_face("Black Lotus") == "Black Lotus"


def test_load_name_list_splits_custom_and_dedupes(fc):
    names, custom = fc.load_name_list([
        {"name": "Delver of Secrets",
         "scryfall_name": "Delver of Secrets // Insectile Aberration"},
        {"name": "Mox Pearl", "scryfall_name": "Mox Pearl"},
        {"name": "Mox Pearl", "scryfall_name": "Mox Pearl"},
        {"name": "Meddling Wizard", "scryfall_name": None, "custom_text": "reveal"},
        {"name": "Odd One", "scryfall_name": None},
    ])
    assert names == ["Delver of Secrets // Insectile Aberration", "Mox Pearl"]
    assert custom == [
        {"name": "Meddling Wizard", "custom_text": "reveal"},
        {"name": "Odd One", "custom_text": None},
    ]


def test_load_name_list_rejects_bad_payloads(fc):
    with pytest.raises(ValueError, match="empty"):
        fc.load_name_list([])
    with pytest.raises(ValueError, match="no name"):
        fc.load_name_list([{"scryfall_name": "X"}])


def test_resolve_names_uses_front_face_and_keeps_order(fc):
    post = _collection_by_front_face({
        "Life": {"name": "Life // Death"},
        "Mox Pearl": {"name": "Mox Pearl"},
    })
    cards = fc.resolve_names(
        ["Mox Pearl", "Life // Death"], post=post,
        get=lambda url: pytest.fail("no retry expected"), delay_sec=0,
    )
    assert [c["name"] for c in cards] == ["Mox Pearl", "Life // Death"]
    assert post.calls[0]["identifiers"] == [{"name": "Mox Pearl"}, {"name": "Life"}]


def test_resolve_names_retries_mismatch_with_exact_lookup(fc):
    """``Regrowth`` by name can come back as a newer card whose second
    face is called Regrowth; the exact-name retry fixes it."""
    post = _collection_by_front_face({
        "Regrowth": {"name": "Emeritus of Abundance // Regrowth"},
    })
    urls: list[str] = []

    def get(url):
        urls.append(url)
        return {"name": "Regrowth", "set": "lea"}

    cards = fc.resolve_names(["Regrowth"], post=post, get=get, delay_sec=0)
    assert cards == [{"name": "Regrowth", "set": "lea"}]
    assert urls == [fc.SCRYFALL_NAMED_URL.format(name="Regrowth")]


def test_resolve_names_unresolved_is_loud(fc):
    post = _collection_by_front_face({})

    def get(url):
        raise OSError("404")

    with pytest.raises(ValueError, match="could not resolve 2 names"):
        fc.resolve_names(["Nope", "Also Nope"], post=post, get=get, delay_sec=0)
    with pytest.raises(ValueError, match="could not resolve 1 names"):
        fc.resolve_names(["Nope"], post=post,
                         get=lambda url: {"name": "Something Else"}, delay_sec=0)


def test_resolve_names_sleeps_between_requests(fc, monkeypatch):
    slept: list[float] = []
    monkeypatch.setattr(fc.time, "sleep", slept.append)
    post = _collection_by_front_face({})
    cards =fc.resolve_names(["X"], post=post,
                             get=lambda url: {"name": "X"}, delay_sec=0.5)
    assert cards == [{"name": "X"}]
    assert slept == [0.5, 0.5]  # one collection page + one exact retry


def test_build_pool_from_names_appends_missing_basics(fc):
    catalog = {n: {"name": n} for n in ("Mox Pearl", *fc.NAME_LIST_BASIC_LAND_NAMES)}
    post = _collection_by_front_face(catalog)
    pool = fc.build_pool_from_names(
        ["Mox Pearl", "Island"], post=post, get=lambda url: {}, delay_sec=0,
    )
    names = [c["name"] for c in pool]
    assert names[:2] == ["Mox Pearl", "Island"]
    assert sorted(names[2:]) == sorted(
        b for b in fc.NAME_LIST_BASIC_LAND_NAMES if b != "Island"
    )
    assert len(names) == len(set(names))


def test_compare_with_cube_two_way_and_printed_names(fc):
    cube = _cube_json(["id-a", "id-b", "id-c"])
    cube["id"] = "uuid-1"
    cube["shortId"] = ""
    mainboard = cube["cards"]["mainboard"]
    mainboard[0]["details"]["name"] = "Ademi of the Silkchutes"
    mainboard[1]["details"]["name"] = "Delver of Secrets"
    mainboard[2]["details"]["name"] = "Cube Only"
    post = _collection_by_front_face({
        "id-a": {"name": "Spectacular Spider-Man", "id": "id-a"},
        "id-b": {"name": "Delver of Secrets // Insectile Aberration", "id": "id-b"},
        "id-c": {"name": "Cube Only", "id": "id-c"},
    })
    diff = fc.compare_with_cube(
        ["Spectacular Spider-Man", "Delver of Secrets // Insectile Aberration",
         "Smokestack"],
        cube, post=post, delay_sec=0,
    )
    assert diff["cube_id"] == "uuid-1"
    assert diff["cube_card_count"] == 3
    assert diff["cube_last_updated"].startswith("2023-11-14")
    assert diff["only_in_list"] == ["Smokestack"]
    assert diff["only_in_cube"] == ["Cube Only"]
    # A DFC's front-face name is not a rename; a printed alias is.
    assert diff["cube_printed_name_differs"] == [
        ["Ademi of the Silkchutes", "Spectacular Spider-Man"],
    ]
    no_date = dict(cube, dateLastUpdated=None)
    assert fc.compare_with_cube(
        [], no_date, post=post, delay_sec=0,
    )["cube_last_updated"] is None


def test_name_list_metadata_paths(fc, tmp_path):
    inside = fc.REPO_ROOT / "data" / "x.sheet.json"
    meta = fc.name_list_metadata(
        names_path=inside, cube_name="Roto", n_list_entries=3,
        scryfall_names=["A", "B"], custom=[{"name": "C", "custom_text": None}],
        n_cards=7, comparison=None,
    )
    assert meta["name_list_path"] == "data/x.sheet.json"
    assert meta["source"] == "name_list"
    assert meta["list_entry_count"] == 3
    assert meta["scryfall_card_count"] == 2
    assert meta["pool_card_count"] == 7
    assert meta["custom_cards_without_scryfall"][0]["name"] == "C"
    outside = tmp_path / "list.json"
    assert fc.name_list_metadata(
        names_path=outside, cube_name=None, n_list_entries=0,
        scryfall_names=[], custom=[], n_cards=0, comparison=None,
    )["name_list_path"] == outside.as_posix()


def test_main_names_mode_writes_pool_meta_and_comparison(
    fc, tmp_path, monkeypatch, capsys,
):
    names_path = tmp_path / "list.json"
    names_path.write_text(json.dumps([
        {"name": "Card a", "scryfall_name": "Card a"},
        {"name": "Custom", "scryfall_name": None, "custom_text": "draft text"},
    ]), encoding="utf-8")
    catalog = {"Card a": {"name": "Card a", "id": "a"},
               "a": {"name": "Card a", "id": "a"}}
    catalog.update({b: {"name": b} for b in fc.NAME_LIST_BASIC_LAND_NAMES})
    monkeypatch.setattr(fc, "post_json", _collection_by_front_face(catalog))
    monkeypatch.setattr(fc, "get_json", lambda url: _cube_json(["a"]))
    monkeypatch.setattr(fc.time, "sleep", lambda s: None)
    out = tmp_path / "roto.json"
    assert fc.main([
        "--names", str(names_path), "--cube-name", "Roto",
        "--compare-cube", "uuid", "--out", str(out),
    ]) == 0
    cards = json.loads(out.read_text(encoding="utf-8"))
    assert cards[0]["name"] == "Card a"
    assert len(cards) == 1 + len(fc.NAME_LIST_BASIC_LAND_NAMES)
    meta = json.loads((tmp_path / "roto.meta.json").read_text(encoding="utf-8"))
    assert meta["cube_name"] == "Roto"
    assert meta["custom_cards_without_scryfall"] == [
        {"name": "Custom", "custom_text": "draft text"},
    ]
    assert meta["cubecobra_comparison"]["only_in_list"] == []
    printed = capsys.readouterr().out
    assert "custom (no Scryfall record): Custom" in printed
    assert "only in CubeCobra" in printed


def test_main_names_mode_without_comparison(fc, tmp_path, monkeypatch):
    names_path = tmp_path / "list.json"
    names_path.write_text(json.dumps([
        {"name": "Card a", "scryfall_name": "Card a"},
    ]), encoding="utf-8")
    catalog = {"Card a": {"name": "Card a"}}
    catalog.update({b: {"name": b} for b in fc.NAME_LIST_BASIC_LAND_NAMES})
    monkeypatch.setattr(fc, "post_json", _collection_by_front_face(catalog))
    monkeypatch.setattr(fc, "get_json",
                        lambda url: pytest.fail("no CubeCobra call"))
    monkeypatch.setattr(fc.time, "sleep", lambda s: None)
    out = tmp_path / "roto.json"
    assert fc.main(["--names", str(names_path), "--out", str(out)]) == 0
    meta = json.loads((tmp_path / "roto.meta.json").read_text(encoding="utf-8"))
    assert meta["cubecobra_comparison"] is None


def test_committed_roto_pool_matches_meta(fc):
    """The committed roto dump agrees with its sidecar and the vendored
    sheet list: every Scryfall-backed sheet card + the basics, the four
    custom conspiracies listed (not faked), and the CubeCobra diff."""
    root = fc.REPO_ROOT / "data"
    cards = json.loads((root / "cube_roto.json").read_text(encoding="utf-8"))
    meta = json.loads((root / "cube_roto.meta.json").read_text(encoding="utf-8"))
    sheet = json.loads((root / "cube_roto.sheet.json").read_text(encoding="utf-8"))
    names, custom = fc.load_name_list(sheet)
    pool_names = [c["name"] for c in cards]
    assert meta["pool_card_count"] == len(cards) == len(set(pool_names))
    assert meta["list_entry_count"] == len(sheet) == 864
    assert meta["scryfall_card_count"] == len(names) == 860
    assert set(names) | set(fc.NAME_LIST_BASIC_LAND_NAMES) == set(pool_names)
    assert [c["name"] for c in meta["custom_cards_without_scryfall"]] == [
        c["name"] for c in custom
    ]
    assert all(c["custom_text"] for c in meta["custom_cards_without_scryfall"])
    assert not {c["name"] for c in custom} & set(pool_names)
    cmp = meta["cubecobra_comparison"]
    assert cmp["only_in_list"] == ["Smokestack"]
    assert cmp["only_in_cube"] == []


def test_main_names_mode_without_basics(fc, tmp_path, monkeypatch):
    # ``--no-basics``: a pool of extra cards only (the legacy pool).
    names_path = tmp_path / "list.json"
    names_path.write_text(json.dumps([{"name": "Card a", "scryfall_name": "Card a"}]), encoding="utf-8")
    monkeypatch.setattr(fc, "post_json", _collection_by_front_face({"Card a": {"name": "Card a"}}))
    monkeypatch.setattr(fc, "get_json", lambda url: pytest.fail("no other call"))
    monkeypatch.setattr(fc.time, "sleep", lambda s: None)
    out = tmp_path / "extra.json"
    assert fc.main(["--names", str(names_path), "--no-basics", "--out", str(out)]) == 0
    assert [c["name"] for c in json.loads(out.read_text(encoding="utf-8"))] == ["Card a"]
    meta = json.loads((tmp_path / "extra.meta.json").read_text(encoding="utf-8"))
    assert meta["includes_basic_lands"] == [] and meta["pool_card_count"] == 1


def test_committed_legacy_pool_matches_meta(fc):
    """The committed legacy pool (mtg-zero's Delver mirror cards outside the
    cube, then the Legacy Doomsday deck's cards) is its sheet's
    cards, no basic lands, as its sidecar says."""
    root = fc.REPO_ROOT / "data"
    cards = json.loads((root / "legacy_delver.json").read_text(encoding="utf-8"))
    meta = json.loads((root / "legacy_delver.meta.json").read_text(encoding="utf-8"))
    sheet = json.loads((root / "legacy_delver.sheet.json").read_text(encoding="utf-8"))
    names, custom = fc.load_name_list(sheet)
    # The Delver mirror's two, then mtg-zero's Legacy Doomsday deck's cards:
    # lane A's four, lane B's three, lane C's three (mtg-zero ENGINE.md
    # 124.1-124.3), then its Legacy Sneak and Show deck's six (ENGINE.md 123),
    # then the Delver 75's five sideboard cards (ENGINE.md 122), then the
    # Legacy Hogaak Tempo deck's seven by lane (ENGINE.md 220.1-220.3):
    # lane A's one, lane B's two, lane C's four.
    assert [c["name"] for c in cards] == names == [
        "Murktide Regent", "Prismari Charm",
        "Doomsday", "Pact of Negation", "Personal Tutor", "Flow State",
        "Misdirection", "Flusterstorm", "Cavern of Souls",
        "Edge of Autumn", "Street Wraith", "Leyline of the Void",
        "Show and Tell", "Omniscience", "Mistrise Village",
        "Brotherhood's End", "Disruptor Flute", "Magus of the Moon",
        "Consign to Memory", "Hydroblast", "Meltdown", "Price of Progress", "Rough // Tumble",
        "Hogaak, Arisen Necropolis",
        "Mockingbird", "Massacre",
        "Stitcher's Supplier", "Dark Matter Manipulator", "Moonshadow", "Dryad Arbor",
    ]
    assert not custom and meta["includes_basic_lands"] == [] and meta["pool_card_count"] == 30
