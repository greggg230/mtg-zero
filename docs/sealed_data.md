# Sealed data: the 17lands EoE yardstick and the Play Booster collation (S2 groundwork)

Branch `eoe-s2-data` (from `eoe-base` 9cdda51), 2026-11-08. DESIGN.md 9, milestone S2. Code:
`mtgz/sealed/` (pure Python, no engine module), `tools/sealed_eoe_set.py`, `tools/sealed_measure.py`;
tests `tests/test_sealed_17lands.py`, `tests/test_sealed_collation.py`; data `data/sealed/`.

**The rule (DESIGN.md 0).** The 17lands data is a **yardstick only**: it measures (a pool generator's
statistics against what Arena opened; later our builds against the human builds of the same pools),
and is never a training target, reward or prior. Nothing in `mtgz/sealed/collation.py` was fitted to
it: its rates are Wizards' published ones. Section 4 reports where Arena differs; whether to model
those differences is the user's call (section 5).

## 1. The 17lands EoE sealed yardstick (`data/sealed/eoe_17lands_sealed.json.gz`, 3.1 MB)

**Sources** (17lands public data, CC BY 4.0, <https://www.17lands.com/public_datasets>; downloaded
2026-11-08, not committed; URLs, sizes and sha256 are in the file's `sources`):

| File | Rows (games) | Pools | Builds | Draft times |
|---|---|---|---|---|
| [`game_data_public.EOE.Sealed.csv.gz`](https://17lands-public.s3.amazonaws.com/analysis_data/game_data/game_data_public.EOE.Sealed.csv.gz) (Premier Sealed, best-of-one) | 56,256 | 9,765 | 13,787 | 2025-07-29 .. 08-22 |
| [`game_data_public.EOE.TradSealed.csv.gz`](https://17lands-public.s3.amazonaws.com/analysis_data/game_data/game_data_public.EOE.TradSealed.csv.gz) (Traditional Sealed, best-of-three) | 7,773 | 903 | 2,887 | 2025-07-29 .. 08-13 |

One CSV row is one game of one build of one pool (`draft_id`): 20 metadata columns and, per card
(321: the set's 316 nonbasic cards and the 5 basics), `deck_` / `sideboard_` / `opening_hand_` /
`drawn_` / `tutored_` counts.

**Contents.** Per pool: id, draft time, the user's games / win-rate buckets, the pool (nonbasic
cards with counts); per build: `build_index`, the deck (with basics), the colours 17lands assigned,
and its games: match and game number, won, on the play, mulligans, the opponent's colours and
mulligans, turns, ranks, and the cards in the opening hand, drawn and tutored (for 17lands-style
drawn-card win rates, the S5 card-rating yardstick). Cards are indices into one name table. The
sideboard is not stored: it is the pool minus the deck's nonbasic cards (checked when built), so
the file is a third smaller.

**Loader** (`mtgz.sealed.seventeen_lands`): `load('Sealed' | 'TradSealed')` -> `Pool` (`cards`,
`builds`, `size`, `wins`, `losses`, `match_record()`, `incomplete`, `not_in_ir()`,
`unsupported()`), `Build` (`deck`, `sideboard`, `short`, `games`, ...), `Game`; `report()`;
`build()` rebuilds the file from the CSVs (`python -m mtgz.sealed.seventeen_lands build --sealed
S.csv.gz --trad T.csv.gz`; a fixed gzip mtime: the same input gives the same bytes).

**What the rows showed.**

* **A pool is 78 nonbasic cards.** deck + sideboard minus basics is 78 in 9,753 of 9,765 Premier pools
  and all 903 Traditional ones. **Basics are counted only in decks** (9-23 per deck, 15-17 in 90%),
  never in a sideboard: Arena's basics are free, so the 91-96 "deck + sideboard" totals are 78 +
  the deck's basics.
* **Launch-day rows miss a Special Guest.** 19 pools (all 2025-07-29/30) have rows with 77 cards: 7
  pools later show the 78th card (Burgeoning 5, Magus of the Moon 2), so 17lands' tracker did not
  yet know the Special Guests; 12 pools only ever show 77. The pool is the per-card maximum over its
  rows, the build that missed the card lists it in `Build.short`, and `Pool.incomplete` flags all 19.
  Those 7 pools' build indices each split into two `Build`s (13,780 build indices, 13,787 builds).
* **Traditional Sealed rebuilds between games.** Sideboarding makes a new `build_index`, so a match
  can span builds (2,887 builds in 903 pools); `Pool.match_record()` groups the games by match.
* **Records:** Premier Sealed ends at 7 wins or 3 losses (5 pools exceed it in 17lands' rows);
  Traditional at 4 match wins or 2 losses (all within). Game win rate 57.0% (Premier) and 60.2%
  (Traditional): 17lands users are better than the field.
* **Decks:** 40 cards in 90.5% of builds, 41 in 7.8%, up to 60.
* **Names:** every 17lands name is the set's name (`cards.map_name`: exact, else a split / double-
  faced front face, else a loose ASCII key; none needed). The one card outside the engine's table is
  **Paradox Haze** (a Special Guest with no IR): in 87 Premier and 17 Traditional pools.
* **Engine support** (`Pool.unsupported()`, read live from `engine/tests/support.rs`'s
  `SUPPORTED_EOE_*` lists, so it follows the checkout): at `eoe-base` 9cdda51, 295 of the table's 320
  cards (the workstreams of ENGINE.md 99.11-99.14 still open): only **794 of 9,765 Premier pools (8.1%) and 53 of 903 Traditional
  are fully supported**; 6,445 of 13,787 Premier builds put an unsupported card in the deck. The
  commons Gene Pollinator (4,404 pools), Unravel, Steelswarm Operator and Tractor Beam (~2,050 each)
  dominate. Pools are flagged, never dropped.

## 2. What an EoE sealed pool is, and the card list (`data/sealed/eoe_set.json`)

**Sealed is six Play Boosters, with basic lands added freely** ([Sealed Deck
format](https://magic.wizards.com/en/formats/sealed-deck): "six unopened booster packs", "as many
basic lands ... as you'd like", 40-card minimum). The paper **Prerelease Pack** is "6 Edge of
Eternities Play Boosters" plus one traditional foil year-stamped rare or mythic promo ([Where and How
to Play Edge of Eternities](https://magic.wizards.com/en/news/feature/where-and-how-to-play-edge-of-eternities)),
so a prerelease pool is 79 nonbasic cards; Arena has no promo. A Play Booster has 14 cards, one a
basic land in EoE's land slot (section 3), so **six boosters hold 78 nonbasic cards**: exactly the
17lands pools. Arena uses Play Boosters for Limited since Murders at Karlov Manor ([Play Boosters on
MTG Arena](https://magic.wizards.com/news/mtg-arena/play-boosters-on-mtg-arena), 2023-10-16); its
boosters drop the land slot (basics are free: 17lands' 6,018 fresh draft packs, section 4, are all 13
cards with no basic) and have no foils (the foil slot is a nonfoil card of any rarity). So the
nonbasic contents of paper and Arena sealed have the same structure.

`tools/sealed_eoe_set.py` builds the card list from mtg-bot's Scryfall dumps
(`edge_of_eternities.json`, `stellar_sights.json`, `special_guests_eoe.json`, read-only; sha256s in
the file) and one Scryfall search for the booster-fun printings EOE 277-316. 321 cards: the main set
`eoe` (81 commons, 100 uncommons, 60 rares, 20 mythics = 261), Stellar Sights `eos` (30 rares, 15
mythics), the Special Guests `spg` (10 mythics, SPG 119-128), the 5 basics; per card its rarity,
collector number, colours, colour identity, mana value, type line and `in_ir`. Booster-fun printings:
viewport lands 277-286 (the five shocks, rare; the five planets, mythic), triumphant 287-302 (12
rares, 4 mythics), surreal space 303-316 (11 rares, 3 mythics; Starwinder is in both groups). The
commons are 15 per colour plus 6 colourless (no multicolour common).

## 3. The Play Booster collation and the generator (`mtgz/sealed/collation.py`)

**Spec** (Wizards, [Collecting Edge of Eternities](https://magic.wizards.com/en/news/feature/collecting-edge-of-eternities),
2025-07; the same figures are transcribed in mtg.wtf's
[`data/boosters/eoe/play.yaml`](https://github.com/taw/magic-search-engine/blob/master/data/boosters/eoe/play.yaml)).
14 cards and a double-sided token (no ad or art card):

| Slot | Contents (rates per booster) |
|---|---|
| Commons (7) | 7 of the 81 main-set commons; in **1.8%** of boosters a non-foil Special Guest (one of 10) replaces one (never in the wildcard or foil slot) |
| Uncommons (3) | 3 of the 100 main-set uncommons |
| Wildcard (1) | main set C 12.5%, U 62.5%, R 10.6%, M <1%; **Stellar Sights R 10%, M 2.5%** ("a land from the Stellar Sights bonus sheet in 1 out of 8 Play Boosters"); viewport land R 1%, M <1%; triumphant or surreal R <1%, M <1% |
| Rare (1) | main set R 80.4%, M 14.2%; triumphant R 2%, surreal R 2%; triumphant / surreal / viewport M <1% each |
| Traditional foil (1) | main set C 58%, U 32%, R 6.4%, M 1.1%; Stellar Sights R 1%, M <1%; viewport / triumphant / surreal R <1%, M <1% |
| Land (1) | basic land: default frame 80% (foil 16 of it), borderless celestial 20% (foil 4) |

**Assumptions** (Wizards gives no figure): each "<1%" entry of a slot gets an equal share of what the
slot's printed rates leave: wildcard 0.225% each (as mtg.wtf), rare slot 0.467 / 0.467 / 0.466%,
foil 0.5% each. Within a group every printing is equally likely; within a rarity of a sheet every
card. Rates are integers per 100,000 (`WILDCARD`, `RARE`, `FOIL`, `SPECIAL_GUEST`).

**Generator.** `Collation(booster_fun=False).booster(rng)` -> 13 (slot, name) pairs (14 with
`land_slot=True`); `sealed_pool(seed)` -> the 78 sorted names of six boosters. Randomness is
splitmix64 with integer-only sampling (rejection for uniform ints, partial Fisher-Yates for the
distinct commons / uncommons, cumulative integer weights for slots): a Rust port can reproduce a
pool bit for bit (`test_the_generator_is_pinned`). Draw order per booster: the guest roll, the
commons, the guest card, the uncommons, the wildcard, rare and foil slots (a category roll, then a
printing), the land. `booster_fun=True` weights names by the paper booster-fun printings; the
default folds those rates into the main-set sheet of their rarity, as Arena shows (4.2; mtg.wtf's
`play-arena.yaml` does the same). A Special Guest without IR (Paradox Haze, ~1.1% of pools) is
emitted like any card: `in_table(name)` tells the engine's table, and nothing is dropped.

## 4. Measurements: the generator against what Arena opened (`tools/sealed_measure.py`)

17lands pools: the 9,746 complete Premier and 902 Traditional pools. Arena packs: the fresh packs
(pick 0 of each pack) of the first 2,006 drafts of
[`draft_data_public.EOE.PremierDraft.csv.gz`](https://17lands-public.s3.amazonaws.com/analysis_data/draft_data/draft_data_public.EOE.PremierDraft.csv.gz)
(its first 20 MB of 142 MB, a byte-range download), 6,018 packs (5,998 of 13 cards; 20 launch-day
packs of 12, the unseen Special Guests), committed as `data/sealed/eoe_17lands_draft_packs.json.gz`
(178 KB; `load_draft_packs()`). Generator: seeds 0-19,999 (pools) and the 19,998 boosters of seeds
0-3,332 (packs). Means per pool with standard errors (sd in brackets):

| Per pool | 17lands Premier | 17lands Trad | Generator | Generator, paper booster-fun |
|---|---|---|---|---|
| main commons | 46.11 ±0.02 | 46.01 ±0.05 | 46.13 ±0.01 | = |
| main uncommons | 23.67 ±0.02 | 23.69 ±0.05 | 23.67 ±0.01 | = |
| main rares | 6.15 ±0.01 | 6.21 ±0.05 | 6.18 ±0.01 | = |
| main mythics | 1.044 ±0.009 | 1.04 ±0.03 | 1.077 ±0.007 | = |
| Stellar Sights rares | **0.729** ±0.008 | 0.76 ±0.03 | **0.660** ±0.005 | = |
| Stellar Sights mythics | 0.179 ±0.004 | 0.17 ±0.01 | 0.173 ±0.003 | = |
| Special Guests | 0.113 ±0.003 | 0.12 ±0.01 | 0.109 ±0.002 | = |
| pools with a Stellar Sights land | **62.2%** | 63.2% | **59.1%** | = |
| pools with Paradox Haze | 0.89% | 1.9% | 1.07% | = |
| commons of one colour (W) | 8.52 (**sd 1.22**) | 8.42 (sd 1.23) | 8.54 (**sd 2.57**) | = |
| cards of one colour (W) | 15.13 (**sd 2.03**) | 15.01 (sd 2.11) | 15.14 (**sd 3.43**) | = |
| uncommons of one colour (W) | 4.04 (**sd 1.35**) | 3.96 (sd 1.33) | 4.03 (**sd 1.85**) | = |
| distinct cards | 66.00 | 65.97 | 65.43 | = |
| main rares with a booster-fun printing / others, per card | 1.00 | 1.02 | 1.01 | **1.13** |
| same for mythics | 0.97 | 0.96 | 1.01 | **1.27** |
| commons' chi-square against uniform (df 80) | 71.3 | 74.3 | 67.5 | 67.5 |

| Per pack | 17lands fresh packs | Generator |
|---|---|---|
| commons / uncommons / main rares / main mythics | 7.687 / 3.940 / 1.033 / 0.169 | 7.679 / 3.948 / 1.035 / 0.179 |
| Stellar Sights | **0.157** ±0.005 | **0.140** ±0.003 |
| Special Guests | 1.45% (1.78% with the 12-card packs) | 1.8% |
| every colour has 1 or 2 commons | **100%** | **15.4%** |
| colours with a common | 5.00 | 4.02 |
| colourless commons | 0.561 | 0.565 |
| no colour with 2+ uncommons (multicolour cards count for each colour) | **81.1%** | **23.7%** |
| multicolour uncommons | 0.402 | 0.398 |
| a card twice in the pack | **0%** | **9.1%** (the wildcard or foil card repeating one) |
| GROUP9 commons (below) at most once | **100%** | **79.6%** |

### 4.1 Agreement: slots, rarities, sizes

Pool size (78), the rarity mix per pool and per pack (commons, uncommons, rares; the any-rarity
slots), the Special Guest rate (1.8%), Paradox Haze's frequency, colourless and multicolour rates,
and per-card uniformity of the commons all match the published Play Booster within about two
standard errors. Arena's sealed pools are six independent Arena boosters: the pack-level spread of
the commons per colour (sd 0.49) times sqrt(6) is 1.21, the pools' 1.22 (uncommons: 0.549 ×
sqrt(6) = 1.34, pools 1.35).

### 4.2 Booster-fun printings don't weight names on Arena

In paper, the triumphant, surreal and viewport printings make 27 rares and 12 mythics come up 13% and
27% more often than the others. Arena's pools show no difference (1.00 and 0.97; the paper weighting
is ~16 standard errors away for rares). So the generator's default folds those rates into the plain
main-set rares and mythics; `booster_fun=True` keeps the paper weighting.

### 4.3 Disagreement: Arena's packs are colour-balanced (a print-run collation)

The largest difference, and the one that matters for deckbuilding: the generator draws each slot
independently, and **Arena's packs are colour-balanced**. In every one of 5,998 packs each colour has
one or two commons, counting the wildcard and foil commons (9-common packs too); no card appears twice
in a pack; uncommons spread over the colours (no colour with two in 81% of packs, against 24%
independent), with the multicolour rate unchanged; and nine commons (`GROUP9` in
`tools/sealed_measure.py`: Chrome Companion, Command Bridge, Drill Too Deep, Meltstrider's Gear,
Nanoform Sentinel, Red Tiger Mechan, Temporal Intervention, Thaumaton Torpedo, Zealous Display) are
never together, though each is as frequent as any common (~50 shared packs expected per pair). A
pack holds one of them with probability 0.58 / 0.78 / 0.89 / 1.00 given 6 / 7 / 8 / 9 commons: k/9
for k commons, the signature of **a run of k consecutive cards from a sequence of 81 commons with
those nine every ninth position**, i.e. a print-run (sheet) collation. A sequence also keeps the
colourless rate per run length (0.51 / 0.58 / 0.71 per 7 / 8 / 9 commons, against 6k/81 = 0.52 /
0.59 / 0.67), which simple rejection sampling ("each colour 1-2 commons") does not (tried: 0.34 for 7
commons). Other pairs co-occur at chance, so Arena does not use one fixed sequence. Consequence: a
generated pool's colour depth is far more spread (commons of a colour sd 2.57 against 1.22; all
cards 3.43 against 2.03): many more lopsided pools than Arena gives. Whether paper Play Boosters are
balanced the same way is not published (Wizards' article gives slots and rates only).

### 4.4 Disagreement: more Stellar Sights on Arena

Arena opens a Stellar Sights land in 15.1% (sealed) to 15.7% (draft packs) of boosters against the
published ~14% (12.5% wildcard + 1.5% foil), with main-set rares and mythics correspondingly fewer
(7.20 against 7.25 per pool; rare-or-higher in total the same, 8.11 against 8.09). About 3-4
standard errors in each source; the slot it comes from cannot be told from pools.

### 4.5 Which collation the generator should follow

For "sealed as people play it", **Arena's**: the 17lands yardstick pools are Arena's, and its pools
are six of its boosters. The nonbasic structure (13 cards, rarity slots, rates) is the published
paper Play Booster, which the generator implements; it already follows Arena where Arena is
documented or measured without fitting (no land slot, no foils, no booster-fun weighting). It does
not yet reproduce Arena's colour balance (4.3) or its Stellar Sights rate (4.4): the open question
below.

## 5. Open questions for S2

1. **Model Arena's colour balance?** A print-run model (commons as runs of a colour-interleaved
   sequence with GROUP9 every ninth, the any-rarity commons continuing the run; uncommons spread
   likewise) reproduces the structure but is inferred from 17lands packs: a collation fact, not human
   play, yet the user should decide whether that is yardstick use. Without it, the network trains on
   pools with twice the colour-depth spread of the evaluation pools.
2. **Stellar Sights at ~15.5% or the published 14%?** Same question; the effect is small (a Stellar
   Sights land in 62% of pools instead of 59%).
3. **Paradox Haze** has no IR (mtg-bot's parser leaves it out): 1% of pools. Port it, or treat pools
   holding it as unsupported (the engine can't play them today either way).
4. **Support**: until Gene Pollinator, Unravel, Steelswarm Operator, Tractor Beam (commons) and the
   rest land, 8% of real pools are fully supported; the S5 comparison needs (nearly) all.
5. **Whole draft file**: 2,006 drafts gave 6,018 packs; the full 142 MB file gives ~7x more if a
   finer collation model needs it (e.g. GROUP9's analogue among uncommons).
6. **Best-of-three**: the 903 Traditional pools (with per-game sideboarding) are kept for the later
   best-of-three milestone.

## 6. Reproducing

```bash
# the card list (needs mtg-bot's dumps and the saved Scryfall search for EOE 277-316)
python tools/sealed_eoe_set.py --treatments eoe_277_316.json
# the 17lands datasets (CSV downloads outside the repo)
python -m mtgz.sealed.seventeen_lands build --sealed game_data_public.EOE.Sealed.csv.gz \
    --trad game_data_public.EOE.TradSealed.csv.gz
python -m mtgz.sealed.seventeen_lands packs --draft draft_head.csv.gz.part --byte-range 0-20000000
python -m mtgz.sealed.seventeen_lands report
# the measurements of section 4 (~30 s)
python tools/sealed_measure.py --pools 20000
```
