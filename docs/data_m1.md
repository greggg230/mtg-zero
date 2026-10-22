# M1: PowerLSV draft-data pipeline

Status: done, 2026-10-21. Builds on `docs/research/draft_data.md` and
`docs/research/prior_art.md`. Code: `tools/data/` (see
`tools/data/README.md` for exact rerun commands). Data: `E:\mtg-zero\data\`
(never committed); a small (<1 MB) copy of the card-value table is committed
at `mtg-zero/data/powerlsv_card_values.json`.

## 1. Sources and dates

| Source | What | Date pulled | License |
|---|---|---|---|
| [17lands public datasets](https://www.17lands.com/public_datasets) | `draft_data` + `game_data`, format "Cube - Powered", event types PremierDraft and TradDraft; `cards.csv` | 2026-10-21 (manifest: `E:\mtg-zero\data\raw\17lands\manifest.json`) | CC BY 4.0 |
| CubeCobra bulk export, `s3://cubecobra-public/export/` | `decks/`, `picks/`, `cubeInstances/`, `indexToOracleMap.json`, `simpleCardDict.json` | 2026-10-21 (bucket's own `LastModified`: 2026-10-19/20) | no stated data license; site TOS forbids scraping cubecobra.com pages but the S3 export is the bulk channel the maintainer offers for this purpose -- see `docs/research/draft_data.md` 1.2 for the full caveat. Used read-only, non-commercial, research purpose; credit CubeCobra. |
| `mtg-bot/data/cube_power_lsv.json` | canonical PowerLSV card list (541 nonbasic + 5 basic Scryfall objects) | read-only, not modified | n/a (internal) |

17lands file sizes pulled: draft_data PremierDraft 92.8 MB gz / TradDraft
8.7 MB gz; game_data PremierDraft 26.9 MB gz / TradDraft 2.9 MB gz;
cards.csv 1.7 MB. These match the sizes `prior_art.md` recorded on
2026-10-21, so the files have not moved since that research pass.

## 2. CubeCobra extraction

Per `draft_data.md`'s plan: stream all of `decks/` (3.78 GB, 6,599 batches)
to find which batches contain PowerLSV (`cube == 786d3a10-43bd-438e-acf4-bb150b881254`),
then fetch only those batches' `picks/` and `cubeInstances/`.

* **Index pass** (`02_index_cubecobra_batches.py`): 4,365 of 6,599 batches
  (66%) contain at least one PowerLSV deck. 33,631 PowerLSV deck records
  across **1,644 distinct human drafters**.
* **Filter pass** (`03_filter_cubecobra_picks.py`, 16 parallel workers,
  ~42 min): downloaded and filtered 22.70 GB of raw `picks/`+`cubeInstances/`
  JSON across those 4,365 batches, keeping **1,414,037 PowerLSV picks**
  across **31,505 drafts** (close to `draft_data.md`'s ~31k estimate from
  the site's own analytics). Output: 29 parquet part files each for picks
  and cubeInstances, 53 MB total. Raw batch JSON was deleted immediately
  after each batch was filtered (never more than ~2 batches' raw JSON on
  disk at once); the `decks/` raw sync (3.6 GB) was deleted after step 02
  confirmed it was fully indexed.
* **Why deck count (33,631) > picks-drafts (31,505):** CubeCobra's export
  job queries DynamoDB by draft type in the order `d` (booster draft), `g`
  (grid), `s` (sealed), `u` (uploaded deck), and only `d` drafts get a
  `picks/` record. `decks/` has no such filter -- any owned seat, any draft
  type, goes in. So a PowerLSV sealed/grid/uploaded deck with an owner
  shows up in `decks/` with no matching `picks/` entry. Not a bug.
* **Draft identity in `picks/`:** CubeCobra's pick records carry no draft
  id. We use `draft_id = f"{batch}:{owner}:{cube_cards_index}"` --
  `cube_cards_index` is the field CubeCobra calls `cubeCards`, the
  position of this draft in the batch's `cubeInstances` list. Verified on
  a hand-fetched sample (batch 0): grouping by `(owner, cubeCards)` gives
  exactly 14 groups of 45 picks each, matching the 14 PowerLSV decks found
  in that batch.

## 3. Card table (oracle-id mapping)

`04_build_card_table.py`, output `E:\mtg-zero\data\cards\powerlsv_card_table.parquet`
(1,091 rows) and `card_table_report.json`.

* Canonical list: **546 cards** (541 nonbasic + 5 basic lands), unchanged
  from `mtg-bot/data/cube_power_lsv.json`.
* **CubeCobra overlap: all 546 canonical cards (100%) have appeared in the
  PowerLSV draft history** (decks, picks or cubeInstances). CubeCobra is
  oracle-id native, so this join is exact, no name matching needed.
* **545 additional oracle ids** appear in the CubeCobra PowerLSV history
  but are **not** in the current 546-card list -- real cube version drift
  (PowerLSV is versioned; its own analytics track 1,873 distinct oracle ids
  over its life per `draft_data.md` 1.3). Kept, flagged
  `in_current_cube = False`. Examples: Veil of Summer, Utopia Sprawl,
  Reclamation Sage, Nethergoyf. (Craterhoof Behemoth and Tarmogoyf, which
  come up below as 17lands-only names, are a *different* case -- they
  never appear in CubeCobra's PowerLSV history either, at any version; see
  the next bullet.) Sample of 40 in `card_table_report.json`.
* **17lands overlap is a genuinely different story, not version drift.**
  17lands' "Cube - Powered" format (Magic Arena's own curated cube) has a
  **545-card vocabulary that only partially matches PowerLSV**: 391 of 545
  names (71.7%) match a canonical card (380 exact name match, 11 via
  front-face name for transform/MDFC/adventure cards), leaving **154
  17lands names that never appear anywhere in PowerLSV's history at all**
  -- Tarmogoyf, Craterhoof Behemoth, Cryptic Command, Liliana of the Veil,
  Scavenging Ooze, all five "Talisman of X" cards, all the "Restless"/
  "Verge" dual-land cycles, etc. Confirmed by direct search, not a matching
  bug: these cards are absent from the 546-card canonical list under any
  name. Treat 17lands "Cube - Powered" as a different (if closely related)
  powered-cube card pool, and only use the matched 391 names for anything
  PowerLSV-specific. Conversely, 155 canonical cards (155/546, mostly
  newer additions) don't appear in 17lands' vocabulary at all.

