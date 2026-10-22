"""Step 6: baseline yardsticks on the held-out pick sets.

(a) random: expected top-1/top-3 agreement by chance given the TRUE pack
    size at each pick (no model).
(b) greedy by pick strength: argmax of a card "rating" (log of a
    train-only-refit Bradley-Terry/Luce strength, min-max normalized to
    [0,1] so it is commensurate with the colour-commitment term below).
(c) pick strength + colour commitment (Draftmancer SimpleBot style):
    score = rating + lambda * colour_share, colour_share = fraction of the
    drafter's pool-so-far that shares >=1 colour with the candidate
    (colourless candidates get colour_share = 1, i.e. "always on-colour").
    lambda is grid-searched on TRAIN ONLY (0.0 to 2.0 step 0.1) to maximize
    top-1 agreement, then fixed for the TEST report.

IMPORTANT: the rating used here is refit from scratch on the CubeCobra
TRAIN split only (bt_luce.fit_from_pick_rows on cubecobra_picks_train.parquet),
not the published card-value table (which is fit on the full corpus for
production use). This keeps the "baseline the draft AI must beat" honestly
held out, at the cost of being a slightly different number from
powerlsv_card_values.json's cubecobra_bt_strength.

Two evaluations are reported:
  - CubeCobra-native: train-refit rating, evaluated on cubecobra_picks_test.
  - 17lands cross-dataset: the SAME CubeCobra-trained rating (mapped by
    name) evaluated on lands17_picks_test, restricted to picks where the
    human's actual choice is itself a canon-matched card, and restricted to
    candidates that are canon-matched (no rating exists for ~28% of the
    17lands card pool that isn't in PowerLSV at all -- see
    04_build_card_table.py). This is a generalization check across a
    different cube, not the primary yardstick; it is expected to be weaker.

Output: E:\\mtg-zero\\data\\cards\\baselines_report.json
        mtg-zero\\docs\\data_m1.md cites this report's numbers directly.
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pandas as pd

sys.path.insert(0, str(Path(__file__).parent))
from bt_luce import fit_from_pick_rows  # noqa: E402
from common import CARDS_DIR, SPLITS_DIR, dump_json  # noqa: E402

LAMBDA_GRID = np.round(np.arange(0.0, 2.01, 0.1), 2)
RNG_SEED = 42
TRAIN_SAMPLE_DRAFTS = 6000  # subsample of train drafts for lambda tuning (speed)


def random_baseline(pack_sizes: pd.Series) -> tuple[float, float]:
    top1 = float((1.0 / pack_sizes).mean())
    top3 = float((np.minimum(3, pack_sizes) / pack_sizes).mean())
    return top1, top3


def build_cubecobra_events(df: pd.DataFrame, rating: dict, colors_by_oid: dict) -> pd.DataFrame:
    """Long-form table: one row per (event, candidate). Reconstructs the
    drafter's pool-so-far incrementally per draft, in pick order."""
    df = df.sort_values(["draft_id", "pick_no_overall"])
    rows = []
    event_id = 0
    for draft_id, g in df.groupby("draft_id", sort=False):
        color_counts = {c: 0 for c in "WUBRG"}
        pool_size = 0
        for _, rec in g.iterrows():
            pack = rec["pack_oracle_ids"]
            picked = rec["picked_oracle_id"]
            for cand in pack:
                if cand is None:
                    continue
                r = rating.get(cand)
                if r is None:
                    continue
                colors = colors_by_oid.get(cand)
                colors = colors if isinstance(colors, str) else ""
                if colors:
                    share = sum(color_counts.get(c, 0) for c in colors) / max(1, pool_size)
                else:
                    share = 1.0
                rows.append((event_id, cand, r, share, cand == picked))
            event_id += 1
            # update running pool with the actual pick
            pc = colors_by_oid.get(picked)
            pc = pc if isinstance(pc, str) else ""
            if pc:
                for c in pc:
                    if c in color_counts:
                        color_counts[c] += 1
            pool_size += 1
    return pd.DataFrame(rows, columns=["event_id", "oracle_id", "rating", "color_share", "is_picked"])


def grid_search_lambda(events: pd.DataFrame) -> tuple[float, dict]:
    results = {}
    for lam in LAMBDA_GRID:
        events["score"] = events["rating"] + lam * events["color_share"]
        events["rank"] = events.groupby("event_id")["score"].rank(ascending=False, method="first")
        n_events = events["event_id"].nunique()
        top1 = events.loc[events["is_picked"], "rank"].le(1).sum() / n_events
        top3 = events.loc[events["is_picked"], "rank"].le(3).sum() / n_events
        results[float(lam)] = {"top1": float(top1), "top3": float(top3)}
    best_lam = max(results, key=lambda k: results[k]["top1"])
    return best_lam, results


def eval_fixed(events: pd.DataFrame, lam: float) -> dict:
    events = events.copy()
    events["score"] = events["rating"] + lam * events["color_share"]
    events["rank"] = events.groupby("event_id")["score"].rank(ascending=False, method="first")
    n_events = events["event_id"].nunique()
    top1 = events.loc[events["is_picked"], "rank"].le(1).sum() / n_events
    top3 = events.loc[events["is_picked"], "rank"].le(3).sum() / n_events
    return {"top1": float(top1), "top3": float(top3), "n_events": int(n_events)}


