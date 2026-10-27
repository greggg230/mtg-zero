# MTG Zero - team cube drafting and play on zero-core (design)

Status: draft, 2026-10-21 (revised the same day: pure self-play team draft). Research behind it:
`docs/research/` (card_ir_survey.md, rust_engine_reuse.md, draft_data.md, prior_art.md),
`docs/data_m1.md`.

## 0. Decisions (user, 2026-10-21)

* Goal: AI drafters and players for **team cube draft** with LSV's PowerLSV cube, built on the
  shared AlphaZero framework `~/zero-core` like Glass Road, Caverna and Kingdom Builder.
* **Pure AlphaZero, no imitation of human drafters.** Every training signal comes from games the
  engine plays. Human data (CubeCobra / 17lands picks, Draft Ranker, roto) is used only as a
  yardstick, never as a training target or reward.
* MTG gameplay is **ported to Rust** as a zero-core `SearchGame` (not bridged to the Python
  engine, not a Python-game adapter).
* The mtg-bot EoE lineage (s2, v30) and `feature/s2-resolution-decisions` stay parked.
* Pacing: at most two agents at a time, Opus for design and review, Sonnet for mechanical
  porting, a stop for the user's go-ahead at every milestone (section 7).

Decisions (user, 2026-10-25), for M8 "matches and sideboarding":

* **A best-of-three match is the self-play unit** of the `mtg` game (one `SearchGame` episode
  is a match, not a game), with sideboarding between games; its result is the match's.
* **Each player knows the opponent's pool except the opponent's first picks** (one per pack:
  in a team draft the other players see every pick but the first of each pack). This replaces
  the open-decklist assumption (both decklists known).
* Phase 1 (M8a, ENGINE.md 35) lays the foundations inside one game: a sideboard per player
  (cards outside the game), the play-or-draw choice with the chooser a spec parameter (the
  loser of the previous game in later games of a match), companions. Phase 2: the match as the
  unit, sideboarding, the pool knowledge, layout v4 completed and a new training run.

## 1. Shape

Repo `~/mtg-zero` (local git), laid out per zero-core `docs/new_game.md`: `third_party/zero-core`
(pinned submodule), `engine/` (Rust), `card_ir/` (generated card types), `py/` (binding crate,
module `mtgz._engine`), package `mtgz` with `mtgz/az/*` shims, `tools/`, `tests/`, `runs/`.
One engine module registers two games:

| Game | What |
|---|---|
| `mtg` | one match of Magic between two decks (best-of-1 first, best-of-3 later) |
| `team_draft` | a team-draft event: draft, deckbuilding, then the matches (played by the `mtg` agent) |

The Python engine in `Documents/Claude/Projects/mtg-bot` stays the source of card data (its
oracle-text parser) and a reference for differential tests. Nothing is imported from it at run
time: cards cross over as a data file (section 3.2).

## 2. Team Draft Zero

### 2.1 The event

* Two teams of four, seated alternately (A1 B1 A2 B2 A3 B3 A4 B4), so every pass goes to an
  opponent.
* Draft: 3 packs of 15 from the cube, passed left, right, left. No communication during the draft.
* After the draft, teammates share everything: their pools, and what each of them saw and passed
  (which says a lot about the opponents' pools).
* Deckbuilding: each player builds a 40-card deck from their own pool plus basics.
* Matches: 3 rounds. Each round pairs every A player with a different B player; the 3 rounds are
  3 of the 4 rotations of a 4x4 Latin square, chosen at random, so everyone plays 3 of the 4
  opponents. 12 matches in total.
* Outcome: the team with more match wins wins; 6-6 is a draw. Per seat:
  1 / 0 / 0.5. Margin (aux target): (team match wins - opponent match wins) / 12.
* Settings: team size (2 or 4), rounds, match format (best-of-1 / best-of-3), cube list.
  Staging pod: **2v2** (4 seats, 180 cards, 2 rounds) while the Rust engine supports fewer than
  the 360 cards a 4v4 draft uses. (2026-10-27: every cube card is supported; 4v4 PowerLSV is the
  target pod, 2v2 for smoke tests; the event exactly: 2.3.1.)
* A match (best-of-three, 2026-10-25) is one episode of the `mtg` game: in game 1 a player
  chosen at random chooses to play or draw, in games 2 and 3 the loser of the previous game
  (CR 103.1); between games each player may exchange cards between deck and sideboard (CR
  100.4; in limited the sideboard is the rest of the pool, 100.4b). Each player knows the
  opponent's pool except the opponent's first picks (one per pack).

### 2.2 How it learns

Two self-play loops share each event:

* **Draft and deckbuilding decisions** are the `team_draft` game, searched by the draft network.
  Its terminal result is the team outcome after the matches have been played by the current
  play agent. This needs a zero-core hook for results that arrive after an expensive
  evaluation step (section 5).
* **Match decisions** are the `mtg` game, self-played on decks from team drafts. Before drafting
  works, the decks are random decks from the supported cards. Winning your own match is exactly
  how you help your team, so match outcomes are the right targets for in-game decisions.

Bootstrap order: rules engine, then the play agent on random decks, then drafting from random,
then the two co-evolve.

Risks, stated up front:
* The draft can only be as smart as the player. A weak play agent rewards decks that are easy
  to pilot (aggro creatures) and undervalues combo and control until play improves.
* Outcomes are noisy (published deck-strength prediction from drafts reaches Spearman ~0.20
  against a ~0.23 noise ceiling). Twelve matches per event help; volume does the rest, so the
  Rust engine's speed matters.

### 2.3 The `team_draft` game (M6 design, 2026-10-27)

The user's decisions of 2026-10-27: full co-training now (the matches that score a draft are the
play network's own self-play, `mtg` training data, and their results are the draft's terminal
result: two networks in one run); 4v4 PowerLSV is the target pod (team size stays a setting,
2v2 for smoke tests); learned deckbuilding inside `team_draft`; pure AlphaZero (human pick data
is a yardstick only); best-of-three matches with sideboarding; each player knows the opponent's
pool except their first picks, one per pack. Part 1 (this section, built in
`engine/src/az/draft*.rs`, docs/ENGINE.md 41) is the game; part 2 is zero-core's deferred hook
and co-training (section 5.1); part 3 the integration (section 7.1).

#### 2.3.1 The event, exactly

