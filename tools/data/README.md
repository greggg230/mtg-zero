# M1 draft-data pipeline

Builds the PowerLSV human-pick dataset: card values, main-deck rates, win
rates, a held-out pick split, and baseline yardsticks. See
`docs/data_m1.md` for the results; this file is about rerunning the steps.

## Setup

```
cd C:\Users\Gregg Keithley\mtg-zero
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install pandas pyarrow numpy requests scipy
```

All scripts are run with `.\.venv\Scripts\python.exe tools\data\<script>.py`
from the repo root (or `"./.venv/Scripts/python.exe"` from Git Bash). They
assume `E:\mtg-zero\data\` exists (created automatically by
`common.ensure_dirs()`) and that E: has enough free space; every
long-running script calls `common.check_disk_or_die()` and stops if E: has
less than 50 GB free.

## Steps (run in order)

| # | Script | What it does | Depends on |
|---|---|---|---|
| 1 | `01_download_17lands.py` | Downloads 17lands "Cube - Powered" draft_data/game_data (Premier+Trad) + cards.csv. Writes a manifest with URLs/dates/sha256. | network |
| 2 | `02_index_cubecobra_batches.py` | Scans the locally-synced `decks/` export (see below) for PowerLSV (`cube == 786d3a10-...`) records; writes `powerlsv_decks.parquet` and the list of matching batch numbers. | `decks/` synced locally |
| 3 | `03_filter_cubecobra_picks.py` | Downloads *only* the matching batches' `picks/` + `cubeInstances/`, decodes card indexes to oracle ids/names, writes parquet part files, deletes each batch's raw JSON immediately after filtering. | step 2, `indexToOracleMap.json` + `simpleCardDict.json` |
| 4 | `04_build_card_table.py` | Builds the canonical card table: PowerLSV's 546-card list + CubeCobra oracle ids seen in the data (flagging any not in the current cube) + a name-matched 17lands column. | steps 2-3 |
| 5 | `05_card_values.py` | Per-card pick strength (Bradley-Terry/Luce over CubeCobra pack contents), CubeCobra main-deck rate, 17lands main-deck rate, 17lands in-deck win rates. Writes the full table to E: and a <1MB committed copy to `mtg-zero/data/`. | step 4 |
| 6 | `06_holdout_split.py` | Splits CubeCobra and 17lands picks by DRAFT (seed 42, 90/10) into train/test parquet. | steps 1, 3 |
| 7 | `07_baselines.py` | Random / greedy-by-pick-strength / pick-strength+colour-commitment baselines on the held-out test sets (lambda for the colour term is grid-searched on train only). | steps 4, 6 |

Before step 2, sync the CubeCobra `decks/` export locally (3.8 GB; this is
the one piece not wrapped in a script, since `aws s3 sync` is already a
one-liner and safe to resume):

```
"/c/Program Files/Amazon/AWSCLIV2/aws.exe" s3 sync s3://cubecobra-public/export/decks/ \
    "E:\mtg-zero\data\raw\cubecobra\decks\" --no-sign-request
```

Also fetch the two small CubeCobra lookup files once, if not already present:

```
curl -s -o "E:\mtg-zero\data\raw\cubecobra\indexToOracleMap.json" https://cubecobra-public.s3.amazonaws.com/export/indexToOracleMap.json
curl -s -o "E:\mtg-zero\data\raw\cubecobra\simpleCardDict.json" https://cubecobra-public.s3.amazonaws.com/export/simpleCardDict.json
```

## Notes / gotchas (read before rerunning)

* **CubeCobra picks/ have no draft id.** We define
  `draft_id = f"{batch}:{owner}:{cube_cards_index}"` -- verified on a
  hand-fetched sample that grouping by (owner, cubeCards) within one batch
  recovers exactly the drafts' pick sequences (45 picks each). `pick_no_overall`
  is `len(pool)` at the time of the pick (the record's `pool` field includes
  the just-picked card).
* **"Built" CubeCobra decks** = `len(mainboard) + len(sideboard) == 45`
  (the full drafted pool, no duplicated/looped records) **and**
  `len(sideboard) > 0` (the user actually moved something out, as opposed to
  dumping the whole pool into "mainboard" and never building). About 41% of
  PowerLSV decks are "built" by this definition; see `docs/data_m1.md`.
* **17lands "Cube - Powered" is a different card pool from PowerLSV**, not
  just a version-drifted snapshot of it (confirmed: staples like Tarmogoyf,
  Craterhoof Behemoth, Cryptic Command are in 17lands' Arena Cube vocabulary
  but never appear anywhere in the PowerLSV canonical list). Only the name
  overlap (~72% of the 545-card 17lands vocabulary) is used; see
  `card_table_report.json`.
* **17lands game_data is read with `usecols` restricted to the ~391
  canon-matched `deck_<name>` columns**, not all 545+2,180 (deck/sideboard/
  opening_hand/drawn/tutored) columns -- that keeps the win-rate pass to
  <1 GB RAM instead of needing chunked reads of the full 2,745-column file.
* **Re-running step 3 from scratch is safe** (idempotent: downloads, filters,
  deletes). It takes ~15-25 minutes for ~4,365 batches at 16 parallel
  workers; it checks free space on E: every 100 batches.
* **Card-value "pick strength"** (in `powerlsv_card_values.json`) is fit on
  the FULL CubeCobra pick corpus. The baseline report (step 7) refits the
  same Bradley-Terry/Luce model on the TRAIN split only, so the "number the
  draft AI must beat" is a clean held-out comparison; the two `bt_strength`
  numbers will differ slightly for that reason.
