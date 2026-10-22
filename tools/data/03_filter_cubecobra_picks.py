"""Step 2b: fetch only the CubeCobra picks/ and cubeInstances/ batches that
contain PowerLSV drafts (per powerlsv_batches.json from step 02), keep the
PowerLSV rows, decode card indexes to Scryfall oracle ids + names, and write
parquet. Raw per-batch JSON is deleted immediately after it is filtered, so
disk use stays bounded (never holds more than a couple of batches' raw JSON
at once despite ~80 GB of total raw bytes behind the ~4,365 matching batches).

Draft identity: CubeCobra's picks/ records carry no draft id. Within one
batch, picks that share (owner, cubeCards) belong to the same draft --
cubeCards is the position of this draft in that batch's cubeInstances list
(verified against a hand-fetched sample: 14 PowerLSV (owner, cubeCards)
groups in batch 0, each with exactly 45 picks). We use
draft_id = f"{batch}:{owner}:{cubeCards}" throughout. pick_number_overall is
len(pool) at the time of the pick (pool includes the picked card); pack
number / pick-in-pack assume 3 packs of 15 (per PowerLSV's spec), which the
data confirms (pool sizes run 1..45 per draft, no draft sampled runs longer).

Output (parquet, partitioned into part files to keep memory bounded):
  E:\\mtg-zero\\data\\cubecobra\\picks\\part_*.parquet
    one row per pick: draft_id, batch, owner, pack_number, pick_number,
    pick_no_overall, picked_oracle_id, picked_name, pack_oracle_ids (list),
    pack_names (list), pool_size_before (pool size excluding this pick)
  E:\\mtg-zero\\data\\cubecobra\\cube_instances\\part_*.parquet
    one row per PowerLSV draft referenced by picks: draft_id, batch,
    cube_cards_index, oracle_ids (list, the dealt card pool incl. basics)

Rerun: safe to re-run from scratch (it only reads from S3 and
powerlsv_batches.json); it overwrites the part files. For a partial re-run
after an interruption, see README.md.
"""
from __future__ import annotations

import concurrent.futures as cf
import json
import sys
import time
from pathlib import Path

import pandas as pd
import requests

sys.path.insert(0, str(Path(__file__).parent))
from common import CUBECOBRA_DIR, POWERLSV_CUBE_ID, RAW_CUBECOBRA, check_disk_or_die, load_json  # noqa: E402

BUCKET = "https://cubecobra-public.s3.amazonaws.com/export"
PICKS_OUT = CUBECOBRA_DIR / "picks"
CI_OUT = CUBECOBRA_DIR / "cube_instances"
TMP = RAW_CUBECOBRA / "batches_tmp"
SHARD_SIZE = 150  # batches per output part file
WORKERS = 16


def fetch(url: str, dest: Path, tries: int = 3) -> None:
    for attempt in range(tries):
        try:
            with requests.get(url, stream=True, timeout=120) as r:
                r.raise_for_status()
                with open(dest, "wb") as f:
                    for chunk in r.iter_content(1 << 20):
                        f.write(chunk)
            return
        except Exception as e:  # noqa: BLE001
            if attempt == tries - 1:
                raise
            print(f"  retry {url}: {e}")
            time.sleep(2 * (attempt + 1))