* **Seats and teams.** `2T` seats (`T` = team size 1-4; 4 for PowerLSV, 2 for smoke tests;
  zero-core's `MAX_SEATS` is 8), seated A1 B1 A2 B2 ...: seat `s` is in team `s % 2` (A = even
  seats), player `s / 2` of it. The players count of zero-core's `new_game` is `2T`.
* **The cube and the packs.** The draftable cube is the PowerLSV IR card list less the five basic
  lands: **541 cards**, singleton, every one supported by the engine (a cube card the engine does
  not support is an error, never skipped). The game seed shuffles the 541 (Fisher-Yates); pack
  `(round r, opener s)` is the next 15 cards of the shuffle (`r = 0..2`, `s = 0..2T-1`); the rest
  is never drafted (4v4: 360 drafted, 181 not; 2v2: 180 / 361). Settings `packs` (1-3) and
  `pack_size` (2-15) for small tests; a pool is `packs x pack_size` cards (45).
* **Passing.** Round 0 passes left (seat `s` to `s + 1`), round 1 right, round 2 left. At pick
  `t` (0-14) of round `r` seat `s` holds the pack opened by `s - t` (left) or `s + t` (right),
  modulo `2T`. Seats alternate teams, so **every pass goes to an opponent**, in both directions.
* **Picks** are simultaneous within a pick round (every seat holds a pack, all pick, all pass);
  the game asks them **in seat order 0..2T-1, each pick hidden from the others** (Glass Road
  Zero's simultaneous-choice pattern, its docs/search.md 9.3-9.5: the later chooser's
  `determinize` resamples what the earlier ones chose; `encode` never shows it). A pack's last
  card is a single-option pick (zero-core plays it without a search).
* **Deckbuilding** (2.3.3) follows the draft, seat by seat.
* **Pairings.** Round `k` of the matches pairs A-player `i` with B-player `(i + d_k) mod T`: the
  `rounds` rotations `d_k` are distinct rotations of a `T x T` Latin square drawn at random from
  the game seed (4v4: 3 of the 4 rotations, every player meets 3 of the 4 opponents, **12
  matches**). `rounds` defaults to `min(T, 3)`: 2v2 plays both rotations (4 matches), 1v1 one.
  The pairings are hidden until the matches (nothing reveals them earlier; `determinize`
  redraws them).
* **Matches**: best-of-three `mtg` matches (ENGINE.md 36) between the players' built decks, the
  A player as `mtg` seat 0; game 1's chooser at random (CR 103.1); sideboarding between games;
  the deck goes back to the built deck at the start of every match.
* **Outcome.** Per seat: 1 for a team win (more match wins than the other team), 0 for a loss,
  0.5 for a draw (6-6, or equal wins with drawn matches: a drawn match counts for neither).
  zero-core's result vector sums to 1, so `result()[seat]` is that outcome **divided by `T`**
  (each winner `1/T`; a draw `1/2T` each) and part 2's `team_size` hook scales the search's
  utility back to [0, 1] (5.1). Margin (the auxiliary target, every seat of a team the same):
  `(team match wins - opponent match wins) / matches` (4v4: / 12).

#### 2.3.2 The information model

What a seat knows is the packs it has seen: every time a pack reaches it (or it opens one) it
sees the pack's contents, and it knows its own picks. **During the draft nothing else is known**:
no communication, so not even a teammate's picks; the other seats' picks of the current pick
round are hidden (simultaneous). **After the draft teammates share everything**: each player
knows the union of its team's observations, and during deckbuilding the decks its teammates are
building.

**The first-picks rule follows from this** (my reading of the 2026-10-25 rule, checked by a
test, 2.3.7): a pick of an opposing seat at pick `t >= 1` of a pack was made between two seats of
my team (the seats alternate): the teammate before it passed the pack (its contents known), the
teammate after it received what was left (known), so the card taken is the difference; the last
card of a pack is passed by a teammate. The only opposing picks no seat of my team ever sees are
**the cards each opponent takes first from the pack it opens: its first picks, one per pack**.
After the draft a team knows the whole draft but the opposing team's `3T` first picks (4v4: 12
of 360 cards). So at the matches each player knows each opponent's pool except that opponent's 3
first picks, and also everything else of the event (its teammates' pools, the other opponents'
known cards), which narrows what an unseen first pick can be: a card the team never saw (4v4:
193 candidates, 12 of them the opposing first picks, 181 undrafted).

Readings to confirm (open questions 2.3.10): a player's knowledge at each match is the
post-draft knowledge only (what another match reveals is not carried to other matches); a
teammate's deck is shared with its team; the built deck is every match's game-1 deck.

**`determinize(viewer)`** keeps exactly what the viewer knows and redraws the rest uniformly. A
pack is stored as its removal order (positions `0..14`: the card taken at pick `t` is at position
`t`; the cards still in it after them, unordered). The viewer's "knowing seats" are itself during
the draft and its team after it; their views of a pack are the picks `t_1 < t_2 < ...` at which a
knowing seat held it. Kept: every view's contents (positions `t_i..14` as a set) and the knowing
seats' own picks (position `t_i`). Redrawn: (a) positions before the first view (unseen picks:
an opener's first picks, picks made before the pack reached the viewer) and every position of a
pack no knowing seat has seen (later rounds, packs not yet reached), together with the undrafted
cards: the cards none of the knowing seats has seen, shuffled over those places; (b) between two
views, which of the seats in between took which of the cards that disappeared (a permutation);
(c) after the last view, which of the cards left were taken by the next seats and which are
still in the pack (a permutation of the suffix). Every such world is a legal draft history (a
pick is legal iff the card is in the pack, which the removal order guarantees). After the draft
only (a) remains: the opposing first picks and the undrafted cards. Also redrawn: the pairing
rotations and the match seeds (future chance), the decks of opponents who have built (hidden:
their deck becomes the heuristic deckbuilder's deck from their redrawn pool, `pools::build_deck`;
nothing the viewer sees depends on it) and the deckbuilding of an opponent building now (restarted;
only another viewer than the decider can see this state). The decider's own legal actions are
never changed. The uniform redraw ignores that strong cards go early (an opener's first pick is
usually strong): the opponents' simulated picks inside the viewer's tree play in worlds whose
unseen cards are average; a policy-aware prior is an option for later (2.3.10).

