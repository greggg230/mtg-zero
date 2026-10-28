"""Step 8 (M6 part 3, docs/ENGINE.md 45): held-out human drafts for the draft network's
agreement yardstick (DESIGN.md 2.3.8: a logged curiosity, never optimised).

From the CubeCobra TEST split (06_holdout_split.py: whole drafts held out, seed 42) keep the
drafts that are one seat's whole draft of three 15-card packs (45 picks in pack order, pack
sizes 15..1) and consistent with an 8-seat pod (every pack seen again eight picks later holds
only cards of its earlier view less the card taken then, no card in two packs: the shape of a
4v4 `team_draft` event's seat), and write a seeded sample of them as JSON (raw CubeCobra names; the mapping to
the current cube, face names, and substitutes for cards of older cube versions are the
evaluation's, mtgz/az/human_picks.py):

  {"source": ..., "seed": 42, "drafts": [{"draft_id": ..., "views": [[names] x 45], "picks": [names] x 45}, ...]}

Output: E:\\mtg-zero\\data\\splits\\cubecobra_heldout_8seat.json (never committed: raw human picks).

  "C:/Users/Gregg Keithley/mtg-zero/.venv/Scripts/python.exe" tools/data/08_heldout_drafts.py [--n 400]
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq

sys.path.insert(0, str(Path(__file__).parent))
from common import SPLITS_DIR  # noqa: E402

SEED = 42
OUT = SPLITS_DIR / "cubecobra_heldout_8seat.json"


def consistent(rows: list[dict], seats: int = 8) -> bool:
    """An 8-seat pod's seat: a pack seen again `seats` picks later holds only cards of its earlier
    view less the card taken then, and no card is in two packs (the 24 packs' first views are
    disjoint: a singleton cube; a few drafts of the split repeat a card)."""
    first = []
    for p in (1, 2, 3):
        rr = [r for r in rows if r["pack_number"] == p]
        for k in range(15 - seats):
            if not set(rr[k + seats]["pack_names"]) <= set(rr[k]["pack_names"]) - {rr[k]["picked_name"]}:
                return False
        for k in range(seats):
            first += rr[k]["pack_names"]
    return len(first) == len(set(first))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--n", type=int, default=400, help="drafts in the sample")
    a = ap.parse_args()
    t = pq.read_table(SPLITS_DIR / "cubecobra_picks_test.parquet",
                      columns=["draft_id", "pick_no_overall", "pack_number", "pick_in_pack", "picked_name",
                               "pack_names"]).to_pylist()
    by = defaultdict(list)
    for r in t:
        by[r["draft_id"]].append(r)
    keep = []
    order = [(p, k) for p in (1, 2, 3) for k in range(1, 16)]
    for d in sorted(by):
        rows = sorted(by[d], key=lambda r: r["pick_no_overall"])
        if [(r["pack_number"], r["pick_in_pack"]) for r in rows] != order:
            continue
        if any(len(r["pack_names"]) != 16 - r["pick_in_pack"] or len(set(r["pack_names"])) != len(r["pack_names"])
               or r["picked_name"] not in r["pack_names"] for r in rows):
            continue
        if consistent(rows):
            keep.append((d, rows))
    rng = np.random.default_rng(SEED)
    pick = sorted(rng.choice(len(keep), size=min(a.n, len(keep)), replace=False).tolist())
    drafts = [{"draft_id": keep[i][0], "views": [list(r["pack_names"]) for r in keep[i][1]],
               "picks": [r["picked_name"] for r in keep[i][1]]} for i in pick]
    OUT.write_text(json.dumps({"source": "CubeCobra PowerLSV picks, test split (06_holdout_split.py, seed 42)",
                               "seed": SEED, "eligible": len(keep), "test_drafts": len(by), "drafts": drafts}),
                   encoding="utf-8")
    print(f"{len(by)} test drafts, {len(keep)} whole 3x15 drafts of an 8-seat pod; wrote {len(drafts)} to {OUT} "
          f"({OUT.stat().st_size / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
