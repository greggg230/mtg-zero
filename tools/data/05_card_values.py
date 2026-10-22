"""Step 4: per-card values.

1. Pick strength (CubeCobra): multiway Bradley-Terry/Luce fit over pack
   contents (bt_luce.py), keyed by oracle_id. This is the context-free
   "which card do humans take over which other cards" signal; see
   docs/research/draft_data.md 4.2 for why BT over avg-pick-position (it
   handles variable pack size/composition without an ad hoc correction
   term, and is the standard MLE for "picked card beats each passed card").
2. Main-deck rate (CubeCobra): P(card in mainboard | card in pool, deck
   "built"). "Built" = mainboard+sideboard totals exactly 45 (one full
   drafted pool) and sideboard is non-empty (the user actually cut
   something, not just left the whole pool as "mainboard"); see
   tools/data/README.md for how this threshold was picked.
3. Main-deck rate (17lands): the dataset's own pick_maindeck_rate column,
   deduped per card name, PremierDraft and TradDraft separately.
4. Win rates (17lands game_data): per-card win rate when the game's deck
   contains >=1 copy, with sample size; same restricted to "experienced"
   users (user_n_games_bucket >= 50) as a light confound-reduction; plus the
   unconditional baseline win rate for reference. These are NOT causal --
   see docs/research/prior_art.md ("Embarrassingly Causal"): who drafts a
   card and whether a drafted card makes the final deck are both selection
   effects, so a card's raw in-deck win rate conflates card quality with
   drafter skill and deck context. Treated as a confounded signal here, not
   as ground truth.

Only the 545-card 17lands "Cube - Powered" vocabulary that matches the
PowerLSV canonical list by name (see 04_build_card_table.py) gets 17lands
columns; this keeps game_data reads to the ~391 matched columns instead of
545+, and keeps the 17lands numbers scoped to cards that are actually in
PowerLSV.

Output:
  E:\\mtg-zero\\data\\cards\\powerlsv_card_values.json          -- full table
  mtg-zero\\data\\powerlsv_card_values.json                     -- same, committed copy
"""
from __future__ import annotations

import gzip
import sys
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).parent))
from bt_luce import fit_from_pick_rows  # noqa: E402
from common import CARDS_DIR, COMMITTED_DATA, CUBECOBRA_DIR, RAW_17LANDS, RAW_CUBECOBRA, dump_json, load_json  # noqa: E402

EXPERIENCED_MIN_GAMES = 50


def cubecobra_pick_strength() -> pd.DataFrame:
    parts = sorted((CUBECOBRA_DIR / "picks").glob("part_*.parquet"))
    print(f"Loading {len(parts)} picks part files for pick-strength fit...")
    dfs = [pd.read_parquet(p, columns=["picked_oracle_id", "pack_oracle_ids"]) for p in parts]
    picks = pd.concat(dfs, ignore_index=True)
    print(f"  {len(picks)} pick events")
    out = fit_from_pick_rows(picks, "pack_oracle_ids", "picked_oracle_id", n_iter=60, ridge=0.5)
    out.index.name = "oracle_id"
    return out.rename(columns={"bt_strength": "cubecobra_bt_strength", "n_picked": "cubecobra_n_picked", "n_seen": "cubecobra_n_seen"})


def cubecobra_maindeck_rate() -> pd.DataFrame:
    index_map = load_json(RAW_CUBECOBRA / "indexToOracleMap.json")
    decks = pd.read_parquet(CUBECOBRA_DIR / "powerlsv_decks.parquet")
    decks["mb_len"] = decks["mainboard"].apply(len)
    decks["sb_len"] = decks["sideboard"].apply(len)
    built = decks[(decks["mb_len"] + decks["sb_len"] == 45) & (decks["sb_len"] > 0)]
    print(f"  built decks: {len(built)} of {len(decks)} ({len(built)/len(decks):.1%})")

    pool_count: dict[str, int] = {}
    main_count: dict[str, int] = {}
    for mb, sb in zip(built["mainboard"], built["sideboard"]):
        seen_this_deck = set()
        for idx in mb:
            oid = index_map.get(str(idx))
            if oid is None or oid in seen_this_deck:
                continue
            seen_this_deck.add(oid)
            pool_count[oid] = pool_count.get(oid, 0) + 1
            main_count[oid] = main_count.get(oid, 0) + 1
        for idx in sb:
            oid = index_map.get(str(idx))
            if oid is None or oid in seen_this_deck:
                continue
            seen_this_deck.add(oid)
            pool_count[oid] = pool_count.get(oid, 0) + 1

    rows = [
        {
            "oracle_id": oid,
            "cubecobra_pool_count": pool_count[oid],
            "cubecobra_mainboard_count": main_count.get(oid, 0),
            "cubecobra_maindeck_rate": main_count.get(oid, 0) / pool_count[oid],
        }
        for oid in pool_count
    ]
    return pd.DataFrame(rows).set_index("oracle_id")


