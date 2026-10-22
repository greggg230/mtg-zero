# Draft data and prior art for a PowerLSV booster-draft AlphaZero bot

Research note, 2026-10-21. Scope: 8-seat booster draft of LSV's PowerLSV cube on CubeCobra
([cube list](https://cubecobra.com/cube/list/PowerLSV), 3 packs of 15). This covers what human data
exists, what earlier draft bots did, and a provisional terminal evaluator for the `zero-core` game.

Method: CubeCobra source on GitHub ([dekkerglen/CubeCobra](https://github.com/dekkerglen/CubeCobra),
Apache-2.0, pushed 2026-10-20), single small fetches against cubecobra.com and its public S3 bucket,
read-only D1 queries against the user's two Cloudflare apps, and web research on prior art. Nothing
was posted, uploaded or signed into. The only things written are this file and scratch files in Git
Bash's temp dir, since deleted.

---

## TL;DR

* **CubeCobra publishes a bulk ML dataset**: `s3://cubecobra-public/export/` (anonymous, refreshed
  about every 3 months; current copy dated **2026-10-19/20**). It has every completed draft from public
  cubes: human decks (`decks/`), the human seat's **pick-by-pick log with the full pack contents**
  (`picks/`), and each draft's card pool (`cubeInstances/`). It is **~125 GB** of uncompressed JSON,
  mostly `picks/` (116 GB). PowerLSV's drafts are spread across the batches, so pulling them out
  means streaming the whole picks set once and filtering on PowerLSV's cube UUID
  `786d3a10-43bd-438e-acf4-bb150b881254`.
* **There are probably about 30k human PowerLSV drafts.** CubeCobra's own per-cube analytics for
  PowerLSV count 1.39M human picks, which is about 31k drafts of 45 picks. The playtest page showed
  97 new PowerLSV drafts in under 10 hours today. Nearly all of them are 1 human + 7 CubeCobra ML
  bots.
* **There are no game results for PowerLSV anywhere**: CubeCobra "Records" for the cube = 0, and the
  export has no win/loss. A deck-strength evaluator therefore can't be fit on PowerLSV win rates
  yet. It has to come from card values implied by picks and deck inclusion, then be replaced by
  play-engine simulations.
* **CubeCobra's current bots are a public 4-part MLP over a 37,238-card vocabulary** (weights in the
  same bucket, `model/`). It is a ready-made baseline for the pick prior and the deckbuilder.
* **Local human data is small but expert**:
  * Rotisserie history: 9 drafts, 3,172 picks, 22 drafters, W-L for 83 player-drafts, and
    per-match results for 5 events.
  * Draft Ranker: 40 PowerLSV runs by 39 people, 13,064 ordering screens, 11 finished top-100s.

  Use both as yardsticks, not as training data.

---

## 1. CubeCobra

### 1.1 Card list endpoints (GET, no auth)

| Endpoint | Returns | Notes |
|---|---|---|
| [`/cube/api/cubeJSON/PowerLSV`](https://cubecobra.com/cube/api/cubeJSON/PowerLSV) | full cube JSON (1.56 MB) | `cards.mainboard` (528 cards today; `version` 87; `numDecks` 1531, which looks stale, see 1.3). Each card has `details` with `oracle_id`, `scryfall_id`, type, cmc, colors, `elo`, `pickCount`, `cubeCount`, `popularity`. **`elo`/`pickCount` here are site-wide, not PowerLSV-specific** (Elo range 1112-2465, median 1414). Already used by `mtg-bot/scripts/fetch_cube.py`. |
| `/cube/api/cubelist/PowerLSV` | plain-text names, one per line | `?date=<ms>` gives a historical list ([source](https://github.com/dekkerglen/CubeCobra/blob/master/packages/server/src/router/routes/cube/api/cubelist.ts)) |
| `/cube/download/{csv,plaintext,mtgo,forge,xmage,cubecobra}/PowerLSV` | list exports | [download.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/server/src/router/routes/cube/download.ts) |
| `/cube/deck/<draftId>` | one draft (HTML + embedded JSON, every seat's picks) | per-deck downloads: `/cube/deck/download/{txt,mtgo,arena,cockatrice,forge,xmage,csv}/<id>/<seat>` ([deck.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/server/src/router/routes/cube/deck.ts)) |
| `/cube/playtest/PowerLSV` | page of recent drafts (unhydrated) | 97 drafts on the first page, all from 2026-10-21 13:18-22:44 UTC; 95 type `d` (draft), 2 `s` (sealed); auto archetype names such as "WR Aggro Draft of PowerLSV" |
| [`/cube/analysis/PowerLSV`](https://cubecobra.com/cube/analysis/PowerLSV) | page with embedded `cubeAnalytics` | **PowerLSV-specific** per-card `elo`, `picks`, `passes`, `mainboards`, `sideboards`, see 1.3 |
| `/cube/records/export/PowerLSV` | JSON of tournament records (W/L, pairings, decks) | **0 records for PowerLSV** (fetched 2026-10-21) ([export.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/server/src/router/routes/cube/records/export.ts)) |

The playtest-data analysis tab (`/cube/analysis/playtestdata/:id`) is a CSRF-protected POST, so it
was not used.

### 1.2 Bulk export: the main dataset

Documented on the [Data Exports Guide](https://cubecobra.com/tool/exports) (source:
[ExportsGuidePage.tsx](https://github.com/dekkerglen/CubeCobra/blob/master/packages/client/src/pages/ExportsGuidePage.tsx)).
Get it with `aws s3 sync s3://cubecobra-public/export/ ./data/ --no-sign-request`, or plain HTTPS GETs on
`https://cubecobra-public.s3.amazonaws.com/<key>`; anonymous bucket listing works. "Data exports are
updated every three months."

**Measured from the bucket listing (2026-10-21):**

| Key | Files | Size | Content |
|---|---|---|---|
| `export/cubes.json` | 1 | 480 MB | every public cube: id, name, owner, `cards` as int indexes |
| `export/indexToOracleMap.json` | 1 | 1.8 MB | int index -> Scryfall oracle id |
| `export/simpleCardDict.json` | 1 | 8.8 MB | oracle id -> {name, image, elo, type, cmc} |
| `export/decks/{n}.json` | 6,599 | 3.78 GB | `{id (draft), cube, owner, mainboard[], sideboard[], basics[]}` |
| `export/picks/{n}.json` | 6,599 | **116.3 GB** | `{cube, cubeCards, owner, pack[], picked, pool[], landCount, nonlandCount}` |
| `export/cubeInstances/{n}.json` | 6,599 | 4.89 GB | per-draft card pool (int indexes) |
| `model/{encoder,draft_decoder,deck_build_decoder,cube_decoder}/` | 20 each | ~77 MB each | TF.js graph models (2026-07-03) |
| `cards/*` | 22 | ~850 MB | full card DB (`carddict.json` 226 MB, `metadatadict.json` 96 MB with per-card global Elo/picks/co-occurrence) |

**How the export is produced**
([export_decks.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/jobs/src/export_decks.ts)).
These details matter when you use it:

* Only **complete drafts of cubes with `visibility === 'pu'`** are included. PowerLSV is `pu`, and
  PowerLSV decks turned up in 5 of the 6 sampled deck batches.
* Batches are DynamoDB pages of up to 2,000 drafts, queried by type in the order `d` (booster
  draft), `g` (grid), `s` (sealed), `u` (uploaded deck). Only `d` drafts get picks. Deck records
  have **no type or date field**; the batch number is the only clue.
* **`decks/`**: one record per seat that has an `owner`, so humans only. CubeCobra bot seats are
  excluded. `mainboard` includes the basics the user added (40-card decks are typical).
  `basics[]` is just the cube's basic-land menu (e.g. 5 ids), **not** the deck's basic counts.
  Many users never build: in the sample, 28 of 160 mainboards held the full 45-card pool.
* **`picks/`**: for every `d` draft with an `InitialState`, one record per pick of **seat 0**. In
  bot drafts that is the human. `pack` is the full pack before the pick (sizes 15 down to 1,
  three packs). `pool` **includes the card just picked**: 99.7% in the sample, and pool sizes run
  1, 2, 3, .... The code has a quirk: it loops over every owned seat but always reads seat 0, so a
  draft with several human seats repeats seat 0's picks under each owner.
* **Draftmancer-published drafts are not in the export.** CubeCobra's
  [draftmancer/publish](https://github.com/dekkerglen/CubeCobra/blob/master/packages/server/src/router/routes/api/draftmancer/publish.ts)
  stores them with `owner: undefined` and `InitialState: undefined`. They exist only as
  `/cube/deck/<id>` pages, which hold every seat's picks.
* No match results, no timestamps, and no user skill signal beyond the owner UUID. You can count
  drafts per owner and weight frequent drafters up.

**Sample fetch** (decks/0, picks/0, cubeInstances/0; 72 KB + 2.7 MB + 0.35 MB):

* `decks/0.json`: 160 drafts, **14 of them PowerLSV**.
* `picks/0.json`: 7,372 picks; most drafts have 45 picks (138 of 160), some 60 (4 packs).
* `cubeInstances/0.json`: a PowerLSV instance had 365 ids (360 dealt cards plus 5 basics).

Five more deck batches (1, 10, 100, 1000, 3000) had 8, 6, 2, 0 and 3 PowerLSV decks. So PowerLSV
appears throughout the export, and there's no cheap way to pick out its batches. The plan:

1. Stream all of `decks/` (3.8 GB) and record which batch numbers contain PowerLSV.
2. Stream only those batches' `picks/` and `cubeInstances/` files, filter on
   `cube == 786d3a10-...`, and keep about 1-2 GB of PowerLSV picks.

`cubeInstances` gives the exact card set of each draft, which handles the cube's version drift
(PowerLSV is at version 87 and its analytics hold 1,873 distinct oracle ids over its life).

Joining picks to decks inside a batch: the `cubeCards` index matches the draft's position in that
batch's list of public drafts. Decks are emitted in the same order, but drafts without an owned
seat emit no deck. So join on `(batch, owner, cube)` plus order, and check that the pool matches
mainboard ∪ sideboard.

**Licence/terms.** The repo is Apache-2.0, but the export has no data licence. The site's
[Terms](https://cubecobra.com/tos) forbid "data mining, data harvesting, data extracting or any
other similar activity in relation to this Website" and commercial use of site material.
[robots.txt](https://cubecobra.com/robots.txt) declares `Content-Signal: ai-train=no` for the website.

The S3 export is the one channel CubeCobra offers on purpose for bulk use; their own recommender is
trained on it. So:

* Use the export, not page scraping.
* Keep the use non-commercial and research-only.
* Credit CubeCobra.
* Consider asking the maintainer (Gwen Dekker) before publishing a model trained on it.

### 1.3 PowerLSV analytics (per cube)

One GET of the analysis page returned `cubeAnalytics` for **1,873 oracle ids**: every card ever in
PowerLSV. Totals: picks 1,391,811; passes 11,239,279; mainboards 1,455,973; sideboards 341,501.

These come from
[update_draft_history.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/jobs/src/update_draft_history.ts):

* Only seat 0 (the human) is counted.
* Card Elo is per cube, with K=10 (`CUBE_ELO_SPEED`), updated as "picked card beats each passed
  card" (standard 400-scale Elo). PowerLSV Elo range: 697-2881.
* Global Elo uses K=1 across all cubes.
* 1.39M picks / 45 is about **31k human PowerLSV drafts**. The cube JSON's `numDecks` (1531) is
  clearly not the draft count.

These analytics can be rebuilt exactly from the export, so the page only serves as a check.

### 1.4 CubeCobra's bots (what the 7 opponents in every human draft were)

From
[recommenderService/src/mlutils/ml.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/recommenderService/src/mlutils/ml.ts)
and the model graphs in `s3://cubecobra-public/model/`:

* **Encoder**: multi-hot of the pool over 37,238 oracle ids -> 512 ReLU -> 256 ReLU -> 128
  (linear bottleneck). About 19.2M parameters.
* **draft_decoder**: 128 -> 256 ReLU -> 512 ReLU -> 37,238 logits.
  * A pick is `softmax(logits masked to the pack)`.
  * **The pack only enters as a mask**: there is no pack context, no pick number and nothing about
    signals.
* **deck_build_decoder**: same shape with a sigmoid output. It scores pool cards for the
  mainboard.
* **cube_decoder**: the same recommender for adding cards to or cutting cards from a cube.
* **Bot deckbuild**
  ([deckbuildCore.ts](https://github.com/dekkerglen/CubeCobra/blob/master/packages/utils/src/drafting/deckbuildCore.ts)):
  1. Seed the first 10 cards from the build model.
  2. Fill the remaining slots greedily: call the draft model with the current mainboard as "pool"
     and the remaining pool cards as "pack", one card per step.
  3. Cap spells at `deckbuildSpells` (23) and lands at `deckbuildLands` (17).
  4. Run a land-trim heuristic and add basics by pip count.

  The repo's
  [deckbuild-eval-harness.md](https://github.com/dekkerglen/CubeCobra/blob/master/packages/docs/deckbuild-eval-harness.md)
  says Moxen sometimes get left out of Vintage-cube decks, which it blames on the policy/search
  layer.
* The training code isn't in the repo. Everything is pure dense layers, so a numpy port is about
  30 lines and lets the bots run inside our own simulator as **baseline opponents and a baseline
  pick prior**.

---

## 2. Prior art (PROVISIONAL: not yet verified)

> A separate web-research pass on prior art was still running when this note was handed in. Apart
> from the CubeCobra material in 1.4, which was checked against source, the items below are from
> background knowledge. **Check the links and numbers before relying on them.**

| Work | Model / inputs | Training target | Takeaway |
|---|---|---|---|
| **CubeCobra bots, current** (verified, 1.4) | pool multi-hot -> MLP autoencoder; pack only as a mask | human picks from CubeCobra drafts (next-pick softmax); mainboard membership | Simple and set-based. It works well enough for 1-human pods. There are no signals or pick-number features, and deckbuild needs heuristics (Moxen left out). |
| **mtgdraftbots** (ruler501; CubeCobra's earlier bots, also used by Draftmancer; unverified) | hand-built "oracles": card rating, pick synergy, internal synergy, colour commitment, openness; weights per pick tuned on human picks | human pick agreement | Interpretable linear-in-features scoring. A good template for the provisional conditional-logit pick model. |
| **Ward et al., "AI solutions for drafting in Magic: the Gathering"** ([arXiv 2009.00655](https://arxiv.org/abs/2009.00655), 2020; Draftsim data) | heuristic, Bayesian, naive-Bayes and deep-NN bots over pack + pool | human pick agreement on M19 Draftsim drafts | The NN beat the heuristic bots on top-1 agreement. Accuracies were in the tens of percent; check the exact figures. |
| **Bertram, Fürnkranz, Müller, contextual preference ranking** (IEEE CoG 2021; unverified link) | Siamese embedding: score(card \| pool context) | pairwise "picked over passed" given the pool | Context-conditioned pairwise preference beat the Draftsim NN. It's the direct ancestor of the 4.2 pick model. |
| **17lands public datasets** ([17lands.com/public_datasets](https://www.17lands.com/public_datasets); licence believed CC BY 4.0) | per-set CSVs: `draft_data` (pack, pool, pick), `game_data` (deck + game outcome), `replay_data` | n/a | The only large source with **deck -> win** labels, but for retail sets, not cube. It shows that mean card quality plus colour/curve sanity predicts deck win rate. Usable to sanity-check the *form* of 4.4, not its values. |
| **Ryan Saxe's draft/deckbuild bots** ([github.com/RyanSaxe/mtg](https://github.com/RyanSaxe/mtg); unverified) | attention/transformer over pack + pool; separate deckbuilder net | 17lands human picks; human decklists | Learned deckbuilders trained on human decks are workable. Deckbuilding is treated as a separate supervised problem. |
| **Draftmancer** ([github.com/Senryoku/Draftmancer](https://github.com/Senryoku/Draftmancer)) | bots from an external mtgdraftbots-style service, with a simple fallback (unverified) | n/a | Publishes all-seat drafts to CubeCobra (verified, 1.2). Those drafts are **not** in the S3 export. |
| **RL drafting in CCGs**, e.g. Legends of Code and Magic arena (Vieira/Chaimowicz/Tavares, 2020+; Kowalski & Miernik; unverified) | RL drafting policies, reward = win rate of the drafted deck from simulated play | game outcomes from a fast simulator | The closest analogue to the long-term plan: draft reward from engine play. Those games are far simpler than MTG. |
| Ramos, "Embarrassingly Causal..." ([arXiv 2604.18314](https://arxiv.org/abs/2604.18314), 2026) | causal reading of 17lands win-rate metrics | n/a | Card win-rate stats are confounded (who drafts a card, and whether it's played) but still usable. The same caution applies to pick-derived values. |

**What generally worked:** supervised imitation of human picks with pool context (colour commitment +
card quality explains most picks). Deckbuilding was handled as a separate, mostly heuristic step.
Nobody has a win-labelled dataset for powered cube.

---

## 3. Local human data (read-only)

### 3.1 Rotisserie history (`mtg-rotisserie-queue`, D1 `rotisserie-queue`)

Schema: [`migrations/018-history.sql`](../../../mtg-rotisserie-queue/migrations/018-history.sql),
`019` (group), `028` (records); logic in `worker/history.ts`. Results of read-only queries on
2026-10-21:

| Table | Rows | Notes |
|---|---|---|
| `history_drafts` | 9 | all group "LSV Roto": Roto 1-3, 8.18, 8.25, 8.31, 9.14, 9.28, 10.5 |
| `history_picks` | 3,172 | 22 drafters (aliases folded), 672 distinct cards; 9-10 seats; 37-40 rounds |
| `history_records` | 83 | per player per draft **match** W-L (sums balance: e.g. 33-33, 44-44) |
| `drafts.announced_matches` | 156 matches | per-pair game scores (`"a\|b": "2-1"`) for Roto 8.31 (31), 9.14 (42), 9.28 (35), 10.5 (35), plus the "Luis vs World" exhibition (13) |

Cube overlap: 452 of PowerLSV's 541 nonbasic cards (84%) appear in roto picks, and 2,512 of the
3,172 roto picks (79%) are PowerLSV cards. The stored roto `main` list (893 cards) has 496 PowerLSV
cards (56% of the roto list, Jaccard 0.53). The `team` list is "PowerLSV Loaded" (547 cards, Jaccard
0.975 with PowerLSV).

**Caveats.**

* The format is different: rotisserie means every card is visible, picks snake, and each player
  drafts about 38 cards from a roughly 890-card cube.
* No packs and no hidden information. Pick position reflects "best available overall" plus
  denial, not pick-from-15.
* 9 events is far too few to fit anything.

**Use**:

* An expert ADP (average pick) yardstick: rank correlation with learned card values on the 452
  shared cards.
* The 156 matches plus decks (rebuildable from the sheets) are a small real-results check on any
  deck evaluator. Do the stronger-rated decks win more than 50%?

### 3.2 Draft Ranker (`cube-top100`, D1 `cuberanker`)

`runs(id, owner_id, pool, cube, depth, top, sorted, ranked, screens, entries JSON)`. Each entry is
one screen: the user puts 2-4 cards in order inside a colour category (W/U/B/R/G/C/M/L), or in a
final cross-category merge (`F`). The engine never shows a pair whose order is already known, so
the screens never contradict each other. Counts (read-only, 2026-10-21):

| | value |
|---|---|
| PowerLSV runs | 40, by 39 Discord users (two by `.lsv`, neither finished) |
| ordering screens | 13,064, which is up to about 6 pairwise preferences each, roughly 50-75k comparisons |
| finished top-100s | 11 (one `full` depth, the rest `half` depth, i.e. the top half of each category sorted) |
| cube versions | v1/v2/v3 category maps (528-card list of 2026-10-13) |

**Use**: a context-free expert value scale.

* Fit Bradley-Terry over all screens. Within-category comparisons are dense and the cross-category
  link comes from the final screens.
* Use it as (a) a yardstick (Spearman/Kendall against the learned values and the CubeCobra cube
  Elo), and (b) an optional prior or regulariser for card values.
* It says nothing about context: colour commitment, synergy, pick number.

### 3.3 Already on disk

`Documents/Claude/Projects/mtg-bot/data/cube/power_lsv_decks.json.gz` (2,000 decks) is **synthetic**.
`scripts/build_cube_deck_pool.py` makes random colour-pair decks for engine self-play; they are not
human decks. `data/17lands/eoe_decks.json.gz` is retail-set (EOE) data.

---

## 4. Recommendation

### 4.1 Where the terminal reward comes from, in stages

There are no PowerLSV win labels. So the provisional evaluator is a **card-value deck model**:
values learned from human picks and deck inclusion, put together with simple deck-structure terms.
Each seat's result comes from a pairwise win model. The evaluator is designed so that **only the
pairwise matchup function changes** when the Rust play engine arrives.

```
pools[8] --deckbuild()--> decks[8] --strength()--> s[8] --matchups--> p[i][j] --> r[8], sum r = 1
```

### 4.2 Card values: fit from the CubeCobra export

The data is the PowerLSV subset of `picks/` (about 31k drafts x 45, so about 1.4M pick decisions),
de-duplicated per the seat-0 quirk, plus the matching `decks/`.

1. **Pick model, contextual conditional logit.** This is the model that worked in prior art
   (Bertram et al.'s contextual preference ranking; mtgdraftbots' rating plus colour terms):

   `P(pick c | pack, pool) ∝ exp( v_c + w·φ(c, pool, pick_no) )`

   * `v_c` is a context-free card value. It is the scalar the evaluator reuses.
   * `φ` holds: on-colour fraction of the pool versus the card's colours/pips; colour count after
     taking c; a pick-number interaction; and optionally a low-rank synergy term `u_c·mean(u_pool)`.

   The first version is about 600 + a few dozen parameters, trained by softmax cross-entropy over
   the pack. Hold out 10% of drafts. Because the pool includes the picked card, use
   `pool \ {picked}` as context.

2. **Deck inclusion.** `m_c = logit P(card c is in the mainboard | c drafted, deck built)`.

   * "Built" means `len(mainboard) ≈ 40` and the mainboard is not the full pool.
   * This captures "people take it but don't play it" (hate picks, speculation).

3. **Value for deck strength**: `val_c = v_c + β·m_c`, on a log-odds scale.

   Cards with no data (new additions) get a simple ridge prior: Draft Ranker BT score, CubeCobra
   global Elo, and type/cmc features.

The pick model is also the **supervised pick prior** for the AlphaZero policy, either as a warm start
or as a KL-regularisation target. The CubeCobra `draft_decoder` (numpy port) is the baseline it has
to beat on held-out top-1 agreement.

### 4.3 Deckbuild step (45 picks -> 40 cards)

Prior work handles this with a separate builder, almost always a heuristic or a separately trained
model, outside the draft RL loop. For the provisional evaluator, use a **deterministic heuristic
inside `result()`**:

1. Enumerate candidate colour sets: the 10 pairs, the 5 mono colours, and pair + splash where
   the splash cards are worth it and fixing is in the pool.
2. For each candidate, take the playables: on-colour cards plus colourless, with splash cards
   allowed only if (pool fixing sources for that colour) >= 2.
3. Pick the best 23 by `val_c`. Use 22-24 spells if 0-cmc mana (Moxen, Lotus, Sol Ring) makes
   up the difference; Vintage-cube decks often run 16 lands plus fast mana.
4. Score `S(colors) = Σ val_c − λ_curve·curve_penalty − λ_short·max(0, 23 − n_playables)`.
5. Keep the best candidate. Lands are on-colour nonbasics first, then fetches/duals, then
   basics in proportion to pips.

Check the builder against humans: Jaccard/F1 of the predicted mainboard versus the human
mainboard on held-out built decks, and against CubeCobra's `deck_build_decoder`. Long term, the
build can become seat decisions in the game (pick colours, then cards, about 25 actions per seat
after pick 45), or a learned builder trained by the engine's results.

### 4.4 Deck strength and per-seat results

* **Deck strength.** `s_i = mean_{c in 23 spells} val_c + γ·colour_penalty_i + δ·synergy_i`.
  Use the mean, not the sum, so 22- and 24-spell decks compare fairly.
  * `colour_penalty_i` counts colours beyond 2, double-pip splash cards, and fixing shortfall.
  * `synergy_i` is optional: mean pairwise PMI of mainboard cards from `decks/` co-occurrence,
    which gives archetype coherence (Reanimator, Sneak/Show, artifacts) a voice.
* **Pairwise matchup.** `p_ij = σ((s_i − s_j)/τ)`.
* **Per-seat result.** Expected share of a round-robin:

  `r_i = Σ_{j≠i} p_ij / C(8,2)`, which sums to 1 exactly because p_ij + p_ji = 1.

  * The range is 0 to 7/28 = 0.25 per seat, with a mean of 1/8.
  * This is lower-variance than "probability of winning the pod". If a sharper target is wanted,
    use Plackett-Luce first place: `r_i = softmax(s/τ')_i`.
  * Either one fits `SearchGame::result() -> [f32; 8]` (`MAX_SEATS = 8` in zero-core).
  * `margin(seat)` can return `s_i − mean(s)`.
* **Calibrating τ.**
  * Start so that a strong deck beats an average one about 60-65% of the time. For scale:
    17lands' top player tier wins about 60% of games and its bottom tier about 48%
    ([SCG article](https://articles.starcitygames.com/select/using-17lands-com-as-a-resource-to-improve-at-limited/)),
    and that spread already includes skill as well as deck quality.
  * Sanity-check on the roto data: build decks from the 4 roto events with match scores
    (156 matches) and check the evaluator's matchup log-loss against a coin flip.
  * When the engine runs, refit `p_ij` directly from simulated matches. Keep `s_i` as a cheap
    feature, or distil engine results into a deck-value network.
* **Known biases to accept for now.**
  * Pick-implied values overrate flexible, high-power cards and underrate narrow synergy pieces.
  * One human seat plays against 7 CubeCobra bots, so the packs reflect bot behaviour.
  * There's no opponent or metagame model.
  * Treat the evaluator as a scaffold, version it, and don't over-train on it. Self-play will
    exploit its blind spots, for example drafting all the "highest-Elo" cards with no mana.

### 4.5 Datasets: what each one is for

| Use | Dataset | Notes |
|---|---|---|
| Supervised pick prior / policy warm start | CubeCobra export, PowerLSV subset (~31k drafts, ~1.4M picks with full pack + pool) | Hold out by draft. Optional pre-training on other Vintage/powered cubes in the export (match by `cubes.json` card overlap), then fine-tune on PowerLSV. |
| Card values for the evaluator | same, plus `decks/` mainboard rates | Rebuild PowerLSV cube Elo from the export as a check against `/cube/analysis`. |
| Deckbuilder supervision and check | PowerLSV `decks/` with ~40-card mainboards | Drop unbuilt (full-pool) mainboards. |
| Baseline bots / opponents | CubeCobra `model/` (encoder + draft/deck decoders) | numpy port; also the "humans' opponents" model, useful for simulating the export's draft conditions. |
| Pick-agreement yardstick | held-out PowerLSV human picks | top-1 / top-3 accuracy and NLL by pick number (P1P1 is hardest; late picks are easy). Report CubeCobra bot, pick model, and AlphaZero policy side by side. |
| Expert value yardstick | Draft Ranker (40 runs, 13k screens, 11 top-100s) | BT fit, then Spearman with `v_c`. A target of ρ > 0.8 on the top 200 is reasonable. |
| Expert pick-position yardstick | Roto history (3,172 picks, 452 shared cards) | Spearman of `v_c` vs mean roto pick position, then look at outliers. |
| Real-results sanity | Roto `announced_matches` (156 matches) + `history_records` (83 W-L) | Tiny; for direction only. |
| Future ground truth | Rust play-engine matches between drafted decks | Replaces `p_ij`. |

### 4.6 Concrete next steps

1. Write `tools/cubecobra_export_filter.py`. It streams `decks/` to index PowerLSV batches, then
   pulls only those `picks/` and `cubeInstances/` files and writes parquet. Budget: about 4 GB
   for the index pass plus whatever share of the 116 GB the matching batches hold. Run it once,
   and keep a checksum and the export date. Next refresh is about 2027-01.
2. Port the CubeCobra MLP to numpy and measure its top-1 on the PowerLSV holdout. That's the
   baseline number.
3. Fit the conditional-logit pick model and `val_c`, then measure against the yardsticks above.
4. Implement `deckbuild()` + `strength()` + the round-robin `result()` in the draft `SearchGame`.
   Unit-test that `result()` sums to 1 and is invariant to seat permutation.