def cubecobra_baselines(table: pd.DataFrame) -> dict:
    train = pd.read_parquet(SPLITS_DIR / "cubecobra_picks_train.parquet")
    test = pd.read_parquet(SPLITS_DIR / "cubecobra_picks_test.parquet")
    print(f"CubeCobra train picks {len(train)} ({train['draft_id'].nunique()} drafts), test picks {len(test)} ({test['draft_id'].nunique()} drafts)")

    rng = np.random.default_rng(RNG_SEED)
    train_drafts = train["draft_id"].unique()
    sample_drafts = rng.choice(train_drafts, size=min(TRAIN_SAMPLE_DRAFTS, len(train_drafts)), replace=False)
    train_sample = train[train["draft_id"].isin(sample_drafts)]
    print(f"  tuning lambda on a sample of {len(sample_drafts)} train drafts ({len(train_sample)} picks)")

    bt_out = fit_from_pick_rows(train, "pack_oracle_ids", "picked_oracle_id", n_iter=60, ridge=0.5)
    log_v = np.log(bt_out["bt_strength"])
    rating_norm = (log_v - log_v.min()) / (log_v.max() - log_v.min())
    rating = rating_norm.to_dict()
    print(f"  refit BT on train: {len(rating)} oracle ids, n_picked range {bt_out['n_picked'].min()}-{bt_out['n_picked'].max()}")

    colors_by_oid = dict(zip(table["oracle_id"], table["colors"]))

    train_events = build_cubecobra_events(train_sample, rating, colors_by_oid)
    best_lam, grid_results = grid_search_lambda(train_events)
    print(f"  best lambda on train sample: {best_lam}")

    test_events = build_cubecobra_events(test, rating, colors_by_oid)
    pack_sizes = test.groupby("draft_id")["pack_size"] if False else test["pack_size"]
    rand_top1, rand_top3 = random_baseline(test["pack_size"])
    greedy = eval_fixed(test_events, 0.0)
    combo = eval_fixed(test_events, best_lam)

    return {
        "n_test_picks": int(len(test)),
        "n_test_drafts": int(test["draft_id"].nunique()),
        "random": {"top1": rand_top1, "top3": rand_top3},
        "greedy_pick_strength": greedy,
        "pick_strength_plus_color": {**combo, "lambda": float(best_lam)},
        "lambda_grid_on_train_sample": grid_results,
    }


def lands17_cross_eval(table: pd.DataFrame) -> dict:
    test = pd.read_parquet(SPLITS_DIR / "lands17_picks_test.parquet")
    print(f"17lands test picks: {len(test)} ({test['draft_id'].nunique()} drafts)")

    # reuse the CubeCobra train-refit rating by name
    train = pd.read_parquet(SPLITS_DIR / "cubecobra_picks_train.parquet")
    bt_out = fit_from_pick_rows(train, "pack_oracle_ids", "picked_oracle_id", n_iter=60, ridge=0.5)
    log_v = np.log(bt_out["bt_strength"])
    rating_norm = (log_v - log_v.min()) / (log_v.max() - log_v.min())
    rating_by_oid = rating_norm.to_dict()

    name_to_oid = dict(zip(table.loc[table["in_17lands"], "lands_name"], table.loc[table["in_17lands"], "oracle_id"]))
    rating_by_name = {n: rating_by_oid.get(oid) for n, oid in name_to_oid.items() if rating_by_oid.get(oid) is not None}

    scoreable = test[test["pick_matched"]].copy()
    print(f"  picks where human's choice is canon-matched: {len(scoreable)} of {len(test)}")

    rand_top1, rand_top3 = random_baseline(scoreable["pack_size_total"])

    colors_by_name = dict(zip(table["name"], table["colors"]))
    rows = []
    event_id = 0
    for _, rec in scoreable.iterrows():
        candidates = rec["pack_matched_present"]
        picked = rec["picked_name"]
        pool_sizes = {
            "W": rec["pool_W"], "U": rec["pool_U"], "B": rec["pool_B"], "R": rec["pool_R"], "G": rec["pool_G"],
        }
        pool_total = max(1, rec["pool_size_total"])
        for cand in candidates:
            r = rating_by_name.get(cand)
            if r is None:
                continue
            colors = colors_by_name.get(cand)
            colors = colors if isinstance(colors, str) else ""
            share = (sum(pool_sizes.get(c, 0) for c in colors) / pool_total) if colors else 1.0
            rows.append((event_id, cand, r, share, cand == picked))
        event_id += 1
    events = pd.DataFrame(rows, columns=["event_id", "name", "rating", "color_share", "is_picked"])
    events = events[events.groupby("event_id")["is_picked"].transform("any")]

    greedy = eval_fixed(events, 0.0)
    # reuse the lambda tuned on CubeCobra train (cross-dataset: no separate 17lands-train tuning)
    return {
        "n_test_picks_total": int(len(test)),
        "n_test_picks_scoreable": int(events["event_id"].nunique()),
        "random": {"top1": rand_top1, "top3": rand_top3},
        "greedy_pick_strength": greedy,
        "note": "cross-dataset check: rating trained on CubeCobra, scored on 17lands 'Cube - Powered' test picks restricted to canon-matched choices and candidates",
    }


def main() -> None:
    table = pd.read_parquet(CARDS_DIR / "powerlsv_card_table.parquet")
    report = {
        "cubecobra_native": cubecobra_baselines(table),
        "lands17_cross_dataset": lands17_cross_eval(table),
    }
    dump_json(report, CARDS_DIR / "baselines_report.json")
    print("Wrote", CARDS_DIR / "baselines_report.json")
    print(report["cubecobra_native"]["random"], report["cubecobra_native"]["greedy_pick_strength"], report["cubecobra_native"]["pick_strength_plus_color"])


if __name__ == "__main__":
    main()
