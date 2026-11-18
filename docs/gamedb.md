# The game database (`tools/gamedb.py`)

Every game of a run - its self-play, its evaluations, its yardsticks, its publisher's - and of the standalone tools that
play games, replayed through the engine's play-by-play, in Parquet tables that DuckDB queries in well under a second.
Design and measurements: `docs/ENGINE.md` 187 (self-play) and 190 (every other game, the engine identity). This page is
the schema and the queries: enough to write SQL against it cold.

## Pipeline

1. **Records** (zero-core's run setting `game_records` 1, zero-core `docs/az.md` 59): every self-play game an iteration
   accepts, from every executor (this machine's driver, the remote workers, the farm pool), is one JSON line in
   `<run>/records/iter_NNNN/<source>-<pid>.jsonl.gz`: its seed, every action, the spec (by key), the setup (the decks
   and the metagame's pairing), the outcome, the resignation, the networks. Outside the training window: never rolled
   off by `keep_data`. `DONE.json` closes an iteration. **Every other game** the run plays with the same setting (the
   kinds below) is a record of the same format plus its `kind`, its `players` (per seat) and `meta`, in
   `<run>/records/<kind>/iter_NNNN/*.jsonl.gz` (one whole file per match or batch); a standalone tool's with `--records
   DIR` in `DIR/<kind>/misc/`. The publisher's replays (`<run>/replays/`) are indexed as they are (no copy). Every
   records file names the engine that played it (below: "The engine").
2. **Ingest** (`python tools/gamedb.py ingest RUN`; a standalone tool's: `ingest --records DIR --db DB`): each new unit
   - a complete self-play iteration, a records file, a replay - replayed game by game on the CPU at below-normal
   priority, written to `<db>/<table>/iter=NNNN/part.parquet` (self-play), `<db>/<table>/m=<kind>.<sub>/<file>.parquet`
   (a records file) or `r=publisher/<file>.parquet`. Incremental and idempotent (`<db>/state.json`).
   Every game gets a `games` row (light replay); a deterministic share `--sample` (default 1.0: all) is replayed in
   full: `events` and the combo tables. The full replay reads each step with `tools/combo_stats.py`'s Reader, the code
   `combo_stats.py scan` runs, so the combo tables are its numbers (validated game by game: ENGINE.md 187.4).
3. **Query**: `python tools/gamedb.py query RUN "SQL" [--csv|--json]`, `python tools/gamedb.py shell RUN`, or from
   Python `gamedb.connect(db)` (a DuckDB connection with every table and view below).
4. **Retention**: `python tools/gamedb.py prune RUN --keep-iters 64 --old-sample 0.1 [--delete-raw]` (self-play
   iterations; the other kinds are kept whole). **The raw records are kept by default** (the user's choice: no
   `--delete-raw` unless asked; they are what replays a game on its own engine). Their cost on disk: ~150 bytes a
   self-play game (148 measured, ENGINE.md 187.4) - the metagame run's ~61,000 games an hour are ~9 MB an hour, ~0.2 GB a
   day -; an evaluation or yardstick game (played out, no resignation) ~230 bytes (ENGINE.md 190.5): its ~1,000 a day
   (the anchors' 100 every 16 iterations, the cells' 400-1,200 a measured generation) are well under 1 MB a day.
   `gamedb.py status RUN` prints the raw records' size.

The database of run R: `$MTGZ_GAMEDB/R`, else `<ZERO_BIG_DISK>/mtg-zero-gamedb/R`, else `~/.cache/mtg-zero-gamedb/R`
(`--db DIR` overrides). Needs `pip install duckdb pyarrow`.

## Kinds

| kind | played by | `iter` | players (`p0_*` / `p1_*`, per seat) |
|---|---|---|---|
| `selfplay` | the run's self-play (every executor) | the accepting iteration | both the network its samples were searched with ('<run>/iter_NNNN' of `net_hi`) |
| `eval` | the run's evaluation: the yardstick and random games (zero-core `Run.evaluate`; on the Mac with `eval_remote`, copied back) | the evaluated iteration | the checkpoint ('iter_NNNN') and the bot |
| `elo` | the Elo matches against older checkpoints (`elo_games` > 0) | the evaluation's iteration | both checkpoints ('iter_NNNN') |
| `anchor` | the fixed anchors' games (`elo_anchors`) and an anchor's calibration | the evaluation's iteration | the checkpoint and the anchor ('anchor:<label>') |
| `ref` | the reference games (random vs the yardstick bot) | the evaluation's iteration | both bots |
| `cells`, `cells_gen` | the yardsticks' metagame cells (local or farm) against a reference / the previous generation | the measured checkpoint | the checkpoint ('iter_NNNN') and the reference (its label: 'gen0', 'iter_0016') |
| `cells_base` | a cells baseline (the reference against itself) | none | the reference twice |
| `bo1` | the yardsticks' best-of-one against a fixed opponent (local or farm) | the measured checkpoint | the checkpoint and the opponent (its name) |
| `combo` | `tools/combo_stats.py play --records DIR`; the publisher's combo pass (the run's `records/combo/iter_NNNN`) | the generation (publisher), none | the checkpoint at each player's simulations |
| `combo_selfplay` | `combo_stats.py selfplay --records DIR` (the forced find on) | none | as `combo` |
| `h2h` | `python -m mtgz.az.h2h A B --records DIR` (any zero.h2h match; `--records-kind` names another) | none | A and B (`--label-a`, `--label-b`) |
| `keep_h2h` | `python -m mtgz.az.keep_h2h ... --records DIR` (chained best-of-three: kept, not replayed here) | none | A and B |
| `publisher` | the delverai publisher's recorded games (`<run>/replays/`, recorded with `game_records`) | the recorded generation | the replay's seats |

A player is `{ckpt, sha, label, sims, rule}`: `ckpt` '<run>/iter_NNNN' (a run's checkpoint) or the file's name (an
anchor's, a frozen reference's), `sha` sha256[:12] of the file - the identity across machines (the Mac's mirror names an
anchor's file by its label) -, `label` what the code that played it calls it, `sims` and `rule` its simulations and
move rule; a bot: `ckpt` NULL, its name as the label. `meta` (JSON) is the code's own: the driver seed, `paired`, a
cells match's `pair` and `chooser`, a batch's `batch` / `gid_offset`, `backend` 'farm'. `executor` is 'local', 'remote'
(the Mac), 'farm' (a farm play job's traced game, or the pool's self-play), 'publisher' or 'combo'.

Not recorded (no whole game to keep): the canary library (`blunders`: positions searched, not games played), the cells'
joint fit (no games), a cached cells baseline (its games were recorded when they were played). zero-core's standalone
`zero.farm_h2h` (its eval jobs run without `eval_trace`: the farm returns no moves; the yardsticks' farm jobs, by
`mtgz.az.farm_campaign`, trace every game and are recorded). A chained contest (a best-of-three match: `keep_h2h`, a
best-of-three run's evaluation) is recorded but not replayed by the database (its steps hold episode boundaries; ingest
says how many it skipped).

## The engine

A record replays only on the engine that played it: an engine change (a rule, a card, the play-by-play) makes an older
game replay differently or not at all. Every records file starts with a build line (zero-core `docs/az.md` 59.4):
`commit` (the mtg-zero commit), `core` (the zero-core commit), `engine` (sha256[:16] over the blob ids of every Rust input
- `mtgz.az.ENGINE_PATHS`, the artifact cache's: engine/, card_ir/, py/, worker/, data/, the Cargo files - and zero-core's
crates: a Python or docs commit keeps it, any engine change moves it; 'dirty' when built from uncommitted engine
changes), `module` (the engine module's sha256). `python tools/gamedb.py engine` prints this checkout's.

`ingest` replays only the units of this checkout's engine. A unit of another engine is **refused loudly**: the log names
it with the commits that made it, `state.json` keeps it under `refused`, `status` lists them, and the command exits 3.
To ingest them, use a checkout of that engine:

```bash
git worktree add --detach ../mtgz-engine-<sha12> <the commit the refusal names>
(cd ../mtgz-engine-<sha12> && git submodule update --init && bash tools/build_py.sh)   # (or tools/zcache.py fetch pyd)
python tools/gamedb.py ingest RUN --engine-checkout ../mtgz-engine-<sha12>
```

`--engine-checkout PATH` asks PATH's gamedb for its engine id and hands it the refused units of that engine (`python
PATH/tools/gamedb.py ingest RUN --db <the same db> --only-engine <id>`): they are replayed by the engine that played them
and written into the same database (its tables read by name: a later checkout's columns are NULL in an earlier one's
files). The checkout must be at or after this page's ENGINE.md 190 (the build lines exist from there). So records made
before an engine change - the play-by-play export in view.rs, Mishra's Bauble remembering what it saw - stay ingestible
after it: run the ingest of the new commit, then `--engine-checkout` a checkout of the old one (`refused` names its
commit). A unit without an engine id (records of zero-core 5c2d3f39, before the build lines) is refused unless
`--allow-unidentified`, from a checkout of the engine that played them; a replay without one (recorded without
`game_records`) is not a record and is skipped (counted in the log).

Conventions: seats are 0 and 1 (`deck0` plays seat 0); `turn` is the game's turn number (both players' turns
counted, as the engine and the play-by-play count them); card names are the engine's (a split / double-faced card by
its full name, `Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar`); lists are DuckDB `VARCHAR[]` (1-based:
`pile[1]` is the top card). `iter` is the iteration whose quota accepted the game (its samples trained iteration
`iter`); `net_lo` / `net_hi` the checkpoints its search used (iter - 1, or older for a lingering / farm game).

## Tables

**games** - one row per game (every game).

| column | meaning |
|---|---|
| game_id | BIGINT, unique (a hash of iteration, source, game id, seed, actions; another kind's: of its unit, game id, seed, actions) |
| iter | self-play: the accepting iteration; another kind's: see "Kinds" (NULL for a standalone tool's) |
| kind | the game's kind (above: `selfplay`, `eval`, `elo`, `anchor`, `ref`, `cells`, `cells_gen`, `cells_base`, `bo1`, `combo`, `h2h`, `keep_h2h`, `publisher`, ...) |
| unit | its unit (`iter=NNNN`, `m=<kind>.<sub>/<file>`, `r=publisher/<file>`) |
| engine | the engine id that played it (and replayed it here) |
| p0_ckpt, p1_ckpt | each seat's player: its checkpoint id (NULL: a bot) |
| p0_label, p1_label | each seat's player's label ('iter_0120', 'gen0', 'anchor:fork0', 'heuristic') |
| p0_sha, p1_sha | each seat's checkpoint file's sha256[:12] |
| p0_sims, p1_sims, p0_rule, p1_rule | each seat's simulations and move rule |
| meta | JSON: the record's `meta` (the code's own: pairing, chooser, driver seed, batch, backend) |
| src, executor | the record's source ('local', 'w0', 'mac/w1', 'pool:<farm run>', 'remote', 'farm', 'publisher'); executor 'local' / 'remote' / 'farm' / 'publisher' |
| machine | a farm game's machine as a short hash (never the machine id) |
| gid, seed, spec | the driver's game id, the game's seed (text: a u64), the spec's key |
| deck0, deck1 | each seat's decklist (data/decks names, e.g. `doomsday_8027957`) |
| net_lo, net_hi | the networks its samples were searched with |
| forced_find_p | the Doomsday wheel's share in the game's spec (ENGINE.md 180), NULL without it |
| over, aborted, resigned | ended by the rules or a resignation; aborted; ended by self-play resignation |
| starting | the seat on the play |
| turns, decisions, n_actions | the game's turn number at its end; decisions with two or more options; actions |
| mull0, mull1 | each seat's mulligans |
| life0, life1 | life at the end |
| winner | the winning seat, NULL for a draw / unfinished game |
| reason | the engine's end reason (`Sba`, `Effect`, `TurnLimit`, ...; `Resigned`) |
| method | how it ended: `oracle` (the winner's Thassa's Oracle trigger), `pact` (the loser's own Pact of Negation trigger), `combat` / `damage` (0 life in / outside combat damage), `decking`, `turn_cap`, `resigned`, `unfinished`, `effect`, `sba_other`, `draw_<reason>` |
| score0, score1 | the result per seat (1 / 0 / 0.5) |
| detailed | replayed in full (its events and combo rows exist) |
| last_attackers | JSON {turn, attackers} of the winner's last attack (detailed games) |
| lines_lost | play-by-play lines a state did not carry (detailed games): 0 since the engine exports every line a step writes (ENGINE.md 191), a check; before it a state carried the newest 40 and a step that wrote more (Doomsday exiling a library, ~50 lines) lost its oldest, ~6 a real self-play game |

**events** - one row per play-by-play line and per choice of a detailed game.

| column | meaning |
|---|---|
| game_id, iter, seq | the game; the event's order in it |
| action | the index of the action that produced it (-1: before the first action, the opening hands) |
| turn, step | the turn and the step name (`Main1`, `Upkeep`, `DeclareAttackers`, `Setup`, ...) |
| player | the seat (for a zone move: the seat whose destination zone holds the card, when exactly one does) |
| type | `cast`, `land` (a land played), `activate`, `mana` (a mana ability), `move` (a zone change), `find` (a search's pick), `trigger` (a trigger put on the stack), `counter` (a spell countered), `attack`, `look`, `choose`, `turn` (a turn begins), `forced_find` (the wheel's draw line), `empty_draw`, `mulligan` (detail `n`: the mulligan's number), `damage` (detail `amount`; `card` the damaged object, or `player` the damaged seat with detail `player` true), `target` (a spell or ability's target as it is chosen: `card`, or `player`), `life_half` (Doomsday's cost: detail `amount`), `fizzle`, `wins`, `lines_lost` (detail `n`: the next state did not carry `n` of a step's lines, which are not in `events`; never since ENGINE.md 191, a check - an engine before it exported a state's newest 40 lines, and a resolving Doomsday exiling the rest of a library lost its oldest; `combo_stats` reads the same lines, and the `doomsday` rows come from the state, not the lines), `choice` (a decision with two or more options other than priority and mana payment: mulligans, targets, modes, piles, Brainstorm's put-backs, ...), `other` |
| card | the card the line names (cast / land / activate / mana / find / move / trigger / counter / attack: the attacker); a choice's chosen option text |
| from_zone, to_zone | a move's zones (`Library`, `Hand`, `Battlefield`, `Graveyard`, `Stack`, `Exile`); a cast's zone it was cast from (`Hand`, `Graveyard`, ...) |
| detail | JSON: a choice's `kind` (`Mulligan`, `ChooseTarget`, `SearchLibrary`, `PutOnTop`, ...) and `n_options` (and `source` while a spell resolves); an activation's `ability`; the forced find's `class` and `drawn`; an `other` line's `text` |

**doomsday** - one row per resolved Doomsday (detailed games): caster, turn, `pile` (the library after it, top first),
`pile_size`, `library`, `choices` (the option texts that built it), `hand` (the caster's hand after it), `life`,
`oracle_in_pile`, `oracle_in_hand`, `oracle_where` (where the caster's Oracle was as it began to resolve: `hand`,
`graveyard`, `library`, `gone`), `oracle` (the class: `voluntary` the pile holds it by the network's choice, `forced`
the wheel's forced find chose it, `in_hand`, `not_findable`, `missed`), `ff_drawn` (the wheel's draw; NULL without the
wheel).

**shows** - one row per resolved Show and Tell: caster, turn, `put_caster` / `put_opponent` (the cards each put onto
the battlefield, sorted; `[]` nothing; NULL unknown).

**spells** - every cast of the tracked combo cards (Doomsday, Thassa's Oracle, Show and Tell, Sneak Attack, Omniscience,
Emrakul, Griselbrand, Atraxa): player, card, turn, `outcome` (`resolved`, `countered`, NULL still on the stack at the
end), `countered_by`.

**sneak** - Sneak Attack activations (player, turn). **entered** - fatties and Omniscience entering the battlefield:
player, card, turn, `how` (`show`, `sneak`, `cast`, `other`).

**bauble** - one row per activation of Mishra's Bauble or Urza's Bauble (detailed games; `tools/gamedb_bauble.py`,
ENGINE.md 194). Every condition is the replayed engine state's before the action that activated it (the spectator export
and its `known` block: the engine's exact hidden-information state, `Obj.known`); the target is the engine's "target
P<n>" line after it.

| column | meaning |
|---|---|
| game_id, iter, idx | the game; the activation's order in it |
| action, turn, step | the index of the activating action (its target decision is the next one), the turn and step |
| activator, own_turn | the seat that activated it; on its own turn |
| bauble, target | `urza` / `mishra`; `self` / `opponent` |
| opp_targetable | the target decision offered the opponent (false: the engine answered a one-target decision alone) |
| opp_hand, opp_hand_known, opp_hand_all_known | the opponent's hand size, how many of its cards the activator knows (exactly), all of them |
| opp_library, opp_top_known, own_top_known | the opponent's library size; the activator knows the opponent's top card (a remembered look: ENGINE.md 192; on an engine before it only when the owner knew it too) / its own top card |
| fetch_in_hand, fetch_on_battlefield, fetch_available | the activator's fetchlands (the sacrifice-and-search lands, `FETCHLANDS`) in hand / on its battlefield (uncracked, tapped or not); either |
| manipulation_in_hand, brainstorm_in_hand, ponder_in_hand | the cards in its hand that see or rearrange its own top card (Brainstorm, Ponder, Preordain, ..., `LIBRARY_MANIPULATION`); Brainstorm / Ponder among them |
| excluded | why the opponent could not be targeted, or meaninglessly (`opponent_not_targetable`, `opponent_library_empty` for Mishra's), or the game stopped before the target was chosen (`game_ended_before_target`: a resignation at the target decision; `target` NULL); NULL for a counted activation |
| blunder, blunder_reason | the user's rules (below); NULL when excluded |

## Views

| view | one row per | columns |
|---|---|---|
| game_results | game x seat | game_id, iter, kind, seat, deck, opp_deck, player, player_label, opp_player, opp_label, on_play, score, outcome (win / loss / draw), method, turns, resigned, executor, mulligans |
| mulligans | game x seat | game_id, iter, kind, seat, deck, opp_deck, on_play, mulligans, score, outcome |
| doomsday_piles | resolved Doomsday | game_id, iter, kind, idx, caster, caster_deck, caster_label, opp_label, opp_deck, turn, pile, pile_size, card1..card5 (top first), oracle (class), oracle_in_pile, oracle_in_hand, oracle_where, oracle_place (`in_pile` / `in_hand` / `not_findable` / `missed`), forced (the wheel chose the Oracle), ff_drawn, forced_find_p, life, hand, choices, won (the caster won the game), win_method (its method when it won), method, turns |
| doomsday_pile_cards | card of a pile | game_id, iter, kind, idx, caster, caster_deck, opp_deck, oracle, forced, won, win_method, pos (1 = top), card |
| casts | cast | game_id, iter, kind, seq, turn, step, player, card, cast_from, deck, opp_deck, won |
| show_and_tell | resolved Show and Tell | game_id, iter, kind, idx, caster, caster_deck, opp_deck, turn, put_caster, put_opponent, caster_pick / opponent_pick (the first of each list), won, method |
| sneak_attack | Sneak Attack activation | game_id, iter, kind, idx, player, deck, turn, sneaked_in (what entered by Sneak Attack that turn), won, method |

Every view holds every kind: add `WHERE kind = 'selfplay'` (or another) for one kind's games - the ten queries below
were written for self-play. Rates over the combo tables and `events` use detailed games only: with `--sample` below 1, count the denominators
from `games WHERE detailed` (every query below does that implicitly by joining the detailed tables).

## The Doomsday series (for a publisher)

`gamedb.doomsday_series(run, since_iter=0, by_opponent=False, deck='doomsday_8027957', db=None)` (CLI: `python
tools/gamedb.py series RUN [--since N] [--by-opponent]`) - self-play's games only - returns plain dicts, one per iteration (and opponent deck with
`by_opponent`; `opp_deck` None otherwise), from one query of a few milliseconds:

| key | meaning |
|---|---|
| iter, opp_deck | the iteration; the opponent's deck (None: all) |
| games, wins, oracle_wins | the deck's seat-games (a mirror counts both seats), its wins, its wins by the Oracle (every game) |
| detailed_games | of them, the games replayed in full: the denominator of the pile counts (= games at `--sample 1`) |
| resolutions | the deck's resolved Doomsdays (as caster) in those games |
| own_pile, forced, in_hand, not_findable, missed | each resolution's Oracle class (`doomsday.oracle`): the network put the Oracle in its own pile / the wheel's forced find did / in hand / none findable / findable and not chosen |
| undrawn, undrawn_own_pile | the resolutions the wheel's draw passed over (all of them without the wheel) and of those the own piles: ENGINE.md 180's exit-gate share |
| own_pile_share, undrawn_own_pile_share | own_pile / resolutions, undrawn_own_pile / undrawn (None without a resolution) |

```python
import sys; sys.path.insert(0, 'tools')
import gamedb
rows = gamedb.doomsday_series('<runs>/legacy-meta3-r1', since_iter=120, by_opponent=True)   # or the run's name
```

## Bauble targeting (`tools/gamedb_bauble.py`, ENGINE.md 194)

The user's blunder rules (2026-11-17), `blunder_reason` in brackets:

* **Urza's Bauble** ("look at a card at random in target player's hand"): at itself is a blunder
  (`self_with_unknown_opponent_hand`) unless the opponent's hand is empty (`ok_opponent_hand_empty`) or the activator
  knows every card in it (`ok_opponent_hand_known`).
* **Mishra's Bauble** ("look at the top card of target player's library"): at itself is a blunder with no fetchland in
  hand or on its battlefield (`self_without_fetch`), unless the activator knows the opponent's top card
  (`ok_opponent_top_known`). At itself with a fetchland is not a blunder (`ok_self_with_fetch`: reported on its own,
  with `manipulation_in_hand`).
* At the opponent: `ok_target_opponent`. Excluded (`excluded`, not counted): the opponent was not a legal target, or
  (Mishra's) its library was empty.

The view **bauble_activations** (one row per activation): game_id, iter, kind (`selfplay`; the game's kind where
`games` has one), checkpoint (the activator's: its seat's checkpoint where `games` names it, else `iter_NNNN` of
`net_hi`), deck, opp_deck, pairing (`deck v opp_deck`), every `bauble` column, won (the activator won the game).

`gamedb.bauble_blunder_series(run, since_iter=0, by_kind=False, kind=None, db=None)` (CLI: `python tools/gamedb.py
bauble RUN [--since N] [--by-kind] [--kind selfplay]`) returns one dict per iteration and Bauble (`urza`, `mishra`,
`all`; and kind with `by_kind`): `games`, `activations`, `excluded`, `counted` (the shares' denominator), `self_targets`,
`blunders`, `self_with_fetch`, `self_with_fetch_and_manipulation`, `self_with_manipulation`, and `self_share` /
`self_se`, `blunder_share` / `blunder_se`, `self_with_fetch_share` / `self_with_fetch_se` (standard errors clustered by
game; None without a counted activation, the SE also with a single game). The delverai publisher puts the self-play
rows in the progress record (`bauble_targeting`) of a run with a database; the site's run pages draw the "Bauble
targeting" card from it.

A database ingested before the table existed has no `bauble` rows for those iterations: `ingest --redo` replays them.

The blunders by reason, with the activator's library manipulation: `SELECT bauble, blunder_reason, count(*) AS n,
count(*) FILTER (WHERE len(manipulation_in_hand) > 0) AS with_manipulation FROM bauble_activations WHERE excluded IS
NULL GROUP BY ALL ORDER BY ALL`.

## Ten example queries

```sql
-- 1. How often does a Doomsday pile hold card X, by iteration window and opponent (here X = Lotus Petal)
SELECT opp_deck, CASE WHEN iter >= 120 THEN '120+' ELSE 'before 120' END AS window, count(*) AS piles,
       count(*) FILTER (WHERE list_contains(pile, 'Lotus Petal')) AS with_card,
       round(avg(CASE WHEN list_contains(pile, 'Lotus Petal') THEN 1.0 ELSE 0.0 END), 3) AS share
FROM doomsday_piles GROUP BY ALL ORDER BY ALL;

-- 2. The cards Doomsday piles are made of (piles that hold each card), since iteration 120
SELECT card, count(*) AS copies, count(DISTINCT (game_id, idx)) AS piles
FROM doomsday_pile_cards WHERE iter >= 120 GROUP BY card ORDER BY piles DESC LIMIT 15;

-- 3. The Oracle at each resolution and the caster's win rate after it, per 10 iterations
SELECT iter // 10 * 10 AS iters, oracle, count(*) AS n, round(avg(CASE WHEN won THEN 1.0 ELSE 0.0 END), 3) AS won
FROM doomsday_piles GROUP BY ALL ORDER BY ALL;

-- 4. Each cell of the metagame: score, on the play and on the draw
SELECT deck, opp_deck, count(*) AS games, round(avg(score), 3) AS score,
       round(avg(score) FILTER (WHERE on_play), 3) AS on_play, round(avg(score) FILTER (WHERE NOT on_play), 3) AS on_draw
FROM game_results WHERE iter >= 120 GROUP BY ALL ORDER BY ALL;

-- 5. How each deck wins
SELECT deck, method, count(*) AS wins FROM game_results WHERE outcome = 'win' GROUP BY ALL ORDER BY deck, wins DESC;

-- 6. What Show and Tell puts in for its caster and what the opponent answers with
SELECT caster_pick, opponent_pick, count(*) AS n, round(avg(CASE WHEN won THEN 1.0 ELSE 0.0 END), 3) AS caster_won
FROM show_and_tell GROUP BY ALL ORDER BY n DESC LIMIT 20;

-- 7. Mulligans: how often and what they cost, by deck and play / draw
SELECT deck, on_play, mulligans, count(*) AS n, round(avg(score), 3) AS score FROM mulligans GROUP BY ALL ORDER BY ALL;

-- 8. The most cast cards per deck (casts per detailed game that deck played)
SELECT c.deck, c.card, round(count(*) / any_value(n.g), 3) AS per_game
FROM casts c JOIN (SELECT deck, count(*) AS g FROM game_results JOIN games USING (game_id) WHERE detailed GROUP BY deck) n
  USING (deck)
GROUP BY ALL ORDER BY per_game DESC LIMIT 20;

-- 9. What counters the combo: tracked spells and their answers
SELECT card, outcome, countered_by, count(*) AS n FROM spells GROUP BY ALL ORDER BY n DESC;

-- 10. Games with a voluntary Oracle pile that still lost, with the moves after the resolution
SELECT d.game_id, d.turn, d.pile, d.method, e.seq, e.turn AS t, e.type, e.player, e.card
FROM doomsday_piles d JOIN events e USING (game_id)
WHERE d.oracle = 'voluntary' AND NOT d.won AND e.turn >= d.turn AND e.type IN ('cast', 'counter', 'trigger', 'choice')
ORDER BY d.game_id, e.seq LIMIT 50;
```

## Every kind: examples

```sql
-- 11. The cells games of generation N (here 128): the checkpoint's score per deck pair and seat, against each reference
SELECT g.p0_label = 'iter_0128' AS ckpt_in_seat0, r.deck, r.opp_deck, r.opp_label AS reference, count(*) AS games,
       round(avg(r.score), 3) AS ckpt_score
FROM games g JOIN game_results r USING (game_id)
WHERE g.kind = 'cells' AND g.iter = 128 AND r.player_label = 'iter_0128' GROUP BY ALL ORDER BY ALL;

-- 12. Anchor games by opponent: each evaluated checkpoint's score against each anchor, per evaluation
SELECT iter, player_label AS ckpt, opp_label AS anchor, count(*) AS games, round(avg(score), 3) AS score
FROM game_results WHERE kind = 'anchor' AND player_label LIKE 'iter_%' AND opp_label LIKE 'anchor:%'
GROUP BY ALL ORDER BY iter, anchor;

-- 13. Doomsday piles in evaluation vs self-play: how the Oracle stood at each resolution, per kind
SELECT kind, oracle, count(*) AS piles, round(avg(CASE WHEN won THEN 1.0 ELSE 0.0 END), 3) AS caster_won
FROM doomsday_piles WHERE iter >= 120 GROUP BY ALL ORDER BY kind, piles DESC;

-- 14. What each kind of game is: games, players, mean length (the database's contents)
SELECT kind, count(*) AS games, count(DISTINCT p0_label) AS players, round(avg(turns), 1) AS turns,
       min(iter) AS first, max(iter) AS last FROM games GROUP BY kind ORDER BY games DESC;

-- 15. The best-of-one against a fixed opponent, per generation and play / draw (bo1 records)
SELECT iter, opp_label, on_play, count(*) AS games, round(avg(score), 3) AS score
FROM game_results WHERE kind = 'bo1' AND player_label LIKE 'iter_%' GROUP BY ALL ORDER BY ALL;
```

More shapes: the Oracle's position in its piles (`SELECT pos, count(*) FROM doomsday_pile_cards WHERE card =
'Thassa''s Oracle' GROUP BY pos`), what gets countered (`SELECT card, count(*) FROM events WHERE type = 'counter' GROUP
BY 1`), the wheel's share of forced piles over iterations (`SELECT iter, avg(forced::INT) FROM doomsday_piles GROUP BY
iter`), self-play resignations per iteration (`SELECT iter, avg(resigned::INT) FROM games GROUP BY iter`), a game's
whole play-by-play (`SELECT seq, turn, step, player, type, card, from_zone, to_zone, detail FROM events WHERE game_id =
... ORDER BY seq`). Replaying a game in the engine: its record is in `<run>/records/iter_NNNN/` (self-play) or its unit's file
(`<run>/records/<kind>/<sub>/<file>.jsonl.gz`: `zero.gamerec.read_file`) until pruned (`zero.gamerec.read_iteration`,
`actions_of`: `Game('mtg', spec, 2, seed)` then `step` each action) - on a checkout of its engine (`games.engine`).

## Turning it all on (a run)

One setting: the run's `game_records` 1 (`--game-records 1` at the launch; zero-core docs/az.md 59). With it, on a
commit at or after ENGINE.md 190 (the run's pinned worktree and the Mac's eval checkout of it):
* self-play on every executor (as ENGINE.md 187), each file with its build line;
* the evaluation (`eval`, `elo`, `anchor`, `ref`): here, or on the Mac with `eval_remote` 'mac' (the mirror gets the run's
  config, records there, and the files come back with the result);
* the yardsticks' watcher (`tools/run_yardsticks.py watch`, the run's companion: it reads the run's config.json) -
  `cells`, `cells_gen`, `cells_base`, `bo1`, local or farm;
* the delverai publisher (it reads the run's config.json): its replays carry their build (kind `publisher`) and its
  combo pass's games are recorded (`combo`).
Then `python tools/gamedb.py ingest <run> --follow 120` picks up every kind (the units of another engine: above).