def lands_maindeck_rate() -> pd.DataFrame:
    frames = []
    for fmt in ("PremierDraft", "TradDraft"):
        path = RAW_17LANDS / f"draft_data_public.Cube_-_Powered.{fmt}.csv.gz"
        with gzip.open(path, "rt", encoding="utf-8") as f:
            df = pd.read_csv(f, usecols=["pick", "pick_maindeck_rate"])
        g = df.groupby("pick").agg(
            **{f"lands17_maindeck_rate_{fmt.lower()}": ("pick_maindeck_rate", "mean"), f"lands17_pick_count_{fmt.lower()}": ("pick_maindeck_rate", "size")}
        )
        frames.append(g)
    out = frames[0].join(frames[1], how="outer")
    out.index.name = "lands_name"
    return out


def lands_win_rates(matched_names: list[str]) -> pd.DataFrame:
    rows = {}
    baselines = {}
    for fmt in ("PremierDraft", "TradDraft"):
        path = RAW_17LANDS / f"game_data_public.Cube_-_Powered.{fmt}.csv.gz"
        usecols = ["won", "user_n_games_bucket"] + [f"deck_{n}" for n in matched_names]
        with gzip.open(path, "rt", encoding="utf-8") as f:
            header = pd.read_csv(f, nrows=0).columns.tolist()
        usecols = [c for c in usecols if c in header]
        dtype = {c: "int8" for c in usecols if c.startswith("deck_")}
        dtype["won"] = "boolean"
        dtype["user_n_games_bucket"] = "int32"
        with gzip.open(path, "rt", encoding="utf-8") as f:
            df = pd.read_csv(f, usecols=usecols, dtype=dtype)
        print(f"  {fmt} game_data: {df.shape}")
        baselines[fmt] = (float(df["won"].mean()), int(len(df)))
        exp = df[df["user_n_games_bucket"] >= EXPERIENCED_MIN_GAMES]
        for name in matched_names:
            col = f"deck_{name}"
            if col not in df.columns:
                continue
            mask = df[col] >= 1
            n = int(mask.sum())
            wr = float(df.loc[mask, "won"].mean()) if n else None
            mask_exp = exp[col] >= 1 if col in exp.columns else None
            n_exp = int(mask_exp.sum()) if mask_exp is not None else 0
            wr_exp = float(exp.loc[mask_exp, "won"].mean()) if n_exp else None
            r = rows.setdefault(name, {})
            r[f"lands17_win_rate_indeck_{fmt.lower()}"] = wr
            r[f"lands17_win_rate_indeck_{fmt.lower()}_n"] = n
            r[f"lands17_win_rate_indeck_{fmt.lower()}_experienced"] = wr_exp
            r[f"lands17_win_rate_indeck_{fmt.lower()}_experienced_n"] = n_exp
    out = pd.DataFrame.from_dict(rows, orient="index")
    out.index.name = "lands_name"
    return out, baselines


def main() -> None:
    table = pd.read_parquet(CARDS_DIR / "powerlsv_card_table.parquet")

    bt = cubecobra_pick_strength()
    mdr = cubecobra_maindeck_rate()
    cc = bt.join(mdr, how="outer")

    matched_names = sorted(table.loc[table["in_17lands"], "lands_name"].dropna().unique().tolist())
    print(f"17lands matched names: {len(matched_names)}")
    l_mdr = lands_maindeck_rate()
    l_wr, baselines = lands_win_rates(matched_names)
    lands = l_mdr.join(l_wr, how="outer")

    merged = table.merge(cc, left_on="oracle_id", right_index=True, how="left")
    merged = merged.merge(lands, left_on="lands_name", right_index=True, how="left")
    # drop the duplicate of cubecobra_n_picked computed in step 04 from decks/picks scan
    merged = merged.drop(columns=["cubecobra_pick_count"], errors="ignore")

    float_cols = merged.select_dtypes(include=["float64", "float32"]).columns
    merged[float_cols] = merged[float_cols].round(6)

    merged_full = merged.copy()
    merged_full = merged_full.replace({np.nan: None})
    full_path = CARDS_DIR / "powerlsv_card_values.json"
    dump_json(
        {
            "baselines": {
                "win_rate_premierdraft": baselines["PremierDraft"],
                "win_rate_traddraft": baselines["TradDraft"],
            },
            "cards": merged_full.to_dict(orient="records"),
        },
        full_path,
    )
    print("Wrote", full_path, len(merged_full), "cards")

    # Small committed copy: columnar (columns + rows) instead of list-of-dicts,
    # so ~27 column names aren't repeated per row 1091 times -- that alone is
    # the difference between ~1.7 MB and comfortably under the 1 MB budget.
    # Full per-card-dict detail (incl. front_name/layout/type_line) stays in
    # the E: copy above.
    small = merged.drop(columns=["front_name", "layout", "type_line"], errors="ignore")
    small = small.replace({np.nan: None})
    import json

    committed_path = COMMITTED_DATA / "powerlsv_card_values.json"
    payload = {
        "baselines": {
            "win_rate_premierdraft": baselines["PremierDraft"],
            "win_rate_traddraft": baselines["TradDraft"],
        },
        "columns": small.columns.tolist(),
        "rows": small.to_numpy(dtype=object).tolist(),
    }
    with open(committed_path, "w", encoding="utf-8") as f:
        json.dump(payload, f, separators=(",", ":"), ensure_ascii=False)
    size_mb = committed_path.stat().st_size / 1e6
    print("Wrote", committed_path, f"({size_mb:.2f} MB)")
    if size_mb > 1.0:
        print("WARNING: committed copy exceeds 1 MB budget")


if __name__ == "__main__":
    main()
