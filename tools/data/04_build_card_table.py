"""Step 3: build the canonical PowerLSV card table.

Keyed by Scryfall oracle_id from the 546-card canonical list
(mtg-bot/data/cube_power_lsv.json, read-only, 541 nonbasics + 5 basics).
Maps onto it:
  - CubeCobra oracle ids actually seen in the filtered PowerLSV export
    (decks mainboard/sideboard/basics, picks pack/picked, cubeInstances dealt
    pool). CubeCobra is already oracle_id-native, so this is an exact join;
    extra oracle ids seen in the data but absent from the current 546-card
    list are kept as additional rows (in_current_cube=False) -- that is cube
    version drift, not a bug (PowerLSV is versioned; see docs/research/
    draft_data.md 1.2-1.3).
  - 17lands "Cube - Powered" card names (pack_card_ / pool_ columns of
    draft_data). 17lands has no oracle ids, only names, and (per
    prior_art.md) its Arena "Cube" pool is a *different* curated list from
    PowerLSV with real, substantial non-overlap -- not just version drift.
    So 17lands match is name-based (exact, else front-face for
    transform/MDFC/adventure/split cards) and only enriches the 546 canon
    rows; 17lands names with no canon match are reported but not added as
    rows (they are not PowerLSV cards).

Requires: steps 02 (powerlsv_decks.parquet) and 03 (picks/, cube_instances/)
to have completed.

Output:
  E:\\mtg-zero\\data\\cards\\powerlsv_card_table.parquet   -- full table
  E:\\mtg-zero\\data\\cards\\card_table_report.json        -- overlap/unmapped counts
"""
from __future__ import annotations

import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).parent))
from common import (  # noqa: E402
    CARDS_DIR,
    CUBECOBRA_DIR,
    MTG_BOT_CUBE_JSON,
    RAW_17LANDS,
    RAW_CUBECOBRA,
    dump_json,
    ensure_dirs,
    load_json,
)


def load_canon() -> pd.DataFrame:
    cube = load_json(MTG_BOT_CUBE_JSON)
    rows = []
    for c in cube:
        faces = c.get("card_faces")
        front_name = faces[0]["name"] if faces else None
        colors = c.get("colors") or (faces[0].get("colors") if faces else None) or []
        rows.append(
            {
                "oracle_id": c["oracle_id"],
                "name": c["name"],
                "front_name": front_name,
                "layout": c.get("layout"),
                "type_line": c.get("type_line"),
                "cmc": c.get("cmc"),
                "colors": "".join(colors),
                "is_basic_land": "Basic Land" in (c.get("type_line") or ""),
                "in_current_cube": True,
            }
        )
    df = pd.DataFrame(rows)
    assert df["oracle_id"].is_unique, "duplicate oracle_id in canonical cube list"
    return df


def load_17lands_names() -> set[str]:
    import gzip

    path = RAW_17LANDS / "draft_data_public.Cube_-_Powered.PremierDraft.csv.gz"
    with gzip.open(path, "rt", encoding="utf-8") as f:
        header = pd.read_csv(f, nrows=0).columns.tolist()
    return {c[len("pack_card_"):] for c in header if c.startswith("pack_card_")}


def load_cubecobra_oracle_ids() -> tuple[set[str], pd.Series]:
    """All oracle ids seen anywhere in the filtered PowerLSV CubeCobra export,
    plus a value_counts of picked_oracle_id for a quick popularity cross-check."""
    index_map = load_json(RAW_CUBECOBRA / "indexToOracleMap.json")

    decks = pd.read_parquet(CUBECOBRA_DIR / "powerlsv_decks.parquet")
    deck_oids: set[str] = set()
    for col in ("mainboard", "sideboard", "basics"):
        for lst in decks[col]:
            for idx in lst:
                oid = index_map.get(str(idx))
                if oid:
                    deck_oids.add(oid)

    picks_dir = CUBECOBRA_DIR / "picks"
    pick_parts = sorted(picks_dir.glob("part_*.parquet"))
    pick_oids: set[str] = set()
    picked_counts = pd.Series(dtype="int64")
    for p in pick_parts:
        df = pd.read_parquet(p, columns=["picked_oracle_id", "pack_oracle_ids"])
        pick_oids.update(df["picked_oracle_id"].dropna().unique())
        for lst in df["pack_oracle_ids"]:
            pick_oids.update(oid for oid in lst if oid)
        picked_counts = picked_counts.add(df["picked_oracle_id"].value_counts(), fill_value=0)

    ci_dir = CUBECOBRA_DIR / "cube_instances"
    ci_oids: set[str] = set()
    for p in sorted(ci_dir.glob("part_*.parquet")):
        df = pd.read_parquet(p, columns=["oracle_ids"])
        for lst in df["oracle_ids"]:
            ci_oids.update(oid for oid in lst if oid)

    all_oids = deck_oids | pick_oids | ci_oids
    print(f"  cubecobra oracle ids: decks={len(deck_oids)} picks={len(pick_oids)} cubeInstances={len(ci_oids)} union={len(all_oids)}")
    return all_oids, picked_counts