#### 2.3.3 Learned deckbuilding

After the draft each seat, in seat order, builds its deck with its own decisions (searched by the
draft network, never the heuristic builder of `az/pools.rs`):

* **A binary decision per pool card**, in the seat's **build order** (its 45 cards sorted:
  spells before lands; spells by colour group W, U, B, R, G, multicolour, colourless; then mana
  value; then identity): the card's identity = put it in the deck, `OUT` = leave it in the
  sideboard. Both are always legal (basic lands can always fill a deck).
* **Then five counts**, Plains, Island, Swamp, Mountain, Forest: `COUNT + n` basic lands of that
  type, `n` limited so that a deck of `DECK_MIN..=DECK_MAX` = **40..=60** cards stays reachable
  (CR 100.2b: at least 40, any number of basic lands; 60 is the engine bound that sideboarding
  already uses, `matches::MAX_DECK`) and `n <= 31` (`matches::MAX_COUNT`, the `SideboardCount`
  range of the match).
* Every legal deck (pool cards + basics, 40-60 cards, at most 31 of a basic) is reachable in
  exactly one way; a deck is always 50 decisions (45 binary, 5 counts) per player, 400 per 4v4
  event. The sideboard is the rest of the pool (CR 100.4b). **Companions** as in ENGINE.md 35: a
  companion left in the sideboard is revealed in a match when the deck fulfills its condition
  (the `mtg` game asks); a companion put in the deck is an ordinary card. The encoding tells the
  builder which pool companions the deck so far still fulfills.

#### 2.3.4 The `team_draft` SearchGame

