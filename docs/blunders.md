# Blunder measurement for the Delver mirror (`tools/delver_blunders.py`)

Added 2026-11-02 (branch `blunders`, from `view-known` 7a53953). Elo says whether a checkpoint got
stronger; this says which specific mistakes it makes, as rates with standard errors, so a training fork
can be watched for the mistake it was meant to fix (the first use: the float-mana fork, where Delver
training sees mana floating for the first time) and for ones it should not create. A separate page, not
an ENGINE.md section: the float-mana branch is writing the next section.

## 1. What it measures

Every rate is per opportunity; "/100 g" rows are counts per 100 games. The AI = the counted seats (both
with `--opp self`). A *mana land* is Volcanic Island, Island, Thundering Falls or Wasteland; a *fetch* is
one of the four fetchlands.

| block | metric | definition |
|---|---|---|
| Daze | into payable mana | of the AI's Dazes at an opponent's spell, the share where that spell's controller could pay {1} at the cast. Classes, first that applies: **floating mana** in its pool (the user's "dazing into floated mana"), an **untapped mana land**, only an **uncracked fetch** (crack in response, fetch an untapped land). |
| | then paid | of those, the share where the controller was asked at Daze's resolution and paid. |
| | Daze at its own spell | the AI Dazing its own spell (a Cori-Steel Cutter flurry trigger), apart from the above. |
| float | floats | a mana ability taken outside a payment: the float build's options (its ENGINE.md 86.4) "Tap <source> for mana" at priority and "Tap Island for mana, then return it to hand" in Daze's cost (a Volcanic Island then asks `ChooseMana` "Add {U}" / "Add {R}"); one mana each. Pool growth no such decision explains is counted apart (`pool growth no option explains`: a detector check, 0 in 9,000 float-engine games). |
| | spent / pointless | the floater's pool fell after the mana arrived, before the step ended / it did not (the mana emptied with the step). |
| | Wasteland at an untapped mana land | opportunities: an opponent's Wasteland targeting the AI's untapped mana land (today's engine: only where the AI had a decision while it was on the stack, see 3). **used** = the land tapped for mana before it died, any way (a float, or paying for a spell cast in response); **floated**, **floated and spent**; **no value taken, a castable instant could use it** = neither a spell paid with the land nor a spent float of it, while `Cast Brainstorm / Lightning Bolt / Unholy Heat / Prismari Charm` was a legal option at the AI's priority and the land makes one of its colours (the float-then-spend line, a cheap check). |
| | Daze returning an untapped Island | opportunities: the AI's Daze paid by returning an untapped Island. **floated** (the float rule's tap-before-return), **floated and spent**, **no float, an instant in hand could use it** (cost <= 1 + other untapped mana, a colour the Island makes). |
| | Daze returned untapped over a tapped Island | of the Dazes paid with a tapped and an untapped Island both there, the share returning the untapped one without a float (a mana lost on any engine); **... floated it, mana unspent**: returning it after floating its mana and not spending the mana (the float rule's version of the same loss). |
| burn | creature survived / could not kill | of Lightning Bolts and Unholy Heats resolving at an opponent's creature (countered or fizzled ones apart): it survived; the damage dealt was below its toughness minus damage already marked. |
| | into a damaged creature that survived | of burn into a creature already damaged, the share that survived (the Play-tab double Bolt into a 7/7 Murktide); also /100 g. |
| | killed nothing (survived the turn) | (suite 4; the user, 11-02) of burn at an opponent's creature, the share where it survived and nothing finished it before the turn ended - a follow-up burn, combat or anything else that killed it the same turn takes it out (the burn did its part); also /100 g. |
| land drop | tap-out, then the land played | of the non-cantrip, non-free spells cast in the caster's own main phase (empty stack) with its land drop unused and an untapped-entering land in hand, the share that left it no untapped mana land or fetch and was followed by that land played the same turn (the 11-01 scan's definition); also /100 g, into a live Daze, Dazed. |
| Tamiyo | own cantrip flipped her, she could attack | (suite 3; the user's Play-tab report of 11-02) the AI's own Brainstorm, Ponder, Prismari Charm or Clue - the stack's top object, the AI's, where her third-draw trigger went on the stack - flipped its Tamiyo, Inquisitive Student on its own turn while she could attack: not summoning sick, untapped or attacked this turn, no untapped opposing flier or reach creature with power 3+ (attacking would risk her). /100 g. |
| | before her attack (the Clue lost) | of those, the share whose trigger came before the declare-attackers step (upkeep, draw, main 1, beginning of combat): her attack's investigate never happened. Attacking first and casting the cantrip after (or in response to her attack trigger) flips her the same and keeps the Clue. **after combat without an attack**: main 2 or later, she never attacked. |
| | clean board: before her attack | the same share where the AI had no Dragon's Rage Channeler, Monk or Cori-Steel Cutter (a cantrip before combat has no surveil, prowess or flurry upside there: the Clue is the whole difference); also /100 g. |
| | (into the legend rule) | (suite 4) a flip while the AI already controls a Tamiyo, Seasoned Scholar is out of the before-her-attack rows. Not reported as a blunder (the user, 11-02: the card now against flipping her later is a judgment call); the scan still counts it (`tam_legend`, of `tam_flip_any`): 3.5-5.5 a 100 games. |
| habits | | `tools/delver_fingerprints.py`'s metrics (ENGINE.md 73.5): fetch cracked at once, Brainstorm with a shuffle, Murktide delve best, FoW pitched a FoW, tap-out into a live Daze (+ then Dazed), T1-2 Volcanic for blue only, kept a one-land seven, starting player won. |

The float block is a no-op on an engine without floating: today's engine never leaves mana in a pool
outside a payment (0 pool states in every game measured), so its float rows are 0 by construction and
the table says so. Its opportunity rows (Wasteland, Daze bounces) and their "used" / "missed spend"
counts are live on both engines. On the float build (section 3a) every float came from a Wasteland or a
Daze and no pool growth went unexplained.

## 2. How

* **Games**: each checkpoint plays itself with its run's evaluation seat on both sides (zero.h2h's
  `run_rule`: the 'cq' rule min-visits 4 at c-scale 0.3, root-view leaves, belief tilt 1 mix 0.25,
  chance_k 0), unpaired, driver seed 20261102, with the driver's move log; both seats on one evaluator
  (`mirror`: zero.h2h's match gives each side its own, so a mirror's leaves went to the GPU in two half
  batches; the 32-simulation baselines were played that way, the same games' statistics). The same seed
  gives every checkpoint the same game seeds - the same opening deals; the games diverge from there.
  Paired games would be wasted in a mirror (the pair shares the search streams too: the same game twice).
* **Move rule**: `--select visits` (the most visited option, the 'cq' keys dropped as zero.h2h's
  `--select visits`) or `--select cq --c-scale S`; the label gets `_visits` / `_cqS`. At 1024 simulations
  the run's 'cq' rule (its scale 0.3 was tuned at 32) scales Q by ~(50 + max N) x 0.3, so a small Q edge
  can beat a strong prior (the d7 session's Unholy Heat at a 0/3 Tamiyo): the 1024 baselines are given
  under both rules.
* **Why self-play, not a fixed anchor**: every decision is the checkpoint's (twice the events per game);
  the opportunities (the opponent's floats, Wastelands, spells into Daze) come from the checkpoint's own
  play, which is what its training sees and what the float fork changes; and it is the population the
  existing fingerprints were measured on (ENGINE.md 73.5, 80.10's control). `--opp ANCHOR.pt` is there
  for a fixed opponent (only the checkpoint's seat counted).
* **Budget**: `--sims` (default the run's eval_sims, 32). Every decision gets it, mana payments too - as
  zero.h2h's seats and the Delver Lab Play tab's AI (which plays humans at 1024). A metric that falls
  from 32 to 1024 is a mistake search fixes; one that does not is the network's.
* **Rebuild**: zero-core's Driver derives a game's engine seed from the driver seed and the gid
  (`game_seed`: splitmix64 of the seed and the game's stream, the first xoshiro256++ draw); the scan
  replays each move log in the engine with the spectator view (`"log": true`) at every engine step -
  forced steps included, so frames and states are 1:1 - and stops loudly when a log does not replay
  (another engine build, or a wrong seed).
* **Standard errors**: per-game cluster standard errors of a ratio, sqrt(G / (G - 1) sum_g (x_g - r
  n_g)^2) / sum n: games are independent, events within a game are not. A difference column uses the se of
  two independent sets.
* **Replays**: `replays RUN` counts the same metrics over a run's recorded zero-replay/1 evaluation games
  (the publisher's: 64 simulations) without a rebuild: coarser (a state per frame, forced picks have
  none), but free.
* `tools/delver_fingerprints.py` now pairs each frame with `states[frame['s']]` (it zipped them: in ~5%
  of recorded games frames and states are not 1:1, hub todo of 11-01); rebuilt games were 1:1 anyway.

## 3. Baselines on the current engine (digest 132437374243f7b2)

r3-2b-beliefsearch generations 424 (best anchored Elo, 1677) and 550 (the line's last), each against
itself, seed 20261102: 2,000 games at the evaluation budget (32 simulations, the run's 'cq' rule), 400
at 1024 (the Play tab's budget; the first 400 seeds) under the run's rule and under 'visits'. Rates in %
+- 1 se (per-game clusters); /100 g rows per 100 games. Full tables (every metric, n, differences):
`E:/mtg-zero-scratch/blunders/report_cq.txt`, `base/report_s32.txt`, the scans under
`E:/mtg-zero-scratch/blunders/base*/`.

```
metric                                                 424 s32           550 s32      424 s1024 cq  424 s1024 visits      550 s1024 cq
games                                                     2000              2000               400               400               400
Daze: into payable mana (any)                       10.0 +-0.5         8.5 +-0.5        10.2 +-1.3         8.2 +-1.1        12.6 +-1.5
Daze: ... an untapped mana land                      8.6 +-0.5         6.8 +-0.5         9.2 +-1.2         6.3 +-1.0        11.4 +-1.4
Daze: payable: then paid                            93.3 +-1.3        90.4 +-1.8        95.6 +-2.4        98.2 +-1.8        88.6 +-3.6
Daze: Daze at its own spell (/100 g)                10.2 +-0.7        11.5 +-0.8        12.0 +-1.8        13.8 +-2.0        15.5 +-2.2
Wasteland: no value taken, castable instant         39.1 +-2.0        40.3 +-2.0        41.8 +-5.1        37.9 +-4.6        44.0 +-5.2
Daze returned untapped over a tapped Island          3.2 +-0.7         2.6 +-0.6         6.0 +-1.9         3.8 +-1.5         5.4 +-1.8
burn: creature survived                              7.8 +-0.4         8.0 +-0.4         6.4 +-0.8         6.9 +-0.8         7.5 +-0.8
burn: into a damaged creature that survived         12.0 +-3.1         7.4 +-2.4         4.2 +-4.1         5.6 +-5.1         9.1 +-4.9
burn: damaged survivor (/100 g)                      0.7 +-0.2         0.4 +-0.1         0.2 +-0.2         0.2 +-0.2         0.8 +-0.4
land drop: tap-out, then the land played            12.1 +-0.9        10.5 +-0.9        13.8 +-2.1        11.7 +-1.9        10.3 +-1.8
land drop: (/100 g)                                  8.6 +-0.7         7.2 +-0.6        10.0 +-1.7         8.8 +-1.5         7.5 +-1.4
habits: fetch cracked at once                       86.5 +-0.3        84.2 +-0.4        84.0 +-0.8        85.4 +-0.9        82.6 +-0.9
habits: Brainstorm with a shuffle                   19.4 +-0.6        23.0 +-0.6        20.1 +-1.3        21.0 +-1.3        20.1 +-1.3
habits: Murktide delve best                         83.6 +-0.8        86.9 +-0.7        78.0 +-1.9        84.1 +-1.8        84.4 +-1.6
habits: FoW pitched a FoW                           63.0 +-2.0        60.5 +-2.1        58.7 +-4.7        67.4 +-4.5        62.6 +-4.8
habits: tap-out into a live Daze                    54.8 +-0.7        52.3 +-0.7        56.3 +-1.5        55.5 +-1.6        51.0 +-1.6
habits: T1-2 Volcanic, blue only                    68.1 +-1.2        50.5 +-1.3        60.3 +-2.8        64.2 +-2.8        44.4 +-3.1
habits: kept a 1-land seven                         79.0 +-1.4        80.6 +-1.3        79.2 +-3.1        77.6 +-3.1        84.2 +-2.9
```

Floating mana: 0 pool states in all of these games (today's engine cannot float). Read:

* **550 against 424** (32 sims): Daze into an untapped land -1.9 +- 0.7, fetch cracked at once -2.4 +- 0.5,
  Brainstorm with a shuffle +3.6 +- 0.8, Murktide delve +3.3 +- 1.0, tap-out into Daze -2.5 +- 1.0, T1-2
  Volcanic -17.6 +- 1.8; the blunder rows (burn, land drop, Wasteland) flat.
* **1024 against 32** (424, the run's 'cq' rule): nearly nothing moves - the land-drop tap-out (12.1 ->
  13.8), Daze into payable mana, the Wasteland line and the habits are the network's; burn survivors fall a
  little (7.8 -> 6.4, -1.4 +- 0.8; the damaged-survivor rate 12 -> 4 on few events), and Murktide's delve gets
  worse (83.6 -> 78.0, -5.6 +- 2.0).
* **'visits' against 'cq' at 1024** (424): the delve recovers (84.1, +6.1 +- 2.6 over cq) and Daze into an
  untapped land falls (6.3 against 9.2, -2.9 +- 1.5): part of what cq does at 1024 is the rule's scale
  (tuned at 32), as the canaries' Heat at Tamiyo shows (section 6).
* The Wasteland rows count only Wastelands the targeted player could answer: today's engine passes priority
  by itself when pass is the only option, so a Wasteland nobody could answer never shows in a state (the
  float build offers every targeted untapped source a float: its count is higher, ~49 against ~33 /100 g).

**Cost a checkpoint** (desktop RTX 5090, CUDA graphs, below-normal priority): 32 simulations, 2,000 games
6.4-6.8 min on an idle GPU (0.19-0.20 s a game, h2h's two-evaluator path; the one-evaluator mirror is the
default now) + 10-20 s to rebuild and scan; 3,000 games 11-22 min with the GPU shared by two h2h matches
and another run. 1024 simulations, 400 games: 91 min alone on the GPU (13.6 s a game; the last ~10% of
the time a few long games run alone, latency-bound: 1,024 sequential network calls a decision), 4-4.5 h
when shared with two or three other 1024 runs. Canaries: 8 s a checkpoint and rule at 32 simulations,
~4.5 min at 1024 (CPU, 8 threads, 63 spots x 8 searches).

## 3a. The float-mana fork (runs/r3-2b-float550, engine float-mana 1d67542, digest f6543eb2b1b8752f)

Main@550 trained 48 generations on the float build; 3,000 games each, 32 simulations:

```
                                                     550             574             598     598 - 550
floats (/100 g)                                85.2 +-1.9      76.3 +-1.8      71.7 +-1.8   -13.5 +-2.6
  spent before the pool emptied                19.4 +-0.8      14.5 +-0.8      12.1 +-0.7    -7.3 +-1.1
  pointless (none spent)                       80.6 +-0.8      85.5 +-0.8      87.9 +-0.7    +7.3 +-1.1
  pool growth no option explains                 0               0               0
Wasteland at an untapped mana land (/100 g)    49.2 +-1.4      49.5 +-1.4      48.8 +-1.4
  floated                                      83.8 +-1.0      85.2 +-0.9      84.5 +-1.0    +0.7 +-1.4
  floated and spent                             7.2 +-0.7       6.5 +-0.7       5.0 +-0.6    -2.2 +-0.9
  no value taken, a castable instant could     26.3 +-1.2      27.3 +-1.3      25.8 +-1.2    -0.5 +-1.7
Daze returning an untapped Island (/100 g)     41.8 +-1.2      33.0 +-1.1      29.4 +-1.0   -12.4 +-1.6
  floated                                      98.6 +-0.3      98.2 +-0.4      97.6 +-0.5    -0.9 +-0.6
  floated and spent                            31.3 +-1.4      23.9 +-1.4      21.3 +-1.4   -10.0 +-2.0
  floated + wasted it over a tapped Island     56.9 +-1.5      37.3 +-1.4      31.3 +-1.4   -25.6 +-2.1
Daze into payable mana                         12.6 +-0.5      12.0 +-0.5      12.9 +-0.5    +0.2 +-0.7
  ... floating mana                             5.1 +-0.3       2.7 +-0.2       2.4 +-0.2    -2.7 +-0.4
  ... an untapped mana land                     5.8 +-0.3       7.7 +-0.4       8.5 +-0.4    +2.7 +-0.5
  floating: then paid                          88.5 +-1.9      93.4 +-2.1      97.6 +-1.4    +9.1 +-2.4
canary: Daze into floating mana (16 spots)       100%             51%             59%
```

Every other category moved less than 2 se from 550 to 598 (T1-2 Volcanic -6.2 +- 1.5 at 574, -1.9 +- 1.5 at
598). The network trained on the old rules floats nearly every chance it gets (harmless: the source is
dying) and spends a fifth of it; the fork learned to Daze into floated mana half as often and to stop
floating-and-wasting an untapped Island when a tapped one could go back, while its floats grew more
pointless. Files: `E:/mtg-zero-scratch/blunders/float550/` (report_float.txt, report_float_vs_baseline.txt,
spots_float550.json, canary_s32.json).

## 3b. Tamiyo flipped before her attack (suite 3, 11-02; the legend rule split out in suite 4)

The user's Play-tab report (game 20261102-181844-24ecfc #47, report 20261102-183938-f44695): the AI, its
untapped Tamiyo, Inquisitive Student its only creature, cast Brainstorm in main 1 as its third draw of the
turn; she flipped before combat and the Clue her attack makes was lost. At that root the policy gives
Brainstorm 0.9998 and Pass 1.7e-4, so a 1024-simulation search spends 1023 visits on Brainstorm (both
rules); after a forced pass it Brainstorms again at beginning of combat (0.994), though at declare attackers
it does attack with her (0.85). With 150 forced root visits a branch, Pass and Brainstorm score about
alike (+0.016 / +0.006 / -0.002): inside Pass the network again cantrips before attacking. Its value head
prices the Clue (the same position at the opponent's next upkeep, identical but for the Clue: 0.251
against 0.220). Probes: `E:/mtg-zero-scratch/tamiyo-bs/` (probe.py, line.py).

The same games re-scanned (no new play; the old metrics unchanged; suite 3 numbers - suite 4 takes the
legend-rule flips out, e.g. 424 s32 85.5 -> 85.9 before her attack, n 1055 -> 1011, and adds them apart: 3.5-5.5
a 100 games at every checkpoint and budget, ~5-8% of all her flips by the AI's own cantrips):

```
                                          424 s32     424 s1024 cq  424 s1024 visits     550 s32    550 s1024 cq   f550@550 s32  f550@598 s32
games                                        2000             400               400        2000             400           3000          3000
own cantrip flipped her, she could (/100g) 52.8 +-1.5   48.8 +-3.3        54.2 +-3.4   50.1 +-1.5      47.8 +-3.3     51.1 +-1.3    49.4 +-1.2
  before her attack (the Clue lost)        85.5 +-1.1   85.1 +-2.5        82.9 +-2.7   82.1 +-1.3      86.4 +-2.3     80.9 +-1.1    82.8 +-1.0
  after combat without an attack            1.2 +-0.3    0.5 +-0.5         0.0 +-0.0    1.1 +-0.3       0.5 +-0.5      1.1 +-0.3     0.3 +-0.2
  clean board: before her attack           87.8 +-1.2   87.3 +-2.9        86.6 +-2.7   82.8 +-1.6      89.0 +-2.6     81.6 +-1.3    83.4 +-1.2
  clean board: before (/100 g)             31.9 +-1.2   29.2 +-2.6        34.0 +-2.7   28.0 +-1.1      30.2 +-2.6     27.1 +-0.9    27.7 +-0.9
```

(f550 = runs/r3-2b-float550 on the float build.) The network's habit: ~85% of the flips it causes on its
own turn, ~30 a 100 games on a clean board, at every budget, under both rules, and flat from 424 to
550 and over the float fork's 48 generations. 96% of the clean pre-combat casts (424 s32: 616 of 639) went on an empty stack -
proactive, not answers (18 answered an opponent's object, mostly a Bauble); 228 of them were in the AI's
own draw step and 79 in its upkeep. Search cannot repair it (it needs two low-prior passes in a row); a
fix is a training matter (exploration of the pass at roots like this one).

## 4. The fixed baseline and the per-generation sidecar

**Baseline** (`data/delver_blunder_baseline.json`, versioned in the repo): generation 424 of
r3-2b-beliefsearch at the evaluation budget (32 simulations, the run's evaluation seat), the 2,000 games of
seed 20261102 on engine digest 132437374243f7b2 (build sha256 cb230cd28457fcbb = view-known 7a53953's);
every metric's rate, se and n with the suite version (`SUITE_VERSION`, bumped when a definition changes).
`python tools/delver_blunders.py baseline <424 scan> [--games-file F] [--digest D]` adds only the metrics
the file lacks: a new category joins by re-scanning the baseline's games file (recorded in the file: the
same games, no new play), the old entries keep their numbers (`--replace` to overwrite).
`report ... --baseline data/delver_blunder_baseline.json` puts the baseline first and marks every cell
more than 2 se from it (`*`: the se of the difference of two independent sets, the new se floored at the
baseline rate's binomial se for that n, no flag under 10 events). With ~40 metrics, ~2 flags per
measurement are expected by chance at 2 se: read a flag that persists over several generations.

**Sidecar** (`watch`): follows a run's evaluated checkpoints (models/iter_N.pt with N a multiple of its
eval_every, settled for 60 s) and measures each at the evaluation budget, 1,000 games by default (~3-4
min on an idle GPU; ~1.4x the baseline's se), against the baseline:

* `runs/<run>/blunders/iter_NNNN.json`: every metric's value / se / n, the baseline's value and se, the
  gap, z and flag, the engine, the games' settings, the seconds;
* `runs/<run>/blunders.jsonl` (next to evals.jsonl): one flat line a measurement, `{"iter", "time",
  "games", "sims", "suite", "engine", "flags": [...], "<metric>": rate, "<metric>_se": se, ...}` - plot
  `<metric>` against `iter` with the baseline as a line;
* `runs/<run>/blunders/work/`: the games and scans (re-scan them after a metric change).

Not in the training loop (a measurement costs more than a generation's ~183 s): a separate low-priority
process, newest checkpoint first - when behind it skips to the newest (`--backfill` measures skipped ones
when idle) -, resumable (a checkpoint with its iter_NNNN.json is done; a failure leaves
iter_NNNN.error.txt and is not retried until a restart), one watcher a run (blunders/watch.pid), stopped
by `runs/<run>/blunders/STOP`. It starts from the newest evaluated checkpoint at launch (`--min-iter N`
for history). It shares the GPU with the run's self-play (the cost lands on the run's generation time
while it plays; `--games 500` halves it). One command beside a run, from this checkout, with the run's
own engine checkout:

```
cd "C:/Users/Gregg Keithley/mtg-zero-wt/blunders"
python tools/launch_hidden.py "C:/Users/Gregg Keithley/mtg-zero/runs/<run>/blunders_console.txt" \
    python tools/delver_blunders.py watch "C:/Users/Gregg Keithley/mtg-zero/runs/<run>" \
    --engine "C:/Users/Gregg Keithley/mtg-zero-wt/<the run's pinned worktree>" < /dev/null > /dev/null
```

(stop: `touch runs/<run>/blunders/STOP`; the console file has a line a measurement with its flags.)

**Display (proposed, not built)**: where Elo is shown - the zerodash run page and delverai.greggg230.com's
generation charts - a "Blunders" panel on the same iteration axis: one small chart per section (Daze,
burn, land drop, habits, floating on a float engine), each metric a line with its +- 1 se band and the
baseline as a dashed line, flagged points marked; and a per-generation table under the Elo table (the
flagged metrics first, value vs baseline). Both read runs/<run>/blunders.jsonl.

## 5. Running it on a training fork

From this checkout (its tools), with the fork's engine checkout (its built `mtgz/_engine.pyd`; any
checkout works, the tool's own code is pure Python), below-normal priority, the GPU with CUDA graphs:

```
cd "C:/Users/Gregg Keithley/mtg-zero-wt/blunders"
F="C:/Users/Gregg Keithley/mtg-zero/runs/<fork>/models"
ENG="C:/Users/Gregg Keithley/mtg-zero-wt/<the fork's pinned run worktree>"
# the start checkpoint on the fork's engine (its own control), then the fork's checkpoints:
python tools/delver_blunders.py run "C:/Users/Gregg Keithley/mtg-zero/runs/r3-2b-beliefsearch/models/iter_0550.pt" \
    "$F/iter_0560.pt" "$F/iter_0570.pt" --engine "$ENG" --out E:/mtg-zero-scratch/blunders/<fork> --games 2000
# side by side with today's baselines (old engine), differences against the start checkpoint:
python tools/delver_blunders.py report E:/mtg-zero-scratch/blunders/base/r3-2b-beliefsearch_0550_s32.scan.json \
    E:/mtg-zero-scratch/blunders/<fork>/*_s32.scan.json --diff 3-2,4-2
# the 1024-simulation budget (the Play tab's), fewer games:
python tools/delver_blunders.py run "$F/iter_0570.pt" --engine "$ENG" --out E:/mtg-zero-scratch/blunders/<fork> \
    --games 400 --sims 1024 --conc 400
# canaries: freeze spots from the start checkpoint's games on the fork's engine, then search them
python tools/delver_blunders.py spots E:/mtg-zero-scratch/blunders/<fork>/r3-2b-beliefsearch_0550_s32.games.json.gz \
    --engine "$ENG" --out E:/mtg-zero-scratch/blunders/<fork>/spots.json
python tools/delver_blunders.py canary E:/mtg-zero-scratch/blunders/<fork>/spots.json "$F/iter_0560.pt" "$F/iter_0570.pt" \
    --engine "$ENG" --sims 32,1024 --device cuda
```

The run's `--seed` (20261102) and `--games` must stay the same across checkpoints for the fixed set.
`run` writes `<label>.games.json.gz` (the move logs, ~1 MB per 2,000 games) and `<label>.scan.json`
(per-game counts, examples, canary candidates) under `--out`; `scan` and `report` can be re-run on them
at any time (a changed metric needs no new games). The canary set built from a checkpoint's own games
favours that checkpoint's choices (it was picked where it made them): build it from the start
checkpoint and read the fork's columns against each other.

## 6. Canaries: fixed spots, searched again by each checkpoint

The game rates move with the games (a different checkpoint reaches different positions); a canary holds
the positions fixed. `spots` freezes a set: from a games file and its scan, the first `--per-kind`
candidates of each kind in game order, each stored as the game's engine seed + the actions up to the
decision + the watched option; `--play <game.json>:<action index>` adds a Delver Lab Play-tab spot.
`canary` replays every spot and runs each checkpoint's evaluation search there `--repeats` times (seeds
apart) at each budget (`--sims 32,1024`) and move rule (`--rules run,visits,cq:0.1`) (`search_many`: the lockstep search of ENGINE.md 80.10's re-searches - the run's
'cq' rule and transform, root-view leaves, the root belief from the network's own belief head); the
table is the share of searches choosing the watched option, mean over a kind's spots +- se over spots.

| kind | the spot | watched |
|---|---|---|
| daze_payable_land | the AI Dazed a spell whose controller had an untapped mana land | Cast Daze |
| daze_payable_pool | the AI Dazed a spell whose controller had mana floating (float engine only) | Cast Daze |
| waste_brainstorm | Wasteland at the AI's untapped land, Brainstorm castable from it, the AI let the land die | Cast Brainstorm (the good line) |
| burn_could_not_kill | burn cast at an undamaged creature it could not kill | the cast |
| burn_damaged_survivor | burn cast into an already damaged creature that survived | the cast |
| land_drop_tapout | the tap-out with the land drop unused, the land played after | the cast |
| tamiyo_flip_precombat | (suite 3) the AI's own cantrip or Clue, cast before combat on a clean board, that flipped its attack-ready Tamiyo | the cast |
| tamiyo_legend_flip | (suite 4) the AI's own cantrip or Clue that flipped a Tamiyo with a Seasoned Scholar already in play | the cast |
| burn_no_kill | (suite 4) burn at an opponent's creature that survived the turn (nothing finished it) | the cast |
| murktide_delve_short | a short Murktide delve by the user's rule (after all picks: fewer than 5 instants/sorceries exiled, and 2+ instants or 2+ sorceries left in the graveyard - one of each can stay for delirium) that lost a counter; the spot is the pick that cost it (`tools/delver_murktide_spots.py`: the engine searches every delve choice) | the pick |
| play | the user's game 20261101-215939-adeb97: #174 the first Lightning Bolt at the 7/7 Murktide, #178 the second into it (3 damage marked) | Cast Lightning Bolt |
| play | game 20261102-092826-5b7aab #51 (turn 3, declare blockers): a no-delirium Unholy Heat at a 0/3 Tamiyo (the d7 session's horizon-effect find) | Cast Unholy Heat |
| play | game 20261102-181844-24ecfc #47 (turn 4, main 1; report 20261102-183938-f44695): Brainstorm as the third draw, flipping an attack-ready Tamiyo before combat (section 3b) | Cast Brainstorm |

The first set (`E:/mtg-zero-scratch/blunders/base/spots_424.json`: 12 spots of each kind from generation
424's 32-simulation games + the three Play-tab spots), 8 searches a spot, CPU (8 threads): 8 s a
checkpoint and rule at 32 simulations, ~4.5 min at 1024. Share of searches choosing the watched option:

```
checkpoint, rule, budget        daze_pay (12)  waste_bs (12)  burn_cnk (12)  burn_dmg (12)  ld_tapout (12)  #174 Bolt  #178 Bolt  #51 Heat
424 cq0.3 (run's)   s32              100% +-0        12% +-9        86% +-9       82% +-10        100% +-0      100%       100%        0%
424 cq0.3 (run's)   s1024            82% +-10       21% +-11       77% +-11       53% +-14        100% +-0        0%         0%       75%
424 visits          s32               92% +-8         2% +-2       74% +-12       84% +-11        100% +-0      100%       100%        0%
424 visits          s1024            83% +-11        11% +-9       78% +-12       83% +-11        100% +-0       12%         0%        0%
424 cq0.1           s1024            73% +-13        12% +-9       78% +-12       74% +-12        100% +-0        0%         0%        0%
550 cq0.3 (run's)   s32               77% +-12       33% +-14       83% +-11       78% +-11        100% +-0        0%        88%        0%
550 cq0.3 (run's)   s1024             83% +-9       25% +-12       76% +-12       50% +-14         86% +-9        0%         0%        0%
550 visits          s1024            62% +-14       32% +-14       61% +-14       69% +-12        100% +-0        0%         0%        0%
550 cq0.1           s1024            78% +-12       31% +-13       64% +-13       60% +-13        100% +-0        0%         0%        0%
```

(424's 32-simulation row is high by construction: the spots are where it made those choices.) Read: the
user's double Bolt is search-fixable (1024 holds both Bolts under every rule); the Heat at Tamiyo is the
'cq' rule's at 1024 (6 of 8 searches, 0 under 'visits' or cq 0.1, 0 at 32) - the d7 finding reproduced;
the land-drop tap-out is the network's (100% at every budget and rule); burn into a damaged survivor
falls 82% -> 53% from 32 to 1024 under cq 0.3 but not under 'visits' (83%).

Spots replay only on an engine whose rules agree up to them: the float build offers new decisions (a
mana ability at priority when Wasteland targets a land), so its canary set is frozen from its own games
(`spots` on its games), and the floating-mana canary the user described (the opponent with {U} floating
casting Brainstorm into a held Daze) comes from its `daze into floating mana` examples; today's engine
cannot reach a pool at a decision.

**Tamiyo** (suite 3; `E:/mtg-zero-scratch/blunders/float550/spots_tamiyo.json`: 12 clean-board spots from
the float fork's 550 games + the Play spot #47, float engine; `canary_tamiyo.json`), share casting the
flipping cantrip:

```
checkpoint, rule, budget     tam_flip (12)  #47 24ecfc
424 cq0.3 (run's)  s32           100% +-0        100%
424 cq0.3 (run's)  s1024          85% +-9        100%
424 visits         s32           100% +-0        100%
424 visits         s1024          92% +-8        100%
550 cq0.3 (run's)  s32           100% +-0        100%
550 cq0.3 (run's)  s1024         74% +-12        100%
550 visits         s32            99% +-1        100%
550 visits         s1024          92% +-8        100%
f550@624 cq0.3     s32            91% +-6        100%
f550@624 cq0.3     s1024         83% +-10        100%
f550@624 visits    s32           83% +-11        100%
f550@624 visits    s1024         85% +-10        100%
```

The user's spot: Brainstorm in all 8 searches of every checkpoint, rule and budget - the network's.

## 6a. The canary library: known right answers, scored every generation

The canaries above are mined per measurement, from one checkpoint's own games, and only say which move it
made. The library (`data/delver_canary_library.json`, versioned in the repo; the user's ask of 11-02) is a
fixed, curated set of positions where the right answer is known and the network is known to go wrong,
each with that answer and the reason:

* **answer**: `bad` - the spot's watched option is the blunder, any other option is right (a Play-tab
  report: the AI's move there; a mined kind whose watched option is the mistake) - or `good` - the watched
  option is the right line (no kind uses it today). A spot's **score** is the share of
  its searches choosing a right answer; a kind's score the mean over its spots (the Play-tab reports
  together are the kind `play`); **all kinds** the mean over the kinds. Each spot's own score is kept too.
* **which kinds**: only those with a clear right answer (`KIND_ANSWER`): tamiyo_flip_precombat (attack
  first), murktide_delve_short (a delve short by the user's rule),
  burn_no_kill (the burn killed nothing), land_drop_tapout (play the land first), daze_payable_pool (the
  opponent has the mana floating). Daze into an untapped land and burn that could not kill stay out (a tax
  or a bait can be right); waste_brainstorm was taken out by the user (11-02: not a clear class of blunder;
  10 of its 12 spots marked drop), and so was tamiyo_legend_flip (11-02: the card now against flipping
  Tamiyo later is a judgment call). Every Play-tab report goes in with the user's reason (`--why`).
* **review**: the user audits the spots on a review page (each spot drawn as the Play tab's board, its
  answer, the context the board does not show; Keep / Drop / Unsure: https://claude.ai/artifact/1HPUzAhC5Q3rofWB9uS5ms,
  verdicts in its db collection `verdicts`, doc id = spot id). A drop leaves the library (`dropped`: id -> why);
  a keep is recorded on the spot (`review`); `unsure` (or `recheck`) **holds** a spot - kept, not scored
  (`HELD`), listed as `held` in the scores and the sidecar's record.
* **the fingerprint**: each spot stores `sig` (`spot_sig`: turn, step, decider, the options' words, the
  decider's hand, the life totals) on the engine it was added on. A later engine whose rules change the
  game before the spot replays it to another position: the spot is then **stale** - not scored, listed by
  id in the output and the sidecar's line (`canary_stale`), never silently skipped; if none replays, the
  scoring fails. The four Play spots replay to the same fingerprint on run-4589760, view-known and the
  float build.
* **the reference**: `library-score CKPT --reference` stores the first checkpoint's scores (the run's rule)
  in the file: generation 424, the Play tab's network.

```
cd "C:/Users/Gregg Keithley/mtg-zero-wt/blunders"
ENG="C:/Users/Gregg Keithley/mtg-zero-wt/run-1d67542"
# mined spots (`spots` output) + Play spots, each Play spot with its reason:
python tools/delver_blunders.py spots GAMES.games.json.gz --engine "$ENG" --out spots.json \
    --play "<delver-lab>/data/play/<game>/game.json:<k>"
python tools/delver_blunders.py library-add spots.json --engine "$ENG" --why "<game>:<k>=<why the AI's move was wrong>"
# Murktide delve spots (the engine searches every delve choice; not in the scan: it needs the engine):
python tools/delver_murktide_spots.py GAMES.games.json.gz murk.json 12 --engine="$ENG"
python tools/delver_blunders.py library-add murk.json --kinds murktide_delve_short --engine "$ENG"
# a checkpoint's scores (CPU; ~1 s at 32 simulations, ~5 min at 1024 for ~70 spots x 8 searches):
python tools/delver_blunders.py library-score CKPT [CKPT ...] --engine "$ENG" --sims 32,1024 --rules run,visits
```

**In the sidecar**: `watch` scores every measured checkpoint on the library (CPU, 8 searches a spot, at
the games' budget; `--library-sims 1024` adds the Play tab's budget, ~3 min of CPU a checkpoint; `--library
''` turns it off). The library is re-read at every measurement, so a spot added today counts from the next
generation on without a restart. iter_NNNN.json gets `canary` (per spot, per kind, all kinds, the stale
ids, the reference); blunders.jsonl gets `canary_s<sims>_all` and `canary_s<sims>_<kind>` beside the game
rates.

The first library (11-02, before the audit; float engine 1d67542): 12 spots each of the four kinds, mined from the float
fork's 550 games, and the four Play-tab reports (#174 and #178 Murktide Bolts, #51 Heat at Tamiyo, #47
Brainstorm before Tamiyo's attack). The mined spots favour 550's choices (they are where it made them):
read the generations against each other and against the reference, not as absolute skill.

**Audit** (11-02, every mined board read with both hands: `E:/mtg-zero-scratch/tamiyo-bs/audit.md`; the
file's `audit` key): tamiyo_flip and daze_float 12/12 hold; waste_bs 11/12 - one dropped (the dying land
was the only mana and the opponent held a free Daze: keeping Brainstorm is arguable); land_drop_tapout
12/12 kept, though in 8 the opponent's actual hand had no Daze - the AI cannot see that hand, and the land
first is never worse (the same mana at the end, and mana up while the spell is on the stack). Remove a
spot by deleting its entry (the reference's per-spot scores let the kind means be recomputed).

**The user's audit** (11-02, all 52 spots then on the page): play 4/4 keep; tamiyo_flip 9 keep, 1 unsure (game 3
turn 5: a Seasoned Scholar already in play - the legend rule, now its own kind), 2 drop - games 7 and 13,
Ponders the page had not shown to be the third draw (a Bauble's upkeep draw + the draw step: the replay
confirms both flip her), held as `recheck`; daze_float 11 keep, 1 drop; land_drop 8 keep, 4 unsure (held);
waste_bs 10 drop, 2 unsure (the kind removed). Then added for review (not yet audited by the user): 12
tamiyo_legend_flip (game 3 turn 5 + 11 mined; each replayed: the cast flips her with a Seasoned Scholar in
play), 12 murktide_delve_short, 12 burn_no_kill. Every Tamiyo spot was replayed: the cast makes the turn's
third draw and she flips as it resolves (`E:/mtg-zero-scratch/tamiyo-bs/verify_tamiyo.py`).

**The library after the audit** (62 spots, 56 scored, 6 held; the 12 tamiyo_legend_flip spots removed with
their kind; the Murktide spots re-mined by the user's rule - the first rule flagged any pick that lowered the
best count still reachable, which caught harmless picks such as a Wasteland first while every instant could
still go): reference generation 424, 8 searches a spot:

```
budget  burn_nokill  daze_float  ld_tapout  murk_delve  tam_flip  play  all kinds
s32         30%          9%         12%         56%         0%      25%     22%
s1024       45%         92%         36%         57%        19%      59%     52%
```

Murktide by the user's rule (the first 1,000 of float550@550's games): 61 of 1,290 casts short (4.7%), every
one avoidable; under the first rule 13%. The spot is the pick that cost the counter: the first pick that lowers
the most instants/sorceries the delve can still exile (the user, spot 55: the first cut froze game 99's last
pick, a Ponder that changed only what was left behind, not the size - the counter was lost two picks earlier to
Tamiyo). A short delve that still got every counter it could is not a spot (none in these 1,000 games). Reference
424 on these spots: 50% at 32, 49% at 1024; the library 21% at 32, 50% at 1024.

First scores (the first library, before the audit; CPU, 8 searches a spot; share choosing a right answer; `E:/mtg-zero-scratch/tamiyo-bs/lib_*.json`):

```
checkpoint, rule, budget   daze_float (12)  ld_tapout (12)  tam_flip (12)  waste_bs (11)  #47 Tamiyo  #174 Bolt  #178 Bolt  #51 Heat  all kinds
424 cq0.3 (ref)   s32                  8%             17%             0%             0%          0%        12%        12%      100%        11%
424 cq0.3 (ref)   s1024               89%             32%            15%             0%          0%       100%       100%       38%        39%
424 visits        s32                  0%             17%             0%             0%          0%         0%         0%      100%         8%
424 visits        s1024               58%             17%             8%             0%          0%        88%       100%      100%        31%
550 cq0.3         s32                  0%              4%             0%             0%          0%       100%        12%      100%        11%
550 cq0.3         s1024               91%              8%            25%             0%          0%       100%       100%      100%        40%
f550@624 cq0.3    s32                 50%              0%             9%             0%          0%       100%        12%      100%        22%
f550@624 cq0.3    s1024               93%             24%            21%             0%          0%       100%       100%       88%        42%
f550@632 cq0.3    s32                 50%              8%             6%             0%          0%       100%        38%      100%        25%
```

(The `all kinds` column counts the four Play spots as one kind, `play`.) Read: at the evaluation budget
the library is hard by construction (mined where 550 went wrong); the float fork's 32-simulation score
rose 11% -> 22-25%, all of it Daze into floating mana (0 -> 50%, what the fork trains on). At 1024 search
fixes the floating Daze (~90%) and both Murktide Bolts; it never fixes Brainstorm with a Wastelanded land
(0% everywhere) or the Tamiyo flip (#47 0% everywhere, the mined ones <= 25%) - those need training. The
Heat at Tamiyo is the 'cq' rule's at 1024 (38% right for 424, 100% under 'visits').

**Decided games out; the site at the Play tab's setup** (11-02, the user): a spot in a game the network already
counts as won or lost tests only habit (a deep dive found 11/12 land-drop, 5/5 Murktide and 5/11 burn spots
there). LIVE = (0.10, 0.90): the decider's raw value at the spot by the network that played it (gen 550 for
spots mined from float550@550's games, a Play game's own model for the reports); the library dropped the 26
decided spots (`dropped` names each value; Murktide's kind emptied, land drops kept 1) and `library-add` now
refuses decided spots (a spot's `value`, else `--live-ckpt CKPT`'s network). The site's blunder checks
(`tools/delverai_blunders.py`) search with the Play tab's setup: the run's evaluation seat at 1024 simulations
taking the most-visited move (delver-lab PLAY_SELECT 'visits'), 8 searches a spot, ~95 s of CPU a
generation for ~27 spots; scores cached per run and spot (a spot added is scored everywhere, a spot dropped
costs nothing); the main line's history every 16 generations (`--every 16`), the fork every evaluated one;
a lock file keeps a backfill and the sidecar's hook from scoring a run at once (the hook skips a round).
The sidecar's own library score (canary_s32_*) stays at the evaluation budget and rule.

## 6b. The Blunders tab: a generation playing each test case

The user (11-03): "pick a generation and see how it plays each test case. Starting from the canary case, record
until end of the turn and let me click through the steps with a game player UI". `tools/delverai_blunder_games.py
RUN [--iters N,M | --every K] [--upload]`: for a checkpoint and every user-kept spot, with the Play tab's seat
(delver-lab playcore.Net: the run's evaluation seat, 1024 simulations, the most-visited move, the full budget for
every decision - zero-core's Python driver has no sims_minor), zero.replay's RecordingDriver started at the spots
(`SpotDriver`: a slot's game replaced by the spot's, a game ended when its turn number changes):

1. 8 fresh searches at the spot (first decision only): the tally per option, the verdict (right in 5 of 8);
2. the move most of them chose (its search is the spot's frame), then the generation plays both seats to the end
   of that turn, every decision recorded (search, win chance, hand read).

One zero-replay/1 game a spot (~5 KB gzipped) with a `blunder` block, uploaded as a game of `blunders.<run>`
(listing record kind 'blunder'; the generation's earlier upload deleted first) - the site's #/blunders tab lists
a generation's cases by kind with each case's verdict over the recorded generations, and opens each in the game
viewer with a test-case panel. Files: runs/<run>/blunders/games/iter_NNNN/ (`<spot>.json.gz`, `tally.json`,
`uploaded.json`); a generation recorded for the current library (tally.json's library id) is not recorded again.
~10-15 min of CPU a generation for 53 spots (the turns' searches dominate). The tallies replace the scoring
cache's scores of those generations and spots (`delverai_blunders.merge_games`, whenever the cache is read under
its lock; `refresh` merges and uploads without scoring), so the Progress tab's counts and the tab's verdicts are
the same searches. The public reason drops the library's notes to the reviewer (`public_why`).

## 7. Not done

* Value readouts at burn resolutions (E:/mtg-zero-scratch/burn-scan): the suite counts outcomes, not the
  network's pending-damage misreads; burn_scan.py remains the tool for that.
* 550 under 'visits' at 1024 in games (stopped unfinished: the GPU was shared; the canaries have it, section 6).
* The 1024-simulation games' tail: the last few long games run alone (1,024 sequential network calls a
  decision); several checkpoints or rules in one driver (per-game seats) would overlap the tails.
* Value probes: none (the suite counts outcomes and search choices; a value head is relative to the
  encoded player: for `g.encode(p)` read `val[0][0]`, not `val[0][p]` - burn_scan.py is right).
* The display (section 4) is a proposal for the user's screenshot approval; it should show the canary
  library's scores (6a) beside the game rates.
* The canary library: the mined spots' right answers follow their kind's rule, checked by an audit of
  each board (11-02), not by the user; a review page (each spot's board, its answer, keep / drop) would
  let the user curate it. More kinds with clear answers as reports come in; the library's 1024 budget in
  the sidecar is opt-in (`--library-sims 1024`, ~3 min of CPU a checkpoint).
