"""Step 5: held-out pick sets, split by DRAFT (not by row), fixed seed.

CubeCobra: draft_id = "{batch}:{owner}:{cube_cards_index}" (see
03_filter_cubecobra_picks.py). 17lands: draft_id is already a column.

For CubeCobra we keep full pack_oracle_ids per pick (already small, list of
<=15 oracle ids -- see 03). For 17lands we do NOT materialize all 545
pack_card_*/pool_* one-hot columns per row (2.3M rows x 545 cols is a lot of
memory for no benefit): we only need, per pick, (a) the TRUE total pack/pool
size for a correct random baseline and pool-size denominator, (b) which of
the ~391 canon-matched cards are present in the pack (enough to run the
value-greedy baselines -- an unmatched alternative never has a value so it
never gets chosen by baselines (b)/(c); see 04_build_card_table.py for the
17lands/PowerLSV name overlap), and (c) per-colour pool totals for the
colour-commitment baseline. That turns 545 one-hot columns into ~6 summed
columns + one short list column, computed in chunks (chunksize=40_000) to
keep memory bounded.

Output (parquet), seed=42, 90/10 by draft:
  E:\\mtg-zero\\data\\splits\\cubecobra_picks_{train,test}.parquet
  E:\\mtg-zero\\data\\splits\\lands17_picks_{train,test}.parquet
"""
from __future__ import annotations

import gzip
import sys
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).parent))
from common import CARDS_DIR, CUBECOBRA_DIR, RAW_17LANDS, SPLITS_DIR, ensure_dirs  # noqa: E402

SEED = 42
TEST_FRAC = 0.10


def split_ids(ids: list, seed: int = SEED, test_frac: float = TEST_FRAC) -> tuple[set, set]:
    rng = np.random.default_rng(seed)
    ids = sorted(set(ids))
    perm = rng.permutation(len(ids))
    n_test = int(round(len(ids) * test_frac))
    test_idx = set(perm[:n_test].tolist())
    test_ids = {ids[i] for i in test_idx}
    train_ids = set(ids) - test_ids
    return train_ids, test_ids


def split_cubecobra() -> None:
    parts = sorted((CUBECOBRA_DIR / "picks").glob("part_*.parquet"))
    df = pd.concat([pd.read_parquet(p) for p in parts], ignore_index=True)
    print(f"CubeCobra picks: {len(df)} rows, {df['draft_id'].nunique()} drafts")
    train_ids, test_ids = split_ids(df["draft_id"].unique().tolist())
    print(f"  train drafts: {len(train_ids)}, test drafts: {len(test_ids)}")
    df[df["draft_id"].isin(train_ids)].to_parquet(SPLITS_DIR / "cubecobra_picks_train.parquet", index=False)
    df[df["draft_id"].isin(test_ids)].to_parquet(SPLITS_DIR / "cubecobra_picks_test.parquet", index=False)
    print("  wrote cubecobra_picks_{train,test}.parquet")


def build_color_cols(matched_names: list[str], colors_by_name: dict[str, str]) -> dict[str, list[str]]:
    by_color: dict[str, list[str]] = {c: [] for c in "WUBRG"}
    colorless = []
    for name in matched_names:
        colors = colors_by_name.get(name, "") or ""
        if not colors:
            colorless.append(name)
        for c in colors:
            if c in by_color:
                by_color[c].append(name)
    by_color["C"] = colorless
    return by_color