## 4. Card values

`05_card_values.py`, output `E:\mtg-zero\data\cards\powerlsv_card_values.json`
(1,091 cards, full detail) and the committed `mtg-zero/data/powerlsv_card_values.json`
(columnar `{columns, rows}` format to stay under 1 MB -- 0.24 MB for 1,091
cards x 24 columns).

**Pick strength (CubeCobra).** Fit a multiway Bradley-Terry/Luce model
(`tools/data/bt_luce.py`) over the pack contents of all 1,414,037 picks:
each pick is one "N-way comparison" (the pack, size 1-15) with exactly one
winner. This is the MLE Hunter (2004) MM algorithm generalizes Zermelo's
pairwise Elo-style fit to a full pack at once, rather than decomposing a
15-way pick into 14 separate pairwise Elo updates (which depends on update
order the way online Elo does -- see `draft_data.md` 4.2's "contextual
conditional logit" framing; this is its context-free term, `v_c`). Chosen
over average-pick-position because it falls out of the same likelihood used
for the eventual contextual model, and it handles variable pack
size/composition (pick 1 of 15 vs. pick 15 of 15 vs. pick 1 of a pack with
some already-missing cards) without an ad hoc correction term. Ridge
regularization (0.5 pseudo-win/loss against a fixed-strength "ghost"
opponent) keeps thinly-observed cards finite; `n_picked`/`n_seen` are
reported per card so thin estimates are visible (canonical nonbasics:
median 2,294 picks, min 154, max 4,336; the 5 basics are never drafted from
a pack, so they have no pick-strength value by construction). Validated on
synthetic 4-item data before use (recovered true strengths to within
~1-2% -- see `tools/data/bt_luce.py` docstring/test). Top cards by fitted
strength: Black Lotus, Time Walk, Mox Sapphire, Sol Ring, Ancestral Recall,
Gleemox, the remaining Moxen, Mana Crypt -- exactly the expected powered-cube
top tier, a good sanity check.

**Main-deck rate (CubeCobra).** `P(card in mainboard | card in pool, deck
"built")`. "Built" = `len(mainboard) + len(sideboard) == 45` (the full
drafted pool, not a duplicated/looped record) **and** `len(sideboard) > 0`
(the drafter moved at least one card out, as opposed to leaving the whole
pool as "mainboard" and never building). 13,674 of 33,631 PowerLSV decks
(40.7%) are "built" by this definition; the rest are mostly untouched
45-card pools (`sideboard` empty) or export artifacts with `mainboard +
sideboard` equal to a multiple of 45 (duplicated records, ~29% of decks --
excluded, see `tools/data/README.md`).

**Main-deck rate (17lands).** The dataset's own `pick_maindeck_rate`
column, deduplicated per card name, reported separately for PremierDraft
and TradDraft (391 matched cards only).

**Win rates (17lands `game_data`).** Per-card win rate when the game's
deck contains >=1 copy (`deck_<name> >= 1`), with sample size; the same
restricted to "experienced" users (`user_n_games_bucket >= 50`) as a light
confound-reduction; plus the unconditional baseline win rate for reference
(PremierDraft 54.5% over 266,117 games; TradDraft 58.4% over 28,858 games --
TradDraft is best-of-3 and skews toward more deliberate play, which is
likely most of that 4-point gap). **These are explicitly not causal.**
Per `prior_art.md`'s citation of Ramos, "Embarrassingly Causal" (arXiv
2604.18314): both *who drafts* a card and *whether a drafted card makes the
final deck* are selection effects, so raw in-deck win rate conflates card
quality with drafter skill, archetype fit and deck context. Treated here as
a confounded signal to report alongside pick strength and main-deck rate,
not as ground truth for card value -- consistent with `draft_data.md` 4.2's
plan to source deck strength primarily from pick-implied value, not win
rate, until the Rust play engine (Track C) can simulate real outcomes.

