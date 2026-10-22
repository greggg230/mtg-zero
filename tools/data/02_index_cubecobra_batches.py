"""Step 2a: index which CubeCobra export batches contain PowerLSV drafts.

CubeCobra's bulk export (s3://cubecobra-public/export/) is organized as
6,599 numbered batches under decks/, picks/ and cubeInstances/ -- batch N's
three files describe the same set of drafts in the same order. decks/{n}.json
is one record per *owned* (human) seat, so it is the cheap way (3.8 GB total
vs. 116 GB for picks/) to find which batch numbers are worth pulling picks
and cubeInstances for.

This script expects decks/ to already be synced locally to
E:\\mtg-zero\\data\\raw\\cubecobra\\decks\\ (see README.md: the fetch command
is `aws s3 sync s3://cubecobra-public/export/decks/ <dir> --no-sign-request`).

Output:
  E:\\mtg-zero\\data\\cubecobra\\powerlsv_decks.parquet   -- filtered human decks
  E:\\mtg-zero\\data\\cubecobra\\powerlsv_batches.json    -- sorted list of batch
                                                              numbers containing >=1
                                                              PowerLSV deck
After this runs successfully, the decks/ raw directory is deleted (it is the
"big" raw input the task spec says to drop once filtered).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pandas as pd

sys.path.insert(0, str(Path(__file__).parent))
from common import CUBECOBRA_DIR, POWERLSV_CUBE_ID, RAW_CUBECOBRA, check_disk_or_die, dump_json, ensure_dirs  # noqa: E402

DECKS_DIR = RAW_CUBECOBRA / "decks"


def main() -> None:
    ensure_dirs()
    check_disk_or_die()

    files = sorted(DECKS_DIR.glob("*.json"), key=lambda p: int(p.stem))
    print(f"Found {len(files)} decks batch files in {DECKS_DIR}")
    if not files:
        raise SystemExit(f"No batch files found under {DECKS_DIR} -- sync decks/ first.")

    matching_batches: set[int] = set()
    rows = []
    total_records = 0
    for fp in files:
        batch_no = int(fp.stem)
        with open(fp, encoding="utf-8") as f:
            records = json.load(f)
        total_records += len(records)
        for rec in records:
            if rec.get("cube") == POWERLSV_CUBE_ID:
                matching_batches.add(batch_no)
                rows.append(
                    {
                        "batch": batch_no,
                        "draft_id": rec.get("id"),
                        "owner": rec.get("owner"),
                        "mainboard": rec.get("mainboard"),
                        "sideboard": rec.get("sideboard"),
                        "basics": rec.get("basics"),
                    }
                )
        if batch_no % 500 == 0:
            print(f"  ...batch {batch_no}: {len(matching_batches)} matching batches so far, {len(rows)} PowerLSV decks")

    print(f"Scanned {total_records} total deck records across {len(files)} batches")
    print(f"PowerLSV decks: {len(rows)}; batches containing >=1 PowerLSV deck: {len(matching_batches)}")

    df = pd.DataFrame(rows)
    # mainboard/sideboard/basics are lists of ints (cube indexes); keep as python lists
    # inside object columns -- parquet via pyarrow handles list<int64> fine.
    out_path = CUBECOBRA_DIR / "powerlsv_decks.parquet"
    df.to_parquet(out_path, index=False)
    print("Wrote", out_path, df.shape)

    batches_path = CUBECOBRA_DIR / "powerlsv_batches.json"
    dump_json(sorted(matching_batches), batches_path)
    print("Wrote", batches_path)


if __name__ == "__main__":
    main()
