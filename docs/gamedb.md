# The game database (`tools/gamedb.py`)

Every self-play game of a run, replayed through the engine's play-by-play, in Parquet tables that DuckDB queries in
well under a second. Design and measurements: `docs/ENGINE.md` 187. This page is the schema and the queries: enough to
write SQL against it cold.

## Pipeline

1. **Records** (zero-core's run setting `game_records` 1, zero-core `docs/az.md` 59): every self-play game an iteration
   accepts, from every executor (this machine's driver, the remote workers, the farm pool), is one JSON line in
   `<run>/records/iter_NNNN/<source>-<pid>.jsonl.gz`: its seed, every action, the spec (by key), the setup (the decks
   and the metagame's pairing), the outcome, the resignation, the networks. Outside the training window: never rolled
   off by `keep_data`. `DONE.json` closes an iteration.
2. **Ingest** (`python tools/gamedb.py ingest RUN`): each new complete iteration replayed game by game on the CPU at
   below-normal priority, written to `<db>/<table>/iter=NNNN/part.parquet`. Incremental and idempotent (`<db>/state.json`).
   Every game gets a `games` row (light replay); a deterministic share `--sample` (default 1.0: all) is replayed in
   full: `events` and the combo tables. The full replay reads each step with `tools/combo_stats.py`'s Reader, the code
   `combo_stats.py scan` runs, so the combo tables are its numbers (validated game by game: ENGINE.md 187.4).
3. **Query**: `python tools/gamedb.py query RUN "SQL" [--csv|--json]`, `python tools/gamedb.py shell RUN`, or from
   Python `gamedb.connect(db)` (a DuckDB connection with every table and view below).
4. **Retention**: `python tools/gamedb.py prune RUN --keep-iters 64 --old-sample 0.1 [--delete-raw]`.

The database of run R: `$MTGZ_GAMEDB/R`, else `<ZERO_BIG_DISK>/mtg-zero-gamedb/R`, else `~/.cache/mtg-zero-gamedb/R`
(`--db DIR` overrides). Needs `pip install duckdb pyarrow`.

Conventions: seats are 0 and 1 (`deck0` plays seat 0); `turn` is the game's turn number (both players' turns
counted, as the engine and the play-by-play count them); card names are the engine's (a split / double-faced card by
its full name, `Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar`); lists are DuckDB `VARCHAR[]` (1-based:
`pile[1]` is the top card). `iter` is the iteration whose quota accepted the game (its samples trained iteration
`iter`); `net_lo` / `net_hi` the checkpoints its search used (iter - 1, or older for a lingering / farm game).

## Tables

**games** - one row per game (every game).

| column | meaning |
|---|---|
| game_id | BIGINT, unique (a hash of iteration, source, game id, seed, actions) |
| iter, src, executor | the accepting iteration; the record's source ('local', 'w0', 'mac/w1', 'pool:<farm run>'); executor 'local' / 'remote' / 'farm' |
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
| lines_lost | play-by-play lines lost to the 40-line tail (detailed games; 0 normally) |

**events** - one row per play-by-play line and per choice of a detailed game.

| column | meaning |
|---|---|
| game_id, iter, seq | the game; the event's order in it |
| action | the index of the action that produced it (-1: before the first action, the opening hands) |
| turn, step | the turn and the step name (`Main1`, `Upkeep`, `DeclareAttackers`, `Setup`, ...) |
| player | the seat (for a zone move: the seat whose destination zone holds the card, when exactly one does) |
| type | `cast`, `land` (a land played), `activate`, `mana` (a mana ability), `move` (a zone change), `find` (a search's pick), `trigger` (a trigger put on the stack), `counter` (a spell countered), `attack`, `look`, `choose`, `turn` (a turn begins), `forced_find` (the wheel's draw line), `empty_draw`, `choice` (a decision with two or more options other than priority and mana payment: mulligans, targets, modes, piles, Brainstorm's put-backs, ...), `other` |
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

## Views

| view | one row per | columns |
|---|---|---|
| game_results | game x seat | game_id, iter, seat, deck, opp_deck, on_play, score, outcome (win / loss / draw), method, turns, resigned, executor, mulligans |
| mulligans | game x seat | game_id, iter, seat, deck, opp_deck, on_play, mulligans, score, outcome |
| doomsday_piles | resolved Doomsday | game_id, iter, idx, caster, caster_deck, opp_deck, turn, pile, pile_size, card1..card5 (top first), oracle (class), oracle_in_pile, oracle_in_hand, oracle_where, oracle_place (`in_pile` / `in_hand` / `not_findable` / `missed`), forced (the wheel chose the Oracle), ff_drawn, forced_find_p, life, hand, choices, won (the caster won the game), win_method (its method when it won), method, turns |
| doomsday_pile_cards | card of a pile | game_id, iter, idx, caster, caster_deck, opp_deck, oracle, forced, won, win_method, pos (1 = top), card |
| casts | cast | game_id, iter, seq, turn, step, player, card, cast_from, deck, opp_deck, won |
| show_and_tell | resolved Show and Tell | game_id, iter, idx, caster, caster_deck, opp_deck, turn, put_caster, put_opponent, caster_pick / opponent_pick (the first of each list), won, method |
| sneak_attack | Sneak Attack activation | game_id, iter, idx, player, deck, turn, sneaked_in (what entered by Sneak Attack that turn), won, method |

Rates over the combo tables and `events` use detailed games only: with `--sample` below 1, count the denominators
from `games WHERE detailed` (every query below does that implicitly by joining the detailed tables).

## The Doomsday series (for a publisher)

`gamedb.doomsday_series(run, since_iter=0, by_opponent=False, deck='doomsday_8027957', db=None)` (CLI: `python
tools/gamedb.py series RUN [--since N] [--by-opponent]`) returns plain dicts, one per iteration (and opponent deck with
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

More shapes: the Oracle's position in its piles (`SELECT pos, count(*) FROM doomsday_pile_cards WHERE card =
'Thassa''s Oracle' GROUP BY pos`), what gets countered (`SELECT card, count(*) FROM events WHERE type = 'counter' GROUP
BY 1`), the wheel's share of forced piles over iterations (`SELECT iter, avg(forced::INT) FROM doomsday_piles GROUP BY
iter`), self-play resignations per iteration (`SELECT iter, avg(resigned::INT) FROM games GROUP BY iter`), a game's
whole play-by-play (`SELECT seq, turn, step, player, type, card, from_zone, to_zone, detail FROM events WHERE game_id =
... ORDER BY seq`). Replaying a game in the engine: its record is in `<run>/records/iter_NNNN/` until pruned
(`zero.gamerec.read_iteration`, `actions_of`: `Game('mtg', spec, 2, seed)` then `step` each action).