def process_batch(batch_no: int, index_map: dict, name_map: dict) -> tuple[list, list, int, int]:
    """Download, filter, decode and delete one batch's picks + cubeInstances."""
    picks_url = f"{BUCKET}/picks/{batch_no}.json"
    ci_url = f"{BUCKET}/cubeInstances/{batch_no}.json"
    picks_dest = TMP / f"picks_{batch_no}.json"
    ci_dest = TMP / f"ci_{batch_no}.json"

    fetch(picks_url, picks_dest)
    fetch(ci_url, ci_dest)

    with open(picks_dest, encoding="utf-8") as f:
        picks = json.load(f)
    with open(ci_dest, encoding="utf-8") as f:
        cube_instances = json.load(f)

    picked_bytes = picks_dest.stat().st_size + ci_dest.stat().st_size
    picks_dest.unlink()
    ci_dest.unlink()

    pl_picks = [p for p in picks if p.get("cube") == POWERLSV_CUBE_ID]

    def decode(idx):
        oid = index_map.get(str(idx))
        if oid is None:
            return None, None
        info = name_map.get(oid)
        return oid, (info["name"] if info else None)

    # group by (owner, cubeCards) to assign pick_no_overall deterministically
    from collections import defaultdict

    groups: dict[tuple, list] = defaultdict(list)
    for p in pl_picks:
        groups[(p.get("owner"), p.get("cubeCards"))].append(p)

    pick_rows = []
    referenced_cube_cards: set[int] = set()
    for (owner, cube_cards), recs in groups.items():
        recs_sorted = sorted(recs, key=lambda r: len(r.get("pool", [])))
        draft_id = f"{batch_no}:{owner}:{cube_cards}"
        referenced_cube_cards.add(cube_cards)
        for rec in recs_sorted:
            pool = rec.get("pool", [])
            n = len(pool)  # pick_no_overall, 1-based
            pack_number = (n - 1) // 15 + 1
            pick_in_pack = (n - 1) % 15 + 1
            picked_oid, picked_name = decode(rec.get("picked"))
            pack_oids, pack_names = [], []
            for c in rec.get("pack", []):
                oid, nm = decode(c)
                pack_oids.append(oid)
                pack_names.append(nm)
            pick_rows.append(
                {
                    "draft_id": draft_id,
                    "batch": batch_no,
                    "owner": owner,
                    "cube_cards_index": cube_cards,
                    "pick_no_overall": n,
                    "pack_number": pack_number,
                    "pick_in_pack": pick_in_pack,
                    "picked_oracle_id": picked_oid,
                    "picked_name": picked_name,
                    "pack_size": len(rec.get("pack", [])),
                    "pack_oracle_ids": pack_oids,
                    "pack_names": pack_names,
                    "land_count": rec.get("landCount"),
                    "nonland_count": rec.get("nonlandCount"),
                }
            )

    ci_rows = []
    for cube_cards in sorted(referenced_cube_cards):
        if cube_cards >= len(cube_instances):
            continue
        idxs = cube_instances[cube_cards]
        oids = [decode(i)[0] for i in idxs]
        ci_rows.append({"batch": batch_no, "cube_cards_index": cube_cards, "oracle_ids": oids})

    return pick_rows, ci_rows, len(pl_picks), picked_bytes


def main() -> None:
    check_disk_or_die()
    TMP.mkdir(parents=True, exist_ok=True)
    PICKS_OUT.mkdir(parents=True, exist_ok=True)
    CI_OUT.mkdir(parents=True, exist_ok=True)

    batches = load_json(CUBECOBRA_DIR / "powerlsv_batches.json")
    print(f"{len(batches)} batches to fetch")

    index_map = load_json(RAW_CUBECOBRA / "indexToOracleMap.json")
    name_map = load_json(RAW_CUBECOBRA / "simpleCardDict.json")

    shard_picks: list = []
    shard_ci: list = []
    shard_idx = 0
    total_picks = 0
    total_bytes = 0
    done = 0

    def flush():
        nonlocal shard_picks, shard_ci, shard_idx
        if shard_picks:
            pd.DataFrame(shard_picks).to_parquet(PICKS_OUT / f"part_{shard_idx:04d}.parquet", index=False)
        if shard_ci:
            pd.DataFrame(shard_ci).to_parquet(CI_OUT / f"part_{shard_idx:04d}.parquet", index=False)
        shard_picks, shard_ci = [], []
        shard_idx += 1

    t0 = time.time()
    with cf.ThreadPoolExecutor(max_workers=WORKERS) as ex:
        futs = {ex.submit(process_batch, b, index_map, name_map): b for b in batches}
        for fut in cf.as_completed(futs):
            b = futs[fut]
            try:
                pick_rows, ci_rows, n_picks, nbytes = fut.result()
            except Exception as e:  # noqa: BLE001
                print(f"FAILED batch {b}: {e}")
                continue
            shard_picks.extend(pick_rows)
            shard_ci.extend(ci_rows)
            total_picks += n_picks
            total_bytes += nbytes
            done += 1
            if done % SHARD_SIZE == 0:
                flush()
            if done % 100 == 0:
                elapsed = time.time() - t0
                rate = total_bytes / elapsed / 1e6 if elapsed > 0 else 0
                print(
                    f"[{done}/{len(batches)}] batches done, {total_picks} picks, "
                    f"{total_bytes/1e9:.2f} GB processed, {rate:.1f} MB/s, {elapsed:.0f}s elapsed"
                )
                check_disk_or_die()
    flush()
    print(f"DONE. {done}/{len(batches)} batches, {total_picks} PowerLSV picks, {total_bytes/1e9:.2f} GB raw processed")


if __name__ == "__main__":
    main()