Only the 391 canon-matched 17lands card names got win-rate columns (not all
545): `game_data`'s CSV has 2,745 columns (`deck_`/`sideboard_`/
`opening_hand_`/`drawn_`/`tutored_` x 545 cards + metadata); restricting
`usecols` to `deck_<matched-name>` + `won` + `user_n_games_bucket` kept the
read under 1 GB instead of needing chunked reads of the full file.

## 5. Held-out pick split

`06_holdout_split.py`, seed 42, split 90/10 **by draft** (never by row).

| | drafts | picks | train drafts | test drafts |
|---|---:|---:|---:|---:|
| CubeCobra | 31,505 | 1,414,037 | 28,355 (1,272,670 picks) | 3,150 (141,367 picks) |
| 17lands PremierDraft | 48,152 | 2,118,673 | -- | -- |
| 17lands TradDraft | 4,500 | 197,999 | -- | -- |
| 17lands combined | 52,652 | 2,316,672 | 47,387 | 5,265 |

73.4% (PremierDraft) / 74.4% (TradDraft) of 17lands picks are for a
canon-matched card; the rest are cards that exist in Arena's "Cube" pool but
not in PowerLSV (section 3) and can't be scored against PowerLSV card
values.

Output: `E:\mtg-zero\data\splits\{cubecobra,lands17}_picks_{train,test}.parquet`.
The 17lands parquet does **not** carry all 545 pack_card_/pool_ one-hot
columns (that would be ~1,245 columns x 2.3M rows); instead each row has the
true pack/pool *sizes* (for a correct random baseline and colour-share
denominator), the list of canon-matched cards present in the pack, and
per-colour pool totals (WUBRG) -- everything the baselines in section 6
need, without materializing the full one-hot matrix.

## 6. Baselines (the numbers the draft AI must beat)

`07_baselines.py`. Rating = log(Bradley-Terry strength), **refit on the
CubeCobra TRAIN split only** (not the published card-value table, which is
fit on the full corpus) so the yardstick is a clean held-out comparison;
min-max normalized to [0,1]. Colour-commitment term (Draftmancer SimpleBot
style): `score = rating + lambda * colour_share`, `colour_share` = fraction
of the drafter's pool-so-far sharing >=1 colour with the candidate
(colourless candidates get `colour_share = 1`). `lambda` grid-searched on a
6,000-draft TRAIN sample (0.0 to 2.0, step 0.1), by top-1 agreement.

### CubeCobra-native (test: 141,367 picks, 3,150 drafts)

| Baseline | top-1 | top-3 |
|---|---:|---:|
| (a) random | 21.9% | 49.4% |
| (b) greedy by pick strength (lambda = 0) | **42.3%** | **74.7%** |
| (c) pick strength + colour commitment (tuned lambda) | 42.3% | 74.7% |

**The tuned lambda is 0.0** -- the grid search's own curve
(`baselines_report.json`, `lambda_grid_on_train_sample`) is monotonically
decreasing from lambda=0 (top-1 42.5%) to lambda=2.0 (top-1 33.1%), so (c)
collapses to (b) here. Pure pick-strength already captures most of what
this simple colour term would add, and the additive formulation actively
hurts: `colour_share` grows toward 1.0 as the pool fills up (by pick ~20+ it
is often >0.7), while `rating` differences between the top few cards in a
pack are usually small on the normalized [0,1] scale, so any lambda above
~0.1 lets "more of my colour" dominate over "the objectively stronger
card" -- exactly the failure mode `draft_data.md` 4.4 warned the self-play
loop could exploit ("drafting all the highest-value cards with no mana" is
the opposite error, but this shows the additive form is sensitive to scale
either direction). This is a real finding, not a bug: Draftmancer's own
SimpleBot lambda (0.35) is calibrated against its own card-rating scale, not
this one. Worth revisiting in M2 with a better-scaled or multiplicative
colour term; for now, (b) and (c) are numerically identical as reported.