def process_lands_format(fmt: str, matched_names: list[str], colors_by_name: dict[str, str]) -> pd.DataFrame:
    path = RAW_17LANDS / f"draft_data_public.Cube_-_Powered.{fmt}.csv.gz"
    with gzip.open(path, "rt", encoding="utf-8") as f:
        header = pd.read_csv(f, nrows=0).columns.tolist()
    all_pack_cols = [c for c in header if c.startswith("pack_card_")]
    all_pool_cols = [c for c in header if c.startswith("pool_")]
    matched_pack_cols = [f"pack_card_{n}" for n in matched_names if f"pack_card_{n}" in header]
    matched_pack_names = [c[len("pack_card_"):] for c in matched_pack_cols]
    pool_cols_by_color = build_color_cols(matched_names, colors_by_name)
    # translate to actual pool_ column names present in header
    pool_cols_by_color = {
        color: [f"pool_{n}" for n in names if f"pool_{n}" in header] for color, names in pool_cols_by_color.items()
    }

    base_cols = ["draft_id", "pack_number", "pick_number", "pick"]
    keep_cols = base_cols + all_pack_cols + all_pool_cols
    dtype = {c: "int8" for c in all_pack_cols + all_pool_cols}
    chunks_out = []
    chunksize = 40_000
    with gzip.open(path, "rt", encoding="utf-8") as f:
        reader = pd.read_csv(f, usecols=keep_cols, dtype=dtype, chunksize=chunksize)
        for chunk in reader:
            pack_size_total = chunk[all_pack_cols].sum(axis=1).astype("int32")
            pool_size_total = chunk[all_pool_cols].sum(axis=1).astype("int32")

            pack_bool = (chunk[matched_pack_cols].to_numpy() > 0) if matched_pack_cols else np.zeros((len(chunk), 0), dtype=bool)
            pack_matched_present = [
                [matched_pack_names[j] for j in row.nonzero()[0]] for row in pack_bool
            ]

            out = pd.DataFrame(
                {
                    "draft_id": chunk["draft_id"].to_numpy(),
                    "pack_number": chunk["pack_number"].to_numpy(),
                    "pick_number": chunk["pick_number"].to_numpy(),
                    "pick_no_overall": ((chunk["pack_number"] - 1) * 15 + chunk["pick_number"]).to_numpy(),
                    "picked_name": chunk["pick"].to_numpy(),
                    "pick_matched": chunk["pick"].isin(matched_names).to_numpy(),
                    "pack_size_total": pack_size_total.to_numpy(),
                    "pack_matched_present": pack_matched_present,
                    "pool_size_total": pool_size_total.to_numpy(),
                }
            )
            for color, cols in pool_cols_by_color.items():
                out[f"pool_{color}"] = chunk[cols].sum(axis=1).to_numpy() if cols else 0
            chunks_out.append(out)
    return pd.concat(chunks_out, ignore_index=True)


def split_lands() -> None:
    table = pd.read_parquet(CARDS_DIR / "powerlsv_card_table.parquet")
    matched = table[table["in_17lands"]][["lands_name", "colors"]].dropna(subset=["lands_name"])
    matched_names = matched["lands_name"].tolist()
    colors_by_name = dict(zip(matched["lands_name"], matched["colors"]))
    print(f"17lands matched names used for pack reconstruction: {len(matched_names)}")

    frames = []
    for fmt in ("PremierDraft", "TradDraft"):
        df = process_lands_format(fmt, matched_names, colors_by_name)
        df["format"] = fmt
        print(f"  {fmt}: {len(df)} picks, {df['draft_id'].nunique()} drafts, matched-pick rate {df['pick_matched'].mean():.3f}")
        frames.append(df)
    df = pd.concat(frames, ignore_index=True)

    train_ids, test_ids = split_ids(df["draft_id"].unique().tolist())
    print(f"  total drafts {df['draft_id'].nunique()}: train {len(train_ids)}, test {len(test_ids)}")
    df[df["draft_id"].isin(train_ids)].to_parquet(SPLITS_DIR / "lands17_picks_train.parquet", index=False)
    df[df["draft_id"].isin(test_ids)].to_parquet(SPLITS_DIR / "lands17_picks_test.parquet", index=False)
    print("  wrote lands17_picks_{train,test}.parquet")


def main() -> None:
    ensure_dirs()
    split_cubecobra()
    split_lands()


if __name__ == "__main__":
    main()