| zero-core | `TeamDraftAz` |
|---|---|
| players | `2T` = 2, 4, 6 or 8 (`MIN_PLAYERS` 2, `MAX_PLAYERS` 8) |
| `NUM_ACTIONS` | 1024: `0..767` a card by identity (ENGINE.md 22.2: the cube IR position; pick it, or put it in the deck); `OUT` = 768; `COUNT + n` = 800 + n (n = 0..31); the rest unused (room) |
| `NUM_FEATURES` | 14,488 (layout `mtgz/az/layout_draft_v1.json`, 2.3.5) |
| spec (JSON, every key optional) | `{"rounds": min(T, 3), "packs": 3, "pack_size": 15, "max_turns": 60, "format": "bo3"}` (the last two are the matches'); unknown keys are errors |
| `to_move`, `legal_actions` | the seat whose pick it is (its pack's identities, sorted) or the builder (`[identity, OUT]`, or the counts in range); `None` at the deferred terminal |
| `step` | a pick moves the card into the picker's pool; after the last seat of a pick round the packs pass (revelation keys); after the last pick the draft ends (team keys) and deckbuilding begins; after the last seat's Forest count the event is at its **deferred terminal** |
| chance and revelation keys | `KEY_SEE(seat, identity)`: every card of the pack a seat receives (or opens), sorted, when packs arrive; `KEY_TEAM(team, pack << 24 \| pick << 16 \| identity)`: at the end of the draft, every cell of the draft the team knows; **observer masking** as the `mtg` game (ENGINE.md 10): after `determinize(viewer)` a key private to other seats is emitted once per seat (or team) with the payload `HIDDEN`, so key counts are the same in every world of the viewer and the viewer's tree never branches on what it cannot see. Deckbuilding and the pairings emit none |
| `force_chance` | an opening (`KEY_SEE` at pick 0) takes the forced cards when the world allows it: a forced card is swapped in from a place nobody has seen (the undrafted cards, a later round's pack, another pack opened in the same transition); everything else (arrivals, team keys, `HIDDEN`) falls back and is consumed in order |
| `determinize(viewer)` | 2.3.2; sets the observer; reseeds the event's RNG |
| `encode(viewer)` | 2.3.5 |
| deferred terminal | `to_move()` is `None`; `TeamDraftAz::deferred()` is true until `resolve(results)`; `deferred_payload()` lists the matches (2.3.6). `result()`, `margin()` and `score()` panic while the event is unresolved (a loud failure: before part 2 zero-core would read them as a real terminal); once resolved they are the outcome of 2.3.1 |
| `margin(seat)` | (team match wins - opponent match wins) / matches |
| `score(seat)`, `rounds` | the team's match wins; pick rounds played |
| `minor_decision` | none |
| bots | `random` (zero-core), `consensus` (2.3.8) |

**At the deferred terminal the search must not read a result** (there is none yet): part 2's
hook (5.1) makes the tree treat a deferred terminal as a leaf evaluated by the network from the
root viewer's encoding (the final pools and decks: the draft network's own estimate of the event,
the same thing it learns from the real outcomes), and the self-play driver park the event until
its matches are played.

#### 2.3.5 Layout `draft_v1` (14,488 floats; `mtgz/az/layout_draft_v1.json`)

A card code here is 64 floats: the identity's digits (16, 8, 8: the vocabulary and the identity
embedding are the `mtg` layout's, 1024) and a draft descriptor (types incl. planeswalker,
colours, colourless, mana value, coloured pips per colour, power, toughness, legendary,
companion, the colours a land makes or fetches, colourless mana, fetch land, interaction: all
read from the card's definition, nothing from human data).

| Block | Offset | Floats | Content |
|---|---:|---:|---|
| globals | 0 | 64 | phase (draft, deckbuilding, deferred terminal, resolved); round (3), pick / 15, picks left, pass direction; team size; my player index in my team, my team; I decide, a teammate decides, an opponent decides; my pool and pack sizes; my pool's colours; the deck being built (its size, colour pips, curve 0-7+, basics so far, the card or basic type being decided, options) |
| pack | 64 | 15 x 68 | my current pack (the pack I hold, sorted by identity): card code, present, an option of my decision, wheeled (it was there at my previous view of this pack) |
| pool | 1,084 | 45 x 76 | my picks in pick order: card code, present, round (3), pick / 15, a first pick, in the deck, out of the deck, the card decided now, an option now, its build-order position, a companion the deck so far fulfills |
| bags | 4,504 | 13 x 768 | multisets of identities (0.25 per card): my draft (cards others took between two of my views of a pack; cards I passed whose fate I don't know; cards that wheeled), my teammates' pools (3, in seat order after me), their decks (3, deck cards and basics), the opponents' known pools (4, in seat order from my left) |

The pack and pool segments are the token network's objects; their slot's action is the card's
identity (the layout's `id_actions: 0`: a slot scores action `0 + identity`), which part 2 adds to
`TokenNet` (today's core ignores the key and scores those actions from the pooled state, as
`OUT` and the counts always are). A position holds about 31 tokens during picks (pack ~8, pool
~22, the global token) and 47 during deckbuilding. Everything is the viewer's own knowledge, so
the vector is the same in every world `determinize(viewer)` produces (tested).

#### 2.3.6 What changes in `mtg` (designed here, made in part 3; digests unchanged)

`deferred_payload()` describes each match: the round, the two seats, and per player its deck and
sideboard (the rest of its pool), its first picks (one per pack, the real ones) and **what it
knows is elsewhere**: the cards its team saw in the draft that are not in this opponent's known
pool (its own and its teammates' pools, the other opponents' known cards). Part 3 turns that
into an `mtg` spec:

```json
{"max_turns": 60, "format": "bo3",
 "decks": {"p0": [...], "p0_sideboard": [...], "p0_first_picks": [...], "p0_elsewhere": [...],
           "p1": [...], "p1_sideboard": [...], "p1_first_picks": [...], "p1_elsewhere": [...]}}
```

* `pN_first_picks` become `MatchPool::first_picks` (today only generated pools have them): the
  opponent's first picks are hidden exactly as ENGINE.md 36.11 hides generated ones.
* `pN_elsewhere` narrows the opponent's `FirstPickPrior`: its candidates today are the supported
  cards outside the known part of the opponent's pool and outside the viewer's own pool; a
  drafted match also removes every card the viewer's team saw elsewhere (4v4: 193 candidates
  instead of ~450). The weights stay the engine heuristic's (generator weights x 4 in the known
  pool's colours); a prior learned from the run's drafts is an open question.
* Both keys are new and optional: every existing spec, the generated pools, the evaluation
  pools, best-of-one and the behaviour digests are unchanged (7721970fa2b16be2).
* Spec decks of 40-60 cards with up to 31 of a basic: `MatchPool::new` already takes them (its
  deck bounds come from the first deck).

The part-1 test utility `draft::play_matches_heuristic` builds these matches directly
(`MtgAz` from `Seat`s with the real first picks) and plays them with the heuristic bot, so no
`mtg` change is needed before part 3.

#### 2.3.7 Tests (part 1, `engine/tests/draft.rs`)

Pack generation and sizes, seat alternation and pass directions, pick legality, every pool 45
cards, legal decks (40-60, at most 31 of a basic, the sideboard the rest of the pool), the
Latin-square pairings, the first-picks rule (after the draft a team knows every cell but the
opposing openers' first picks), the information model (`determinize` keeps the viewer's encoding,
known cells and the decider's actions; equal keys mean equal features), clones, `force_chance`,
a random-actions fuzz of whole 4v4 and 2v2 events in which every offered option completes, an
event resolved end to end by the heuristic matches, the bots, the spec, the layout file checked
in; measurements (events per second of random drafts, encode, determinize).

#### 2.3.8 Yardsticks (2.4) for the draft network

* **Draft Elo**: events whose team A seats are draft checkpoint `k` and team B seats checkpoint
  `j` (and swapped), the matches played by the same play agent for both teams. Anchor (1000):
  the `consensus` team.
* **Built-in teams**: `random` (zero-core's: uniform picks and deckbuilding) and `consensus`:
  picks the card with the highest human pick strength (M1: `cubecobra_bt_strength` in
  `data/powerlsv_card_values.json`; ties by identity) and builds the heuristic deckbuilder's
  deck (`pools::build_deck`). The AI team must beat it in real matches.
* **Cheap and frequent**: the matches of draft evaluations played by the heuristic bot (a 4v4
  event's 12 matches take ~0.1 s on one thread); **true but costly**: played by the current play
  network (every few iterations).
* Curiosity only: the draft network's top-1 agreement with held-out human picks (M1 baselines:
  random 21.9%, strongest card 42.3%), never optimised.

#### 2.3.9 Compute budget

Measured on this desktop (runs/mtg5, preset mtg-4: 64 / 16 simulations, p_full 0.25, minor
decisions at 4; GPU inference of the 128 x 4 token network, 768 matches in flight): iterations 1-3
played 600 best-of-three matches each in 774-1,411 s (2,400-1,530 matches per hour as the
matches grew from 339 to 595 searched decisions), **17.1k-20.9k leaves per match**, ~11.4k
leaves/s; training 29-79 s; evaluation (every 2nd iteration) 600 s against the yardsticks and
1,052 s of Elo matches.

A 4v4 event (part-1 fuzz, ENGINE.md 41): 336 searched picks (24 single-option) and 400 deckbuilding
decisions. Proposal:

| | Simulations (full / fast, p_full 0.25) | Leaves per event | mtg-leaf equivalents |
|---|---|---:|---:|
| picks | 64 / 16 (mean 28) | 9.4k | |
| deckbuilding | 32 / 8 (mean 14) | 5.6k | |
| draft total | | 15.0k | ~30k (a draft position has 31-47 tokens, `mtg`'s ~21: ~2x per leaf at the same width) |
| 12 matches (mtg-4's search) | 64 / 16 | 205k-250k | 205k-250k |

* **GPU split**: the draft is ~11-13% of the GPU's leaves, the matches ~87-89%.
* **Events per hour**: 41M leaf-equivalents per hour / 235-280k per event = **~145-175 events
  (1,750-2,100 matches) per hour**.
* **An iteration = 48 events** (576 matches, mtg5's scale): self-play ~17-20 min; training both
  nets ~1-2 min (the draft net gets ~0.25 x 736 = ~185 samples per event, ~8.9k per iteration;
  the play net ~45-85k); evaluation: the play net as mtg-4 (every 2nd iteration, ~27 min), the
  draft Elo with heuristic-played matches every iteration (4 opponents x 16 events: ~2-3 min of
  draft search, the matches in seconds), with play-network matches every 4th iteration (32 events
  against `consensus`: ~10-13 min). **~35-40 min per iteration, ~1.6 iterations per hour**; 100
  iterations ~60-65 h, ~4,800 events, ~58k matches.
* **In flight**: ~64 events drafting (an event's draft is ~736 sequential decisions: ~15-20 min
  of latency with batched search), the matches queued up to 2 x 768 (backpressure: drafting
  pauses beyond it); a pending event's records (~185 x 29 KB in float16) wait ~10-20 min for
  their matches.
* **The signal is thin**: 48 event outcomes per iteration (each the result of 12 matches, with
  the margin as a graded auxiliary target), ~5k over 100 iterations. Ways to more events per
  hour (open question): matches at 32 / 8 simulations (~1.9x), best-of-one matches during
  co-training (~2.5x: against the user's best-of-three decision), 2v2 events (4 matches each:
  a different game, 180 cards).

#### 2.3.10 Open questions for the user

1. **Where game-1 deckbuilding lives.** Default (built): in `team_draft`, one deck per player for
   all its matches, learned by the draft network from the event's outcome. The alternative: no
   deckbuilding in `team_draft`; before game 1 of each match the players "sideboard" from a
   starting deck (the heuristic builder's) with the `mtg` game's existing `Sideboard` decisions
   under the frozen layout v4. For: the credit for a deck decision then comes from one match, not
   12 (far less noise), the play network has ~10x the data, nothing new in the layout. Against:
   a deck per match knowing the opponent's pool (a format change: in the event as designed the
   deck is built before the pairings), the heuristic deck as the search's starting point, and the
   draft network no longer learns what makes a pool good to build from (only to draft it).
2. **The first-picks reading** (2.3.2): the rule follows from teammates sharing what they saw;
   knowledge at a match is the post-draft knowledge (cards revealed in another match, even a
   teammate's earlier match against the same opponent, are not carried over); teammates share
   decks; the built deck is every match's game-1 deck (no rebuilding between rounds).
3. **Team values in zero-core**: `result()` = outcome / `T` (the value head is a softmax over
   seats); part 2 adds `SearchGame::team_size` so the search's utility is `T x` the decider's
   value (else `c_puct` and `cq_c_scale` would have to be scaled by hand).
4. **Margin**: the team's match difference / 12 as specified; an alternative auxiliary target is
   the player's own match difference / 3 (more direct credit, still match-derived).
5. **2v2 rounds**: both rotations (4 matches) as DESIGN.md 2.1 had; 1 would be "3 of 4" scaled
   literally (2 matches, many 1-1 draws).
6. **The first-pick prior in drafted matches**: the engine heuristic's weights over the team's
   unseen cards (2.3.6), or a prior learned from the run's own drafts (e.g. how often each card is
   first-picked).
7. **The play network's start**: tabula rasa with the draft network, or from mtg5's best
   checkpoint (itself pure self-play).
8. **Throughput vs signal** (2.3.9): keep mtg-4's match search (~160 events per hour), or cheaper
   matches during co-training.
9. **Draft evaluation's play agent**: heuristic-played matches every iteration plus
   play-network matches every 4th, as proposed.
10. **The uniform determinization** (2.3.2) of unseen cards, or a policy-aware prior later.

### 2.4 Yardsticks (measuring only)

* Elo between draft checkpoints (teams of different checkpoints), with the same play agent.
* Built-in opponent teams: `random`, and a **human-consensus drafter** that takes the card with
  the highest human pick strength (from M1). The AI team must beat it in real matches.
* Curiosity only: agreement with held-out human picks (M1 baselines: random 21.9%, strongest card
  42.3% top-1). Never optimised.

## 3. The Rust rules engine

### 3.1 Principles (lessons from the old crate, rust_engine_reuse.md section 7)

* **Fresh crate, rules-correct.** The old `mtg-bot/rust` is reference only. Where the Python
  engine and the Comprehensive Rules disagree, the CR wins and the difference is written down.
  The Python engine is an imperfect oracle, not a spec.
* **Every choice is an action.** Resolution is resumable: an effect that needs a choice (target,
  mode, X, may, sacrifice, search, discard, order of triggers, damage assignment) suspends, and
  the choice comes back through `step()`. The Python engine has about 400 choice call sites of
  37 kinds; its search sees only 11.
* **Cards are data.** No per-card or per-class bridge. Unknown tags are hard errors; a card is
  supported only when Rust claims every part of it. No silent drops.
* **The game loop lives in Rust.** Priority, steps, SBAs and combat all run in Rust.
* **Tests are Rust tests.** Rules tests with CR expectations (from the old parity-gap catalogue),
  random-game fuzzing for panics and invariants, and a differential harness that drives the
  Python engine with the Rust engine's recorded choices.

### 3.2 Card IR

In mtg-bot: a generic encoder from the parser's frozen dataclasses to JSON (`$t` class tags,
sparse fields; the cube is 0.5 MB) plus a schema dump that generates the Rust `serde` types.
Before freezing IR v1, the string-keyed soft spots are cleaned up (card_ir_survey.md 2.3).
The 193 per-card flags stay a flat struct. (Milestone M3.)

### 3.3 Stages (card_ir_survey.md section 7)

Greedy order: port the card needing the fewest new features next. Feature = primitive class,
flag, keyword or trigger event; 759 in total.

| Stage | Content | Features | Cube cards |
|---|---|---:|---:|
| 0 | Engine core: state, turns, priority, stack, resumable resolution, mana and casting, SBAs, combat + basic keywords, zone moves, core events, targeting | - | basics, vanilla |
| 1 | lands, cantrips, counterspells, burn, simple tutors | 68 | 137 (25%) |
| 2 | common effect combinators, tokens, equipment, initiative/monarch, transform | 184 | 273 (50%) |
| 2b | enough of Stage 3 to reach 360 cards (a full 4v4 draft) | ~300 | 360 (66%) |
| 3 | layers 4/6/7 statics, alternative costs, planeswalkers, copies, delayed triggers | 446 | 437 (80%) |
| 4 | single-card tail, replacement effects, lock pieces, vehicles | 759 | 546 (100%) |

Self-play always uses only supported cards. Size: about 45-50k lines of Python semantics,
roughly 50-70k lines of Rust.

### 3.4 The `mtg` SearchGame

Two players. Actions are decision-relative slots under a fixed u16 cap. Hidden information: the
opponent's hand and library and both libraries' order; in a team draft, partial knowledge of the
opponent's pool: all of it but their first picks, one per pack (2026-10-25), so their deck and
sideboard are sampled from it (best-of-three, M8b part b: the unseen first picks from a prior,
the deck consistent with what the viewer has seen; best-of-one keeps open decklists). The episode is a best-of-three match (2026-10-25, M8 phase 2) with
the play-or-draw choice and sideboarding between games. Shuffles, searches and random discards
are chance events. Fixed-width `encode`
for a start; real strength needs a network that reads the board card by card (section 5).

## 4. Order of work

Rules core (Stage 0), then `mtg` self-play on random decks from supported cards (Stage 1), then
`team_draft` 2v2 on the supported cards with the deferred-result hook, then coverage to 360+
cards and 4v4 PowerLSV, then best-of-3 with sideboarding.

## 5. zero-core work this needs

* **Deferred results**: a game whose terminal result comes from a separate evaluation step (the
  matches), with samples held until the result arrives. Designed in 5.1 (M6 part 2), with the
  co-training of the draft and play networks in one run.
* A set/transformer encoder network: done in M5b (zero-core arch `tokens`, `net.TokenNet`, its
  docs/az.md 10; MTG's object layout `mtgz/az/layout_v4.json` since M8a, v3 before). Also for `team_draft`
  (`layout_draft_v1.json`, 2.3.5), with one addition in part 2: a segment's slots score the action of
  their card's identity (`id_actions`).
* Remote (Mac) workers: zero-core step 2, already planned.

### 5.1 Deferred results and co-training (M6 part 2 design, 2026-10-27)

Generic: nothing in the core names MTG. A **primary** game (`team_draft`) whose episodes end at a
deferred terminal, a **resolver** game (`mtg`) whose episodes produce the result, one network
each, in one run. Single-game runs never touch the new paths.

**The `SearchGame` hook** (defaults keep every game as it is):

```rust
fn deferred(&self) -> bool { false }                 // over (to_move None) but the result comes from outside
fn deferred_payload(&self) -> String { String::new() } // JSON: {"episodes": [<resolver spec JSON>, ...], ...}
fn resolve(&mut self, results: &str) -> Result<(), String> { Err(..) } // the episodes' results, in order
fn team_size(&self) -> usize { 1 }                     // seats per team: result() = team outcome / team_size
```

`team_draft`'s payload lists its matches as `mtg` specs (2.3.6: decks, sideboards, first picks,
what each player knows elsewhere, the match seed); `resolve` takes
`[{"result": [r0, r1], "scores": [...]}, ...]` in the same order.

**Search** (`tree.rs`, `mcts.py`, parity): a state with `to_move() == None && deferred()` is a
leaf evaluated by the network: the leaf is the tree's viewer's `encode`, with zero legal actions
(the protocol allows an empty option segment for such rows; the evaluator returns value and
margin, no priors); the value is stored on the node (as an expanded node's `vec`) and backed up
on every later visit; `result()` is never called there. `team_size() = T`: the backup utility is
`T x win[decider]` (+ the margin term), so Q stays in [0, 1] and `c_puct`, `cq_c_scale` keep their
meaning. Parity tests on a toy team game with a deferred terminal.

**Driver** (`driver.rs`, `worker.rs`, bindings):

* A game reaching a deferred terminal is **parked** with its records and root-search values
  (`rq`); its slot starts the next game (rolling) or the next queued setup. `take_deferred()` ->
  `[(gid, payload)]`; `resolve(gid, results_json)` calls the game's `resolve`, then emits its
  samples exactly as `finish` does today (`z` from `result()`, `q` from `rq`, `sc` from
  `margin()`) and its result row. `pending()` counts parked games. Rolling quotas count resolved
  games. Parked records are float16 in memory (~5 MB per 4v4 event).
* **Queued setups**: `enqueue(tag, spec_json)` makes the next new game of a free slot use that spec
  (parsed per game) instead of the driver's; `queue_only = True` leaves slots idle when the queue
  is empty; result rows carry the `tag`. The seats are the driver's (self-play: both the play
  network).
* Golden digests (toy, Glass Road self-play records) unchanged: no game defines `deferred`, no
  driver enqueues.

**The run** (`zero.run`; settings with defaults that keep today's loop): `resolver` (''), the
resolver's settings `resolver_cfg` (its game, spec, network, search and evaluation settings, as a
nested dict saved in config.json), `resolver_init` (a checkpoint to start the play network from,
or ''), `resolver_conc` (matches in flight), `pending_max` (backpressure). With a resolver:

* **Directories**: the primary at the run root as today (`models/`, `data/`, `elo/`,
  `metrics.jsonl`, `progress.json`: the dashboards and tools work unchanged); the resolver in
  `<run>/resolver/` with the same layout, iteration numbers shared. Checkpoints, optimizer states
  and windows per net.
* **Iteration `i`**: two rolling drivers in the run process, each driven on its own thread
  (`infer.drive` releases the GIL in `next_batch`): the draft driver (`team_draft`, draft net
  `i-1`, recording) and the match driver (`mtg`, play net `i-1`, recording, `queue_only`). A
  coordinator moves each deferred event's matches into the match driver's queue (tag = event,
  match), collects the results by tag and resolves the event when all of its matches are in.
  The draft samples go to `data/iter_i/`, the match samples to `resolver/data/iter_i/`. The
  phase ends when `games` **events** have been resolved in it; events still drafting or waiting
  for matches carry over in the drivers' memory (rolling; a new net is a new search epoch for
  both).
* **Training**: the draft net on its window, then the play net on its own (settings per net);
  **the non-finite guard for both** (shards on write and load, every step, checkpoints and
  optimizer states; `NonFinite` names the net; quarantined shards in `<run>/debug/` and
  `<run>/resolver/debug/`).
* **Evaluation**: the play net as today (the `mtg` evaluation spec, the yardstick, random, Elo
  between play checkpoints) into `resolver/metrics.jsonl`; the draft net (2.3.8): events against
  `consensus` (the anchor), `random` and earlier draft checkpoints, team seats alternating
  between the two agents (`team_size` tells the core which seats are a team), their matches
  played through a second match driver: the heuristic bot every `eval_every` iterations, the
  play net every `draft_eval_net_every`; Elo per net (`elo/`, `resolver/elo/`).
* **progress.json**: `{iter, phase, events_resolved, target, drafting, pending_events,
  matches_queued, matches_in_flight, matches_done}`.
* **Resume**: as today, per iteration: interrupted before both checkpoints, the iteration is
  replayed from its start (both data directories cleared; events and matches in flight
  dropped; matches already played for a dropped event are play data and stay only if their
  iteration completes); interrupted after them, the evaluations finish. The supervisor, PAUSE,
  STOP, kill and fork work on the one process; `fork_run` copies both nets.
* **Bootstrap**: iteration 0 = a fresh draft net and a fresh play net or `resolver_init`.

**Unchanged for single-game runs**: `resolver = ''` is today's loop line for line; the core's
pytest, `smoke_all` (Glass Road, Caverna, Kingdom Builder, MTG smoke runs) and the golden
digests stay green; `checkpoint_compat` for an mtg checkpoint.

## 6. Not doing

No imitation of human picks; no human-derived reward or deck scorer; no Python-to-Rust parity
"flip" campaign; no work on the EoE lineage; no reuse of the old bridge or crate beyond ideas
and edge cases.

## 7. Milestones (stop for the user's go-ahead after each)

| # | Milestone | Status |
|---|---|---|
| M1 | Human pick data + human pick strength (yardsticks only) | done (branch m1-draft-data) |
| M3 | Card IR v1 in mtg-bot + generated Rust types | in progress |
| M4 | Rust Stage 0: full games between vanilla/basic decks, every choice an action, fuzzed | next |
| M5 | Stage 1 cards + `mtg` SearchGame on zero-core + play self-play on random supported-card decks + differential harness | M5a done (branch m5a-zero): `mtg` on zero-core with 191 cards, fixed-width encoding, decks, bots, first self-play run (ENGINE.md 21) |
| M5b | A network that reads the board card by card (in zero-core), replacing M5a's fixed-width encoding | done (branch m5b-run2): zero-core arch `tokens` (a transformer over the encoding's object slots), stable card identities (layout v2), minor decisions, eval specs, cheaper Elo; runs/mtg2 (tokens, 5 iterations) vs runs/mtg2c (MLP control, 10): ENGINE.md 22 |
| M6 | Team cube drafting end to end (decisions of 2026-10-27, 2.3): `team_draft` (4v4 PowerLSV, 2v2 for tests) with learned deckbuilding, the deferred-result hook and co-training in zero-core (5.1), the yardsticks; parts in 7.1 | part 1 (branch m6-team-draft): the design (2.3, 5.1, 7.1) |
| M7 | Coverage to 360+ cards, 4v4 PowerLSV team drafts; then best-of-3 + sideboarding; Stages 3-4 | M7a threats-first coverage (branch m7a-threats): 360 cards, 113 creatures (ENGINE.md 23). M7b part 1 planeswalkers (branch m7b-walkers): 378 cards, 18 of the 20 planeswalkers, attacking planeswalkers, emblems (ENGINE.md 24). M7b part 2 faces (branch m7b-faces): 394 cards, transforming and modal double-faced cards, Adventures, Sagas, day and night, a split card (ENGINE.md 25). M7b part 3a copies (branch m7b-copies): 410 cards, copy effects, token copies, copies of spells, entering as a copy, squad, offspring, replicate, spree, bestow, prepare (ENGINE.md 26). M7b part 3b the stack (branch m7b-stack): 438 cards, 155 creatures, spells and abilities on the stack as targets, "becomes the target", ward, improvise, the initiative and Undercity, abilities activated from graveyards, storm, mobilize; the total cost of 601.2f and reductions on alternative costs fixed (ENGINE.md 27); the M7b card batch ends. M7c tail part 1 (branch m7c-tail1): 467 cards, 168 creatures: mana abilities with a mana cost or a choice (with priority), exhaust, spending restrictions, converge, a painted colour, incubate, turn skipping, granted leave triggers (earthbend, Serra Paragon), a target linked to another's controller (ENGINE.md 28); Chromatic Star blocked by an IR defect, Memory Jar and Headliner Scarlett by face-down exile, Rain of Filth by a granted mana ability. M7c tail part 2 (branch m7c-tail2): 491 cards, 184 creatures: the cards needing two features: cascade, miracle, warp, boast, ascend, airbend and waterbend, harness, teamwork, additional combat phases, Yawgmoth's Will, X in activation costs and "X targets", X life, "triggers only once each turn" (ENGINE.md 29); Forth Eorlingas! and Green Sun's Zenith blocked by IR defects, Abhorrent Oculus by face-down permanents, Shelldock Isle by face-down exile. M7c tail part 3 (branch m7c-tail3): 524 cards, 201 creatures: the cards needing three or more features: face-down exile (a hidden exile zone: Headliner Scarlett, Memory Jar, Shelldock Isle), splice, kicker on permanents (multikicker, kicker {X}), "put into a graveyard from anywhere", infect, annihilator, draw replacements, reflexive "you may" triggers, a player's protection, reanimating Auras, saddle, fading, graveyard triggers, X life removal (ENGINE.md 30); 12 still blocked (face-down permanents, Class levels, reconfigure, plot, emerge, exert, Citadel, Dauthi, Nadu, Urza, Peacekeeper, Gemstone Caverns), Wizard's Rockets by an IR defect. M7d (branch m7d-last): 537 cards, 208 creatures: the last 13 cards needing subsystems: face-down permanents and manifest dread, plot and special actions, an opening-hand action, shadow and void counters, reconfigure, Class levels, a granted target trigger limited per creature, a chosen card name and its taxes, a mana ability that taps another permanent, exert and haste mana, emerge, a top card known and cast for life, a granted sacrifice mana ability used with the land's own (ENGINE.md 31); the 9 cards still blocked are IR defects. M7d's IR fixes (branch m7d-ir): the IR of mtg-bot feature/ir-fixes-mtgzero 8ea9fcca fixes them: 546 cards, the whole cube, 209 creatures: reflexive triggers with an intervening "if", "this turn" triggered abilities, a spell shuffling itself into its library, mana abilities with {X} (ENGINE.md 32) M7e part 1 (branch m7e-rules): the documented rules shortcuts removed, tracked in docs/shortcuts.md: the monarch's and Okoye's triggers on the stack, "one or more" triggers once per event, the order of replacement effects chosen by the controller (616.1), Mox Diamond however it enters, entering with counters, Metamorph not copying what enters with it, a granted escape or retrace with a card's own additional costs, the permission a graveyard card is cast with chosen (Paragon, Lurrus, flashback, Yawgmoth's Will), the Citadel's faces, last known information of a damage source, saddle and teamwork, turning face up after losing abilities, Necromancy's flash permission, exact converge, Equipment timestamps, granted triggers; two decision kinds (ENGINE.md 33); the payment model is part 2's. M7e part 2 (branch m7e-pay): the payment model's shortcuts removed (docs/shortcuts.md): the player chooses which pool mana pays (PayPool), mana abilities with a mana cost or a sacrifice of another permanent while paying any cost, Pentad Prism once per counter, a tap and a sacrifice of one land in one payment whatever the tap's condition, mana abilities activated before the costs are paid (a Lotus Petal delved or exiled for escape, X life after a painland's damage); one decision kind (ENGINE.md 34) |
| M8 | Matches and sideboarding (decisions of 2026-10-25, section 0): a best-of-three match as the `mtg` episode, sideboarding, the opponent's pool known but their first picks, layout v4, a new run | M8a, phase 1 (branch m8a-companion): a sideboard per player (cards outside the game, hidden but a revealed companion), the play-or-draw choice (CR 103.1; the chooser a spec parameter), companions (702.139: revealed before the hands, the {3} special action) with their deck conditions in the IR (mtg-bot feature/companion-ir), self-play decks with sideboards and companions, layout v4 started (ENGINE.md 35). M8b part a (branch m8b-match): the `mtg` episode a match: `"format": "bo1"` (the game of before) or `"bo3"` (two wins or three games, drawn games count, a match can be drawn; the next game's chooser the loser, after a draw the previous chooser, CR 103.1), sideboarding between games (two decision kinds over the pool list in increasing order: every legal deck of 40-60 cards reachable once), seeded 45-card pools with three first picks and a heuristic deckbuilder, the heuristic keeps its deck, layout v4's match block, pool list and two bags, preset `mtg-4`; open decklists kept, with the list of what part b (the hidden pool) changes (ENGINE.md 36). M8b part b (branch m8b-hidden): the hidden pool in best-of-three: `determinize` samples the opponent's unseen first picks (a colour-weighted prior over the cards known to be elsewhere) and their deck and sideboard within the pool, consistent with the public sizes, what the viewer has seen this game and before, its clouds and a revealed companion; the first sideboarder's deck resampled for the second; the companion question asked of every player; the bags read only what the viewer knows; generated pools disjoint (singleton cube); best-of-one keeps open decklists; `temp_moves` kept at 60 (ENGINE.md 36.11-36.12). M8d part 1 (the cube's official rulings as engine tests: 1,607 fetched and triaged, three engine faults fixed; ENGINE.md 37). M8d part 2 (branch m8d-interact): interaction confidence: 77 tests of the riskiest rulings left and of the interaction matrix's cells (`docs/interactions.md`: 16 mechanics, every reachable pair tested or n/a but two), a combo-dense fuzz mode (`FUZZ_POOL=interactions`); seven engine faults found and fixed (exile-until one card at a time, combat damage one dealt-damage event per recipient, a split card with two graveyard permissions, Bolas's Citadel's life kept for additional mana costs, Containment Priest and a card entering as a copy, an instant or sorcery card can't enter the battlefield, a Lotus Petal exiled by Yawgmoth's Will as a graveyard exile cost) (ENGINE.md 38). M8d part 3 (branch m8d-rulings2): the rulings triage narrowed (990 untested class-a texts decided: 181 already tested, 214 reclassified b or c, 595 to test) and 122 tests of the replacement, copies, stack, triggers, protection, dfc and walkers rulings and of the matrix's last two cells; ten engine faults fixed (an attached reconfigure permanent's creature types, an Aura entering without being cast, CDAs in every zone, Tezzeret's emblem keeping subtypes, Bolas's Citadel's new top card unseen mid-cast, a reflexive trigger per token, last known power and saddled, a new Nadu's triggers, Bilbo casting an Adventure); 913 rulings (726 distinct) tested, 251 distinct texts left in zones, combat, mana and other (ENGINE.md 39). M8d part 4 (branch m8d-rulings3): the last 251 distinct texts (zones, combat, mana, other) decided: 8 already tested, 1 reclassified, 242 tested by 120 new tests; eight engine faults fixed (Lurrus's and Serra Paragon's mana value limit with X, alternative costs with a permission to cast from exile or a graveyard, escape exiling the cards its own costs discard or sacrifice, a card's two flashback costs, a latent mana ability for Squandered Resources, either face for casts during a resolution, Thieving Skydiver attaching Lion Sash, Gut's "another" from the IR); every class-a ruling tested: 1,225 (976 distinct), none deferred (ENGINE.md 40) |

(The old M2, a draft game scored by a human-derived deck evaluator, was dropped on 2026-10-21.)

### 7.1 M6 parts (a stop for the user after each)

| Part | What | Stop |
|---|---|---|
| 1 | The design (2.3, 5.1); the `team_draft` game in Rust (`engine/src/az/draft*.rs`, registered next to `mtg`): 4v4 and 2v2, the packs, picks, the information model, learned deckbuilding, `determinize`, `encode` and `layout_draft_v1.json`, keys and `force_chance`, the deferred terminal, the bots `random` and `consensus`, a test-only utility that resolves an event with heuristic-played matches; tests and measurements (ENGINE.md 41) | the user approves or amends the design |
| 2 | zero-core: the deferred hook (search leaf, driver parking and `resolve`, queued setups, `team_size`), the co-training loop (two networks, directories, windows, the non-finite guard for both, progress, resume, evaluation and Elo of both), a toy team game with a deferred terminal for the core's tests, parity, `smoke_all` green for Glass Road, Caverna, Kingdom Builder and MTG; then the pin in mtg-zero | the user reviews the core change |
| 3 | Integration: `mtg` matches from drafts (spec keys `pN_first_picks`, `pN_elsewhere`; digests unchanged), `TokenNet` actions by identity (`id_actions`), the co-training presets (a 2v2 smoke, the 4v4 run), the draft yardsticks (draft Elo, `consensus` and `random` teams, held-out human-pick agreement), the search smoke for `team_draft`, a smoke run | the user's go-ahead for the first run |
| run | The first co-training run (4v4 PowerLSV) | |