### 17lands cross-dataset check (test: 170,062 scoreable of 231,658 picks)

Same CubeCobra-trained rating, scored on 17lands "Cube - Powered" test
picks restricted to picks where the human's actual choice is canon-matched
and candidates are canon-matched (see section 5). A generalization check
across a different (if related) cube, not the primary yardstick -- expected
to be noisier since ~26% of each pack's true alternatives are invisible to
this rating.

| Baseline | top-1 | top-3 |
|---|---:|---:|
| (a) random (true pack size) | 19.1% | 45.7% |
| (b) greedy by pick strength | 44.5% | 78.2% |

(Slightly *higher* than the CubeCobra-native numbers, likely because
restricting to canon-matched picks/candidates removes the narrowest/most
idiosyncratic cards from both sides of the comparison, not because the
cross-cube transfer is somehow better.)

**Context for both:** CubeCobraML (the site's own pool-only multi-hot MLP,
`prior_art.md`) reports ~50% top-1 / ~83% top-3 on cube picks generally and
calls that range "data/capacity-bound" for this kind of pool-only model.
Our zero-context Bradley-Terry pick strength -- no pool interaction, no
pick-number term, nothing except "which card is stronger" -- lands at
42-44% top-1 / 75-78% top-3, roughly 2x random and a believable distance
below a model that also sees the drafter's pool. That gap (and closing it)
is squarely the contextual term `draft_data.md` 4.2 describes for M2.

## 7. File locations

| What | Path |
|---|---|
| Pipeline code | `tools/data/01_download_17lands.py` ... `07_baselines.py`, `bt_luce.py`, `common.py`, `README.md` |
| This doc | `docs/data_m1.md` |
| 17lands raw (kept; not "big") | `E:\mtg-zero\data\raw\17lands\` (+ `manifest.json`) |
| CubeCobra lookups (kept) | `E:\mtg-zero\data\raw\cubecobra\{indexToOracleMap,simpleCardDict}.json` |
| CubeCobra filtered decks | `E:\mtg-zero\data\cubecobra\powerlsv_decks.parquet`, `powerlsv_batches.json` |
| CubeCobra filtered picks/cubeInstances | `E:\mtg-zero\data\cubecobra\{picks,cube_instances}\part_*.parquet` |
| Card table | `E:\mtg-zero\data\cards\powerlsv_card_table.parquet`, `card_table_report.json` |
| Card values (full) | `E:\mtg-zero\data\cards\powerlsv_card_values.json` |
| Card values (committed, <1 MB) | `mtg-zero\data\powerlsv_card_values.json` |
| Held-out splits | `E:\mtg-zero\data\splits\{cubecobra,lands17}_picks_{train,test}.parquet` |
| Baseline report | `E:\mtg-zero\data\cards\baselines_report.json` |

E: free space: 322 GB after this milestone (started at 356 GB; peak raw
usage during CubeCobra filtering was ~23 GB transient, never close to the
50 GB floor).

## 8. Surprises / things to revisit in M2

1. 17lands "Cube - Powered" is a meaningfully different card pool from
   PowerLSV (28% non-overlap), not just a version-drifted snapshot -- its
   win-rate/main-deck-rate numbers only cover the 391 shared cards.
2. The additive colour-commitment term made top-1 agreement *worse* at
   every tested lambda > 0.1 on CubeCobra; see section 6. Don't carry
   lambda=0.35 over from Draftmancer without recalibrating the rating
   scale, and consider a multiplicative or capped colour term for M2.
3. Only 40.7% of CubeCobra PowerLSV decks are "built" (rest are untouched
   pools or export-artifact duplicates) -- main-deck rate is computed on a
   smaller, specific subset; see `tools/data/README.md` for the exact
   filter.
4. All 546 current-cube cards have CubeCobra pick history (good -- no
   cold-start cards), but 545 *additional* historical oracle ids show up in
   the same data from past cube versions; they're kept and flagged rather
   than dropped, per the task's own instruction.

## 9. How to rerun

See `tools/data/README.md` for the full step-by-step (setup, each script's
purpose/dependencies, and the `aws s3 sync` command for `decks/` that isn't
wrapped in a script). Rough order: `01` -> sync `decks/` + fetch the two
CubeCobra lookup JSONs -> `02` -> `03` -> `04` -> `05` -> `06` -> `07`.
Total wall time on this machine: ~5 min (17lands download) + a few minutes
(decks sync, 3.8 GB) + ~1 min (index pass) + ~42 min (picks/cubeInstances
filter, 16 workers) + a few minutes each for card values / splits /
baselines.
