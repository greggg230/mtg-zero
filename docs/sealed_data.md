# Sealed data: the 17lands EoE yardstick and the Play Booster collations (S2 groundwork)

Branch `eoe-s2-data` (from `eoe-base` 9cdda51), 2026-11-08. DESIGN.md 9, milestone S2. Code:
`mtgz/sealed/` (pure Python, no engine module), `tools/sealed_eoe_set.py`, `tools/sealed_measure.py`;
tests `tests/test_sealed_17lands.py`, `tests/test_sealed_collation.py`; data `data/sealed/`.

**The rule (DESIGN.md 0).** The 17lands data is a **yardstick**: it measures (a pool generator's
statistics against what Arena opened; later our builds against the human builds of the same pools),
and is never a training target, reward or prior. **One allowed use besides (user, 2026-11-08):
inferring how the game generates packs.** Arena's collation (section 4) is an environment model
(how boosters are made, not how humans play), so the `arena` collation's structure and Stellar Sights
rate are inferred from 17lands' packs and pools. Nothing about human picks, builds or results feeds
the generator.

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
cards with no basic) and have no foils (the foil slot is a nonfoil card of any rarity). So paper and Arena
sealed have the same nonbasic slots; how the cards in them are drawn differs (section 3).

`tools/sealed_eoe_set.py` builds the card list from mtg-bot's Scryfall dumps
(`edge_of_eternities.json`, `stellar_sights.json`, `special_guests_eoe.json`, read-only; sha256s in
the file) and one Scryfall search for the booster-fun printings EOE 277-316. 321 cards: the main set
`eoe` (81 commons, 100 uncommons, 60 rares, 20 mythics = 261), Stellar Sights `eos` (30 rares, 15
mythics), the Special Guests `spg` (10 mythics, SPG 119-128), the 5 basics; per card its rarity,
collector number, colours, colour identity, mana value, type line and `in_ir`. Booster-fun printings:
viewport lands 277-286 (the five shocks, rare; the five planets, mythic), triumphant 287-302 (12
rares, 4 mythics), surreal space 303-316 (11 rares, 3 mythics; Starwinder is in both groups). The
commons are 15 per colour plus 6 colourless (no multicolour common).

## 3. The generator: two collations (`mtgz/sealed/collation.py`)

```python
from mtgz.sealed import collation
pool = collation.generate(seed, collation='arena')   # or 'paper'; no default
pool.cards     # the 78 nonbasic names, sorted        pool.boosters  # each booster's (slot, name)
pool.meta      # {'seed', 'collation', 'boosters', 'prerelease_promo', 'paradox_haze', 'generator'}
collation.sealed_pool(seed, collation='paper')       # just the names
```

Options: `boosters` (6), `prerelease_promo` (paper only, default off), `paradox_haze` (default off).
Randomness is splitmix64 with integer-only sampling (uniform ints by rejection, distinct cards by a
partial Fisher-Yates shuffle, weighted choices by integer weights): a Rust port reproduces a pool bit
for bit from its seed and collation (`test_the_generator_is_pinned`); the draws come in the order of
`_paper_booster` / `_arena_booster`.

**Paradox Haze** (a Special Guest the user excluded from the engine; it has no IR) is skipped by
default in both collations: when the Special Guest slot draws it, the slot draws again, so the slot's
1.8% rate stays and the other nine guests share it. `paradox_haze=True` keeps it (for statistics). In
the 17lands data the pools holding it stay flagged (`Pool.not_in_ir()`).

### 3.1 `paper`: Wizards' published Play Booster