def main() -> None:
    ensure_dirs()
    canon = load_canon()
    print(f"Canonical cube: {len(canon)} cards ({canon['is_basic_land'].sum()} basics)")

    lands_names = load_17lands_names()
    print(f"17lands 'Cube - Powered' vocabulary: {len(lands_names)} names")

    canon["lands_name"] = None
    canon["in_17lands"] = False
    name_hit = canon["name"].isin(lands_names)
    canon.loc[name_hit, "lands_name"] = canon.loc[name_hit, "name"]
    canon.loc[name_hit, "in_17lands"] = True
    front_hit = (~name_hit) & canon["front_name"].isin(lands_names)
    canon.loc[front_hit, "lands_name"] = canon.loc[front_hit, "front_name"]
    canon.loc[front_hit, "in_17lands"] = True

    matched_lands_names = set(canon.loc[canon["in_17lands"], "lands_name"])
    unmatched_lands_names = sorted(lands_names - matched_lands_names)

    cc_oids, picked_counts = load_cubecobra_oracle_ids()
    canon["in_cubecobra"] = canon["oracle_id"].isin(cc_oids)
    canon["cubecobra_pick_count"] = canon["oracle_id"].map(picked_counts).fillna(0).astype(int)

    extra_oids = sorted(cc_oids - set(canon["oracle_id"]))
    name_map = load_json(RAW_CUBECOBRA / "simpleCardDict.json")
    extra_rows = []
    for oid in extra_oids:
        info = name_map.get(oid, {})
        extra_rows.append(
            {
                "oracle_id": oid,
                "name": info.get("name"),
                "front_name": None,
                "layout": None,
                "type_line": info.get("type"),
                "cmc": info.get("cmc"),
                "colors": None,
                "is_basic_land": False,
                "in_current_cube": False,
                "lands_name": None,
                "in_17lands": False,
                "in_cubecobra": True,
                "cubecobra_pick_count": int(picked_counts.get(oid, 0)),
            }
        )
    extra_df = pd.DataFrame(extra_rows)

    table = pd.concat([canon, extra_df], ignore_index=True)
    out_path = CARDS_DIR / "powerlsv_card_table.parquet"
    table.to_parquet(out_path, index=False)
    print("Wrote", out_path, table.shape)

    report = {
        "canonical_cards": int(len(canon)),
        "canonical_basics": int(canon["is_basic_land"].sum()),
        "canonical_matched_17lands": int(canon["in_17lands"].sum()),
        "canonical_matched_17lands_via_front_face": int(front_hit.sum()),
        "canonical_unmatched_17lands": int((~canon["in_17lands"]).sum()),
        "17lands_vocab_size": len(lands_names),
        "17lands_names_not_in_canon": len(unmatched_lands_names),
        "17lands_names_not_in_canon_sample": unmatched_lands_names[:40],
        "canonical_in_cubecobra_history": int(canon["in_cubecobra"].sum()),
        "canonical_never_seen_in_cubecobra": int((~canon["in_cubecobra"]).sum()),
        "cubecobra_oracle_ids_outside_canon": len(extra_oids),
        "cubecobra_outside_canon_sample_names": [r["name"] for r in extra_rows[:40]],
    }
    dump_json(report, CARDS_DIR / "card_table_report.json")
    print("Wrote report:", CARDS_DIR / "card_table_report.json")
    for k, v in report.items():
        if not isinstance(v, list):
            print(f"  {k}: {v}")


if __name__ == "__main__":
    main()
