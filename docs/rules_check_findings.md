# Rules-check findings

Engine rules bugs found by the rule-derived invariants (`engine/src/rules_check.rs`, `docs/ENGINE.md` 150)
and while writing them. None is fixed on branch `fuzz-invariants` (its behaviour digests equal main's): each
has a repro test in `rules_check::tests`, `#[ignore]`d, that asserts what the Comprehensive Rules require and
fails on the engine as it is:

    cargo test --release -p engine --lib rules_check::tests::f -- --include-ignored

Fixing one changes play (re-record the digests on purpose) and lets its test drop `#[ignore]`.

| # | CR | Found by | Repro | Expected | Actual |
|---|---|---|---|---|---|
| F1 | 704.5h | the fuzz (704.5h's check) | `FUZZ_FIRST=366 FUZZ_GAMES=1 FUZZ_LOG=1 cargo test --release -p engine --test fuzz fuzz_random_games` (default pool, step 741); test `f1_cr_704_5h_old_deathtouch_damage_does_not_destroy_after_indestructible_ends` | A creature is destroyed by 704.5h only for damage dealt by a deathtouch source "since the last time state-based actions were checked" | The record of deathtouch damage (`Obj::deathtouched`) survives a check that performs nothing |
| F2 | 704.5h, 702.90c | reading `sba.rs` for 704.5h's check | test `f2_cr_704_5h_deathtouch_damage_with_infect_destroys` | Damage from a source with infect and deathtouch puts -1/-1 counters (702.90c) and destroys the creature (704.5h) | Not destroyed: the engine reads deathtouch only with marked damage (`deathtouched && damage > 0`) |
| F3 | 702.19b, 702.2c | reading `combat.rs` for 510.1c's check | test `f3_cr_702_19b_deathtouch_trample_counts_marked_lethal_damage` | A deathtouch trampler need assign nothing to a blocker whose marked damage is already lethal | `lethal_needed` with deathtouch ignores marked damage: at least 1 |
| F4 | 508.8 | reading `flow.rs` for the turn checks | test `f4_cr_508_8_declared_attackers_keep_the_declare_blockers_step` | The declare blockers and combat damage steps are skipped only if no creature was declared as an attacker (or put onto the battlefield attacking) | Skipped when no creature is attacking as the declare attackers step ends |
| F5 | 510.4 | reading `combat.rs` for the damage checks | test `f5_cr_510_4_a_first_striker_never_deals_damage_in_the_second_step` | In the second combat damage step only creatures that had neither first nor double strike as the first step began, and those with double strike, deal damage | A first striker that assigned no damage in the first step (0 power) deals damage in the second |

## F1: a stale deathtouch record (CR 704.5h)

**Seen.** Default pool, seed 366, turn 44: Test Indestructible attacks and is blocked by Test Reacher and
Barrowgoyf (deathtouch); Barrowgoyf deals it 6 damage. It survives (indestructible). At the next priority the
rules check finds it still recorded as "dealt damage by a source with deathtouch since the last state-based
check", which has just happened (step 741; the determinized worlds of that state report it too).

**Cause.** `Game::sba_check` returns before `perform_sbas` when `sba_possible()` finds nothing to do, and only
`perform_sbas` clears `Obj::deathtouched`. An indestructible creature's deathtouch damage makes `sba_possible()`
false, so the record stays until a check performs something else, the creature changes zones, or the cleanup
step.

**Consequence.** If the creature stops being indestructible later in the turn (the Equipment granting it moves:
Kaldra Compleat; an effect ends; it loses its abilities), the next check destroys it for damage dealt before
earlier checks: the repro test does exactly this with Kaldra Compleat.

**Fix (not done here).** Clear the record on every check, including the fast path; 704.5h's condition is the
record alone (F2).

## F2: deathtouch with infect (CR 704.5h, 702.90c)

`Game::dying_creatures` and `sba_possible` destroy for deathtouch only `if o.deathtouched && o.damage > 0`.
Damage from a source with infect isn't marked (702.90c: -1/-1 counters instead), so a creature dealt damage by
a source with infect and deathtouch survives. 704.5h requires only that it "has been dealt damage by a source
with deathtouch". Reachable where a source has both: in Edge of Eternities, an animated Inkmoth Nexus (flying,
infect) given deathtouch by Blooming Stinger's or Syr Vondam, the Lucent's enter trigger.

## F3: deathtouch, trample and damage already marked (CR 702.19b, 702.2c)

`Game::lethal_needed` returns, for an attacker with deathtouch, 1 unless this attacker already assigned the
blocker damage, ignoring damage already marked. 702.19b counts damage already marked; 702.2c makes any nonzero
amount lethal, it doesn't make an already lethal amount insufficient. With an indestructible blocker that has
lethal damage marked (from a first-strike step, say) a deathtouch trampler must still assign it 1: the engine
offers 1..=n where the rules allow 0..=n. The rules check's damage-assignment options (510.1c, 702.19b) catch
this state when it is reached.

## F4: the declare blockers step after an attacker left combat (CR 508.8)

`Game::next_step` goes from the declare attackers step to the end of combat step when no creature on the
battlefield is attacking. 508.8 skips the declare blockers and combat damage steps only "if no creatures are
declared as attackers or put onto the battlefield attacking": an attacker declared and then killed or bounced
in the declare attackers step leaves both steps in the turn (both players get priority in each). Common in real
games (an instant on an attacker); the steps' priority windows are lost.

## F5: a first striker in the second combat damage step (CR 510.4)

`Game::deals_damage_now` lets a creature without double strike deal damage in the second step unless
`struck_first` (set as it deals first-strike damage). A creature with first strike as the first step began
that assigned no damage there (power 0) is not barred: pumped before the second step, it deals damage again.
510.4 bars every creature that had first strike or double strike as the first combat damage step began (but
those that have double strike).

## The 20k campaign's findings (2026-11-13)

The campaign (`docs/ENGINE.md` 150.7) found **no new engine rules bug** (no F6). Every rules-check violation it
reported is CR 704.5h's stale-record message (`Obj::deathtouched` still set at a priority right after a state-based
check), F1's signature: the check's other 704.5h branch ("dealt damage by a source with deathtouch and wasn't
destroyed") did not fire first, so the permanent was indestructible (or not a creature) at that check, and its record
survived a check that performed nothing (`sba_check`'s fast path). One was read in its game's log to make sure (names,
seed 17,951): the Goblin Shaman token gains indestructible from Cecil, Redeemed Paladin's attack trigger, Cecil, Dark
Knight (deathtouch) blocks it and deals it 2 damage, it survives, the record stays.

| Pool | Seed | Where | Permanent |
|---|---|---|---|
| default | 366 | the game (step 741) and a world for P0 | Test Indestructible (F1's repro) |
| default | 14,978 | a world for P0 | Test Indestructible |
| default | 17,506 | the game (step 284) | Test Indestructible |
| default matches | 1,786 | the game (step 1,620) and a world for P1 | Arwen, Mortal Queen (an indestructible counter) |
| default matches | 1,798 | the game (step 743) | Arwen, Mortal Queen |
| sealed | 2,634 | a world for P1 | Gene Pollinator |
| eoe | 4,210 | a world for P1 | Syr Vondam, the Lucent |
| names | 17,951 | the game (step 295) and a world for P1 | Goblin Shaman token (indestructible from Cecil, Redeemed Paladin) |
| `FUZZ_HIDDEN_DECKS=1` | 5,281 | a world for P1 | Construct token |
| `FUZZ_HIDDEN_DECKS=1` | 16,206 | a world for P1 | Golem token |
| `FUZZ_HIDDEN_DECKS=1` | 19,381 | the game (step 428) and a world for P0 | Test Indestructible |
| draft | events 10,757 and 13,157 | heuristic-played matches | Arwen, Mortal Queen |

Rerun one: `FUZZ_RULES_ALLOW=* FUZZ_POOL=names FUZZ_FIRST=17951 FUZZ_GAMES=1 FUZZ_LOG=1 cargo test --release -p engine
--test fuzz -- --exact fuzz_random_games` (0.5 s).

It also stopped at failures of the fuzzer's **other** checks (not rules checks), each reproduced on main 267ae603's own
fuzz binary (this branch changes no engine code), none fixed here. A failing seed ends its chunk; the campaign reran
the range before it (so its rules verdict prints) and after it, so every other seed was checked.

| # | Pool | Seeds | Check that fails | Repro (`cargo test --release -p engine --test fuzz -- --exact <test>`) |
|---|---|---|---|---|
| X1 | exponential | 2,095, 2,295, 2,570 | the encoding bound (ENGINE.md 46): feature 3248 = `bags[OPP_BATTLEFIELD]` at identity 816 is 385.25 (seed 2,295: 1,024) > `FEATURE_BOUND` 256. `encode::bag_add` adds 0.25 per permanent, unsquashed; `encode.rs`'s header still bounds a bag "by the 96-permanent cap", which ENGINE.md 26.11.6 ("any number of permanents a player") removed: seed 2,095 has 1,541 permanents of one token identity | `FUZZ_POOL=exponential FUZZ_FIRST=2095 FUZZ_GAMES=1`, `fuzz_random_games` (5 s) |
| X2 | `FUZZ_HIDDEN_DECKS=1` | 2,679, 3,191, 3,279, 5,095, 6,247, 7,943 | `check_hidden_world`, "the viewer's own cards": a hidden-decklist world dealt for viewer v changes one of v's own cards (seed 2,679: def 234 dealt as 259; seed 3,191: 280 as 259) | `FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST=2679 FUZZ_GAMES=1`, `fuzz_random_games` (2 s) |
| X3 | interactions | 5,682, 6,025, 7,790, 12,725 | a panic in `az/tail_encode.rs:676`, "picks in progress in two tasks of the agenda" (the second a `Run(Frame)` of def 138's `Trigger(0)` every time) | `FUZZ_POOL=interactions FUZZ_FIRST=5682 FUZZ_GAMES=1`, `fuzz_random_games` (<1 s) |
| X4 | `FUZZ_HIDDEN_DECKS=1` | 3,671, 9,671 | `check_hidden_world`, "the sampled deck lacks a shown <card>" (seed 3,671: Bejeweled Warg) | `FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST=3671 FUZZ_GAMES=1`, `fuzz_random_games` (<1 s) |
| X5 | `FUZZ_HIDDEN_DECKS=1` | 4,949, 17,779 | `check_hidden_world`, "the viewer's feature 56468 differs (1 / 0)" (also 56488, 56495): the viewer's encoding in the world is not the real game's | `FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST=4949 FUZZ_GAMES=1`, `fuzz_random_games` (<1 s) |
| X6 | `FUZZ_HIDDEN_DECKS=1` | 6,375, 6,419, 7,143, 12,247, 12,771, 13,227, 14,299, 15,287, 16,427, 17,215, 18,175 | `check_hidden_world`, "the match's deck of the opponent" is not the world's deck | `FUZZ_HIDDEN_DECKS=1 FUZZ_FIRST=6375 FUZZ_GAMES=1`, `fuzz_random_games` (<1 s) |
| X7 | interactions matches | 1,550 | `play_match`'s determinize check, "determinize changed the encoding of P0 (mover P0) at 56472: 1 -> 0" (next to X5's features: likely X5's cause) | `FUZZ_POOL=interactions FUZZ_FIRST=1550 FUZZ_MATCHES=1`, `fuzz_random_matches` (1.4 s) |

X2, X4, X5 and X6 are four checks of one family (hidden-decklist worlds, `MtgAz::determinize` with `TablePrior`,
ENGINE.md 102), none before seed 2,000; X5 and X7 name neighbouring features. X1 is the encoding's bound; X3 the v7
tail encoding (ENGINE.md 125).

**Exponential at 20k is out of reach on this desktop** (a decision for the maintainer: 150.7). Some seeds of
`FUZZ_POOL=exponential` (life 10^18, boards growing to the 60-turn cap) cost CPU-hours each, on main exactly as here:
seeds 0-99 ran 8,909 s of CPU (main) and 8,911 s (this branch) side by side without finishing; the chunk of seeds
0-1,999 ran 12,952 s in 8.6 hours and was stopped. The campaign ran exponential in 50-seed chunks for 20 minutes once
the other pools were done: 468 games (seeds 2,000-2,569 but the X1 seeds and 2,346-2,445, two chunks stopped after 20
minutes each), no rules check violated; at that rate the other ~19,500 seeds would take about 13 hours of four slots.

## STATUS (2026-11-13; branch `fuzz-invariants`, from main 267ae603; nothing pushed or merged)

(Integration, `integrate/fable-1112`, ENGINE.md 157: F1-F3 are fixed by `fix/rules-f1-f5` (ENGINE.md 151); their repro
tests here run, no longer `#[ignore]`d. F4 and F5 are fixed by `fix/rules-f4-f5` (ENGINE.md 151.4-151.5), brought onto
the integration as `fix/rules-f4-f5-int` (151.8): their repro tests run too; none of the five is ignored now.)

**Done** (`docs/ENGINE.md` 150 documents all of it).
* `engine/src/rules_check.rs` (`pub mod rules_check`, never called by the engine: zero cost in release):
  `RulesCheck` (per game: `new(&g)`, `step(&g)` after every apply, `forced_chance(&g)` before a transition that
  replays forced chance outcomes; stops after its first violation), `MatchCheck`, `check_state`, `DraftCheck`,
  `Stats`. The invariants and what is left out: ENGINE.md 150.2-150.3.
* 28 unit tests (crafted violations caught, legal look-alikes pass) + 5 repro tests (F1-F5; ignored on this branch,
  running since their fixes: F1-F3 ENGINE.md 151.1-151.3, F4-F5 151.4-151.5).
* Wiring: `engine/tests/fuzz.rs`, `engine/tests/draft.rs` (ENGINE.md 150.5).
* The chance-walk false positives (108.1 forced outcomes, 500.7 at a Time Vault choice) fixed in b57f5174
  (ENGINE.md 150.4); `FUZZ_CHANCE=20` clean over nine pools.
* The campaign (desktop only, single-threaded below-normal processes): every pool's 20,000 game seeds, 2,000 match
  seeds and 20,000 draft events but exponential (above), less the X seeds; no new rules bug; F1 hits; X1-X7 (main's
  too). ENGINE.md 150.7.
* Gate: the engine tier on 6bf8d661 (the branch's code; the last commit adds documentation only): `GATE PASSED engine
  6bf8d6612a7d in 4011s`. Digests equal main's (438164c0429d7fa3 / a0062bb5569e06d3).

**Left (the maintainer's).**
1. Merge into main (the F1-F3 fixes of `fix/rules-f1-f5` then let those repro tests drop `#[ignore]`).
2. The farm branch port: copy the fuzz.rs hunks into `engine/src/fuzz/games.rs` / `draft.rs` with
   `crate::rules_check` (one `RulesCheck::new` at game start and one `step` after each apply per loop; there a
   violation should fail the seed like a panic, the farm's model), and `play_match_heuristic_with` comes with this
   branch. Chance walks there need `forced_chance(&w)` before each forced transition.
3. X1-X7 to their owners (encoding, hidden decklists, v7 tail encoding); exponential at 20k (150.7).