Wizards, [Collecting Edge of Eternities](https://magic.wizards.com/en/news/feature/collecting-edge-of-eternities)
(2025-07; the same figures are transcribed in mtg.wtf's
[`data/boosters/eoe/play.yaml`](https://github.com/taw/magic-search-engine/blob/master/data/boosters/eoe/play.yaml)).
14 cards and a double-sided token (no ad or art card):

| Slot | Contents (rates per booster) |
|---|---|
| Commons (7) | 7 of the 81 main-set commons; in **1.8%** of boosters a non-foil Special Guest (one of 10) replaces one (never in the wildcard or foil slot) |
| Uncommons (3) | 3 of the 100 main-set uncommons |
| Wildcard (1) | main set C 12.5%, U 62.5%, R 10.6%, M <1%; **Stellar Sights R 10%, M 2.5%** ("a land from the Stellar Sights bonus sheet in 1 out of 8 Play Boosters"); viewport land R 1%, M <1%; triumphant or surreal R <1%, M <1% |
| Rare (1) | main set R 80.4%, M 14.2%; triumphant R 2%, surreal R 2%; triumphant / surreal / viewport M <1% each |
| Traditional foil (1) | main set C 58%, U 32%, R 6.4%, M 1.1%; Stellar Sights R 1%, M <1%; viewport / triumphant / surreal R <1%, M <1% |
| Land (1) | basic land: default frame 80% (foil 16 of it), borderless celestial 20% (foil 4); uniform by name |

**Assumptions** (Wizards gives no figure): each "<1%" entry of a slot gets an equal share of what the
slot's printed rates leave: wildcard 0.225% each (as mtg.wtf), rare slot 0.467 / 0.467 / 0.466%,
foil 0.5% each. Within a group every printing is equally likely (so a card with a booster-fun
printing comes up more often: 13% for rares, 24-27% for mythics), within a rarity of a sheet every
card. Slots are independent draws (Wizards publishes no in-pack constraint). Stellar Sights in 14.0%
of boosters (12.5 wildcard + 1.5 foil). The land slot is in `pool.boosters` / `pool.basics`, not in
`pool.cards` (basics are free in sealed). **The prerelease promo** (`prerelease_promo=True`; the
Prerelease Pack's "traditional foil year-stamped rare or mythic rare"): assumed main-set rare vs mythic
as the rare slot's 80.4 : 14.2, uniform within each; drawn after the boosters, so `pool.cards` is 79.

### 3.2 `arena`: MTG Arena's boosters as 17lands shows them

13 cards. What changes from paper, and the evidence (section 4):

* **No land slot** (Arena's basics are free; 17lands never records one: 6,018 fresh packs of 13 cards)
  and **no foils** (the foil slot is a nonfoil card of any rarity, its paper rarity mix).
* **No booster-fun weighting**: Arena's pools show the 27 rares and 12 mythics with booster-fun
  printings exactly as often as the others (4.2), so those slots' rates go to the main-set rarity:
  rare slot R 84.4% / M 15.6%; wildcard and foil main-set R/M get the viewport / triumphant /
  surreal shares.
* **Stellar Sights in 15.22% of boosters** (17lands, 69,886 boosters of the complete Premier and
  Traditional pools and the fresh draft packs: 15.218%, standard error 0.139%, **95% CI 14.95-15.49%**;
  paper 14.0%). The data can't tell which slot: the 1.218% over paper goes to the wildcard (13.718%,
  rare : mythic 2 : 1 per card as paper), taken from the wildcard's main-set rares and mythics in
  proportion (`ARENA_WILDCARD`); the foil slot's 1.5% as paper.
* **Colour balance as print runs.** A pack's commons are a window of k consecutive cards (k = 6 to
  9: the commons slots plus a wildcard or foil common, which continue the run) at a uniformly random
  start in a fresh run of the 81 commons: the nine GROUP9 commons at every ninth place; the six
  colourless commons never adjacent and at most two in any nine places; one random order of W U B R G
  repeated over the 75 coloured places (consistent with the GROUP9 cards' colours: the colour with two
  GROUP9 cards sits on the residue two GROUP9 places share). A pack's uncommons are a window of k = 3
  to 5 in a fresh run of the 100 uncommons where no two cards within `gap - 1` places share a colour
  (multicolour cards count for both, colourless is a colour of its own and stays 5 apart): gap 4 for
  up to four uncommons, gap 3 for five, built greedily with integer weights (cards left, x10 for a
  multicolour card, the colours' urgency, x5 for continuing the colour 3 places back in gap-3 runs),
  restarting on a dead end. Rare-or-higher slots draw again on a repeat. **A window over a run holds
  each card with the same probability (k / run length), so the balance changes no card's frequency:
  no filtering.**
* Six independent boosters per pool (17lands pools match six independent Arena packs, 4.1).

The run layout is a model: it reproduces every measured fact (4.3), not Arena's code. Speed: ~40 ms per
arena pool in Python (the uncommon run dominates), 0.2 ms per paper pool.

## 4. Measurements (`tools/sealed_measure.py`)

17lands pools: the 9,746 complete Premier and 902 Traditional pools. Arena packs: the fresh packs
(pick 0 of each pack) of the first 2,006 drafts of
[`draft_data_public.EOE.PremierDraft.csv.gz`](https://17lands-public.s3.amazonaws.com/analysis_data/draft_data/draft_data_public.EOE.PremierDraft.csv.gz)
(its first 20 MB of 142 MB, a byte-range download), 6,018 packs (5,998 of 13 cards; 20 launch-day
packs of 12, the unseen Special Guests), committed as `data/sealed/eoe_17lands_draft_packs.json.gz`
(178 KB; `load_draft_packs()`). Generated: paper seeds 0-19,999, arena seeds 0-2,999 (pools); packs:
the 18,000 boosters of the first 3,000 pools of each (paper without its land). The arena model's
structure and Stellar Sights rate were inferred from these packs and pools, so its agreement there is
in-sample; the rest is a check.

Per pool: mean ±standard error (sd).

| Per pool | 17lands Premier | 17lands Trad | paper | arena |
|---|---|---|---|---|
| main commons | 46.11 ±0.02 | 46.01 ±0.05 | 46.12 ±0.01 | 46.10 ±0.03 |
| main uncommons | 23.67 ±0.02 | 23.69 ±0.05 | 23.67 ±0.01 | 23.67 ±0.03 |
| main rares | 6.15 ±0.01 | 6.21 ±0.05 | 6.19 ±0.01 | 6.11 ±0.02 |
| main mythics | 1.044 ±0.009 | 1.04 ±0.03 | 1.076 ±0.007 | **1.094** ±0.018 |
| Stellar Sights rares + mythics | 0.729 + 0.179 | 0.760 + 0.172 | **0.655 + 0.175** | 0.719 + 0.197 |
| Special Guests | 0.113 ±0.003 | 0.12 ±0.01 | 0.109 ±0.002 | 0.116 ±0.006 |
| pools with a Stellar Sights land | 62.2% | 63.2% | **58.9%** | 61.9% |
| cards of one colour (W) | 15.13 (sd 2.03) | 15.01 (sd 2.11) | 15.16 (**sd 3.41**) | 15.15 (sd 2.02) |
| commons of one colour (W) | 8.52 (sd 1.22) | 8.42 (sd 1.23) | 8.53 (**sd 2.55**) | 8.54 (sd 1.21) |
| uncommons of one colour (W) | 4.04 (sd 1.35) | 3.96 (sd 1.33) | 4.05 (**sd 1.82**) | 4.03 (sd 1.32) |
| colourless commons | 3.45 (sd 1.54) | 3.54 (sd 1.58) | 3.43 (sd 1.72) | 3.45 (sd 1.57) |
| multicolour uncommons | 2.35 (sd 1.33) | 2.38 (sd 1.29) | 2.35 (sd 1.45) | 2.36 (sd 1.34) |
| **colour depth: deepest colour** | 17.45 (sd 1.26) | 17.42 (sd 1.20) | **19.41 (sd 2.09)** | 17.42 (sd 1.30) |
| **shallowest colour** | 12.53 (sd 1.21) | 12.48 (sd 1.22) | **10.79 (sd 1.82)** | 12.55 (sd 1.22) |
| **best two-colour pair's cards** | 30.82 (sd 1.71) | 30.75 (sd 1.67) | **33.37 (sd 2.61)** | 30.82 (sd 1.74) |
| distinct cards | 66.00 ±0.03 | 65.97 ±0.09 | **65.46** ±0.02 | 65.78 ±0.05 |
| main rares with a booster-fun printing / others, per card | 1.00 | 1.02 | **1.13** | 1.02 |
| same for mythics | 0.97 | 0.96 | **1.24** | 1.00 |
| chi-square against uniform: commons (df 80) / uncommons (99) / rares (59) | 71 / 114 / 53 | 74 / 131 / 47 | 56 / 124 / **542** | 66 / 86 / 71 |

| Per pack | 17lands fresh packs | paper (no land) | arena |
|---|---|---|---|
| commons / uncommons / main rares / main mythics | 7.687 / 3.940 / 1.033 / 0.169 | 7.682 / 3.948 / 1.034 / 0.177 | 7.683 / 3.944 / 1.019 / 0.182 |
| Stellar Sights | 0.157 ±0.005 | **0.140** ±0.003 | 0.153 ±0.003 |
| Special Guests | 1.45% (1.78% with the 12-card packs) | 1.9% | 1.9% |
| commons of one colour (W) | 1.424 (sd 0.49) | 1.432 (**sd 1.05**) | 1.423 (sd 0.49) |
| every colour has 1 or 2 commons | 100% | **15.2%** | 99.99% |
| colourless commons | 0.561 (sd 0.62) | 0.573 (sd 0.70) | 0.575 (sd 0.63) |
| no colour with 2+ uncommons (multicolour counted in each, colourless a colour) | 81.1% | **23.8%** | 82.3% |
| multicolour uncommons | 0.402 | 0.389 | 0.393 |
| a card twice in the pack | 0% | **9.1%** | 0% |
| GROUP9 commons per pack / at most one | 0.855 / 100% | 0.850 / **80.1%** | 0.854 / 100% |
| common pairs, observed / independent: same colour | 0.449 | **0.888** | 0.448 |
| different colours / with a colourless / two colourless | 0.993 / 0.949 / 0.656 | 0.886 / 0.887 / 0.885 | 0.994 / 0.946 / 0.696 |
| GROUP9 pairs; common pairs never together (expected >= 20) | 0; 36 | 0.871; **0** | 0; 36 |
| uncommon pairs: share a colour / disjoint / with colourless / two colourless | 0.19 / 0.965 / 0.812 / 0.0 | 0.78 / 0.78 / 0.78 / 0.78 | 0.163 / 0.971 / 0.819 / 0.0 |

### 4.1 Both agree with Arena on slots, rarities and sizes

Pool size (78), the rarity mix per pool and per pack, the Special Guest rate, colourless and
multicolour rates, and per-card uniformity of commons and uncommons match the published Play Booster
within about two standard errors in both collations. Arena's sealed pools are six independent Arena
boosters: the pack-level spread of the commons per colour (sd 0.49) times sqrt(6) is 1.21, the pools'
1.22 (uncommons: 0.549 x sqrt(6) = 1.34, pools 1.35).

### 4.2 Booster-fun printings don't weight names on Arena

In paper the triumphant, surreal and viewport printings make 27 rares and 12 mythics come up 13% and
24% more often (the paper column; the rares' chi-square 542 on 59 degrees of freedom). Arena's pools
show no difference (1.00 and 0.97; the paper weighting is ~16 standard errors away for rares).

### 4.3 Arena's colour balance, and how well the print-run model fits

What 17lands' 5,998 Arena packs show, and the arena model's fit (paper, independent slots, for contrast):

* **Commons:** every colour has one or two commons in every pack, the wildcard and foil commons
  included (arena 99.99%: a 6-common pack with two colourless misses a colour, 0.01% of packs; paper
  15%); no card twice (arena 0%; paper 9.1%); same-colour pairs at 0.45 of independence (arena 0.45),
  different colours at 0.99 (0.99) with no colour-order structure (every colour pair 0.97-0.98).
* **GROUP9:** nine commons (Chrome Companion, Command Bridge, Drill Too Deep, Meltstrider's Gear,
  Nanoform Sentinel, Red Tiger Mechan, Temporal Intervention, Thaumaton Torpedo, Zealous Display) never
  share a pack (36 pairs, ~50 shared packs expected each), each as frequent as any common; a pack holds
  one with probability 0.58 / 0.78 / 0.89 / 1.00 given 6 / 7 / 8 / 9 commons: k/9, the signature of
  a window over a run with those nine every ninth place. Arena model: the same 36 zero pairs, 0.854
  per pack (data 0.855).
* **Colourless commons:** at most two per pack; two non-GROUP9 colourless commons share a pack at 0.45
  of independence, a GROUP9 one with another at 0.75; two colourless in 5% / 8% / 10% of 7- / 8- /
  9-common packs. The rule "never adjacent, at most two in nine places" gives 0.41 / 0.70 and 5.2% /
  7.4% / 9.9% (a minimum spacing of 6, the first model, gave 0.07 / 0.93 and too few pairs).
* **Uncommons:** with three or four uncommons no two share a colour (100%); with five, 94% repeat one
  (1, 2 or 3 sharing pairs in 41% / 47% / 7%: a window of five over a gap-3 run with the lane weight
  gives 44% / 47% / 1%); the five colourless uncommons never share a pack (0 of ~90 expected); the
  multicolour rate is unchanged. A single gap-4 run can't hold the five-uncommon packs (it allows one
  sharing pair at most), hence gap 3 for those.
* **Colour depth** (what deckbuilding feels): arena matches Arena's deepest colour (17.42 vs 17.45),
  shallowest (12.55 vs 12.53), best pair (30.82 vs 30.82) and their spreads; paper's independent slots
  give much deeper and more lopsided pools (19.41, 10.79, 33.37, spreads x1.5-1.7).

**Residuals:** arena has a few more main-set mythics (1.094 vs 1.044 per pool, 2.5 standard errors;
0.182 vs 0.169 per pack, 2.2) and slightly fewer distinct cards per pool (65.78 vs 66.00, ~4 standard
errors: Arena may avoid repeats across a pool's packs a little); three sharing pairs in five-uncommon
packs are rarer than Arena's (1% vs 7%); and some same-colour common pairs co-occur from 0.13 to 0.83
of independence in the data (GROUP9 cards with their colour high, a few pairs low), more spread than a
random run gives: Arena's runs aren't fully random. Whether paper Play Boosters are balanced the same
way is not published.

### 4.4 Stellar Sights

Arena opens a Stellar Sights land in 15.22% of boosters (95% CI 14.95-15.49%; Premier sealed 15.14%,
Traditional 15.54%, draft packs 15.67%) against paper's published 14.0%; the arena collation uses the
estimate (3.2), the paper collation the published rate.

### 4.5 Which collation for what

`arena` for **sealed as people play it**: the 17lands yardstick pools are Arena's, and the arena
collation reproduces their pools and packs (4.3). `paper` for tabletop sealed exactly as Wizards
publishes it (with the land slot and the optional prerelease promo); its colour depth is far more
spread than Arena's, and whether paper boosters are balanced is not public.

## 5. Open questions for S2

(Status 2026-11-09, branch `eoe-s2-pools`: the Rust port, the builder and the sealed spec are
docs/ENGINE.md 101.)

1. **The residuals of 4.3** (mythics, cross-pack repeats, five-uncommon sharing, uneven same-colour
   pairs). The whole draft file (142 MB, ~7x the packs) would sharpen all of them if they matter.
   Open; the Rust port reproduces the model as it is, so a change goes to both (and the comparison
   test shows a drift).
2. **Paradox Haze**: excluded from both collations; 1% of 17lands pools hold it (flagged, and
   `mtgz.sealed.yardstick` reports them as unplayable: no IR).
3. **Support**: at `eoe-s2-pools` (319 of the table's 320 cards; The Dominion Bracelet on another
   branch) **9,149 of 9,765 Premier pools (93.7%) and 835 of 903 Traditional (92.5%) are fully
   supported** (`python -m mtgz.sealed.yardstick report`, read from the engine's card table); with
   the Bracelet merged 99.1% / 98.1%, Paradox Haze the rest. A generated pool holding an
   unsupported card fails loudly in the `mtg` spec (~4.8% of pools on this base).
4. **Speed**: done. `engine/src/az/sealed/collation.rs` reproduces both collations bit for bit
   (`tests/test_sealed_rust.py`: 2,006 seeds per collation and the options, slot by slot): 0.015 ms
   per paper pool, 1.8 ms per arena pool (Python ~50 ms).
5. **Best-of-three**: the 903 Traditional pools (with per-game sideboarding) are kept for the later
   best-of-three milestone. Sealed best-of-three also needs a pool list past 64 entries (a pool has
   ~66 different cards; ENGINE.md 36.2's sideboarding list holds 59 and the basics) and hidden
   decklists (ENGINE.md 101.5).
6. **The builder's spread** (ENGINE.md 101.2, no human data): ~1.6% (arena) / 4.5% (paper) mono
   decks, ~25% splashes, lands uniform 16-18, every colour pair. Whether these rates suit the play
   network (S3) is open: they are settings of the spec.

## 6. Reproducing

```bash
# the card list (needs mtg-bot's dumps and the saved Scryfall search for EOE 277-316)
python tools/sealed_eoe_set.py --treatments eoe_277_316.json
# the 17lands datasets (CSV downloads outside the repo)
python -m mtgz.sealed.seventeen_lands build --sealed game_data_public.EOE.Sealed.csv.gz \
    --trad game_data_public.EOE.TradSealed.csv.gz
python -m mtgz.sealed.seventeen_lands packs --draft draft_head.csv.gz.part --byte-range 0-20000000
python -m mtgz.sealed.seventeen_lands report
# the measurements of section 4 (~2 min)
python tools/sealed_measure.py --pools 20000 --arena-pools 3000
```
