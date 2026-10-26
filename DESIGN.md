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
  the 360 cards a 4v4 draft uses.
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

### 2.3 The `team_draft` SearchGame

* Simultaneous picks are modelled as sequential decisions within a pick round, seat 0..7, each
  hidden from the others (Glass Road Zero's simultaneous-choice pattern, its docs/search.md 9).
* Draft action = the picked card's index in the cube (`NUM_ACTIONS` cap such as 1024); legal =
  the cards in the seat's current pack. Deckbuilding actions = include / exclude decisions over
  the pool and basic-land counts (exact encoding to be designed with the network).
* Chance and revelation keys: a pack's contents when opened; its remainder each time it reaches
  a seat; teammates' information at the end of the draft; match results.
* `determinize(viewer)`: resample everything the viewer has not seen (unopened packs, unseen
  parts of packs in flight, the opponents' pools) consistent with what the viewer and, after the
  draft, its teammates saw.
* `encode(viewer)`: own pool, current pack, cards seen and passed per direction, pack and pick
  number, colour and curve summary; after the draft, teammates' pools and the known parts of
  opponents' pools.

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
sideboard are sampled from it. The episode is a best-of-three match (2026-10-25, M8 phase 2) with
the play-or-draw choice and sideboarding between games. Shuffles, searches and random discards
are chance events. Fixed-width `encode`
for a start; real strength needs a network that reads the board card by card (section 5).

## 4. Order of work

Rules core (Stage 0), then `mtg` self-play on random decks from supported cards (Stage 1), then
`team_draft` 2v2 on the supported cards with the deferred-result hook, then coverage to 360+
cards and 4v4 PowerLSV, then best-of-3 with sideboarding.

## 5. zero-core work this needs

* **Deferred results**: a game whose terminal result comes from a separate evaluation step (the
  matches), with samples held until the result arrives.
* A set/transformer encoder network: done in M5b (zero-core arch `tokens`, `net.TokenNet`, its
  docs/az.md 10; MTG's object layout `mtgz/az/layout_v4.json` since M8a, v3 before). Maybe also for `team_draft`.
* Remote (Mac) workers: zero-core step 2, already planned.

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
| M6 | `team_draft` (2v2) + deferred-result hook in zero-core + co-training + yardsticks | |
| M7 | Coverage to 360+ cards, 4v4 PowerLSV team drafts; then best-of-3 + sideboarding; Stages 3-4 | M7a threats-first coverage (branch m7a-threats): 360 cards, 113 creatures (ENGINE.md 23). M7b part 1 planeswalkers (branch m7b-walkers): 378 cards, 18 of the 20 planeswalkers, attacking planeswalkers, emblems (ENGINE.md 24). M7b part 2 faces (branch m7b-faces): 394 cards, transforming and modal double-faced cards, Adventures, Sagas, day and night, a split card (ENGINE.md 25). M7b part 3a copies (branch m7b-copies): 410 cards, copy effects, token copies, copies of spells, entering as a copy, squad, offspring, replicate, spree, bestow, prepare (ENGINE.md 26). M7b part 3b the stack (branch m7b-stack): 438 cards, 155 creatures, spells and abilities on the stack as targets, "becomes the target", ward, improvise, the initiative and Undercity, abilities activated from graveyards, storm, mobilize; the total cost of 601.2f and reductions on alternative costs fixed (ENGINE.md 27); the M7b card batch ends. M7c tail part 1 (branch m7c-tail1): 467 cards, 168 creatures: mana abilities with a mana cost or a choice (with priority), exhaust, spending restrictions, converge, a painted colour, incubate, turn skipping, granted leave triggers (earthbend, Serra Paragon), a target linked to another's controller (ENGINE.md 28); Chromatic Star blocked by an IR defect, Memory Jar and Headliner Scarlett by face-down exile, Rain of Filth by a granted mana ability. M7c tail part 2 (branch m7c-tail2): 491 cards, 184 creatures: the cards needing two features: cascade, miracle, warp, boast, ascend, airbend and waterbend, harness, teamwork, additional combat phases, Yawgmoth's Will, X in activation costs and "X targets", X life, "triggers only once each turn" (ENGINE.md 29); Forth Eorlingas! and Green Sun's Zenith blocked by IR defects, Abhorrent Oculus by face-down permanents, Shelldock Isle by face-down exile. M7c tail part 3 (branch m7c-tail3): 524 cards, 201 creatures: the cards needing three or more features: face-down exile (a hidden exile zone: Headliner Scarlett, Memory Jar, Shelldock Isle), splice, kicker on permanents (multikicker, kicker {X}), "put into a graveyard from anywhere", infect, annihilator, draw replacements, reflexive "you may" triggers, a player's protection, reanimating Auras, saddle, fading, graveyard triggers, X life removal (ENGINE.md 30); 12 still blocked (face-down permanents, Class levels, reconfigure, plot, emerge, exert, Citadel, Dauthi, Nadu, Urza, Peacekeeper, Gemstone Caverns), Wizard's Rockets by an IR defect. M7d (branch m7d-last): 537 cards, 208 creatures: the last 13 cards needing subsystems: face-down permanents and manifest dread, plot and special actions, an opening-hand action, shadow and void counters, reconfigure, Class levels, a granted target trigger limited per creature, a chosen card name and its taxes, a mana ability that taps another permanent, exert and haste mana, emerge, a top card known and cast for life, a granted sacrifice mana ability used with the land's own (ENGINE.md 31); the 9 cards still blocked are IR defects. M7d's IR fixes (branch m7d-ir): the IR of mtg-bot feature/ir-fixes-mtgzero 8ea9fcca fixes them: 546 cards, the whole cube, 209 creatures: reflexive triggers with an intervening "if", "this turn" triggered abilities, a spell shuffling itself into its library, mana abilities with {X} (ENGINE.md 32) M7e part 1 (branch m7e-rules): the documented rules shortcuts removed, tracked in docs/shortcuts.md: the monarch's and Okoye's triggers on the stack, "one or more" triggers once per event, the order of replacement effects chosen by the controller (616.1), Mox Diamond however it enters, entering with counters, Metamorph not copying what enters with it, a granted escape or retrace with a card's own additional costs, the permission a graveyard card is cast with chosen (Paragon, Lurrus, flashback, Yawgmoth's Will), the Citadel's faces, last known information of a damage source, saddle and teamwork, turning face up after losing abilities, Necromancy's flash permission, exact converge, Equipment timestamps, granted triggers; two decision kinds (ENGINE.md 33); the payment model is part 2's. M7e part 2 (branch m7e-pay): the payment model's shortcuts removed (docs/shortcuts.md): the player chooses which pool mana pays (PayPool), mana abilities with a mana cost or a sacrifice of another permanent while paying any cost, Pentad Prism once per counter, a tap and a sacrifice of one land in one payment whatever the tap's condition, mana abilities activated before the costs are paid (a Lotus Petal delved or exiled for escape, X life after a painland's damage); one decision kind (ENGINE.md 34) |
| M8 | Matches and sideboarding (decisions of 2026-10-25, section 0): a best-of-three match as the `mtg` episode, sideboarding, the opponent's pool known but their first picks, layout v4, a new run | M8a, phase 1 (branch m8a-companion): a sideboard per player (cards outside the game, hidden but a revealed companion), the play-or-draw choice (CR 103.1; the chooser a spec parameter), companions (702.139: revealed before the hands, the {3} special action) with their deck conditions in the IR (mtg-bot feature/companion-ir), self-play decks with sideboards and companions, layout v4 started (ENGINE.md 35) |

(The old M2, a draft game scored by a human-derived deck evaluator, was dropped on 2026-10-21.)
